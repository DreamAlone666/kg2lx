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
use crate::services::kugou_lite_client::{
    KugouLiteClient, MusicUrlFetcher, MusicUrlRequest, UpstreamObservation, map_quality,
    next_lower_quality, upstream_refused_quality,
};

#[derive(serde::Deserialize)]
pub struct RuntimeMusicUrlRequest {
    pub hash: String,
    pub album_audio_id: Option<String>,
    pub quality: String,
    pub track_title: Option<String>,
    pub artist_name: Option<String>,
    pub album_name: Option<String>,
    /// 客户端上报的 LX 档位元数据（宿主扁平 `musicInfo.types`，即 `meta.qualitys`）。
    ///
    /// **原样落盘，不参与任何判断。** 它说的是「LX 认为这首歌有哪些档位」，而那是
    /// **目录级**元数据、与账号权益无关——那些档位很可能一直都在，只是这个账号拿不到。
    /// 因此它不能用来回答「该不该降级」；它能回答的是「客户端以为有什么 vs 上游实际
    /// 给了什么」，那是个事后诊断问题。
    ///
    /// 类型是 `Value` 而不是某个具体结构，是刻意的：这是记录，不是契约。给它定 shape
    /// 只会让上游一改就整行解析失败，而解析失败在这里等于**整条取链请求被拒**。
    /// `#[serde(default)]` 让 `0.1.2` 及以前的脚本（只发三个字段）继续可用。
    #[serde(default)]
    pub types: Option<serde_json::Value>,
}

#[derive(serde::Serialize)]
pub struct RuntimeMusicUrlResponse {
    pub url: String,
}

