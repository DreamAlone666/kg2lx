use async_trait::async_trait;
use rusqlite::{Connection, ToSql, TransactionBehavior, params};

use super::{Sqlite, decode, encode};
use crate::domain::event_log::{EventLog, EventType, ListGlobalEvents, LogView, StoredEventLog};
use crate::error::AppError;
use crate::repos::event_log::EventLogRepo;

const ACCOUNT_EVENT_LIMIT: i64 = 1_000;

fn retention_limit(kind: EventType, configured: i64) -> i64 {
    match kind {
        EventType::MusicUrl => configured,
        _ => configured.min(ACCOUNT_EVENT_LIMIT),
    }
}

pub(super) fn prune_all(conn: &mut Connection, configured: i64) -> rusqlite::Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for kind in [
        EventType::MusicUrl,
        EventType::TokenRefresh,
        EventType::AccountLogin,
        EventType::Entitlement,
    ] {
        prune(&tx, kind, configured)?;
    }
    tx.commit()
}

fn prune(conn: &Connection, kind: EventType, configured: i64) -> rusqlite::Result<()> {
    // Within one type, seq can have gaps. Select the excess oldest rows using the type index.
    conn.prepare_cached(
        "DELETE FROM event_logs WHERE seq IN (
            SELECT seq FROM event_logs WHERE event_type = ?1 ORDER BY seq
            LIMIT max(0, coalesce((SELECT row_count FROM event_log_counts WHERE event_type = ?1), 0) - ?2)
         )",
    )?
    .execute(params![kind.as_str(), retention_limit(kind, configured)])?;
    Ok(())
}

