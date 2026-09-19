use std::sync::Arc;
use std::time::Instant;

use crate::config::Config;
use crate::domain::account::{self, AccountStatus};
use crate::domain::provider::ProviderKind;
use crate::domain::runtime_log::{RuntimeLog, RuntimeLogStage, sanitize_runtime_log_error};
use crate::error::AppError;
use crate::error::ErrorCode;
use crate::repos::account::AccountRepo;
use crate::repos::runtime_log::RuntimeLogRepo;
use crate::repos::source::SourceRepo;
use crate::services::kugou_lite_client::{KugouLiteClient, MusicUrlRequest, map_quality};

#[derive(serde::Deserialize)]
pub struct RuntimeMusicUrlRequest {
    pub hash: String,
    pub album_audio_id: Option<String>,
    pub quality: String,
    pub track_title: Option<String>,
    pub artist_name: Option<String>,
    pub album_name: Option<String>,
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

fn error_code_string(code: ErrorCode) -> String {
    serde_json::to_string(&code)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

/// 本行应当记录的上游质量参数 —— 只有描述 `song/url` 那次调用的行才有值。
///
/// 落盘而不是让前端拿 requested_quality 去推：日志行的职责是记录**发生过什么**，
/// 「这次向上游要的是哪个质量」正是发生过的事实（四档里 flac24bit → high 还是真正的
/// 重命名，常常就是失败与成功的分界）。前端再复制一张映射表，等于把服务端逻辑的影子
/// 副本放进界面，服务端一改就安静地开始说错话。
///
/// 判断用 stage 而不是 upstream_endpoint：两者严格 1:1（FetchMusicUrl ⟺ "song/url",
/// 见本文件各处 append_log 调用点逐一对上），而 stage 是枚举、更适合当判据。
/// `ensure_dfid` / `refresh_login` 这两次调用压根不带质量参数，给它们记一个
/// 「本来会要的质量」就是发明一个没发生过的事实。
fn upstream_quality_for(stage: RuntimeLogStage, quality: &str) -> Option<String> {
    matches!(stage, RuntimeLogStage::FetchMusicUrl).then(|| map_quality(quality).to_string())
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

        let mut refresh_attempted = false;

        if account.cookies.is_empty("dfid") {
            match self.client.ensure_dfid(&account.cookies).await {
                Ok(with_dfid) => {
                    account.cookies = with_dfid;
                    self.account_repo.upsert(&account).await?;
                }
                Err(e) => {
                    return self
                        .log_and_err(
                            &source,
                            &account,
                            &req,
                            LogErrorContext {
                                stage: RuntimeLogStage::EnsureDfid,
                                refresh_attempted: false,
                                retry_count: 0,
                                upstream_endpoint: "ensure_dfid",
                                status_code: None,
                                latency_ms: 0,
                            },
                            &e,
                        )
                        .await;
                }
            }
        }

        let now = account::now_ts();
        let needs_refresh = account
            .last_refresh_at
            .is_none_or(|t| now - t > self.config.refresh_interval_secs as i64);

        if needs_refresh {
            refresh_attempted = true;
            if let Err(e) = self.do_refresh(&mut account).await {
                return self
                    .log_and_err(
                        &source,
                        &account,
                        &req,
                        LogErrorContext {
                            stage: RuntimeLogStage::RefreshLogin,
                            refresh_attempted: true,
                            retry_count: 0,
                            upstream_endpoint: "refresh_login",
                            status_code: None,
                            latency_ms: 0,
                        },
                        &e,
                    )
                    .await;
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
                        refresh_attempted = true;
                        if let Err(e) = self.do_refresh(&mut account).await {
                            return self
                                .log_and_err(
                                    &source,
                                    &account,
                                    &req,
                                    LogErrorContext {
                                        stage: RuntimeLogStage::RefreshLogin,
                                        refresh_attempted: true,
                                        retry_count: 1,
                                        upstream_endpoint: "refresh_login",
                                        status_code: Some(mr.status_code),
                                        latency_ms: start.elapsed().as_millis(),
                                    },
                                    &e,
                                )
                                .await;
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
                                        stage: RuntimeLogStage::FetchMusicUrl,
                                        refresh_attempted,
                                        retry_count: 1,
                                        upstream_endpoint: "song/url",
                                        ok: false,
                                        status_code: Some(rr.status_code),
                                        latency_ms: retry_latency,
                                        error: Some("empty url after retry"),
                                        error_code: Some(error_code_string(
                                            ErrorCode::UpstreamPlayUrlEmpty,
                                        )),
                                    })
                                    .await;
                                    return Err(AppError::upstream_play_url_empty());
                                }
                                self.append_log(AppendLogInput {
                                    source: &source,
                                    account: &account,
                                    req: &req,
                                    stage: RuntimeLogStage::FetchMusicUrl,
                                    refresh_attempted,
                                    retry_count: 1,
                                    upstream_endpoint: "song/url",
                                    ok: true,
                                    status_code: Some(rr.status_code),
                                    latency_ms: retry_latency,
                                    error: None,
                                    error_code: None,
                                })
                                .await;
                                return Ok(RuntimeMusicUrlResponse { url: rr.url });
                            }
                            Err(e) => {
                                return self
                                    .log_and_err(
                                        &source,
                                        &account,
                                        &req,
                                        LogErrorContext {
                                            stage: RuntimeLogStage::FetchMusicUrl,
                                            refresh_attempted,
                                            retry_count: 1,
                                            upstream_endpoint: "song/url",
                                            status_code: None,
                                            latency_ms: start.elapsed().as_millis(),
                                        },
                                        &e,
                                    )
                                    .await;
                            }
                        }
                    }
                    self.append_log(AppendLogInput {
                        source: &source,
                        account: &account,
                        req: &req,
                        stage: RuntimeLogStage::FetchMusicUrl,
                        refresh_attempted,
                        retry_count: 0,
                        upstream_endpoint: "song/url",
                        ok: false,
                        status_code: Some(mr.status_code),
                        latency_ms: latency,
                        error: Some("empty play url"),
                        error_code: Some(error_code_string(ErrorCode::UpstreamPlayUrlEmpty)),
                    })
                    .await;
                    return Err(AppError::upstream_play_url_empty());
                }

                self.append_log(AppendLogInput {
                    source: &source,
                    account: &account,
                    req: &req,
                    stage: RuntimeLogStage::FetchMusicUrl,
                    refresh_attempted,
                    retry_count: 0,
                    upstream_endpoint: "song/url",
                    ok: true,
                    status_code: Some(mr.status_code),
                    latency_ms: latency,
                    error: None,
                    error_code: None,
                })
                .await;
                Ok(RuntimeMusicUrlResponse { url: mr.url })
            }
            Err(e) => {
                self.log_and_err(
                    &source,
                    &account,
                    &req,
                    LogErrorContext {
                        stage: RuntimeLogStage::FetchMusicUrl,
                        refresh_attempted,
                        retry_count: 0,
                        upstream_endpoint: "song/url",
                        status_code: None,
                        latency_ms: latency,
                    },
                    &e,
                )
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
            track_title: input.req.track_title.clone(),
            artist_name: input.req.artist_name.clone(),
            album_name: input.req.album_name.clone(),
            request_hash: input.req.hash.clone(),
            album_audio_id: input.req.album_audio_id.clone(),
            requested_quality: input.req.quality.clone(),
            upstream_quality: upstream_quality_for(input.stage, &input.req.quality),
            upstream_endpoint: input.upstream_endpoint.into(),
            stage: input.stage,
            refresh_attempted: input.refresh_attempted,
            retry_count: input.retry_count,
            ok: input.ok,
            status_code: input.status_code,
            error_code: input.error_code,
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
        ctx: LogErrorContext<'_>,
        err: &AppError,
    ) -> Result<RuntimeMusicUrlResponse, AppError> {
        let err_code = error_code_string(err.code);
        let err_msg = err.to_string();
        self.append_log(AppendLogInput {
            source,
            account,
            req,
            stage: ctx.stage,
            refresh_attempted: ctx.refresh_attempted,
            retry_count: ctx.retry_count,
            upstream_endpoint: ctx.upstream_endpoint,
            ok: false,
            status_code: ctx.status_code,
            latency_ms: ctx.latency_ms,
            error: Some(&err_msg),
            error_code: Some(err_code),
        })
        .await;
        Err(AppError::upstream_request_failed(err_msg))
    }
}

