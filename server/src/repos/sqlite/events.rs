use async_trait::async_trait;
use rusqlite::{Connection, ToSql, params};

use super::{Sqlite, decode, encode};
use crate::domain::event_log::{EventLog, EventType, ListEvents, LogView};
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
    let tx = conn.transaction()?;
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

fn list_sql(filtered: bool, include_renewal: bool) -> String {
    // The two branches are disjoint. A concrete type predicate lets each index seek to the
    // selected category and supply seq order directly to UNION ALL's merge.
    let type_filter = if filtered { "AND event_type = ?5" } else { "" };
    let renewal_filter = if include_renewal {
        ""
    } else {
        // The existing web client only labels snapshot syncs.
        "AND (event_type <> 'entitlement' OR json_extract(payload_json, '$.detail.action') = 'sync')"
    };
    format!(
        "SELECT payload_json FROM (
            SELECT seq, payload_json FROM event_logs WHERE source_id = ?1 AND (NOT ?3 OR NOT ok)
                {type_filter} {renewal_filter}
            UNION ALL
            SELECT seq, payload_json FROM event_logs WHERE source_id IS NULL AND account_id = ?2
                AND (NOT ?3 OR NOT ok) {type_filter} {renewal_filter}
            ORDER BY seq DESC LIMIT ?4
        )"
    )
}

#[async_trait]
impl EventLogRepo for Sqlite {
    async fn append(&self, log: &EventLog) -> Result<(), AppError> {
        let log = log.clone();
        let configured = self.event_log_limit;
        self.run(move |conn| {
            let tx = conn.transaction()?;
            let kind = log.event.event_type();
            tx.prepare_cached(
                "INSERT INTO event_logs (source_id, account_id, event_type, ok, payload_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                log.context.source_id,
                log.context.account_id,
                kind.as_str(),
                log.ok,
                encode(&log)?
            ])?;
            prune(&tx, kind, configured)?;
            tx.commit()
        })
        .await
    }

    async fn list(&self, query: &ListEvents) -> Result<Vec<EventLog>, AppError> {
        let source_id = query.source_id.clone();
        let account_id = query.account_id.clone();
        let event_type = query.event_type.map(EventType::as_str);
        let errors_only = query.view == LogView::Errors;
        let include_renewal = query.include_renewal;
        let limit = query.limit as i64;
        self.run(move |conn| {
            let mut stmt = conn.prepare_cached(&list_sql(event_type.is_some(), include_renewal))?;
            let params: [&dyn ToSql; 5] =
                [&source_id, &account_id, &errors_only, &limit, &event_type];
            let count = if event_type.is_some() { 5 } else { 4 };
            stmt.query_map(&params[..count], |row| decode(row.get_ref(0)?.as_str()?, 0))?
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
    use rusqlite::StatementStatus;

    fn context(source: Option<&str>) -> EventContext {
        EventContext::new(
            EventTrigger::Admin,
            Some("test-account".into()),
            source.map(str::to_owned),
        )
    }

    fn query(event_type: EventType) -> ListEvents {
        ListEvents {
            source_id: "src-test".into(),
            account_id: "test-account".into(),
            limit: 20,
            view: LogView::All,
            event_type: Some(event_type),
            include_renewal: false,
        }
    }

    #[tokio::test]
    async fn busy_music_pool_preserves_each_low_frequency_type_and_uses_a_type_seek() {
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
        db.append(&EventLog::new(
            context(Some("src-other")),
            sync(),
            None,
            None,
        ))
        .await
        .unwrap();

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

        let rows = db.list(&query(EventType::Entitlement)).await.unwrap();
        assert_eq!(
            rows.iter().map(|row| &row.log_id).collect::<Vec<_>>(),
            [&low_frequency[3].log_id, &low_frequency[2].log_id]
        );
        for log in &low_frequency[..2] {
            let rows = db.list(&query(log.event.event_type())).await.unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].log_id, log.log_id);
        }

        db.run(|conn| {
            let log_total: i64 = conn.query_row(
                "SELECT row_count FROM event_log_counts WHERE event_type = 'music_url'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(log_total, 10_000);
            let sql = list_sql(true, false);
            let parameters = params!["src-test", "test-account", false, 20, "entitlement"];
            let mut plan = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let details = plan
                .query_map(parameters, |row| row.get::<_, String>(3))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .join("\n");
            assert!(
                details.contains("event_logs_by_source_type (source_id=? AND event_type=?)"),
                "{details}"
            );
            assert!(
                details.contains("event_logs_by_account_type (account_id=? AND event_type=?)"),
                "{details}"
            );

            let mut statement = conn.prepare(&sql)?;
            assert_eq!(statement.query_map(parameters, |_| Ok(()))?.count(), 2);
            assert!(statement.get_status(StatementStatus::VmStep) < 1_000);
            Ok(())
        })
        .await
        .unwrap();
    }
}
