use std::sync::Arc;

use crate::config::Config;
use crate::domain::entitlement::EntitlementSnapshot;
use crate::domain::event_log::{EventContext, EventTrigger};
use crate::domain::renewal::{RenewalReason, RenewalStatus};
use crate::domain::source::{self, SCRIPT_NAME_ADVISORY_CHARS};
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::sqlite::{AccountRecord, Sqlite};
use crate::services::auth::AccountAuth;
use crate::services::entitlement::EntitlementService;

#[derive(serde::Serialize)]
pub struct AccountSourceItem {
    pub source_id: String,
    pub provider: String,
    pub name: String,
    pub default_name: String,
    pub name_advisory_chars: usize,
    pub script_url: String,
    pub runtime_token_preview: String,
    pub quality_fallback: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub struct AccountItem {
    pub account_id: String,
    pub provider: String,
    pub userid: String,
    pub status: String,
    pub last_refresh_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_code: Option<String>,
    #[serde(flatten)]
    pub entitlement: EntitlementSnapshot,
    pub renewal: RenewalStatus,
    pub sources: Vec<AccountSourceItem>,
}

#[derive(serde::Serialize)]
pub struct RefreshResponse {
    pub ok: bool,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub struct SyncEntitlementsResponse {
    pub ok: bool,
    pub synced_at: i64,
}

pub struct AccountService {
    config: Arc<Config>,
    storage: Arc<Sqlite>,
    auth: Arc<AccountAuth>,
    entitlements: Arc<EntitlementService>,
}

impl AccountService {
    pub fn new(
        config: Arc<Config>,
        storage: Arc<Sqlite>,
        auth: Arc<AccountAuth>,
        entitlements: Arc<EntitlementService>,
    ) -> Self {
        Self {
            config,
            storage,
            auth,
            entitlements,
        }
    }

    pub async fn list(&self) -> Result<Vec<AccountItem>, AppError> {
        let records = self
            .storage
            .account_records(None, self.entitlements.current_time())
            .await?;
        Ok(records
            .into_iter()
            .map(|record| self.present(record))
            .collect())
    }

    pub async fn get(&self, account_id: &str) -> Result<AccountItem, AppError> {
        self.storage
            .account_records(Some(account_id), self.entitlements.current_time())
            .await?
            .into_iter()
            .next()
            .map(|record| self.present(record))
            .ok_or_else(AppError::account_not_found)
    }

    pub async fn refresh(&self, account_id: &str) -> Result<RefreshResponse, AppError> {
        let account = self
            .storage
            .find_by_id(account_id)
            .await?
            .ok_or_else(AppError::account_not_found)?;
        let context = EventContext::new(EventTrigger::Admin, Some(account_id.to_owned()), None);
        let account = self.auth.refresh_manual(account, context).await?;
        Ok(RefreshResponse {
            ok: true,
            updated_at: account.last_refresh_at.unwrap_or(account.updated_at),
        })
    }

    pub async fn sync(&self, account_id: String) -> Result<SyncEntitlementsResponse, AppError> {
        let context = EventContext::new(EventTrigger::Admin, Some(account_id.clone()), None);
        let synced_at = self
            .entitlements
            .sync(account_id, context, RenewalReason::Manual)
            .await?;
        Ok(SyncEntitlementsResponse {
            ok: true,
            synced_at,
        })
    }

    pub async fn set_auto_claim(
        &self,
        account_id: &str,
        enabled: bool,
    ) -> Result<RenewalStatus, AppError> {
        self.entitlements.set_auto_claim(account_id, enabled).await
    }

    pub async fn claim(&self, account_id: String) -> Result<RenewalStatus, AppError> {
        self.entitlements.renew_manually(account_id).await
    }

    fn present(&self, record: AccountRecord) -> AccountItem {
        let AccountRecord {
            account,
            entitlement,
            renewal,
            sources,
        } = record;
        let default_name = source::default_name(&account.upstream_userid);
        let sources = sources
            .into_iter()
            .map(|source| {
                let prefix: String = source.runtime_token.chars().take(8).collect();
                AccountSourceItem {
                    source_id: source.source_id,
                    provider: source.provider.to_string(),
                    name: source.name,
                    default_name: default_name.clone(),
                    name_advisory_chars: SCRIPT_NAME_ADVISORY_CHARS,
                    script_url: format!(
                        "{}/s/{}.js",
                        self.config.public_base_url, source.script_token
                    ),
                    runtime_token_preview: format!("{prefix}..."),
                    quality_fallback: source.quality_fallback,
                    created_at: source.created_at,
                    updated_at: source.updated_at,
                }
            })
            .collect();
        AccountItem {
            account_id: account.account_id,
            provider: account.provider.to_string(),
            userid: account.upstream_userid,
            status: account.status.as_str().to_owned(),
            last_refresh_at: account.last_refresh_at,
            last_error: account.last_error,
            last_error_code: account.last_error_code,
            entitlement,
            renewal,
            sources,
        }
    }
}
