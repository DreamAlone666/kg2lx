use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use crate::app_state::AppState;
use crate::config::Config;
use crate::domain::event_log::{EventType, ListGlobalEvents, LogView};
use crate::domain::renewal::RenewalStatus;
use crate::error::AppError;
use crate::services::account::{
    AccountItem, DeleteAccountResponse, RefreshResponse, SyncEntitlementsResponse,
};
use crate::services::event_log::GlobalLogsResponse;
use crate::services::login::{QrLoginPollResponse, QrLoginStartResponse};
use crate::services::source::{RotateTokensResponse, UpdateSourceInput, UpdateSourceResponse};

fn verify_admin(headers: &HeaderMap, token: &str) -> Result<(), AppError> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let bearer = auth.strip_prefix("Bearer ").unwrap_or("");
    if !token.is_empty() && bool::from(bearer.as_bytes().ct_eq(token.as_bytes())) {
        Ok(())
    } else {
        Err(AppError::admin_unauthorized())
    }
}

pub async fn require_admin(
    State(config): State<Arc<Config>>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    verify_admin(request.headers(), &config.admin_token)?;
    Ok(next.run(request).await)
}

pub async fn start_qr_login(
    State(state): State<AppState>,
) -> Result<Json<QrLoginStartResponse>, AppError> {
    Ok(Json(state.login.start_kugou_lite_qr_login().await?))
}

