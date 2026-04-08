//! Embedded HTTP server for browser mode.
//!
//! Serves the Svelte frontend as static files, proxies IPC commands as REST
//! endpoints, streams events via SSE, and handles heartbeat keep-alive.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State as AxumState};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use serde::Deserialize;

use crate::auth::AuthState;
use crate::commands;
use crate::config;
use crate::state::AppState;

/// Shared state for axum handlers.
pub struct WebState {
    pub app: Arc<AppState>,
    pub auth: Arc<AuthState>,
    pub lan_enabled: bool,
}

pub type SharedState = Arc<WebState>;

/// Start the embedded web server.
/// Returns the `JoinHandle` for the server task.
pub async fn start_server(
    state: Arc<AppState>,
    port: u16,
    lan_enabled: bool,
) -> tokio::task::JoinHandle<()> {
    let dist_dir = resolve_dist_dir();
    let web_state = Arc::new(WebState {
        app: state,
        auth: Arc::new(AuthState::new()),
        lan_enabled,
    });

    let app = Router::new()
        // Auth endpoints (always accessible)
        .route("/internal-api/_auth/login", post(auth_login_handler))
        .route("/internal-api/_auth/register", post(auth_register_handler))
        .route("/internal-api/_auth/status", get(auth_status_handler))
        // SSE event stream
        .route("/internal-api/_events", get(sse_handler))
        // Heartbeat endpoints
        .route("/internal-api/_heartbeat", post(heartbeat_handler))
        .route("/internal-api/_heartbeat_stop", post(heartbeat_stop_handler))
        // Thumbnail endpoint
        .route(
            "/internal-api/_thumbnail/{filename}",
            get(thumbnail_handler),
        )
        // Generic IPC command proxy
        .route("/internal-api/{command}", post(command_handler))
        // Static file serving (frontend)
        .fallback(get(move |req: axum::extract::Request| {
            let dist = dist_dir.clone();
            async move { serve_static(dist, req).await }
        }))
        .with_state(web_state);

    let bind_addr: SocketAddr = if lan_enabled {
        SocketAddr::from(([0, 0, 0, 0], port))
    } else {
        SocketAddr::from(([127, 0, 0, 1], port))
    };

    log::info!("Starting UI web server on {}", bind_addr);

    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .expect("Failed to bind UI web server");
        axum::serve(listener, app)
            .await
            .expect("UI web server crashed");
    })
}

/// Resolve the path to the frontend dist directory.
fn resolve_dist_dir() -> PathBuf {
    // In a Tauri app, the dist files are bundled. We need to find them.
    // During development, they're at ../dist relative to the Cargo project.
    // In production, they're bundled inside the binary. For browser mode,
    // we need them on disk, so we'll check a few locations.
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    // Check several candidate locations
    let candidates = [
        // Development: relative to Cargo project root
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist"),
        // Production: next to the executable
        exe_dir
            .as_ref()
            .map(|d| d.join("dist"))
            .unwrap_or_default(),
        // Production: in a resources subdirectory
        exe_dir
            .as_ref()
            .map(|d| d.join("resources/dist"))
            .unwrap_or_default(),
        // AppImage: relative to APPDIR
        std::env::var("APPDIR")
            .ok()
            .map(|d| PathBuf::from(d).join("usr/share/dist"))
            .unwrap_or_default(),
    ];

    for candidate in &candidates {
        if candidate.join("index.html").exists() {
            log::info!("Serving frontend from: {}", candidate.display());
            return candidate.clone();
        }
    }

    // Fallback — will 404 on requests but won't crash
    log::warn!("Could not find frontend dist directory, tried: {:?}", candidates);
    candidates[0].clone()
}

