mod legacy;

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
use crate::domain::login_session::LoginSession;
use crate::domain::provider::ProviderKind;
use crate::domain::runtime_log::{ListRuntimeLogsQuery, RuntimeLog, RuntimeLogView};
use crate::domain::source::Source;
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::login_session::LoginSessionRepo;
use crate::repos::runtime_log::RuntimeLogRepo;
use crate::repos::source::SourceRepo;

const SCHEMA_VERSION: i64 = 1;
const DATABASE_FILE: &str = "kg2lx.db";
const DATABASE_QUEUE_CAPACITY: usize = 64;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

pub struct Sqlite {
    jobs: mpsc::Sender<Job>,
}

impl Sqlite {
    pub fn open(data_dir: &Path) -> Result<Self, AppError> {
        let mut connection = open_database(data_dir)?;
        let (jobs, mut worker) = mpsc::channel::<Job>(DATABASE_QUEUE_CAPACITY);
        std::thread::Builder::new()
            .name("kg2lx-sqlite".into())
            .spawn(move || {
                while let Some(job) = worker.blocking_recv() {
                    job(&mut connection);
                }
            })?;
        Ok(Self { jobs })
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
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(sqlite_error)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sqlite_error)?;
    if version != SCHEMA_VERSION {
        return Err(AppError::internal(format!(
            "unsupported SQLite schema version {version} in {}",
            path.display()
        )));
    }
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(sqlite_error)?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(sqlite_error)?;
    connection
        .pragma_update(None, "foreign_keys", true)
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
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    tx.execute_batch(include_str!("sqlite/schema.sql"))
        .map_err(sqlite_error)?;
    legacy::import(&tx, data_dir)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(sqlite_error)?;
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
    SELECT a.account_id, a.provider, a.upstream_userid, a.display_name, a.status,
           a.cookies_json, a.last_refresh_at, a.last_success_at, a.last_error,
           a.last_error_code, a.created_at, a.updated_at, e.items_json
    FROM accounts a LEFT JOIN account_entitlements e USING (account_id)";

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
        last_success_at: row.get(7)?,
        last_error: row.get(8)?,
        last_error_code: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        entitlements: row
            .get::<_, Option<String>>(12)?
            .map(|json| decode(&json, 12))
            .transpose()?,
    })
}

fn write_account(
    conn: &Connection,
    account: &ProviderAccount,
    update: bool,
) -> rusqlite::Result<()> {
    let insert = "INSERT INTO accounts (account_id, provider, upstream_userid, display_name, status,
        cookies_json, last_refresh_at, last_success_at, last_error, last_error_code, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";
    let conflict = if update {
        "ON CONFLICT(account_id) DO UPDATE SET display_name = excluded.display_name,
         status = excluded.status, cookies_json = excluded.cookies_json,
         last_refresh_at = excluded.last_refresh_at, last_success_at = excluded.last_success_at,
         last_error = excluded.last_error, last_error_code = excluded.last_error_code,
         updated_at = excluded.updated_at"
    } else {
        ""
    };
    conn.execute(
        &format!("{insert} {conflict}"),
        params![
            account.account_id,
            account.provider.to_string(),
            account.upstream_userid,
            account.display_name,
            account.status.as_str(),
            encode(&account.cookies)?,
            account.last_refresh_at,
            account.last_success_at,
            account.last_error,
            account.last_error_code,
            account.created_at,
            account.updated_at,
        ],
    )?;
    if let Some(items) = &account.entitlements {
        conn.execute(
            "INSERT INTO account_entitlements (account_id, items_json) VALUES (?1, ?2)
             ON CONFLICT(account_id) DO UPDATE SET items_json = excluded.items_json",
            params![account.account_id, encode(items)?],
        )?;
    }
    Ok(())
}

#[async_trait]
impl AccountRepo for Sqlite {
    async fn find_by_id(&self, account_id: &str) -> Result<Option<ProviderAccount>, AppError> {
        let account_id = account_id.to_owned();
        self.run(move |conn| {
            conn.query_row(
                &format!("{ACCOUNT_SELECT} WHERE a.account_id = ?1"),
                [account_id],
                read_account,
            )
            .optional()
        })
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
                &format!("{ACCOUNT_SELECT} WHERE a.provider = ?1 AND a.upstream_userid = ?2"),
                params![provider.to_string(), upstream_userid],
                read_account,
            )
            .optional()
        })
        .await
    }

    async fn upsert(&self, account: &ProviderAccount) -> Result<(), AppError> {
        let account = account.clone();
        self.run(move |conn| {
            let tx = conn.transaction()?;
            write_account(&tx, &account, true)?;
            tx.commit()
        })
        .await
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

fn write_source(conn: &Connection, source: &Source, update: bool) -> rusqlite::Result<()> {
    let insert = "INSERT INTO sources (source_id, account_id, provider, name, script_token,
        runtime_token, created_at, updated_at, quality_fallback)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)";
    let conflict = if update {
        "ON CONFLICT(source_id) DO UPDATE SET name = excluded.name,
         updated_at = excluded.updated_at, quality_fallback = excluded.quality_fallback"
    } else {
        ""
    };
    conn.execute(
        &format!("{insert} {conflict}"),
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

    async fn upsert(&self, source: &Source) -> Result<(), AppError> {
        let source = source.clone();
        self.run(move |conn| write_source(conn, &source, true))
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
                "INSERT INTO login_sessions (session_id, session_json) VALUES (?1, ?2)
                 ON CONFLICT(session_id) DO UPDATE SET session_json = excluded.session_json",
                params![session.session_id, encode(&session)?],
            )?;
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl RuntimeLogRepo for Sqlite {
    async fn append(&self, log: &RuntimeLog) -> Result<(), AppError> {
        let log = log.clone();
        self.run(move |conn| {
            conn.execute(
                "INSERT INTO event_logs (source_id, ok, payload_json) VALUES (?1, ?2, ?3)",
                params![log.source_id, log.ok, encode(&log)?],
            )?;
            Ok(())
        })
        .await
    }

    async fn list_by_source(
        &self,
        query: &ListRuntimeLogsQuery,
    ) -> Result<Vec<RuntimeLog>, AppError> {
        let source_id = query.source_id.clone();
        let errors_only = query.view == RuntimeLogView::Errors;
        let limit = query.limit as i64;
        self.run(move |conn| {
            conn.prepare(
                "SELECT payload_json FROM event_logs WHERE source_id = ?1 AND (NOT ?2 OR NOT ok)
                 ORDER BY seq DESC LIMIT ?3",
            )?
            .query_map(params![source_id, errors_only, limit], |row| {
                decode(row.get_ref(0)?.as_str()?, 0)
            })?
            .collect()
        })
        .await
    }
}
