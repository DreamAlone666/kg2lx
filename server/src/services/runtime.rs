use std::sync::Arc;
use std::time::Instant;

use crate::domain::account::AccountStatus;
use crate::domain::event_log::{
    Event, EventContext, EventLog, EventTrigger, MusicUrlEvent, MusicUrlStage, RefreshReason,
};
use crate::domain::renewal::PlaybackSignal;
use crate::error::{AppError, ErrorCode};
use crate::repos::account::AccountRepo;
use crate::repos::event_log::{self, EventLogRepo};
use crate::repos::source::SourceRepo;
use crate::services::auth::AccountAuth;
use crate::services::entitlement::EntitlementService;
use crate::services::kugou_lite_client::{
    MusicUrlFetcher, MusicUrlRequest, MusicUrlResult, map_quality, next_lower_quality,
    upstream_refused_quality,
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
    auth: Arc<AccountAuth>,
    fetcher: Arc<dyn MusicUrlFetcher>,
    source_repo: Arc<dyn SourceRepo>,
    account_repo: Arc<dyn AccountRepo>,
    logs: Arc<dyn EventLogRepo>,
    entitlement: Option<Arc<EntitlementService>>,
}

impl RuntimeService {
    pub fn new(
        auth: Arc<AccountAuth>,
        fetcher: Arc<dyn MusicUrlFetcher>,
        source_repo: Arc<dyn SourceRepo>,
        account_repo: Arc<dyn AccountRepo>,
        logs: Arc<dyn EventLogRepo>,
        entitlement: Option<Arc<EntitlementService>>,
    ) -> Self {
        Self {
            auth,
            fetcher,
            source_repo,
            account_repo,
            logs,
            entitlement,
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
        let context = EventContext::new(
            EventTrigger::Runtime,
            Some(source.account_id.clone()),
            Some(source.source_id),
        );

        let precheck = async {
            let account = self
                .account_repo
                .find_by_id(&source.account_id)
                .await?
                .ok_or_else(AppError::account_not_found)?;
            if account.status == AccountStatus::Disabled {
                return Err(AppError::account_disabled());
            }
            self.auth.prepare_playback(account, &context).await
        }
        .await;
        let mut account = match precheck {
            Ok(account) => account,
            Err(error) => {
                self.record(&context, &req, None, Some(error.code)).await;
                return Err(error);
            }
        };

        let mut upstream_req = MusicUrlRequest {
            hash: req.hash.clone(),
            album_audio_id: req.album_audio_id.clone(),
            quality: req.quality.clone(),
        };
        let mut fallback_index = 0;
        let mut retry_count = 0;
        let mut renewal_signal = None;
        let result = loop {
            let started = Instant::now();
            let response = self
                .fetcher
                .fetch_music_url(&account.cookies, &upstream_req)
                .await;
            let mut attempt = Attempt {
                quality: &upstream_req.quality,
                fallback_index,
                retry_count,
                latency_ms: started.elapsed().as_millis() as u64,
                response: None,
            };
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    self.record(&context, &req, Some(attempt), Some(error.code))
                        .await;
                    break Err(error);
                }
            };
            attempt.response = Some(&response);
            let sent = account;
            // A late response may refer to an obsolete login. Use the sent version for both
            // conditional cookie merge and reactive authentication.
            account = if response.human_verification {
                sent.clone()
            } else {
                self.account_repo
                    .merge_cookies(&sent, &response.cookies)
                    .await?
            };

            if response.human_verification {
                let error =
                    AppError::upstream_request_failed("upstream requires human verification");
                self.record(&context, &req, Some(attempt), Some(error.code))
                    .await;
                break Err(error);
            }
            let refused = upstream_refused_quality(response.observation.status);
            if fallback_index == 0 && (200..300).contains(&response.status_code) {
                renewal_signal = if !response.url.is_empty() {
                    Some(PlaybackSignal::Link)
                } else if refused {
                    Some(PlaybackSignal::TargetRefused)
                } else {
                    None
                };
            }
            if !response.url.is_empty() {
                self.record(&context, &req, Some(attempt), None).await;
                break Ok(RuntimeMusicUrlResponse { url: response.url });
            }
            // A known refusal owns the fallback decision even when an auth heuristic also matches.
            if refused {
                if retry_count == 0
                    && source.quality_fallback
                    && let Some(lower) = next_lower_quality(&upstream_req.quality)
                {
                    self.record(
                        &context,
                        &req,
                        Some(attempt),
                        Some(ErrorCode::UpstreamQualityRefused),
                    )
                    .await;
                    upstream_req.quality = lower.into();
                    fallback_index += 1;
                    continue;
                }
                let error = if fallback_index > 0 && retry_count == 0 {
                    AppError::upstream_quality_exhausted()
                } else {
                    AppError::upstream_quality_refused()
                };
                self.record(&context, &req, Some(attempt), Some(error.code))
                    .await;
                break Err(error);
            }
            if retry_count == 0 && response.auth_failed {
                // Record the rejected attempt before the refresh so its observation is visible.
                self.record(
                    &context,
                    &req,
                    Some(attempt),
                    Some(ErrorCode::UpstreamPlayUrlEmpty),
                )
                .await;
                account = self
                    .auth
                    .refresh(sent, RefreshReason::Rejected, context.clone())
                    .await?;
                retry_count = 1;
                continue;
            }
            let error = AppError::upstream_play_url_empty();
            self.record(&context, &req, Some(attempt), Some(error.code))
                .await;
            break Err(error);
        };
        if let (Some(entitlement), Some(signal)) = (&self.entitlement, renewal_signal) {
            entitlement.on_playback(context, signal);
        }
        result
    }

    async fn record(
        &self,
        context: &EventContext,
        req: &RuntimeMusicUrlRequest,
        attempt: Option<Attempt<'_>>,
        error: Option<ErrorCode>,
    ) {
        let response = attempt.as_ref().and_then(|attempt| attempt.response);
        let observed = response.map(|response| &response.observation);
        let quality = attempt.as_ref().map(|attempt| attempt.quality);
        let payload = MusicUrlEvent {
            stage: if attempt.is_some() {
                MusicUrlStage::FetchMusicUrl
            } else {
                MusicUrlStage::Precheck
            },
            track_title: req.track_title.clone(),
            artist_name: req.artist_name.clone(),
            album_name: req.album_name.clone(),
            request_hash: req.hash.clone(),
            album_audio_id: req.album_audio_id.clone(),
            requested_quality: req.quality.clone(),
            attempted_quality: quality.map(str::to_owned),
            quality_fallback_index: attempt.as_ref().map_or(0, |attempt| attempt.fallback_index),
            retry_count: attempt.as_ref().map_or(0, |attempt| attempt.retry_count),
            reported_types: req.types.clone(),
            upstream_endpoint: attempt.as_ref().map(|_| "song/url".into()),
            status_code: response.map(|response| response.status_code),
            human_verification: response.is_some_and(|response| response.human_verification),
            upstream_quality: quality.map(|quality| map_quality(quality).to_owned()),
            upstream_status: observed.and_then(|o| o.status),
            upstream_priv_status: observed.and_then(|o| o.priv_status),
            upstream_fail_process: observed.and_then(|o| o.fail_process.clone()),
            upstream_ext_name: observed.and_then(|o| o.ext_name.clone()),
            upstream_file_size: observed.and_then(|o| o.file_size),
            upstream_bit_rate: observed.and_then(|o| o.bit_rate),
            upstream_time_length: observed.and_then(|o| o.time_length),
        };
        event_log::record(
            self.logs.as_ref(),
            EventLog::new(
                context.clone(),
                Event::MusicUrl(Box::new(payload)),
                attempt.map(|attempt| attempt.latency_ms),
                error,
            ),
        )
        .await;
    }
}

