mod app_state;
mod config;
mod domain;
mod error;
mod repos;
mod routes;
mod services;

use std::borrow::Cow;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::MatchedPath;
use axum::http::{Request, Response};
use axum::routing::{get, post};
use tower_http::trace::TraceLayer;
use tracing::Span;

use crate::app_state::AppState;
use crate::config::Config;

fn validate_web_dist_dir(dir: &str) {
    let path = std::path::Path::new(dir);
    std::fs::canonicalize(path)
        .unwrap_or_else(|err| panic!("WEB_DIST_DIR is unreadable or missing: {dir}: {err}"));

    std::fs::read_dir(path)
        .unwrap_or_else(|err| panic!("WEB_DIST_DIR is unreadable: {dir}: {err}"));
}

fn logged_path(req: &Request<Body>) -> Cow<'_, str> {
    if let Some(matched) = req.extensions().get::<MatchedPath>() {
        return Cow::Borrowed(matched.as_str());
    }

    Cow::Borrowed(req.uri().path())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = Config::from_env();

    if let Some(ref dir) = config.web_dist_dir {
        validate_web_dist_dir(dir);
    }

    let listen_addr = config.listen_addr.clone();
    let has_web_dist = config.web_dist_dir.is_some();
    let state = AppState::new(config);

    let admin_routes = Router::new()
        .route(
            "/providers/kugou-lite/login/qr",
            post(routes::admin::start_qr_login),
        )
        .route(
            "/providers/kugou-lite/login/qr/{session_id}",
            get(routes::admin::poll_qr_login),
        )
        .route("/sources", get(routes::admin::list_sources))
        .route("/sources/{source_id}", get(routes::admin::get_source))
        .route(
            "/sources/{source_id}/refresh",
            post(routes::admin::refresh_source),
        )
        .route(
            "/sources/{source_id}/logs",
            get(routes::admin::list_source_logs),
        );

    let runtime_routes = Router::new().route("/music-url", post(routes::runtime::music_url));

    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|req: &Request<Body>| {
            let path = logged_path(req);
            tracing::info_span!(
                "request",
                method = %req.method(),
                path = %path,
            )
        })
        .on_request(())
        .on_response(|res: &Response<Body>, latency: Duration, _span: &Span| {
            tracing::info!(
                status = %res.status(),
                latency_ms = latency.as_millis() as u64,
                "completed",
            );
        })
        .on_failure(());

    let app = Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/s/{script_path}", get(routes::script::get_script))
        .nest("/api/v1/admin", admin_routes)
        .nest("/api/v1/runtime", runtime_routes)
        .layer(trace_layer);

    let app: Router<()> = if has_web_dist {
        app.fallback(routes::static_fs::serve)
    } else {
        app
    }
    .with_state(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .expect("bind failed");
    eprintln!("listening on {}", listen_addr);
    axum::serve(listener, app).await.expect("server error");
}
