use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use rusqlite::Transaction;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{encode, write_account, write_source};
use crate::domain::account::{AccountStatus, ProviderAccount};
use crate::domain::cookie_store::CookieStore;
use crate::domain::entitlement::VipEntitlement;
use crate::domain::provider::ProviderKind;
use crate::domain::source::Source;
use crate::error::AppError;

#[derive(Deserialize)]
struct LegacyAccount {
    account_id: String,
    provider: ProviderKind,
    upstream_userid: String,
    status: String,
    cookies: CookieStore,
    last_refresh_at: Option<i64>,
    last_success_at: Option<i64>,
    last_error: Option<String>,
    last_error_code: Option<String>,
    created_at: i64,
    updated_at: i64,
    #[serde(default, deserialize_with = "read_snapshot")]
    entitlements: Option<Vec<VipEntitlement>>,
}

fn read_snapshot<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<VipEntitlement>>, D::Error> {
    Vec::deserialize(deserializer).map(Some)
}

impl LegacyAccount {
    fn into_account(self, path: &Path) -> Result<ProviderAccount, AppError> {
        let status = match self.status.as_str() {
            "active" => AccountStatus::Active,
            "pending" | "disabled" | "expired" => {
                tracing::warn!(account_id = %self.account_id, previous_status = %self.status, "normalized legacy account status");
                AccountStatus::Active
            }
            "login_failed" => AccountStatus::LoginFailed,
            _ => return Err(import_error(path, "unknown account status")),
        };
        validate_filename(path, &self.account_id)?;
        if self.upstream_userid.is_empty() {
            return Err(import_error(path, "missing upstream user ID"));
        }
        Ok(ProviderAccount {
            account_id: self.account_id,
            provider: self.provider,
            upstream_userid: self.upstream_userid,
            status,
            cookies: self.cookies,
            credential_version: 0,
            last_refresh_at: self.last_refresh_at.or(self.last_success_at),
            last_error: self.last_error,
            last_error_code: self.last_error_code,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

#[derive(Deserialize)]
struct LegacySource {
    source_id: String,
    account_id: String,
    provider: ProviderKind,
    name: String,
    script_token: String,
    runtime_token: String,
    created_at: i64,
    updated_at: i64,
    #[serde(default)]
    quality_fallback: bool,
}

impl LegacySource {
    fn into_source(self, path: &Path) -> Result<Source, AppError> {
        validate_filename(path, &self.source_id)?;
        if self.script_token.is_empty() || self.runtime_token.is_empty() {
            return Err(import_error(path, "missing script or runtime token"));
        }
        Ok(Source {
            source_id: self.source_id,
            account_id: self.account_id,
            provider: self.provider,
            name: self.name,
            script_token: self.script_token,
            runtime_token: self.runtime_token,
            created_at: self.created_at,
            updated_at: self.updated_at,
            quality_fallback: self.quality_fallback,
        })
    }
}

pub(super) fn import(tx: &Transaction<'_>, data_dir: &Path) -> Result<(), AppError> {
    for path in json_files(&data_dir.join("accounts"))? {
        let mut legacy = read_file::<LegacyAccount>(&path)?;
        let items = legacy.entitlements.take();
        let account = legacy.into_account(&path)?;
        write_account(tx, &account).map_err(|error| import_error(&path, error))?;
        if let Some(items) = items {
            (|| {
                tx.execute(
                    "INSERT INTO account_entitlements (account_id, items_json, sync_failed) VALUES (?1, ?2, 0)",
                    rusqlite::params![account.account_id, encode(&items)?],
                )
            })().map_err(|error| import_error(&path, error))?;
        }
    }
    for path in json_files(&data_dir.join("sources"))? {
        let source = read_file::<LegacySource>(&path)?.into_source(&path)?;
        write_source(tx, &source).map_err(|error| import_error(&path, error))?;
    }
    Ok(())
}

fn json_files(dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound && !dir.is_symlink() => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(import_error(dir, error)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| import_error(dir, error))?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            paths.push(path);
        }
    }
    paths.sort_unstable();
    Ok(paths)
}

fn read_file<T: DeserializeOwned>(path: &Path) -> Result<T, AppError> {
    let bytes = fs::read(path).map_err(|error| import_error(path, error))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        // JSON errors can include the offending value. Report location without credentials.
        import_error(
            path,
            format!(
                "invalid account/source JSON at line {}, column {}",
                error.line(),
                error.column()
            ),
        )
    })
}

