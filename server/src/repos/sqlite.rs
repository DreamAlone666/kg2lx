mod events;
mod legacy;
mod migrations;
mod renewal;

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tempfile::Builder;
use tokio::sync::{mpsc, oneshot};

use crate::domain::account::{AccountStatus, ProviderAccount};
use crate::domain::cookie_store::CookieStore;
use crate::domain::entitlement::{EntitlementObservation, EntitlementSnapshot};
use crate::domain::login_session::LoginSession;
use crate::domain::provider::ProviderKind;
use crate::domain::source::Source;
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::entitlement::EntitlementRepo;
use crate::repos::login_session::LoginSessionRepo;
use crate::repos::source::SourceRepo;

const DATABASE_FILE: &str = "kg2lx.db";
const DATABASE_QUEUE_CAPACITY: usize = 64;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

pub struct Sqlite {
    jobs: mpsc::Sender<Job>,
    event_log_limit: i64,
}

impl Sqlite {
    pub fn open(data_dir: &Path, event_log_limit: u32) -> Result<Self, AppError> {
        let mut connection = open_database(data_dir)?;
        let event_log_limit = i64::from(event_log_limit);
        events::prune_all(&mut connection, event_log_limit).map_err(sqlite_error)?;
        let (jobs, mut worker) = mpsc::channel::<Job>(DATABASE_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("kg2lx-sqlite".into())
            .spawn(move || {
                while let Some(job) = worker.blocking_recv() {
                    job(&mut connection);
                }
            })?;
        Ok(Self {
            jobs,
            event_log_limit,
        })
    }

    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T, AppError> {
        let (reply, receive) = oneshot::channel();
        self.jobs
            .send(Box::new(move |conn| {
                let _ = reply.send(f(conn));
            }))
            .await
            .map_err(|_| AppError::internal("SQLite worker stopped"))?;
        receive
            .await
            .map_err(|_| AppError::internal("SQLite worker did not return a result"))?
            .map_err(sqlite_error)
    }
}

fn open_database(data_dir: &Path) -> Result<Connection, AppError> {
    fs::create_dir_all(data_dir)?;
    let path = data_dir.join(DATABASE_FILE);
    if !path.try_exists()? {
        initialize(data_dir, &path)?;
    }
    let mut connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(sqlite_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(sqlite_error)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(sqlite_error)?;
    migrations::migrate(&mut connection, &path, 1, migrations::MIGRATIONS)?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(sqlite_error)?;
    Ok(connection)
}

fn initialize(data_dir: &Path, target: &Path) -> Result<(), AppError> {
    // Publish only a complete database. An import error leaves the destination absent, and
    // simultaneous initializers may only install their prepared file if it is still absent.
    let file = Builder::new()
        .prefix(".kg2lx-init-")
        .tempfile_in(data_dir)?;
    let mut connection = Connection::open(file.path()).map_err(sqlite_error)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(sqlite_error)?;
    migrations::migrate(&mut connection, target, 0, migrations::MIGRATIONS)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    legacy::import(&tx, data_dir)?;
    tx.commit().map_err(sqlite_error)?;
    connection
        .close()
        .map_err(|(_, error)| sqlite_error(error))?;
    match file.persist_noclobber(target) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.error.into()),
    }
}

fn sqlite_error(error: rusqlite::Error) -> AppError {
    AppError::internal(format!("SQLite: {error}"))
}

