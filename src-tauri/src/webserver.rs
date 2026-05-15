//! Embedded HTTP server for browser mode.
//!
//! Serves the Svelte frontend as static files, proxies IPC commands as REST
//! endpoints, streams events via SSE, and handles heartbeat keep-alive.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::{future::Future, pin::Pin};

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

/// Frontend assets embedded at compile time from `../dist/`. Used as a
/// fallback when the on-disk dist directory isn't found (e.g. installed
/// production builds where the dist folder isn't unpacked next to the exe).
/// In dev builds rust-embed reads from disk at runtime, so `npm run dev`
/// output is picked up without rebuilding the Rust binary.
#[derive(rust_embed::Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../dist/"]
struct FrontendAssets;

/// Shared state for axum handlers.
pub struct WebState {
    pub app: Arc<AppState>,
    pub auth: Arc<AuthState>,
    pub lan_enabled: bool,
}

pub type SharedState = Arc<WebState>;

type BackgroundTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[cfg(feature = "desktop")]
fn spawn_background(task: BackgroundTask) {
    tauri::async_runtime::spawn(task);
}

#[cfg(not(feature = "desktop"))]
fn spawn_background(task: BackgroundTask) {
    tokio::spawn(task);
}

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
    // Check bearer token — all remote users must authenticate
    if let Some(token) = extract_token(headers) {
        if let Some(username) = state.auth.validate_token(&token) {
            if let Some(role) = state.auth.get_account_role(&username) {
                if role == "admin" {
                    return UserRole::Admin;
                }
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
        if let Some(username) = state.auth.validate_token(&token) {
            // Authenticated admin accounts use the shared gallery, same as localhost
            if state.auth.get_account_role(&username).as_deref() == Some("admin") {
                return None;
            }
            return Some(username);
        }
    }
    None
}

/// Commands that moderators (and admins) can execute.
/// Moderators have full operational access; filesystem/server panels are
/// hidden in the UI for mods but all commands are permitted at the API level.
const MODERATOR_COMMANDS: &[&str] = &[
    // server / config control
    "update_config",
    "stop_comfyui",
    "kill_port_process",
    "export_logs",
    "install_pip_package",
    "clear_all_queues",
    // previously admin-only: mode switching, filesystem, node install
    "switch_to_app_mode",
    "set_gallery_path",
    "install_custom_node",
    "import_image_directory",
    "open_directory",
    "move_installation",
    "read_image_metadata_path",
    "save_image_file",
    "save_text_file",
    "upload_image",
];

/// Model Hub commands that require explicit per-user access for regular users.
const MODELHUB_COMMANDS: &[&str] = &[
    "civitai_search_models",
    "civitai_list_architectures",
    "civitai_lookup_hash",
    "download_model",
    "get_model_install_dirs",
    "get_lora_civitai_info",
    "get_checkpoint_civitai_info",
];

fn is_modelhub_command(command: &str) -> bool {
    MODELHUB_COMMANDS.contains(&command)
}

/// Check command permission level.
/// Returns the minimum role required to execute the command.
fn min_role_for_command(command: &str) -> UserRole {
    if MODERATOR_COMMANDS.contains(&command) {
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
///
/// Attempts to bind to `port`; if that port is already in use, tries the
/// next 9 sequential ports (e.g. 3200 → 3201 → … → 3209).  Returns a tuple
/// of `(actual_bound_port, JoinHandle)` so callers can open the correct
/// browser URL even when fallback ports were used.
///
/// Panics if none of the candidate ports can be bound.
/// Spawn the prompt-queue cleanup reactor.  Listens on the shared broadcast
/// channel for ComfyUI completion/error events and:
///   * releases the GPU worker that handled the prompt
///   * removes the prompt from the fair queue
///   * notifies the held-prompt drain reactor
///
/// Idempotent — calling this more than once is a no-op.  Must be started in
/// both desktop and browser modes, otherwise workers stay reserved forever
/// after the first successful prompt and subsequent `submit_prompt` calls
/// block for the full 300s timeout.
pub fn spawn_prompt_cleanup_reactor(state: Arc<AppState>) {
    if state
        .cleanup_reactors_started
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }

    let cleanup_state = state;
    let mut cleanup_rx = cleanup_state.event_tx.subscribe();
    spawn_background(Box::pin(async move {
        loop {
            match cleanup_rx.recv().await {
                Ok(evt) => {
                    let prompt_id = evt
                        .payload
                        .get("prompt_id")
                        .and_then(|v| v.as_str())
                        .map(|s| cleanup_state.prompt_queue.resolve_alias(s));

                    match evt.event.as_str() {
                        "comfyui:executing" => {
                            if evt.payload.get("node").is_some_and(|n| n.is_null()) {
                                if let Some(pid) = prompt_id {
                                    let owner = cleanup_state.prompt_queue.owner_of(&pid);
                                    log::info!(
                                        "[gen] completed prompt={} user={}",
                                        &pid[..8.min(pid.len())],
                                        owner.as_deref().unwrap_or("admin"),
                                    );
                                    if let Some(wid) = cleanup_state.prompt_queue.finish(&pid) {
                                        cleanup_state.gpu_manager.mark_worker_idle(wid).await;
                                    }
                                    let alias_pid = pid.clone();
                                    let alias_state = cleanup_state.clone();
                                    tokio::spawn(async move {
                                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                        alias_state.prompt_queue.cleanup_alias(&alias_pid);
                                    });
                                    cleanup_state.broadcast_queue_positions();
                                    cleanup_state.prompt_queue.drain_notify.notify_one();
                                }
                            }
                        }
                        "comfyui:execution_error" => {
                            if let Some(pid) = prompt_id {
                                let owner = cleanup_state.prompt_queue.owner_of(&pid);
                                log::warn!(
                                    "[gen] error prompt={} user={}",
                                    &pid[..8.min(pid.len())],
                                    owner.as_deref().unwrap_or("admin"),
                                );
                                if let Some(wid) = cleanup_state.prompt_queue.finish(&pid) {
                                    cleanup_state
                                        .gpu_manager
                                        .mark_worker_error_then_idle(wid)
                                        .await;
                                }
                                let alias_pid = pid.clone();
                                let alias_state = cleanup_state.clone();
                                tokio::spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                    alias_state.prompt_queue.cleanup_alias(&alias_pid);
                                });
                                cleanup_state.broadcast_queue_positions();
                                cleanup_state.prompt_queue.drain_notify.notify_one();
                            }
                        }
                        _ => {}
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("Queue cleanup reactor lagged by {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));
}

/// Spawn the stuck-worker watchdog.  Every 60s, checks for workers that have
/// been reserved for longer than 10 minutes without a corresponding queue
/// entry and force-releases them.  Catches cases where the WebSocket
/// completion event was missed.
///
/// This uses the same idempotency flag as the cleanup reactor; call
/// [`spawn_prompt_cleanup_reactor`] first (or rely on [`start_server`] /
/// app init which calls both).
pub fn spawn_stuck_worker_watchdog(state: Arc<AppState>) {
    let watchdog_state = state;
    spawn_background(Box::pin(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        let max_stuck_secs = 600u64;
        loop {
            interval.tick().await;
            for worker in &watchdog_state.gpu_manager.workers {
                let status = *worker.status.read().await;
                let reserved = worker.reserved.load(std::sync::atomic::Ordering::Acquire);
                if status == crate::comfyui::gpu_manager::WorkerStatus::Running && reserved {
                    let has_active_prompt = {
                        let wmap = watchdog_state.prompt_queue.worker_map_snapshot();
                        wmap.values().any(|&wid| wid == worker.id)
                    };
                    if !has_active_prompt {
                        let last_released = worker
                            .last_released
                            .load(std::sync::atomic::Ordering::Acquire);
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let stuck_secs = if last_released == 0 {
                            now_ms / 1000
                        } else {
                            (now_ms.saturating_sub(last_released)) / 1000
                        };
                        if stuck_secs > max_stuck_secs {
                            log::warn!(
                                "[watchdog] Releasing stuck worker {} (GPU {}, stuck {}s, no queue entry)",
                                worker.id,
                                worker.gpu_index,
                                stuck_secs,
                            );
                            watchdog_state.gpu_manager.mark_worker_idle(worker.id).await;
                        }
                    }
                }
            }
        }
    }));
}

pub async fn start_server(
    state: Arc<AppState>,
    port: u16,
    lan_enabled: bool,
) -> (u16, tokio::task::JoinHandle<()>) {
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
        .route(
            "/internal-api/_auth/set_modelhub_access",
            post(auth_set_modelhub_access_handler),
        )
        .route("/internal-api/_auth/logout", post(auth_logout_handler))
        .route("/internal-api/_auth/lan_info", get(auth_lan_info_handler))
        // Storage management
        .route("/internal-api/_storage/info", get(storage_info_handler))
        .route(
            "/internal-api/_storage/set_limit",
            post(storage_set_limit_handler),
        )
        // Health check (unauthenticated, for K8s probes)
        .route("/health", get(health_handler))
        // Update check (admin/moderator only)
        .route("/internal-api/_check_update", get(check_update_handler))
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
        // Full gallery image endpoint (serves original PNG/JPEG with metadata)
        .route(
            "/internal-api/_gallery/{filename}",
            get(gallery_image_handler),
        )
        // Temp image endpoint (ephemeral images from WS for SSE clients)
        .route(
            "/internal-api/_temp_image/{filename}",
            get(temp_image_handler),
        )
        // Embed metadata into a temp image and return a new temp URL
        .route(
            "/internal-api/_embed_temp_metadata",
            post(embed_temp_metadata_handler),
        )
        // GPU stats — available to all authenticated users
        .route("/internal-api/_gpu_stats", get(gpu_stats_handler))
        // CDN proxy — serves assets from cdn.mooshieblob.com to avoid CORS issues
        .route("/internal-api/_cdn/{*path}", get(cdn_proxy_handler))
        // Generic IPC command proxy
        .route("/internal-api/{command}", post(command_handler))
        // Static file serving (frontend)
        .fallback(get(move |req: axum::extract::Request| {
            let dist = dist_dir.clone();
            async move { serve_static(dist, req).await }
        }))
        // Images sent as JSON arrays of numbers inflate ~4x, so allow large bodies
        .layer(axum::extract::DefaultBodyLimit::max(256 * 1024 * 1024))
        .with_state(web_state.clone());

    let host: [u8; 4] = if lan_enabled {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    };

    // Probe upward from the configured port until we find a free one.  This
    // keeps development smooth when 3200 is held by a crashed webview or
    // another tool — we just move to 3201, 3202, ... automatically.
    const MAX_PORT_ATTEMPTS: u16 = 10;
    let mut listener: Option<tokio::net::TcpListener> = None;
    let mut bound_addr: Option<SocketAddr> = None;
    for offset in 0..MAX_PORT_ATTEMPTS {
        let candidate = SocketAddr::from((host, port.saturating_add(offset)));
        match tokio::net::TcpListener::bind(candidate).await {
            Ok(l) => {
                if offset > 0 {
                    log::warn!(
                        "UI web server port {} in use; falling back to {}",
                        port,
                        candidate.port(),
                    );
                }
                listener = Some(l);
                bound_addr = Some(candidate);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                log::debug!("Port {} in use, trying next", candidate.port());
                continue;
            }
            Err(e) => {
                panic!("Failed to bind UI web server on {}: {}", candidate, e);
            }
        }
    }

    let listener = listener.unwrap_or_else(|| {
        panic!(
            "Failed to bind UI web server: no free port in range {}..{}",
            port,
            port.saturating_add(MAX_PORT_ATTEMPTS),
        )
    });
    let bind_addr = bound_addr.expect("bound_addr set when listener is Some");

    log::info!("Starting UI web server on {}", bind_addr);

    // Start the shared prompt cleanup reactor + stuck-worker watchdog.
    // These are idempotent (guarded by web_state.app.cleanup_reactors_started)
    // so calling start_server multiple times (or from both desktop and browser
    // modes) won't spawn duplicates.
    spawn_prompt_cleanup_reactor(web_state.app.clone());
    spawn_stuck_worker_watchdog(web_state.app.clone());

    // Spawn held-prompt drain reactor — when a prompt finishes, submits the next
    // held prompt to ComfyUI (one per user at a time, round-robin fair).
    {
        let drain_state = web_state.app.clone();
        tokio::spawn(async move {
            loop {
                drain_state.prompt_queue.drain_notify.notified().await;
                // Submit one held prompt per completion signal.
                if let Some(hp) = drain_state.prompt_queue.take_next_held() {
                    let timeout = std::time::Duration::from_secs(300);
                    let res = drain_state
                        .gpu_manager
                        .submit_prompt(hp.workflow, &drain_state.client_id, timeout)
                        .await;
                    match res {
                        Ok((worker_id, response)) => {
                            // Bind alias immediately to prevent race with WebSocket events
                            let was_deferred = drain_state
                                .prompt_queue
                                .bind_alias(&hp.placeholder_id, &response.prompt_id);
                            if was_deferred {
                                // Completion/error arrived before bind_alias; release worker.
                                drain_state
                                    .gpu_manager
                                    .mark_worker_error_then_idle(worker_id)
                                    .await;
                                *hp.result.lock().await =
                                    Some(Err("execution completed before alias bind".to_string()));
                                drain_state.prompt_queue.drain_notify.notify_one();
                            } else {
                                drain_state
                                    .prompt_queue
                                    .set_worker(&hp.placeholder_id, worker_id);
                                *hp.result.lock().await = Some(Ok((response.prompt_id, worker_id)));
                            }
                        }
                        Err(e) => {
                            *hp.result.lock().await =
                                Some(Err(format!("Queue prompt failed: {}", e)));
                        }
                    }
                    hp.submitted.notify_one();
                }
            }
        });
    }

    // Periodic flush of last_online timestamps to disk (every 60s).
    {
        let flush_auth = web_state.auth.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                flush_auth.flush_last_online();
            }
        });
    }

    // Stuck-worker watchdog is spawned by spawn_stuck_worker_watchdog() at
    // the top of this function.

    // Periodic image expiry cleanup — delete images older than 7 days (every 30 min).
    {
        let expiry_auth = web_state.auth.clone();
        tokio::spawn(async move {
            // Run once at startup after a short delay, then every 30 minutes
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30 * 60));
            loop {
                interval.tick().await;
                cleanup_expired_images(&expiry_auth);
            }
        });
    }

    let actual_port = bind_addr.port();
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("UI web server crashed");
    });
    (actual_port, handle)
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

    // No on-disk dist found — production builds rely on the compile-time
    // embedded FrontendAssets instead, so this is not necessarily an error.
    log::info!(
        "No on-disk frontend dist directory; using embedded assets. Searched: {:?}",
        candidates
    );
    candidates[0].clone()
}