fn validate_filename(path: &Path, id: &str) -> Result<(), AppError> {
    if path.file_stem().and_then(|stem| stem.to_str()) == Some(id) && !id.is_empty() {
        Ok(())
    } else {
        Err(import_error(path, "record ID does not match filename"))
    }
}

fn import_error(path: &Path, error: impl std::fmt::Display) -> AppError {
    AppError::internal(format!("legacy import {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repos::account::AccountRepo;
    use crate::repos::entitlement::EntitlementRepo;
    use crate::repos::source::SourceRepo;
    use crate::repos::sqlite::{DATABASE_FILE, Sqlite, open_database};
    use serde_json::{Value, json};

    fn account(id: &str, userid: &str, status: &str) -> Value {
        json!({
            "account_id": id, "provider": "kugou_lite", "upstream_userid": userid,
            "status": status, "cookies": {"items": {"token": "test-cookie", "dfid": "test-device"}},
            "display_name": null, "last_error": null, "last_refresh_at": 100,
            "last_success_at": 100, "created_at": 99, "updated_at": 100
        })
    }

    fn source(id: &str, account_id: &str, script_token: &str, runtime_token: &str) -> Value {
        json!({
            "source_id": id, "account_id": account_id, "provider": "kugou_lite",
            "name": "Fixture source", "script_token": script_token, "runtime_token": runtime_token,
            "created_at": 100, "updated_at": 101
        })
    }

    fn write(dir: &Path, name: &str, value: Value) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }

    #[test]
    fn fresh_database_is_empty_and_never_imports_after_initialization() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_database(dir.path()).unwrap();
        drop(conn);
        write(
            dir.path(),
            "accounts/acc_late.json",
            account("acc_late", "1", "active"),
        );
        let conn = open_database(dir.path()).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM accounts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn imports_released_shapes_preserving_order_identity_and_confirmed_empty() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let mut old_account = account("acc_old", "1", "expired");
        old_account["vip_active"] = json!(false);
        old_account["vip_type"] = json!(0);
        let mut old_source = source("src_old", "acc_old", "st_old", "rt_old");
        old_source["enabled"] = json!(true);
        write(dir, "accounts/acc_old.json", old_account);
        write(dir, "sources/src_old.json", old_source);

        let mut known = account("acc_known", "2", "disabled");
        known["entitlements"] = json!([
            {"product_type": "tvip", "active": true, "begin_at": null, "end_at": 200},
            {"product_type": "future", "active": false, "begin_at": 100, "end_at": null}
        ]);
        let mut known_source = source("src_known", "acc_known", "st_known", "rt_known");
        known_source["quality_fallback"] = json!(true);
        write(dir, "accounts/acc_known.json", known);
        write(dir, "sources/src_known.json", known_source);

        let mut empty = account("acc_empty", "3", "pending");
        empty["entitlements"] = json!([]);
        write(dir, "accounts/acc_empty.json", empty);
        write(dir, "login_sessions/old.json", json!({"legacy": true}));
        write(dir, "logs/runtime-test.jsonl", json!({"legacy": true}));
        let before = fs::read(dir.join("accounts/acc_old.json")).unwrap();

        let db = Sqlite::open(dir, crate::config::DEFAULT_EVENT_LOG_LIMIT).unwrap();
        let old = AccountRepo::find_by_id(&db, "acc_old")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(old.status, AccountStatus::Active);
        assert_eq!(old.cookies.get("token"), Some("test-cookie"));
        assert_eq!(old.cookies.get("dfid"), Some("test-device"));
        assert_eq!(old.last_refresh_at, Some(100));
        assert!(
            db.entitlement_snapshot("acc_old")
                .await
                .unwrap()
                .items
                .is_none()
        );
        let old_source = db.find_by_runtime_token("rt_old").await.unwrap().unwrap();
        assert_eq!(old_source.source_id, "src_old");
        assert_eq!(old_source.account_id, "acc_old");
        assert_eq!(old_source.script_token, "st_old");
        assert!(!old_source.quality_fallback);
        let new_source = db.find_by_script_token("st_known").await.unwrap().unwrap();
        assert_eq!(new_source.name, "Fixture source");
        assert_eq!(new_source.runtime_token, "rt_known");
        assert!(new_source.quality_fallback);
        let known = AccountRepo::find_by_id(&db, "acc_known")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(known.status, AccountStatus::Active);
        let items = db
            .entitlement_snapshot("acc_known")
            .await
            .unwrap()
            .items
            .unwrap();
        assert_eq!(
            (
                items[0].product_type.as_str(),
                items[0].active,
                items[0].end_at
            ),
            ("tvip", true, Some(200))
        );
        assert_eq!(
            (
                items[1].product_type.as_str(),
                items[1].begin_at,
                items[1].end_at
            ),
            ("future", Some(100), None)
        );
        let empty = AccountRepo::find_by_id(&db, "acc_empty")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(empty.status, AccountStatus::Active);
        assert!(
            db.entitlement_snapshot("acc_empty")
                .await
                .unwrap()
                .items
                .unwrap()
                .is_empty()
        );
        assert_eq!(fs::read(dir.join("accounts/acc_old.json")).unwrap(), before);

        // A stale legacy directory remains available for rollback. SQLite is now authoritative.
        fs::write(dir.join("accounts/acc_known.json"), "broken after import").unwrap();
        let reopened = open_database(dir).unwrap();
        for (table, expected) in [
            ("accounts", 3),
            ("sources", 2),
            ("event_logs", 0),
            ("login_sessions", 0),
        ] {
            let count: i64 = reopened
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, expected);
        }
        assert!(dir.join("login_sessions/old.json").exists());
        assert!(dir.join("logs/runtime-test.jsonl").exists());
    }

    #[test]
    fn bad_json_or_conflicting_records_leave_no_database_and_can_be_retried() {
        for (file, bad) in [
            ("accounts/acc_bad.json", None),
            (
                "accounts/acc_bad.json",
                Some(account("acc_bad", "1", "pending")),
            ),
            (
                "sources/src_bad.json",
                Some(source("src_bad", "acc_missing", "st_bad", "rt_bad")),
            ),
            (
                "sources/src_bad.json",
                Some(source("src_bad", "acc_a", "st_a", "rt_bad")),
            ),
            (
                "sources/src_bad.json",
                Some(source("src_bad", "acc_a", "st_bad", "rt_a")),
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path();
            write(dir, "accounts/acc_a.json", account("acc_a", "1", "active"));
            write(
                dir,
                "sources/src_a.json",
                source("src_a", "acc_a", "st_a", "rt_a"),
            );
            if let Some(bad) = bad {
                write(dir, file, bad);
            } else {
                fs::write(dir.join(file), "{ incomplete").unwrap();
            }
            let error = open_database(dir).unwrap_err();
            assert!(error.message.contains(file), "{}", error.message);
            assert!(!dir.join(DATABASE_FILE).exists());
            assert_eq!(
                fs::read_dir(dir).unwrap().count(),
                2,
                "only legacy directories remain"
            );
            fs::remove_file(dir.join(file)).unwrap();
            let db = open_database(dir).unwrap();
            let count: i64 = db
                .query_row("SELECT count(*) FROM sources", [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }
    }
}
