mod app_state;
mod config;
mod domain;
mod error;
mod repos;
mod routes;
mod services;

use axum::Router;
use axum::routing::{get, post};
use tower_http::trace::TraceLayer;

use crate::app_state::AppState;
use crate::config::Config;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = Config::from_env();
    let listen_addr = config.listen_addr.clone();
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
        );

    let runtime_routes = Router::new().route("/music-url", post(routes::runtime::music_url));

    let app = Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/s/{script_path}", get(routes::script::get_script))
        .nest("/api/v1/admin", admin_routes)
        .nest("/api/v1/runtime", runtime_routes)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .expect("bind failed");
    eprintln!("listening on {}", listen_addr);
    axum::serve(listener, app).await.expect("server error");
}