/// Serve static files from the dist directory.
async fn serve_static(dist_dir: PathBuf, req: axum::extract::Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    let file_path = if path.is_empty() {
        dist_dir.join("index.html")
    } else {
        dist_dir.join(path)
    };

    // If the path doesn't exist, serve index.html (SPA fallback)
    let file_path = if file_path.exists() && file_path.is_file() {
        file_path
    } else {
        dist_dir.join("index.html")
    };

    match tokio::fs::read(&file_path).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(&file_path)
                .first_or_octet_stream()
                .to_string();
            (
                StatusCode::OK,
                [
                    ("content-type", mime),
                    ("cache-control", "no-cache".to_string()),
                ],
                contents,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

/// SSE endpoint — streams all backend events to browser clients.
async fn sse_handler(
    AxumState(state): AxumState<SharedState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.app.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(evt) => {
            let json = serde_json::json!({
                "event": evt.event,
                "payload": evt.payload,
            });
            Some(Ok(Event::default().data(json.to_string())))
        }
        Err(_) => None, // lagged — skip
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Heartbeat — browser pings this to keep the backend alive.
async fn heartbeat_handler(AxumState(state): AxumState<SharedState>) -> StatusCode {
    let mut hb = state.app.last_heartbeat.lock().await;
    *hb = std::time::Instant::now();
    StatusCode::OK
}

/// Heartbeat stop — browser sends this via sendBeacon on page unload.
async fn heartbeat_stop_handler(AxumState(state): AxumState<SharedState>) -> StatusCode {
    // Set heartbeat to epoch so the watchdog triggers immediately
    let mut hb = state.app.last_heartbeat.lock().await;
    *hb = std::time::Instant::now() - Duration::from_secs(3600);
    StatusCode::OK
}

/// Gallery thumbnail endpoint.
async fn thumbnail_handler(
    Path(filename): Path<String>,
    req: axum::extract::Request,
) -> Response {
    let filename = percent_encoding::percent_decode_str(&filename)
        .decode_utf8()
        .map(|s| s.into_owned())
        .unwrap_or(filename);

    // Parse optional ?size= query param
    let max_size: u32 = req
        .uri()
        .query()
        .and_then(|q| {
            q.split('&')
                .find_map(|p| p.strip_prefix("size="))
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(256);

    let gallery_dir = match config::gallery_dir() {
        Some(d) => d,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "No gallery dir").into_response();
        }
    };

    match commands::api::generate_thumbnail(&gallery_dir, &filename, max_size) {
        Ok(data) => (
            StatusCode::OK,
            [
                ("content-type", "image/webp".to_string()),
                ("cache-control", "no-cache".to_string()),
            ],
            data,
        )
            .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            format!("Thumbnail error: {}", e),
        )
            .into_response(),
    }
}

/// Generic command handler — proxies IPC commands via HTTP POST.
///
/// The frontend sends `POST /internal-api/{command}` with a JSON body
/// containing the command arguments. We deserialize them and dispatch
/// to the same underlying functions the Tauri commands use.
async fn command_handler(
    AxumState(state): AxumState<SharedState>,
    Path(command): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let args: serde_json::Value = if body.is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid JSON: {}", e),
                )
                    .into_response();
            }
        }
    };

    // Auth check for LAN mode
    if state.lan_enabled {
        let auth_header = body.is_empty(); // placeholder — check below
        let _ = auth_header;
        // TODO: For now, LAN auth is checked at the auth endpoints level.
        // Full middleware auth will be added when we have the login page.
    }

    match dispatch_command(&state.app, &command, &args).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// Dispatch a command by name to the appropriate handler function.
