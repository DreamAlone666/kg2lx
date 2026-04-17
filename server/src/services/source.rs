use std::sync::Arc;

use crate::config::Config;
use crate::domain::account::{self, AccountStatus};
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::source::SourceRepo;
use crate::services::kugou_lite_client::KugouLiteClient;

#[derive(serde::Serialize)]
pub struct SourceListItem {
    pub source_id: String,
    pub account_id: String,
    pub provider: String,
    pub enabled: bool,
    pub script_url: String,
    pub userid: String,
    pub vip_active: bool,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub struct SourceDetail {
    pub source_id: String,
    pub account_id: String,
    pub provider: String,
    pub enabled: bool,
    pub script_url: String,
    pub runtime_token_preview: String,
    pub account: SourceAccountInfo,
}

#[derive(serde::Serialize)]
pub struct SourceAccountInfo {
    pub userid: String,
    pub vip_active: bool,
    pub vip_type: i32,
    pub status: String,
    pub last_refresh_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(serde::Serialize)]
pub struct RefreshResponse {
    pub ok: bool,
    pub source_id: String,
    pub vip_active: bool,
    pub updated_at: i64,
}

pub struct SourceService {
    config: Arc<Config>,
    client: Arc<KugouLiteClient>,
    account_repo: Arc<dyn AccountRepo>,
    source_repo: Arc<dyn SourceRepo>,
}

impl SourceService {
    pub fn new(
        config: Arc<Config>,
        client: Arc<KugouLiteClient>,
        account_repo: Arc<dyn AccountRepo>,
        source_repo: Arc<dyn SourceRepo>,
    ) -> Self {
        Self {
            config,
            client,
            account_repo,
            source_repo,
        }
    }

    pub async fn list_sources(&self) -> Result<Vec<SourceListItem>, AppError> {
        let sources = self.source_repo.list_all().await?;
        let mut items = Vec::new();
        for s in sources {
            let account = self.account_repo.find_by_id(&s.account_id).await?;
            let (userid, vip_active) = match account {
                Some(ref a) => (a.upstream_userid.clone(), a.vip_active),
                None => (String::new(), false),
            };
            let script_url = format!("{}/s/{}.js", self.config.public_base_url, s.script_token);
            items.push(SourceListItem {
                source_id: s.source_id,
                account_id: s.account_id,
                provider: s.provider.to_string(),
                enabled: s.enabled,
                script_url,
                userid,
                vip_active,
                updated_at: s.updated_at,
            });
        }
        Ok(items)
    }

    pub async fn get_source(&self, source_id: &str) -> Result<SourceDetail, AppError> {
        let s = self
            .source_repo
            .find_by_id(source_id)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        let account = self
            .account_repo
            .find_by_id(&s.account_id)
            .await?
            .ok_or_else(AppError::account_not_found)?;

        let script_url = format!("{}/s/{}.js", self.config.public_base_url, s.script_token);

        let rt_preview = if s.runtime_token.len() > 8 {
            format!("{}...", &s.runtime_token[..8])
        } else {
            format!("{}...", s.runtime_token)
        };

        Ok(SourceDetail {
            source_id: s.source_id,
            account_id: s.account_id,
            provider: s.provider.to_string(),
            enabled: s.enabled,
            script_url,
            runtime_token_preview: rt_preview,
            account: SourceAccountInfo {
                userid: account.upstream_userid,
                vip_active: account.vip_active,
                vip_type: account.vip_type,
                status: format!("{:?}", account.status).to_lowercase(),
                last_refresh_at: account.last_refresh_at,
                last_error: account.last_error,
            },
        })
    }

    pub async fn refresh_source(&self, source_id: &str) -> Result<RefreshResponse, AppError> {
        let s = self
            .source_repo
            .find_by_id(source_id)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        let mut account = self
            .account_repo
            .find_by_id(&s.account_id)
            .await?
            .ok_or_else(AppError::account_not_found)?;

        let result: Result<i64, AppError> = async {
            let refresh = self.client.refresh_login(&account.cookies).await?;
            account.cookies = refresh.cookies;

            if account.cookies.is_empty("dfid") {
                let with_dfid = self.client.ensure_dfid(&account.cookies).await?;
                account.cookies = with_dfid;
            }

            let vip = self.client.fetch_vip_status(&account.cookies).await?;
            account.cookies = vip.cookies;
            account.vip_type = vip.vip_type;
            account.vip_active = vip.vip_active;

            let now = account::now_ts();
            account.last_refresh_at = Some(now);
            account.last_success_at = Some(now);
            account.last_error = None;
            account.status = if vip.vip_active {
                AccountStatus::Active
            } else {
                AccountStatus::Expired
            };
            account.updated_at = now;
            self.account_repo.upsert(&account).await?;
            Ok(now)
        }
        .await;

        let now = match result {
            Ok(now) => now,
            Err(err) => {
                let now = account::now_ts();
                account.status = AccountStatus::LoginFailed;
                account.last_error = Some(err.to_string());
                account.updated_at = now;
                let _ = self.account_repo.upsert(&account).await;
                return Err(err);
            }
        };

        Ok(RefreshResponse {
            ok: true,
            source_id: source_id.into(),
            vip_active: account.vip_active,
            updated_at: now,
        })
    }
}