pub struct RuntimeService {
    config: Arc<Config>,
    /// 登录 / DFID / VIP 刷新仍走具体类型：这些调用不需要替身，而 trait 一旦把它们
    /// 也收进去就会长成一个大接口。
    client: Arc<KugouLiteClient>,
    /// 取链走可注入的接口——降级循环必须能在无网络下被测试。
    ///
    /// 注意与 `client` 是**两个字段、同一个对象**（生产环境如此，见 `new`）：分开持有
    /// 而不是从 `client` 现转，是因为 trait object 的转换需要一次性发生在构造点上，
    /// 每调一次链就转一次不但没有意义，还会让「这个依赖可替换」这件事变得看不出来。
    fetcher: Arc<dyn MusicUrlFetcher>,
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
/// 入参是 `attempted_quality`（**本次**真正要的那一档），不是 `req.quality`（用户当初
/// 要的）。降级开启后，一次入站请求会沿阶梯逐档调用上游、每档各写一行；若这里继续读
/// 用户请求的档位，阶梯上每一行都会记成同一个值，降级恰恰在最该看得见的那一列上消失。
/// 关着降级时两者恒等，因此这次改动对既有行零影响。
///
/// 判断用 stage 而不是 upstream_endpoint：stage 是枚举、更适合当判据。此前这里给的
/// 依据是「两者严格 1:1，见各处 append_log 调用点逐一对上」，那句已经不准——预检那几处
/// 走 `LogErrorContext::precheck()`，按设计把端点写成空串（那些检查发生在任何上游调用
/// 之前），端点自此不再与 stage 一一对应。结论不受影响：`FetchMusicUrl` 仍是唯一会带
/// `"song/url"` 的阶段，端点为空的行也进不到这个 `matches!` 里。
/// `ensure_dfid` / `refresh_login` 这两次调用压根不带质量参数，给它们记一个
/// 「本来会要的质量」就是发明一个没发生过的事实。
fn upstream_quality_for(stage: RuntimeLogStage, attempted_quality: Option<&str>) -> Option<String> {
    match stage {
        RuntimeLogStage::FetchMusicUrl => {
            attempted_quality.map(|quality| map_quality(quality).to_string())
        }
        _ => None,
    }
}

impl RuntimeService {
    /// `client` 与 `fetcher` 在生产环境里是**同一个对象传两次**（`app_state.rs` 传的是
    /// 同一份 `Arc<KugouLiteClient>`，第二个参数位置上发生 `Arc<KugouLiteClient>` →
    /// `Arc<dyn MusicUrlFetcher>` 的强制转换）。看起来冗余，但这是刻意的：
    ///
    /// - 只留 `fetcher` 就只能表达「取链」，而 `ensure_dfid` / `refresh_login` /
    ///   `fetch_vip_status` 也需要这个客户端，于是 trait 会被撑成一个大接口，每个
    ///   测试替身都得为实现与降级无关的方法付出成本；
    /// - 只留 `client` 就无法在无网络下测试降级循环，而那正是这段逻辑唯一的验收方式。
    ///
    /// 测试则传「一个真的 `KugouLiteClient`（用不到，但构造它没有网络副作用）+
    /// 一个假 fetcher」。
    pub fn new(
        config: Arc<Config>,
        client: Arc<KugouLiteClient>,
        fetcher: Arc<dyn MusicUrlFetcher>,
        source_repo: Arc<dyn SourceRepo>,
        account_repo: Arc<dyn AccountRepo>,
        log_repo: Arc<dyn RuntimeLogRepo>,
    ) -> Self {
        Self {
            config,
            client,
            fetcher,
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

        // 一次入站 LX 请求一个 id，**查到音源之后立刻生成**——这样预检那四条早退路径
        // 写出的行也带上同一个 id，一次请求的全部落盘行（可能跨阶段、跨档位）因此能被
        // 一条 id 串起来。
        //
        // 它存在的唯一理由是把「同一次请求的多行」与「两次独立请求」分开，而这两件事
        // 都会发生、且在日志里长得一模一样：洛雪音乐对任何非「用户取消」的失败会把整条
        // 取链流程无条件重跑一遍（同一首歌的失败于是成对出现），而服务端的降级也会为一次
        // 请求写出多行。只看时间戳与 hash 分不开，`retry_count` 也分不开（它只记「刷新
        // 登录之后重试同一档」）。
        let client_request_id = format!("req_{}", uuid::Uuid::new_v4());

        if !source.enabled {
            return self
                .log_and_err(
                    &source,
                    &source.account_id,
                    &req,
                    LogErrorContext::precheck(&client_request_id),
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
                    LogErrorContext::precheck(&client_request_id),
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
                    LogErrorContext::precheck(&client_request_id),
                    AppError::account_disabled(),
                )
                .await;
        }
        // 这里曾是 `if !account.vip_active` 的准入闸，连同它写的
        // `ACCOUNT_NOT_VIP` 预检行一起删除了：**VIP 不是取链的前提**。账号有没有
        // 会员只决定上游肯给到哪一档音质（`song/url` 会以 `status:2` 明确拒绝），
        // 而免费高音质的歌、以及任何免费歌曲，没有会员照样能取到链——旧闸把这类
        // 请求在打到上游之前就挡掉了，用户看到的是一个与真实原因无关的错误。
        // 权益是否在期改由账号的 `entitlements` 承载（只作展示）。

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
                                client_request_id: &client_request_id,
                                stage: RuntimeLogStage::EnsureDfid,
                                refresh_attempted: false,
                                retry_count: 0,
                                upstream_endpoint: "ensure_dfid",
                                status_code: None,
                                latency_ms: 0,
                                attempted_quality: None,
                                quality_fallback_index: 0,
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
                            client_request_id: &client_request_id,
                            stage: RuntimeLogStage::RefreshLogin,
                            refresh_attempted: true,
                            retry_count: 0,
                            upstream_endpoint: "refresh_login",
                            status_code: None,
                            latency_ms: 0,
                            // 刷新登录那次调用不带质量参数，这一行也就没要过任何档位。
                            attempted_quality: None,
                            quality_fallback_index: 0,
                        },
                        e,
                    )
                    .await;
            }
        }

        // ---- 音质阶梯降级循环 ----
        //
        // 循环只做一件事：当上游**明确拒绝**了这一档（`status:2`）且音源开关打开时，
        // 沿 LX 阶梯向下改请求更低档位，直到拿到 url 或者降到底。
        //
        // 阶梯位置放在 `quality_fallback_index`，**不用 `retry_count`**：后者已经在表达
        // 另一件事（「刷新登录之后重试同一档」）。让两列各说各的话，比一列同时编码两种
        // 含义要好——否则读到 index=1 的人分不清它是「降了一档」还是「刷新后重试了一次」，
        // 而那正是排查时要问的第一个问题。
        let mut quality = req.quality.clone();
        let mut quality_fallback_index: u8 = 0;

        loop {
            // 逐档重建请求。**不要**把它提到循环外：`quality` 是循环里唯一变化的输入，
            // 提到外面就等于每一档都在向上游要同一个档位，阶梯只剩日志好看。
            let upstream_req = MusicUrlRequest {
                hash: req.hash.clone(),
                album_audio_id: req.album_audio_id.clone(),
                quality: quality.clone(),
            };

            let start = Instant::now();
            let result = self
                .fetcher
                .fetch_music_url(&account.cookies, &upstream_req)
                .await;
            let latency = start.elapsed().as_millis();

            let mr = match result {
                Ok(mr) => mr,
                Err(e) => {
                    // 传输层失败（超时 / 连接被拒 / 5xx）**不降级**：它说的是「这次调用
                    // 没成功」，不是「这一档不可用」。据此降档只会在上游抖动或限流时把
                    // 请求数乘四，还会把 429 的「稍后重试」语义换成「换个档位试试」。
                    // 错误原样抛出，可重试性由 `is_retryable` 保持原样判定。
                    return self
                        .log_and_err(
                            &source,
                            &account.account_id,
                            &req,
                            LogErrorContext {
                                client_request_id: &client_request_id,
                                stage: RuntimeLogStage::FetchMusicUrl,
                                refresh_attempted,
                                retry_count: 0,
                                upstream_endpoint: "song/url",
                                status_code: None,
                                latency_ms: latency,
                                attempted_quality: Some(&quality),
                                quality_fallback_index,
                            },
                            e,
                        )
                        .await;
                }
            };

            account.cookies = mr.cookies;
            self.account_repo.upsert(&account).await?;

            if !mr.url.is_empty() {
                self.append_log(AppendLogInput {
                    source: &source,
                    account_id: &account.account_id,
                    req: &req,
                    client_request_id: &client_request_id,
                    stage: RuntimeLogStage::FetchMusicUrl,
                    refresh_attempted,
                    retry_count: 0,
                    upstream_endpoint: "song/url",
                    attempted_quality: Some(&quality),
                    quality_fallback_index,
                    observation: Some(&mr.observation),
                    ok: true,
                    status_code: Some(mr.status_code),
                    latency_ms: latency,
                    error: None,
                    error_code: None,
                })
                .await;
                return Ok(RuntimeMusicUrlResponse { url: mr.url });
            }

            // ---- 顺序是承重的：拒绝判定必须在 auth 判定之前 ----
            //
            // `is_auth_failure` 是一组子串启发式（`token` / `dfid` / `需要验证` /
            // `"code":301`），一个 `status:2` 的响应体完全可能命中其中任何一条。若把
            // auth 判定放在前面，这一档的拒绝就会被当成「登录失效」，于是代码去刷新
            // 登录而不是降级——而刷新之后重试的仍是同一个档位，会再拿到同一个
            // `status:2`。降级这个功能恰恰在最需要它的场景里静默失效。
            //
            // 这里**刻意不**写 `status == 2 && !auth_failed`：那会让整个特性取决于一个
            // 模糊子串启发式**没有**命中，等于把降级交给运气。实测证据支持去掉它——
            // 本地实测的那批真实拒绝行全部是
            // `retry_count: 0` 且 `status_code: 200`，而 auth 路径（刷新登录 + 同一档
            // 重试）必然写出 `retry_count: 1` 的行；反推可知那些拒绝里 `auth_failed`
            // 恒为 false。因此去掉 `!auth_failed` 在真实数据上零行为差异，只是严格更稳。
            //
            // 为什么**只有** `status == 2` 才降级：`status:0` 是「资源不存在 / 参数错」
            // （与音质无关，换一档照样没有），HTTP 429、超时、传输错误、`status` 缺失
            // 也都不是对音质的表态。对它们降档只会把一次限流放大四倍。
            if upstream_refused_quality(mr.observation.status) {
                if source.quality_fallback
                    && let Some(lower) = next_lower_quality(&quality)
                {
                    self.append_log(AppendLogInput {
                        source: &source,
                        account_id: &account.account_id,
                        req: &req,
                        client_request_id: &client_request_id,
                        stage: RuntimeLogStage::FetchMusicUrl,
                        refresh_attempted,
                        // 阶梯位置只由 quality_fallback_index 承载，retry_count 保持
                        // 「刷新登录后重试同一档」的原义，恒为 0。
                        retry_count: 0,
                        upstream_endpoint: "song/url",
                        attempted_quality: Some(&quality),
                        quality_fallback_index,
                        observation: Some(&mr.observation),
                        ok: false,
                        status_code: Some(mr.status_code),
                        latency_ms: latency,
                        error: Some("upstream refused requested quality"),
                        // 中间档复用 `UPSTREAM_QUALITY_REFUSED`：这个码描述的是**上游做
                        // 了什么**（明确拒绝了这一档），不是我们的策略。我们的策略（这是
                        // 同一次请求的第几档）由 index + client_request_id 承载。只有降
                        // 到底仍被拒才用 `EXHAUSTED`。
                        error_code: Some(ErrorCode::UpstreamQualityRefused.as_str().to_string()),
                    })
                    .await;
                    quality = lower.to_string();
                    quality_fallback_index += 1;
                    continue;
                }

                let err = if quality_fallback_index > 0 {
                    AppError::upstream_quality_exhausted()
                } else {
                    AppError::upstream_quality_refused()
                };
                let err_code = err.code.as_str().to_string();
                // 终局行用 `append_log` 而不是 `log_and_err`：它带着这次拒绝的观测，而
                // `LogErrorContext` 没有 `observation` 字段（那个结构体描述的是「根本没
                // 拿到响应体」的错误路径）。用后者就等于把最该留下的证据丢掉。
                self.append_log(AppendLogInput {
                    source: &source,
                    account_id: &account.account_id,
                    req: &req,
                    client_request_id: &client_request_id,
                    stage: RuntimeLogStage::FetchMusicUrl,
                    refresh_attempted,
                    retry_count: 0,
                    upstream_endpoint: "song/url",
                    attempted_quality: Some(&quality),
                    quality_fallback_index,
                    observation: Some(&mr.observation),
                    ok: false,
                    status_code: Some(mr.status_code),
                    latency_ms: latency,
                    error: Some(&err.message),
                    error_code: Some(err_code),
                })
                .await;
                return Err(err);
            }

            if mr.auth_failed {
                refresh_attempted = true;
                if let Err(e) = self.do_refresh(&mut account).await {
                    return self
                        .log_and_err(
                            &source,
                            &account.account_id,
                            &req,
                            LogErrorContext {
                                client_request_id: &client_request_id,
                                stage: RuntimeLogStage::RefreshLogin,
                                refresh_attempted: true,
                                retry_count: 1,
                                upstream_endpoint: "refresh_login",
                                status_code: Some(mr.status_code),
                                latency_ms: start.elapsed().as_millis(),
                                // 同上：这一行描述的是刷新登录，不是取链。
                                attempted_quality: None,
                                quality_fallback_index: 0,
                            },
                            e,
                        )
                        .await;
                }

                let retry = self
                    .fetcher
                    .fetch_music_url(&account.cookies, &upstream_req)
                    .await;
                let retry_latency = start.elapsed().as_millis();
                match retry {
                    Ok(rr) => {
                        account.cookies = rr.cookies;
                        self.account_repo.upsert(&account).await?;
                        if rr.url.is_empty() {
                            // 与主循环同一条顺序规则：先问「上游是不是明确拒绝了这一档」，
                            // 再谈「没拿到地址」。区别只在于这里**不降级**——本分支已经是
                            // 「刷新登录后对同一档的重试」，阶梯的推进由主循环负责；而这条
                            // 路径进不来测试（`do_refresh` 走真实客户端，见 build_loop_service），
                            // 不值得再养一段没人能验收的循环代码。
                            //
                            // 但错误码必须说对：`UPSTREAM_PLAY_URL_EMPTY` 本轮已收窄为
                            // 「没拿到 url、且上游没有表达拒绝」，用它描述一次明确的拒绝，
                            // 会让这一行的 `upstream_status=2` 与码自身的定义相反。这里也
                            // 不用 `EXHAUSTED`——那是「沿阶梯降到底」，而这一档未必是底。
                            let refused = upstream_refused_quality(rr.observation.status);
                            let (error_message, error_code, err) = if refused {
                                (
                                    "upstream refused requested quality",
                                    ErrorCode::UpstreamQualityRefused,
                                    AppError::upstream_quality_refused(),
                                )
                            } else {
                                (
                                    "empty url after retry",
                                    ErrorCode::UpstreamPlayUrlEmpty,
                                    AppError::upstream_play_url_empty(),
                                )
                            };
                            self.append_log(AppendLogInput {
                                source: &source,
                                account_id: &account.account_id,
                                req: &req,
                                client_request_id: &client_request_id,
                                stage: RuntimeLogStage::FetchMusicUrl,
                                refresh_attempted,
                                retry_count: 1,
                                upstream_endpoint: "song/url",
                                // 记的是**重试的那一档**（循环当前的 `quality`），不是
                                // `req.quality`：降级之后才走到这条 auth 重试路径时两者
                                // 不同，写后者会让这一行谎报自己请求过的档位。
                                attempted_quality: Some(&quality),
                                quality_fallback_index,
                                observation: Some(&rr.observation),
                                ok: false,
                                status_code: Some(rr.status_code),
                                latency_ms: retry_latency,
                                error: Some(error_message),
                                error_code: Some(error_code.as_str().to_string()),
                            })
                            .await;
                            return Err(err);
                        }
                        self.append_log(AppendLogInput {
                            source: &source,
                            account_id: &account.account_id,
                            req: &req,
                            client_request_id: &client_request_id,
                            stage: RuntimeLogStage::FetchMusicUrl,
                            refresh_attempted,
                            retry_count: 1,
                            upstream_endpoint: "song/url",
                            attempted_quality: Some(&quality),
                            quality_fallback_index,
                            observation: Some(&rr.observation),
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
                                    client_request_id: &client_request_id,
                                    stage: RuntimeLogStage::FetchMusicUrl,
                                    refresh_attempted,
                                    retry_count: 1,
                                    upstream_endpoint: "song/url",
                                    status_code: None,
                                    latency_ms: start.elapsed().as_millis(),
                                    attempted_quality: Some(&quality),
                                    quality_fallback_index,
                                },
                                e,
                            )
                            .await;
                    }
                }
            }

            // 走到这里：拿到了响应、没有 url、上游没表达拒绝、也不是登录失效。
            // 这一档仍然是「没拿到地址」，且没有理由换档再试。
            self.append_log(AppendLogInput {
                source: &source,
                account_id: &account.account_id,
                req: &req,
                client_request_id: &client_request_id,
                stage: RuntimeLogStage::FetchMusicUrl,
                refresh_attempted,
                retry_count: 0,
                upstream_endpoint: "song/url",
                attempted_quality: Some(&quality),
                quality_fallback_index,
                observation: Some(&mr.observation),
                ok: false,
                status_code: Some(mr.status_code),
                latency_ms: latency,
                error: Some("empty play url"),
                error_code: Some(ErrorCode::UpstreamPlayUrlEmpty.as_str().to_string()),
            })
            .await;
            return Err(AppError::upstream_play_url_empty());
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
            account.entitlements = vip.entitlements;

            let now = account::now_ts();
            account.last_refresh_at = Some(now);
            account.last_success_at = Some(now);
            // 成功时两字段必须一起清：只清 `last_error` 会留下上一次失败的 code，
            // 界面按 code 翻译时会显示一条已经不再成立的错误标签。
            account.last_error = None;
            account.last_error_code = None;
            // 账号状态只由**登录态**决定：能走到这里就说明刷新成功了。权益在不在期
            // 改由 `entitlements` 表达——那是一件独立的、账号正常与否之外的事。
            account.status = AccountStatus::Active;
            account.updated_at = now;
            self.account_repo.upsert(account).await?;

            Ok(())
        }
        .await;

        // 刷新里的任何失败都记进账号。此前这里额外放行一条错误码
        // （`ACCOUNT_NOT_VIP`）——「没有会员」不是刷新失败，不该把账号标成
        // `login_failed`。现在整条准入闸都不存在了，那个例外也就随之消失：这里
        // 剩下的每一条错误都真的是刷新本身失败。
        if let Err(err) = &result {
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
            attempted_quality: input.attempted_quality.map(str::to_string),
            quality_fallback_index: input.quality_fallback_index,
            reported_types: input.req.types.clone(),
            client_request_id: Some(input.client_request_id.to_string()),
            upstream_quality: upstream_quality_for(input.stage, input.attempted_quality),
            // 观测只在真的拿到响应时才有值。注意这里全部映射成 `Option`，绝不折成
            // `unwrap_or(0)`：「上游说 0」与「上游什么都没说」是两件事，而界面正是
            // 靠 `upstream_status !== null` 区分「这一行有没有上游观测」。
            upstream_status: input.observation.and_then(|o| o.status),
            upstream_priv_status: input.observation.and_then(|o| o.priv_status),
            upstream_fail_process: input.observation.and_then(|o| o.fail_process.clone()),
            upstream_ext_name: input.observation.and_then(|o| o.ext_name.clone()),
            upstream_file_size: input.observation.and_then(|o| o.file_size),
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
            client_request_id: ctx.client_request_id,
            stage: ctx.stage,
            refresh_attempted: ctx.refresh_attempted,
            retry_count: ctx.retry_count,
            upstream_endpoint: ctx.upstream_endpoint,
            attempted_quality: ctx.attempted_quality,
            quality_fallback_index: ctx.quality_fallback_index,
            // 错误路径按定义没有可用的响应观测：`Err` 分支根本没拿到响应体，而拿到
            // 响应却为空 url 的几种情形都由调用方直接用 `append_log` 落盘（它们才
            // 有观测）。因此这里恒为 None，不是漏填。
            observation: None,
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
    /// 一次入站 LX 请求一个 id，由 `fetch_music_url` 在查到音源后生成。
    ///
    /// 必须从外面传进来而不是在 `append_log` 里现生成：一次请求可能写出多行（降级时
    /// 逐档各一行），这些行要共享同一个 id 才能被认成「同一次请求的第 N 档」。
    client_request_id: &'a str,
    stage: RuntimeLogStage,
    refresh_attempted: bool,
    retry_count: u8,
    upstream_endpoint: &'a str,
    /// **本次**向上游要的 LX 档位。未走到取链的行传 `None`。
    attempted_quality: Option<&'a str>,
    /// 已经沿阶梯降了几档；未发生降级时为 0。
    quality_fallback_index: u8,
    /// 本次上游响应的观测，仅在真的拿到响应时才有；未走到取链或传输失败时为 `None`。
    observation: Option<&'a UpstreamObservation>,
    ok: bool,
    status_code: Option<u16>,
    latency_ms: u128,
    error: Option<&'a str>,
    error_code: Option<String>,
}