/// Serve static files from the dist directory, falling back to assets
/// embedded into the binary at compile time.
///
/// Lookup order:
///   1. File on disk under `dist_dir` (prefers freshly-built `npm run dev`
///      / `npm run build` output for dev workflows).
///   2. Embedded asset at the same relative path.
///   3. Embedded `index.html` (SPA fallback).
///
/// Production installs typically hit path 2/3 because the Tauri bundle
/// embeds the frontend into the binary via the asset protocol and does
/// not copy `dist/` next to the exe. Before this fallback existed, every
/// browser-mode request in a production install returned 404 ("Not Found").
async fn serve_static(dist_dir: PathBuf, req: axum::extract::Request) -> Response {
    let raw_path = req.uri().path().trim_start_matches('/');
    let rel_path = if raw_path.is_empty() {
        "index.html"
    } else {
        raw_path
    };

    // 1. On-disk first so hot-reloaded dev builds override any stale
    //    compile-time embed.
    let disk_path = dist_dir.join(rel_path);
    if disk_path.is_file() {
        if let Ok(contents) = tokio::fs::read(&disk_path).await {
            return build_asset_response(rel_path, contents);
        }
    }

    // 2. Embedded fallback.
    if let Some(file) = FrontendAssets::get(rel_path) {
        return build_asset_response(rel_path, file.data.into_owned());
    }

    // 3. SPA fallback — serve index.html for unknown client-side routes.
    let index_disk = dist_dir.join("index.html");
    if index_disk.is_file() {
        if let Ok(contents) = tokio::fs::read(&index_disk).await {
            return build_asset_response("index.html", contents);
        }
    }
    if let Some(file) = FrontendAssets::get("index.html") {
        return build_asset_response("index.html", file.data.into_owned());
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

/// Build an HTTP response for a static asset. Injects the browser-mode flag
/// into HTML payloads so the frontend IPC layer routes through HTTP instead
/// of Tauri.
fn build_asset_response(rel_path: &str, contents: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(rel_path)
        .first_or_octet_stream()
        .to_string();

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

/// Health check endpoint for K8s liveness/readiness probes.
/// No authentication required.
async fn health_handler(AxumState(state): AxumState<SharedState>) -> Json<serde_json::Value> {
    let comfyui_running = state.app.comfyui_process.lock().await.is_some();
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "comfyui_running": comfyui_running,
    }))
}

/// GET /internal-api/_check_update — check GitHub for a newer release.
/// Returns `{ "update_available": bool, "latest_version": "x.y.z", "current_version": "x.y.z" }`.
/// Only accessible to admin/moderator; regular users get 403.
async fn check_update_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can check for updates.");
    }

    let current = env!("CARGO_PKG_VERSION");
    let url = "https://api.github.com/repos/Mooshieblob1/MooshieUI/releases/latest";

    let resp = state
        .app
        .http_client
        .get(url)
        .header("User-Agent", format!("MooshieUI/{}", current))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
            Ok(release) => {
                let tag = release["tag_name"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches('v');
                let update_available = version_newer_than(tag, current);
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "update_available": update_available,
                        "latest_version": tag,
                        "current_version": current,
                    })),
                )
                    .into_response()
            }
            Err(e) => {
                log::warn!("Failed to parse GitHub release response: {}", e);
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "update_available": false,
                        "latest_version": current,
                        "current_version": current,
                        "error": "Failed to parse release info",
                    })),
                )
                    .into_response()
            }
        },
        Ok(r) => {
            log::warn!("GitHub release check returned HTTP {}", r.status());
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "update_available": false,
                    "latest_version": current,
                    "current_version": current,
                    "error": format!("GitHub API returned {}", r.status()),
                })),
            )
                .into_response()
        }
        Err(e) => {
            log::warn!("Failed to check for updates: {}", e);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "update_available": false,
                    "latest_version": current,
                    "current_version": current,
                    "error": "Network error checking for updates",
                })),
            )
                .into_response()
        }
    }
}

/// Compare two semver-like version strings. Returns true if `latest` > `current`.
fn version_newer_than(latest: &str, current: &str) -> bool {
    let parse =
        |s: &str| -> Vec<u32> { s.split('.').filter_map(|p| p.parse::<u32>().ok()).collect() };
    let l = parse(latest);
    let c = parse(current);
    for i in 0..l.len().max(c.len()) {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv > cv {
            return true;
        }
        if lv < cv {
            return false;
        }
    }
    false
}

