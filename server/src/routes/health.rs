use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app_state::AppState;

pub async fn healthz(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    if let Err(error) = state.storage.ping().await {
        tracing::warn!(%error, "SQLite health check failed");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false })),
        );
    }
    (StatusCode::OK, Json(json!({ "ok": true })))
}
