use std::sync::Arc;
use std::time::Instant;

use crate::config::Config;
use crate::domain::account::{self, AccountStatus};
use crate::domain::provider::ProviderKind;
use crate::domain::runtime_log::{RuntimeLog, sanitize_runtime_log_error};
use crate::error::AppError;
use crate::error::ErrorCode;
use crate::repos::account::AccountRepo;
use crate::repos::runtime_log::RuntimeLogRepo;
use crate::repos::source::SourceRepo;
use crate::services::kugou_lite_client::{KugouLiteClient, MusicUrlRequest};

#[derive(serde::Deserialize)]
pub struct RuntimeMusicUrlRequest {
    pub hash: String,
    pub album_audio_id: Option<String>,
    pub quality: String,
}

#[derive(serde::Serialize)]
pub struct RuntimeMusicUrlResponse {
    pub url: String,
}

pub struct RuntimeService {
    config: Arc<Config>,
    client: Arc<KugouLiteClient>,
    source_repo: Arc<dyn SourceRepo>,
    account_repo: Arc<dyn AccountRepo>,
    log_repo: Arc<dyn RuntimeLogRepo>,
}

impl RuntimeService {
    pub fn new(
        config: Arc<Config>,
        client: Arc<KugouLiteClient>,
        source_repo: Arc<dyn SourceRepo>,
        account_repo: Arc<dyn AccountRepo>,
        log_repo: Arc<dyn RuntimeLogRepo>,
    ) -> Self {
        Self {
            config,
            client,
            source_repo,
            account_repo,
            log_repo,
        }
    }

    pub async fn fetch_music_url(
        &self,
        runtime_token: &str,
        req: RuntimeMusicUrlRequest,
    ) -> Result<RuntimeMusicUrlResponse, AppError> {
        let source = self
            .source_repo
            .find_by_runtime_token(runtime_token)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        if !source.enabled {
            return Err(AppError::source_disabled());
        }

        let mut account = self
            .account_repo
            .find_by_id(&source.account_id)
            .await?
            .ok_or_else(AppError::account_not_found)?;

        if matches!(account.status, AccountStatus::Disabled) {
            return Err(AppError::account_disabled());
        }
        if !account.vip_active {
            return Err(AppError::account_not_vip());
        }

        if account.cookies.is_empty("dfid") {
            let with_dfid = self.client.ensure_dfid(&account.cookies).await?;
            account.cookies = with_dfid;
            self.account_repo.upsert(&account).await?;
        }

        let now = account::now_ts();
        let needs_refresh = account
            .last_refresh_at
            .is_none_or(|t| now - t > self.config.refresh_interval_secs as i64);

        if needs_refresh {
            match self.do_refresh(&mut account).await {
                Ok(()) => {}
                Err(e) => {
                    return self
                        .log_and_err(&source, &account, &req, 0, &e.to_string())
                        .await;
                }
            }
        }

        let upstream_req = MusicUrlRequest {
            hash: req.hash.clone(),
            album_audio_id: req.album_audio_id.clone(),
            quality: req.quality.clone(),
        };

        let start = Instant::now();
        let result = self
            .client
            .fetch_music_url(&account.cookies, &upstream_req)
            .await;
        let latency = start.elapsed().as_millis();

        match result {
            Ok(mr) => {
                account.cookies = mr.cookies;
                self.account_repo.upsert(&account).await?;

                if mr.url.is_empty() {
                    if mr.auth_failed {
                        match self.do_refresh(&mut account).await {
                            Ok(()) => {}
                            Err(e) => {
                                return self
                                    .log_and_err(
                                        &source,
                                        &account,
                                        &req,
                                        mr.status_code,
                                        &e.to_string(),
                                    )
                                    .await;
                            }
                        }

                        let retry = self
                            .client
                            .fetch_music_url(&account.cookies, &upstream_req)
                            .await;
                        let retry_latency = start.elapsed().as_millis();
                        match retry {
                            Ok(rr) => {
                                account.cookies = rr.cookies;
                                self.account_repo.upsert(&account).await?;
                                if rr.url.is_empty() {
                                    self.append_log(AppendLogInput {
                                        source: &source,
                                        account: &account,
                                        req: &req,
                                        endpoint: "song/url",
                                        ok: false,
                                        status_code: rr.status_code,
                                        latency_ms: retry_latency,
                                        error: Some("empty url after retry"),
                                    })
                                    .await;
                                    return Err(AppError::upstream_play_url_empty());
                                }
                                self.append_log(AppendLogInput {
                                    source: &source,
                                    account: &account,
                                    req: &req,
                                    endpoint: "song/url",
                                    ok: true,
                                    status_code: rr.status_code,
                                    latency_ms: retry_latency,
                                    error: None,
                                })
                                .await;
                                return Ok(RuntimeMusicUrlResponse { url: rr.url });
                            }
                            Err(e) => {
                                return self
                                    .log_and_err(&source, &account, &req, 0, &e.to_string())
                                    .await;
                            }
                        }
                    }
                    self.append_log(AppendLogInput {
                        source: &source,
                        account: &account,
                        req: &req,
                        endpoint: "song/url",
                        ok: false,
                        status_code: mr.status_code,
                        latency_ms: latency,
                        error: Some("empty play url"),
                    })
                    .await;
                    return Err(AppError::upstream_play_url_empty());
                }

                self.append_log(AppendLogInput {
                    source: &source,
                    account: &account,
                    req: &req,
                    endpoint: "song/url",
                    ok: true,
                    status_code: mr.status_code,
                    latency_ms: latency,
                    error: None,
                })
                .await;
                Ok(RuntimeMusicUrlResponse { url: mr.url })
            }
            Err(e) => {
                self.log_and_err(&source, &account, &req, 0, &e.to_string())
                    .await
            }
        }
    }

