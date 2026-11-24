use std::collections::BTreeMap;

use rusqlite::{Row, params};
use serde::de::DeserializeOwned;

use super::{ACCOUNT_SELECT, SOURCE_SELECT, Sqlite, decode, read_account, read_source};
use crate::domain::account::ProviderAccount;
use crate::domain::entitlement::EntitlementSnapshot;
use crate::domain::renewal::{RenewalSettings, RenewalStatus, SNAPSHOT_MAX_AGE, upstream_day};
use crate::domain::source::Source;
use crate::error::AppError;

pub struct AccountRecord {
    pub account: ProviderAccount,
    pub entitlement: EntitlementSnapshot,
    pub renewal: RenewalStatus,
    pub sources: Vec<Source>,
}

fn optional_json<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<T>> {
    let value: Option<String> = row.get(index)?;
    value.map(|value| decode(&value, index)).transpose()
}

impl Sqlite {
    pub async fn account_records(
        &self,
        account_id: Option<&str>,
        now: i64,
    ) -> Result<Vec<AccountRecord>, AppError> {
        let account_id = account_id.map(str::to_owned);
        self.run(move |conn| {
            let tx = conn.transaction()?;
            let day = upstream_day(now);
            let sql = format!(
                "SELECT a.*, e.items_json, e.synced_at, coalesce(e.sync_failed, 0),
                        coalesce(r.auto_claim_enabled, 0), r.rate_limit_until,
                        coalesce(r.rate_limit_streak, 0), r.propagating_until, d.progress_json,
                        EXISTS(SELECT 1 FROM entitlement_daily_runs
                               WHERE account_id = a.account_id AND upstream_day < ?2 AND unsettled),
                        CASE WHEN e.sync_failed = 0
                                  AND e.synced_at BETWEEN max(?3, coalesce(r.propagating_until, 0)) AND ?4
                             THEN e.svip_evidence_json END
                 FROM ({ACCOUNT_SELECT}) AS a
                 LEFT JOIN account_entitlements AS e USING (account_id)
                 LEFT JOIN entitlement_renewal AS r USING (account_id)
                 LEFT JOIN entitlement_daily_runs AS d ON d.account_id = a.account_id AND d.upstream_day = ?2
                 WHERE (?1 IS NULL OR a.account_id = ?1)
                 ORDER BY a.created_at, a.account_id"
            );
            let mut records = tx.prepare_cached(&sql)?.query_map(
                params![account_id, day.to_string(), now.saturating_sub(SNAPSHOT_MAX_AGE), now],
                |row| {
                    let settings = RenewalSettings {
                        enabled: row.get(14)?,
                        rate_limit_until: row.get(15)?,
                        rate_limit_streak: row.get(16)?,
                        propagating_until: row.get(17)?,
                    };
                    Ok(AccountRecord {
                        account: read_account(row)?,
                        entitlement: EntitlementSnapshot {
                            items: optional_json(row, 11)?,
                            synced_at: row.get(12)?,
                            sync_failed: row.get(13)?,
                        },
                        renewal: RenewalStatus::new(
                            day, now, settings,
                            &optional_json(row, 18)?.unwrap_or_default(),
                            row.get(19)?,
                            optional_json(row, 20)?.unwrap_or_default(),
                        ),
                        sources: Vec::new(),
                    })
                },
            )?.collect::<rusqlite::Result<Vec<_>>>()?;

            let mut sources = BTreeMap::<String, Vec<Source>>::new();
            let mut stmt = tx.prepare_cached(&format!(
                "{SOURCE_SELECT} WHERE (?1 IS NULL OR account_id = ?1) ORDER BY created_at, source_id"
            ))?;
            for source in stmt.query_map([account_id], read_source)? {
                let source = source?;
                sources.entry(source.account_id.clone()).or_default().push(source);
            }
            for record in &mut records {
                record.sources = sources.remove(&record.account.account_id).unwrap_or_default();
            }
            Ok(records)
        }).await
    }
}
