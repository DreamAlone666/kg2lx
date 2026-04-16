use std::sync::Arc;

use crate::config::Config;
use crate::domain::account::{self, AccountStatus, ProviderAccount};
use crate::domain::login_session::{self, LoginSession, LoginSessionStatus};
use crate::domain::provider::ProviderKind;
use crate::domain::source;
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::login_session::LoginSessionRepo;
use crate::repos::source::SourceRepo;
use crate::services::kugou_lite_client::KugouLiteClient;

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

#[derive(serde::Serialize)]
pub struct AccountInfo {
    pub account_id: String,
    pub userid: String,
    pub vip_active: bool,
    pub vip_type: i32,
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
    session_repo: Arc<dyn LoginSessionRepo>,
    account_repo: Arc<dyn AccountRepo>,
    source_repo: Arc<dyn SourceRepo>,
}

impl LoginService {
    pub fn new(
        config: Arc<Config>,
        client: Arc<KugouLiteClient>,
        session_repo: Arc<dyn LoginSessionRepo>,
        account_repo: Arc<dyn AccountRepo>,
        source_repo: Arc<dyn SourceRepo>,
    ) -> Self {
        Self {
            config,
            client,
            session_repo,
            account_repo,
            source_repo,
        }
    }

    pub async fn start_kugou_lite_qr_login(&self) -> Result<QrLoginStartResponse, AppError> {
        let now = login_session::now_ts();
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

        self.session_repo.save(&session).await?;

        let key = self.client.request_qr_key(&session.temp_cookies).await?;
        session.qr_key = key.qr_key;
        session.temp_cookies = key.cookies;
        session.updated_at = login_session::now_ts();
        self.session_repo.save(&session).await?;

        let qr = self
            .client
            .create_qr_code(&session.qr_key, &session.temp_cookies)
            .await?;
        session.qr_url = qr.qr_url.clone();
        session.qr_base64 = qr.qr_base64.clone();
        session.temp_cookies = qr.cookies;
        session.status = LoginSessionStatus::WaitingScan;
        session.updated_at = login_session::now_ts();
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
        let mut session = self
            .session_repo
            .find_by_id(session_id)
            .await?
            .ok_or_else(AppError::login_session_not_found)?;

        if matches!(session.status, LoginSessionStatus::Bound) {
            return self.build_bound_response(&session).await;
        }

        let now = login_session::now_ts();
        if now >= session.expires_at {
            session.status = LoginSessionStatus::Expired;
            session.updated_at = now;
            self.session_repo.save(&session).await?;
            return Ok(Self::state_response(session_id, "expired"));
        }

        let poll = self
            .client
            .poll_qr_login(&session.qr_key, &session.temp_cookies)
            .await?;
        session.temp_cookies = poll.cookies;
        session.updated_at = now;
        self.session_repo.save(&session).await?;

        match poll.status_code {
            0 => {
                session.status = LoginSessionStatus::Expired;
                session.updated_at = now;
                self.session_repo.save(&session).await?;
                return Ok(Self::state_response(session_id, "expired"));
            }
            2 => {
                session.status = LoginSessionStatus::WaitingConfirm;
                session.updated_at = now;
                self.session_repo.save(&session).await?;
                return Ok(Self::state_response(session_id, "waiting_confirm"));
            }
            1 => {
                session.updated_at = now;
                self.session_repo.save(&session).await?;
                return Ok(Self::state_response(session_id, "waiting_scan"));
            }
            4 => {}
            _ => {
                session.status = LoginSessionStatus::Failed;
                session.error = poll.message.clone();
                session.updated_at = now;
                self.session_repo.save(&session).await?;
                return Err(AppError::upstream_login_failed(
                    poll.message.unwrap_or_else(|| "unknown qr check error".into()),
                ));
            }
        }

        session.status = LoginSessionStatus::Authorized;
        session.updated_at = now;
        self.session_repo.save(&session).await?;

        let refresh = self.client.refresh_login(&session.temp_cookies).await?;
        session.temp_cookies = refresh.cookies;
        session.updated_at = login_session::now_ts();
        self.session_repo.save(&session).await?;

        if session.temp_cookies.is_empty("dfid") {
            let with_dfid = self.client.ensure_dfid(&session.temp_cookies).await?;
            session.temp_cookies = with_dfid;
            session.updated_at = login_session::now_ts();
            self.session_repo.save(&session).await?;
        }

        let vip = self
            .client
            .fetch_vip_status(&session.temp_cookies)
            .await?;
        session.temp_cookies = vip.cookies.clone();
        session.updated_at = login_session::now_ts();
        self.session_repo.save(&session).await?;

        if !vip.vip_active {
            session.status = LoginSessionStatus::Failed;
            session.error = Some("VIP not active for concept membership".into());
            session.updated_at = login_session::now_ts();
            self.session_repo.save(&session).await?;
            return Err(AppError::account_not_vip());
        }

        let userid = session
            .temp_cookies
            .get("userid")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::upstream_login_failed("missing userid after qr authorization"))?
            .to_string();