/// SSE endpoint — streams backend events to browser clients.
/// Events are filtered per-user: each user only receives events for their own
/// prompts, plus system-level events (connection status, queue updates).
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

    // Resolve the username for this SSE connection (None = admin)
    let sse_username = resolve_username(&state, &hdrs, &remote);
    let prompt_queue = state.app.clone();

    // Build the initial burst — queue positions + last preview frame for any
    // prompt this user already has in flight (handles page refresh mid-gen).
    let initial_events: Vec<Result<Event, std::convert::Infallible>> = {
        let app = state.app.clone();
        let queue = app.prompt_queue.queue.read().unwrap();
        let total = queue.len();
        let mut evts = Vec::new();
        for (pos, (pid, _owner)) in queue.iter().enumerate() {
            if app.prompt_queue.is_owned_by(pid, &sse_username) {
                let json = serde_json::json!({
                    "event": "mooshie:queue_update",
                    "payload": { "prompt_id": pid, "position": pos, "total": total }
                });
                evts.push(Ok(Event::default().data(json.to_string())));

                // Re-send last preview frame so the user sees the latest frame
                // immediately without waiting for the next ComfyUI preview tick.
                if let Some(temp_fn) = app
                    .last_preview_by_prompt
                    .read()
                    .unwrap()
                    .get(pid.as_str())
                    .cloned()
                {
                    let preview_json = serde_json::json!({
                        "event": "comfyui:preview",
                        "payload": { "temp_filename": temp_fn, "format": "jpeg", "prompt_id": pid }
                    });
                    evts.push(Ok(Event::default().data(preview_json.to_string())));
                }
            }
        }
        evts
    };

    let rx = state.app.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        let sse_username = sse_username.clone();
        let prompt_queue = prompt_queue.clone();
        match result {
            Ok(evt) => {
                // Resolve alias: translate ComfyUI's real prompt_id to our placeholder
                let raw_prompt_id = evt
                    .payload
                    .get("prompt_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let resolved_id = raw_prompt_id
                    .as_deref()
                    .map(|pid| prompt_queue.prompt_queue.resolve_alias(pid));

                // Filter by ownership using the resolved (placeholder) id
                if let Some(ref pid) = resolved_id {
                    if !prompt_queue.prompt_queue.is_owned_by(pid, &sse_username) {
                        return None; // Not this user's prompt — skip
                    }
                }

                // Replace prompt_id in payload with resolved placeholder so the
                // frontend sees the same ID it received from the generate response.
                let payload =
                    if let (Some(ref resolved), Some(ref raw)) = (&resolved_id, &raw_prompt_id) {
                        if resolved != raw {
                            let mut p = evt.payload.clone();
                            p["prompt_id"] = serde_json::Value::String(resolved.clone());
                            p
                        } else {
                            evt.payload
                        }
                    } else {
                        evt.payload
                    };

                let json = serde_json::json!({
                    "event": evt.event,
                    "payload": payload,
                });
                Some(Ok::<_, std::convert::Infallible>(
                    Event::default().data(json.to_string()),
                ))
            }
            Err(e) => {
                log::warn!(
                    "SSE stream lagged for user={}: {:?}",
                    sse_username.as_deref().unwrap_or("admin"),
                    e,
                );
                None
            }
        }
    });

    Sse::new(tokio_stream::iter(initial_events).chain(stream))
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("ping"),
        )
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
    // Cancel any in-progress generation before the watchdog fires and exits.
    // Best-effort: ignore errors (ComfyUI may not be running).
    let _ = state.app.gpu_manager.interrupt(None).await;
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

    // Parse optional ?size= and ?token= query params
    let query = req.uri().query().unwrap_or("");
    let max_size: u32 = query
        .split('&')
        .find_map(|p| p.strip_prefix("size="))
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);

    // Try auth from headers first, then from ?token= query param (for <img> tags)
    let username = {
        let from_headers = resolve_username(&state, &headers, &remote);
        if from_headers.is_some() {
            from_headers
        } else if !is_localhost(&remote) && state.lan_enabled {
            query
                .split('&')
                .find_map(|p| p.strip_prefix("token="))
                .and_then(|t| state.auth.validate_token(t))
        } else {
            None
        }
    };
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

/// Serve a full-resolution gallery image (original PNG/JPEG with metadata intact).
async fn gallery_image_handler(
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

    let query = req.uri().query().unwrap_or("");

    // Auth: try headers first, then ?token= query param
    let username = {
        let from_headers = resolve_username(&state, &headers, &remote);
        if from_headers.is_some() {
            from_headers
        } else if !is_localhost(&remote) && state.lan_enabled {
            query
                .split('&')
                .find_map(|p| p.strip_prefix("token="))
                .and_then(|t| state.auth.validate_token(t))
        } else {
            None
        }
    };
    let gallery_dir = match user_gallery_dir(username.as_deref()) {
        Some(d) => d,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "No gallery dir").into_response();
        }
    };

    let file_path = gallery_dir.join(&filename);
    match tokio::fs::read(&file_path).await {
        Ok(data) => {
            let lower = filename.to_ascii_lowercase();
            // JXL: decode and transcode to lossless WebP so WebView2 / Chromium
            // (which don't ship with a JXL decoder) can still render the image.
            // The canonical `.jxl` file on disk is untouched.
            if lower.ends_with(".jxl") {
                // Suggest a `.webp` filename when the browser saves the image
                // (right-click → "Save Image As"). Without this, Edge silently
                // saves the file with the URL's `.jxl` extension even though
                // the bytes are WebP.
                let webp_filename = {
                    let stem = filename
                        .rsplit_once('.')
                        .map(|(s, _)| s)
                        .unwrap_or(&filename);
                    format!("{}.webp", stem)
                };
                let transcode = tokio::task::spawn_blocking(move || {
                    commands::api::transcode_jxl_to_webp(&data)
                })
                .await;
                return match transcode {
                    Ok(Ok(webp)) => (
                        StatusCode::OK,
                        [
                            ("content-type", "image/webp".to_string()),
                            ("cache-control", "no-cache".to_string()),
                            (
                                "content-disposition",
                                format!("inline; filename=\"{}\"", webp_filename),
                            ),
                        ],
                        webp,
                    )
                        .into_response(),
                    Ok(Err(e)) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("JXL transcode failed: {}", e),
                    )
                        .into_response(),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("JXL transcode task panicked: {}", e),
                    )
                        .into_response(),
                };
            }

            let content_type = if lower.ends_with(".webp") {
                "image/webp"
            } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
                "image/jpeg"
            } else {
                "image/png"
            };
            (
                StatusCode::OK,
                [
                    ("content-type", content_type.to_string()),
                    ("cache-control", "no-cache".to_string()),
                ],
                data,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Image not found").into_response(),
    }
}

/// Serve an ephemeral temp image (written by the WS handler for SSE delivery).
/// After serving, the temp file is deleted to free space.
async fn temp_image_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Path(filename): Path<String>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Response {
    // Auth check (same as thumbnail handler)
    let query = req.uri().query().unwrap_or("");
    let username = {
        let from_headers = resolve_username(&state, &headers, &remote);
        if from_headers.is_some() {
            from_headers
        } else if !is_localhost(&remote) && state.lan_enabled {
            query
                .split('&')
                .find_map(|p| p.strip_prefix("token="))
                .and_then(|t| state.auth.validate_token(t))
        } else {
            None
        }
    };
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous && username.is_none() && !is_localhost(&remote) {
        return unauthorized_response("Authentication required");
    }

    match crate::temp_images::load(&filename) {
        Some(data) => {
            let lower = filename.to_ascii_lowercase();
            let want_webp = query.split('&').any(|p| p == "format=webp");
            let want_raw = query.split('&').any(|p| p == "raw=true");

            // JXL has no native browser support on WebView2 / Chromium. Transcode
            // on request (or unconditionally for JXL in all current browsers).
            // Skip transcoding when ?raw=true is requested (for gallery save).
            if !want_raw && (lower.ends_with(".jxl") || want_webp) {
                let needs_transcode =
                    lower.ends_with(".jxl") || (want_webp && !lower.ends_with(".webp"));
                if needs_transcode {
                    let transcode = tokio::task::spawn_blocking(move || {
                        commands::api::transcode_jxl_to_webp(&data)
                    })
                    .await;
                    return match transcode {
                        Ok(Ok(webp)) => (
                            StatusCode::OK,
                            [
                                ("content-type", "image/webp".to_string()),
                                ("cache-control", "no-store".to_string()),
                            ],
                            webp,
                        )
                            .into_response(),
                        Ok(Err(e)) => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("JXL transcode failed: {}", e),
                        )
                            .into_response(),
                        Err(e) => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("JXL transcode task panicked: {}", e),
                        )
                            .into_response(),
                    };
                }
            }

            let content_type = if lower.ends_with(".png") {
                "image/png"
            } else if lower.ends_with(".webp") {
                "image/webp"
            } else if lower.ends_with(".jxl") {
                "image/jxl"
            } else {
                "image/jpeg"
            };
            // Don't delete immediately — the image may be needed later for
            // save_to_gallery_temp.  Periodic cleanup handles expiry.
            (
                StatusCode::OK,
                [
                    ("content-type", content_type.to_string()),
                    ("cache-control", "no-store".to_string()),
                ],
                data,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Temp image not found").into_response(),
    }
}