fn encode(value: &impl Serialize) -> rusqlite::Result<String> {
    serde_json::to_string(value)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn decode<T: DeserializeOwned>(json: &str, column: usize) -> rusqlite::Result<T> {
    serde_json::from_str(json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn provider_at(row: &Row<'_>, index: usize) -> rusqlite::Result<ProviderKind> {
    match row.get_ref(index)?.as_str()? {
        "kugou_lite" => Ok(ProviderKind::KugouLite),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

const ACCOUNT_SELECT: &str = "
    SELECT account_id, provider, upstream_userid, display_name, status, cookies_json,
           last_refresh_at, credential_version, last_error, last_error_code, created_at, updated_at
    FROM accounts";

fn read_account(row: &Row<'_>) -> rusqlite::Result<ProviderAccount> {
    Ok(ProviderAccount {
        account_id: row.get(0)?,
        provider: provider_at(row, 1)?,
        upstream_userid: row.get(2)?,
        display_name: row.get(3)?,
        status: match row.get_ref(4)?.as_str()? {
            "active" => AccountStatus::Active,
            "pending" => AccountStatus::Pending,
            "disabled" => AccountStatus::Disabled,
            "login_failed" => AccountStatus::LoginFailed,
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
        cookies: decode(row.get_ref(5)?.as_str()?, 5)?,
        last_refresh_at: row.get(6)?,
        credential_version: row.get(7)?,
        last_error: row.get(8)?,
        last_error_code: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn account_by_id(conn: &Connection, account_id: &str) -> rusqlite::Result<ProviderAccount> {
    conn.prepare_cached(&format!("{ACCOUNT_SELECT} WHERE account_id = ?1"))?
        .query_row([account_id], read_account)
}

fn write_account(conn: &Connection, account: &ProviderAccount) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO accounts (account_id, provider, upstream_userid, display_name, status,
         cookies_json, last_refresh_at, credential_version, last_error, last_error_code, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            account.account_id, account.provider.to_string(), account.upstream_userid,
            account.display_name, account.status.as_str(), encode(&account.cookies)?,
            account.last_refresh_at, account.credential_version, account.last_error,
            account.last_error_code, account.created_at, account.updated_at,
        ],
    )?;
    Ok(())
}

#[async_trait]
impl AccountRepo for Sqlite {
    async fn find_by_id(&self, account_id: &str) -> Result<Option<ProviderAccount>, AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| account_by_id(conn, &account_id).optional())
            .await
    }

    async fn find_by_provider_userid(
        &self,
        provider: ProviderKind,
        upstream_userid: &str,
    ) -> Result<Option<ProviderAccount>, AppError> {
        let upstream_userid = upstream_userid.to_owned();
        self.run(move |conn| {
            conn.query_row(
                &format!("{ACCOUNT_SELECT} WHERE provider = ?1 AND upstream_userid = ?2"),
                params![provider.to_string(), upstream_userid],
                read_account,
            )
            .optional()
        })
        .await
    }

    async fn save_auth(
        &self,
        previous: &ProviderAccount,
        updated: &ProviderAccount,
    ) -> Result<ProviderAccount, AppError> {
        let previous = previous.clone();
        let updated = updated.clone();
        self.run(move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current = account_by_id(&tx, &previous.account_id)?;
            if current.credential_version != previous.credential_version {
                return Ok(current);
            }

            let mut cookies = current.cookies;
            // An authentication response owns the keys it changed. Keep unrelated cookies
            // received while authentication was in flight.
            for (key, value) in updated.cookies.items {
                let authoritative = updated.credential_version != previous.credential_version
                    && matches!(
                        key.as_str(),
                        "token" | "userid" | "dfid" | "t1" | "vip_token" | "vip_type"
                    );
                if authoritative || previous.cookies.get(&key) != Some(value.as_str()) {
                    cookies.insert(key, value);
                }
            }
            tx.execute(
                "UPDATE accounts SET status = ?2, cookies_json = ?3, last_refresh_at = ?4,
                 credential_version = ?5, last_error = ?6, last_error_code = ?7,
                 updated_at = ?8 WHERE account_id = ?1",
                params![
                    updated.account_id,
                    updated.status.as_str(),
                    encode(&cookies)?,
                    updated.last_refresh_at,
                    updated.credential_version,
                    updated.last_error,
                    updated.last_error_code,
                    updated.updated_at
                ],
            )?;
            let account = account_by_id(&tx, &updated.account_id)?;
            tx.commit()?;
            Ok(account)
        })
        .await
    }

    async fn merge_cookies(
        &self,
        request: &ProviderAccount,
        response: &CookieStore,
    ) -> Result<ProviderAccount, AppError> {
        let request = request.clone();
        let response = response.clone();
        self.run(move |conn| {
            if request.cookies == response {
                return account_by_id(conn, &request.account_id);
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut current = account_by_id(&tx, &request.account_id)?;
            if current.credential_version != request.credential_version {
                return Ok(current);
            }
            let mut changed = false;
            for (key, value) in response.items {
                let sent = request.cookies.get(&key);
                if sent != Some(value.as_str()) && current.cookies.get(&key) == sent {
                    current.cookies.insert(key, value);
                    changed = true;
                }
            }
            if changed {
                tx.execute(
                    "UPDATE accounts SET cookies_json = ?2 WHERE account_id = ?1",
                    params![current.account_id, encode(&current.cookies)?],
                )?;
            }
            tx.commit()?;
            Ok(current)
        })
        .await
    }
}

#[async_trait]
impl EntitlementRepo for Sqlite {
    async fn entitlement_snapshot(
        &self,
        account_id: &str,
    ) -> Result<EntitlementSnapshot, AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| {
            let row = conn.query_row(
                "SELECT items_json, synced_at, sync_failed FROM account_entitlements WHERE account_id = ?1",
                [account_id], |row| {
                    let json: Option<String> = row.get(0)?;
                    Ok(EntitlementSnapshot {
                        items: json.map(|value| decode(&value, 0)).transpose()?,
                        synced_at: row.get(1)?,
                        sync_failed: row.get(2)?,
                    })
                },
            ).optional()?;
            Ok(row.unwrap_or_default())
        }).await
    }

    async fn record_sync(
        &self,
        account_id: &str,
        snapshot: Option<(EntitlementObservation, i64)>,
    ) -> Result<(), AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| {
            if let Some((observed, synced_at)) = snapshot {
                conn.execute(
                    "INSERT INTO account_entitlements (account_id, items_json, synced_at, sync_failed, svip_evidence_json)
                     VALUES (?1, ?2, ?3, 0, ?4) ON CONFLICT(account_id) DO UPDATE SET
                     items_json = excluded.items_json, synced_at = excluded.synced_at, sync_failed = 0,
                     svip_evidence_json = excluded.svip_evidence_json",
                    params![account_id, encode(&observed.items)?, synced_at, encode(&observed.svip)?],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO account_entitlements (account_id, sync_failed) VALUES (?1, 1)
                     ON CONFLICT(account_id) DO UPDATE SET sync_failed = 1",
                    [account_id],
                )?;
            }
            Ok(())
        }).await
    }
}

const SOURCE_SELECT: &str = "SELECT source_id, account_id, provider, name, script_token,
    runtime_token, created_at, updated_at, quality_fallback FROM sources";

fn read_source(row: &Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        source_id: row.get(0)?,
        account_id: row.get(1)?,
        provider: provider_at(row, 2)?,
        name: row.get(3)?,
        script_token: row.get(4)?,
        runtime_token: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        quality_fallback: row.get(8)?,
    })
}

