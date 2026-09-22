use std::sync::Arc;

use crate::config::Config;
use crate::domain::account::{self, AccountStatus, VipEntitlement};
use crate::domain::runtime_log::{ListRuntimeLogsQuery, RuntimeLog, RuntimeLogView};
use crate::domain::source::{
    self, SCRIPT_NAME_ADVISORY_CHARS, SCRIPT_NAME_MAX_BYTES, sanitize_script_meta,
};
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::runtime_log::RuntimeLogRepo;
use crate::repos::source::SourceRepo;
use crate::services::kugou_lite_client::KugouLiteClient;

#[derive(serde::Serialize)]
pub struct SourceListItem {
    pub source_id: String,
    pub account_id: String,
    pub provider: String,
    pub script_url: String,
    /// 当前生效的标题（即 `Source.name`），原样返回以便后台显示计数与超长提示。
    pub name: String,
    /// 系统默认标题，供后台展示「默认：酷狗概念版 12345678」与一键恢复。
    /// `userid` 缺失时返回去掉尾随空格的结果，不会产生「酷狗概念版 」这类文案。
    pub default_name: String,
    pub userid: String,
    /// 上游会员权益明细，与详情页的 `account.entitlements` 是**同一个结构体、同一份
    /// 数据**（`VipEntitlement`），刻意不另造一个精简版：两个页面对「这个账号有哪些
    /// 权益」必须给出同一个答案，各留一份字段就是留给它们漂移的机会。
    ///
    /// 列表**必须**带它：列表要回答「这个音源现在还能不能用」，而此前回答这件事的是
    /// 账号级的聚合布尔值（`vip_active`）——那个值既冗余又低效，已经删除（权益是
    /// 并列的增量信息，聚合值只是它的派生品，上游新增一档权益时会静默失真）。改成
    /// 「有哪些**有名字**的权益」之后，列表与详情口径一致，扫一眼就能看出这个账号
    /// 有没有权益、是哪几条，而不用逐个点进详情。
    ///
    /// 空数组是**正常状态**（这个账号一条权益都没有），不是「查不到上游」：这一列只
    /// 由**成功**的权益查询写入，因此空数组的成因只可能是「上一次查到的就是空的」；
    /// 问不到上游时账号会被标成 `login_failed`，权益保持上一次成功时的值不变。
    pub entitlements: Vec<VipEntitlement>,
    /// 账号状态，与详情页 `account.status` 同源同格式。
    ///
    /// 列表要回答「账号还活着吗」这个最核心的问题，而列表本来就是用来扫一眼做判断的。
    /// 它与上面的 `entitlements` 回答的是**两件不相干的事**：账号状态只由登录态决定，
    /// 权益是并列的增量信息。列表此前只有一个账号级的聚合布尔值（`vip_active`），
    /// 把「活着吗」和「有权益吗」混成了同一个问题的答案，两项现在各由自己的字段回答。
    pub account_status: String,
    /// 账号最近一次刷新成功的时刻。
    ///
    /// 不能复用下面的 `updated_at`：`refresh_source()` 只写 account 侧的时间戳，
    /// 从不写 `Source.updated_at`，所以源记录的时间在刷新后纹丝不动，
    /// 用户点完刷新看不到任何变化。两个字段语义不同，界面展示刷新结果时必须用这一个。
    pub last_refresh_at: Option<i64>,
    /// 源记录的更新时间（绑定/改配置时变动），保留以维持 API 兼容，界面不再展示。
    pub updated_at: i64,
    /// 音质自动降级开关的当前值（即 `Source.quality_fallback`）。
    ///
    /// 列表页要直接渲染开关状态，若只放在详情里，扫一眼列表无法判断哪些音源开了
    /// 这个会改变实际返回音质的行为。
    pub quality_fallback: bool,
}

