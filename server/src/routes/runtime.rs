use axum::Json;
use axum::body::Bytes;
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
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    let runtime_token = verify_runtime(&headers)?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| AppError::invalid_request("invalid json"))?;
    // lx-music-mobile may send a JSON string that contains the actual JSON body,
    // while desktop sends the object directly.
    let value = match value {
        serde_json::Value::String(s) => serde_json::from_str(&s)
            .map_err(|_| AppError::invalid_request("invalid request body"))?,
        value => value,
    };
    let req: RuntimeMusicUrlRequest = serde_json::from_value(value)
        .map_err(|_| AppError::invalid_request("invalid request body"))?;

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
