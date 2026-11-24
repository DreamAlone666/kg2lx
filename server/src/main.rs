mod app_state;
mod config;
mod domain;
mod error;
#[cfg(test)]
mod integration_tests;
mod repos;
mod routes;
mod services;

use std::borrow::Cow;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::MatchedPath;
use axum::http::{Request, Response};
use axum::middleware;
use axum::routing::{get, patch, post};
use tower_http::trace::TraceLayer;
use tracing::Span;

use crate::app_state::AppState;
use crate::config::Config;

fn logged_path(req: &Request<Body>) -> Cow<'_, str> {
    if let Some(matched) = req.extensions().get::<MatchedPath>() {
        return Cow::Borrowed(matched.as_str());
    }

    let path = req.uri().path();
    Cow::Borrowed(if path.starts_with("/s/") {
        "/s/{script_path}"
    } else {
        path
    })
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    if let Err(error) = serve().await {
        tracing::error!(%error, "server stopped with an error");
        std::process::exit(1);
    }
}

async fn serve() -> Result<(), String> {
    let config = Config::from_env()?;
    let listen_addr = config.listen_addr.clone();
    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .map_err(|error| format!("invalid or unavailable LISTEN_ADDR {listen_addr:?}: {error}"))?;
    let state = AppState::new(config).map_err(|error| error.to_string())?;
    let storage = state.storage.clone();
    tracing::info!(addr = %listener.local_addr().map_err(|error| error.to_string())?, "listening");
    let result = axum::serve(listener, build_app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| error.to_string());
    storage
        .shutdown()
        .await
        .map_err(|error| error.to_string())?;
    result
}

async fn shutdown_signal() {
    #[cfg(unix)]
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
        Err(error) => tracing::error!(%error, "could not install SIGTERM handler"),
    }
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "could not wait for SIGINT");
    }
}

fn build_app(state: AppState) -> Router {
    let has_web_dist = state.config.web_dist_dir.is_some();
    let admin_routes = Router::new()
        .route(
            "/providers/kugou-lite/login/qr",
            post(routes::admin::start_qr_login),
        )
        .route(
            "/providers/kugou-lite/login/qr/{session_id}",
            get(routes::admin::poll_qr_login),
        )
        .route("/accounts", get(routes::admin::list_accounts))
        .route("/accounts/{account_id}", get(routes::admin::get_account))
        .route("/logs", get(routes::admin::list_global_logs))
        .route(
            "/accounts/{account_id}/refresh-token",
            post(routes::admin::refresh_account_token),
        )
        .route(
            "/accounts/{account_id}/entitlements/sync",
            post(routes::admin::sync_account_entitlements),
        )
        .route(
            "/accounts/{account_id}/entitlements/renewal",
            patch(routes::admin::set_auto_claim),
        )
        .route(
            "/accounts/{account_id}/entitlements/renewal/claim",
            post(routes::admin::claim_account_entitlements),
        )
        .route("/sources/{source_id}", patch(routes::admin::update_source))
        .route_layer(middleware::from_fn_with_state(
            state.config.clone(),
            routes::admin::require_admin,
        ));

    let runtime_routes = Router::new()
        .route("/music-url", post(routes::runtime::music_url))
        .route("/script-update", post(routes::runtime::script_update));

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
        .nest("/api/v1/runtime", runtime_routes);

    if has_web_dist {
        app.fallback(routes::static_fs::serve)
    } else {
        app
    }
    .layer(trace_layer)
    .with_state(state)
}
