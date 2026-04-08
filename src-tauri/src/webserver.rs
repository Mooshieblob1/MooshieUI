//! Embedded HTTP server for browser mode.
//!
//! Serves the Svelte frontend as static files, proxies IPC commands as REST
//! endpoints, streams events via SSE, and handles heartbeat keep-alive.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, Path, State as AxumState};
use axum::http::{HeaderMap, StatusCode};
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

// ---------------------------------------------------------------------------
// Auth helpers — role-based access for LAN vs localhost
// ---------------------------------------------------------------------------

/// User role derived from auth context.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UserRole {
    /// Full access — localhost or the machine owner.
    Admin,
    /// Moderator — full settings access except mode switching, LAN config, and filesystem paths.
    Moderator,
    /// Authenticated LAN user — can generate, browse gallery, but not change settings.
    User,
    /// Not authenticated.
    Anonymous,
}

/// Check if a socket address is localhost.
fn is_localhost(addr: &SocketAddr) -> bool {
    let ip = addr.ip();
    ip.is_loopback()
}

/// Extract the bearer token from request headers.
fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.to_string())
}

/// Determine the user's role from the request context.
fn resolve_role(state: &WebState, headers: &HeaderMap, remote: &SocketAddr) -> UserRole {
    // Localhost always gets admin
    if is_localhost(remote) {
        return UserRole::Admin;
    }
    // LAN not enabled → admin (shouldn't happen since LAN users can't reach us, but be safe)
    if !state.lan_enabled {
        return UserRole::Admin;
    }
    // No accounts configured → allow as user (open access, warned in UI)
    if !state.auth.has_accounts() {
        return UserRole::User;
    }
    // Check bearer token
    if let Some(token) = extract_token(headers) {
        if let Some(username) = state.auth.validate_token(&token) {
            // Check if the account has moderator role
            if let Some(role) = state.auth.get_account_role(&username) {
                if role == "moderator" {
                    return UserRole::Moderator;
                }
            }
            return UserRole::User;
        }
    }
    UserRole::Anonymous
}

/// Resolve the username for the current request.
/// Returns None for localhost/admin (they use the shared gallery root).
fn resolve_username(state: &WebState, headers: &HeaderMap, remote: &SocketAddr) -> Option<String> {
    if is_localhost(remote) || !state.lan_enabled {
        return None; // admin — uses root gallery
    }
    if let Some(token) = extract_token(headers) {
        return state.auth.validate_token(&token);
    }
    None
}

/// Commands that ONLY the admin (localhost) can execute.
/// These involve mode switching, filesystem access, or LAN configuration.
const ADMIN_ONLY_COMMANDS: &[&str] = &[
    "switch_to_app_mode",
    "set_gallery_path",
    "install_custom_node",
    "install_pip_package",
    "import_image_directory",
    "open_directory",
    "move_installation",
    "read_image_metadata_path",
];

/// Commands that moderators (and admins) can execute.
/// These involve server control and configuration but no filesystem access.
const MODERATOR_COMMANDS: &[&str] = &[
    "update_config",
    "stop_comfyui",
    "download_model",
    "export_logs",
];

/// Check command permission level.
/// Returns the minimum role required to execute the command.
fn min_role_for_command(command: &str) -> UserRole {
    if ADMIN_ONLY_COMMANDS.contains(&command) {
        UserRole::Admin
    } else if MODERATOR_COMMANDS.contains(&command) {
        UserRole::Moderator
    } else {
        UserRole::User
    }
}

fn unauthorized_response(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

fn forbidden_response(msg: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

/// Start the embedded web server.
/// Returns the `JoinHandle` for the server task.
pub async fn start_server(
    state: Arc<AppState>,
    port: u16,
    lan_enabled: bool,
) -> tokio::task::JoinHandle<()> {
    let dist_dir = resolve_dist_dir();

    // Mark web server as running before moving state into the router
    state
        .web_server_running
        .store(true, std::sync::atomic::Ordering::SeqCst);

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
        .route(
            "/internal-api/_auth/accounts",
            get(auth_list_accounts_handler),
        )
        .route(
            "/internal-api/_auth/delete",
            post(auth_delete_account_handler),
        )
        .route(
            "/internal-api/_auth/change_password",
            post(auth_change_password_handler),
        )
        .route(
            "/internal-api/_auth/reset_password",
            post(auth_reset_password_handler),
        )
        .route("/internal-api/_auth/set_role", post(auth_set_role_handler))
        .route("/internal-api/_auth/lan_info", get(auth_lan_info_handler))
        // SSE event stream
        .route("/internal-api/_events", get(sse_handler))
        // Heartbeat endpoints
        .route("/internal-api/_heartbeat", post(heartbeat_handler))
        .route(
            "/internal-api/_heartbeat_stop",
            post(heartbeat_stop_handler),
        )
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
        // Images sent as JSON arrays of numbers inflate ~4x, so allow large bodies
        .layer(axum::extract::DefaultBodyLimit::max(256 * 1024 * 1024))
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
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
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
        exe_dir.as_ref().map(|d| d.join("dist")).unwrap_or_default(),
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
    log::warn!(
        "Could not find frontend dist directory, tried: {:?}",
        candidates
    );
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

            // Inject browser-mode flag into HTML so the frontend IPC layer
            // knows to use HTTP endpoints instead of Tauri IPC.
            let contents = if mime == "text/html" {
                let html = String::from_utf8_lossy(&contents);
                let injected = html.replacen(
                    "<head>",
                    "<head><script>window.__MOOSHIE_BROWSER_MODE__=true;</script>",
                    1,
                );
                injected.into_bytes()
            } else {
                contents
            };

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
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    // Auth check — SSE uses query param since EventSource can't set headers
    let mut hdrs = headers.clone();
    if let Some(token) = query.get("token") {
        hdrs.insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );
    }
    let role = resolve_role(&state, &hdrs, &remote);
    if role == UserRole::Anonymous {
        return unauthorized_response("Authentication required");
    }

    let rx = state.app.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(evt) => {
            let json = serde_json::json!({
                "event": evt.event,
                "payload": evt.payload,
            });
            Some(Ok::<_, std::convert::Infallible>(
                Event::default().data(json.to_string()),
            ))
        }
        Err(_) => None, // lagged — skip
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Heartbeat — browser pings this to keep the backend alive.
async fn heartbeat_handler(AxumState(state): AxumState<SharedState>) -> StatusCode {
    let mut hb = state.app.last_heartbeat.lock().await;
    *hb = std::time::Instant::now();
    StatusCode::OK
}

/// Heartbeat stop — browser sends this via sendBeacon on page unload.
async fn heartbeat_stop_handler(AxumState(state): AxumState<SharedState>) -> StatusCode {
    // If we've already switched to app mode, ignore the stop signal.
    if state
        .app
        .app_mode_active
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return StatusCode::OK;
    }
    // Set heartbeat to epoch so the watchdog triggers immediately
    let mut hb = state.app.last_heartbeat.lock().await;
    *hb = std::time::Instant::now() - Duration::from_secs(3600);
    StatusCode::OK
}

/// Gallery thumbnail endpoint.
async fn thumbnail_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Path(filename): Path<String>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Response {
    let filename = percent_encoding::percent_decode_str(&filename)
        .decode_utf8()
        .map(|s| s.into_owned())
        .unwrap_or(filename);

    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return (StatusCode::BAD_REQUEST, "Invalid filename").into_response();
    }

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

    let username = resolve_username(&state, &headers, &remote);
    let gallery_dir = match user_gallery_dir(username.as_deref()) {
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
        Err(e) => (StatusCode::NOT_FOUND, format!("Thumbnail error: {}", e)).into_response(),
    }
}