/// Embed metadata into an existing temp image and return a new temp filename.
/// Avoids the slow JSON number-array round-trip: the image bytes stay on the
/// server side; only the compact metadata JSON crosses the wire.
async fn embed_temp_metadata_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return unauthorized_response("Authentication required");
    }

    let args: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response(),
    };

    let temp_filename = match args["tempFilename"].as_str() {
        Some(f) => f,
        None => return (StatusCode::BAD_REQUEST, "Missing tempFilename").into_response(),
    };

    let metadata: std::collections::HashMap<String, String> =
        match serde_json::from_value(args["metadata"].clone()) {
            Ok(m) => m,
            Err(_) => return (StatusCode::BAD_REQUEST, "Invalid metadata").into_response(),
        };

    let metadata_mode = args["metadataMode"].as_str().unwrap_or("stealth");

    let bytes = match crate::temp_images::load(temp_filename) {
        Some(b) => b,
        None => return (StatusCode::NOT_FOUND, "Temp image not found").into_response(),
    };

    let embed_mode = crate::metadata::MetadataMode::from_str(metadata_mode);
    let detected_format = crate::metadata::detect_format(&bytes);

    let (embedded, out_ext) = match detected_format {
        crate::metadata::ImageFormat::Jxl => {
            match crate::metadata::embed_jxl_metadata(&bytes, &metadata) {
                Ok(b) => (b, "jxl"),
                Err(e) => {
                    log::warn!("embed_temp_metadata (JXL) failed: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
                }
            }
        }
        _ => match crate::metadata::embed_png_metadata(&bytes, &metadata, embed_mode) {
            Ok(b) => (b, "png"),
            Err(e) => {
                log::warn!("embed_temp_metadata (PNG) failed: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
            }
        },
    };

    match crate::temp_images::save(&embedded, out_ext) {
        Some(new_filename) => {
            let json = serde_json::json!({ "tempFilename": new_filename });
            axum::Json(json).into_response()
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save embedded image",
        )
            .into_response(),
    }
}

/// GPU stats handler — returns nvidia-smi data merged with worker statuses.
async fn gpu_stats_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return unauthorized_response("Authentication required");
    }

    match crate::commands::api::get_gpu_stats_inner(&state.app).await {
        Ok(stats) => axum::Json(stats).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get GPU stats: {}", e),
        )
            .into_response(),
    }
}

