use std::path::Path;

use rusqlite::Connection;

use super::sqlite_error;
use crate::error::AppError;

const VERSION: i64 = 1;

pub(super) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(include_str!("migrations/001_init.sql"))?;
    conn.pragma_update(None, "user_version", VERSION)
}

pub(super) fn validate(conn: &Connection, path: &Path) -> Result<(), AppError> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sqlite_error)?;
    // The first development schema also used version 1 but had no correlation_id column.
    let has_release_schema: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('event_logs') WHERE name = 'correlation_id')",
        [],
        |row| row.get(0),
    ).map_err(sqlite_error)?;
    if version != VERSION || !has_release_schema {
        return Err(AppError::internal(format!(
            "unsupported SQLite schema version {version} in {}. Back up the existing database and use a fresh DATA_DIR for v0.3.0",
            path.display(),
        )));
    }
    Ok(())
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