struct Attempt<'a> {
    quality: &'a str,
    fallback_index: u8,
    retry_count: u8,
    latency_ms: u64,
    response: Option<&'a MusicUrlResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use crate::config::Config;
    use crate::domain::account::{self, ProviderAccount};
    use crate::domain::cookie_store::CookieStore;
    use crate::domain::event_log::ListEvents;
    use crate::domain::provider::ProviderKind;
    use crate::domain::source::Source;
    use crate::services::kugou_lite_client::{KugouLiteClient, UpstreamObservation};
    // 只在测试里出现（生产代码只经 trait 使用这个类型），因此导入放在测试模块内，
    // 免得非测试构建报 unused import。
    use crate::services::kugou_lite_client::MusicUrlResult;

    // ---- 测试替身 ----
    //
    // 预检分支在真正打到上游之前就返回了，所以这些测试不需要任何网络；只需要能
    // 分别给出「查得到 / 查不到」的音源与账号，以及一个能把落盘行扣下来检查的
    // 日志仓库。这里使用内存记录，直接检查「这一行该不该写」
    // 以及载荷是否忠实。

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

        async fn list_by_account_id(&self, _account_id: &str) -> Result<Vec<Source>, AppError> {
            Ok(Vec::new())
        }

        async fn update_settings(
            &self,
            _source_id: &str,
            _name: Option<String>,
            _quality_fallback: Option<bool>,
            _updated_at: i64,
        ) -> Result<Source, AppError> {
            unreachable!("test did not update the source")
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
        async fn list_all(&self) -> Result<Vec<ProviderAccount>, AppError> {
            Ok(self.account.clone().into_iter().collect())
        }

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

        async fn save_auth(
            &self,
            _previous: &ProviderAccount,
            _updated: &ProviderAccount,
        ) -> Result<ProviderAccount, AppError> {
            unreachable!("test did not expect authentication");
        }

        async fn merge_cookies(
            &self,
            request: &ProviderAccount,
            _response: &crate::domain::cookie_store::CookieStore,
        ) -> Result<ProviderAccount, AppError> {
            Ok(request.clone())
        }
    }

    /// 把 `append` 收到的东西原样留一份，测试才能断言「写了什么 / 有没有写」。
    #[derive(Default)]
    struct CapturingLogRepo {
        rows: Mutex<Vec<EventLog>>,
    }

    fn music(row: &EventLog) -> &MusicUrlEvent {
        match &row.event {
            Event::MusicUrl(event) => event,
            _ => panic!("expected a music URL event"),
        }
    }

    impl CapturingLogRepo {
        fn rows(&self) -> Vec<EventLog> {
            self.rows.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl EventLogRepo for CapturingLogRepo {
        async fn append(&self, log: &EventLog) -> Result<(), AppError> {
            self.rows.lock().unwrap().push(log.clone());
            Ok(())
        }

        async fn list(&self, _query: &ListEvents) -> Result<Vec<EventLog>, AppError> {
            Ok(Vec::new())
        }

        async fn list_global(
            &self,
            _query: &crate::domain::event_log::ListGlobalEvents,
        ) -> Result<Vec<crate::domain::event_log::StoredEventLog>, AppError> {
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
            account_refresh_ttl: 6 * 3600,
            event_log_limit: crate::config::DEFAULT_EVENT_LOG_LIMIT,
            web_dist_dir: None,
        })
    }

    fn sample_source(runtime_token: &str) -> Source {
        Source {
            source_id: "src_test".into(),
            account_id: "acc_test".into(),
            provider: ProviderKind::KugouLite,
            name: "测试音源".into(),
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
            cookies: CookieStore::new(),
            last_refresh_at: None,
            credential_version: 0,
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
            human_verification: false,
            observation: UpstreamObservation {
                status: Some(2),
                priv_status: Some(0),
                fail_process: Some(vec!["pkg".into(), "buy".into()]),
                ext_name: None,
                file_size: None,
                bit_rate: None,
                time_length: None,
            },
        }
    }

    /// 成功形状。观测里带上 `ext_name` / `file_size`——它们正是「实际拿到的是不是
    /// 所要档位」的自证材料（`quality=high` 会静默回落到 128k mp3 却仍是 `status:1`）。
    ///
    /// 码率与时长这次也一并造出来（mp3 320 的形状：精确的 `320000` 加 180 秒），
    /// 因为落盘映射的每一行都是穷举字段的——不造出来，`append_log` 里少映射一个字段
    /// 不会有任何一条测试变红（映射成 `None` 与「本来就没有」长得一样）。
    fn success_result(url: &str) -> MusicUrlResult {
        MusicUrlResult {
            url: url.into(),
            cookies: CookieStore::new(),
            status_code: 200,
            auth_failed: false,
            human_verification: false,
            observation: UpstreamObservation {
                status: Some(1),
                priv_status: Some(1),
                fail_process: None,
                ext_name: Some("mp3".into()),
                file_size: Some(9_412_859),
                bit_rate: Some(320_000),
                time_length: Some(180),
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
            human_verification: false,
            observation: UpstreamObservation {
                status: Some(0),
                priv_status: None,
                fail_process: None,
                ext_name: None,
                file_size: None,
                bit_rate: None,
                time_length: None,
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
        let auth = Arc::new(AccountAuth::new(
            &config,
            client,
            account_repo.clone(),
            log_repo.clone(),
        ));
        let service = RuntimeService::new(
            auth,
            Arc::new(NeverCalledFetcher),
            source_repo,
            account_repo,
            log_repo.clone(),
            None,
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
        let auth = Arc::new(AccountAuth::new(
            &config,
            client,
            account_repo.clone(),
            log_repo.clone(),
        ));
        let service = RuntimeService::new(
            auth,
            fetcher,
            source_repo,
            account_repo,
            log_repo.clone(),
            None,
        );
        (service, log_repo)
    }

    /// 开关打开的音源。
    fn fallback_source(runtime_token: &str) -> Source {
        let mut source = sample_source(runtime_token);
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
        let source = sample_source("tok_no_account");
        let (service, log_repo) = build_service(Some(source), None);

        let _ = expect_err(
            service
                .fetch_music_url("tok_no_account", sample_req())
                .await,
        );

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(
            row.context.correlation_id.starts_with("req_"),
            "预检行必须带上本次入站请求的 id，否则同一次请求的落盘行在预检这段断链：{:?}",
            row.context.correlation_id
        );
        assert_eq!(
            music(row).attempted_quality,
            None,
            "没走到取链，就没要过任何档位"
        );
        assert_eq!(music(row).quality_fallback_index, 0, "没发生降级");
        assert_eq!(music(row).upstream_status, None);
        assert_eq!(music(row).upstream_priv_status, None);
        assert_eq!(music(row).upstream_fail_process, None);
        assert_eq!(music(row).upstream_ext_name, None);
        assert_eq!(music(row).upstream_file_size, None);
        assert_eq!(music(row).upstream_bit_rate, None);
        assert_eq!(music(row).upstream_time_length, None);
        assert_eq!(music(row).upstream_quality, None);
        assert_eq!(music(row).reported_types, None);
    }

    /// 客户端上报的档位元数据必须**原样**落盘，且不参与任何判断。
    ///
    /// 这条走预检路径，因此不需要上游——正好也说明它与取链无关：它是一份记录，
    /// 在任何取链决定之前就已经被记下来了。
    #[tokio::test]
    async fn reported_types_are_recorded_verbatim() {
        let source = sample_source("tok_no_account");
        let (service, log_repo) = build_service(Some(source), None);

        let reported = serde_json::json!({
            "flac": { "size": 29183562, "hash": "6F9F8AD76DDE83739386A9C29082CEC7" },
            "320k": { "size": 9412859 },
        });
        let mut req = sample_req();
        req.types = Some(reported.clone());

        let _ = expect_err(service.fetch_music_url("tok_no_account", req).await);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            music(&rows[0]).reported_types,
            Some(reported),
            "上报的元数据必须逐字落盘：它只在客户端这一跳是免费的，过后再也拿不回来"
        );
    }

    /// 老脚本不发 `types`，请求体解析必须照常——落盘成 `None` 而不是报错。
    #[tokio::test]
    async fn missing_reported_types_is_recorded_as_none() {
        let source = sample_source("tok_no_account");
        let (service, log_repo) = build_service(Some(source), None);

        let _ = expect_err(
            service
                .fetch_music_url("tok_no_account", sample_req())
                .await,
        );

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            music(&rows[0]).reported_types,
            None,
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
        let source = sample_source("tok_no_account");
        let (service, log_repo) = build_service(Some(source.clone()), None);

        let err = expect_err(
            service
                .fetch_music_url("tok_no_account", sample_req())
                .await,
        );
        assert_eq!(err.code, ErrorCode::AccountNotFound);

        let rows = log_repo.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(music(&rows[0]).stage, MusicUrlStage::Precheck);
        assert_eq!(rows[0].error_code.as_deref(), Some("ACCOUNT_NOT_FOUND"));
        assert_eq!(
            rows[0].context.source_id.as_deref(),
            Some(source.source_id.as_str())
        );
        assert_eq!(
            rows[0].context.account_id.as_deref(),
            Some(source.account_id.as_str())
        );
    }

    /// 预检阶段的早退必须**带账号 id**且**错误码精确**：面板读到的 `error` 文本会被
    /// `sanitize_runtime_log_error` 塌成常量，code 是那行日志唯一的机器可读信号。
    ///
    /// 这条测试原先钉的是 `ACCOUNT_NOT_VIP`（VIP 到期的预检行）。那个闸已随
    /// 「VIP 不是准入条件」一并删除，但这条要求对**还留着**的闸同样成立，因此把断言
    /// 换成账号被禁用（另一条带账号对象的预检早退），而不是删掉整条测试。
    #[tokio::test]
    async fn precheck_early_exit_carries_the_account_id_and_an_exact_code() {
        let source = sample_source("tok_gate_removed");
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
        assert_eq!(music(&rows[0]).stage, MusicUrlStage::Precheck);
        assert_eq!(rows[0].error_code.as_deref(), Some("ACCOUNT_DISABLED"));
        assert_eq!(
            rows[0].error_code.as_deref(),
            Some(ErrorCode::AccountDisabled.as_str()),
            "日志里的 code 必须与客户端拿到的 code 同源"
        );
        assert_eq!(
            rows[0].context.account_id.as_deref(),
            Some(account.account_id.as_str())
        );
        assert_eq!(
            rows[0].context.source_id.as_deref(),
            Some(source.source_id.as_str())
        );
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
        let account = account_ready_for_fetch();
        let (service, log_repo) =
            build_loop_service_with_account(sample_source("tok_no_vip"), account, fetcher.clone());

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
                .all(|row| music(row).stage != MusicUrlStage::Precheck),
            "没有发生任何预检早退，就不该有 Precheck 行：{:?}",
            rows.iter().map(|row| music(row).stage).collect::<Vec<_>>()
        );
        assert_eq!(music(&rows[0]).stage, MusicUrlStage::FetchMusicUrl);
        assert!(rows[0].ok);
    }

    /// 账号被禁用同样落在预检阶段，且用的是账号自身的 id。
    #[tokio::test]
    async fn disabled_account_writes_a_precheck_row() {
        let source = sample_source("tok_acc_disabled");
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
        assert_eq!(music(&rows[0]).stage, MusicUrlStage::Precheck);
        assert_eq!(rows[0].error_code.as_deref(), Some("ACCOUNT_DISABLED"));
        assert_eq!(
            rows[0].context.account_id.as_deref(),
            Some(account.account_id.as_str())
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
        let (service, log_repo) = build_loop_service(sample_source("tok_off"), fetcher.clone());

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
        assert_eq!(music(row).quality_fallback_index, 0);
        assert_eq!(music(row).retry_count, 0);
        assert_eq!(music(row).attempted_quality.as_deref(), Some("flac"));
        assert_eq!(music(row).upstream_status, Some(2));
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
            first.context.correlation_id, second.context.correlation_id,
            "同一次入站请求的多行必须共享一个 id"
        );
        assert!(!first.context.correlation_id.is_empty());

        assert!(!first.ok);
        assert_eq!(
            first.error_code.as_deref(),
            Some("UPSTREAM_QUALITY_REFUSED")
        );
        assert_eq!(music(first).quality_fallback_index, 0);
        assert_eq!(music(first).attempted_quality.as_deref(), Some("flac"));

        assert!(second.ok);
        assert_eq!(second.error_code, None);
        assert_eq!(music(second).quality_fallback_index, 1);
        assert_eq!(music(second).attempted_quality.as_deref(), Some("320k"));

        // 断言的是**字面量**而不是 `map_quality(...)`：用被测函数自己算期望值等于同义
        // 反复，真正被钉住的是「flac 档记 flac、320k 档记 320」这两条映射。
        assert_eq!(music(first).upstream_quality.as_deref(), Some("flac"));
        assert_eq!(music(second).upstream_quality.as_deref(), Some("320"));
        assert_ne!(
            music(first).upstream_quality,
            music(second).upstream_quality,
            "两行记同一个 upstream_quality 就意味着降级在最该看得见的列上消失了"
        );

        // 实测到的格式 / 字节 / 码率 / 时长必须从观测一路走到落盘行。
        // `bit_rate` 与 `time_length` 是本轮新增的两个，少映射一个都不会有别的测试
        // 变红——`append_log` 写 `None` 与「上游没说」在断言层面长得一样。
        assert_eq!(music(second).upstream_ext_name.as_deref(), Some("mp3"));
        assert_eq!(music(second).upstream_file_size, Some(9_412_859));
        assert_eq!(music(second).upstream_bit_rate, Some(320_000));
        assert_eq!(music(second).upstream_time_length, Some(180));
        assert_eq!(
            music(first).upstream_bit_rate,
            None,
            "被拒的那一档没有文件，就没有码率可谈：不许写成 0"
        );

        // 阶梯位置不得借用 retry_count——那一列另有含义（刷新登录后重试同一档）。
        assert!(rows.iter().all(|row| music(row).retry_count == 0));
        assert!(
            rows.iter()
                .all(|row| matches!(row.event, Event::MusicUrl(_))),
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
                .map(|row| music(row).quality_fallback_index)
                .collect::<Vec<u8>>(),
            vec![0, 1, 2, 3]
        );
        assert!(rows.iter().all(|row| !row.ok));
        assert!(rows.iter().all(|row| music(row).retry_count == 0));
        assert_eq!(music(&rows[3]).attempted_quality.as_deref(), Some("128k"));
        assert_eq!(music(&rows[3]).upstream_status, Some(2));
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
        assert_eq!(music(row).quality_fallback_index, 0);
        assert_eq!(music(row).attempted_quality.as_deref(), Some("flac"));
        assert_eq!(
            music(row).upstream_status,
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
                .all(|row| matches!(row.event, Event::MusicUrl(_))),
            "拒绝判定若排在 auth 之前，这里会出现一行 RefreshLogin"
        );
        assert!(rows.iter().all(|row| music(row).retry_count == 0));
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
            build_loop_service(sample_source("tok_order_off"), fetcher.clone());

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
                .all(|row| matches!(row.event, Event::MusicUrl(_))),
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
        assert_eq!(music(row).stage, MusicUrlStage::FetchMusicUrl);
        assert_eq!(music(row).attempted_quality.as_deref(), Some("flac"));
        assert_eq!(music(row).quality_fallback_index, 0);
        assert_eq!(music(row).retry_count, 0);
        assert_eq!(music(row).upstream_status, None, "没拿到响应体就没有观测");
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
        assert_eq!(music(&rows[0]).quality_fallback_index, 0);
        assert_eq!(music(&rows[0]).attempted_quality.as_deref(), Some("128k"));
    }
}
