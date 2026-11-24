use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, Response, StatusCode, header};
use axum::response::IntoResponse;

use crate::app_state::AppState;

pub async fn serve(State(state): State<AppState>, req: Request<Body>) -> Response<Body> {
    let Some(base) = state.config.web_dist_dir.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let relative = req.uri().path().trim_start_matches('/');
    if relative == "api"
        || relative == "s"
        || relative.starts_with("api/")
        || relative.starts_with("s/")
        || relative.split('/').any(|component| component == "..")
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let head = req.method() == Method::HEAD;
    if req.method() != Method::GET && !head {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
        )
            .into_response();
    }
    if !relative.is_empty() {
        if let Some(response) = serve_file(base, relative, head).await {
            return response;
        }
        if relative == "_app"
            || relative.starts_with("_app/")
            || std::path::Path::new(relative).extension().is_some()
        {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    serve_file(base, "index.html", head)
        .await
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

async fn serve_file(base: &std::path::Path, relative: &str, head: bool) -> Option<Response<Body>> {
    let canonical = tokio::fs::canonicalize(base.join(relative)).await.ok()?;
    if !canonical.starts_with(base) {
        return None;
    }

    let meta = tokio::fs::metadata(&canonical).await.ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, mime_from_path(&canonical))
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::X_FRAME_OPTIONS, "DENY");
    if relative.starts_with("_app/immutable/") {
        response = response.header(header::CACHE_CONTROL, "public, max-age=31536000, immutable");
    } else if matches!(relative, "index.html" | "_app/version.json") {
        response = response.header(header::CACHE_CONTROL, "no-cache");
    }
    let body = if head {
        response = response.header(header::CONTENT_LENGTH, meta.len());
        Body::empty()
    } else {
        Body::from(tokio::fs::read(canonical).await.ok()?)
    };
    response.body(body).ok()
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
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
