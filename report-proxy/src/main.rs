mod dedup;
mod github;
mod ratelimit;
mod report;
mod types;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Method};
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use github::GithubClient;
use ratelimit::RateLimiter;
use types::{AppState, Config};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Config::from_env();
    let bind_addr = config.bind_addr.clone();
    let max_body = config.max_body_bytes;

    let github = GithubClient::new(
        reqwest::Client::new(),
        config.github_token.clone(),
        config.github_repo.clone(),
    );
    let limiter = Arc::new(RateLimiter::new(config.rate_limit_per_min));

    let state = AppState { github, limiter };

    // Permissive-but-header-gated CORS: browser mode posts cross-origin, and the
    // custom X-Mooshie-App header forces a preflight. Abuse control is the header
    // gate + rate limit, not the origin.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("x-mooshie-app"),
        ]);

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/report", post(report::report_handler))
        .layer(cors)
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("failed to bind");
    tracing::info!("report-proxy listening on {bind_addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    {
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler")
                .recv()
                .await;
        };
        tokio::select! {
            _ = ctrl_c => {}
            _ = terminate => {}
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}
