use std::sync::Arc;
use std::time::Instant;

use crate::config::Config;
use crate::domain::account::{AccountStatus, ProviderAccount};
use crate::domain::cookie_store::CookieStore;
use crate::domain::event_log::{Event, EventContext, EventLog, RefreshReason};
use crate::domain::now_ts;
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::event_log::{self, EventLogRepo};
use crate::services::kugou_lite_client::{AuthFailure, KugouLiteClient};
use crate::services::singleflight::{KeyedLocks, SingleFlight};

pub struct AccountAuth {
    client: Arc<KugouLiteClient>,
    accounts: Arc<dyn AccountRepo>,
    logs: Arc<dyn EventLogRepo>,
    refresh_ttl: i64,
    refreshes: SingleFlight<ProviderAccount>,
    manual_refreshes: SingleFlight<ProviderAccount>,
    pub(super) identities: KeyedLocks,
    pub(crate) admin_actions: Arc<KeyedLocks>,
}

impl AccountAuth {
    pub fn new(
        config: &Config,
        client: Arc<KugouLiteClient>,
        accounts: Arc<dyn AccountRepo>,
        logs: Arc<dyn EventLogRepo>,
    ) -> Self {
        Self {
            client,
            accounts,
            logs,
            refresh_ttl: config.account_refresh_ttl as i64,
            refreshes: SingleFlight::default(),
            manual_refreshes: SingleFlight::default(),
            identities: KeyedLocks::default(),
            admin_actions: Arc::default(),
        }
    }

    pub async fn prepare_playback(
        self: &Arc<Self>,
        account: ProviderAccount,
        context: &EventContext,
    ) -> Result<ProviderAccount, AppError> {
        if account.status == AccountStatus::LoginFailed {
            return Err(AppError::upstream_login_failed(
                "account must sign in again",
            ));
        }
        let reason = if account
            .last_refresh_at
            .is_none_or(|t| now_ts() - t > self.refresh_ttl)
        {
            RefreshReason::Ttl
        } else if account.cookies.is_empty("dfid") {
            RefreshReason::Device
        } else {
            return Ok(account);
        };
        self.refresh(account, reason, context.clone()).await
    }

    pub async fn refresh_manual(
        self: &Arc<Self>,
        account: ProviderAccount,
        context: EventContext,
    ) -> Result<ProviderAccount, AppError> {
        let auth = self.clone();
        self.manual_refreshes
            .run(account.account_id.clone(), async move {
                // Only explicit admin actions wait for each other. Playback can always start an
                // auth refresh even when a manual request is queued behind an entitlement sync.
                let _admin = auth.admin_actions.lock(&account.account_id).await;
                auth.refresh(account, RefreshReason::Manual, context).await
            })
            .await
    }

    pub async fn refresh(
        self: &Arc<Self>,
        observed: ProviderAccount,
        reason: RefreshReason,
        context: EventContext,
    ) -> Result<ProviderAccount, AppError> {
        let auth = self.clone();
        let key = if reason == RefreshReason::Device {
            format!("device/{}", observed.account_id)
        } else {
            observed.account_id.clone()
        };
        self.refreshes
            .run(key, async move {
                let _guard = auth.identities.lock(&observed.upstream_userid).await;
                let current = auth
                    .accounts
                    .find_by_id(&observed.account_id)
                    .await?
                    .ok_or_else(AppError::account_not_found)?;
                // Another refresh or QR binding may have completed while this caller waited.
                if reason == RefreshReason::Device {
                    if !current.cookies.is_empty("dfid") {
                        return Ok(current);
                    }
                } else if current.credential_version != observed.credential_version {
                    return Ok(current);
                }
                auth.refresh_current(current, reason, context).await
            })
            .await
    }