struct AppendLogInput<'a> {
    source: &'a crate::domain::source::Source,
    account: &'a crate::domain::account::ProviderAccount,
    req: &'a RuntimeMusicUrlRequest,
    stage: RuntimeLogStage,
    refresh_attempted: bool,
    retry_count: u8,
    upstream_endpoint: &'a str,
    ok: bool,
    status_code: Option<u16>,
    latency_ms: u128,
    error: Option<&'a str>,
    error_code: Option<String>,
}

struct LogErrorContext<'a> {
    stage: RuntimeLogStage,
    refresh_attempted: bool,
    retry_count: u8,
    upstream_endpoint: &'a str,
    status_code: Option<u16>,
    latency_ms: u128,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只有「取链」这一步真的带 quality 参数，也只有它该被记录。
    /// ensure_dfid / refresh_login 不带，给它们记一个是发明事实。
    #[test]
    fn upstream_quality_only_for_the_song_url_call() {
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::FetchMusicUrl, "flac24bit").as_deref(),
            Some("high")
        );
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::EnsureDfid, "flac24bit"),
            None
        );
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::RefreshLogin, "flac24bit"),
            None
        );
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::Precheck, "flac"),
            None
        );
    }

    /// 取值域由 routes/runtime.rs 的 parse_music_url_request 限定为这四个。
    /// 这四个映射同时也是前端 tooltip 的依据，改动即用户可见的语义变化。
    #[test]
    fn maps_the_four_whitelisted_qualities() {
        assert_eq!(map_quality("128k"), "128");
        assert_eq!(map_quality("320k"), "320");
        assert_eq!(map_quality("flac"), "flac");
        assert_eq!(map_quality("flac24bit"), "high");
    }

    /// 白名单之外的取值到不了这里。钉住兜底行为，免得它被改动后
    /// 落盘的值与上游实际所用不符却无人发现。
    #[test]
    fn falls_back_to_128_for_unknown_quality() {
        assert_eq!(map_quality("hires"), "128");
        assert_eq!(map_quality(""), "128");
    }
}
