use std::sync::Arc;
use std::time::Instant;

use crate::config::Config;
use crate::domain::account::{AccountStatus, ProviderAccount};
use crate::domain::event_log::{Event, EventContext, EventLog, EventTrigger, LoginOutcome};
use crate::domain::login_session::{LoginSession, LoginSessionStatus};
use crate::domain::provider::ProviderKind;
use crate::domain::source;
use crate::error::{AppError, ErrorCode};
use crate::repos::account::AccountRepo;
use crate::repos::event_log::{self, EventLogRepo};
use crate::repos::login_session::LoginSessionRepo;
use crate::repos::source::SourceRepo;
use crate::services::auth::AccountAuth;
use crate::services::kugou_lite_client::KugouLiteClient;
use crate::services::singleflight::KeyedLocks;

#[derive(serde::Serialize)]
pub struct QrLoginStartResponse {
    pub session_id: String,
    pub status: String,
    pub qr_url: String,
    pub qr_base64: Option<String>,
    pub expires_at: i64,
}

#[derive(serde::Serialize)]
pub struct QrLoginPollResponse {
    pub session_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceInfo>,
}

/// 扫码绑定成功后回给前端的账号摘要。
///
/// 这里**只有身份**，没有会员信息：账号级「有没有会员」的聚合布尔值（`vip_active`
/// 与 `vip_type`）已随权益口径统一而删除——绑定成功与否只由登录态决定，权益明细
/// 是另一件事，要看权益请走详情接口的 `entitlements`。
#[derive(serde::Serialize)]
pub struct AccountInfo {
    pub account_id: String,
    pub userid: String,
}

#[derive(serde::Serialize)]
pub struct SourceInfo {
    pub source_id: String,
    pub name: String,
    pub script_url: String,
}

pub struct LoginService {
    config: Arc<Config>,
    client: Arc<KugouLiteClient>,
    auth: Arc<AccountAuth>,
    logs: Arc<dyn EventLogRepo>,
    session_locks: KeyedLocks,
    session_repo: Arc<dyn LoginSessionRepo>,
    account_repo: Arc<dyn AccountRepo>,
    source_repo: Arc<dyn SourceRepo>,
}

impl LoginService {
    pub fn new(
        config: Arc<Config>,
        client: Arc<KugouLiteClient>,
        auth: Arc<AccountAuth>,
        logs: Arc<dyn EventLogRepo>,
        session_repo: Arc<dyn LoginSessionRepo>,
        account_repo: Arc<dyn AccountRepo>,
        source_repo: Arc<dyn SourceRepo>,
    ) -> Self {
        Self {
            config,
            client,
            auth,
            logs,
            session_locks: KeyedLocks::default(),
            session_repo,
            account_repo,
            source_repo,
        }
    }

    pub async fn start_kugou_lite_qr_login(&self) -> Result<QrLoginStartResponse, AppError> {
        let now = crate::domain::now_ts();
        let session_id = format!("sess_{}", uuid::Uuid::new_v4());
        let expires_at = now + 300;

        let mut session = LoginSession {
            session_id: session_id.clone(),
            provider: ProviderKind::KugouLite,
            status: LoginSessionStatus::Pending,
            qr_key: String::new(),
            qr_url: String::new(),
            qr_base64: None,
            temp_cookies: crate::domain::cookie_store::CookieStore::new(),
            bound_account_id: None,
            error: None,
            created_at: now,
            expires_at,
            updated_at: now,
        };

        let key = self.client.request_qr_key(&session.temp_cookies).await;
        let key = self.record_qr_transport_result(key).await?;
        session.qr_key = key.qr_key;
        session.temp_cookies = key.cookies;
        session.updated_at = crate::domain::now_ts();
        let qr = self
            .client
            .create_qr_code(&session.qr_key, &session.temp_cookies)
            .await;
        let qr = self.record_qr_transport_result(qr).await?;
        session.qr_url = qr.qr_url.clone();
        session.qr_base64 = qr.qr_base64.clone();
        session.temp_cookies = qr.cookies;
        session.status = LoginSessionStatus::WaitingScan;
        session.updated_at = crate::domain::now_ts();
        self.session_repo.save(&session).await?;

        Ok(QrLoginStartResponse {
            session_id,
            status: "waiting_scan".into(),
            qr_url: session.qr_url.clone(),
            qr_base64: session.qr_base64.clone(),
            expires_at,
        })
    }

