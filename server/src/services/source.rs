use std::sync::Arc;

use crate::domain::source::{self, SCRIPT_NAME_MAX_BYTES, sanitize_script_meta};
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::source::SourceRepo;

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
    account_repo: Arc<dyn AccountRepo>,
    source_repo: Arc<dyn SourceRepo>,
}

impl SourceService {
    pub fn new(account_repo: Arc<dyn AccountRepo>, source_repo: Arc<dyn SourceRepo>) -> Self {
        Self {
            account_repo,
            source_repo,
        }
    }

    pub async fn update_source(
        &self,
        source_id: &str,
        input: UpdateSourceInput,
    ) -> Result<UpdateSourceResponse, AppError> {
        let s = self
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
        let name = match input.name {
            None => None,
            Some(None) => Some(default_name.clone()),
            Some(Some(ref raw)) if raw.trim().is_empty() => Some(default_name.clone()),
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
                Some(cleaned)
            }
        };
        let s = if name.is_some() || input.quality_fallback.is_some() {
            self.source_repo
                .update_settings(
                    &s.source_id,
                    name,
                    input.quality_fallback,
                    crate::domain::now_ts(),
                )
                .await?
        } else {
            s
        };

        Ok(UpdateSourceResponse {
            ok: true,
            source_id: s.source_id,
            name: s.name,
            default_name,
            quality_fallback: s.quality_fallback,
            updated_at: s.updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::{AccountStatus, ProviderAccount};
    use crate::domain::cookie_store::CookieStore;
    use crate::domain::provider::ProviderKind;
    use crate::domain::source::Source;
    use async_trait::async_trait;
    use std::sync::Mutex;

    // ---- 测试替身 ----
    //
    // `update_source` 只在「查音源 → 查账号 → 写音源」这三步里活动，没有任何上游
    // 调用，所以这些测试不需要网络。刻意用内存替身：
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

        async fn update_settings(
            &self,
            _source_id: &str,
            name: Option<String>,
            quality_fallback: Option<bool>,
            updated_at: i64,
        ) -> Result<Source, AppError> {
            let mut writes = self.upserts.lock().unwrap();
            let mut source = writes.last().or(self.source.as_ref()).unwrap().clone();
            if let Some(name) = name {
                source.name = name;
            }
            if let Some(quality_fallback) = quality_fallback {
                source.quality_fallback = quality_fallback;
            }
            source.updated_at = updated_at;
            writes.push(source.clone());
            Ok(source)
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

    fn build_service(source: Source) -> (SourceService, Arc<RecordingSourceRepo>) {
        let source_repo = Arc::new(RecordingSourceRepo::new(source));
        let account_repo = Arc::new(FakeAccountRepo {
            account: Some(sample_account()),
        });
        (
            SourceService::new(account_repo, source_repo.clone()),
            source_repo,
        )
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
}