#[derive(serde::Serialize)]
pub struct SourceDetail {
    pub source_id: String,
    pub account_id: String,
    pub provider: String,
    pub script_url: String,
    /// 当前生效的标题（即 `Source.name`）。
    pub name: String,
    /// 系统默认标题，供后台「恢复默认」使用。
    pub default_name: String,
    /// 官方对标题的**建议**字符数上限，仅供后台计数提示，不参与拒绝逻辑。
    pub name_advisory_chars: usize,
    /// 音质自动降级开关的当前值；详情页据此渲染开关初态。
    pub quality_fallback: bool,
    pub runtime_token_preview: String,
    pub account: SourceAccountInfo,
}

#[derive(serde::Serialize)]
pub struct UpdateSourceResponse {
    pub ok: bool,
    pub source_id: String,
    pub name: String,
    pub default_name: String,
    /// 回显开关的**当前**值：请求里没带这个字段时也必须有值，否则界面上的开关会
    /// 在一次只改标题的 PATCH 之后被渲染回默认态。
    pub quality_fallback: bool,
    pub updated_at: i64,
}

/// 详情页里的账号信息。
///
/// 会员权益**只有 `entitlements` 这一个口径**：账号级的聚合布尔值（`vip_active`）
/// 与它的同族 `vip_type` 都已删除——权益本就是并列的增量信息，聚合值只是它的派生品，
/// 既冗余又会在上游新增一档权益时静默失真。
#[derive(serde::Serialize)]
pub struct SourceAccountInfo {
    pub userid: String,
    /// 上游会员权益明细（逐条 `product_type` + 是否在期 + 起止时刻），与列表
    /// （`SourceListItem`）用的是**同一个结构体**，见那里的说明。
    ///
    /// 空数组是**正常状态**（这个账号一条权益都没有），不是「查不到上游」；两者在
    /// 这里可区分，因为后者走的是 `UPSTREAM_VIP_CHECK_FAILED` 错误路径，根本不会有
    /// 这一次响应。
    pub entitlements: Vec<VipEntitlement>,
    pub status: String,
    pub last_refresh_at: Option<i64>,
    pub last_error: Option<String>,
    /// 与 `last_error` 成对：界面按这个稳定 code 查中文文案，`last_error` 只作原文兜底。
    pub last_error_code: Option<String>,
}

/// 手动刷新的回执。
///
/// 只回执**这次动作本身**的结果（`ok` / 时间戳），不回执权益：刷新成功与「有没有
/// 权益」是两件事，权益明细由列表与详情接口给出（口径已经统一在那里），在这里再
/// 回一个聚合布尔值只会多出第二个口径。
#[derive(serde::Serialize)]
pub struct RefreshResponse {
    pub ok: bool,
    pub source_id: String,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub struct SourceLogsResponse {
    pub source_id: String,
    pub view: RuntimeLogView,
    pub limit: usize,
    pub items: Vec<RuntimeLog>,
}

/// 音源 PATCH 的输入。
///
/// 与路由层的 `UpdateSourceBody` 一一对应：字段的外层 `None` 表示「请求里没提这个
/// 字段，保持原值」，而不是「把它设成某个默认值」。两个字段分别对应一个可独立
/// 修改的配置项，任一为 `None` 都不影响另一个。
pub struct UpdateSourceInput {
    /// `None` = 不改标题；`Some(None)` / `Some(Some(空白))` = 恢复默认标题；`Some(Some(raw))` = 清洗后写入。
    pub name: Option<Option<String>>,
    /// `None` = 不改开关。
    pub quality_fallback: Option<bool>,
}

pub struct SourceService {
    config: Arc<Config>,
    client: Arc<KugouLiteClient>,
    account_repo: Arc<dyn AccountRepo>,
    source_repo: Arc<dyn SourceRepo>,
    log_repo: Arc<dyn RuntimeLogRepo>,
}

impl SourceService {
    pub fn new(
        config: Arc<Config>,
        client: Arc<KugouLiteClient>,
        account_repo: Arc<dyn AccountRepo>,
        source_repo: Arc<dyn SourceRepo>,
        log_repo: Arc<dyn RuntimeLogRepo>,
    ) -> Self {
        Self {
            config,
            client,
            account_repo,
            source_repo,
            log_repo,
        }
    }