    pub async fn poll_kugou_lite_qr_login(
        &self,
        session_id: &str,
    ) -> Result<QrLoginPollResponse, AppError> {
        let _guard = self.session_locks.lock(session_id).await;
        let mut session = self
            .session_repo
            .find_by_id(session_id)
            .await?
            .ok_or_else(AppError::login_session_not_found)?;

        if matches!(session.status, LoginSessionStatus::Bound) {
            return self.build_bound_response(&session).await;
        }

        if session.status == LoginSessionStatus::Failed {
            return Err(AppError::upstream_login_failed(
                session.error.unwrap_or_else(|| "QR login failed".into()),
            ));
        }
        if session.status == LoginSessionStatus::Pending {
            return Err(AppError::upstream_login_failed(
                "QR login was not initialized",
            ));
        }
        let now = crate::domain::now_ts();
        if session.status == LoginSessionStatus::Expired {
            return Ok(Self::state_response(session_id, "expired"));
        }
        if now >= session.expires_at {
            return self.expire(&mut session).await;
        }
        if session.status == LoginSessionStatus::Authorized {
            return self.finish_binding(&mut session).await;
        }

        if session.qr_key.is_empty() {
            return Err(AppError::upstream_login_failed("QR session has no key"));
        }
        let poll = self
            .client
            .poll_qr_login(&session.qr_key, &session.temp_cookies)
            .await;
        let poll = match self.record_qr_transport_result(poll).await {
            Ok(poll) => poll,
            Err(error) => {
                if error.code == ErrorCode::UpstreamLoginFailed {
                    self.remember_failure(&mut session, &error).await?;
                }
                return Err(error);
            }
        };
        session.temp_cookies = poll.cookies;
        session.updated_at = now;

        match poll.status_code {
            0 => return self.expire(&mut session).await,
            1 | 2 => {
                let (status, label) = if poll.status_code == 1 {
                    (LoginSessionStatus::WaitingScan, "waiting_scan")
                } else {
                    (LoginSessionStatus::WaitingConfirm, "waiting_confirm")
                };
                session.status = status;
                self.session_repo.save(&session).await?;
                return Ok(Self::state_response(session_id, label));
            }
            4 => {
                session.status = LoginSessionStatus::Authorized;
                self.session_repo.save(&session).await?;
            }
            _ => {
                let error = AppError::upstream_login_failed(
                    poll.message
                        .unwrap_or_else(|| "unknown qr check error".into()),
                );
                self.remember_failure(&mut session, &error).await?;
                self.log_login(
                    EventContext::new(EventTrigger::Qr, None, None),
                    LoginOutcome::Failed,
                    None,
                    Some(&error),
                )
                .await;
                return Err(error);
            }
        }

        self.finish_binding(&mut session).await
    }

    async fn finish_binding(
        &self,
        session: &mut LoginSession,
    ) -> Result<QrLoginPollResponse, AppError> {
        let started = Instant::now();
        let userid = match session
            .temp_cookies
            .get("userid")
            .filter(|id| !id.is_empty() && *id != "0")
        {
            Some(id) => id.to_owned(),
            None => {
                let error =
                    AppError::upstream_login_failed("missing userid after qr authorization");
                self.remember_failure(session, &error).await?;
                self.log_login(
                    EventContext::new(EventTrigger::Qr, None, None),
                    LoginOutcome::Failed,
                    None,
                    Some(&error),
                )
                .await;
                return Err(error);
            }
        };
        let _identity = self.auth.identities.lock(&userid).await;
        let existing = self
            .account_repo
            .find_by_provider_userid(ProviderKind::KugouLite, &userid)
            .await?;
        let account_id = existing.as_ref().map_or_else(
            || format!("acc_{}", uuid::Uuid::new_v4()),
            |account| account.account_id.clone(),
        );
        let rebind = existing.is_some();
        let existing_source = if rebind {
            self.source_repo.find_by_account_id(&account_id).await?
        } else {
            None
        };
        let context = EventContext::new(EventTrigger::Qr, Some(account_id.clone()), None);
        let result = async {
            let mut cookies = session.temp_cookies.clone();
            if let Err(error) = self
                .auth
                .authenticate_qr(&mut cookies, &userid, context.clone())
                .await
            {
                if cookies.get("userid") == Some(userid.as_str()) && cookies != session.temp_cookies
                {
                    session.temp_cookies = cookies;
                    self.session_repo.save(session).await?;
                }
                return Err(error);
            }
            if cookies.get("userid") != Some(userid.as_str()) {
                return Err(AppError::upstream_login_failed(
                    "refreshed userid differs from QR authorization",
                ));
            }
            let now = crate::domain::now_ts();
            let mut account = existing.unwrap_or_else(|| ProviderAccount {
                account_id: account_id.clone(),
                provider: ProviderKind::KugouLite,
                upstream_userid: userid.clone(),
                status: AccountStatus::Active,
                cookies: crate::domain::cookie_store::CookieStore::new(),
                credential_version: 0,
                last_refresh_at: None,
                last_error: None,
                last_error_code: None,
                created_at: now,
                updated_at: now,
            });
            account.cookies = cookies;
            account.last_refresh_at = Some(now);
            account.updated_at = now;

            let mut bound = session.clone();
            bound.status = LoginSessionStatus::Bound;
            bound.bound_account_id = Some(account_id.clone());
            bound.updated_at = now;
            bound.temp_cookies = crate::domain::cookie_store::CookieStore::new();
            let new_source = existing_source.is_none().then(|| source::Source {
                source_id: format!("src_{}", uuid::Uuid::new_v4()),
                account_id: account_id.clone(),
                provider: ProviderKind::KugouLite,
                name: source::default_name(&userid),
                script_token: format!("st_{}", uuid::Uuid::new_v4()),
                runtime_token: format!("rt_{}", uuid::Uuid::new_v4()),
                created_at: now,
                updated_at: now,
                quality_fallback: false,
            });
            self.session_repo
                .bind(&bound, &account, new_source.as_ref())
                .await?;
            *session = bound;
            self.build_bound_response(session).await
        }
        .await;
        if let Err(error) = &result
            && session.status != LoginSessionStatus::Bound
            && matches!(
                error.code,
                ErrorCode::UpstreamLoginFailed
                    | ErrorCode::AccountNotFound
                    | ErrorCode::SourceNotFound
            )
        {
            self.remember_failure(session, error).await?;
        }
        let outcome = match (&result, rebind) {
            (Ok(_), true) => LoginOutcome::Rebound,
            (Ok(_), false) => LoginOutcome::Bound,
            (Err(_), _) => LoginOutcome::Failed,
        };
        self.log_login(
            context,
            outcome,
            Some(started.elapsed().as_millis() as u64),
            result.as_ref().err(),
        )
        .await;
        result
    }

