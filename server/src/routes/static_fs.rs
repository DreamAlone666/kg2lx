use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use axum::response::IntoResponse;

use crate::app_state::AppState;

pub async fn serve(State(state): State<AppState>, req: Request<Body>) -> Response<Body> {
    let base = match &state.config.web_dist_dir {
        Some(d) => d.as_str(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let path = req.uri().path();
    let relative = path.trim_start_matches('/');

    if relative.is_empty() {
        return serve_file(base, "index.html")
            .await
            .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response());
    }

    if relative.split('/').any(|c| c == "..") {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(resp) = serve_file(base, relative).await {
        return resp;
    }

    serve_file(base, "index.html")
        .await
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

async fn serve_file(base: &str, relative: &str) -> Option<Response<Body>> {
    let root = tokio::fs::canonicalize(base).await.ok()?;
    let full = root.join(relative);
    let canonical = tokio::fs::canonicalize(&full).await.ok()?;
    if !canonical.starts_with(&root) {
        return None;
    }

    let meta = tokio::fs::metadata(&canonical).await.ok()?;
    if !meta.is_file() {
        return None;
    }
    let bytes = tokio::fs::read(&canonical).await.ok()?;
    let ct = mime_from_path(&canonical);
    Some(
        Response::builder()
            .status(200)
            .header("content-type", ct)
            .body(Body::from(bytes))
            .unwrap(),
    )
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "webp" => "image/webp",
        "wasm" => "application/wasm",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}