/// Generic command handler — proxies IPC commands via HTTP POST.
///
/// The frontend sends `POST /internal-api/{command}` with a JSON body
/// containing the command arguments. We deserialize them and dispatch
/// to the same underlying functions the Tauri commands use.
async fn command_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Path(command): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let args: serde_json::Value = if body.is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)).into_response();
            }
        }
    };

    // Auth enforcement
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return unauthorized_response("Authentication required. Please log in.");
    }
    let required = min_role_for_command(&command);
    let allowed = match required {
        UserRole::Admin => role == UserRole::Admin,
        UserRole::Moderator => role == UserRole::Admin || role == UserRole::Moderator,
        _ => true,
    };
    if !allowed {
        return forbidden_response("You do not have permission for this action.");
    }

    // Resolve username for per-user gallery isolation
    let username = resolve_username(&state, &headers, &remote);

    match dispatch_command(state.app.clone(), &command, &args, username.as_deref()).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// Dispatch a command by name to the appropriate handler function.
///
/// This is the central routing table that maps command names to their
/// implementations. Each command extracts its arguments from the JSON body.
///
/// `username` is `Some("bob")` for authenticated LAN users, `None` for admin/localhost.
/// Gallery commands use this to isolate per-user image storage.
async fn dispatch_command(
    state: Arc<AppState>,
    command: &str,
    args: &serde_json::Value,
    username: Option<&str>,
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
        "switch_to_app_mode" => {
            // Step 1: Save config
            let mut cfg = state.config.write().await;
            cfg.browser_mode = false;
            config::save_config(&cfg).map_err(|e| e)?;
            drop(cfg);

            // Step 2: Disarm heartbeat watchdog
            state
                .app_mode_active
                .store(true, std::sync::atomic::Ordering::SeqCst);

            // Step 3: Show the existing hidden Tauri window.
            // The window was only hidden (not destroyed) by switch_to_browser_mode,
            // so it still has the fully-loaded app. Just make it visible again.
            let handle_guard = state.app_handle.lock().await;
            if let Some(ref app_handle) = *handle_guard {
                use tauri::Manager;
                if let Some(win) = app_handle.get_webview_window("main") {
                    // Reload the page first — the hidden webview's JS context
                    // gets stale (suspended timers, dead WebSocket connections)
                    // so we need a fresh init.
                    let _ = win.eval("location.reload()");
                    let _ = win.show();
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                    log::info!("switch_to_app_mode: reloaded and showed existing window");
                } else {
                    log::error!("switch_to_app_mode: no 'main' window found");
                    return Err("No app window found — please restart the application".into());
                }
            } else {
                log::error!("switch_to_app_mode: AppHandle not available");
                return Err("AppHandle not available — please restart the application".into());
            }

            Ok(serde_json::json!(null))
        }
        "get_gallery_path" => {
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
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
        "start_comfyui" => {
            use crate::comfyui::process::{self, StartResult};
            use crate::comfyui::websocket;
            let result = process::start_comfyui_process(&state)
                .await
                .map_err(|e| e.to_string())?;
            let event_tx = state.event_tx.clone();
            match result {
                StartResult::AlreadyRunning => {
                    // Connect websocket so progress events flow to SSE
                    if let Err(e) = websocket::connect_websocket_headless(&state, event_tx).await {
                        log::error!("Failed to connect WebSocket (headless): {}", e);
                    }
                    state.broadcast("comfyui:server_ready", serde_json::json!(null));
                    Ok(serde_json::json!("already_running"))
                }
                StartResult::Spawned => {
                    let state_clone = state.clone();
                    tokio::spawn(async move {
                        match process::wait_for_ready(&state_clone, 120).await {
                            Ok(()) => {
                                log::info!("ComfyUI server is ready (browser mode)");
                                // Connect websocket so progress events flow to SSE
                                if let Err(e) = websocket::connect_websocket_headless(
                                    &state_clone,
                                    event_tx.clone(),
                                )
                                .await
                                {
                                    log::error!("Failed to connect WebSocket (headless): {}", e);
                                }
                                let _ = event_tx.send(crate::state::BroadcastEvent {
                                    event: "comfyui:server_ready".to_string(),
                                    payload: serde_json::json!(null),
                                });
                            }
                            Err(e) => {
                                let err_str = e.to_string();
                                log::error!("ComfyUI failed to become ready: {}", err_str);
                                let _ = event_tx.send(crate::state::BroadcastEvent {
                                    event: "comfyui:server_error".to_string(),
                                    payload: serde_json::json!({
                                        "error": err_str,
                                        "crashed": err_str.contains("exited with"),
                                    }),
                                });
                            }
                        }
                    });
                    Ok(serde_json::json!("spawned"))
                }
                StartResult::Skipped => {
                    // Remote mode — connect websocket directly
                    if let Err(e) = websocket::connect_websocket_headless(&state, event_tx).await {
                        log::error!("Failed to connect WebSocket (headless): {}", e);
                    }
                    state.broadcast("comfyui:server_ready", serde_json::json!(null));
                    Ok(serde_json::json!("skipped"))
                }
            }
        }
        "stop_comfyui" => {
            crate::comfyui::process::stop_comfyui_process(&state)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "connect_ws" => {
            use crate::comfyui::websocket;
            let event_tx = state.event_tx.clone();
            websocket::connect_websocket_headless(&state, event_tx)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "disconnect_ws" => {
            crate::comfyui::websocket::disconnect_websocket(&state)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }

        // --- API proxy commands (forwarded to ComfyUI backend) ---
        "get_models" => {
            let category = args["category"].as_str().unwrap_or("checkpoints");
            let result = state
                .get_models_list(category)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }
        "get_samplers" => {
            let result = state
                .get_samplers_and_schedulers()
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "get_embeddings" => {
            let result = state
                .get_embeddings_list()
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }
        "get_queue" => {
            let result = state.get_queue_info().await.map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "get_history" => {
            let prompt_id = args["promptId"].as_str().ok_or("Missing promptId")?;
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
        "get_client_id" => Ok(serde_json::json!(state.client_id)),

        // --- Gallery (per-user isolated in LAN mode) ---
        "list_gallery_images" => {
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            if !dir.exists() {
                return Ok(serde_json::json!([]));
            }
            let mut files: Vec<_> = std::fs::read_dir(&dir)
                .map_err(|e| e.to_string())?
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    // Skip the "users" subdirectory
                    if entry.file_type().ok()?.is_dir() {
                        return None;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.ends_with(".png")
                        || name.ends_with(".jpg")
                        || name.ends_with(".jpeg")
                        || name.ends_with(".webp")
                    {
                        Some((entry.metadata().ok()?.modified().ok()?, name))
                    } else {
                        None
                    }
                })
                .collect();
            files.sort_by(|a, b| b.0.cmp(&a.0));
            Ok(serde_json::json!(files
                .into_iter()
                .map(|(_, n)| n)
                .collect::<Vec<_>>()))
        }
        "list_gallery_image_entries" => {
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            if !dir.exists() {
                return Ok(serde_json::json!([]));
            }
            let mut files: Vec<_> = std::fs::read_dir(&dir)
                .map_err(|e| e.to_string())?
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    if entry.file_type().ok()?.is_dir() {
                        return None;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !(name.ends_with(".png")
                        || name.ends_with(".jpg")
                        || name.ends_with(".jpeg")
                        || name.ends_with(".webp"))
                    {
                        return None;
                    }
                    let metadata = entry.metadata().ok()?;
                    let modified = metadata.modified().ok()?;
                    let modified_ms = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()?
                        .as_millis() as u64;
                    Some(serde_json::json!({
                        "filename": name,
                        "size_bytes": metadata.len(),
                        "modified_ms": modified_ms,
                    }))
                })
                .collect();
            files.sort_by(|a, b| {
                let am = a["modified_ms"].as_u64().unwrap_or(0);
                let bm = b["modified_ms"].as_u64().unwrap_or(0);
                bm.cmp(&am)
            });
            Ok(serde_json::json!(files))
        }
        "load_gallery_image" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                return Err("Invalid filename".into());
            }
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            let path = dir.join(&filename);
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            Ok(serde_json::json!(bytes))
        }
        "get_gallery_image_path" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                return Err("Invalid filename".into());
            }
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            let path = dir.join(&filename);
            Ok(serde_json::json!(path.to_string_lossy()))
        }
        "get_output_image" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let subfolder = args["subfolder"].as_str().unwrap_or("").to_string();
            let result = state
                .get_output_image_bytes(&filename, &subfolder)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }

        // --- Generation ---
        "generate" => {
            let params: crate::comfyui::types::GenerationParams =
                serde_json::from_value(args["params"].clone())
                    .map_err(|e| format!("Invalid params: {}", e))?;
            let seed = if params.seed < 0 {
                (rand::random::<u64>() >> 1) as i64
            } else {
                params.seed
            };
            let workflow = crate::templates::build_workflow(&params, seed);
            let response = state
                .queue_prompt_request(workflow, &state.client_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "prompt_id": response.prompt_id,
                "seed": seed,
            }))
        }
        "delete_queue_item" => {
            let prompt_id = args["promptId"].as_str().ok_or("Missing promptId")?;
            state
                .delete_queue_items(vec![prompt_id.to_string()])
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "upload_image_bytes" => {
            let image_bytes: Vec<u8> = serde_json::from_value(args["imageBytes"].clone())
                .map_err(|e| format!("Invalid imageBytes: {}", e))?;
            let filename = args["filename"]
                .as_str()
                .unwrap_or("upload.png")
                .to_string();
            let result = state
                .upload_image_from_bytes(image_bytes, filename)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }

        // --- Gallery write operations (per-user isolated in LAN mode) ---
        "save_to_gallery" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let subfolder = args["subfolder"].as_str().unwrap_or("").to_string();
            let prompt_id = args["promptId"].as_str().unwrap_or("").to_string();
            let mode = args
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let metadata: Option<std::collections::HashMap<String, String>> = args
                .get("metadata")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let metadata_mode = args
                .get("metadataMode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let bytes = state
                .get_output_image_bytes(&filename, &subfolder)
                .await
                .map_err(|e| e.to_string())?;
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            let result = save_to_gallery_in_dir(
                &dir,
                &bytes,
                &filename,
                &prompt_id,
                mode.as_deref(),
                metadata.as_ref(),
                metadata_mode.as_deref(),
            )?;
            Ok(serde_json::json!(result))
        }
        "save_to_gallery_bytes" => {
            let image_bytes: Vec<u8> = serde_json::from_value(args["imageBytes"].clone())
                .map_err(|e| format!("Invalid imageBytes: {}", e))?;
            let filename = args["filename"].as_str().unwrap_or("image.png").to_string();
            let prompt_id = args["promptId"].as_str().unwrap_or("").to_string();
            let mode = args
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let metadata: Option<std::collections::HashMap<String, String>> = args
                .get("metadata")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let metadata_mode = args
                .get("metadataMode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            let result = save_to_gallery_in_dir(
                &dir,
                &image_bytes,
                &filename,
                &prompt_id,
                mode.as_deref(),
                metadata.as_ref(),
                metadata_mode.as_deref(),
            )?;
            Ok(serde_json::json!(result))
        }
        "delete_gallery_image" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                return Err("Invalid filename".into());
            }
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            let path = dir.join(&filename);
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
            Ok(serde_json::json!(null))
        }
        "rename_gallery_image" => {
            let old = args["oldFilename"]
                .as_str()
                .ok_or("Missing oldFilename")?
                .to_string();
            let new_name = args["newFilename"]
                .as_str()
                .ok_or("Missing newFilename")?
                .to_string();
            if old.contains('/')
                || old.contains('\\')
                || old.contains("..")
                || new_name.contains('/')
                || new_name.contains('\\')
                || new_name.contains("..")
            {
                return Err("Invalid filename".into());
            }
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            let old_path = dir.join(&old);
            let new_path = dir.join(&new_name);
            std::fs::rename(&old_path, &new_path).map_err(|e| e.to_string())?;
            Ok(serde_json::json!(new_name))
        }
        "import_image_directory" => {
            Err("import_image_directory not yet available in browser mode".to_string())
        }

        // --- Metadata (per-user gallery aware) ---
        "read_image_metadata" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                return Err("Invalid filename".into());
            }
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            let path = dir.join(&filename);
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let result = crate::metadata::read_png_metadata(&bytes).map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "read_image_metadata_bytes" => {
            let image_bytes: Vec<u8> = serde_json::from_value(args["imageBytes"].clone())
                .map_err(|e| format!("Invalid imageBytes: {}", e))?;
            let result = commands::api::read_image_metadata_bytes(image_bytes)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "read_image_metadata_path" => {
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            let result = commands::api::read_image_metadata_path(path)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "embed_png_metadata_bytes" => {
            let image_bytes: Vec<u8> = serde_json::from_value(args["imageBytes"].clone())
                .map_err(|e| format!("Invalid imageBytes: {}", e))?;
            let metadata: std::collections::HashMap<String, String> =
                serde_json::from_value(args["metadata"].clone())
                    .map_err(|e| format!("Invalid metadata: {}", e))?;
            let metadata_mode = args
                .get("metadataMode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let result =
                commands::api::embed_png_metadata_bytes(image_bytes, metadata, metadata_mode)
                    .await
                    .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(result))
        }

        // --- Custom node / pip install ---
        "install_custom_node" => {
            let git_url = args["gitUrl"].as_str().ok_or("Missing gitUrl")?.to_string();
            let node_name = args["nodeName"]
                .as_str()
                .ok_or("Missing nodeName")?
                .to_string();

            let config = state.config.read().await;
            let custom_nodes_dir = std::path::Path::new(&config.comfyui_path).join("custom_nodes");
            let target_dir = custom_nodes_dir.join(&node_name);
            let venv_path = config.venv_path.clone();
            drop(config);

            let emit = |step: &str, message: &str, done: bool| {
                state.broadcast(
                    "install:progress",
                    serde_json::json!({
                        "node_name": node_name,
                        "step": step,
                        "message": message,
                        "done": done,
                    }),
                );
            };

            if target_dir.exists() {
                emit("done", "Already installed", true);
                return Ok(serde_json::json!(null));
            }

            emit("clone", &format!("Cloning {}...", node_name), false);

            let status = tokio::process::Command::new("git")
                .args([
                    "clone",
                    "--progress",
                    &git_url,
                    &target_dir.to_string_lossy(),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .map_err(|e| format!("git clone failed to start: {}", e))?;

            if !status.success() {
                emit("error", "git clone failed", true);
                return Err("git clone failed".to_string());
            }

            let req_file = target_dir.join("requirements.txt");
            if req_file.exists() {
                emit("pip", "Installing Python dependencies...", false);
                let uv_path = crate::commands::api::resolve_uv_bin_pub(&venv_path);
                let pip_status = if uv_path.exists() {
                    tokio::process::Command::new(&uv_path)
                        .args(["pip", "install", "-r", &req_file.to_string_lossy()])
                        .env("VIRTUAL_ENV", &venv_path)
                        .status()
                        .await
                        .map_err(|e| format!("uv pip install failed: {}", e))?
                } else {
                    #[cfg(target_os = "windows")]
                    let pip_path = format!("{}/Scripts/pip.exe", venv_path);
                    #[cfg(not(target_os = "windows"))]
                    let pip_path = format!("{}/bin/pip", venv_path);
                    tokio::process::Command::new(&pip_path)
                        .args(["install", "-r", &req_file.to_string_lossy()])
                        .status()
                        .await
                        .map_err(|e| format!("pip install failed: {}", e))?
                };
                if !pip_status.success() {
                    emit(
                        "error",
                        "pip install failed (some features may not work)",
                        false,
                    );
                }
            }

            emit(
                "done",
                &format!("{} installed successfully", node_name),
                true,
            );
            state.broadcast("custom_node:installed", serde_json::json!(node_name));
            Ok(serde_json::json!(null))
        }
        "install_pip_package" => {
            let package = args["package"]
                .as_str()
                .ok_or("Missing package")?
                .to_string();
            let config = state.config.read().await;
            let venv_path = config.venv_path.clone();
            drop(config);

            let uv_path = crate::commands::api::resolve_uv_bin_pub(&venv_path);

            let output = if uv_path.exists() {
                tokio::process::Command::new(&uv_path)
                    .args(["pip", "install", &package])
                    .env("VIRTUAL_ENV", &venv_path)
                    .output()
                    .await
                    .map_err(|e| format!("uv pip install failed to start: {}", e))?
            } else {
                #[cfg(target_os = "windows")]
                let pip_path = format!("{}/Scripts/pip.exe", venv_path);
                #[cfg(not(target_os = "windows"))]
                let pip_path = format!("{}/bin/pip", venv_path);
                tokio::process::Command::new(&pip_path)
                    .args(["install", &package])
                    .output()
                    .await
                    .map_err(|e| format!("pip install failed to start: {}", e))?
            };

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("pip install {} failed: {}", package, stderr));
            }
            log::info!("Installed pip package (browser mode): {}", package);
            Ok(serde_json::json!(null))
        }

        // --- Model info ---
        "get_model_install_dirs" | "find_model_by_hash" | "hash_model_file" | "read_modelspec" => {
            Err(format!(
                "Command '{}' not yet available in browser mode",
                command
            ))
        }

        // --- CivitAI ---
        "civitai_search_models" => {
            let params: crate::commands::api::CivitaiSearchParams =
                serde_json::from_value(args["params"].clone())
                    .map_err(|e| format!("Invalid params: {}", e))?;

            let encode_val = |v: &str| -> String {
                url::form_urlencoded::byte_serialize(v.as_bytes()).collect()
            };

            let mut parts: Vec<String> = vec![
                format!(
                    "sort={}",
                    encode_val(&params.sort.unwrap_or_else(|| "Most Downloaded".to_string()))
                ),
                format!(
                    "period={}",
                    encode_val(&params.period.unwrap_or_else(|| "AllTime".to_string()))
                ),
                format!("nsfw={}", params.nsfw.unwrap_or(false)),
                format!("limit={}", params.limit.unwrap_or(20)),
            ];

            let has_query = params
                .query
                .as_ref()
                .filter(|v| !v.trim().is_empty())
                .is_some();
            if !has_query {
                parts.push(format!("page={}", params.page.unwrap_or(1)));
            }
            if let Some(cursor) = params.cursor.filter(|v| !v.trim().is_empty()) {
                parts.push(format!("cursor={}", encode_val(&cursor)));
            }
            if let Some(q) = params.query.filter(|v| !v.trim().is_empty()) {
                parts.push(format!("query={}", encode_val(&q)));
            }
            if let Some(t) = params.model_type.filter(|v| !v.trim().is_empty()) {
                parts.push(format!("types[]={}", encode_val(&t)));
            }
            if let Some(base_model) = params.base_model.filter(|v| !v.trim().is_empty()) {
                parts.push(format!("baseModels[]={}", encode_val(&base_model)));
            }
            if let Some(file_format) = params.file_format.filter(|v| !v.trim().is_empty()) {
                parts.push(format!("fileFormats[]={}", encode_val(&file_format)));
            }

            let url = format!("https://civitai.com/api/v1/models?{}", parts.join("&"));
            let mut req = state
                .http_client
                .get(&url)
                .header("Accept", "application/json")
                .header("User-Agent", "MooshieUI/0.3.9");
            if let Some(key) = params.api_key.filter(|v| !v.trim().is_empty()) {
                req = req.bearer_auth(key);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("CivitAI API error {}: {}", status, body));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            Ok(data)
        }
        "civitai_list_architectures" => {
            let api_key = args
                .get("apiKey")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // Return the hardcoded common architectures (matching the Tauri command)
            let mut architectures: Vec<String> = vec![
                "SD 1.4",
                "SD 1.5",
                "SD 1.5 LCM",
                "SD 1.5 Hyper",
                "SD 2.0",
                "SD 2.0 768",
                "SD 2.1",
                "SD 2.1 768",
                "SD 2.1 Unclip",
                "SD 3",
                "SD 3.5",
                "SD 3.5 Large",
                "SD 3.5 Large Turbo",
                "SD 3.5 Medium",
                "SDXL 0.9",
                "SDXL 1.0",
                "SDXL 1.0 LCM",
                "SDXL Distilled",
                "SDXL Turbo",
                "SDXL Lightning",
                "SDXL Hyper",
                "Illustrious",
                "NoobAI",
                "Pony",
                "Flux.1 S",
                "Flux.1 D",
                "Flux.1 S Turbo",
                "AuraFlow",
                "Hunyuan 1",
                "HunyuanDiT",
                "Hunyuan Video",
                "Lumina",
                "Kolors",
                "PixArt-a",
                "PixArt-E",
                "Stable Cascade",
                "SVD",
                "SVD XT",
                "PlaygroundV2.5",
                "CogVideoX",
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect();

            // Try to fetch more from CivitAI API
            let mut req = state
                .http_client
                .get("https://civitai.com/api/v1/models?limit=1")
                .header("User-Agent", "MooshieUI/0.3.9");
            if let Some(ref key) = api_key {
                req = req.bearer_auth(key);
            }
            // Best-effort — don't fail if CivitAI is unreachable
            if let Ok(resp) = req.send().await {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if let Some(items) = data.get("items").and_then(|i| i.as_array()) {
                        for item in items {
                            if let Some(versions) =
                                item.get("modelVersions").and_then(|v| v.as_array())
                            {
                                for version in versions {
                                    if let Some(bm) =
                                        version.get("baseModel").and_then(|b| b.as_str())
                                    {
                                        let s = bm.to_string();
                                        if !architectures.contains(&s) {
                                            architectures.push(s);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            architectures.sort();
            architectures.dedup();
            serde_json::to_value(architectures).map_err(|e| e.to_string())
        }
        "civitai_lookup_hash" => {
            let hash = args["hash"].as_str().ok_or("Missing hash")?.to_string();
            let api_key = state.config.read().await.civitai_api_key.clone();
            let url = format!("https://civitai.com/api/v1/model-versions/by-hash/{}", hash);
            let mut req = state.http_client.get(&url);
            if let Some(ref key) = api_key {
                req = req.header("Authorization", format!("Bearer {}", key));
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            Ok(val)
        }
        "get_lora_civitai_info" | "get_checkpoint_civitai_info" => {
            // These are complex — need config values and hash computation.
            // Deferred to future implementation.
            Err(format!(
                "Command '{}' not yet available in browser mode",
                command
            ))
        }
        "fetch_cached_image" => {
            let url = args["url"].as_str().ok_or("Missing url")?.to_string();
            let resp = state
                .http_client
                .get(&url)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
            use base64::{engine::general_purpose::STANDARD, Engine};
            let b64 = STANDARD.encode(&bytes);
            Ok(serde_json::json!(b64))
        }

        // --- ComfyUI node checks ---
        "check_node_available" => {
            let node_class = args["nodeClass"]
                .as_str()
                .ok_or("Missing nodeClass")?
                .to_string();
            match state.api_get(&format!("/object_info/{}", node_class)).await {
                Ok(val) => Ok(serde_json::json!(val.get(&node_class).is_some())),
                Err(_) => Ok(serde_json::json!(false)),
            }
        }
        "is_custom_node_installed" => {
            let node_name = args["nodeName"]
                .as_str()
                .ok_or("Missing nodeName")?
                .to_string();
            let config = state.config.read().await;
            let target_dir = std::path::Path::new(&config.comfyui_path)
                .join("custom_nodes")
                .join(&node_name);
            Ok(serde_json::json!(target_dir.exists()))
        }

        // --- Config extras ---
        "set_gallery_path" => {
            let path = args["path"].as_str().unwrap_or("").to_string();
            let trimmed = path.trim().to_string();
            let resolved = if trimmed.is_empty() {
                let mut cfg = state.config.write().await;
                cfg.gallery_path = None;
                config::save_config(&cfg).map_err(|e| e)?;
                let dir = config::app_data_dir()
                    .ok_or("Cannot find app data directory")?
                    .join("gallery");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                dir.to_string_lossy().into_owned()
            } else {
                let p = std::path::Path::new(&trimmed);
                std::fs::create_dir_all(p)
                    .map_err(|e| format!("Cannot create gallery directory: {}", e))?;
                let mut cfg = state.config.write().await;
                cfg.gallery_path = Some(trimmed.clone());
                config::save_config(&cfg).map_err(|e| e)?;
                trimmed
            };
            Ok(serde_json::json!(resolved))
        }

        // --- Misc ---
        "fetch_release_notes" => {
            let resp = state
                .http_client
                .get("https://api.github.com/repos/Mooshieblob1/MooshieUI/releases")
                .query(&[("per_page", "20")])
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "MooshieUI-Desktop")
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let releases: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            Ok(releases)
        }
        "export_logs" => {
            let destination = args["destination"]
                .as_str()
                .ok_or("Missing destination")?
                .to_string();
            // Simplified: just write config info to the destination
            let cfg = state.config.read().await;
            let info = format!("MooshieUI Log Export\nConfig: {:?}", *cfg);
            std::fs::write(&destination, info).map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }

        "download_model" => {
            let url = args["url"].as_str().ok_or("Missing url")?.to_string();
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let install_dir = args["installDir"].as_str().map(|s| s.to_string());
            let expected_sha256 = args["expectedSha256"].as_str().map(|s| s.to_string());

            // Resolve destination directory
            let models_dir = if let Some(ref dir) = install_dir {
                std::path::PathBuf::from(dir)
            } else {
                let cfg = state.config.read().await;
                let comfyui_path = if cfg.comfyui_path.is_empty() {
                    ".".to_string()
                } else {
                    cfg.comfyui_path.clone()
                };
                std::path::Path::new(&comfyui_path)
                    .join("models")
                    .join(&category)
            };
            tokio::fs::create_dir_all(&models_dir)
                .await
                .map_err(|e| e.to_string())?;
            let dest = models_dir.join(&filename);

            // Skip if file exists and is valid
            if dest.exists() {
                let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                if size > 0 {
                    if let Some(ref expected_hex) = expected_sha256 {
                        let dest_clone = dest.clone();
                        let expected = expected_hex.to_lowercase();
                        let computed = tokio::task::spawn_blocking(move || {
                            crate::comfyui::client::sha256_file(&dest_clone)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?;
                        if computed == expected {
                            return Ok(serde_json::json!(null));
                        }
                        let _ = std::fs::remove_file(&dest);
                    } else {
                        return Ok(serde_json::json!(null));
                    }
                } else {
                    let _ = std::fs::remove_file(&dest);
                }
            }

            // Download with progress broadcast
            let event_tx = state.event_tx.clone();
            let resp = state
                .http_client
                .get(&url)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!(
                    "Failed to download {}: HTTP {}",
                    url,
                    resp.status()
                ));
            }
            let total = resp.content_length().unwrap_or(0);
            let mut downloaded: u64 = 0;
            let mut file = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
            let mut last_emit: u64 = 0;

            let progress_event =
                |tx: &tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
                 fname: &str,
                 dl: u64,
                 tot: u64,
                 done: bool| {
                    let _ = tx.send(crate::state::BroadcastEvent {
                        event: "download:progress".to_string(),
                        payload: serde_json::json!({
                            "filename": fname,
                            "downloaded": dl,
                            "total": tot,
                            "done": done,
                        }),
                    });
                };

            progress_event(&event_tx, &filename, 0, total, false);
            let mut resp = resp;
            while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
                use std::io::Write;
                if let Err(e) = file.write_all(&chunk) {
                    drop(file);
                    let _ = std::fs::remove_file(&dest);
                    return Err(e.to_string());
                }
                downloaded += chunk.len() as u64;
                if downloaded - last_emit > 256 * 1024 || downloaded == total {
                    last_emit = downloaded;
                    progress_event(&event_tx, &filename, downloaded, total, false);
                }
            }
            progress_event(&event_tx, &filename, downloaded, total, true);

            // Verify SHA256 if provided
            if let Some(ref expected_hex) = expected_sha256 {
                let dest_clone = dest.clone();
                let expected = expected_hex.to_lowercase();
                let computed = tokio::task::spawn_blocking(move || {
                    crate::comfyui::client::sha256_file(&dest_clone)
                })
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
                if computed != expected {
                    let _ = std::fs::remove_file(&dest);
                    return Err(format!(
                        "SHA256 mismatch: expected {}, got {}",
                        expected, computed
                    ));
                }
            }

            Ok(serde_json::json!(null))
        }

        // --- Interrogator ---
        "interrogate_image" => {
            let image_base64 = args["imageBase64"]
                .as_str()
                .ok_or("Missing imageBase64")?
                .to_string();
            use base64::Engine;
            let image_bytes = base64::engine::general_purpose::STANDARD
                .decode(&image_base64)
                .map_err(|e| format!("Invalid base64: {}", e))?;
            let result = run_interrogation_headless(&state, image_bytes).await?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "interrogate_image_path" => {
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            let image_bytes =
                std::fs::read(&path).map_err(|e| format!("Failed to read image: {}", e))?;
            let result = run_interrogation_headless(&state, image_bytes).await?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "interrogate_gallery_image" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                return Err("Invalid filename".to_string());
            }
            let dir = crate::config::gallery_dir().ok_or("Cannot find gallery directory")?;
            let path = dir.join(&filename);
            let image_bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let result = run_interrogation_headless(&state, image_bytes).await?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "interrogate_clipboard" => Err(
            "interrogate_clipboard not available in browser mode (no clipboard access)".to_string(),
        ),

        // --- File operations ---
        "save_image_file" => {
            let image_bytes: Vec<u8> = serde_json::from_value(args["imageBytes"].clone())
                .map_err(|e| format!("Invalid imageBytes: {}", e))?;
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            std::fs::write(&path, &image_bytes).map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "upload_image" => {
            let image_path = args["imagePath"]
                .as_str()
                .ok_or("Missing imagePath")?
                .to_string();
            let bytes =
                std::fs::read(&image_path).map_err(|e| format!("Failed to read image: {}", e))?;
            let fname = std::path::Path::new(&image_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let result = state
                .upload_image_from_bytes(bytes, fname)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "open_directory" => {
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            std::fs::create_dir_all(&path).ok();
            #[cfg(target_os = "windows")]
            {
                let _ = tokio::process::Command::new("explorer.exe")
                    .arg(&path)
                    .spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = tokio::process::Command::new("open").arg(&path).spawn();
            }
            #[cfg(target_os = "linux")]
            {
                let _ = tokio::process::Command::new("xdg-open").arg(&path).spawn();
            }
            Ok(serde_json::json!(null))
        }

        // --- Clipboard ---
        "copy_image_to_clipboard" => {
            let file_path = args["filePath"]
                .as_str()
                .ok_or("Missing filePath")?
                .to_string();
            crate::commands::api::copy_image_to_clipboard(file_path)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "copy_bytes_to_clipboard" => {
            let bytes: Vec<u8> = serde_json::from_value(args["bytes"].clone())
                .map_err(|e| format!("Invalid bytes: {}", e))?;
            let ext = args["ext"].as_str().ok_or("Missing ext")?.to_string();
            crate::commands::api::copy_bytes_to_clipboard(bytes, ext)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "read_clipboard_image" => {
            Err("read_clipboard_image is not yet available in browser mode".to_string())
        }

        // For commands not yet mapped, return an error
        _ => Err(format!(
            "Command '{}' not implemented in browser mode",
            command
        )),
    }
}

// ---------------------------------------------------------------------------
// Interrogation helper (headless — no AppHandle)
// ---------------------------------------------------------------------------

/// Run interrogation without AppHandle (browser mode).
/// Emits progress via the broadcast channel instead of Tauri events.
async fn run_interrogation_headless(
    state: &Arc<AppState>,
    image_bytes: Vec<u8>,
) -> Result<crate::interrogator::InterrogationResult, String> {
    // Ensure model is downloaded
    {
        let interrogator = state.interrogator.read().await;
        if !interrogator.is_model_downloaded() {
            drop(interrogator);
            let interrogator = state.interrogator.read().await;
            interrogator
                .ensure_model_downloaded_headless(&state.http_client)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    // Ensure ONNX Runtime shared library is downloaded
    {
        let interrogator = state.interrogator.read().await;
        if !interrogator.is_ort_library_present() {
            drop(interrogator);
            let interrogator = state.interrogator.read().await;
            interrogator
                .ensure_ort_library_headless(&state.http_client)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    let (general_threshold, character_threshold) = {
        let config = state.config.read().await;
        (
            config.interrogator_general_threshold,
            config.interrogator_character_threshold,
        )
    };

    let event_tx = state.event_tx.clone();
    let interrogator = state.interrogator.clone();
    tokio::task::spawn_blocking(move || {
        let mut guard = interrogator.blocking_write();
        let is_first_load = guard.session_not_loaded();
        if is_first_load {
            let _ = event_tx.send(crate::state::BroadcastEvent {
                event: "interrogator:stage".to_string(),
                payload: serde_json::json!("loading_model"),
            });
        }
        guard.load_session().map_err(|e| e.to_string())?;
        let _ = event_tx.send(crate::state::BroadcastEvent {
            event: "interrogator:stage".to_string(),
            payload: serde_json::json!("running_inference"),
        });
        guard
            .run_inference(&image_bytes, general_threshold, character_threshold)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Inference task failed: {}", e))?
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
        Ok((token, must_change)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "token": token,
                "must_change_password": must_change,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /internal-api/_auth/register — create a new account. Admin only.
async fn auth_register_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<AuthRequest>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin {
        return forbidden_response("Only the admin (localhost) can create accounts.");
    }
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
                Ok((token, _)) => {
                    (StatusCode::OK, Json(serde_json::json!({ "token": token }))).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// GET /internal-api/_auth/status — check if auth is required, accounts exist, and caller's role.
async fn auth_status_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    let role = resolve_role(&state, &headers, &remote);
    let role_str = match role {
        UserRole::Admin => "admin",
        UserRole::Moderator => "moderator",
        UserRole::User => "user",
        UserRole::Anonymous => "anonymous",
    };
    Json(serde_json::json!({
        "auth_required": state.lan_enabled && state.auth.has_accounts(),
        "has_accounts": state.auth.has_accounts(),
        "role": role_str,
        "lan_enabled": state.lan_enabled,
    }))
}

/// GET /internal-api/_auth/accounts — list all accounts with roles. Admin only.
async fn auth_list_accounts_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin {
        return forbidden_response("Only the admin (localhost) can list accounts.");
    }
    let accounts: Vec<serde_json::Value> = state
        .auth
        .list_accounts_with_roles()
        .into_iter()
        .map(|(username, role)| serde_json::json!({ "username": username, "role": role }))
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({ "accounts": accounts })),
    )
        .into_response()
}

/// POST /internal-api/_auth/delete — delete an account by username. Admin only.
async fn auth_delete_account_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin {
        return forbidden_response("Only the admin (localhost) can delete accounts.");
    }
    let username = match req.get("username").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing username" })),
            )
                .into_response();
        }
    };
    match state.auth.delete_account(username) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /internal-api/_auth/change_password — user changes their own password.
/// Requires valid session token + current password.
async fn auth_change_password_handler(
    AxumState(state): AxumState<SharedState>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    // Must be authenticated
    let token = extract_token(&headers);
    let username = match token.as_deref().and_then(|t| state.auth.validate_token(t)) {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Not authenticated" })),
            )
                .into_response();
        }
    };

    let current = match req.get("current_password").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing current_password" })),
            )
                .into_response();
        }
    };
    let new_pass = match req.get("new_password").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing new_password" })),
            )
                .into_response();
        }
    };

    match state.auth.change_password(&username, current, new_pass) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /internal-api/_auth/reset_password — admin sets a temporary password.
/// The user will be forced to choose a new password on next login.
async fn auth_reset_password_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin {
        return forbidden_response("Only the admin (localhost) can reset passwords.");
    }

    let username = match req.get("username").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing username" })),
            )
                .into_response();
        }
    };
    let temp_pass = match req.get("temp_password").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing temp_password" })),
            )
                .into_response();
        }
    };

    match state.auth.reset_password(username, temp_pass) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /internal-api/_auth/set_role — admin sets the role of an account.
async fn auth_set_role_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin {
        return forbidden_response("Only the admin (localhost) can change user roles.");
    }
    let username = match req.get("username").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing username" })),
            )
                .into_response();
        }
    };
    let new_role = match req.get("role").and_then(|v| v.as_str()) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing role" })),
            )
                .into_response();
        }
    };
    match state.auth.set_account_role(username, new_role) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// GET /internal-api/_auth/lan_info — return the machine's LAN IPs and port. Admin only.