pub async fn poll_qr_login(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<QrLoginPollResponse>, AppError> {
    Ok(Json(
        state.login.poll_kugou_lite_qr_login(&session_id).await?,
    ))
}

#[derive(serde::Serialize)]
pub struct AccountList {
    items: Vec<AccountItem>,
}

pub async fn list_accounts(State(state): State<AppState>) -> Result<Json<AccountList>, AppError> {
    Ok(Json(AccountList {
        items: state.accounts.list().await?,
    }))
}

pub async fn get_account(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<AccountItem>, AppError> {
    Ok(Json(state.accounts.get(&account_id).await?))
}

pub async fn delete_account(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<DeleteAccountResponse>, AppError> {
    Ok(Json(state.accounts.delete(account_id).await?))
}

pub async fn refresh_account_token(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<RefreshResponse>, AppError> {
    Ok(Json(state.accounts.refresh(&account_id).await?))
}

pub async fn sync_account_entitlements(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<SyncEntitlementsResponse>, AppError> {
    Ok(Json(state.accounts.sync(account_id).await?))
}

#[derive(serde::Deserialize)]
pub struct RenewalSettingsBody {
    enabled: bool,
}

pub async fn set_auto_claim(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Json(body): Json<RenewalSettingsBody>,
) -> Result<Json<RenewalStatus>, AppError> {
    Ok(Json(
        state
            .accounts
            .set_auto_claim(&account_id, body.enabled)
            .await?,
    ))
}

pub async fn claim_account_entitlements(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<RenewalStatus>, AppError> {
    Ok(Json(state.accounts.claim(account_id).await?))
}

#[derive(serde::Deserialize)]
pub struct UpdateSourceBody {
    /// 缺失 = 不改；null / 纯空白 = 恢复默认标题；非空 = 设为该值（写入前清洗）。
    ///
    /// 非空但清洗后为空（例如只填了 `*/`）仍然 400，而不是静默变成默认名，
    /// 否则用户会被无声地「改名成功」误导。
    #[serde(default, deserialize_with = "present")]
    pub name: Option<Option<String>>,
    /// 缺失 = 不改。
    #[serde(default)]
    pub quality_fallback: Option<bool>,
}

/// `deserialize_with` 的辅助：字段**出现**（哪怕是 `null`）即包一层 `Some`。
/// 配合字段上的 `#[serde(default)]`，`{}`（字段缺失）与 `{"name": null}` 于是变得可区分。
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

pub async fn update_source(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    Json(body): Json<UpdateSourceBody>,
) -> Result<Json<UpdateSourceResponse>, AppError> {
    let input = UpdateSourceInput {
        name: body.name,
        quality_fallback: body.quality_fallback,
    };
    Ok(Json(state.source.update_source(&source_id, input).await?))
}

pub async fn rotate_source_tokens(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<RotateTokensResponse>, AppError> {
    Ok(Json(state.source.rotate_tokens(&source_id).await?))
}

#[derive(serde::Deserialize)]
pub struct ListGlobalLogsParams {
    limit: Option<String>,
    view: Option<String>,
    event_type: Option<String>,
    account_id: Option<String>,
    source_id: Option<String>,
    correlation_id: Option<String>,
    before_seq: Option<String>,
    after_seq: Option<String>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

fn cursor(value: Option<String>, field: &str) -> Result<Option<i64>, AppError> {
    value
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| AppError::invalid_request(format!("invalid {field}")))
        })
        .transpose()
}

impl TryFrom<ListGlobalLogsParams> for ListGlobalEvents {
    type Error = AppError;

    fn try_from(params: ListGlobalLogsParams) -> Result<Self, AppError> {
        let limit = match params.limit.as_deref() {
            None => 30,
            Some(value) => value
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=100).contains(n))
                .ok_or_else(|| AppError::invalid_request("invalid limit"))?,
        };
        let view = match params.view.as_deref() {
            None | Some("all") => LogView::All,
            Some("errors") => LogView::Errors,
            _ => return Err(AppError::invalid_request("invalid view")),
        };
        let event_type = match non_empty(params.event_type).as_deref() {
            None => None,
            Some("music_url") => Some(EventType::MusicUrl),
            Some("token_refresh") => Some(EventType::TokenRefresh),
            Some("account_login") => Some(EventType::AccountLogin),
            Some("entitlement") => Some(EventType::Entitlement),
            Some(_) => return Err(AppError::invalid_request("invalid event_type")),
        };
        let before_seq = cursor(params.before_seq, "before_seq")?;
        let after_seq = cursor(params.after_seq, "after_seq")?;
        let correlation_id = non_empty(params.correlation_id);
        if before_seq.is_some() && after_seq.is_some() {
            return Err(AppError::invalid_request(
                "before_seq and after_seq cannot be combined",
            ));
        }
        if correlation_id.is_some() && (before_seq.is_some() || after_seq.is_some()) {
            return Err(AppError::invalid_request(
                "correlation_id cannot be combined with a cursor",
            ));
        }
        Ok(Self {
            limit,
            view,
            event_type,
            before_seq,
            after_seq,
            correlation_id,
            account_id: non_empty(params.account_id),
            source_id: non_empty(params.source_id),
        })
    }
}

pub async fn list_global_logs(
    State(state): State<AppState>,
    Query(params): Query<ListGlobalLogsParams>,
) -> Result<Json<GlobalLogsResponse>, AppError> {
    Ok(Json(state.events.list(params.try_into()?).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> UpdateSourceBody {
        serde_json::from_str(body).expect("请求体必须能解析")
    }

    /// 空对象是**空操作**，不是「重置一切」。这是把本端点从「改名端点」改成通用
    /// PATCH 之后的新契约：缺失的字段一律不动。
    #[test]
    fn empty_body_changes_nothing() {
        let body = parse("{}");
        assert_eq!(body.name, None, "缺失 = 不改标题");
        assert_eq!(body.quality_fallback, None, "缺失 = 不改开关");
    }

    /// 显式 `null` 才是「恢复默认标题」。
    #[test]
    fn explicit_null_name_means_restore_default() {
        let body = parse(r#"{"name":null}"#);
        assert_eq!(body.name, Some(None));
        assert_eq!(body.quality_fallback, None);
    }

    #[test]
    fn non_null_name_is_kept_for_sanitizing() {
        let body = parse(r#"{"name":"x"}"#);
        assert_eq!(body.name, Some(Some("x".to_string())));
    }

    /// **本次改动的回归点**：只想改开关的请求，绝不能把用户自定义的标题重置掉。
    /// 用早先的 `Option<String>` 时，`{"quality_fallback":true}` 会让 `name` 读成
    /// `None`，服务端把它解释为「恢复默认名」——标题被静默改写而请求里根本没提它。
    #[test]
    fn quality_fallback_only_leaves_name_absent() {
        let body = parse(r#"{"quality_fallback":true}"#);
        assert_eq!(
            body.name, None,
            "没提到 name 就必须是「不改」，而不是「恢复默认名」"
        );
        assert_eq!(body.quality_fallback, Some(true));
    }

    /// `"quality_fallback": null` 与缺失同义（= 不改），绝不能被当作「设为 false」
    /// 写下去——那会在用户只是想清空字段时把开关静默关掉。
    #[test]
    fn explicit_null_quality_fallback_is_absent_equivalent() {
        let body = parse(r#"{"quality_fallback":null}"#);
        assert_eq!(body.name, None);
        assert_eq!(body.quality_fallback, None);
    }

    #[test]
    fn verify_admin_rejects_empty_configured_token() {
        assert!(verify_admin(&HeaderMap::new(), "").is_err());
    }

    /// 两个字段同时出现（含 `name: null`）时都要被识别为「已提供」。
    #[test]
    fn both_fields_present_are_both_recognised() {
        let body = parse(r#"{"name":null,"quality_fallback":false}"#);
        assert_eq!(body.name, Some(None), "显式 null 仍是「恢复默认名」");
        assert_eq!(
            body.quality_fallback,
            Some(false),
            "显式的 false 是「关掉」，与缺失不同"
        );
    }
}
