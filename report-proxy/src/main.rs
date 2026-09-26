mod dedup;
mod github;
mod origin;
mod ratelimit;
mod report;
mod types;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Method};
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};

use github::GithubClient;
use ratelimit::{RateLimiter, WriteBudget};
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

    // Bounded timeouts: a stalled GitHub call must not pin a request (and its
    // per-IP / global budget) open indefinitely.
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client");
    let github = GithubClient::new(
        http,
        config.github_token.clone(),
        config.github_repo.clone(),
    );
    let limiter = Arc::new(RateLimiter::new(config.rate_limit_per_min));
    let budget = Arc::new(WriteBudget::new(config.global_writes_per_min));
    let web_budget = Arc::new(WriteBudget::new(config.web_origin_writes_per_min));

    let state = AppState {
        github,
        limiter,
        budget,
        web_budget,
    };

    let app = app(state, max_body);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("failed to bind");
    tracing::info!("report-proxy listening on {bind_addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

fn app(state: AppState, max_body: usize) -> Router {
    // Header-gated CORS with an origin policy (origin.rs): the desktop webview
    // origins and http(s) web origins (browser mode runs on any host) pass the
    // preflight, `null` and non-web origins do not. The custom X-Mooshie-App
    // header forces that preflight. Abuse control beyond it is the per-IP rate
    // limit, the global GitHub write budget with its smaller web-origin share,
    // and the log cap.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin::allowed_by_cors(origin)
        }))
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("x-mooshie-app"),
        ]);

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/report", post(report::report_handler))
        .layer(cors)
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(state)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    const PAYLOAD: &str = r#"{"errorCode":"generic","rawMessage":"test","appVersion":"0","os":"x","arch":"x","mode":"desktop","timestamp":"2026-01-01T00:00:00Z"}"#;

    fn state(global_writes: u32, web_writes: u32) -> AppState {
        AppState {
            github: GithubClient::new(reqwest::Client::new(), String::new(), "example/repo".into()),
            limiter: Arc::new(RateLimiter::new(100)),
            budget: Arc::new(WriteBudget::new(global_writes)),
            web_budget: Arc::new(WriteBudget::new(web_writes)),
        }
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// The Access-Control-Allow-Origin a browser preflight from `origin` gets.
    async fn preflight(origin: &str) -> Option<String> {
        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/report")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "content-type,x-mooshie-app",
            )
            .body(Body::empty())
            .unwrap();
        let response = app(state(8, 3), 1024).oneshot(request).await.unwrap();
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .map(|v| v.to_str().unwrap().to_string())
    }

    async fn post(state: AppState, origin: Option<&str>, app_header: bool) -> StatusCode {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/report")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        if app_header {
            request = request.header("x-mooshie-app", "1");
        }
        let request = request.body(Body::from(PAYLOAD)).unwrap();
        app(state, 1 << 20).oneshot(request).await.unwrap().status()
    }

    #[tokio::test]
    async fn preflight_allows_the_app_and_web_origins_only() {
        let allowed = origin::TAURI_ORIGINS
            .into_iter()
            .chain(["http://192.168.1.20:3200", "https://mooshie.example.com"]);
        for origin in allowed {
            assert_eq!(preflight(origin).await.as_deref(), Some(origin), "{origin}");
        }
        for origin in ["null", "chrome-extension://abcdefgh", "file://"] {
            assert_eq!(preflight(origin).await, None, "{origin}");
        }
    }

    #[tokio::test]
    async fn refused_origins_and_missing_app_header_are_forbidden() {
        let forbidden = StatusCode::FORBIDDEN;
        assert_eq!(post(state(8, 3), Some("null"), true).await, forbidden);
        assert_eq!(
            post(state(8, 3), Some("moz-extension://x"), true).await,
            forbidden
        );
        assert_eq!(
            post(state(8, 3), Some("tauri://localhost"), false).await,
            forbidden
        );
        assert_eq!(post(state(8, 3), None, false).await, forbidden);
    }

    #[tokio::test]
    async fn web_origins_draw_on_their_own_share_of_the_budget() {
        let busy = StatusCode::TOO_MANY_REQUESTS;
        // With the web share used up, a web origin is refused and the global
        // budget stays available to the app.
        let s = state(1, 0);
        assert_eq!(
            post(s.clone(), Some("https://evil.example"), true).await,
            busy
        );
        assert!(s.budget.try_take(now()));
        // App and non-browser reports never draw on the web share.
        let s = state(0, 1);
        assert_eq!(post(s.clone(), Some("tauri://localhost"), true).await, busy);
        assert_eq!(post(s.clone(), None, true).await, busy);
        assert!(s.web_budget.try_take(now()));
    }
}
