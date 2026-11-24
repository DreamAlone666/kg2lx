use std::path::Path;

use rusqlite::Connection;

use super::sqlite_error;
use crate::error::AppError;

const VERSION: i64 = 1;
const ACCOUNT_CHILD_TABLES: [&str; 4] = [
    "account_entitlements",
    "sources",
    "entitlement_renewal",
    "entitlement_daily_runs",
];

pub(super) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(include_str!("migrations/001_init.sql"))?;
    conn.pragma_update(None, "user_version", VERSION)
}

pub(super) fn validate(conn: &Connection, path: &Path) -> Result<(), AppError> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sqlite_error)?;
    if version != VERSION || !has_release_schema(conn).map_err(sqlite_error)? {
        return Err(AppError::internal(format!(
            "unsupported SQLite schema version {version} in {}. Back up the existing database and use a fresh DATA_DIR for v0.3.0",
            path.display(),
        )));
    }
    Ok(())
}

fn has_release_schema(conn: &Connection) -> rusqlite::Result<bool> {
    // Unreleased version 1 databases may predate the correlation column or the cascades.
    let has_correlation: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('event_logs') WHERE name = 'correlation_id')",
        [],
        |row| row.get(0),
    )?;
    if !has_correlation {
        return Ok(false);
    }
    let mut cascade = conn.prepare(
        r#"SELECT EXISTS(
            SELECT 1 FROM pragma_foreign_key_list(?1)
            WHERE "table" = 'accounts' AND "from" = 'account_id' AND "to" = 'account_id'
                AND on_delete = 'CASCADE'
        )"#,
    )?;
    for table in ACCOUNT_CHILD_TABLES {
        if !cascade.query_row([table], |row| row.get::<_, bool>(0))? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repos::sqlite::{DATABASE_FILE, open_database};

    #[test]
    fn initializes_the_release_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_database(dir.path()).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, VERSION);
        let index_columns: String = conn.query_row(
            "SELECT group_concat(name, ',') FROM pragma_index_info('event_logs_by_correlation')",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(index_columns, "correlation_id,seq");
        let obsolete: u32 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('event_logs_by_account', 'event_logs_by_account_type')",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(obsolete, 0);
    }

    #[test]
    fn deleting_an_account_cascades_to_its_children_and_keeps_events() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_database(dir.path()).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (account_id, provider, upstream_userid, status, cookies_json,
                 created_at, updated_at) VALUES ('acc', 'kugou_lite', '42', 'active', '{}', 1, 1);
             INSERT INTO account_entitlements (account_id, sync_failed) VALUES ('acc', 0);
             INSERT INTO sources (source_id, account_id, provider, name, script_token,
                 runtime_token, created_at, updated_at, quality_fallback)
                 VALUES ('src', 'acc', 'kugou_lite', 'Fixture', 'st_test', 'rt_test', 1, 1, 0);
             INSERT INTO entitlement_renewal (account_id) VALUES ('acc');
             INSERT INTO entitlement_daily_runs (account_id, upstream_day, progress_json)
                 VALUES ('acc', '2035-02-01', '{}');
             INSERT INTO event_logs (source_id, account_id, event_type, ok, correlation_id, payload_json)
                 VALUES ('src', 'acc', 'music_url', 1, 'req', '{}');
             DELETE FROM accounts WHERE account_id = 'acc';",
        )
        .unwrap();
        for table in ACCOUNT_CHILD_TABLES {
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table}");
        }
        let event: (String, String) = conn
            .query_row("SELECT account_id, source_id FROM event_logs", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(event, ("acc".into(), "src".into()));
    }

    #[test]
    fn refuses_the_development_baseline_without_cascades() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        let old_schema = include_str!("migrations/001_init.sql").replace(" ON DELETE CASCADE", "");
        conn.execute_batch(&old_schema).unwrap();
        conn.pragma_update(None, "user_version", VERSION).unwrap();
        drop(conn);
        let error = open_database(dir.path()).unwrap_err();
        assert!(
            error
                .message
                .contains("unsupported SQLite schema version 1")
        );
    }

    #[test]
    fn refuses_a_development_database_without_rewriting_it() {
        for version in [1, 4, 6] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(DATABASE_FILE);
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE event_logs (payload_json TEXT); INSERT INTO event_logs VALUES ('fixture')").unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
            drop(conn);

            let error = open_database(dir.path()).unwrap_err();
            assert!(
                error.message.contains("unsupported SQLite schema"),
                "{error}"
            );
            let conn = Connection::open(&path).unwrap();
            let value: String = conn
                .query_row("SELECT payload_json FROM event_logs", [], |row| row.get(0))
                .unwrap();
            assert_eq!(value, "fixture");
            let kept_version: i64 = conn
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(kept_version, version);
        }
    }
}
