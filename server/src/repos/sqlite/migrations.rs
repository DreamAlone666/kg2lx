use std::path::Path;

use rusqlite::{Connection, TransactionBehavior};

use super::sqlite_error;
use crate::error::AppError;

// Each published migration keeps its original position and SQL.
pub(super) const MIGRATIONS: &[&str] = &[
    include_str!("migrations/001_init.sql"),
    include_str!("migrations/002_event_pools.sql"),
    include_str!("migrations/003_entitlement_renewal.sql"),
    include_str!("migrations/004_event_log_queries.sql"),
];

pub(super) fn migrate(
    conn: &mut Connection,
    path: &Path,
    minimum: i64,
    migrations: &[&str],
) -> Result<(), AppError> {
    let latest = migrations.len() as i64;
    loop {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        // Recheck under the write lock so two startup processes cannot apply a step twice.
        let current: i64 = tx
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(sqlite_error)?;
        if !(minimum..=latest).contains(&current) {
            return Err(AppError::internal(format!(
                "unsupported SQLite schema version {current} in {} (supported {minimum}..={latest})",
                path.display()
            )));
        }
        if current == latest {
            return Ok(());
        }

        let next = current + 1;
        let result = (|| {
            tx.execute_batch(migrations[current as usize])?;
            tx.pragma_update(None, "user_version", next)?;
            tx.commit()
        })();
        result.map_err(|error| {
            AppError::internal(format!(
                "SQLite migration {current} -> {next} in {}: {error}",
                path.display()
            ))
        })?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repos::sqlite::{DATABASE_FILE, open_database};

    fn old_disk_database(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch(
            r#"INSERT INTO accounts (account_id, provider, upstream_userid, status, cookies_json, created_at, updated_at)
             VALUES ('test-account', 'kugou_lite', 'test-user', 'active', '{"items":{"token":"fixture"}}', 1, 1);
             INSERT INTO sources (source_id, account_id, provider, name, script_token, runtime_token,
                                  created_at, updated_at, quality_fallback)
             VALUES ('src-test', 'test-account', 'kugou_lite', 'fixture', 'script-fixture', 'runtime-fixture', 1, 1, 0);
             INSERT INTO account_entitlements (account_id, items_json, synced_at, sync_failed)
             VALUES ('test-account', '[{"product_type":"svip","active":true,"begin_at":100,"end_at":200}]', 100, 0);
             INSERT INTO event_logs (source_id, account_id, event_type, ok, payload_json)
             VALUES (NULL, 'test-account', 'entitlement', 1, '{"detail":{"action":"sync"}}');"#,
        )
        .unwrap();
        conn
    }

    fn version(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn upgrades_v1_to_v2_and_keeps_old_snapshots_separate_from_v3_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        let mut v1 = old_disk_database(&path);
        let legacy_dir = dir.path().join("accounts");
        std::fs::create_dir(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("broken.json"), "{").unwrap();

        migrate(&mut v1, &path, 1, &MIGRATIONS[..2]).unwrap();
        assert_eq!(version(&v1), 2);
        drop(v1);
        let migrated = open_database(dir.path()).unwrap();
        assert_eq!(version(&migrated), MIGRATIONS.len() as i64);
        let tokens: (String, String) = migrated
            .query_row(
                "SELECT script_token, runtime_token FROM sources",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(tokens, ("script-fixture".into(), "runtime-fixture".into()));
        let preserved: (String, i64) = migrated
            .query_row(
                "SELECT json_extract(payload_json, '$.detail.action'), row_count
             FROM event_logs JOIN event_log_counts USING (event_type)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(preserved, ("sync".into(), 1));
        let snapshot: (String, Option<String>, i64) = migrated
            .query_row(
                "SELECT json_extract(items_json, '$[0].product_type'), svip_evidence_json,
                    (SELECT count(*) FROM entitlement_renewal) FROM account_entitlements",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(snapshot, ("svip".into(), None, 0));
        drop(migrated);
        assert_eq!(
            version(&open_database(dir.path()).unwrap()),
            MIGRATIONS.len() as i64
        );
    }

    #[test]
    fn a_failing_step_rolls_back_ddl_rows_and_version_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        let mut conn = old_disk_database(&path);
        let steps = [
            MIGRATIONS[0],
            "CREATE TABLE partial (id INTEGER);
            DELETE FROM event_logs;
            PRAGMA user_version = 2;
            SELECT missing FROM absent;",
        ];
        let error = migrate(&mut conn, &path, 1, &steps).unwrap_err();
        assert!(error.message.contains("1 -> 2"));
        drop(conn);

        let examined = Connection::open(&path).unwrap();
        assert_eq!(version(&examined), 1);
        let (partial, events): (i64, i64) = examined
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'partial'),
                    (SELECT count(*) FROM event_logs)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((partial, events), (0, 1));
        drop(examined);
        assert_eq!(
            version(&open_database(dir.path()).unwrap()),
            MIGRATIONS.len() as i64
        );
    }

    #[test]
    fn rejects_a_future_disk_database_before_retention_can_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        let conn = old_disk_database(&path);
        conn.pragma_update(None, "user_version", 500).unwrap();
        drop(conn);
        let error = open_database(dir.path()).unwrap_err();
        assert!(
            error
                .message
                .contains("unsupported SQLite schema version 500")
        );
        let reopened = Connection::open(path).unwrap();
        assert_eq!(version(&reopened), 500);
        let rows: i64 = reopened
            .query_row("SELECT count(*) FROM event_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn upgrades_v3_to_v4_and_creates_event_log_query_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        let mut v1 = old_disk_database(&path);
        migrate(&mut v1, &path, 1, &MIGRATIONS[..3]).unwrap();
        assert_eq!(version(&v1), 3);
        drop(v1);

        let migrated = open_database(dir.path()).unwrap();
        assert_eq!(version(&migrated), 4);

        for index_name in [
            "event_logs_by_account_all",
            "event_logs_by_account_all_type",
            "event_logs_by_correlation",
        ] {
            let exists: i64 = migrated
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
                    [index_name],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "missing index {index_name}");
        }
    }
}
