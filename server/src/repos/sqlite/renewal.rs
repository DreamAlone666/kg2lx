use async_trait::async_trait;
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, params};

use super::{Sqlite, decode, encode};
use crate::domain::renewal::{
    DailyProgress, RenewalSettings, RenewalStatus, SNAPSHOT_MAX_AGE, SvipEvidence, upstream_day,
};
use crate::error::AppError;
use crate::repos::renewal::RenewalRepo;

#[async_trait]
impl RenewalRepo for Sqlite {
    async fn settings(&self, account_id: &str) -> Result<RenewalSettings, AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| read_settings(conn, &account_id)).await
    }

    async fn status(&self, account_id: &str, now: i64) -> Result<RenewalStatus, AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| {
            let tx = conn.transaction()?;
            let day = upstream_day(now);
            let settings = read_settings(&tx, &account_id)?;
            let daily = read_daily(&tx, &account_id, day)?;
            let older_pending = tx
                .prepare_cached(
                    "SELECT EXISTS(SELECT 1 FROM entitlement_daily_runs
                 WHERE account_id = ?1 AND upstream_day < ?2 AND unsettled)",
                )?
                .query_row(params![account_id, day.to_string()], |row| row.get(0))?;
            let evidence = read_svip_evidence(&tx, &account_id, now)?;
            Ok(RenewalStatus::new(
                day,
                now,
                settings,
                &daily,
                older_pending,
                evidence,
            ))
        })
        .await
    }

    async fn set_enabled(&self, account_id: &str, enabled: bool) -> Result<(), AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| {
            conn.execute(
                "INSERT INTO entitlement_renewal(account_id, auto_claim_enabled)
                 VALUES (?1, ?2) ON CONFLICT(account_id) DO UPDATE SET
                 auto_claim_enabled = excluded.auto_claim_enabled",
                params![account_id, enabled],
            )?;
            Ok(())
        })
        .await
    }

    async fn save_rate_limit(
        &self,
        account_id: &str,
        until: Option<i64>,
        streak: u8,
    ) -> Result<(), AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| {
            conn.execute(
                "INSERT INTO entitlement_renewal(account_id, rate_limit_until, rate_limit_streak)
                 VALUES (?1, ?2, ?3) ON CONFLICT(account_id) DO UPDATE SET
                 rate_limit_until = excluded.rate_limit_until, rate_limit_streak = excluded.rate_limit_streak",
                params![account_id, until, streak],
            )?;
            Ok(())
        }).await
    }

    async fn daily(&self, account_id: &str, day: NaiveDate) -> Result<DailyProgress, AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| read_daily(conn, &account_id, day))
            .await
    }

    async fn save_daily(
        &self,
        account_id: &str,
        day: NaiveDate,
        progress: &DailyProgress,
        propagating_until: Option<i64>,
    ) -> Result<(), AppError> {
        let account_id = account_id.to_owned();
        let progress = progress.clone();
        self.run(move |conn| {
            let tx = conn.transaction()?;
            tx.prepare_cached(
                "INSERT INTO entitlement_daily_runs (account_id, upstream_day, progress_json)
                 VALUES (?1, ?2, ?3) ON CONFLICT (account_id, upstream_day) DO UPDATE
                 SET progress_json = excluded.progress_json",
            )?.execute(params![account_id, day.to_string(), encode(&progress)?])?;
            if let Some(until) = propagating_until {
                tx.execute(
                    "INSERT INTO entitlement_renewal(account_id, propagating_until)
                     VALUES (?1, ?2) ON CONFLICT(account_id) DO UPDATE SET
                     propagating_until = max(coalesce(propagating_until, 0), excluded.propagating_until)",
                    params![account_id, until],
                )?;
            }
            tx.commit()
        }).await
    }

    async fn pending_older_days(
        &self,
        account_id: &str,
        today: NaiveDate,
    ) -> Result<Vec<(NaiveDate, DailyProgress)>, AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| {
            conn.prepare_cached(
                "SELECT upstream_day, progress_json FROM entitlement_daily_runs
                 WHERE account_id = ?1 AND upstream_day < ?2 AND unsettled",
            )?
            .query_map(params![account_id, today.to_string()], |row| {
                let date = row.get_ref(0)?.as_str()?;
                let value = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok((value, decode(row.get_ref(1)?.as_str()?, 1)?))
            })?
            .collect()
        })
        .await
    }

    async fn svip_evidence(&self, account_id: &str, now: i64) -> Result<SvipEvidence, AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| read_svip_evidence(conn, &account_id, now))
            .await
    }
}

fn read_settings(conn: &Connection, account_id: &str) -> rusqlite::Result<RenewalSettings> {
    Ok(conn
        .prepare_cached(
            "SELECT auto_claim_enabled, rate_limit_until, rate_limit_streak, propagating_until
         FROM entitlement_renewal WHERE account_id = ?1",
        )?
        .query_row([account_id], |row| {
            Ok(RenewalSettings {
                enabled: row.get(0)?,
                rate_limit_until: row.get(1)?,
                rate_limit_streak: row.get(2)?,
                propagating_until: row.get(3)?,
            })
        })
        .optional()?
        .unwrap_or_default())
}

fn read_daily(
    conn: &Connection,
    account_id: &str,
    day: NaiveDate,
) -> rusqlite::Result<DailyProgress> {
    Ok(conn.prepare_cached(
        "SELECT progress_json FROM entitlement_daily_runs WHERE account_id = ?1 AND upstream_day = ?2",
    )?.query_row(params![account_id, day.to_string()], |row| {
        decode(row.get_ref(0)?.as_str()?, 0)
    }).optional()?.unwrap_or_default())
}

fn read_svip_evidence(
    conn: &Connection,
    account_id: &str,
    now: i64,
) -> rusqlite::Result<SvipEvidence> {
    Ok(conn
        .prepare_cached(
            "SELECT svip_evidence_json FROM account_entitlements
         LEFT JOIN entitlement_renewal USING (account_id) WHERE account_id = ?1
         AND synced_at BETWEEN max(?2, coalesce(propagating_until, 0)) AND ?3
         AND sync_failed = 0 AND svip_evidence_json IS NOT NULL",
        )?
        .query_row(
            params![account_id, now.saturating_sub(SNAPSHOT_MAX_AGE), now],
            |row| decode(row.get_ref(0)?.as_str()?, 0),
        )
        .optional()?
        .unwrap_or_default())
}