/// CDN proxy handler — fetches assets from cdn.mooshieblob.com and forwards
/// them to the browser with CORS headers so in-browser mode works correctly.
/// Only proxies from the hardcoded CDN origin; this is NOT an open proxy.
async fn cdn_proxy_handler(
    AxumState(state): AxumState<SharedState>,
    Path(path): Path<String>,
) -> Response {
    let target_url = format!("https://cdn.mooshieblob.com/{}", path);
    match state.app.http_client.get(&target_url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let content_type = resp
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .cloned();
            let body = match resp.bytes().await {
                Ok(b) => b,
                Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
            };
            let mut response = (status, body).into_response();
            response.headers_mut().insert(
                axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                "*".parse().unwrap(),
            );
            if let Some(ct) = content_type {
                response
                    .headers_mut()
                    .insert(axum::http::header::CONTENT_TYPE, ct);
            }
            response
        }
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
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

    // Model Hub commands require explicit access for regular users
    if is_modelhub_command(&command) && role == UserRole::User {
        let has_access = extract_token(&headers)
            .and_then(|t| state.auth.validate_token(&t))
            .and_then(|u| state.auth.get_modelhub_access(&u))
            .unwrap_or(false);
        if !has_access {
            return forbidden_response("You do not have access to the Model Hub. Ask an admin or moderator to enable it for your account.");
        }
    }

    // Resolve username for per-user gallery isolation
    let username = resolve_username(&state, &headers, &remote);

    // Track last-activity for online/offline status
    if let Some(ref u) = username {
        state.auth.touch_activity(u);
    }

    match dispatch_command(
        state.app.clone(),
        &state.auth,
        &command,
        &args,
        username.as_deref(),
    )
    .await
    {
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
    auth: &Arc<AuthState>,
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
            config::save_config(&new_config)?;
            let mut current = state.config.write().await;
            *current = new_config;
            Ok(serde_json::json!(null))
        }
        "check_attention_backend" => {
            let (venv_path, current) = {
                let config = state.config.read().await;
                (config.venv_path.clone(), config.attention_backend.clone())
            };

            let uv = crate::commands::api::resolve_uv_bin_pub(&venv_path);
            let mut venv_packages = Vec::new();
            let venv_python = {
                #[cfg(target_os = "windows")]
                {
                    std::path::Path::new(&venv_path)
                        .join("Scripts")
                        .join("python.exe")
                }
                #[cfg(not(target_os = "windows"))]
                {
                    std::path::Path::new(&venv_path).join("bin").join("python")
                }
            };

            if uv.exists() {
                if let Ok(output) = tokio::process::Command::new(&uv)
                    .args(["pip", "list", "--python", &venv_python.to_string_lossy()])
                    .output()
                    .await
                {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let known = ["sageattention", "flash-attn", "triton"];
                        for line in stdout.lines() {
                            let pkg = line.split_whitespace().next().unwrap_or("").to_lowercase();
                            if known.iter().any(|k| pkg == *k) {
                                venv_packages.push(pkg);
                            }
                        }
                    }
                }
            }

            Ok(serde_json::json!({
                "current": current,
                "venv_packages": venv_packages,
                "compute_capability": crate::commands::api::detect_compute_capability_pub(),
            }))
        }
        "switch_to_app_mode" => {
            #[cfg(not(feature = "desktop"))]
            {
                return Err("switch_to_app_mode is only available in desktop mode".into());
            }
            #[cfg(feature = "desktop")]
            {
                // Step 1: Save config
                let mut cfg = state.config.write().await;
                cfg.browser_mode = false;
                config::save_config(&cfg)?;
                drop(cfg);

                // Step 2: Disarm heartbeat watchdog
                state
                    .app_mode_active
                    .store(true, std::sync::atomic::Ordering::SeqCst);

                // Step 3: Show the existing hidden Tauri window.
                let handle_guard = state.app_handle.lock().await;
                if let Some(ref app_handle) = *handle_guard {
                    use tauri::Manager;
                    if let Some(win) = app_handle.get_webview_window("main") {
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
        "kill_port_process" => {
            let port = state.config.read().await.server_port;
            crate::comfyui::process::kill_process_on_port(port).await;
            Ok(serde_json::json!(port))
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
            // Aggregate queues from ALL GPU workers so the frontend reconciler
            // can see prompts regardless of which worker is executing them.
            let mut running: Vec<serde_json::Value> = Vec::new();
            let mut pending: Vec<serde_json::Value> = Vec::new();
            for worker in &state.gpu_manager.workers {
                let url = format!("{}/queue", worker.base_url);
                if let Ok(resp) = state.http_client.get(&url).send().await {
                    if let Ok(val) = resp.json::<serde_json::Value>().await {
                        if let Some(arr) = val.get("queue_running").and_then(|v| v.as_array()) {
                            running.extend(arr.iter().cloned());
                        }
                        if let Some(arr) = val.get("queue_pending").and_then(|v| v.as_array()) {
                            pending.extend(arr.iter().cloned());
                        }
                    }
                }
            }
            // Resolve aliases: replace real ComfyUI prompt IDs with the
            // placeholder gen-* IDs the frontend knows about.
            let resolve = |entries: &mut Vec<serde_json::Value>| {
                for entry in entries.iter_mut() {
                    // ComfyUI queue entries are arrays: [index, prompt_id, ...]
                    if let Some(arr) = entry.as_array_mut() {
                        if let Some(pid) = arr.get(1).and_then(|v| v.as_str()) {
                            let resolved = state.prompt_queue.resolve_alias(pid);
                            if resolved != pid {
                                arr[1] = serde_json::Value::String(resolved);
                            }
                        }
                    }
                }
            };
            resolve(&mut running);
            resolve(&mut pending);
            // Include tracked placeholders that haven't been submitted to a
            // ComfyUI worker yet (background submission in flight, or held in
            // the fair queue). Without this, the frontend reconciler falsely
            // concludes these prompts have vanished and clears them
            // immediately after the user clicks generate.
            let known: std::collections::HashSet<String> = running
                .iter()
                .chain(pending.iter())
                .filter_map(|e| {
                    e.as_array()
                        .and_then(|a| a.get(1))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .collect();
            let tracked: Vec<String> = {
                let q = state.prompt_queue.queue.read().unwrap();
                q.iter().map(|(pid, _)| pid.clone()).collect()
            };
            for pid in tracked {
                if !known.contains(&pid) {
                    pending.push(serde_json::json!([0, pid, {}, {}, []]));
                }
            }

            // Check if caller is privileged (admin or moderator) — can see usernames.
            // username=None means localhost (always admin).
            let is_privileged = match username {
                None => true,
                Some(u) => matches!(
                    auth.get_account_role(u).as_deref(),
                    Some("admin") | Some("moderator")
                ),
            };
            // Build ordered queue positions from our internal fair-queue tracker.
            // This is separate from ComfyUI's queue and reflects round-robin ordering.
            let queue_positions: Vec<serde_json::Value> = {
                let queue = state.prompt_queue.queue.read().unwrap();
                queue
                    .iter()
                    .enumerate()
                    .map(|(pos, (id, owner))| {
                        if is_privileged {
                            serde_json::json!({
                                "prompt_id": id,
                                "position": pos,
                                "username": owner,
                            })
                        } else {
                            serde_json::json!({
                                "prompt_id": id,
                                "position": pos,
                            })
                        }
                    })
                    .collect()
            };
            Ok(serde_json::json!({
                "queue_running": running,
                "queue_pending": pending,
                "queue_positions": queue_positions,
            }))
        }
        "get_history" => {
            let prompt_id = args["promptId"].as_str().ok_or("Missing promptId")?;
            let result = state
                .get_history_for(prompt_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(result)
        }
        "recover_prompt_outputs" => {
            // Return cached output temp filenames for a prompt whose SSE events
            // were dropped (e.g. during a client reconnect).  The cleanup reactor
            // populates output_image_cache whenever it sees a comfyui:output_image
            // broadcast — so even if the SSE client missed the event, the image
            // temp file was already saved.
            let placeholder_id = args["promptId"].as_str().ok_or("Missing promptId")?;
            let cached = state
                .output_image_cache
                .write()
                .unwrap()
                .remove(placeholder_id)
                .unwrap_or_default();
            let images: Vec<serde_json::Value> = cached
                .into_iter()
                .map(|f| serde_json::json!({ "temp_filename": f }))
                .collect();
            Ok(serde_json::json!({ "images": images }))
        }
        "interrupt_generation" => {
            if let Some(prompt_id) = args["promptId"].as_str() {
                let caller = username.map(str::to_string);
                if !state.prompt_queue.is_owned_by(prompt_id, &caller) {
                    return Err("Prompt does not belong to the current user".to_string());
                }
                state
                    .interrupt_prompt(Some(prompt_id))
                    .await
                    .map_err(|e| e.to_string())?;
            } else {
                state
                    .interrupt_user_prompts(username)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Ok(serde_json::json!(null))
        }
        "clear_all_queues" => {
            // 1. Drain held prompts and cancel them so their background tasks exit cleanly
            let held_prompts: Vec<crate::state::HeldPrompt> = {
                let mut held = state.prompt_queue.held.lock().unwrap();
                held.drain(..).collect()
            };
            for hp in held_prompts {
                let mut result = hp.result.lock().await;
                *result = Some(Err("Queue cleared by admin".to_string()));
                hp.submitted.notify_one();
            }

            // 2. Interrupt all currently running workers
            let _ = state.gpu_manager.interrupt(None).await;

            // 3. Delete all pending items from each ComfyUI worker queue
            for worker in &state.gpu_manager.workers {
                let queue_url = format!("{}/queue", worker.base_url);
                if let Ok(resp) = state.http_client.get(&queue_url).send().await {
                    if let Ok(val) = resp.json::<serde_json::Value>().await {
                        let mut pending_ids: Vec<String> = Vec::new();
                        if let Some(arr) = val.get("queue_pending").and_then(|v| v.as_array()) {
                            for item in arr {
                                if let Some(pid) = item
                                    .as_array()
                                    .and_then(|a| a.get(1))
                                    .and_then(|v| v.as_str())
                                {
                                    pending_ids.push(pid.to_string());
                                }
                            }
                        }
                        if !pending_ids.is_empty() {
                            let _ = state
                                .http_client
                                .post(format!("{}/queue", worker.base_url))
                                .json(&serde_json::json!({ "delete": pending_ids }))
                                .send()
                                .await;
                        }
                    }
                }
            }

            // 4. Clear the internal queue tracking
            state.prompt_queue.clear_all();

            // 5. Wake the drain reactor so it sees the empty held list
            state.prompt_queue.drain_notify.notify_one();

            // 6. Broadcast empty queue state and a clear event to all clients
            state.broadcast_queue_positions();
            state.broadcast("mooshie:queue_cleared", serde_json::json!({}));

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
        "load_gallery_image_display" => {
            // JXL → WebP transcode so non-JXL browsers (Firefox, Edge, Chrome)
            // can render the image. Other formats are returned as-is.
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
            let out = if filename.to_ascii_lowercase().ends_with(".jxl") {
                tokio::task::spawn_blocking(move || commands::api::transcode_jxl_to_webp(&bytes))
                    .await
                    .map_err(|e| format!("Task panicked: {}", e))?
                    .map_err(|e| e.to_string())?
            } else {
                bytes
            };
            Ok(serde_json::json!(out))
        }
        "load_gallery_image_png" => {
            // JXL → PNG transcode for downloading / clipboard. PNG keeps
            // metadata intact and is supported everywhere.
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
            let out = if filename.to_ascii_lowercase().ends_with(".jxl") {
                tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
                    let img = commands::api::decode_gallery_image(&bytes)?;
                    let mut buf = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut buf, image::ImageFormat::Png)
                        .map_err(|e| format!("PNG encode failed: {}", e))?;
                    Ok(buf.into_inner())
                })
                .await
                .map_err(|e| format!("Task panicked: {}", e))?
                .map_err(|e| e.to_string())?
            } else {
                bytes
            };
            Ok(serde_json::json!(out))
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
            let user = username.map(|s| s.to_string());

            log::info!(
                "[gen] user={} seed={} steps={}",
                user.as_deref().unwrap_or("admin"),
                seed,
                params.steps,
            );

            // Check needs_hold BEFORE inserting the placeholder
            let needs_hold = user.is_some() && state.prompt_queue.active_count_for_user(&user) > 0;

            // Generate a placeholder prompt_id and insert it immediately.
            // This allows us to return to the client right away (avoids Cloudflare 524 timeouts).
            let placeholder_id = format!("gen-{}", uuid::Uuid::new_v4());
            state.prompt_queue.insert(&placeholder_id, user.clone());
            state.broadcast_queue_positions();

            let queue_pos = state.prompt_queue.len().saturating_sub(1);
            let queue_total = state.prompt_queue.len();

            // Spawn background task to do the actual ComfyUI submission.
            let bg_state = Arc::clone(&state);
            let bg_placeholder = placeholder_id.clone();
            tokio::spawn(async move {
                if needs_hold {
                    // Fair queue: hold this prompt until a slot opens for this user.
                    let submitted = Arc::new(tokio::sync::Notify::new());
                    let result_slot: crate::state::HeldPromptResult =
                        Arc::new(tokio::sync::Mutex::new(None));

                    let held = crate::state::HeldPrompt {
                        workflow,
                        username: user.clone(),
                        placeholder_id: bg_placeholder.clone(),
                        submitted: submitted.clone(),
                        result: result_slot.clone(),
                    };

                    {
                        let mut held_queue = bg_state.prompt_queue.held.lock().unwrap();
                        held_queue.push(held);
                    }
                    bg_state.broadcast_queue_positions();

                    // Wait until the drain reactor submits this prompt
                    submitted.notified().await;

                    // Retrieve the result — alias is already bound by the drain reactor
                    let res = result_slot
                        .lock()
                        .await
                        .take()
                        .unwrap_or_else(|| Err("Held prompt was never submitted".into()));

                    match res {
                        Ok(_) => {
                            bg_state.broadcast_queue_positions();
                        }
                        Err(e) => {
                            log::error!(
                                "[gen] held submission failed for {}: {}",
                                bg_placeholder,
                                e
                            );
                            bg_state.prompt_queue.finish(&bg_placeholder);
                            bg_state.prompt_queue.cleanup_alias(&bg_placeholder);
                            bg_state.broadcast_queue_positions();
                            let _ = bg_state.event_tx.send(crate::state::BroadcastEvent {
                                event: "comfyui:execution_error".to_string(),
                                payload: serde_json::json!({
                                    "prompt_id": bg_placeholder,
                                    "error": e,
                                }),
                            });
                        }
                    }
                } else {
                    // Direct submission (admin or user's first prompt)
                    let timeout = std::time::Duration::from_secs(300);
                    match bg_state
                        .gpu_manager
                        .submit_prompt(workflow, &bg_state.client_id, timeout)
                        .await
                    {
                        Ok((worker_id, response)) => {
                            let was_deferred = bg_state
                                .prompt_queue
                                .bind_alias(&bg_placeholder, &response.prompt_id);
                            if was_deferred {
                                // Completion/error arrived in the window before bind_alias.
                                // Placeholder is already removed from the queue; release worker.
                                log::warn!(
                                    "[gen] deferred cleanup on bind: placeholder={}",
                                    &bg_placeholder[..8.min(bg_placeholder.len())],
                                );
                                bg_state
                                    .gpu_manager
                                    .mark_worker_error_then_idle(worker_id)
                                    .await;
                                bg_state
                                    .output_image_cache
                                    .write()
                                    .unwrap()
                                    .remove(&bg_placeholder);
                                let alias_state = bg_state.clone();
                                let alias_pid = bg_placeholder.clone();
                                tokio::spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                    alias_state.prompt_queue.cleanup_alias(&alias_pid);
                                });
                                bg_state.broadcast_queue_positions();
                                bg_state.prompt_queue.drain_notify.notify_one();
                            } else {
                                bg_state.prompt_queue.set_worker(&bg_placeholder, worker_id);
                                bg_state.broadcast_queue_positions();
                            }
                        }
                        Err(e) => {
                            log::error!("[gen] submission failed for {}: {}", bg_placeholder, e);
                            bg_state.prompt_queue.finish(&bg_placeholder);
                            bg_state.broadcast_queue_positions();
                            let _ = bg_state.event_tx.send(crate::state::BroadcastEvent {
                                event: "comfyui:execution_error".to_string(),
                                payload: serde_json::json!({
                                    "prompt_id": bg_placeholder,
                                    "error": e.to_string(),
                                }),
                            });
                        }
                    }
                }
            });

            // Return immediately — the frontend tracks progress via SSE/WebSocket events
            Ok(serde_json::json!({
                "prompt_id": placeholder_id,
                "seed": seed,
                "queue_position": queue_pos,
                "queue_total": queue_total,
            }))
        }
        "generate_controlnet_preprocessor_preview" => {
            crate::temp_images::cleanup(300);

            let image = args["image"]
                .as_str()
                .ok_or("Missing image")?
                .trim()
                .to_string();
            let preprocessor = args["preprocessor"]
                .as_str()
                .ok_or("Missing preprocessor")?
                .trim()
                .to_string();

            if image.is_empty() {
                return Err("ControlNet preprocessor preview needs a control image.".into());
            }
            if preprocessor.is_empty() {
                return Err("ControlNet preprocessor preview needs a preprocessor.".into());
            }

            let workflow = crate::templates::controlnet::build_preprocessor_preview_workflow(
                &image,
                &preprocessor,
            );
            let timeout = std::time::Duration::from_secs(120);
            let (worker_id, response) = state
                .gpu_manager
                .submit_prompt(workflow, &state.client_id, timeout)
                .await
                .map_err(|e| e.to_string())?;

            state
                .prompt_queue
                .insert(&response.prompt_id, username.map(|s| s.to_string()));
            state
                .prompt_queue
                .set_worker(&response.prompt_id, worker_id);
            state.broadcast_queue_positions();

            Ok(serde_json::json!({
                "prompt_id": response.prompt_id,
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
            // Enforce storage limit for non-admin users
            if let Some(name) = username {
                let limit = auth.get_storage_limit(name);
                if limit > 0 {
                    let usage = dir_usage_bytes(&dir);
                    if usage + bytes.len() as u64 > limit {
                        return Err(format!(
                            "Storage limit exceeded ({:.1} MB / {:.1} MB). Download your images and free space, or ask an admin to increase your limit.",
                            usage as f64 / 1_048_576.0,
                            limit as f64 / 1_048_576.0,
                        ));
                    }
                }
            }
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
            // Enforce storage limit for non-admin users
            if let Some(name) = username {
                let limit = auth.get_storage_limit(name);
                if limit > 0 {
                    let usage = dir_usage_bytes(&dir);
                    if usage + image_bytes.len() as u64 > limit {
                        return Err(format!(
                            "Storage limit exceeded ({:.1} MB / {:.1} MB). Download your images and free space, or ask an admin to increase your limit.",
                            usage as f64 / 1_048_576.0,
                            limit as f64 / 1_048_576.0,
                        ));
                    }
                }
            }
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
        "save_to_gallery_temp" => {
            // Save from a temp image file (browser mode: image was already received
            // via WebSocket and stored as a temp file on the server).
            let temp_filename = args["tempFilename"]
                .as_str()
                .ok_or("Missing tempFilename")?
                .to_string();
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
            let bytes = crate::temp_images::load(&temp_filename)
                .ok_or_else(|| format!("Temp image '{}' not found or expired", temp_filename))?;
            let dir = user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
            // Enforce storage limit for non-admin users
            if let Some(name) = username {
                let limit = auth.get_storage_limit(name);
                if limit > 0 {
                    let usage = dir_usage_bytes(&dir);
                    if usage + bytes.len() as u64 > limit {
                        return Err(format!(
                            "Storage limit exceeded ({:.1} MB / {:.1} MB). Download your images and free space, or ask an admin to increase your limit.",
                            usage as f64 / 1_048_576.0,
                            limit as f64 / 1_048_576.0,
                        ));
                    }
                }
            }
            let result = save_to_gallery_in_dir(
                &dir,
                &bytes,
                &filename,
                &prompt_id,
                mode.as_deref(),
                metadata.as_ref(),
                metadata_mode.as_deref(),
            )?;
            // Clean up temp file after successful save
            crate::temp_images::remove(&temp_filename);
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
            let result =
                crate::metadata::read_png_metadata(&image_bytes).map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "read_image_metadata_path" => {
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let result = crate::metadata::read_png_metadata(&bytes).map_err(|e| e.to_string())?;
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
            let mode = crate::metadata::MetadataMode::from_str(
                metadata_mode.as_deref().unwrap_or("text_chunk"),
            );
            let result = crate::metadata::embed_png_metadata(&image_bytes, &metadata, mode)
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
                    let venv_base = std::path::Path::new(&venv_path);
                    #[cfg(target_os = "windows")]
                    let pip_path = venv_base.join("Scripts").join("pip.exe");
                    #[cfg(not(target_os = "windows"))]
                    let pip_path = venv_base.join("bin").join("pip");
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
                let venv_base = std::path::Path::new(&venv_path);
                #[cfg(target_os = "windows")]
                let pip_path = venv_base.join("Scripts").join("pip.exe");
                #[cfg(not(target_os = "windows"))]
                let pip_path = venv_base.join("bin").join("pip");
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

        // --- Model info (server has filesystem access to models) ---
        "get_model_install_dirs" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let config = state.config.read().await;
            let comfyui_path = config.comfyui_path.clone();
            let extra_model_paths = config.extra_model_paths.clone();
            drop(config);

            let mut dirs: Vec<serde_json::Value> = Vec::new();
            if !comfyui_path.is_empty() {
                let primary = std::path::Path::new(&comfyui_path)
                    .join("models")
                    .join(&category);
                let label = std::path::Path::new(&comfyui_path)
                    .file_name()
                    .map(|n| format!("App ({})", n.to_string_lossy()))
                    .unwrap_or_else(|| "App".to_string());
                dirs.push(serde_json::json!({ "path": primary.to_string_lossy(), "label": label }));
            }
            if let Some(extra) = extra_model_paths {
                for line in extra.lines().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    let extra_dir = std::path::Path::new(line).join(&category);
                    if extra_dir.exists() {
                        let label = std::path::Path::new(line)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| line.to_string());
                        dirs.push(serde_json::json!({ "path": extra_dir.to_string_lossy(), "label": label }));
                    }
                }
            }
            serde_json::to_value(dirs).map_err(|e| e.to_string())
        }
        "find_model_by_hash" => {
            let hash = args["hash"].as_str().ok_or("Missing hash")?.to_string();
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let config = state.config.read().await;
            if config.comfyui_path.is_empty() {
                return Err("ComfyUI path not configured".into());
            }
            let models_dir = std::path::Path::new(&config.comfyui_path)
                .join("models")
                .join(&category);
            drop(config);

            if !models_dir.exists() {
                return Ok(serde_json::json!(null));
            }
            let needle = hash.to_uppercase();
            let is_autov2 = needle.len() == 10;
            let result = tokio::task::spawn_blocking(move || {
                let entries = std::fs::read_dir(&models_dir).map_err(|e| e.to_string())?;
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if !(name.ends_with(".safetensors") || name.ends_with(".ckpt")) {
                        continue;
                    }
                    if let Ok(h) = crate::commands::api::full_sha256(&path) {
                        let matches = if is_autov2 {
                            crate::commands::api::autov2_hash(&h) == needle
                        } else {
                            h == needle
                        };
                        if matches {
                            return Ok(Some(name));
                        }
                    }
                }
                Ok::<_, String>(None)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e: String| e)?;
            Ok(serde_json::json!(result))
        }
        "hash_model_file" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let config = state.config.read().await;
            if config.comfyui_path.is_empty() {
                return Err("ComfyUI path not configured".into());
            }
            let path = std::path::Path::new(&config.comfyui_path)
                .join("models")
                .join(&category)
                .join(&filename);
            drop(config);

            if !path.exists() {
                return Err(format!("File not found: {}", filename));
            }
            let result = tokio::task::spawn_blocking(move || {
                let sha256 = crate::commands::api::full_sha256(&path).map_err(|e| e.to_string())?;
                let autov2 = crate::commands::api::autov2_hash(&sha256);
                Ok::<_, String>(serde_json::json!({ "sha256": sha256, "autov2": autov2 }))
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e: String| e)?;
            Ok(result)
        }
        "read_modelspec" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let config = state.config.read().await;
            if config.comfyui_path.is_empty() {
                return Err("ComfyUI path not configured".into());
            }
            let path = std::path::Path::new(&config.comfyui_path)
                .join("models")
                .join(&category)
                .join(&filename);
            drop(config);

            if !path.exists() {
                return Err(format!("File not found: {}", filename));
            }
            if !filename.ends_with(".safetensors") {
                return Ok(serde_json::json!(null));
            }
            let result = tokio::task::spawn_blocking(move || {
                crate::commands::api::read_safetensors_modelspec(&path).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e: String| e)?;
            serde_json::to_value(result).map_err(|e| e.to_string())
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
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let (comfyui_path, extra_model_paths, civitai_api_key) = {
                let config = state.config.read().await;
                if config.comfyui_path.is_empty() {
                    return Err("ComfyUI path not configured".into());
                }
                (
                    config.comfyui_path.clone(),
                    config.extra_model_paths.clone(),
                    config.civitai_api_key.clone(),
                )
            };
            let category = if command == "get_lora_civitai_info" {
                "loras"
            } else {
                "checkpoints"
            };
            let path = crate::commands::api::resolve_model_path(
                &comfyui_path,
                extra_model_paths.as_deref(),
                category,
                &filename,
            )
            .ok_or_else(|| format!("Model file not found: {}", filename))?;

            // Read modelspec (safetensors only)
            let modelspec = if filename.ends_with(".safetensors") {
                let p = path.clone();
                tokio::task::spawn_blocking(move || {
                    crate::commands::api::read_safetensors_modelspec(&p)
                        .ok()
                        .flatten()
                })
                .await
                .unwrap_or(None)
            } else {
                None
            };

            // Hash in blocking task
            let path_clone = path.clone();
            let sha256 =
                tokio::task::spawn_blocking(move || crate::commands::api::full_sha256(&path_clone))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
            let autov2 = crate::commands::api::autov2_hash(&sha256);

            // CivitAI lookup by hash
            let civitai_url = format!(
                "https://civitai.com/api/v1/model-versions/by-hash/{}",
                autov2
            );
            let mut civitai_req = state
                .http_client
                .get(&civitai_url)
                .header("User-Agent", "MooshieUI/0.7");
            if let Some(key) = civitai_api_key.filter(|v| !v.trim().is_empty()) {
                civitai_req = civitai_req.bearer_auth(key);
            }
            let civitai_data = match civitai_req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    resp.json::<serde_json::Value>().await.ok()
                }
                _ => None,
            };

            // Build result
            let mut result = serde_json::json!({
                "filename": filename,
                "hash": autov2,
                "modelspec_title": modelspec.as_ref().and_then(|m| m.get("title")),
                "modelspec_author": modelspec.as_ref().and_then(|m| m.get("author")),
                "modelspec_architecture": modelspec.as_ref().and_then(|m| m.get("architecture")),
                "modelspec_description": modelspec.as_ref().and_then(|m| m.get("description")),
                "modelspec_tags": modelspec.as_ref().and_then(|m| m.get("tags")),
            });

            if command == "get_lora_civitai_info" {
                result["modelspec_trigger_phrase"] =
                    serde_json::json!(modelspec.as_ref().and_then(|m| m.get("trigger_phrase")));
            }

            // Sidecar thumbnail for checkpoints
            if command == "get_checkpoint_civitai_info" {
                if let (Some(model_dir), Some(stem)) =
                    (path.parent(), path.file_stem().and_then(|s| s.to_str()))
                {
                    let candidates = [
                        model_dir.join(format!("{}.png", stem)),
                        model_dir.join(format!("{}.jpg", stem)),
                        model_dir.join(format!("{}.jpeg", stem)),
                        model_dir.join(format!("{}.preview.png", stem)),
                        model_dir.join(format!("{}.preview.jpg", stem)),
                    ];
                    for candidate in &candidates {
                        if candidate.exists() {
                            if let Ok(bytes) = std::fs::read(candidate) {
                                use base64::Engine as _;
                                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                let mime = match candidate
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("")
                                {
                                    "jpg" | "jpeg" => "image/jpeg",
                                    _ => "image/png",
                                };
                                result["thumbnail_url"] =
                                    serde_json::json!(format!("data:{};base64,{}", mime, b64));
                            }
                            break;
                        }
                    }
                }
                result["display_name"] =
                    serde_json::json!(modelspec.as_ref().and_then(|m| m.get("title")));
                result["base_model"] =
                    serde_json::json!(modelspec.as_ref().and_then(|m| m.get("architecture")));
            }

            // Merge CivitAI data
            if let Some(data) = civitai_data {
                result["civitai_version_id"] =
                    serde_json::json!(data.get("id").and_then(|v| v.as_u64()));
                result["civitai_model_id"] =
                    serde_json::json!(data.get("modelId").and_then(|v| v.as_u64()));
                if let Some(bm) = data.get("baseModel").and_then(|v| v.as_str()) {
                    if command == "get_checkpoint_civitai_info" {
                        result["base_model"] = serde_json::json!(bm);
                    }
                    result["civitai_base_model"] = serde_json::json!(bm);
                }
                if let Some(model) = data.get("model") {
                    if command == "get_checkpoint_civitai_info"
                        && result
                            .get("display_name")
                            .and_then(|v| v.as_str())
                            .is_none()
                    {
                        result["display_name"] =
                            serde_json::json!(model.get("name").and_then(|v| v.as_str()));
                    }
                    result["civitai_name"] =
                        serde_json::json!(model.get("name").and_then(|v| v.as_str()));
                    result["civitai_description"] =
                        serde_json::json!(model.get("description").and_then(|v| v.as_str()));
                    result["civitai_creator"] = serde_json::json!(model
                        .get("creator")
                        .and_then(|c| c.get("username"))
                        .or_else(|| model.get("user").and_then(|u| u.get("username")))
                        .and_then(|v| v.as_str()));
                }
                if let Some(stats) = data.get("stats") {
                    result["civitai_download_count"] =
                        serde_json::json!(stats.get("downloadCount").and_then(|v| v.as_u64()));
                    result["civitai_thumbs_up_count"] =
                        serde_json::json!(stats.get("thumbsUpCount").and_then(|v| v.as_u64()));
                }
                if command == "get_lora_civitai_info" {
                    if let Some(tw) = data.get("trainedWords").and_then(|v| v.as_array()) {
                        result["civitai_trigger_words"] = serde_json::json!(tw
                            .iter()
                            .filter_map(|w| w.as_str())
                            .collect::<Vec<_>>());
                    }
                }
                if let Some(images) = data.get("images").and_then(|v| v.as_array()) {
                    let imgs: Vec<serde_json::Value> = images.iter().filter_map(|img| {
                        img.get("url").and_then(|u| u.as_str()).map(|url| {
                            serde_json::json!({
                                "url": url,
                                "width": img.get("width").and_then(|w| w.as_u64()),
                                "height": img.get("height").and_then(|h| h.as_u64()),
                                "nsfw": img.get("nsfwLevel").and_then(|n| n.as_u64()).map(|n| if n <= 1 { "None" } else { "Level" }),
                            })
                        })
                    }).collect();
                    result["civitai_images"] = serde_json::json!(imgs);
                    if command == "get_checkpoint_civitai_info"
                        && result.get("thumbnail_url").is_none()
                    {
                        result["thumbnail_url"] =
                            serde_json::json!(imgs.first().and_then(|i| i.get("url")));
                    }
                }
            }

            Ok(result)
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
                config::save_config(&cfg)?;
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
                config::save_config(&cfg)?;
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
            // Fold any frontend logs from the payload into the shared ring
            // buffer before exporting so this handler matches the desktop
            // command's behaviour.
            if let Some(lines) = args.get("frontendLogs").and_then(|v| v.as_array()) {
                let strings: Vec<String> = lines
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                crate::log_buffer::push_frontend_lines(strings);
            }
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

        // --- Interrogator (ONNX Runtime + model files) ---
        "interrogate_image" | "interrogate_image_path" | "interrogate_gallery_image" => {
            #[cfg(not(any(feature = "desktop", feature = "server")))]
            {
                return Err("Interrogation is not available in this build".to_string());
            }
            #[cfg(any(feature = "desktop", feature = "server"))]
            {
                let image_bytes: Vec<u8> = match command {
                    "interrogate_image" => {
                        let image_base64 = args["imageBase64"]
                            .as_str()
                            .ok_or("Missing imageBase64")?
                            .to_string();
                        use base64::Engine;
                        base64::engine::general_purpose::STANDARD
                            .decode(&image_base64)
                            .map_err(|e| format!("Invalid base64: {}", e))?
                    }
                    "interrogate_image_path" => {
                        let path = args["path"].as_str().ok_or("Missing path")?.to_string();
                        std::fs::read(&path).map_err(|e| format!("Failed to read image: {}", e))?
                    }
                    "interrogate_gallery_image" => {
                        let filename = args["filename"]
                            .as_str()
                            .ok_or("Missing filename")?
                            .to_string();
                        if filename.contains('/')
                            || filename.contains('\\')
                            || filename.contains("..")
                        {
                            return Err("Invalid filename".to_string());
                        }
                        let dir =
                            crate::config::gallery_dir().ok_or("Cannot find gallery directory")?;
                        let path = dir.join(&filename);
                        std::fs::read(&path).map_err(|e| e.to_string())?
                    }
                    _ => unreachable!(),
                };
                let result = run_interrogation_headless(&state, image_bytes).await?;
                serde_json::to_value(result).map_err(|e| e.to_string())
            }
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
        "save_text_file" => {
            let content = args["content"]
                .as_str()
                .ok_or("Missing content")?
                .to_string();
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            tokio::fs::write(&path, content)
                .await
                .map_err(|e| e.to_string())?;
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
            let path = std::path::Path::new(&file_path);
            if !path.exists() {
                return Err(format!("File not found: {}", file_path));
            }
            let bytes = std::fs::read(path).map_err(|e| format!("Read failed: {}", e))?;
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            let mime = crate::commands::api::infer_image_mime_pub(&bytes, ext.as_deref());
            crate::commands::api::native_clipboard_write_pub(&bytes, mime)
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "copy_bytes_to_clipboard" => {
            let bytes: Vec<u8> = serde_json::from_value(args["bytes"].clone())
                .map_err(|e| format!("Invalid bytes: {}", e))?;
            let ext = args["ext"].as_str().ok_or("Missing ext")?.to_string();
            let mime = crate::commands::api::infer_image_mime_pub(&bytes, Some(&ext));
            crate::commands::api::native_clipboard_write_pub(&bytes, mime)
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "read_clipboard_image" => {
            let bytes =
                crate::commands::api::native_clipboard_read_pub().map_err(|e| e.to_string())?;
            let encoded: Vec<serde_json::Value> =
                bytes.iter().map(|b| serde_json::json!(*b)).collect();
            Ok(serde_json::Value::Array(encoded))
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
#[cfg(any(feature = "desktop", feature = "server"))]
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

/// POST /internal-api/_auth/logout — invalidate the current session token.
async fn auth_logout_handler(
    AxumState(state): AxumState<SharedState>,
    headers: HeaderMap,
) -> Response {
    if let Some(token) = extract_token(&headers) {
        state.auth.logout(&token);
    }
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
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
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can create accounts.");
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
    // Admins/mods always have modelhub access; for users check the account flag
    let can_use_modelhub = match role {
        UserRole::Admin | UserRole::Moderator => true,
        _ => {
            if let Some(token) = extract_token(&headers) {
                state
                    .auth
                    .validate_token(&token)
                    .and_then(|u| state.auth.get_modelhub_access(&u))
                    .unwrap_or(false)
            } else {
                false
            }
        }
    };
    Json(serde_json::json!({
        "auth_required": state.lan_enabled,
        "has_accounts": state.auth.has_accounts(),
        "role": role_str,
        "lan_enabled": state.lan_enabled,
        "can_use_modelhub": can_use_modelhub,
    }))
}

/// GET /internal-api/_auth/accounts — list all accounts with roles and online status. Admin/Moderator.
async fn auth_list_accounts_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can list accounts.");
    }
    let online_threshold = std::time::Duration::from_secs(30);
    let accounts: Vec<serde_json::Value> = state
        .auth
        .list_users_status(online_threshold)
        .into_iter()
        .map(
            |(
                username,
                role,
                online,
                created_at,
                last_online,
                storage_limit_bytes,
                can_use_modelhub,
            )| {
                serde_json::json!({
                    "username": username,
                    "role": role,
                    "online": online,
                    "created_at": created_at,
                    "last_online": last_online,
                    "storage_limit_bytes": storage_limit_bytes,
                    "can_use_modelhub": can_use_modelhub,
                })
            },
        )
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({ "accounts": accounts })),
    )
        .into_response()
}

/// POST /internal-api/_auth/delete — delete an account by username. Admin/Moderator.
/// Accepts optional `keep_data` boolean (default false). When false, the user's
/// gallery directory is also removed. When true, data is preserved and will be
/// restored if an account with the same username is re-created.
async fn auth_delete_account_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can delete accounts.");
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
    let keep_data = req
        .get("keep_data")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Moderators cannot delete admin or other moderator accounts
    if role == UserRole::Moderator {
        if let Some(target_role) = state.auth.get_account_role(username) {
            if target_role == "admin" || target_role == "moderator" {
                return forbidden_response("Moderators can only manage regular user accounts.");
            }
        }
    }

    match state.auth.delete_account(username) {
        Ok(()) => {
            if !keep_data {
                // Remove the user's gallery directory
                if let Some(dir) = user_gallery_dir(Some(username)) {
                    if dir.exists() {
                        log::info!("Deleting gallery data for user '{}': {:?}", username, dir);
                        let _ = std::fs::remove_dir_all(&dir);
                    }
                }
            } else {
                log::info!(
                    "Keeping gallery data for deleted user '{}' (re-create to restore)",
                    username
                );
            }
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
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
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can reset passwords.");
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

    // Moderators cannot reset passwords for admin or other moderator accounts
    if role == UserRole::Moderator {
        if let Some(target_role) = state.auth.get_account_role(username) {
            if target_role == "admin" || target_role == "moderator" {
                return forbidden_response("Moderators can only manage regular user accounts.");
            }
        }
    }

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

/// POST /internal-api/_auth/set_role — admin/moderator sets the role of an account.
async fn auth_set_role_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can change user roles.");
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
    // Moderators cannot promote to admin — only admins can do that
    if new_role == "admin" && role != UserRole::Admin {
        return forbidden_response("Only admins can promote accounts to admin.");
    }
    match state.auth.set_account_role(username, new_role) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /internal-api/_auth/set_modelhub_access — admin/moderator toggles Model Hub access for a user.
async fn auth_set_modelhub_access_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can change Model Hub access.");
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
    let allowed = req
        .get("allowed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match state.auth.set_modelhub_access(username, allowed) {
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
            let safe = name.to_ascii_lowercase().replace(['/', '\\', '.'], "_");
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

    // Sanitize client-controlled values to prevent path traversal
    let safe_filename = std::path::Path::new(filename)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let safe_prompt_id = prompt_id.replace(['/', '\\', '.'], "_");

    if safe_filename.is_empty() {
        return Err("Invalid filename".to_string());
    }

    let normalized_mode = match mode {
        Some("txt2img") => "txt2img",
        Some("img2img") => "img2img",
        Some("inpainting") => "inpainting",
        _ => "unknown",
    };

    let gallery_filename = format!("{}__{}__{}", safe_prompt_id, normalized_mode, safe_filename);
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

// ---------------------------------------------------------------------------
// Storage management — usage info, limits, and image expiry
// ---------------------------------------------------------------------------

/// Compute total size of files in a directory (non-recursive, images only).
fn dir_usage_bytes(dir: &std::path::Path) -> u64 {
    if !dir.exists() {
        return 0;
    }
    std::fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_file()))
                .filter_map(|e| e.metadata().ok().map(|m| m.len()))
                .sum()
        })
        .unwrap_or(0)
}

/// GET /internal-api/_storage/info — returns current user's storage usage,
/// limit, and per-image expiry information.
async fn storage_info_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let username = resolve_username(&state, &headers, &remote);
    let gallery_dir = match user_gallery_dir(username.as_deref()) {
        Some(d) => d,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Cannot resolve gallery directory" })),
            )
                .into_response();
        }
    };

    let usage_bytes = dir_usage_bytes(&gallery_dir);

    // For admins (localhost), storage is unlimited
    let (limit_bytes, expiry_secs) = if role == UserRole::Admin && username.is_none() {
        (0_u64, 0_u64) // 0 means unlimited
    } else {
        let name = username.as_deref().unwrap_or("admin");
        let limit = state.auth.get_storage_limit(name);
        (limit, crate::auth::DEFAULT_EXPIRY_SECS)
    };

    // Collect per-image age info (oldest first)
    let now = std::time::SystemTime::now();
    let mut images: Vec<serde_json::Value> = Vec::new();
    if gallery_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&gallery_dir) {
            for entry in entries.flatten() {
                if entry.file_type().ok().is_none_or(|ft| !ft.is_file()) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if !(name.ends_with(".png")
                    || name.ends_with(".jpg")
                    || name.ends_with(".jpeg")
                    || name.ends_with(".webp"))
                {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    let modified = meta.modified().ok();
                    let age_secs = modified
                        .and_then(|m| now.duration_since(m).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let size = meta.len();
                    images.push(serde_json::json!({
                        "filename": name,
                        "size_bytes": size,
                        "age_secs": age_secs,
                        "expires_in_secs": if expiry_secs > 0 { expiry_secs.saturating_sub(age_secs) } else { 0 },
                    }));
                }
            }
        }
    }
    images.sort_by(|a, b| {
        let aa = a["age_secs"].as_u64().unwrap_or(0);
        let ba = b["age_secs"].as_u64().unwrap_or(0);
        ba.cmp(&aa) // oldest first
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "usage_bytes": usage_bytes,
            "limit_bytes": limit_bytes,
            "expiry_secs": expiry_secs,
            "image_count": images.len(),
            "images": images,
        })),
    )
        .into_response()
}

