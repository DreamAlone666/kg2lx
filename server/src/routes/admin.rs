use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;

use crate::app_state::AppState;
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
    let resp = state
        .login
        .poll_kugou_lite_qr_login(&session_id)
        .await?;
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