    async fn remember_failure(
        &self,
        session: &mut LoginSession,
        error: &AppError,
    ) -> Result<(), AppError> {
        session.status = LoginSessionStatus::Failed;
        session.error = Some(error.message.clone());
        session.updated_at = crate::domain::now_ts();
        session.temp_cookies = crate::domain::cookie_store::CookieStore::new();
        self.session_repo.save(session).await
    }

    async fn expire(&self, session: &mut LoginSession) -> Result<QrLoginPollResponse, AppError> {
        session.status = LoginSessionStatus::Expired;
        session.updated_at = crate::domain::now_ts();
        self.session_repo.save(session).await?;
        self.log_login(
            EventContext::new(EventTrigger::Qr, None, None),
            LoginOutcome::Expired,
            None,
            None,
        )
        .await;
        Ok(Self::state_response(&session.session_id, "expired"))
    }

    async fn record_qr_transport_result<T>(
        &self,
        result: Result<T, AppError>,
    ) -> Result<T, AppError> {
        if let Err(error) = &result {
            self.log_login(
                EventContext::new(EventTrigger::Qr, None, None),
                LoginOutcome::Failed,
                None,
                Some(error),
            )
            .await;
        }
        result
    }

    async fn log_login(
        &self,
        context: EventContext,
        outcome: LoginOutcome,
        latency_ms: Option<u64>,
        error: Option<&AppError>,
    ) {
        let mut event = EventLog::new(
            context,
            Event::AccountLogin { outcome },
            latency_ms,
            error.map(|error| error.code),
        );
        event.ok = matches!(outcome, LoginOutcome::Bound | LoginOutcome::Rebound);
        event_log::record(self.logs.as_ref(), event).await;
    }

    async fn build_bound_response(
        &self,
        session: &LoginSession,
    ) -> Result<QrLoginPollResponse, AppError> {
        let account_id = session
            .bound_account_id
            .as_ref()
            .ok_or_else(AppError::account_not_found)?;

        let account = self
            .account_repo
            .find_by_id(account_id)
            .await?
            .ok_or_else(AppError::account_not_found)?;

        let src = self
            .source_repo
            .find_by_account_id(account_id)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        let script_url = format!("{}/s/{}.js", self.config.public_base_url, src.script_token);

        Ok(QrLoginPollResponse {
            session_id: session.session_id.clone(),
            status: "bound".into(),
            account: Some(AccountInfo {
                account_id: account.account_id,
                userid: account.upstream_userid,
            }),
            source: Some(SourceInfo {
                source_id: src.source_id,
                name: src.name,
                script_url,
            }),
        })
    }

    fn state_response(session_id: &str, status: &str) -> QrLoginPollResponse {
        QrLoginPollResponse {
            session_id: session_id.into(),
            status: status.into(),
            account: None,
            source: None,
        }
    }
}