/// POST /internal-api/_storage/set_limit — admin/mod sets a user's storage limit.
/// Body: `{ "username": "...", "limit_bytes": 4294967296 }`
async fn storage_set_limit_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can change storage limits.");
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

    // Moderators cannot change storage limits for admin accounts
    if role == UserRole::Moderator {
        if let Some(target_role) = state.auth.get_account_role(username) {
            if target_role == "admin" {
                return forbidden_response("Moderators cannot modify admin storage limits.");
            }
        }
    }

    let limit_bytes = match req.get("limit_bytes").and_then(|v| v.as_u64()) {
        Some(l) => l,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing or invalid limit_bytes" })),
            )
                .into_response();
        }
    };

    match state.auth.set_storage_limit(username, limit_bytes) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// Clean up expired images for all users. Images older than DEFAULT_EXPIRY_SECS
/// are deleted. Admin (root gallery) images are never expired.
fn cleanup_expired_images(auth: &AuthState) {
    let base = match config::gallery_dir() {
        Some(d) => d,
        None => return,
    };
    let users_dir = base.join("users");
    if !users_dir.exists() {
        return;
    }

    let expiry = std::time::Duration::from_secs(crate::auth::DEFAULT_EXPIRY_SECS);
    let now = std::time::SystemTime::now();
    let _ = auth; // auth is available for future per-user expiry overrides

    let user_dirs = match std::fs::read_dir(&users_dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for dir_entry in user_dirs.flatten() {
        if !dir_entry.file_type().ok().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        let user_dir = dir_entry.path();
        let files = match std::fs::read_dir(&user_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let mut expired_count = 0_u64;
        let mut expired_bytes = 0_u64;
        for file_entry in files.flatten() {
            if file_entry.file_type().ok().is_none_or(|ft| !ft.is_file()) {
                continue;
            }
            let name = file_entry.file_name().to_string_lossy().into_owned();
            if !(name.ends_with(".png")
                || name.ends_with(".jpg")
                || name.ends_with(".jpeg")
                || name.ends_with(".webp"))
            {
                continue;
            }
            if let Ok(meta) = file_entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age > expiry {
                            let size = meta.len();
                            if std::fs::remove_file(file_entry.path()).is_ok() {
                                expired_count += 1;
                                expired_bytes += size;
                            }
                        }
                    }
                }
            }
        }
        if expired_count > 0 {
            log::info!(
                "[storage] Cleaned up {} expired image(s) ({:.1} MB) from {}",
                expired_count,
                expired_bytes as f64 / 1_048_576.0,
                user_dir.display(),
            );
        }
    }
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
                // Cancel any in-progress generation before exiting so the
                // ComfyUI queue doesn't keep running after the tab closes.
                let _ = state.gpu_manager.interrupt(None).await;
                std::process::exit(0);
            }
        }
    });
}