struct LogErrorContext<'a> {
    client_request_id: &'a str,
    stage: RuntimeLogStage,
    refresh_attempted: bool,
    retry_count: u8,
    upstream_endpoint: &'a str,
    status_code: Option<u16>,
    latency_ms: u128,
    /// 见 `AppendLogInput::attempted_quality`。错误路径同样要记下来：一次传输错误发生在
    /// 降级之后的第二档上时，「哪一档炸的」是排查的第一问题。
    attempted_quality: Option<&'a str>,
    /// 见 `AppendLogInput::quality_fallback_index`。
    quality_fallback_index: u8,
}

impl<'a> LogErrorContext<'a> {
    /// 取链之前的准入检查（音源是否启用、账号是否存在 / 是否被禁用）失败时用的
    /// 上下文。
    ///
    /// **Why:** 这些失败此前**一行日志都不写**就直接返回，于是最常出现在用户身上的
    /// 几种情形——后台把音源关了、账号被禁用、账号记录已不存在——在日志面板上
    /// 没有任何痕迹。而面板是 kg2lx 用户唯一能拿到诊断的地方：洛雪音乐会把脚本
    /// 抛出的 `err.message` 丢掉，只显示它自己的「换源失败…」。所以这里用
    /// `stage: Precheck` 把准入失败显式标出来，让面板能区分「没走到取链」与
    /// 「取链失败」。
    ///
    /// 会员权益**不在**这张表里：它不是取链的准入条件，因此没有它对应的预检行。
    ///
    /// **How to apply:** 这些检查都发生在任何上游调用之前，因此没有端点、没有状态码、
    /// 没有重试、也没试图刷新登录——四个字段一律取「未发生」的空值，绝不写成看起来
    /// 像发生过的事实。同理，也没要过任何档位（`attempted_quality: None`）——因此这些行
    /// 的 `upstream_status` 等观测字段必须全是 `None`，**不能**是 `Some(0)`：「上游什么都没
    /// 说」与「上游说了 0（资源不存在）」含义相反，混同会让整页预检行都显示出一个
    /// 根本不存在的上游观测。
    ///
    /// `client_request_id` 是唯一照常携带的：它标识的是「这次入站请求」，与有没有走到
    /// 上游无关。
    fn precheck(client_request_id: &'a str) -> Self {
        Self {
            client_request_id,
            stage: RuntimeLogStage::Precheck,
            refresh_attempted: false,
            retry_count: 0,
            upstream_endpoint: "",
            status_code: None,
            latency_ms: 0,
            attempted_quality: None,
            quality_fallback_index: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use crate::domain::account::ProviderAccount;
    use crate::domain::cookie_store::CookieStore;
    use crate::domain::runtime_log::ListRuntimeLogsQuery;
    use crate::domain::source::Source;
    // 只在测试里出现（生产代码只经 trait 使用这个类型），因此导入放在测试模块内，
    // 免得非测试构建报 unused import。
    use crate::services::kugou_lite_client::MusicUrlResult;

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
            quality_fallback: false,
        }
    }