        let existing = self
            .account_repo
            .find_by_provider_userid(ProviderKind::KugouLite, &userid)
            .await?;

        let now2 = account::now_ts();
        let account = match existing {
            Some(mut acc) => {
                acc.cookies = session.temp_cookies.clone();
                acc.status = AccountStatus::Active;
                acc.vip_type = vip.vip_type;
                acc.vip_active = vip.vip_active;
                acc.last_refresh_at = Some(now2);
                acc.last_success_at = Some(now2);
                acc.last_error = None;
                acc.updated_at = now2;
                self.account_repo.upsert(&acc).await?;
                acc
            }
            None => {
                let account_id = format!("acc_{}", uuid::Uuid::new_v4());
                let acc = ProviderAccount {
                    account_id: account_id.clone(),
                    provider: ProviderKind::KugouLite,
                    upstream_userid: userid.clone(),
                    display_name: None,
                    status: AccountStatus::Active,
                    vip_type: vip.vip_type,
                    vip_active: vip.vip_active,
                    cookies: session.temp_cookies.clone(),
                    last_refresh_at: Some(now2),
                    last_success_at: Some(now2),
                    last_error: None,
                    created_at: now2,
                    updated_at: now2,
                };
                self.account_repo.upsert(&acc).await?;
                acc
            }
        };

        let existing_source = self.source_repo.find_by_account_id(&account.account_id).await?;

        let src = match existing_source {
            Some(s) => s,
            None => {
                let source_id = format!("src_{}", uuid::Uuid::new_v4());
                let script_token = format!("st_{}", uuid::Uuid::new_v4());
                let runtime_token = format!("rt_{}", uuid::Uuid::new_v4());
                let name = format!("{} {}", self.config.source_name_prefix, userid);
                let now3 = source::now_ts();
                let s = source::Source {
                    source_id,
                    account_id: account.account_id.clone(),
                    provider: ProviderKind::KugouLite,
                    name,
                    enabled: true,
                    script_token,
                    runtime_token,
                    created_at: now3,
                    updated_at: now3,
                };
                self.source_repo.upsert(&s).await?;
                s
            }
        };

        session.status = LoginSessionStatus::Bound;
        session.bound_account_id = Some(account.account_id.clone());
        session.updated_at = now2;
        self.session_repo.save(&session).await?;

        let script_url = format!(
            "{}/s/{}.js",
            self.config.public_base_url, src.script_token
        );

        Ok(QrLoginPollResponse {
            session_id: session_id.into(),
            status: "bound".into(),
            account: Some(AccountInfo {
                account_id: account.account_id,
                userid,
                vip_active: vip.vip_active,
                vip_type: vip.vip_type,
            }),
            source: Some(SourceInfo {
                source_id: src.source_id,
                name: src.name,
                script_url,
            }),
        })
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

        let script_url = format!(
            "{}/s/{}.js",
            self.config.public_base_url, src.script_token
        );

        Ok(QrLoginPollResponse {
            session_id: session.session_id.clone(),
            status: "bound".into(),
            account: Some(AccountInfo {
                account_id: account.account_id,
                userid: account.upstream_userid,
                vip_active: account.vip_active,
                vip_type: account.vip_type,
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