async fn auth_lan_info_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin {
        return forbidden_response("Only the admin (localhost) can view LAN info.");
    }
    let port = {
        let cfg = state.app.config.read().await;
        cfg.ui_server_port
    };
    let ips = get_lan_ips();
    let addresses: Vec<String> = ips
        .iter()
        .map(|ip| format!("http://{}:{}", ip, port))
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "addresses": addresses,
            "port": port,
        })),
    )
        .into_response()
}

/// Detect LAN-routable IPv4 addresses by probing a UDP socket.
fn get_lan_ips() -> Vec<String> {
    let mut ips = Vec::new();
    // Primary method: connect a UDP socket to a public IP to find the default route
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip().to_string();
                if ip != "0.0.0.0" && ip != "127.0.0.1" {
                    ips.push(ip);
                }
            }
        }
    }
    if ips.is_empty() {
        ips.push("<unknown>".to_string());
    }
    ips
}

/// Resolve the gallery directory for a given user.
/// Admin/localhost (username=None) uses the root gallery dir.
/// LAN users get a per-user subdirectory: `gallery/users/{username}/`.
fn user_gallery_dir(username: Option<&str>) -> Option<std::path::PathBuf> {
    let base = config::gallery_dir()?;
    match username {
        Some(name) => {
            // Sanitise the username to prevent path traversal
            let safe = name.replace(['/', '\\', '.'], "_");
            Some(base.join("users").join(safe))
        }
        None => Some(base),
    }
}

