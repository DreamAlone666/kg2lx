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

/// 本行应当记录的上游质量参数 —— 只有描述 `song/url` 那次调用的行才有值。
///
/// 落盘而不是让前端拿 requested_quality 去推：日志行的职责是记录**发生过什么**，
/// 「这次向上游要的是哪个质量」正是发生过的事实（四档里 flac24bit → high 还是真正的
/// 重命名，常常就是失败与成功的分界）。前端再复制一张映射表，等于把服务端逻辑的影子
/// 副本放进界面，服务端一改就安静地开始说错话。
///
/// 判断用 stage 而不是 upstream_endpoint：stage 是枚举、更适合当判据。此前这里给的
/// 依据是「两者严格 1:1，见各处 append_log 调用点逐一对上」，那句已经不准——预检那几处
/// 走 `LogErrorContext::precheck()`，按设计把端点写成空串（那些检查发生在任何上游调用
/// 之前），端点自此不再与 stage 一一对应。结论不受影响：`FetchMusicUrl` 仍是唯一会带
/// `"song/url"` 的阶段，端点为空的行也进不到这个 `matches!` 里。
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
        let Some(source) = self
            .source_repo
            .find_by_runtime_token(runtime_token)
            .await?
        else {
            // 刻意**不**写日志行：`RuntimeLog.source_id` 是必填字段，面板查询也一律
            // 按它过滤，而这里根本查不到音源，只能凭空造一个 id——那会产出任何面板
            // 都查不出来的孤儿行。同理，token 缺失（`verify_runtime` 的
            // `ADMIN_UNAUTHORIZED`）与请求体非法（`parse_music_url_request` 的
            // `INVALID_REQUEST`）都在进入本函数之前就返回了，同样没有可归属的音源，
            // 一律不落日志。
            //
            // 只记 token 的短前缀，绝不整串落日志：它等同于该音源的调用凭据。
            let token_prefix: String = runtime_token.chars().take(8).collect();
            tracing::warn!(
                token_prefix = %token_prefix,
                "运行时请求携带未知 token：无归属音源，不写运行时日志行"
            );
            return Err(AppError::source_not_found());
        };

        if !source.enabled {
            return self
                .log_and_err(
                    &source,
                    &source.account_id,
                    &req,
                    LogErrorContext::precheck(),
                    AppError::source_disabled(),
                )
                .await;
        }

        let Some(mut account) = self.account_repo.find_by_id(&source.account_id).await? else {
            // 此分支没有账号对象，只能用源上记着的 account_id。
            return self
                .log_and_err(
                    &source,
                    &source.account_id,
                    &req,
                    LogErrorContext::precheck(),
                    AppError::account_not_found(),
                )
                .await;
        };

        if matches!(account.status, AccountStatus::Disabled) {
            return self
                .log_and_err(
                    &source,
                    &account.account_id,
                    &req,
                    LogErrorContext::precheck(),
                    AppError::account_disabled(),
                )
                .await;
        }
        if !account.vip_active {
            return self
                .log_and_err(
                    &source,
                    &account.account_id,
                    &req,
                    LogErrorContext::precheck(),
                    AppError::account_not_vip(),
                )
                .await;
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
                            &account.account_id,
                            &req,
                            LogErrorContext {
                                stage: RuntimeLogStage::EnsureDfid,
                                refresh_attempted: false,
                                retry_count: 0,
                                upstream_endpoint: "ensure_dfid",
                                status_code: None,
                                latency_ms: 0,
                            },
                            e,
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
                        &account.account_id,
                        &req,
                        LogErrorContext {
                            stage: RuntimeLogStage::RefreshLogin,
                            refresh_attempted: true,
                            retry_count: 0,
                            upstream_endpoint: "refresh_login",
                            status_code: None,
                            latency_ms: 0,
                        },
                        e,
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
                                    &account.account_id,
                                    &req,
                                    LogErrorContext {
                                        stage: RuntimeLogStage::RefreshLogin,
                                        refresh_attempted: true,
                                        retry_count: 1,
                                        upstream_endpoint: "refresh_login",
                                        status_code: Some(mr.status_code),
                                        latency_ms: start.elapsed().as_millis(),
                                    },
                                    e,
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
                                        account_id: &account.account_id,
                                        req: &req,
                                        stage: RuntimeLogStage::FetchMusicUrl,
                                        refresh_attempted,
                                        retry_count: 1,
                                        upstream_endpoint: "song/url",
                                        ok: false,
                                        status_code: Some(rr.status_code),
                                        latency_ms: retry_latency,
                                        error: Some("empty url after retry"),
                                        error_code: Some(
                                            ErrorCode::UpstreamPlayUrlEmpty.as_str().to_string(),
                                        ),
                                    })
                                    .await;
                                    return Err(AppError::upstream_play_url_empty());
                                }
                                self.append_log(AppendLogInput {
                                    source: &source,
                                    account_id: &account.account_id,
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
                                        &account.account_id,
                                        &req,
                                        LogErrorContext {
                                            stage: RuntimeLogStage::FetchMusicUrl,
                                            refresh_attempted,
                                            retry_count: 1,
                                            upstream_endpoint: "song/url",
                                            status_code: None,
                                            latency_ms: start.elapsed().as_millis(),
                                        },
                                        e,
                                    )
                                    .await;
                            }
                        }
                    }
                    self.append_log(AppendLogInput {
                        source: &source,
                        account_id: &account.account_id,
                        req: &req,
                        stage: RuntimeLogStage::FetchMusicUrl,
                        refresh_attempted,
                        retry_count: 0,
                        upstream_endpoint: "song/url",
                        ok: false,
                        status_code: Some(mr.status_code),
                        latency_ms: latency,
                        error: Some("empty play url"),
                        error_code: Some(ErrorCode::UpstreamPlayUrlEmpty.as_str().to_string()),
                    })
                    .await;
                    return Err(AppError::upstream_play_url_empty());
                }

                self.append_log(AppendLogInput {
                    source: &source,
                    account_id: &account.account_id,
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
                    &account.account_id,
                    &req,
                    LogErrorContext {
                        stage: RuntimeLogStage::FetchMusicUrl,
                        refresh_attempted,
                        retry_count: 0,
                        upstream_endpoint: "song/url",
                        status_code: None,
                        latency_ms: latency,
                    },
                    e,
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
            // 成功时两字段必须一起清：只清 `last_error` 会留下上一次失败的 code，
            // 界面按 code 翻译时会显示一条已经不再成立的错误标签。
            account.last_error = None;
            account.last_error_code = None;
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
            // code 与 message 分开存：code 是界面查中文词典的键，message 是给运维的
            // 原文。因此 message 不再需要 `"CODE: "` 前缀——那正是把 Rust 内部串
            // 送上界面的最后一处来源。
            account.last_error_code = Some(err.code.as_str().to_string());
            account.last_error = Some(err.message.clone());
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
            account_id: input.account_id.to_string(),
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

    /// 记一行失败日志，并原样把错误交还调用方。
    ///
    /// 此前这里收的是 `&AppError` 并把每个上游失败都折成
    /// `AppError::upstream_request_failed(err_msg)`，于是无论失败的真实原因是
    /// 登录失效、歌曲无可用链接还是被限流，客户端拿到的 code 永远只有一个
    /// `UPSTREAM_REQUEST_FAILED`（HTTP 502）——冻结在用户机器上的脚本没有任何办法
    /// 区分该重新登录、该换歌还是该稍后重试。现在按值收下错误、原样抛出，让具体的
    /// code 与状态码（403/404/429/500）真的走到客户端。历史脚本只判断 `>= 400`，
    /// 不受影响。
    ///
    /// 日志行的 `error` 字段写 `err.message`（纯文本）而不是 `err.to_string()`
    /// （`"CODE: message"` 形态）：落盘的那一列本来就另有 `error_code` 字段承载 code，
    /// 在同一行里再拼一遍是重复；而且旧写法会把 Rust 变体名也带进去。
    ///
    /// 收 `account_id: &str` 而非账号对象，是因为预检里的 `account_not_found` 分支
    /// 手上根本没有账号对象，只有源上记着的 id；写成对象就等于那条最该被看见的失败
    /// 反而记不下来。
    ///
    /// 这一行的 `error_code` 是**承重字段**：读路径上 `RuntimeLog::sanitized` 会把
    /// `error` 一律塌成常量（`sanitize_runtime_log_error`），面板能渲染的机器可读信号
    /// 只剩 code。所以预检也必须传一个精确的 `AppError`，而不是随手折一个通用错误码。
    async fn log_and_err(
        &self,
        source: &crate::domain::source::Source,
        account_id: &str,
        req: &RuntimeMusicUrlRequest,
        ctx: LogErrorContext<'_>,
        err: AppError,
    ) -> Result<RuntimeMusicUrlResponse, AppError> {
        let err_code = err.code.as_str().to_string();
        self.append_log(AppendLogInput {
            source,
            account_id,
            req,
            stage: ctx.stage,
            refresh_attempted: ctx.refresh_attempted,
            retry_count: ctx.retry_count,
            upstream_endpoint: ctx.upstream_endpoint,
            ok: false,
            status_code: ctx.status_code,
            latency_ms: ctx.latency_ms,
            error: Some(&err.message),
            error_code: Some(err_code),
        })
        .await;
        Err(err)
    }
}

struct AppendLogInput<'a> {
    source: &'a crate::domain::source::Source,
    account_id: &'a str,
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

impl LogErrorContext<'static> {
    /// 取链之前的准入检查（音源是否启用、账号是否存在 / 是否被禁用 / VIP 是否有效）
    /// 失败时用的上下文。
    ///
    /// **Why:** 这些失败此前**一行日志都不写**就直接返回，于是最常出现在用户身上的
    /// 几种情形——后台把音源关了、账号被禁用、会员到期、账号记录已不存在——在日志
    /// 面板上没有任何痕迹。而面板是 kg2lx 用户唯一能拿到诊断的地方：洛雪音乐会把
    /// 脚本抛出的 `err.message` 丢掉，只显示它自己的「换源失败…」。所以这里用
    /// `stage: Precheck` 把准入失败显式标出来，让面板能区分「没走到取链」与
    /// 「取链失败」。
    ///
    /// **How to apply:** 这些检查都发生在任何上游调用之前，因此没有端点、没有状态码、
    /// 没有重试、也没试图刷新登录——四个字段一律取「未发生」的空值，绝不写成看起来
    /// 像发生过的事实。
    fn precheck() -> Self {
        Self {
            stage: RuntimeLogStage::Precheck,
            refresh_attempted: false,
            retry_count: 0,
            upstream_endpoint: "",
            status_code: None,
            latency_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    use crate::domain::account::ProviderAccount;
    use crate::domain::cookie_store::CookieStore;
    use crate::domain::runtime_log::ListRuntimeLogsQuery;
    use crate::domain::source::Source;

    // ---- 测试替身 ----
    //
    // 预检分支在真正打到上游之前就返回了，所以这些测试不需要任何网络；只需要能
    // 分别给出「查得到 / 查不到」的音源与账号，以及一个能把落盘行扣下来检查的
    // 日志仓库。刻意不用真实 Fs* 仓库：那要读写临时目录，反而把「这一行该不该写」
    // 这个断言埋进文件系统噪声里。

    struct FakeSourceRepo {
        source: Option<Source>,
    }

    #[async_trait]
    impl SourceRepo for FakeSourceRepo {
        async fn find_by_id(&self, _source_id: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn find_by_script_token(&self, _token: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn find_by_runtime_token(&self, token: &str) -> Result<Option<Source>, AppError> {
            Ok(self.source.clone().filter(|s| s.runtime_token == token))
        }

        async fn find_by_account_id(&self, _account_id: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn upsert(&self, _source: &Source) -> Result<(), AppError> {
            Ok(())
        }

        async fn list_all(&self) -> Result<Vec<Source>, AppError> {
            Ok(Vec::new())
        }
    }

    struct FakeAccountRepo {
        account: Option<ProviderAccount>,
    }

    #[async_trait]
    impl AccountRepo for FakeAccountRepo {
        async fn find_by_id(&self, _account_id: &str) -> Result<Option<ProviderAccount>, AppError> {
            Ok(self.account.clone())
        }

        async fn find_by_provider_userid(
            &self,
            _provider: ProviderKind,
            _upstream_userid: &str,
        ) -> Result<Option<ProviderAccount>, AppError> {
            Ok(None)
        }

        async fn upsert(&self, _account: &ProviderAccount) -> Result<(), AppError> {
            Ok(())
        }
    }

    /// 把 `append` 收到的东西原样留一份，测试才能断言「写了什么 / 有没有写」。
    #[derive(Default)]
    struct CapturingLogRepo {
        rows: Mutex<Vec<RuntimeLog>>,
    }

    impl CapturingLogRepo {
        fn rows(&self) -> Vec<RuntimeLog> {
            self.rows.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RuntimeLogRepo for CapturingLogRepo {
        async fn append(&self, log: &RuntimeLog) -> Result<(), AppError> {
            self.rows.lock().unwrap().push(log.clone());
            Ok(())
        }

        async fn list_by_source(
            &self,
            _query: &ListRuntimeLogsQuery,
        ) -> Result<Vec<RuntimeLog>, AppError> {
            Ok(Vec::new())
        }
    }

    fn test_config() -> Arc<Config> {
        Arc::new(Config {
            listen_addr: "127.0.0.1:0".into(),
            public_base_url: "http://127.0.0.1:8787".into(),
            admin_token: "test-admin-token".into(),
            // 预检失败早于任何上游调用，这些测试不会真的发出请求。端口取 1 是
            // 有意为之：万一哪天预检被挪到调用之后，连不上会比「连上了测试环境」
            // 更早、更明显地失败。
            kugou_api_base_url: "http://127.0.0.1:1".into(),
            data_dir: "./data".into(),
            upstream_timeout_ms: 1_000,
            refresh_interval_secs: 6 * 3600,
            web_dist_dir: None,
        })
    }

    fn sample_source(runtime_token: &str, enabled: bool) -> Source {
        Source {
            source_id: "src_test".into(),
            account_id: "acc_test".into(),
            provider: ProviderKind::KugouLite,
            name: "测试音源".into(),
            enabled,
            script_token: "script_test".into(),
            runtime_token: runtime_token.into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn sample_account(vip_active: bool) -> ProviderAccount {
        ProviderAccount {
            account_id: "acc_test".into(),
            provider: ProviderKind::KugouLite,
            upstream_userid: "userid_test".into(),
            display_name: None,
            status: AccountStatus::Active,
            vip_type: 0,
            vip_active,
            cookies: CookieStore::new(),
            last_refresh_at: None,
            last_success_at: None,
            last_error: None,
            last_error_code: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn sample_req() -> RuntimeMusicUrlRequest {
        RuntimeMusicUrlRequest {
            hash: "A0AEFE0227938F366FF4F9D4F0F6CBE5".into(),
            album_audio_id: None,
            quality: "flac".into(),
            track_title: None,
            artist_name: None,
            album_name: None,
        }
    }

    fn build_service(
        source: Option<Source>,
        account: Option<ProviderAccount>,
    ) -> (RuntimeService, Arc<CapturingLogRepo>) {
        let config = test_config();
        let client = Arc::new(KugouLiteClient::new(&config));
        let source_repo: Arc<dyn SourceRepo> = Arc::new(FakeSourceRepo { source });
        let account_repo: Arc<dyn AccountRepo> = Arc::new(FakeAccountRepo { account });
        let log_repo = Arc::new(CapturingLogRepo::default());
        let service =
            RuntimeService::new(config, client, source_repo, account_repo, log_repo.clone());
        (service, log_repo)
    }

    /// 成功值 `RuntimeMusicUrlResponse` 没有 `Debug`，用不了 `unwrap_err`。
    fn expect_err(result: Result<RuntimeMusicUrlResponse, AppError>) -> AppError {
        match result {
            Ok(_) => panic!("预期取链失败，实际返回了播放地址"),
            Err(err) => err,
        }
    }

    /// 音源被停用是最常见、也最需要证据的准入失败：用户只会在客户端看到
    /// 「换源失败…」，此刻后台的日志面板必须能说出「音源被停用了」。
    ///
    /// 同时钉住客户端可见行为**没有**变化——返回的仍是 `SourceDisabled`，
    /// 只是多写了一行日志。
    #[tokio::test]
    async fn disabled_source_writes_a_precheck_row() {
        let source = sample_source("tok_disabled", false);
        let (service, log_repo) = build_service(Some(source.clone()), None);

        let err = expect_err(service.fetch_music_url("tok_disabled", sample_req()).await);
        assert_eq!(
            err.code,
            ErrorCode::SourceDisabled,
            "客户端看到的 code 不能因为加日志而改变"
        );

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1, "预检失败必须恰好写一行");
        let row = &rows[0];
        assert_eq!(row.stage, RuntimeLogStage::Precheck);
        assert_eq!(row.error_code.as_deref(), Some("SOURCE_DISABLED"));
        assert_eq!(
            row.error_code.as_deref(),
            Some(ErrorCode::SourceDisabled.as_str())
        );
        assert!(!row.ok);
        assert_eq!(row.source_id, source.source_id);
        assert_eq!(row.account_id, source.account_id);
    }

    /// 未知 runtime token 刻意不写日志行。
    ///
    /// `RuntimeLog.source_id` 是必填字段、面板查询也按它过滤，而这里查不到音源，
    /// 写下的行必须凭空造一个 source_id，其结果是一个任何面板都查不出来的孤儿行。
    /// 这条把「不写」本身钉住，免得日后有人「顺手补齐」出一个查不到的日志。
    #[tokio::test]
    async fn unknown_source_token_writes_no_row() {
        let (service, log_repo) = build_service(None, None);

        let err = expect_err(service.fetch_music_url("tok_unknown", sample_req()).await);
        assert_eq!(err.code, ErrorCode::SourceNotFound);
        assert!(
            log_repo.rows().is_empty(),
            "未知 token 无法归属到任何音源，不得写日志行"
        );
    }

    /// 此分支手上没有账号对象，只能用源上记着的 `source.account_id`。
    /// 这也是 `append_log` / `log_and_err` 从收账号对象改为收 `account_id` 的原因。
    #[tokio::test]
    async fn account_not_found_precheck_row_uses_source_account_id() {
        let source = sample_source("tok_no_account", true);
        let (service, log_repo) = build_service(Some(source.clone()), None);

        let err = expect_err(
            service
                .fetch_music_url("tok_no_account", sample_req())
                .await,
        );
        assert_eq!(err.code, ErrorCode::AccountNotFound);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stage, RuntimeLogStage::Precheck);
        assert_eq!(rows[0].error_code.as_deref(), Some("ACCOUNT_NOT_FOUND"));
        assert_eq!(rows[0].source_id, source.source_id);
        assert_eq!(rows[0].account_id, source.account_id);
    }

    /// VIP 到期的行必须带账号 id，且错误码精确到 `ACCOUNT_NOT_VIP`——面板读到的
    /// `error` 文本会被 `sanitize_runtime_log_error` 塌成常量，code 是唯一信号。
    #[tokio::test]
    async fn account_not_vip_precheck_row_uses_account_id() {
        let source = sample_source("tok_not_vip", true);
        let account = sample_account(false);
        let (service, log_repo) = build_service(Some(source.clone()), Some(account.clone()));

        let err = expect_err(service.fetch_music_url("tok_not_vip", sample_req()).await);
        assert_eq!(err.code, ErrorCode::AccountNotVip);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stage, RuntimeLogStage::Precheck);
        assert_eq!(rows[0].error_code.as_deref(), Some("ACCOUNT_NOT_VIP"));
        assert_eq!(rows[0].account_id, account.account_id);
        assert_eq!(rows[0].source_id, source.source_id);
    }

    /// 账号被禁用同样落在预检阶段，且用的是账号自身的 id。
    #[tokio::test]
    async fn disabled_account_writes_a_precheck_row() {
        let source = sample_source("tok_acc_disabled", true);
        let mut account = sample_account(true);
        account.status = AccountStatus::Disabled;
        let (service, log_repo) = build_service(Some(source.clone()), Some(account.clone()));

        let err = expect_err(
            service
                .fetch_music_url("tok_acc_disabled", sample_req())
                .await,
        );
        assert_eq!(err.code, ErrorCode::AccountDisabled);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stage, RuntimeLogStage::Precheck);
        assert_eq!(rows[0].error_code.as_deref(), Some("ACCOUNT_DISABLED"));
        assert_eq!(rows[0].account_id, account.account_id);
    }

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
