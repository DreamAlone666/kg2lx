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