fn write_source(conn: &Connection, source: &Source) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sources (source_id, account_id, provider, name, script_token,
         runtime_token, created_at, updated_at, quality_fallback)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            source.source_id,
            source.account_id,
            source.provider.to_string(),
            source.name,
            source.script_token,
            source.runtime_token,
            source.created_at,
            source.updated_at,
            source.quality_fallback,
        ],
    )?;
    Ok(())
}

impl Sqlite {
    async fn find_source(
        &self,
        filter: &'static str,
        value: &str,
    ) -> Result<Option<Source>, AppError> {
        let value = value.to_owned();
        self.run(move |conn| {
            conn.query_row(
                &format!("{SOURCE_SELECT} WHERE {filter}"),
                [value],
                read_source,
            )
            .optional()
        })
        .await
    }
}

#[async_trait]
impl SourceRepo for Sqlite {
    async fn find_by_id(&self, source_id: &str) -> Result<Option<Source>, AppError> {
        self.find_source("source_id = ?1", source_id).await
    }

    async fn find_by_script_token(&self, script_token: &str) -> Result<Option<Source>, AppError> {
        self.find_source("script_token = ?1", script_token).await
    }

    async fn find_by_runtime_token(&self, runtime_token: &str) -> Result<Option<Source>, AppError> {
        self.find_source("runtime_token = ?1", runtime_token).await
    }

