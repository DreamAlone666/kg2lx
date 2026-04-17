use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;

use crate::app_state::AppState;
use crate::error::AppError;
use crate::services::runtime::RuntimeMusicUrlRequest;

fn verify_runtime(headers: &HeaderMap) -> Result<String, AppError> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() {
        return Err(AppError::admin_unauthorized());
    }
    Ok(token.to_string())
}

pub async fn music_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RuntimeMusicUrlRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let runtime_token = verify_runtime(&headers)?;

    if req.hash.is_empty() {
        return Err(AppError::invalid_request("hash is required"));
    }
    match req.quality.as_str() {
        "128k" | "320k" | "flac" | "flac24bit" => {}
        _ => return Err(AppError::invalid_request("invalid quality")),
    }

    let resp = state.runtime.fetch_music_url(&runtime_token, req).await?;
    Ok(Json(serde_json::to_value(resp).unwrap()))
}