    async fn do_refresh(
        &self,
        account: &mut crate::domain::account::ProviderAccount,
    ) -> Result<(), AppError> {
        let result: Result<(), AppError> = async {
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
            self.account_repo.upsert(account).await?;

            if !account.vip_active {
                return Err(AppError::account_not_vip());
            }

            Ok(())
        }
        .await;

        if let Err(err) = &result
            && err.code != ErrorCode::AccountNotVip
        {
            let now = account::now_ts();
            account.status = AccountStatus::LoginFailed;
            account.last_error = Some(err.to_string());
            account.updated_at = now;
            let _ = self.account_repo.upsert(account).await;
        }

        result
    }

    async fn append_log(&self, input: AppendLogInput<'_>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let log = RuntimeLog {
            log_id: format!("log_{}", uuid::Uuid::new_v4()),
            source_id: input.source.source_id.clone(),
            account_id: input.account.account_id.clone(),
            provider: ProviderKind::KugouLite,
            action: "musicUrl".into(),
            request_hash: input.req.hash.clone(),
            album_audio_id: input.req.album_audio_id.clone(),
            requested_quality: input.req.quality.clone(),
            upstream_endpoint: input.endpoint.into(),
            ok: input.ok,
            status_code: input.status_code,
            latency_ms: input.latency_ms,
            error: sanitize_runtime_log_error(input.error),
            created_at: now,
        };
        let _ = self.log_repo.append(&log).await;
    }

    async fn log_and_err(
        &self,
        source: &crate::domain::source::Source,
        account: &crate::domain::account::ProviderAccount,
        req: &RuntimeMusicUrlRequest,
        status_code: u16,
        error: &str,
    ) -> Result<RuntimeMusicUrlResponse, AppError> {
        self.append_log(AppendLogInput {
            source,
            account,
            req,
            endpoint: "song/url",
            ok: false,
            status_code,
            latency_ms: 0,
            error: Some(error),
        })
        .await;
        Err(AppError::upstream_request_failed(error))
    }
}

struct AppendLogInput<'a> {
    source: &'a crate::domain::source::Source,
    account: &'a crate::domain::account::ProviderAccount,
    req: &'a RuntimeMusicUrlRequest,
    endpoint: &'a str,
    ok: bool,
    status_code: u16,
    latency_ms: u128,
    error: Option<&'a str>,
}