    pub async fn list_sources(&self) -> Result<Vec<SourceListItem>, AppError> {
        let sources = self.source_repo.list_all().await?;
        let mut items = Vec::new();
        for s in sources {
            let account = self.account_repo.find_by_id(&s.account_id).await?;
            // 账号缺失时状态写空串：前端 labelOf / accountStatusTone 已有回退，
            // 不为此新增枚举值，避免污染两个穷举词典。
            let (userid, entitlements, account_status, last_refresh_at) = match account {
                Some(ref a) => (
                    a.upstream_userid.clone(),
                    a.entitlements.clone(),
                    a.status.as_str().to_string(),
                    a.last_refresh_at,
                ),
                None => (String::new(), Vec::new(), String::new(), None),
            };
            let script_url = format!("{}/s/{}.js", self.config.public_base_url, s.script_token);
            items.push(SourceListItem {
                source_id: s.source_id,
                account_id: s.account_id,
                provider: s.provider.to_string(),
                script_url,
                name: s.name,
                default_name: source::default_name(&userid),
                userid,
                entitlements,
                account_status,
                last_refresh_at,
                updated_at: s.updated_at,
                quality_fallback: s.quality_fallback,
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
            script_url,
            name: s.name,
            default_name: source::default_name(&account.upstream_userid),
            name_advisory_chars: SCRIPT_NAME_ADVISORY_CHARS,
            quality_fallback: s.quality_fallback,
            runtime_token_preview: rt_preview,
            account: SourceAccountInfo {
                userid: account.upstream_userid,
                entitlements: account.entitlements,
                status: account.status.as_str().to_string(),
                last_refresh_at: account.last_refresh_at,
                last_error: account.last_error,
                last_error_code: account.last_error_code,
            },
        })
    }

    /// 修改音源的可配置项：标题与「音质自动降级」开关。
    ///
    /// 标题语义分档，且**缺失与 `null` 是两件事**：
    /// - `name: None` => 标题**原样保留**。这是本端点从「改名端点」变成通用 PATCH 的
    ///   核心：只改开关的请求绝不能碰到标题，否则用户的自定义标题会被静默重置；
    /// - `name: Some(None)` 或 `Some(Some(raw))` 且 `raw.trim()` 为空 => 恢复服务端
    ///   默认标题（实例前缀 + userid）；
    /// - `name: Some(Some(raw))` 非空但清洗后为空（例如只填了 `*/`）=> 400，而不是
    ///   静默变成默认名，否则用户会被无声地「改名成功」误导；
    /// - 清洗后超过 [`SCRIPT_NAME_MAX_BYTES`] => 400。
    ///
    /// 写入的是清洗后的值，因此 `Source.name` 永远不含 `*/`；渲染路径再洗一遍
    /// 只是纵深防御，而非唯一防线。注意建议长度（`SCRIPT_NAME_ADVISORY_CHARS`）
    /// 不参与拒绝逻辑，只由列表/详情回显给后台做计数提示。
    ///
    /// 开关语义：`quality_fallback: None` = 不改；`Some(v)` = 直接写入 `v`。
    /// 两个字段都是 `None` 时**完全不写盘**——见下面的注释。
    pub async fn update_source(
        &self,
        source_id: &str,
        input: UpdateSourceInput,
    ) -> Result<UpdateSourceResponse, AppError> {
        let mut s = self
            .source_repo
            .find_by_id(source_id)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        // userid 仅用于生成默认名；查不到就降级为空串，`default_name` 会省略尾随空格。
        // 无论本次改不改标题都要算出它：响应必须始终带一个正确的 `default_name`。
        let userid = match self.account_repo.find_by_id(&s.account_id).await {
            Ok(Some(account)) => account.upstream_userid,
            Ok(None) => String::new(),
            Err(err) => {
                tracing::warn!(
                    account_id = %s.account_id,
                    error = %err,
                    "default name degraded: userid lookup failed",
                );
                String::new()
            }
        };
        let default_name = source::default_name(&userid);

        // 标题先解析完再落盘：清洗失败要在写入任何字段之前返回 400，绝不允许
        // 「开关写进去了、标题报 400」这种半成功状态。
        let new_name = match input.name {
            // 请求没提标题 => 保持存储值不动，而不是「恢复默认名」。
            None => s.name.clone(),
            Some(None) => default_name.clone(),
            Some(Some(ref raw)) if raw.trim().is_empty() => default_name.clone(),
            Some(Some(ref raw)) => {
                let cleaned = sanitize_script_meta(raw);
                if cleaned.is_empty() {
                    return Err(AppError::source_name_empty());
                }
                if cleaned.len() > SCRIPT_NAME_MAX_BYTES {
                    return Err(AppError::source_name_too_long(
                        cleaned.len(),
                        SCRIPT_NAME_MAX_BYTES,
                    ));
                }
                cleaned
            }
        };

        // 两个字段都没出现在请求里 => 空操作，**不 upsert**。此时唯一会变的只有
        // `updated_at`，而界面拿它回答「这个音源刚被改过吗」：空推一次时间戳会把
        // 一次什么都没做的请求显示成一次修改，也让详情页的「保存成功」看起来成立。
        // 原样回传当前的 `updated_at`，前端据此不会刷新时间。
        if input.name.is_none() && input.quality_fallback.is_none() {
            return Ok(UpdateSourceResponse {
                ok: true,
                source_id: s.source_id,
                name: s.name,
                default_name,
                quality_fallback: s.quality_fallback,
                updated_at: s.updated_at,
            });
        }

        s.name = new_name;
        if let Some(value) = input.quality_fallback {
            s.quality_fallback = value;
        }
        s.updated_at = source::now_ts();
        self.source_repo.upsert(&s).await?;

        Ok(UpdateSourceResponse {
            ok: true,
            source_id: s.source_id,
            name: s.name,
            default_name,
            quality_fallback: s.quality_fallback,
            updated_at: s.updated_at,
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
            // 权益明细**原样**落到账号上，包括空列表：空的意思是「这个账号目前一条
            // 权益都没有」，而它是一次成功的查询结果，不是失败。真正问不到上游时
            // 上面那行已经以 `UPSTREAM_VIP_CHECK_FAILED` 返回了，两条路径在这里就
            // 分开了——不要给空列表加「保留上一次的权益」之类的兜底，那会让界面
            // 继续显示一份已经不成立的权益。
            account.entitlements = vip.entitlements;

            let now = account::now_ts();
            account.last_refresh_at = Some(now);
            account.last_success_at = Some(now);
            // 与 runtime.rs 的刷新路径同规则：成功时 code 必须跟着 message 一起清，
            // 否则界面会把上一次失败的 code 翻译成一条已经不成立的错误标签。
            account.last_error = None;
            account.last_error_code = None;
            // 账号状态只由**登录态**决定，与 runtime.rs 同一约定：能走到这里就说明
            // 刷新成功了。权益在不在期由 `entitlements` 表达。
            account.status = AccountStatus::Active;
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
                // 与 runtime.rs 同一约定：code 供界面查中文，message 只留原文，
                // 不再带 `"CODE: "` 前缀。
                account.last_error_code = Some(err.code.as_str().to_string());
                account.last_error = Some(err.message.clone());
                account.updated_at = now;
                let _ = self.account_repo.upsert(&account).await;
                return Err(err);
            }
        };

        Ok(RefreshResponse {
            ok: true,
            source_id: source_id.into(),
            updated_at: now,
        })
    }

    pub async fn list_source_logs(
        &self,
        source_id: &str,
        limit: usize,
        view: RuntimeLogView,
    ) -> Result<SourceLogsResponse, AppError> {
        self.source_repo
            .find_by_id(source_id)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        let query = ListRuntimeLogsQuery {
            source_id: source_id.into(),
            limit,
            view,
        };
        let items = self.log_repo.list_by_source(&query).await?;

        Ok(SourceLogsResponse {
            source_id: source_id.into(),
            view: query.view,
            limit: query.limit,
            items: items.into_iter().map(RuntimeLog::sanitized).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    use crate::domain::account::ProviderAccount;
    use crate::domain::cookie_store::CookieStore;
    use crate::domain::provider::ProviderKind;
    use crate::domain::runtime_log::ListRuntimeLogsQuery;
    use crate::domain::source::Source;

    // ---- 测试替身 ----
    //
    // `update_source` 只在「查音源 → 查账号 → 写音源」这三步里活动，没有任何上游
    // 调用，所以这些测试不需要网络。刻意不用真实 `Fs*` 仓库：那要读写临时目录，
    // 会把「到底写没写」这个断言埋进文件系统噪声里。

    /// 把 `upsert` 写进来的东西留一份。
    ///
    /// 与 `services/runtime.rs` 的同名替身关键差别有两处：`find_by_id` 必须真的
    /// 返回音源（更新路径是先查后写），`upsert` 必须能被**计数**——「两个字段都
    /// 没传时不得写盘」这条断言的全部意义就在计数上。
    struct RecordingSourceRepo {
        source: Option<Source>,
        upserts: Mutex<Vec<Source>>,
    }

    impl RecordingSourceRepo {
        fn new(source: Source) -> Self {
            Self {
                source: Some(source),
                upserts: Mutex::new(Vec::new()),
            }
        }

        /// 已落盘的写入，按发生顺序。
        fn upserts(&self) -> Vec<Source> {
            self.upserts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SourceRepo for RecordingSourceRepo {
        async fn find_by_id(&self, _source_id: &str) -> Result<Option<Source>, AppError> {
            Ok(self.source.clone())
        }

        async fn find_by_script_token(&self, _token: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn find_by_runtime_token(&self, _token: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn find_by_account_id(&self, _account_id: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn upsert(&self, source: &Source) -> Result<(), AppError> {
            self.upserts.lock().unwrap().push(source.clone());
            Ok(())
        }

        async fn list_all(&self) -> Result<Vec<Source>, AppError> {
            Ok(self.source.clone().into_iter().collect())
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

    /// 本模块不查日志，只需要一个「查了也不炸」的空实现。
    #[derive(Default)]
    struct EmptyLogRepo;

    #[async_trait]
    impl RuntimeLogRepo for EmptyLogRepo {
        async fn append(&self, _log: &RuntimeLog) -> Result<(), AppError> {
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
            // 更新路径不该发出任何上游请求。端口取 1 是有意为之：万一哪天真的发了，
            // 连不上会比「连上了测试环境」更早、更明显地失败。
            kugou_api_base_url: "http://127.0.0.1:1".into(),
            data_dir: "./data".into(),
            upstream_timeout_ms: 1_000,
            account_refresh_ttl: 6 * 3600,
            web_dist_dir: None,
        })
    }

    const TEST_USERID: &str = "userid_test";

    fn sample_source() -> Source {
        Source {
            source_id: "src_test".into(),
            account_id: "acc_test".into(),
            provider: ProviderKind::KugouLite,
            name: "我的自定义标题".into(),
            script_token: "script_test".into(),
            runtime_token: "runtime_test".into(),
            // 取一个非 0 的小值：`updated_at` 必须能被断言「没被动过」，
            // 也要能与 `now_ts()` 写出的真实时间戳区分开。
            created_at: 100,
            updated_at: 100,
            quality_fallback: false,
        }
    }

    /// 一个**一条权益都没有**的账号（`entitlements` 为空），账号状态正常。
    ///
    /// 「没有权益」不再是账号上的一列聚合值（`vip_active` / `vip_type` 已删除），
    /// 它现在的表述就是这里的空权益列表。
    fn sample_account() -> ProviderAccount {
        ProviderAccount {
            account_id: "acc_test".into(),
            provider: ProviderKind::KugouLite,
            upstream_userid: TEST_USERID.into(),
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

    fn build_service(source: Source) -> (SourceService, Arc<RecordingSourceRepo>) {
        build_service_with_account(source, sample_account())
    }

    /// 同上，但账号由调用方给出：权益明细的曝光测试需要账号真的带着权益。
    fn build_service_with_account(
        source: Source,
        account: ProviderAccount,
    ) -> (SourceService, Arc<RecordingSourceRepo>) {
        let config = test_config();
        let client = Arc::new(KugouLiteClient::new(&config));
        let source_repo = Arc::new(RecordingSourceRepo::new(source));
        let account_repo: Arc<dyn AccountRepo> = Arc::new(FakeAccountRepo {
            account: Some(account),
        });
        let log_repo: Arc<dyn RuntimeLogRepo> = Arc::new(EmptyLogRepo);
        let service =
            SourceService::new(config, client, account_repo, source_repo.clone(), log_repo);
        (service, source_repo)
    }

    /// 只改开关时，标题必须**原样保留**。
    ///
    /// 这正是把 `name` 从 `Option<String>` 改成 `Option<Option<String>>` 要保住的
    /// 行为：旧类型下「请求没提标题」与「请求显式 null」不可区分，于是只想拨一下
    /// 开关的用户会发现自己的自定义标题被静默重置成默认名。
    #[tokio::test]
    async fn only_switching_quality_fallback_leaves_title_untouched() {
        let (service, repo) = build_service(sample_source());

        let resp = service
            .update_source(
                "src_test",
                UpdateSourceInput {
                    name: None,
                    quality_fallback: Some(true),
                },
            )
            .await
            .expect("只改开关必须成功");

        assert_eq!(resp.name, "我的自定义标题", "标题不得被这次请求碰到");
        assert!(resp.quality_fallback, "回显的必须是写入后的值");
        assert_eq!(resp.default_name, format!("酷狗概念版 {TEST_USERID}"));

        let writes = repo.upserts();
        assert_eq!(writes.len(), 1, "有字段变更就必须恰好写一次");
        assert_eq!(writes[0].name, "我的自定义标题");
        assert!(writes[0].quality_fallback);
    }

    /// 空 PATCH（`{}`）是空操作：不得 `upsert`，`updated_at` 不得变。
    ///
    /// 界面用 `updated_at` 回答「这个音源刚被改过吗」。空推一次时间戳会把一次
    /// 什么都没做的请求显示成一次修改。
    #[tokio::test]
    async fn empty_patch_does_not_write() {
        let (service, repo) = build_service(sample_source());

        let resp = service
            .update_source(
                "src_test",
                UpdateSourceInput {
                    name: None,
                    quality_fallback: None,
                },
            )
            .await
            .expect("空 PATCH 是合法请求");

        assert!(
            repo.upserts().is_empty(),
            "没有任何字段变更时不得写盘，否则会空推 updated_at"
        );
        assert_eq!(resp.updated_at, 100, "updated_at 必须原样回传");
        assert_eq!(resp.name, "我的自定义标题");
        assert!(!resp.quality_fallback);
    }

    /// 显式 `null` 才是「恢复默认标题」——且恢复之后 `updated_at` 要真的动。
    #[tokio::test]
    async fn explicit_null_name_restores_default() {
        let (service, repo) = build_service(sample_source());

        let resp = service
            .update_source(
                "src_test",
                UpdateSourceInput {
                    name: Some(None),
                    quality_fallback: None,
                },
            )
            .await
            .expect("恢复默认名必须成功");

        let expected = format!("酷狗概念版 {TEST_USERID}");
        assert_eq!(resp.name, expected);
        assert_eq!(resp.default_name, expected);
        assert!(resp.updated_at > 100, "真的改了东西，时间戳必须往后走");

        let writes = repo.upserts();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].name, expected);
        assert!(
            !writes[0].quality_fallback,
            "没提开关就必须保持存储值（false），不能被顺手重置"
        );
    }

    /// 两个字段同时提供时在**一次**写入里落地，不产生两次落盘。
    #[tokio::test]
    async fn title_and_switch_are_applied_in_one_write() {
        let source = Source {
            quality_fallback: true,
            ..sample_source()
        };
        let (service, repo) = build_service(source);

        let resp = service
            .update_source(
                "src_test",
                UpdateSourceInput {
                    name: Some(Some("新标题".into())),
                    quality_fallback: Some(false),
                },
            )
            .await
            .expect("同时改两项必须成功");

        assert_eq!(resp.name, "新标题");
        assert!(!resp.quality_fallback);

        let writes = repo.upserts();
        assert_eq!(writes.len(), 1, "两项改动必须合并为一次 upsert");
        assert_eq!(writes[0].name, "新标题");
        assert!(!writes[0].quality_fallback);
    }

    /// 空白串走的是「恢复默认名」这一档，与改动前的语义一致——它不是 400，
    /// 也不是把空串写进 `Source.name`。
    #[tokio::test]
    async fn blank_name_restores_default() {
        let (service, repo) = build_service(sample_source());

        let resp = service
            .update_source(
                "src_test",
                UpdateSourceInput {
                    name: Some(Some("   ".into())),
                    quality_fallback: None,
                },
            )
            .await
            .expect("纯空白等同恢复默认名");

        let expected = format!("酷狗概念版 {TEST_USERID}");
        assert_eq!(resp.name, expected);
        assert_eq!(repo.upserts().len(), 1);
        assert_eq!(repo.upserts()[0].name, expected);
    }

    /// 新增字段必须出现在列表与详情响应里：界面据此渲染开关初态，
    /// 少一个字段会让开关永远显示为关。
    #[tokio::test]
    async fn list_and_detail_echo_quality_fallback() {
        let source = Source {
            quality_fallback: true,
            ..sample_source()
        };
        let (service, _repo) = build_service(source);

        let list = service.list_sources().await.expect("list");
        assert_eq!(list.len(), 1);
        assert!(list[0].quality_fallback, "列表必须回显开关当前值");

        let detail = service.get_source("src_test").await.expect("detail");
        assert!(detail.quality_fallback, "详情必须回显开关当前值");
    }

    /// 权益明细在**列表与详情上是同一份数据**，且账号级的两个聚合列
    /// （`vip_active` / `vip_type`）不得以任何形式回到 wire 上。
    ///
    /// 断言落在序列化后的 JSON 上而不是结构体的字段上：「列表带不带权益明细」
    /// 只有在 wire 上才有意义（结构体里多加一个字段、只要它没被序列化出去，用户的
    /// 列表请求就没变重）。这条同时钉住两件事——列表**必须**带明细（此前那里只有一个
    /// 聚合布尔值回答「还有没有权益」，那个值已删除，现在由有名字的权益回答），
    /// 以及两个聚合列**不得**复活。
    ///
    /// 列表与详情各自的那一份必须逐字相等：两处口径一旦分叉，用户会看到两个页面对
    /// 同一个账号给出不同的答案，而没有任何一层会报错。
    #[tokio::test]
    async fn entitlements_are_exposed_in_both_the_list_and_the_detail() {
        let account = ProviderAccount {
            entitlements: vec![
                VipEntitlement {
                    product_type: "svip".into(),
                    active: true,
                    begin_at: Some(1700000000),
                    end_at: Some(1702678400),
                },
                VipEntitlement {
                    product_type: "tvip".into(),
                    active: false,
                    begin_at: None,
                    end_at: None,
                },
            ],
            ..sample_account()
        };
        let (service, _repo) = build_service_with_account(sample_source(), account);

        // 「上游没说」的时间必须是 `null` 而不是 0（那是 1970 年）；这条由下面的
        // 整体相等断言一并钉住。
        let expected = serde_json::json!([
            {
                "product_type": "svip",
                "active": true,
                "begin_at": 1700000000,
                "end_at": 1702678400,
            },
            {
                "product_type": "tvip",
                "active": false,
                "begin_at": null,
                "end_at": null,
            },
        ]);

        let list = serde_json::to_value(service.list_sources().await.expect("list")).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(
            list[0]["entitlements"], expected,
            "列表必须带权益明细：扫一眼列表要能看出这个账号有哪些权益"
        );

        let detail =
            serde_json::to_value(service.get_source("src_test").await.expect("detail")).unwrap();
        assert_eq!(
            detail["account"]["entitlements"], expected,
            "详情与列表必须是同一份数据，口径不得分叉"
        );

        for (label, payload) in [("列表", &list[0]), ("详情账号", &detail["account"])] {
            for column in ["vip_active", "vip_type"] {
                assert!(
                    payload.get(column).is_none(),
                    "{label}上不该再有 {column}：权益的唯一口径是 entitlements"
                );
            }
        }
    }
}