    async fn refresh_current(
        &self,
        current: ProviderAccount,
        reason: RefreshReason,
        context: EventContext,
    ) -> Result<ProviderAccount, AppError> {
        let started = Instant::now();
        let mut updated = current.clone();
        let mut token_refreshed = false;
        let outcome = self
            .authenticate(
                &mut updated.cookies,
                &current.upstream_userid,
                reason,
                &mut token_refreshed,
            )
            .await;
        let (endpoint, human_verification) = match &outcome {
            Ok(endpoint) => (*endpoint, false),
            Err((failure, endpoint)) => (*endpoint, failure.human_verification),
        };
        let result = async {
            let returned_userid = updated.cookies.get("userid");
            if (token_refreshed || returned_userid != current.cookies.get("userid"))
                && returned_userid != Some(current.upstream_userid.as_str())
            {
                return Err(mismatched_userid());
            }
            if token_refreshed {
                // Keep a fresh token even if subsequent device registration fails.
                updated.credential_version += 1;
                updated.updated_at = now_ts();
                updated.last_refresh_at = Some(updated.updated_at);
                updated.status = AccountStatus::Active;
                updated.last_error = None;
                updated.last_error_code = None;
                updated = self.accounts.save_auth(&current, &updated).await?;
            } else if let Err((failure, _)) = &outcome
                && failure.invalid_credentials
            {
                updated.status = AccountStatus::LoginFailed;
                updated.updated_at = now_ts();
                updated.last_error = Some(failure.error.message.clone());
                updated.last_error_code = Some(failure.error.code.as_str().into());
                updated = self.accounts.save_auth(&current, &updated).await?;
            } else if updated.cookies != current.cookies {
                updated = self
                    .accounts
                    .merge_cookies(&current, &updated.cookies)
                    .await?;
            }
            outcome
                .map(|_| updated)
                .map_err(|(failure, _)| failure.error)
        }
        .await;
        self.log_refresh(
            context,
            reason,
            started,
            endpoint,
            human_verification,
            &result,
        )
        .await;
        result
    }

    pub async fn authenticate_qr(
        &self,
        cookies: &mut CookieStore,
        userid: &str,
        context: EventContext,
    ) -> Result<(), AppError> {
        let started = Instant::now();
        let reason = RefreshReason::QrLogin;
        let mut token_refreshed = false;
        let (result, endpoint, human_verification) = match self
            .authenticate(cookies, userid, reason, &mut token_refreshed)
            .await
        {
            Ok(endpoint) => (Ok(()), endpoint, false),
            Err((failure, endpoint)) => (Err(failure.error), endpoint, failure.human_verification),
        };
        self.log_refresh(
            context,
            reason,
            started,
            endpoint,
            human_verification,
            &result,
        )
        .await;
        result
    }

    async fn authenticate(
        &self,
        cookies: &mut CookieStore,
        userid: &str,
        reason: RefreshReason,
        token_refreshed: &mut bool,
    ) -> Result<&'static str, (AuthFailure, &'static str)> {
        let timeout = self.client.default_timeout();
        let device_first = matches!(reason, RefreshReason::Ttl | RefreshReason::Device);
        if device_first && cookies.is_empty("dfid") {
            *cookies = self
                .client
                .ensure_dfid(cookies, timeout)
                .await
                .map_err(|failure| (failure, "register/dev"))?;
        }
        if reason == RefreshReason::Device {
            return Ok("register/dev");
        }
        *cookies = self
            .client
            .refresh_login(cookies, timeout)
            .await
            .map_err(|failure| (failure, "login/token"))?;
        *token_refreshed = true;
        if cookies.get("userid") != Some(userid) {
            return Err((AuthFailure::new(mismatched_userid()), "login/token"));
        }
        if cookies.is_empty("dfid") {
            *cookies = self
                .client
                .ensure_dfid(cookies, timeout)
                .await
                .map_err(|failure| (failure, "register/dev"))?;
        }
        Ok("login/token")
    }

    async fn log_refresh<T>(
        &self,
        context: EventContext,
        reason: RefreshReason,
        started: Instant,
        endpoint: &str,
        human_verification: bool,
        result: &Result<T, AppError>,
    ) {
        event_log::record(
            self.logs.as_ref(),
            EventLog::new(
                context,
                Event::TokenRefresh {
                    reason,
                    upstream_endpoint: endpoint.into(),
                    human_verification,
                },
                Some(started.elapsed().as_millis() as u64),
                result.as_ref().err().map(|error| error.code),
            ),
        )
        .await;
    }
}

fn mismatched_userid() -> AppError {
    AppError::upstream_login_failed("upstream returned credentials for a different account")
}