    /// 一个**一条权益都没有**的账号（`entitlements` 为空），账号状态正常。
    ///
    /// 「没有权益」不再是账号上的一列聚合值（`vip_active` / `vip_type` 已删除），
    /// 它现在的表述就是这里的空权益列表：权益是并列的增量信息，一条都没有就是空数组。
    fn sample_account() -> ProviderAccount {
        ProviderAccount {
            account_id: "acc_test".into(),
            provider: ProviderKind::KugouLite,
            upstream_userid: "userid_test".into(),
            display_name: None,
            status: AccountStatus::Active,
            entitlements: Vec::new(),
            cookies: CookieStore::new(),
            last_refresh_at: None,
            last_success_at: None,
            last_error: None,
            last_error_code: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    /// 一个「已经越过所有网络前置步骤」的账号。
    ///
    /// **Why:** `sample_account` 造出的账号 `cookies` 是空的、`last_refresh_at` 是
    /// `None`，于是主路径会先调 `ensure_dfid`、再调 `do_refresh`——两次都是**真实的**
    /// 网络调用（打向 `test_config` 里的 `http://127.0.0.1:1`），且都在取链**之前**。
    /// 降级循环的测试必须能在无网络下跑到取链那一步，所以这里把那两个前置步骤都置成
    /// 「已满足」，两个看起来奇怪的取值正为此：
    ///
    /// - `dfid` cookie 有值 → `is_empty("dfid")` 为假 → 跳过 `ensure_dfid`；
    /// - `last_refresh_at = now` → `needs_refresh` 为假 → 跳过 `do_refresh`。
    ///
    /// 这两个值一旦被「顺手清理掉」，测试就会变成对着端口 1 发请求，失败得又远又不明显。
    fn account_ready_for_fetch() -> ProviderAccount {
        let mut account = sample_account();
        account.cookies.insert("dfid", "dfid_test");
        account.last_refresh_at = Some(account::now_ts());
        account
    }

    fn sample_req() -> RuntimeMusicUrlRequest {
        RuntimeMusicUrlRequest {
            hash: "A0AEFE0227938F366FF4F9D4F0F6CBE5".into(),
            album_audio_id: None,
            quality: "flac".into(),
            track_title: None,
            artist_name: None,
            album_name: None,
            // 老脚本（0.1.2 及以前）不发这个字段；`#[serde(default)]` 让它等价于此。
            types: None,
        }
    }

    /// 一旦被调用就 panic 的取链替身，给预检测试用。
    ///
    /// **Why:** 那些测试断言的都是「预检失败早于任何上游调用」。用真的
    /// `KugouLiteClient`（指向 `test_config` 的端口 1）也能失败，但那种失败走的是
    /// 「连接被拒」的错误分支——测试可能因为别的原因依然是绿的。直接 panic 把这条
    /// 不变量从断言升级成强制，理由与 `test_config` 里端口取 1 的理由同源。
    struct NeverCalledFetcher;

    #[async_trait]
    impl MusicUrlFetcher for NeverCalledFetcher {
        async fn fetch_music_url(
            &self,
            _cookies: &CookieStore,
            req: &MusicUrlRequest,
        ) -> Result<MusicUrlResult, AppError> {
            panic!("预检路径不得调用取链，实际请求了档位 {}", req.quality);
        }
    }

    /// 上游「明确拒绝这一档」的形状：`status:2` + `priv_status:0` + 无 url。
    /// 实测里它还带 `fail_process`，一并造出来，好让落盘断言顺路覆盖它。
    fn refused_result() -> MusicUrlResult {
        MusicUrlResult {
            url: String::new(),
            cookies: CookieStore::new(),
            status_code: 200,
            auth_failed: false,
            observation: UpstreamObservation {
                status: Some(2),
                priv_status: Some(0),
                fail_process: Some(vec!["pkg".into(), "buy".into()]),
                ext_name: None,
                file_size: None,
            },
        }
    }

    /// 成功形状。观测里带上 `ext_name` / `file_size`——它们正是「实际拿到的是不是
    /// 所要档位」的自证材料（`quality=high` 会静默回落到 128k mp3 却仍是 `status:1`）。
    fn success_result(url: &str) -> MusicUrlResult {
        MusicUrlResult {
            url: url.into(),
            cookies: CookieStore::new(),
            status_code: 200,
            auth_failed: false,
            observation: UpstreamObservation {
                status: Some(1),
                priv_status: Some(1),
                fail_process: None,
                ext_name: Some("mp3".into()),
                file_size: Some(9_412_859),
            },
        }
    }

    /// `status:0` 的形状：资源不存在 / 参数错（实测带 `error_code`），无 url。
    /// 与拒绝形状**完全不同**——它说的不是「这一档不行」，而是「这首歌根本没有」。
    fn not_found_result() -> MusicUrlResult {
        MusicUrlResult {
            url: String::new(),
            cookies: CookieStore::new(),
            status_code: 200,
            auth_failed: false,
            observation: UpstreamObservation {
                status: Some(0),
                priv_status: None,
                fail_process: None,
                ext_name: None,
                file_size: None,
            },
        }
    }

    /// 预先排好队的取链结果 + 调用记录。
    ///
    /// **队列耗尽即 panic**，不返回默认值也不返回 `Err`：这正是把「这次请求恰好调了
    /// N 次上游」从愿望变成断言的地方。若改成耗尽就返回错误，一次本不该发生的第四次
    /// 调用会安静地落进错误分支，测试照样是绿的。
    ///
    /// 同时记下每次请求的 `quality`：只数次数证明不了阶梯是**逐档**走的，而且次数对得
    /// 上也可能是因为同一档被请求了多次——「请求了哪几档」才是验收点。
    struct FakeFetcher {
        queue: Mutex<VecDeque<Result<MusicUrlResult, AppError>>>,
        calls: Mutex<Vec<String>>,
    }

    impl FakeFetcher {
        fn new(results: Vec<Result<MusicUrlResult, AppError>>) -> Arc<Self> {
            Arc::new(Self {
                queue: Mutex::new(results.into()),
                calls: Mutex::new(Vec::new()),
            })
        }

        /// 从 `(status, url)` 规格造队列：`url` 非空即成功；空串按 `status` 造出拒绝
        /// （`2`）或不存在（`0`）的形状。让测试读起来就是阶梯本身。
        fn from_status_specs(specs: &[(i64, &str)]) -> Arc<Self> {
            Self::new(
                specs
                    .iter()
                    .map(|(status, url)| {
                        if !url.is_empty() {
                            return Ok(success_result(url));
                        }
                        Ok(match status {
                            0 => not_found_result(),
                            _ => {
                                let mut result = refused_result();
                                result.observation.status = Some(*status);
                                result
                            }
                        })
                    })
                    .collect(),
            )
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        /// 依次请求过的档位。
        fn requested_qualities(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl MusicUrlFetcher for FakeFetcher {
        async fn fetch_music_url(
            &self,
            _cookies: &CookieStore,
            req: &MusicUrlRequest,
        ) -> Result<MusicUrlResult, AppError> {
            self.calls.lock().unwrap().push(req.quality.clone());
            // 注意 guard 只活在这一条语句里，不会跨到下面的 panic 分支去。
            let next = self.queue.lock().unwrap().pop_front();
            match next {
                Some(result) => result,
                None => panic!(
                    "取链替身队列已耗尽：第 {} 次调用请求了档位 `{}`。\
                     被测路径比预期多打了一次上游（降级或重试的次数不对）",
                    self.call_count(),
                    req.quality
                ),
            }
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
        let service = RuntimeService::new(
            config,
            client,
            Arc::new(NeverCalledFetcher),
            source_repo,
            account_repo,
            log_repo.clone(),
        );
        (service, log_repo)
    }

    /// 造一个能真正走到取链那一步的服务：账号已越过 dfid / 刷新两个网络前置步骤。
    fn build_loop_service(
        source: Source,
        fetcher: Arc<dyn MusicUrlFetcher>,
    ) -> (RuntimeService, Arc<CapturingLogRepo>) {
        build_loop_service_with_account(source, account_ready_for_fetch(), fetcher)
    }

    /// 同上，但账号由调用方给出：「VIP 不参与准入」这类断言需要账号带着特定的
    /// 权益状态越过前置步骤，否则测的就只是 `account_ready_for_fetch` 的默认值。
    fn build_loop_service_with_account(
        source: Source,
        account: ProviderAccount,
        fetcher: Arc<dyn MusicUrlFetcher>,
    ) -> (RuntimeService, Arc<CapturingLogRepo>) {
        let config = test_config();
        let client = Arc::new(KugouLiteClient::new(&config));
        let source_repo: Arc<dyn SourceRepo> = Arc::new(FakeSourceRepo {
            source: Some(source),
        });
        let account_repo: Arc<dyn AccountRepo> = Arc::new(FakeAccountRepo {
            account: Some(account),
        });
        let log_repo = Arc::new(CapturingLogRepo::default());
        let service = RuntimeService::new(
            config,
            client,
            fetcher,
            source_repo,
            account_repo,
            log_repo.clone(),
        );
        (service, log_repo)
    }

    /// 开关打开的音源。
    fn fallback_source(runtime_token: &str) -> Source {
        let mut source = sample_source(runtime_token, true);
        source.quality_fallback = true;
        source
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

    /// 预检行也要带上 `client_request_id`，且**观测字段必须全是 `None`**。
    ///
    /// 两件事都容易漏，且都是静默的：
    /// - id 在查到音源之后**立刻**生成，正是为了让这些早退路径也带上它。把它们排除在外
    ///   就等于「同一次请求的落盘行」在预检这一段断链——而那恰恰是最常出现在用户身上的
    ///   一类失败，正是最需要能一条 id 串到底的地方。
    /// - 观测必须是 `None` 而不是 `Some(0)`。`None` 是「上游什么都没说」，`Some(0)` 是
    ///   「上游说资源不存在」（真实的 35104 形态），两者含义相反。混同之后，界面用
    ///   `upstream_status !== null` 判断「这一行有没有观测」就会在**整页预检行**上渲染出
    ///   一个并不存在的上游观测。
    #[tokio::test]
    async fn precheck_row_carries_request_id_and_no_observation() {
        let source = sample_source("tok_disabled", false);
        let (service, log_repo) = build_service(Some(source), None);

        let _ = expect_err(service.fetch_music_url("tok_disabled", sample_req()).await);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(
            row.client_request_id
                .as_deref()
                .is_some_and(|id| id.starts_with("req_")),
            "预检行必须带上本次入站请求的 id，否则同一次请求的落盘行在预检这段断链：{:?}",
            row.client_request_id
        );
        assert_eq!(row.attempted_quality, None, "没走到取链，就没要过任何档位");
        assert_eq!(row.quality_fallback_index, 0, "没发生降级");
        assert_eq!(row.upstream_status, None);
        assert_eq!(row.upstream_priv_status, None);
        assert_eq!(row.upstream_fail_process, None);
        assert_eq!(row.upstream_ext_name, None);
        assert_eq!(row.upstream_file_size, None);
        assert_eq!(row.upstream_quality, None);
        assert_eq!(row.reported_types, None);
    }

    /// 客户端上报的档位元数据必须**原样**落盘，且不参与任何判断。
    ///
    /// 这条走预检路径，因此不需要上游——正好也说明它与取链无关：它是一份记录，
    /// 在任何取链决定之前就已经被记下来了。
    #[tokio::test]
    async fn reported_types_are_recorded_verbatim() {
        let source = sample_source("tok_disabled", false);
        let (service, log_repo) = build_service(Some(source), None);

        let reported = serde_json::json!({
            "flac": { "size": 29183562, "hash": "6F9F8AD76DDE83739386A9C29082CEC7" },
            "320k": { "size": 9412859 },
        });
        let mut req = sample_req();
        req.types = Some(reported.clone());

        let _ = expect_err(service.fetch_music_url("tok_disabled", req).await);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].reported_types,
            Some(reported),
            "上报的元数据必须逐字落盘：它只在客户端这一跳是免费的，过后再也拿不回来"
        );
    }

    /// 老脚本不发 `types`，请求体解析必须照常——落盘成 `None` 而不是报错。
    #[tokio::test]
    async fn missing_reported_types_is_recorded_as_none() {
        let source = sample_source("tok_disabled", false);
        let (service, log_repo) = build_service(Some(source), None);

        let _ = expect_err(service.fetch_music_url("tok_disabled", sample_req()).await);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].reported_types, None,
            "「客户端没上报」与「上报了一个空对象」必须可区分"
        );
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

    /// 预检阶段的早退必须**带账号 id**且**错误码精确**：面板读到的 `error` 文本会被
    /// `sanitize_runtime_log_error` 塌成常量，code 是那行日志唯一的机器可读信号。
    ///
    /// 这条测试原先钉的是 `ACCOUNT_NOT_VIP`（VIP 到期的预检行）。那个闸已随
    /// 「VIP 不是准入条件」一并删除，但这条要求对**还留着**的闸同样成立，因此把断言
    /// 换成账号被禁用（另一条带账号对象的预检早退），而不是删掉整条测试。
    #[tokio::test]
    async fn precheck_early_exit_carries_the_account_id_and_an_exact_code() {
        let source = sample_source("tok_gate_removed", true);
        let mut account = sample_account();
        account.status = AccountStatus::Disabled;
        let (service, log_repo) = build_service(Some(source.clone()), Some(account.clone()));

        let err = expect_err(
            service
                .fetch_music_url("tok_gate_removed", sample_req())
                .await,
        );
        assert_eq!(err.code, ErrorCode::AccountDisabled);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1, "预检失败必须恰好写一行");
        assert_eq!(rows[0].stage, RuntimeLogStage::Precheck);
        assert_eq!(rows[0].error_code.as_deref(), Some("ACCOUNT_DISABLED"));
        assert_eq!(
            rows[0].error_code.as_deref(),
            Some(ErrorCode::AccountDisabled.as_str()),
            "日志里的 code 必须与客户端拿到的 code 同源"
        );
        assert_eq!(rows[0].account_id, account.account_id);
        assert_eq!(rows[0].source_id, source.source_id);
    }

    /// **闸移除的验收点**：一条在期权益都没有的账号在取链预检**不再早退**，
    /// 而是照常打上游。
    ///
    /// 断言的不只是「没返回 `ACCOUNT_NOT_VIP`」：要钉的是**确实发生了一次上游调用**
    /// （用 `FakeFetcher` 数次数，`NeverCalledFetcher` 那种替身在这里帮不上忙），
    /// 以及**一行 `Precheck` 都没有**——旧闸既挡请求又写预检行，两样都得消失。反之，
    /// 如果哪天有人把权益判定重新塞回预检，这条会同时在这两处失败。
    #[tokio::test]
    async fn vip_inactive_account_is_no_longer_rejected_before_fetch() {
        let fetcher = FakeFetcher::new(vec![Ok(success_result("http://cdn.test/flac.flac"))]);
        // 前置步骤（dfid / 刷新）保持已满足：否则「没早退」也可能是被别的前置步骤
        // 挡住了，测不到准入这一段。
        let mut account = account_ready_for_fetch();
        // 「这个账号没有任何权益」现在的表述就是空权益列表——账号级的聚合布尔值
        // （`vip_active`）已删除，权益是并列的增量信息。这里显式写出来而不靠 fixture
        // 的默认值，测试的前提才不随 fixture 漂移。
        account.entitlements = Vec::new();
        let (service, log_repo) = build_loop_service_with_account(
            sample_source("tok_no_vip", true),
            account,
            fetcher.clone(),
        );

        let resp = service
            .fetch_music_url("tok_no_vip", sample_req())
            .await
            .expect("权益不再是准入闸：一条在期权益都没有的账号也必须真的去取链");
        assert_eq!(resp.url, "http://cdn.test/flac.flac");
        assert_eq!(fetcher.call_count(), 1, "应当且只应当打一次上游");

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert!(
            rows.iter()
                .all(|row| row.stage != RuntimeLogStage::Precheck),
            "没有发生任何预检早退，就不该有 Precheck 行：{:?}",
            rows.iter().map(|row| row.stage).collect::<Vec<_>>()
        );
        assert_eq!(rows[0].stage, RuntimeLogStage::FetchMusicUrl);
        assert!(rows[0].ok);
    }

    /// 账号被禁用同样落在预检阶段，且用的是账号自身的 id。
    #[tokio::test]
    async fn disabled_account_writes_a_precheck_row() {
        let source = sample_source("tok_acc_disabled", true);
        let mut account = sample_account();
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
    ///
    /// 入参是**本次**要的那一档（`attempted_quality`）而不是用户请求的档位：降级开启后
    /// 一次请求会为多档各写一行，若这里读用户请求的档位，阶梯上的每一行都会记成同一个
    /// 值，降级在最该看得见的那一列上消失。这里同时钉住「没要过档位就是 None」——
    /// 那正是预检/刷新那几类行的形状。
    #[test]
    fn upstream_quality_only_for_the_song_url_call() {
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::FetchMusicUrl, Some("flac24bit")).as_deref(),
            Some("high")
        );
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::FetchMusicUrl, Some("320k")).as_deref(),
            Some("320"),
            "降级到 320k 的那一行必须记 320，而不是用户请求的 flac"
        );
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::FetchMusicUrl, None),
            None,
            "取链阶段也可能没要过任何档位（异常路径），此时不得凭空记一个"
        );
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::EnsureDfid, Some("flac24bit")),
            None
        );
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::RefreshLogin, Some("flac24bit")),
            None
        );
        assert_eq!(
            upstream_quality_for(RuntimeLogStage::Precheck, Some("flac")),
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

    // ---- 降级循环 ----
    //
    // 下面这些测试全部走 `FakeFetcher`，因此无网络、无账号依赖。它们要钉住的不是
    // 「降级能拿到歌」这一件事，而是**每一次上游调用都恰好发生它该发生的次数、且
    // 每一行日志都说得起真话**——降级最危险的失败模式不是降不下去，而是多打了几次
    // 上游或者把阶梯记成平的一列，两者都不会让任何东西报错。

    /// 开关关着时，「明确拒绝」的行为必须与加这个特性之前**逐字相同**：
    /// 一个错误码、一行日志、一次上游调用。
    ///
    /// 这是整个特性的回退面：默认关闭意味着现网绝大多数音源不受影响，所以这条
    /// 必须钉死，而不是「顺便看一眼」。
    #[tokio::test]
    async fn refusal_with_fallback_off_writes_one_row_and_does_not_degrade() {
        let fetcher = FakeFetcher::new(vec![Ok(refused_result())]);
        // 刻意用 `sample_source`（`quality_fallback` 默认 false）而不是
        // `fallback_source`：这条测的正是**开关关着**的回退面，用错替身会让它变成
        // 主路径测试，而且因为降级会继续请求而替身队列只有一项，表现为替身 panic。
        let (service, log_repo) =
            build_loop_service(sample_source("tok_off", true), fetcher.clone());

        let err = expect_err(service.fetch_music_url("tok_off", sample_req()).await);
        assert_eq!(err.code, ErrorCode::UpstreamQualityRefused);
        assert_eq!(
            fetcher.call_count(),
            1,
            "开关关着时不得为同一首歌打第二次上游"
        );

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.error_code.as_deref(), Some("UPSTREAM_QUALITY_REFUSED"));
        assert!(!row.ok);
        assert_eq!(row.quality_fallback_index, 0);
        assert_eq!(row.retry_count, 0);
        assert_eq!(row.attempted_quality.as_deref(), Some("flac"));
        assert_eq!(row.upstream_status, Some(2));
    }

    /// 开关打开、首档被拒、次档成功：这是这个特性的主路径。
    ///
    /// 三件事必须同时成立，缺一条这个特性就等于没做：
    /// 1. 确实向上游要了**两个不同**的档位，且第二个正是阶梯的下一档；
    /// 2. 两行日志共享同一个 `client_request_id`（否则会被读成两次独立请求）；
    /// 3. `upstream_quality` 两行**不同**——那是「阶梯在日志里可见」的唯一证据。
    ///    若它继续读用户请求的档位（`req.quality`），两行都会是 `flac`。
    #[tokio::test]
    async fn fallback_on_walks_down_one_rung_and_the_ladder_is_visible() {
        let fetcher = FakeFetcher::new(vec![
            Ok(refused_result()),
            Ok(success_result("http://cdn.test/320.mp3")),
        ]);
        let (service, log_repo) = build_loop_service(fallback_source("tok_on"), fetcher.clone());

        let resp = service
            .fetch_music_url("tok_on", sample_req())
            .await
            .expect("降一档就应当拿到播放地址");
        assert_eq!(resp.url, "http://cdn.test/320.mp3");
        assert_eq!(
            fetcher.requested_qualities(),
            vec!["flac".to_string(), "320k".to_string()],
            "必须逐档向下，且第二档是 flac 的下一档"
        );

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 2, "每一档各写一行：被拒的那档也必须留下证据");
        let (first, second) = (&rows[0], &rows[1]);

        assert_eq!(
            first.client_request_id, second.client_request_id,
            "同一次入站请求的多行必须共享一个 id"
        );
        assert!(first.client_request_id.is_some());

        assert!(!first.ok);
        assert_eq!(
            first.error_code.as_deref(),
            Some("UPSTREAM_QUALITY_REFUSED")
        );
        assert_eq!(first.quality_fallback_index, 0);
        assert_eq!(first.attempted_quality.as_deref(), Some("flac"));

        assert!(second.ok);
        assert_eq!(second.error_code, None);
        assert_eq!(second.quality_fallback_index, 1);
        assert_eq!(second.attempted_quality.as_deref(), Some("320k"));

        // 断言的是**字面量**而不是 `map_quality(...)`：用被测函数自己算期望值等于同义
        // 反复，真正被钉住的是「flac 档记 flac、320k 档记 320」这两条映射。
        assert_eq!(first.upstream_quality.as_deref(), Some("flac"));
        assert_eq!(second.upstream_quality.as_deref(), Some("320"));
        assert_ne!(
            first.upstream_quality, second.upstream_quality,
            "两行记同一个 upstream_quality 就意味着降级在最该看得见的列上消失了"
        );

        // 阶梯位置不得借用 retry_count——那一列另有含义（刷新登录后重试同一档）。
        assert!(rows.iter().all(|row| row.retry_count == 0));
        assert!(
            rows.iter()
                .all(|row| row.stage != RuntimeLogStage::RefreshLogin),
            "拒绝路径不得触发登录刷新"
        );
    }

    /// 四档全拒。起始档位取 `flac24bit`（阶梯顶端），因此恰好降三次、写四行。
    /// 这条同时把 `QUALITY_LADDER` 的实际顺序钉在**日志**这一侧。
    #[tokio::test]
    async fn falling_all_the_way_down_ends_in_exhausted() {
        let mut req = sample_req();
        req.quality = "flac24bit".into();

        let fetcher = FakeFetcher::from_status_specs(&[(2, ""), (2, ""), (2, ""), (2, "")]);
        let (service, log_repo) = build_loop_service(fallback_source("tok_all"), fetcher.clone());

        let err = expect_err(service.fetch_music_url("tok_all", req).await);
        assert_eq!(err.code, ErrorCode::UpstreamQualityExhausted);
        assert_eq!(
            fetcher.requested_qualities(),
            vec!["flac24bit", "flac", "320k", "128k"],
            "必须一路走到阶梯底，且每一档只试一次"
        );

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 4);
        let codes: Vec<Option<&str>> = rows.iter().map(|row| row.error_code.as_deref()).collect();
        assert_eq!(
            codes,
            vec![
                Some("UPSTREAM_QUALITY_REFUSED"),
                Some("UPSTREAM_QUALITY_REFUSED"),
                Some("UPSTREAM_QUALITY_REFUSED"),
                // 只有降到底仍被拒的**终局**行才是 EXHAUSTED。中间档复用 REFUSED，
                // 因为那个码描述的是上游做了什么，而不是我们的策略。
                Some("UPSTREAM_QUALITY_EXHAUSTED"),
            ]
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.quality_fallback_index)
                .collect::<Vec<u8>>(),
            vec![0, 1, 2, 3]
        );
        assert!(rows.iter().all(|row| !row.ok));
        assert!(rows.iter().all(|row| row.retry_count == 0));
        assert_eq!(rows[3].attempted_quality.as_deref(), Some("128k"));
        assert_eq!(rows[3].upstream_status, Some(2));
    }

    /// `status:0` 是「资源不存在 / 参数错」，与音质无关：换一档照样没有。
    /// 即使开关打开也**不得**降级，否则一次「没这首歌」会变成四次上游调用。
    ///
    /// 同时断言这一行的观测仍然落盘（`upstream_status == Some(0)`）：观测与是否降级
    /// 是两件事，面板要能看到「上游说了 0，而我们没动」。
    #[tokio::test]
    async fn status_zero_does_not_degrade_even_with_fallback_on() {
        let fetcher = FakeFetcher::from_status_specs(&[(0, "")]);
        let (service, log_repo) = build_loop_service(fallback_source("tok_zero"), fetcher.clone());

        let err = expect_err(service.fetch_music_url("tok_zero", sample_req()).await);
        assert_eq!(err.code, ErrorCode::UpstreamPlayUrlEmpty);
        assert_eq!(fetcher.call_count(), 1, "资源不存在不是降级的理由");

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.error_code.as_deref(), Some("UPSTREAM_PLAY_URL_EMPTY"));
        assert_eq!(row.quality_fallback_index, 0);
        assert_eq!(row.attempted_quality.as_deref(), Some("flac"));
        assert_eq!(
            row.upstream_status,
            Some(0),
            "没降级不等于不用记观测：面板正是靠它区分「上游说 0」与「上游什么都没说」"
        );
    }

    /// **顺序钉**：`status:2` 与 `auth_failed` 同时为真时，必须降级，且不得去刷新登录。
    ///
    /// `auth_failed: true` 是**合成**的：`is_auth_failure` 是一组子串启发式
    /// （`token` / `dfid` / `需要验证` / `"code":301`），而三份真实抓包里没有一份命中过
    /// 它——正因为真实数据碰不出来，这条顺序规则只能靠构造输入来守。若把 auth 判定
    /// 挪到拒绝判定之前，这里会去刷新登录、重试同一个被拒的档位、然后再被拒一次，
    /// 降级在最需要它的场景里静默失效。
    ///
    /// 顺带钉住「拒绝路径一行 RefreshLogin 都不该有」——它是「没走 auth 分支」的
    /// 直接证据，比数上游调用次数更贴近这条规则本身。
    #[tokio::test]
    async fn refusal_wins_over_the_auth_heuristics_and_still_degrades() {
        let mut refused_but_looks_like_auth = refused_result();
        refused_but_looks_like_auth.auth_failed = true;

        let fetcher = FakeFetcher::new(vec![
            Ok(refused_but_looks_like_auth),
            Ok(success_result("http://cdn.test/320.mp3")),
        ]);
        let (service, log_repo) =
            build_loop_service(fallback_source("tok_order_on"), fetcher.clone());

        let resp = service
            .fetch_music_url("tok_order_on", sample_req())
            .await
            .expect("明确拒绝必须走降级，而不是走登录刷新");
        assert_eq!(resp.url, "http://cdn.test/320.mp3");

        assert_eq!(
            fetcher.requested_qualities(),
            vec!["flac".to_string(), "320k".to_string()],
            "第二档必须是阶梯的下一档，而不是被拒档位的重试"
        );
        let rows = log_repo.rows();
        assert!(
            rows.iter()
                .all(|row| row.stage != RuntimeLogStage::RefreshLogin),
            "拒绝判定若排在 auth 之前，这里会出现一行 RefreshLogin"
        );
        assert!(rows.iter().all(|row| row.retry_count == 0));
        assert!(rows.last().unwrap().ok);
    }

    /// 与上一条同样的输入，但开关关着：仍然必须**优先**判成「上游拒绝」，
    /// 不得去刷新登录。关开关只关掉「继续降档」，不关掉「如实说出上游拒绝了你」。
    #[tokio::test]
    async fn refusal_with_fallback_off_also_beats_the_auth_heuristics() {
        let mut refused_but_looks_like_auth = refused_result();
        refused_but_looks_like_auth.auth_failed = true;

        let fetcher = FakeFetcher::new(vec![Ok(refused_but_looks_like_auth)]);
        let (service, log_repo) =
            build_loop_service(sample_source("tok_order_off", true), fetcher.clone());

        let err = expect_err(service.fetch_music_url("tok_order_off", sample_req()).await);
        assert_eq!(err.code, ErrorCode::UpstreamQualityRefused);
        assert_eq!(fetcher.call_count(), 1);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].error_code.as_deref(),
            Some("UPSTREAM_QUALITY_REFUSED")
        );
        assert!(
            rows.iter()
                .all(|row| row.stage != RuntimeLogStage::RefreshLogin),
            "关掉开关不等于把拒绝误判成登录失效"
        );
    }

    /// 传输层失败（超时 / 连接被拒 / 5xx）不是对音质的表态，即使开关打开也不降级：
    /// 否则上游一限流，我们的请求数就乘四。错误原样抛出，可重试语义保持不变。
    ///
    /// `attempted_quality` 要记下**炸掉的那一档**——降级到第二档才炸时，
    /// 「是哪一档炸的」正是排查的第一个问题。
    #[tokio::test]
    async fn transport_error_does_not_degrade_and_records_the_rung() {
        let fetcher = FakeFetcher::new(vec![Err(AppError::upstream_request_failed("boom"))]);
        let (service, log_repo) = build_loop_service(fallback_source("tok_err"), fetcher.clone());

        let err = expect_err(service.fetch_music_url("tok_err", sample_req()).await);
        assert_eq!(err.code, ErrorCode::UpstreamRequestFailed);
        assert_eq!(fetcher.call_count(), 1, "传输失败不得触发降档重试");

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.stage, RuntimeLogStage::FetchMusicUrl);
        assert_eq!(row.attempted_quality.as_deref(), Some("flac"));
        assert_eq!(row.quality_fallback_index, 0);
        assert_eq!(row.retry_count, 0);
        assert_eq!(row.upstream_status, None, "没拿到响应体就没有观测");
    }

    /// 在阶梯**底部**被拒：`next_lower_quality("128k")` 返回 `None`，无处可降。
    ///
    /// 这一行必须是 `REFUSED` 而不是 `EXHAUSTED`：`EXHAUSTED` 的语义是「降到底仍被拒」，
    /// 而这里一档都没降过（`quality_fallback_index == 0`）。把两者混起来会让面板对
    /// 「一上来就被拒」显示成「试遍了所有档位」，把一次失败说成三次。
    #[tokio::test]
    async fn refusal_at_the_bottom_rung_is_refused_not_exhausted() {
        let mut req = sample_req();
        req.quality = "128k".into();

        let fetcher = FakeFetcher::new(vec![Ok(refused_result())]);
        let (service, log_repo) =
            build_loop_service(fallback_source("tok_bottom"), fetcher.clone());

        let err = expect_err(service.fetch_music_url("tok_bottom", req).await);
        assert_eq!(err.code, ErrorCode::UpstreamQualityRefused);
        assert_eq!(fetcher.call_count(), 1);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].error_code.as_deref(),
            Some("UPSTREAM_QUALITY_REFUSED")
        );
        assert_eq!(rows[0].quality_fallback_index, 0);
        assert_eq!(rows[0].attempted_quality.as_deref(), Some("128k"));
    }
}