fn list_global_sql_and_params(query: &ListGlobalEvents) -> (String, Vec<Box<dyn ToSql>>) {
    let mut clauses: Vec<&'static str> = Vec::new();
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();

    if let Some(ref correlation_id) = query.correlation_id {
        clauses.push("correlation_id = ?");
        params.push(Box::new(correlation_id.clone()));
    }
    if let Some(ref source_id) = query.source_id {
        clauses.push("source_id = ?");
        params.push(Box::new(source_id.clone()));
    }
    if let Some(ref account_id) = query.account_id {
        clauses.push("account_id = ?");
        params.push(Box::new(account_id.clone()));
    }
    if let Some(event_type) = query.event_type {
        clauses.push("event_type = ?");
        params.push(Box::new(event_type.as_str().to_owned()));
    }
    if query.view == LogView::Errors {
        clauses.push("NOT ok");
    }
    if let Some(before_seq) = query.before_seq {
        clauses.push("seq < ?");
        params.push(Box::new(before_seq));
    }
    if let Some(after_seq) = query.after_seq {
        clauses.push("seq > ?");
        params.push(Box::new(after_seq));
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    let order_dir = if query.correlation_id.is_some() {
        "ASC"
    } else {
        "DESC"
    };
    params.push(Box::new(query.limit as i64));

    let sql = format!(
        "SELECT seq, payload_json FROM event_logs {where_clause} ORDER BY seq {order_dir} LIMIT ?"
    );
    (sql, params)
}

#[async_trait]
impl EventLogRepo for Sqlite {
    async fn append(&self, log: &EventLog) -> Result<(), AppError> {
        let log = log.clone();
        let configured = self.event_log_limit;
        self.run(move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let kind = log.event.event_type();
            tx.prepare_cached(
                "INSERT INTO event_logs (source_id, account_id, event_type, ok, correlation_id, payload_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                log.context.source_id,
                log.context.account_id,
                kind.as_str(),
                log.ok,
                log.context.correlation_id,
                encode(&log)?
            ])?;
            prune(&tx, kind, configured)?;
            tx.commit()
        })
        .await
    }

    async fn list_global(&self, query: &ListGlobalEvents) -> Result<Vec<StoredEventLog>, AppError> {
        let query = query.clone();
        self.run(move |conn| {
            let (sql, params) = list_global_sql_and_params(&query);
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map(
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                |row| {
                    let seq: i64 = row.get(0)?;
                    let log: EventLog = decode(row.get_ref(1)?.as_str()?, 1)?;
                    Ok(StoredEventLog { seq, log })
                },
            )?
            .collect()
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event_log::{
        EntitlementAction, Event, EventContext, EventTrigger, LoginOutcome, MusicUrlEvent,
        MusicUrlStage, RefreshReason,
    };

    fn context(source: Option<&str>) -> EventContext {
        EventContext::new(
            EventTrigger::Admin,
            Some("test-account".into()),
            source.map(str::to_owned),
        )
    }

    fn query(event_type: EventType) -> ListGlobalEvents {
        ListGlobalEvents {
            source_id: None,
            account_id: Some("test-account".into()),
            correlation_id: None,
            before_seq: None,
            after_seq: None,
            limit: 20,
            view: LogView::All,
            event_type: Some(event_type),
        }
    }

    #[tokio::test]
    async fn busy_music_pool_preserves_each_low_frequency_type() {
        let dir = tempfile::tempdir().unwrap();
        let db = Sqlite::open(dir.path(), 10_000).unwrap();
        let sync = || Event::Entitlement {
            action: EntitlementAction::Sync,
            observation: None,
        };
        let low_frequency = [
            EventLog::new(
                context(None),
                Event::TokenRefresh {
                    reason: RefreshReason::Manual,
                    upstream_endpoint: "login/token".into(),
                    human_verification: false,
                },
                None,
                None,
            ),
            EventLog::new(
                context(None),
                Event::AccountLogin {
                    outcome: LoginOutcome::Bound,
                },
                None,
                None,
            ),
            EventLog::new(context(Some("src-test")), sync(), None, None),
            EventLog::new(context(None), sync(), None, None),
        ];
        for event in &low_frequency {
            db.append(event).await.unwrap();
        }
        let music = EventLog::new(
            context(Some("src-test")),
            Event::MusicUrl(Box::new(MusicUrlEvent {
                stage: MusicUrlStage::FetchMusicUrl,
                request_hash: "hash-fixture".into(),
                requested_quality: "flac".into(),
                attempted_quality: Some("flac".into()),
                track_title: None,
                artist_name: None,
                album_name: None,
                album_audio_id: None,
                quality_fallback_index: 0,
                retry_count: 0,
                reported_types: None,
                human_verification: false,
                upstream_endpoint: None,
                status_code: Some(200),
                upstream_quality: None,
                upstream_status: None,
                upstream_priv_status: None,
                upstream_fail_process: None,
                upstream_ext_name: None,
                upstream_file_size: None,
                upstream_bit_rate: None,
                upstream_time_length: None,
            })),
            Some(1),
            None,
        );
        for _ in 0..10_020 {
            db.append(&music).await.unwrap();
        }

        let rows = db
            .list_global(&query(EventType::Entitlement))
            .await
            .unwrap();
        assert_eq!(
            rows.iter().map(|row| &row.log.log_id).collect::<Vec<_>>(),
            [&low_frequency[3].log_id, &low_frequency[2].log_id]
        );
        for log in &low_frequency[..2] {
            let rows = db
                .list_global(&query(log.event.event_type()))
                .await
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].log.log_id, log.log_id);
        }

        db.run(|conn| {
            let log_total: i64 = conn.query_row(
                "SELECT row_count FROM event_log_counts WHERE event_type = 'music_url'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(log_total, 10_000);
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn list_global_uses_indexes_and_filters_accurately() {
        let dir = tempfile::tempdir().unwrap();
        let db = Sqlite::open(dir.path(), 10_000).unwrap();

        let make_log = |account: Option<&str>,
                        source: Option<&str>,
                        corr: &str,
                        event: Event,
                        err: Option<crate::error::ErrorCode>| {
            let mut ctx = EventContext::new(
                EventTrigger::Admin,
                account.map(str::to_owned),
                source.map(str::to_owned),
            );
            ctx.correlation_id = corr.into();
            EventLog::new(ctx, event, None, err)
        };

        let claim_ev = || Event::Entitlement {
            action: EntitlementAction::Claim,
            observation: None,
        };
        let sync_ev = || Event::Entitlement {
            action: EntitlementAction::Sync,
            observation: None,
        };
        let login_err = Event::AccountLogin {
            outcome: LoginOutcome::Expired,
        };
        let refresh_ev = Event::TokenRefresh {
            reason: RefreshReason::Manual,
            upstream_endpoint: "login/token".into(),
            human_verification: false,
        };

        let fixtures = vec![
            make_log(
                None,
                None,
                "corr_unowned",
                login_err,
                Some(crate::error::ErrorCode::UpstreamLoginFailed),
            ),
            make_log(Some("acc_1"), None, "corr_chain", refresh_ev.clone(), None),
            make_log(Some("acc_1"), Some("src_1"), "corr_chain", sync_ev(), None),
            make_log(
                Some("acc_1"),
                Some("src_1"),
                "corr_chain",
                claim_ev(),
                Some(crate::error::ErrorCode::UpstreamRequestFailed),
            ),
            make_log(Some("acc_2"), Some("src_2"), "corr_other", refresh_ev, None),
        ];
        for item in &fixtures {
            db.append(item).await.unwrap();
        }

        // Verify EXPLAIN QUERY PLAN for every index path and ensure no temp B-tree sort.
        db.run(|conn| {
            let cases: Vec<(ListGlobalEvents, &str)> = vec![
                (
                    ListGlobalEvents {
                        account_id: Some("acc_1".into()),
                        source_id: None,
                        correlation_id: None,
                        event_type: None,
                        view: LogView::All,
                        before_seq: None,
                        after_seq: None,
                        limit: 10,
                    },
                    "event_logs_by_account_all (account_id=?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: Some("acc_1".into()),
                        source_id: None,
                        correlation_id: None,
                        event_type: None,
                        view: LogView::All,
                        before_seq: None,
                        after_seq: Some(1),
                        limit: 10,
                    },
                    "event_logs_by_account_all (account_id=? AND seq>?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: Some("acc_1".into()),
                        source_id: None,
                        correlation_id: None,
                        event_type: Some(EventType::Entitlement),
                        view: LogView::All,
                        before_seq: Some(100),
                        after_seq: None,
                        limit: 10,
                    },
                    "event_logs_by_account_all_type (account_id=? AND event_type=? AND seq<?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: Some("acc_1".into()),
                        source_id: None,
                        correlation_id: None,
                        event_type: Some(EventType::Entitlement),
                        view: LogView::All,
                        before_seq: None,
                        after_seq: Some(1),
                        limit: 10,
                    },
                    "event_logs_by_account_all_type (account_id=? AND event_type=? AND seq>?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: None,
                        source_id: None,
                        correlation_id: None,
                        event_type: Some(EventType::AccountLogin),
                        view: LogView::Errors,
                        before_seq: None,
                        after_seq: None,
                        limit: 10,
                    },
                    "event_logs_by_type (event_type=?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: None,
                        source_id: Some("src_1".into()),
                        correlation_id: None,
                        event_type: Some(EventType::Entitlement),
                        view: LogView::All,
                        before_seq: None,
                        after_seq: None,
                        limit: 10,
                    },
                    "event_logs_by_source_type (source_id=? AND event_type=?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: None,
                        source_id: Some("src_1".into()),
                        correlation_id: None,
                        event_type: None,
                        view: LogView::All,
                        before_seq: None,
                        after_seq: None,
                        limit: 10,
                    },
                    "event_logs_by_source (source_id=?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: None,
                        source_id: None,
                        correlation_id: Some("corr_chain".into()),
                        event_type: None,
                        view: LogView::All,
                        before_seq: None,
                        after_seq: None,
                        limit: 10,
                    },
                    "event_logs_by_correlation (correlation_id=?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: None,
                        source_id: None,
                        correlation_id: Some("corr_chain".into()),
                        event_type: Some(EventType::Entitlement),
                        view: LogView::Errors,
                        before_seq: None,
                        after_seq: None,
                        limit: 10,
                    },
                    "event_logs_by_correlation (correlation_id=?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: None,
                        source_id: None,
                        correlation_id: None,
                        event_type: None,
                        view: LogView::All,
                        before_seq: Some(10),
                        after_seq: None,
                        limit: 10,
                    },
                    "INTEGER PRIMARY KEY (rowid<?)",
                ),
                (
                    ListGlobalEvents {
                        account_id: None,
                        source_id: None,
                        correlation_id: None,
                        event_type: None,
                        view: LogView::All,
                        before_seq: None,
                        after_seq: Some(1),
                        limit: 10,
                    },
                    "INTEGER PRIMARY KEY (rowid>?)",
                ),
            ];

            for (q, expected_plan) in cases {
                let (sql, params) = list_global_sql_and_params(&q);
                let mut plan = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                let details = plan
                    .query_map(
                        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                        |row| row.get::<_, String>(3),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .join("\n");
                assert!(
                    details.contains(expected_plan),
                    "expected `{expected_plan}` in plan for {q:?}, got:
{details}"
                );
                assert!(
                    !details.contains("TEMP B-TREE"),
                    "unexpected TEMP B-TREE sort in plan for {q:?}:
{details}"
                );
            }
            Ok(())
        })
        .await
        .unwrap();

        // All events including account_id=NULL in seq DESC
        let all = db
            .list_global(&ListGlobalEvents {
                account_id: None,
                source_id: None,
                correlation_id: None,
                event_type: None,
                view: LogView::All,
                before_seq: None,
                after_seq: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(all.len(), 5);
        assert!(all.windows(2).all(|w| w[0].seq > w[1].seq));
        assert!(all.iter().any(|item| item.log.context.account_id.is_none()));

        // Correlation chain returned in seq ASC
        let chain = db
            .list_global(&ListGlobalEvents {
                account_id: None,
                source_id: None,
                correlation_id: Some("corr_chain".into()),
                event_type: None,
                view: LogView::All,
                before_seq: None,
                after_seq: None,
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(chain.len(), 3);
        assert!(chain.windows(2).all(|w| w[0].seq < w[1].seq));
        assert_eq!(chain[0].log.event.event_type(), EventType::TokenRefresh);
        assert_eq!(chain[2].log.event.event_type(), EventType::Entitlement);
    }
}
