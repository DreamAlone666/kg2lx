use axum::Json;
use serde_json::{Value, json};

pub async fn healthz() -> Json<Value> {
    Json(json!({ "ok": true }))
}