/// Save image bytes to a specific gallery directory with metadata embedding.
/// This is a per-directory variant of `commands::api::save_to_gallery_inner`.
fn save_to_gallery_in_dir(
    dir: &std::path::Path,
    bytes: &[u8],
    filename: &str,
    prompt_id: &str,
    mode: Option<&str>,
    metadata: Option<&std::collections::HashMap<String, String>>,
    metadata_mode: Option<&str>,
) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    let normalized_mode = match mode {
        Some("txt2img") => "txt2img",
        Some("img2img") => "img2img",
        Some("inpainting") => "inpainting",
        _ => "unknown",
    };

    let gallery_filename = format!("{}__{}__{}", prompt_id, normalized_mode, filename);
    let path = dir.join(&gallery_filename);

    let raw_mode = metadata_mode.unwrap_or("text_chunk");
    let mut embed_mode = crate::metadata::MetadataMode::from_str(raw_mode);

    if filename.to_ascii_lowercase().ends_with(".png")
        && embed_mode == crate::metadata::MetadataMode::StealthAlpha
    {
        if let Ok(true) = crate::metadata::is_png_16bit(bytes) {
            embed_mode = crate::metadata::MetadataMode::Both;
        }
    }

    let final_bytes = if let Some(meta) = metadata {
        if filename.to_ascii_lowercase().ends_with(".png") {
            crate::metadata::embed_png_metadata(bytes, meta, embed_mode)
                .unwrap_or_else(|_| bytes.to_vec())
        } else {
            bytes.to_vec()
        }
    } else {
        bytes.to_vec()
    };

    std::fs::write(&path, &final_bytes).map_err(|e| e.to_string())?;
    Ok(gallery_filename)
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
                // If we've switched to app mode, the watchdog should stop.
                if state
                    .app_mode_active
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    log::info!("Heartbeat watchdog stopping — app mode is active");
                    break;
                }
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