///
/// This is the central routing table that maps command names to their
/// implementations. Each command extracts its arguments from the JSON body.
async fn dispatch_command(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match command {
        // --- Config ---
        "get_config" => {
            let config = state.config.read().await;
            serde_json::to_value(config.clone()).map_err(|e| e.to_string())
        }
        "update_config" => {
            let new_config: crate::config::AppConfig =
                serde_json::from_value(args["config"].clone())
                    .map_err(|e| format!("Invalid config: {}", e))?;
            config::save_config(&new_config).map_err(|e| e)?;
            let mut current = state.config.write().await;
            *current = new_config;
            Ok(serde_json::json!(null))
        }
        "get_gallery_path" => {
            let cfg = state.config.read().await;
            if let Some(ref custom) = cfg.gallery_path {
                let trimmed = custom.trim();
                if !trimmed.is_empty() {
                    return Ok(serde_json::json!(trimmed));
                }
            }
            let dir = config::app_data_dir()
                .ok_or("Cannot find app data directory")?
                .join("gallery");
            Ok(serde_json::json!(dir.to_string_lossy()))
        }

        // --- Server ---
        "check_setup" => {
            let cfg = state.config.read().await;
            Ok(serde_json::json!(cfg.setup_complete))
        }
        "check_server_health" => {
            let stats = state
                .get_system_stats_info()
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(stats).map_err(|e| e.to_string())
        }

        // --- API proxy commands (forwarded to ComfyUI backend) ---
        "get_models" => {
            let category = args["category"].as_str().unwrap_or("checkpoints");
            let result = state.get_models_list(category).await.map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }
        "get_samplers" => {
            let result = state.get_samplers_and_schedulers().await.map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "get_embeddings" => {
            let result = state.get_embeddings_list().await.map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }
        "get_queue" => {
            let result = state.get_queue_info().await.map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "get_history" => {
            let prompt_id = args["promptId"]
                .as_str()
                .ok_or("Missing promptId")?;
            let result = state
                .get_history_for(prompt_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(result)
        }
        "interrupt_generation" => {
            state.interrupt().await.map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "get_client_id" => {
            Ok(serde_json::json!(state.client_id))
        }

        // --- Gallery ---
        "list_gallery_images" => {
            let result = commands::api::list_gallery_images()
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }
        "list_gallery_image_entries" => {
            let result = commands::api::list_gallery_image_entries()
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "load_gallery_image" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let result = commands::api::load_gallery_image(filename)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }
        "get_gallery_image_path" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let result = commands::api::get_gallery_image_path(filename)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }
        "get_output_image" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let subfolder = args["subfolder"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let result = state
                .get_output_image_bytes(&filename, &subfolder)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }

        // For commands not yet mapped, return an error
        _ => Err(format!(
            "Command '{}' not implemented in browser mode",
            command
        )),
    }
}

// ---------------------------------------------------------------------------
// Auth endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AuthRequest {
    username: String,
    password: String,
}

/// POST /internal-api/_auth/login — authenticate and return a session token.
async fn auth_login_handler(
    AxumState(state): AxumState<SharedState>,
    Json(req): Json<AuthRequest>,
) -> Response {
    match state.auth.login(&req.username, &req.password) {
        Ok(token) => (StatusCode::OK, Json(serde_json::json!({ "token": token }))).into_response(),
        Err(e) => (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": e }))).into_response(),
    }
}

/// POST /internal-api/_auth/register — create a new account.
async fn auth_register_handler(
    AxumState(state): AxumState<SharedState>,
    Json(req): Json<AuthRequest>,
) -> Response {
    if req.username.trim().is_empty() || req.password.len() < 4 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Username required, password must be at least 4 characters" })),
        )
            .into_response();
    }
    match state.auth.create_account(&req.username, &req.password) {
        Ok(()) => {
            // Auto-login after registration
            match state.auth.login(&req.username, &req.password) {
                Ok(token) => (StatusCode::OK, Json(serde_json::json!({ "token": token }))).into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))).into_response(),
            }
        }
        Err(e) => (StatusCode::CONFLICT, Json(serde_json::json!({ "error": e }))).into_response(),
    }
}

/// GET /internal-api/_auth/status — check if auth is required and if any accounts exist.
async fn auth_status_handler(
    AxumState(state): AxumState<SharedState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "auth_required": state.lan_enabled,
        "has_accounts": state.auth.has_accounts(),
    }))
}

/// Start the heartbeat watchdog that shuts down the app when the browser
/// tab closes (no heartbeat for N seconds).
pub fn start_heartbeat_watchdog(state: Arc<AppState>, timeout_secs: u64) {
    tokio::spawn(async move {
        let timeout = Duration::from_secs(timeout_secs);
        // Wait a bit before starting to check (let the browser load)
        tokio::time::sleep(Duration::from_secs(10)).await;

        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let elapsed = {
                let hb = state.last_heartbeat.lock().await;
                hb.elapsed()
            };
            if elapsed > timeout {
                log::info!(
                    "No heartbeat for {:?}, shutting down (browser tab likely closed)",
                    elapsed
                );
                // Trigger app exit
                std::process::exit(0);
            }
        }
    });
}