    async fn find_by_account_id(&self, account_id: &str) -> Result<Option<Source>, AppError> {
        self.find_source(
            "account_id = ?1 ORDER BY created_at, source_id LIMIT 1",
            account_id,
        )
        .await
    }

    async fn update_settings(
        &self,
        source_id: &str,
        name: Option<String>,
        quality_fallback: Option<bool>,
        updated_at: i64,
    ) -> Result<Source, AppError> {
        let source_id = source_id.to_owned();
        self.run(move |conn| {
            conn.prepare_cached(
                "UPDATE sources SET name = coalesce(?2, name),
                 quality_fallback = coalesce(?3, quality_fallback), updated_at = ?4
                 WHERE source_id = ?1
                 RETURNING source_id, account_id, provider, name, script_token, runtime_token,
                           created_at, updated_at, quality_fallback",
            )?
            .query_row(
                params![source_id, name, quality_fallback, updated_at],
                read_source,
            )
        })
        .await
    }

    async fn list_all(&self) -> Result<Vec<Source>, AppError> {
        self.run(|conn| {
            conn.prepare(SOURCE_SELECT)?
                .query_map([], read_source)?
                .collect()
        })
        .await
    }
}

#[async_trait]
impl LoginSessionRepo for Sqlite {
    async fn find_by_id(&self, session_id: &str) -> Result<Option<LoginSession>, AppError> {
        let session_id = session_id.to_owned();
        self.run(move |conn| {
            conn.query_row(
                "SELECT session_json FROM login_sessions WHERE session_id = ?1",
                [session_id],
                |row| decode(row.get_ref(0)?.as_str()?, 0),
            )
            .optional()
        })
        .await
    }

    async fn save(&self, session: &LoginSession) -> Result<(), AppError> {
        let session = session.clone();
        self.run(move |conn| {
            conn.execute(
                "DELETE FROM login_sessions WHERE expires_at < ?1",
                [crate::domain::login_session::now_ts() - 300],
            )?;
            write_session(conn, &session)
        })
        .await
    }

    async fn bind(
        &self,
        session: &LoginSession,
        account: &ProviderAccount,
        new_source: Option<&Source>,
    ) -> Result<(), AppError> {
        let session = session.clone();
        let account = account.clone();
        let new_source = new_source.cloned();
        self.run(move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let reattached = tx.execute(
                "UPDATE accounts SET cookies_json = ?2, credential_version = credential_version + 1,
                 status = CASE WHEN status = 'disabled' THEN status ELSE 'active' END,
                 last_refresh_at = ?3, last_error = NULL, last_error_code = NULL, updated_at = ?4
                 WHERE account_id = ?1",
                params![account.account_id, encode(&account.cookies)?,
                    account.last_refresh_at, account.updated_at],
            )?;
            if reattached == 0 {
                write_account(&tx, &account)?;
            }
            if let Some(source) = new_source {
                write_source(&tx, &source)?;
            }
            write_session(&tx, &session)?;
            tx.commit()
        })
        .await
    }
}

fn write_session(conn: &Connection, session: &LoginSession) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO login_sessions (session_id, session_json, expires_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(session_id) DO UPDATE SET session_json = excluded.session_json",
        params![session.session_id, encode(&session)?, session.expires_at],
    )?;
    Ok(())
}
