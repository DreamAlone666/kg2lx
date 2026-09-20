use std::sync::Arc;

use crate::config::Config;
use crate::domain::account::{self, AccountStatus};
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
    pub enabled: bool,
    pub script_url: String,
    /// 当前生效的标题（即 `Source.name`），原样返回以便后台显示计数与超长提示。
    pub name: String,
    /// 系统默认标题，供后台展示「默认：酷狗概念版 12345678」与一键恢复。
    /// `userid` 缺失时返回去掉尾随空格的结果，不会产生「酷狗概念版 」这类文案。
    pub default_name: String,
    pub userid: String,
    pub vip_active: bool,
    /// 账号状态，与详情页 `account.status` 同源同格式。
    ///
    /// 列表此前只有 `vip_active`，导致「账号还活着吗」这个最核心的问题必须逐个点进详情
    /// 才能回答；而列表本来就是用来扫一眼做判断的。
    pub account_status: String,
    /// 账号最近一次刷新成功的时刻。
    ///
    /// 不能复用下面的 `updated_at`：`refresh_source()` 只写 account 侧的时间戳，
    /// 从不写 `Source.updated_at`，所以源记录的时间在刷新后纹丝不动，
    /// 用户点完刷新看不到任何变化。两个字段语义不同，界面展示刷新结果时必须用这一个。
    pub last_refresh_at: Option<i64>,
    /// 源记录的更新时间（绑定/改配置时变动），保留以维持 API 兼容，界面不再展示。
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub struct SourceDetail {
    pub source_id: String,
    pub account_id: String,
    pub provider: String,
    pub enabled: bool,
    pub script_url: String,
    /// 当前生效的标题（即 `Source.name`）。
    pub name: String,
    /// 系统默认标题，供后台「恢复默认」使用。
    pub default_name: String,
    /// 官方对标题的**建议**字符数上限，仅供后台计数提示，不参与拒绝逻辑。
    pub name_advisory_chars: usize,
    pub runtime_token_preview: String,
    pub account: SourceAccountInfo,
}

#[derive(serde::Serialize)]
pub struct UpdateSourceResponse {
    pub ok: bool,
    pub source_id: String,
    pub name: String,
    pub default_name: String,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub struct SourceAccountInfo {
    pub userid: String,
    pub vip_active: bool,
    pub vip_type: i32,
    pub status: String,
    pub last_refresh_at: Option<i64>,
    pub last_error: Option<String>,
    /// 与 `last_error` 成对：界面按这个稳定 code 查中文文案，`last_error` 只作原文兜底。
    pub last_error_code: Option<String>,
}

#[derive(serde::Serialize)]
pub struct RefreshResponse {
    pub ok: bool,
    pub source_id: String,
    pub vip_active: bool,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub struct SourceLogsResponse {
    pub source_id: String,
    pub view: RuntimeLogView,
    pub limit: usize,
    pub items: Vec<RuntimeLog>,
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
            let (userid, vip_active, account_status, last_refresh_at) = match account {
                Some(ref a) => (
                    a.upstream_userid.clone(),
                    a.vip_active,
                    a.status.as_str().to_string(),
                    a.last_refresh_at,
                ),
                None => (String::new(), false, String::new(), None),
            };
            let script_url = format!("{}/s/{}.js", self.config.public_base_url, s.script_token);
            items.push(SourceListItem {
                source_id: s.source_id,
                account_id: s.account_id,
                provider: s.provider.to_string(),
                enabled: s.enabled,
                script_url,
                name: s.name,
                default_name: source::default_name(&userid),
                userid,
                vip_active,
                account_status,
                last_refresh_at,
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
            name: s.name,
            default_name: source::default_name(&account.upstream_userid),
            name_advisory_chars: SCRIPT_NAME_ADVISORY_CHARS,
            runtime_token_preview: rt_preview,
            account: SourceAccountInfo {
                userid: account.upstream_userid,
                vip_active: account.vip_active,
                vip_type: account.vip_type,
                status: account.status.as_str().to_string(),
                last_refresh_at: account.last_refresh_at,
                last_error: account.last_error,
                last_error_code: account.last_error_code,
            },
        })
    }

    /// 修改音源标题。
    ///
    /// 语义分三档：
    /// - `None` 或纯空白 => 恢复服务端默认标题（实例前缀 + userid）；
    /// - 非空但清洗后为空（例如只填了 `*/`）=> 400，而不是静默变成默认名，
    ///   否则用户会被无声地「改名成功」误导；
    /// - 清洗后超过 [`SCRIPT_NAME_MAX_BYTES`] => 400。
    ///
    /// 写入的是清洗后的值，因此 `Source.name` 永远不含 `*/`；渲染路径再洗一遍
    /// 只是纵深防御，而非唯一防线。注意建议长度（`SCRIPT_NAME_ADVISORY_CHARS`）
    /// 不参与拒绝逻辑，只由列表/详情回显给后台做计数提示。
    pub async fn update_source_name(
        &self,
        source_id: &str,
        name: Option<String>,
    ) -> Result<UpdateSourceResponse, AppError> {
        let mut s = self
            .source_repo
            .find_by_id(source_id)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        // userid 仅用于生成默认名；查不到就降级为空串，`default_name` 会省略尾随空格。
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

        let new_name = match name {
            None => default_name.clone(),
            Some(ref raw) if raw.trim().is_empty() => default_name.clone(),
            Some(ref raw) => {
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

        s.name = new_name;
        s.updated_at = source::now_ts();
        self.source_repo.upsert(&s).await?;

        Ok(UpdateSourceResponse {
            ok: true,
            source_id: s.source_id,
            name: s.name,
            default_name,
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
            account.vip_type = vip.vip_type;
            account.vip_active = vip.vip_active;

            let now = account::now_ts();
            account.last_refresh_at = Some(now);
            account.last_success_at = Some(now);
            // 与 runtime.rs 的刷新路径同规则：成功时 code 必须跟着 message 一起清，
            // 否则界面会把上一次失败的 code 翻译成一条已经不成立的错误标签。
            account.last_error = None;
            account.last_error_code = None;
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
            vip_active: account.vip_active,
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
