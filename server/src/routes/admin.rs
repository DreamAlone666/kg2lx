use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;

use crate::app_state::AppState;
use crate::domain::runtime_log::RuntimeLogView;
use crate::error::AppError;

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

#[derive(serde::Deserialize)]
pub struct UpdateSourceBody {
    /// `null` 或纯空白 => 恢复默认名（`SOURCE_NAME_PREFIX + userid`）。
    /// 非空但清洗后为空（例如只填了 `*/`）=> 400，而不是静默变成默认名，
    /// 否则用户会被无声地「改名成功」误导。
    ///
    /// 注意本端点当前是「改名端点」而非通用 PATCH：字段**缺失**（`{}`）与显式
    /// `null` 一样都会重置为默认名。这在只有一个可改字段时无歧义，但**将来往
    /// 这里加第二个可改字段时，必须先把 `name` 改成能区分「缺失」与「null」的
    /// 类型**（例如 `Option<Option<String>>` + 自定义 deserialize），否则「只想
    /// 改字段 B」的请求会把用户自定义的标题静默重置掉。
    pub name: Option<String>,
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
        .update_source_name(&source_id, body.name)
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
