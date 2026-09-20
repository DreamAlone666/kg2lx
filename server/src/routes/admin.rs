use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use crate::app_state::AppState;
use crate::domain::runtime_log::RuntimeLogView;
use crate::error::AppError;
use crate::services::source::UpdateSourceInput;

fn verify_admin(headers: &HeaderMap, token: &str) -> Result<(), AppError> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let bearer = auth.strip_prefix("Bearer ").unwrap_or("");
    if bearer == token {
        Ok(())
    } else {
        Err(AppError::admin_unauthorized())
    }
}

pub async fn start_qr_login(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_admin(&headers, &state.config.admin_token)?;
    let resp = state.login.start_kugou_lite_qr_login().await?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

pub async fn poll_qr_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_admin(&headers, &state.config.admin_token)?;
    let resp = state.login.poll_kugou_lite_qr_login(&session_id).await?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

pub async fn list_sources(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_admin(&headers, &state.config.admin_token)?;
    let items = state.source.list_sources().await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn get_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_admin(&headers, &state.config.admin_token)?;
    let detail = state.source.get_source(&source_id).await?;
    Ok(Json(serde_json::to_value(detail).unwrap()))
}

pub async fn refresh_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_admin(&headers, &state.config.admin_token)?;
    let resp = state.source.refresh_source(&source_id).await?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

/// 音源配置 PATCH 的请求体。
///
/// 契约统一为「字段**缺失** = 不改那个字段」，因此 `{}` 是一个合法的空操作请求
/// （它不会重置任何东西）。要「恢复默认标题」必须显式送 `null` 或纯空白串。
///
/// `name` 用 `Option<Option<String>>` 正是为了表达这条契约：外层 `None` 表示
/// 「请求里没有这个字段」，内层 `None` 表示「字段在、但值是 `null`」。若沿用
/// 早先的 `Option<String>`，缺失与 `null` 会塌缩成同一个值，于是「只想改
/// `quality_fallback`」的请求会把用户自定义的标题静默重置掉——这正是本端点从
/// 「改名端点」变成通用 PATCH 时必须先修掉的那件事。
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

/// `Json` extractor 必须放在参数列表最后：axum 0.8 中 body 消费者只能是最后一个
/// 参数，否则编译期报错。
pub async fn update_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    Json(body): Json<UpdateSourceBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_admin(&headers, &state.config.admin_token)?;
    let resp = state
        .source
        .update_source(
            &source_id,
            UpdateSourceInput {
                name: body.name,
                quality_fallback: body.quality_fallback,
            },
        )
        .await?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}

#[derive(serde::Deserialize)]
pub struct ListLogsParams {
    pub limit: Option<String>,
    pub view: Option<String>,
}

pub async fn list_source_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(source_id): Path<String>,
    Query(params): Query<ListLogsParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_admin(&headers, &state.config.admin_token)?;

    let limit = match params.limit.as_deref() {
        None => 20,
        Some(s) => s
            .parse::<usize>()
            .ok()
            .filter(|&n| (1..=100).contains(&n))
            .ok_or_else(|| AppError::invalid_request("invalid limit"))?,
    };

    let view = match params.view.as_deref() {
        None | Some("all") => RuntimeLogView::All,
        Some("errors") => RuntimeLogView::Errors,
        _ => return Err(AppError::invalid_request("invalid view")),
    };

    let resp = state
        .source
        .list_source_logs(&source_id, limit, view)
        .await?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
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
