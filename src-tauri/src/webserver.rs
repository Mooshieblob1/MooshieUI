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
const TEMP_EVENT_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

#[cfg(feature = "desktop")]
fn spawn_background(task: BackgroundTask) {
    tauri::async_runtime::spawn(task);
}

#[cfg(not(feature = "desktop"))]
fn spawn_background(task: BackgroundTask) {
    tokio::spawn(task);
}

fn schedule_temp_event_cache_cleanup(state: Arc<AppState>, prompt_ids: Vec<String>) {
    if prompt_ids.is_empty() {
        return;
    }

    spawn_background(Box::pin(async move {
        tokio::time::sleep(TEMP_EVENT_CACHE_TTL).await;
        let mut outputs = state.output_image_cache.write().unwrap();
        let mut previews = state.last_preview_by_prompt.write().unwrap();
        for prompt_id in prompt_ids {
            outputs.remove(&prompt_id);
            previews.remove(&prompt_id);
        }
    }));
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

fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{}=", name);
    query.split('&').find_map(|p| p.strip_prefix(&prefix))
}

fn username_for_token(state: &WebState, token: &str) -> Option<Option<String>> {
    let username = state.auth.validate_token(token)?;
    if state.auth.get_account_role(&username).as_deref() == Some("admin") {
        Some(None)
    } else {
        Some(Some(username))
    }
}

/// Resolve a LAN gallery/temp-image user from either Authorization or ?token=.
/// Missing or invalid remote LAN credentials must fail closed; `None` is only
/// valid for localhost/non-LAN mode or an authenticated admin account.
fn resolve_username_with_query_token(
    state: &WebState,
    headers: &HeaderMap,
    remote: &SocketAddr,
    query: &str,
) -> Option<Option<String>> {
    if !state.lan_enabled || is_localhost(remote) {
        return Some(None);
    }

    if resolve_role(state, headers, remote) != UserRole::Anonymous {
        return Some(resolve_username(state, headers, remote));
    }

    if let Some(token) = query_param(query, "token") {
        if let Some(username) = username_for_token(state, token) {
            return Some(username);
        }
    }

    None
}

/// Authentication gate for the read-only external proxies (cdn/animadex).
/// These serve `<img>`-style requests that cannot set an Authorization header,
/// so a `?token=` query param is also accepted. Localhost / non-LAN callers are
/// always allowed; remote LAN clients must present valid credentials.
fn proxy_request_authed(
    state: &WebState,
    headers: &HeaderMap,
    remote: &SocketAddr,
    query: &str,
) -> bool {
    if !state.lan_enabled || is_localhost(remote) {
        return true;
    }
    if resolve_role(state, headers, remote) != UserRole::Anonymous {
        return true;
    }
    query_param(query, "token")
        .and_then(|t| state.auth.validate_token(t))
        .is_some()
}

/// Progress/log events emitted only by admin/moderator operations (setup,
/// model downloads, package/backend installs). Regular LAN users must not
/// receive these on their SSE stream.
fn is_staff_only_event(event: &str) -> bool {
    event.starts_with("setup:")
        || event.starts_with("download:")
        || event.starts_with("install:")
        || event.starts_with("attention:")
        || event == "custom_node:installed"
}

/// Which gallery files the browser-mode endpoints list, quota-count, and
/// expire. Delegates to the canonical filter next to the save pipeline.
fn is_gallery_image_filename(name: &str) -> bool {
    crate::commands::api::is_listable_gallery_file(name)
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
    "install_attention_backend",
    "clear_all_queues",
    // external LLM provider settings: these mutate config and spend the
    // instance's API key, so they follow `update_config` rather than the
    // enhance/compose commands every user may run
    "get_llm_provider",
    "set_llm_provider",
    "set_llm_api_key",
    "set_llm_model",
    "set_llm_base_url",
    "list_external_llm_models",
    // previously admin-only: mode switching, filesystem, node install
    "switch_to_app_mode",
    "set_gallery_path",
    "install_custom_node",
    "install_rife",
    "install_h3_turbo",
    "install_h3_teacache",
    "import_image_directory",
    "open_directory",
    "move_installation",
    "read_image_metadata_path",
    "save_image_file",
    "save_text_file",
    "upload_image",
    "delete_model_file",
    "move_model_file",
    "create_model_folder",
];

/// Model Hub commands that require explicit per-user access for regular users.
const MODELHUB_COMMANDS: &[&str] = &[
    "civitai_search_models",
    "civitai_get_model",
    "civitai_list_architectures",
    "civitai_lookup_hash",
    "download_model",
    "resolve_download_filename",
    "cancel_download",
    "get_model_install_dirs",
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

/// Remote LAN clients must authenticate; localhost and non-LAN mode stay open.
#[allow(dead_code)]
fn require_remote_lan_auth(
    state: &WebState,
    headers: &HeaderMap,
    remote: &SocketAddr,
) -> Option<Response> {
    if state.lan_enabled && !is_localhost(remote) {
        let role = resolve_role(state, headers, remote);
        if role == UserRole::Anonymous {
            return Some(unauthorized_response("Authentication required"));
        }
    }
    None
}

async fn resolve_tls_config(
    state: &Arc<AppState>,
) -> Option<axum_server::tls_rustls::RustlsConfig> {
    let (cert_path, key_path) = {
        let cfg = state.config.read().await;
        (cfg.tls_cert_path.clone(), cfg.tls_key_path.clone())
    };
    let cert_path = cert_path
        .or_else(|| std::env::var("MOOSHIEUI_TLS_CERT_PATH").ok())
        .filter(|s| !s.trim().is_empty());
    let key_path = key_path
        .or_else(|| std::env::var("MOOSHIEUI_TLS_KEY_PATH").ok())
        .filter(|s| !s.trim().is_empty());

    match (cert_path, key_path) {
        (Some(cert), Some(key)) => {
            match axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await {
                Ok(cfg) => {
                    log::info!("TLS enabled for UI web server");
                    Some(cfg)
                }
                Err(e) => {
                    log::error!("Failed to load TLS certificate: {}", e);
                    None
                }
            }
        }
        (Some(_), None) | (None, Some(_)) => {
            log::warn!(
                "TLS cert/key pair incomplete — set both tls_cert_path and tls_key_path (or MOOSHIEUI_TLS_CERT_PATH / MOOSHIEUI_TLS_KEY_PATH)"
            );
            None
        }
        _ => None,
    }
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
                                    let temp_cache_ids =
                                        cleanup_state.prompt_queue.related_ids(&pid);
                                    let owner = cleanup_state.prompt_queue.owner_of(&pid);
                                    log::info!(
                                        "[gen] completed prompt={} user={}",
                                        &pid[..8.min(pid.len())],
                                        owner.as_deref().unwrap_or("admin"),
                                    );
                                    let finished = cleanup_state.prompt_queue.finish(&pid);
                                    if finished.is_none() {
                                        if let Some(raw_pid) =
                                            evt.payload.get("prompt_id").and_then(|v| v.as_str())
                                        {
                                            cleanup_state
                                                .prompt_queue
                                                .park_deferred_finish(raw_pid);
                                        }
                                    }
                                    if let Some(wid) = finished {
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
                                    schedule_temp_event_cache_cleanup(
                                        cleanup_state.clone(),
                                        temp_cache_ids,
                                    );
                                }
                            }
                        }
                        "comfyui:execution_error" => {
                            if let Some(pid) = prompt_id {
                                let temp_cache_ids = cleanup_state.prompt_queue.related_ids(&pid);
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
                                schedule_temp_event_cache_cleanup(
                                    cleanup_state.clone(),
                                    temp_cache_ids,
                                );
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
                        // Measure reservation age from when the worker was
                        // reserved, NOT from `last_released`. The old code used
                        // `now_ms / 1000` (epoch seconds, ~1.7e9) whenever
                        // `last_released == 0`, which is always > 600, so a
                        // freshly-started worker running its very first job got
                        // force-released the instant the watchdog observed it in
                        // the brief window before the dispatcher records its
                        // prompt in the worker map.
                        let reserved_at = worker
                            .reserved_at
                            .load(std::sync::atomic::Ordering::Acquire);
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        // reserved_at == 0 means the current reservation predates
                        // this field (or we observed the tiny race between the
                        // reserve CAS and the timestamp store) — treat as not-yet-
                        // stuck rather than releasing an in-flight job.
                        let stuck_secs = if reserved_at == 0 {
                            0
                        } else {
                            (now_ms.saturating_sub(reserved_at)) / 1000
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
        app: state.clone(),
        auth: Arc::new(AuthState::new()),
        lan_enabled,
    });
    web_state
        .auth
        .try_emit_legacy_password_announcement(&web_state.app.notifications);

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
            "/internal-api/_auth/upgrade_password_encryption",
            post(auth_upgrade_password_encryption_handler),
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
        // Per-user preference sync (cross-device prefs in browser/LAN mode)
        .route(
            "/internal-api/_user/prefs",
            get(user_prefs_get_handler).put(user_prefs_put_handler),
        )
        // Storage management
        .route("/internal-api/_storage/info", get(storage_info_handler))
        .route(
            "/internal-api/_storage/set_limit",
            post(storage_set_limit_handler),
        )
        // Model request queue
        .route(
            "/internal-api/_model_requests",
            get(model_requests_list_handler),
        )
        .route(
            "/internal-api/_model_requests/add",
            post(model_requests_add_handler),
        )
        .route(
            "/internal-api/_model_requests/approve",
            post(model_requests_approve_handler),
        )
        .route(
            "/internal-api/_model_requests/deny",
            post(model_requests_deny_handler),
        )
        // Notifications
        .route(
            "/internal-api/_notifications",
            get(notifications_list_handler),
        )
        .route(
            "/internal-api/_notifications/unread_count",
            get(notifications_unread_count_handler),
        )
        .route(
            "/internal-api/_notifications/mark_read",
            post(notifications_mark_read_handler),
        )
        .route(
            "/internal-api/_notifications/mark_all_read",
            post(notifications_mark_all_read_handler),
        )
        .route(
            "/internal-api/_notifications/dismiss",
            post(notifications_dismiss_handler),
        )
        .route(
            "/internal-api/_notifications/clear",
            post(notifications_clear_handler),
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
        // Export download endpoint (serves encoded animation files from export temp dir)
        .route(
            "/internal-api/_export/{filename}",
            get(export_download_handler),
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
        // Animadex characters API proxy (read-only, api/characters/* only)
        .route(
            "/internal-api/_animadex/{*path}",
            get(animadex_proxy_handler),
        )
        // Generic IPC command proxy
        .route("/internal-api/{command}", post(command_handler))
        // Static file serving (frontend). In debug builds, prefer the Vite dev
        // server (port 1420) when it is running so browser mode works during
        // `pnpm tauri dev` instead of serving a stale `dist/` bundle.
        .fallback(get({
            let dist = dist_dir.clone();
            let http_client = web_state.app.http_client.clone();
            let vite_dev_proxy =
                cfg!(debug_assertions) && is_vite_dev_server_running(&http_client).await;
            if vite_dev_proxy {
                log::info!(
                    "Browser mode UI: proxying to Vite dev server at {}",
                    dev_vite_origin()
                );
            }
            move |req: axum::extract::Request| {
                let dist = dist.clone();
                let http_client = http_client.clone();
                async move { serve_static_or_dev(dist, http_client, vite_dev_proxy, req).await }
            }
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
    // Skipped entirely when `gallery_never_expire` is set (checked each cycle so
    // toggling the setting takes effect without a restart).
    {
        let expiry_auth = web_state.auth.clone();
        let expiry_app = web_state.app.clone();
        tokio::spawn(async move {
            // Run once at startup after a short delay, then every 30 minutes
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30 * 60));
            loop {
                interval.tick().await;
                let never_expire = expiry_app.config.read().await.gallery_never_expire;
                if !never_expire {
                    cleanup_expired_images(&expiry_auth);
                }
            }
        });
    }

    let tls_config = resolve_tls_config(&web_state.app).await;
    let actual_port = bind_addr.port();
    let handle = if let Some(tls_config) = tls_config {
        std::mem::drop(listener);
        tokio::spawn(async move {
            axum_server::bind_rustls(bind_addr, tls_config)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .expect("UI web server crashed");
        })
    } else {
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .expect("UI web server crashed");
        })
    };
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

#[cfg(debug_assertions)]
fn dev_vite_origin() -> &'static str {
    "http://127.0.0.1:1420"
}

#[cfg(not(debug_assertions))]
fn dev_vite_origin() -> &'static str {
    ""
}

/// True when `pnpm tauri dev` has Vite listening on port 1420.
async fn is_vite_dev_server_running(client: &reqwest::Client) -> bool {
    #[cfg(not(debug_assertions))]
    {
        let _ = client;
        return false;
    }
    #[cfg(debug_assertions)]
    {
        client
            .get(format!("{}/", dev_vite_origin()))
            .timeout(Duration::from_millis(800))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

async fn serve_static_or_dev(
    dist_dir: PathBuf,
    http_client: reqwest::Client,
    vite_dev_proxy: bool,
    req: axum::extract::Request,
) -> Response {
    if vite_dev_proxy {
        let fallback_uri = req.uri().clone();
        if let Some(resp) = proxy_vite_dev_request(&http_client, req).await {
            return resp;
        }
        log::warn!("Vite dev proxy failed; falling back to on-disk dist");
        let fallback_req = axum::http::Request::builder()
            .uri(fallback_uri)
            .body(axum::body::Body::empty())
            .unwrap_or_else(|_| axum::extract::Request::new(axum::body::Body::empty()));
        return serve_static(dist_dir, fallback_req).await;
    }
    serve_static(dist_dir, req).await
}

#[cfg(debug_assertions)]
async fn proxy_vite_dev_request(
    client: &reqwest::Client,
    req: axum::extract::Request,
) -> Option<Response> {
    use axum::body::Body;

    let (parts, body) = req.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("{}{}", dev_vite_origin(), path_and_query);

    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes()).ok()?;
    let body_bytes = axum::body::to_bytes(body, usize::MAX).await.ok()?;

    let mut builder = client
        .request(method, &url)
        .timeout(Duration::from_secs(30));
    for (name, value) in parts.headers.iter() {
        let name_str = name.as_str();
        if matches!(
            name_str,
            "host" | "connection" | "transfer-encoding" | "content-length"
        ) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            builder = builder.header(name_str, v);
        }
    }
    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes.to_vec());
    }

    let upstream = builder.send().await.ok()?;
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut response = Response::builder().status(status);

    let mut content_type = None;
    for (name, value) in upstream.headers().iter() {
        let name_str = name.as_str();
        if matches!(
            name_str,
            "connection" | "transfer-encoding" | "content-length"
        ) {
            continue;
        }
        if name_str.eq_ignore_ascii_case("content-type") {
            content_type = value.to_str().ok().map(|s| s.to_string());
        }
        if let Ok(v) = value.to_str() {
            response = response.header(name_str, v);
        }
    }

    let bytes = upstream.bytes().await.ok()?;
    let is_html = content_type
        .as_deref()
        .map(|ct| ct.to_ascii_lowercase().starts_with("text/html"))
        .unwrap_or(false);
    let body = if is_html {
        inject_browser_mode_flag(&String::from_utf8_lossy(&bytes)).into_bytes()
    } else {
        bytes.to_vec()
    };

    response.body(Body::from(body)).ok()
}

#[cfg(not(debug_assertions))]
async fn proxy_vite_dev_request(
    _client: &reqwest::Client,
    _req: axum::extract::Request,
) -> Option<Response> {
    None
}

fn inject_browser_mode_flag(html: &str) -> String {
    html.replacen(
        "<head>",
        "<head><script>window.__MOOSHIE_BROWSER_MODE__=true;</script>",
        1,
    )
}

/// Build an HTTP response for a static asset. Injects the browser-mode flag
/// into HTML payloads so the frontend IPC layer routes through HTTP instead
/// of Tauri.
fn build_asset_response(rel_path: &str, contents: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(rel_path)
        .first_or_octet_stream()
        .to_string();

    let contents = if mime == "text/html" {
        inject_browser_mode_flag(&String::from_utf8_lossy(&contents)).into_bytes()
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
    // Only admins/moderators receive setup/download/install progress events.
    let is_staff = matches!(role, UserRole::Admin | UserRole::Moderator);

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
                // Admin-only operation progress must not leak to regular users.
                if !is_staff && is_staff_only_event(&evt.event) {
                    return None;
                }
                // User-targeted events (e.g. llm:result / llm:error) must reach
                // only the requesting user, never every connected client.
                // `_target_user` is null for the admin connection.
                if let Some(target) = evt.payload.get("_target_user") {
                    let delivers = match target {
                        serde_json::Value::Null => sse_username.is_none(),
                        serde_json::Value::String(u) => Some(u.as_str()) == sse_username.as_deref(),
                        _ => false,
                    };
                    if !delivers {
                        return None;
                    }
                }
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

                // Strip the routing marker so it never reaches the client.
                let payload = {
                    let mut payload = payload;
                    if let Some(obj) = payload.as_object_mut() {
                        obj.remove("_target_user");
                    }
                    payload
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
/// Requires authentication so an unauthenticated LAN client cannot keep the
/// embedded server awake against the idle watchdog.
async fn heartbeat_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> StatusCode {
    if resolve_role(&state, &headers, &remote) == UserRole::Anonymous {
        return StatusCode::UNAUTHORIZED;
    }
    let mut hb = state.app.last_heartbeat.lock().await;
    *hb = std::time::Instant::now();
    StatusCode::OK
}

/// Heartbeat stop — browser sends this via sendBeacon on page unload.
async fn heartbeat_stop_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Response {
    let query = req.uri().query().unwrap_or("");
    let username = match resolve_username_with_query_token(&state, &headers, &remote, query) {
        Some(username) => username,
        None => return unauthorized_response("Authentication required"),
    };

    // If we've already switched to app mode, ignore the stop signal.
    if state
        .app
        .app_mode_active
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return StatusCode::OK.into_response();
    }

    // Cancel in-progress generation. Remote LAN users must only stop their own
    // prompts; localhost keeps the legacy browser-mode "stop everything" behavior.
    if state.lan_enabled && !is_localhost(&remote) {
        let _ = state.app.interrupt_user_prompts(username.as_deref()).await;
    } else {
        let _ = state.app.gpu_manager.interrupt(None).await;
    }
    StatusCode::OK.into_response()
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

    // Try auth from headers first, then from ?token= query param (for <img> tags).
    let username = match resolve_username_with_query_token(&state, &headers, &remote, query) {
        Some(username) => username,
        None => return unauthorized_response("Authentication required"),
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

    // Auth: try headers first, then ?token= query param.
    let username = match resolve_username_with_query_token(&state, &headers, &remote, query) {
        Some(username) => username,
        None => return unauthorized_response("Authentication required"),
    };
    let gallery_dir = match user_gallery_dir(username.as_deref()) {
        Some(d) => d,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "No gallery dir").into_response();
        }
    };

    let file_path = gallery_dir.join(&filename);
    if filename.to_ascii_lowercase().ends_with(".mp4") {
        return serve_video_file(&file_path, headers.get(axum::http::header::RANGE)).await;
    }
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

/// Serve a produced export by basename out of the export temp dir.
///
/// Browser mode runs the encode on the server, so Download has to fetch the
/// bytes back. Basename only, and only from that one directory - a path with
/// any separator in it is rejected outright rather than normalised.
async fn export_download_handler(
    axum::extract::Path(filename): axum::extract::Path<String>,
) -> Response {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let path = crate::commands::video_export::export_temp_dir().join(&filename);
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = if filename.ends_with(".gif") {
        "image/gif"
    } else if filename.ends_with(".webp") {
        "image/webp"
    } else if filename.ends_with(".mp4") {
        "video/mp4"
    } else {
        // AVIF is the fallback arm for the same reason run_export's is: it is the
        // recommended format, so an unexpected extension lands on it.
        "image/avif"
    };
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, mime.to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Serve an mp4 with single-range support so `<video>` seeking works.
/// Open-ended ranges are capped (see `http_range::OPEN_END_CHUNK`) — players
/// re-request as they play, so the server never reads a whole multi-hundred-MB
/// file for one request.
async fn serve_video_file(
    path: &std::path::Path,
    range: Option<&axum::http::HeaderValue>,
) -> Response {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => return (StatusCode::NOT_FOUND, "Video not found").into_response(),
    };
    let len = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => 0,
    };
    match range
        .and_then(|v| v.to_str().ok())
        .and_then(|h| crate::http_range::parse(h, len))
    {
        Some((start, end)) => {
            if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
                return (StatusCode::INTERNAL_SERVER_ERROR, "Seek failed").into_response();
            }
            let mut buf = vec![0u8; (end - start + 1) as usize];
            if file.read_exact(&mut buf).await.is_err() {
                return (StatusCode::INTERNAL_SERVER_ERROR, "Read failed").into_response();
            }
            (
                StatusCode::PARTIAL_CONTENT,
                [
                    ("content-type", "video/mp4".to_string()),
                    ("accept-ranges", "bytes".to_string()),
                    ("content-range", format!("bytes {start}-{end}/{len}")),
                    ("cache-control", "no-cache".to_string()),
                ],
                buf,
            )
                .into_response()
        }
        None => {
            // No Range header: stream the file in chunks rather than reading it
            // whole. A 15 s H3 mp4 can be hundreds of MB, and the browser-mode
            // save-video-as download takes this branch. Content-Length stays
            // exact so downloads are still verifiable and resumable.
            let stream = futures_util::stream::unfold(Some(file), |state| async move {
                let mut file = state?;
                let mut buf = vec![0u8; 256 * 1024];
                match file.read(&mut buf).await {
                    Ok(0) => None,
                    Ok(n) => {
                        buf.truncate(n);
                        Some((
                            Ok::<_, std::io::Error>(axum::body::Bytes::from(buf)),
                            Some(file),
                        ))
                    }
                    // Ending the stream with the error surfaces a truncated
                    // body to the client instead of silently short-reading.
                    Err(e) => Some((Err(e), None)),
                }
            });
            (
                StatusCode::OK,
                [
                    ("content-type", "video/mp4".to_string()),
                    ("accept-ranges", "bytes".to_string()),
                    ("content-length", len.to_string()),
                    ("cache-control", "no-cache".to_string()),
                ],
                axum::body::Body::from_stream(stream),
            )
                .into_response()
        }
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
    if resolve_username_with_query_token(&state, &headers, &remote, query).is_none() {
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
        crate::metadata::ImageFormat::WebP => {
            match crate::metadata::embed_webp_metadata(&bytes, &metadata, embed_mode) {
                Ok(b) => (b, "webp"),
                Err(e) => {
                    log::warn!("embed_temp_metadata (WebP) failed: {}", e);
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

/// Animadex API proxy — characters search/facets only.
async fn animadex_proxy_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(path): Path<String>,
    uri: axum::http::Uri,
) -> Response {
    if !proxy_request_authed(&state, &headers, &remote, uri.query().unwrap_or("")) {
        return unauthorized_response("Authentication required");
    }
    let clean = path.trim_start_matches('/');
    if !clean.starts_with("api/characters/") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut target_url = format!("https://animadex.net/{}", clean);
    if let Some(query) = uri.query() {
        target_url.push('?');
        target_url.push_str(query);
    }
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

/// CDN proxy handler — fetches assets from cdn.mooshieblob.com and forwards
/// them to the browser with CORS headers so in-browser mode works correctly.
/// Only proxies from the hardcoded CDN origin; this is NOT an open proxy.
async fn cdn_proxy_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(path): Path<String>,
    uri: axum::http::Uri,
) -> Response {
    if !proxy_request_authed(&state, &headers, &remote, uri.query().unwrap_or("")) {
        return unauthorized_response("Authentication required");
    }
    let mut target_url = format!("https://cdn.mooshieblob.com/{}", path);
    if let Some(query) = uri.query() {
        target_url.push('?');
        target_url.push_str(query);
    }
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

    // A GUI action (opening a file explorer) only makes sense when the browser
    // client is on the same machine as the server: localhost-only web mode, or a
    // localhost request on a LAN-enabled server. A remote LAN client must never
    // pop a window on the operator's screen.
    let caller_is_local = !state.lan_enabled || is_localhost(&remote);

    match dispatch_command(
        state.app.clone(),
        &state.auth,
        &command,
        &args,
        username.as_deref(),
        role,
        caller_is_local,
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
fn ensure_prompt_owned(
    state: &AppState,
    prompt_id: &str,
    username: Option<&str>,
) -> Result<(), String> {
    let caller = username.map(str::to_string);
    if state.prompt_queue.is_owned_by(prompt_id, &caller) {
        Ok(())
    } else {
        Err("Prompt does not belong to the current user".to_string())
    }
}

async fn dispatch_command(
    state: Arc<AppState>,
    auth: &Arc<AuthState>,
    command: &str,
    args: &serde_json::Value,
    username: Option<&str>,
    caller_role: UserRole,
    caller_is_local: bool,
) -> Result<serde_json::Value, String> {
    match command {
        // --- Config ---
        "get_config" => {
            let config = state.config.read().await;
            let include_secrets = matches!(caller_role, UserRole::Admin | UserRole::Moderator);
            crate::config::config_to_client_json(&config, include_secrets)
                .map_err(|e| e.to_string())
        }
        "update_config" => {
            let mut new_config: crate::config::AppConfig =
                serde_json::from_value(args["config"].clone())
                    .map_err(|e| format!("Invalid config: {}", e))?;
            config::normalize_config_fields(&mut new_config);
            let mut current = state.config.write().await;
            config::preserve_secrets(&mut new_config, &current);
            config::save_config(&new_config)?;
            *current = new_config;
            Ok(serde_json::json!(null))
        }
        "check_attention_backend" => {
            let status = crate::commands::api::check_attention_backend_core(&state)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(&status).map_err(|e| e.to_string())
        }
        "get_compute_capability" => {
            let cc = crate::commands::api::detect_compute_capability_pub();
            serde_json::to_value(cc).map_err(|e| e.to_string())
        }
        "install_attention_backend" => {
            let backend = args["backend"]
                .as_str()
                .ok_or("Missing backend")?
                .to_string();
            let bcast = state.clone();
            crate::commands::api::install_attention_backend_core(&state, backend, move |msg| {
                bcast.broadcast("attention:install_progress", serde_json::json!(msg));
            })
            .await
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "switch_to_app_mode" => {
            #[cfg(not(feature = "desktop"))]
            {
                return Err("switch_to_app_mode is only available in desktop mode".into());
            }
            #[cfg(feature = "desktop")]
            {
                // Step 1: Save config. Snapshot under the guard, then write to
                // disk after dropping it so the blocking file write doesn't hold
                // the config write lock.
                let cfg = {
                    let mut cfg = state.config.write().await;
                    cfg.browser_mode = false;
                    cfg.clone()
                };
                config::save_config(&cfg)?;

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
                        let configured_worker_mode = {
                            let config = state_clone.config.read().await;
                            process::uses_configured_gpu_workers(&config)
                        };
                        if configured_worker_mode {
                            process::wait_all_workers_ready(&state_clone, 120).await;

                            let mut ready_any = false;
                            for worker in &state_clone.gpu_manager.workers {
                                let status = *worker.status.read().await;
                                if status != crate::comfyui::gpu_manager::WorkerStatus::Idle {
                                    continue;
                                }
                                ready_any = true;
                                if let Err(e) = websocket::connect_websocket_for_worker(
                                    &state_clone,
                                    worker,
                                    event_tx.clone(),
                                )
                                .await
                                {
                                    log::error!("Worker {} WebSocket failed: {}", worker.id, e);
                                }
                            }

                            if ready_any {
                                let _ = event_tx.send(crate::state::BroadcastEvent {
                                    event: "comfyui:server_ready".to_string(),
                                    payload: serde_json::json!(null),
                                });
                            } else {
                                let err_str = "No configured GPU workers became ready".to_string();
                                log::error!("{}", err_str);
                                let port = state_clone.config.read().await.server_port;
                                let _ = event_tx.send(crate::state::BroadcastEvent {
                                    event: "comfyui:server_error".to_string(),
                                    payload: crate::comfyui::nodes::server_error_payload(
                                        &err_str, port,
                                    ),
                                });
                            }
                            return;
                        }
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
                                let port = state_clone.config.read().await.server_port;
                                let _ = event_tx.send(crate::state::BroadcastEvent {
                                    event: "comfyui:server_error".to_string(),
                                    payload: crate::comfyui::nodes::server_error_payload(
                                        &err_str, port,
                                    ),
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

            // Regular LAN users must not see other users' prompt ids from the
            // shared ComfyUI queue. Admins/moderators keep the global view.
            let caller = username.map(str::to_string);
            let is_privileged = match username {
                None => true,
                Some(u) => matches!(
                    auth.get_account_role(u).as_deref(),
                    Some("admin") | Some("moderator")
                ),
            };
            if !is_privileged {
                running.retain(|entry| {
                    entry
                        .as_array()
                        .and_then(|a| a.get(1))
                        .and_then(|v| v.as_str())
                        .is_some_and(|pid| state.prompt_queue.is_owned_by(pid, &caller))
                });
                pending.retain(|entry| {
                    entry
                        .as_array()
                        .and_then(|a| a.get(1))
                        .and_then(|v| v.as_str())
                        .is_some_and(|pid| state.prompt_queue.is_owned_by(pid, &caller))
                });
            }

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
                q.iter()
                    .filter(|(_, owner)| is_privileged || *owner == caller)
                    .map(|(pid, _)| pid.clone())
                    .collect()
            };
            const SUBMISSION_SHIELD_SECS: u64 = 120;
            for pid in tracked {
                if !known.contains(&pid) {
                    // Shield unreconciled placeholders only while submission is
                    // still in flight. If submit_prompt hangs past this window,
                    // drop the synthetic entry so the frontend reconciler can
                    // surface "generation lost" instead of staying on Preparing.
                    if pid.starts_with("gen-")
                        && !state.prompt_queue.is_placeholder_bound(&pid)
                        && state
                            .prompt_queue
                            .insert_age_secs(&pid)
                            .is_some_and(|age| age > SUBMISSION_SHIELD_SECS)
                    {
                        continue;
                    }
                    pending.push(serde_json::json!([0, pid, {}, {}, []]));
                }
            }

            // Build ordered queue positions from our internal fair-queue tracker.
            // This is separate from ComfyUI's queue and reflects round-robin ordering.
            let queue_positions: Vec<serde_json::Value> = {
                let queue = state.prompt_queue.queue.read().unwrap();
                queue
                    .iter()
                    .enumerate()
                    .filter(|(_, (id, _))| {
                        is_privileged || state.prompt_queue.is_owned_by(id, &caller)
                    })
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
            ensure_prompt_owned(&state, prompt_id, username)?;
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
            let prompt_id = args["promptId"].as_str().ok_or("Missing promptId")?;
            ensure_prompt_owned(&state, prompt_id, username)?;
            let ids = state.prompt_queue.related_ids(prompt_id);
            let mut cached = Vec::new();
            {
                // Read without removing so a later reconcile pass can retry
                // recovery if the client's image fetch fails. The entries are
                // cleaned up by the TEMP_EVENT_CACHE_TTL reactor, so leaving
                // them in place does not leak.
                let outputs = state.output_image_cache.read().unwrap();
                for id in &ids {
                    if let Some(files) = outputs.get(id) {
                        cached.extend(files.iter().cloned());
                    }
                }
            }
            let mut seen = std::collections::HashSet::new();
            cached.retain(|f| seen.insert(f.clone()));
            // Return every cached output. Recovery only runs when the client
            // received zero images, so a Hires Fix prompt (pre-upscale +
            // refined) must yield both here rather than only the last.
            let images: Vec<serde_json::Value> = cached
                .into_iter()
                .map(|f| serde_json::json!({ "temp_filename": f }))
                .collect();
            Ok(serde_json::json!({ "images": images }))
        }
        "interrupt_generation" => {
            if let Some(prompt_id) = args["promptId"].as_str() {
                ensure_prompt_owned(&state, prompt_id, username)?;
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
                    if is_gallery_image_filename(&name) {
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
            // One query for the whole video table, not one per directory entry.
            let meta = crate::gallery_index::video_meta();
            let mut files: Vec<_> = std::fs::read_dir(&dir)
                .map_err(|e| e.to_string())?
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    if entry.file_type().ok()?.is_dir() {
                        return None;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if !is_gallery_image_filename(&name) {
                        return None;
                    }
                    let metadata = entry.metadata().ok()?;
                    let modified = metadata.modified().ok()?;
                    let modified_ms = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()?
                        .as_millis() as u64;
                    let entry_meta = meta.get(&name);
                    Some(serde_json::json!({
                        "filename": name,
                        "size_bytes": metadata.len(),
                        "modified_ms": modified_ms,
                        "duration_seconds": entry_meta.map(|m| m.duration_seconds),
                        "fps": entry_meta.and_then(|m| m.fps),
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
            // JXL/WebP → PNG transcode for downloading / clipboard. PNG keeps
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
            let lower = filename.to_ascii_lowercase();
            let out = if lower.ends_with(".jxl") || lower.ends_with(".webp") {
                tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
                    let img = commands::api::decode_gallery_image(&bytes)?;
                    let mut buf = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut buf, image::ImageFormat::Png)
                        .map_err(|e| format!("PNG encode failed: {}", e))?;
                    let png = buf.into_inner();
                    // Re-embed the source file's generation metadata (JXL box or
                    // WebP EXIF chunk) as a PNG text chunk so the export is
                    // metadata-complete, matching the desktop path.
                    match crate::metadata::read_image_metadata(&bytes) {
                        Ok(Some(meta)) => Ok(crate::metadata::embed_png_metadata(
                            &png,
                            &meta,
                            crate::metadata::MetadataMode::TextChunk,
                        )
                        .unwrap_or(png)),
                        _ => Ok(png),
                    }
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
            crate::comfyui::process::mark_legacy_worker_idle(&state).await;
            let params: crate::comfyui::types::GenerationParams =
                serde_json::from_value(args["params"].clone())
                    .map_err(|e| format!("Invalid params: {}", e))?;
            crate::templates::validate_generation_params(&params)?;
            {
                let config = state.config.read().await;
                crate::commands::api::validate_lora_files_for_generation(
                    &config.comfyui_path,
                    config.extra_model_paths.as_deref(),
                    &params.loras,
                )
                .map_err(|e| e.to_string())?;
            }
            // Mirrors the same check in the Tauri `generate` command: catches a
            // missing MiniMax H3 node before submission instead of surfacing
            // ComfyUI's raw `missing_node_type` prompt-validation error.
            if params.mode == "video" {
                let base_url = state.base_url().await;
                crate::comfyui::nodes::verify_required_h3_nodes_for_generation(
                    &state.http_client,
                    &base_url,
                    &params,
                )
                .await?;
            }
            let seed = if params.seed < 0 {
                (rand::random::<u64>() >> 1) as i64
            } else {
                params.seed
            };
            let video_metadata_supported = if params.mode == "video" {
                let base_url = state.base_url().await;
                crate::comfyui::nodes::node_declares_input(
                    &state.http_client,
                    &base_url,
                    "MooshieSaveVideo",
                    "metadata_json",
                )
                .await
            } else {
                false
            };
            let workflow =
                crate::templates::build_workflow(&params, seed, video_metadata_supported);
            let user = username.map(|s| s.to_string());

            log::info!(
                "[gen] user={} seed={} steps={} mode={}",
                user.as_deref().unwrap_or("admin"),
                seed,
                params.steps,
                params.mode,
            );
            if params.controlnet.as_ref().is_some_and(|cn| cn.enabled)
                || params.facefix_enabled
                || !params.loras.is_empty()
            {
                log::info!(
                    "Workflow JSON: {}",
                    serde_json::to_string_pretty(&workflow).unwrap_or_default()
                );
            }

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
                // Release the prompt-assistant LLM's VRAM so it doesn't starve
                // ComfyUI's diffusion model during this generation. Done inside
                // the spawned task so it never delays the HTTP acknowledgment.
                bg_state.free_llm_vram_for_generation().await;
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
                    log::info!(
                        "[gen] submitting placeholder={}",
                        &bg_placeholder[..bg_placeholder.len().min(12)]
                    );
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

            // Return immediately — the frontend tracks progress via SSE/WebSocket events.
            // Seed as a string: 63-bit values exceed JS's 2^53 safe-integer range.
            Ok(serde_json::json!({
                "prompt_id": placeholder_id,
                "seed": seed.to_string(),
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
            ensure_prompt_owned(&state, prompt_id, username)?;
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
            let output_template = state.config.read().await.output_filename_template.clone();
            let result = save_to_gallery_in_dir(
                &dir,
                &bytes,
                &filename,
                &prompt_id,
                mode.as_deref(),
                metadata.as_ref(),
                metadata_mode.as_deref(),
                output_template.as_deref(),
            )?;
            let payload = serde_json::json!({
                "filename": result.clone(),
                "prompt_id": prompt_id,
                "mode": mode,
                "source_filename": filename,
                "metadata": metadata,
            });
            state.broadcast("mooshie:image_saved", payload.clone());
            let _ = state.dispatch_webhook_event("image_saved", payload).await;
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
            let output_template = state.config.read().await.output_filename_template.clone();
            let result = save_to_gallery_in_dir(
                &dir,
                &image_bytes,
                &filename,
                &prompt_id,
                mode.as_deref(),
                metadata.as_ref(),
                metadata_mode.as_deref(),
                output_template.as_deref(),
            )?;
            let payload = serde_json::json!({
                "filename": result.clone(),
                "prompt_id": prompt_id,
                "mode": mode,
                "source_filename": filename,
                "metadata": metadata,
            });
            state.broadcast("mooshie:image_saved", payload.clone());
            let _ = state.dispatch_webhook_event("image_saved", payload).await;
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
            let output_template = state.config.read().await.output_filename_template.clone();
            let result = save_to_gallery_in_dir(
                &dir,
                &bytes,
                &filename,
                &prompt_id,
                mode.as_deref(),
                metadata.as_ref(),
                metadata_mode.as_deref(),
                output_template.as_deref(),
            )?;
            let payload = serde_json::json!({
                "filename": result.clone(),
                "prompt_id": prompt_id,
                "mode": mode,
                "source_filename": filename,
                "metadata": metadata,
            });
            state.broadcast("mooshie:image_saved", payload.clone());
            let _ = state.dispatch_webhook_event("image_saved", payload).await;
            // Keep the temp file available briefly for clients that still hold
            // the temp URL or race a manual save against gallery persistence.
            // Periodic cleanup handles expiry.
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
            crate::gallery_index::remove(&path);
            // Videos own a poster sidecar that listings never surface; delete it
            // together with its mp4, matching the desktop `delete_gallery_image`
            // command. Without this, deleting a video in browser mode orphans the
            // poster file and its index row forever.
            if let Some(stem) = filename.strip_suffix(".mp4") {
                let poster = path.with_file_name(format!("{stem}_poster.webp"));
                if poster.is_file() {
                    let _ = std::fs::remove_file(&poster);
                    crate::gallery_index::remove(&poster);
                }
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
            crate::gallery_index::rename(&old_path, &new_path);
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
            let result = crate::metadata::read_image_metadata(&bytes).map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "read_image_metadata_bytes" => {
            let image_bytes: Vec<u8> = serde_json::from_value(args["imageBytes"].clone())
                .map_err(|e| format!("Invalid imageBytes: {}", e))?;
            let result =
                crate::metadata::read_image_metadata(&image_bytes).map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "read_image_metadata_path" => {
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let result = crate::metadata::read_image_metadata(&bytes).map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "embed_image_metadata_bytes" => {
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
            let result = crate::metadata::embed_image_metadata(&image_bytes, &metadata, mode)
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
            let network_proxy = config.network_proxy.clone();
            let pip_index_url = config.pip_index_url.clone();
            drop(config);
            let network_proxy = network_proxy.as_deref();
            let pip_index_url = pip_index_url.as_deref();

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

            let mut git_cmd = crate::comfyui::process::tokio_command_no_window("git");
            git_cmd
                .args([
                    "clone",
                    "--progress",
                    &git_url,
                    &target_dir.to_string_lossy(),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            crate::comfyui::nodes::apply_network_proxy(&mut git_cmd, network_proxy);
            let status = git_cmd
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
                    let mut cmd = crate::comfyui::process::tokio_command_no_window(&uv_path);
                    cmd.args(["pip", "install", "-r", &req_file.to_string_lossy()])
                        .env("VIRTUAL_ENV", &venv_path);
                    crate::comfyui::nodes::apply_pip_install_options(
                        &mut cmd,
                        true,
                        network_proxy,
                        pip_index_url,
                    );
                    cmd.status()
                        .await
                        .map_err(|e| format!("uv pip install failed: {}", e))?
                } else {
                    let venv_base = std::path::Path::new(&venv_path);
                    #[cfg(target_os = "windows")]
                    let pip_path = venv_base.join("Scripts").join("pip.exe");
                    #[cfg(not(target_os = "windows"))]
                    let pip_path = venv_base.join("bin").join("pip");
                    let mut cmd = crate::comfyui::process::tokio_command_no_window(&pip_path);
                    cmd.args(["install", "-r", &req_file.to_string_lossy()]);
                    crate::comfyui::nodes::apply_pip_install_options(
                        &mut cmd,
                        false,
                        network_proxy,
                        pip_index_url,
                    );
                    cmd.status()
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
            let network_proxy = config.network_proxy.clone();
            let pip_index_url = config.pip_index_url.clone();
            drop(config);
            let network_proxy = network_proxy.as_deref();
            let pip_index_url = pip_index_url.as_deref();

            let uv_path = crate::commands::api::resolve_uv_bin_pub(&venv_path);

            let output = if uv_path.exists() {
                let mut cmd = crate::comfyui::process::tokio_command_no_window(&uv_path);
                cmd.args(["pip", "install", &package])
                    .env("VIRTUAL_ENV", &venv_path);
                crate::comfyui::nodes::apply_pip_install_options(
                    &mut cmd,
                    true,
                    network_proxy,
                    pip_index_url,
                );
                cmd.output()
                    .await
                    .map_err(|e| format!("uv pip install failed to start: {}", e))?
            } else {
                let venv_base = std::path::Path::new(&venv_path);
                #[cfg(target_os = "windows")]
                let pip_path = venv_base.join("Scripts").join("pip.exe");
                #[cfg(not(target_os = "windows"))]
                let pip_path = venv_base.join("bin").join("pip");
                let mut cmd = crate::comfyui::process::tokio_command_no_window(&pip_path);
                cmd.args(["install", &package]);
                crate::comfyui::nodes::apply_pip_install_options(
                    &mut cmd,
                    false,
                    network_proxy,
                    pip_index_url,
                );
                cmd.output()
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
        "check_python_import" => {
            let module = args["module"]
                .as_str()
                .ok_or("Missing module")?
                .trim()
                .to_string();
            if !crate::commands::api::is_valid_python_module_name(&module) {
                return Err("Invalid module name".into());
            }
            let config = state.config.read().await;
            let venv_path = config.venv_path.clone();
            drop(config);

            let python_path = crate::commands::api::resolve_venv_python_bin(&venv_path);
            let output = crate::comfyui::process::tokio_command_no_window(&python_path)
                .args(["-c", &format!("import {}", module)])
                .output()
                .await
                .map_err(|e| format!("python import check failed to start: {}", e))?;

            Ok(serde_json::json!(output.status.success()))
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

            let result = crate::commands::api::model_install_dirs_for_config(
                &comfyui_path,
                extra_model_paths.as_deref(),
                &category,
            )
            .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "list_model_files" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let config = state.config.read().await;
            let comfyui_path = config.comfyui_path.clone();
            let extra_model_paths = config.extra_model_paths.clone();
            drop(config);

            let result = tokio::task::spawn_blocking(move || {
                crate::commands::api::list_model_files_for_config(
                    &comfyui_path,
                    extra_model_paths.as_deref(),
                    &category,
                )
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "list_model_folders" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let config = state.config.read().await;
            let comfyui_path = config.comfyui_path.clone();
            let extra_model_paths = config.extra_model_paths.clone();
            drop(config);

            let result = tokio::task::spawn_blocking(move || {
                crate::commands::api::list_model_folders_for_config(
                    &comfyui_path,
                    extra_model_paths.as_deref(),
                    &category,
                )
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "create_model_folder" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let directory = args["directory"]
                .as_str()
                .ok_or("Missing directory")?
                .to_string();
            let folder_path = args["folderPath"]
                .as_str()
                .ok_or("Missing folderPath")?
                .to_string();
            let config = state.config.read().await;
            let comfyui_path = config.comfyui_path.clone();
            let extra_model_paths = config.extra_model_paths.clone();
            drop(config);

            tokio::task::spawn_blocking(move || {
                crate::commands::api::create_model_folder_for_config(
                    &comfyui_path,
                    extra_model_paths.as_deref(),
                    &category,
                    &directory,
                    &folder_path,
                )
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "delete_model_file" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let directory = args["directory"]
                .as_str()
                .ok_or("Missing directory")?
                .to_string();
            let config = state.config.read().await;
            let comfyui_path = config.comfyui_path.clone();
            let extra_model_paths = config.extra_model_paths.clone();
            drop(config);

            tokio::task::spawn_blocking(move || {
                crate::commands::api::delete_model_file_for_config(
                    &comfyui_path,
                    extra_model_paths.as_deref(),
                    &category,
                    &filename,
                    &directory,
                )
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "move_model_file" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let source_directory = args["sourceDirectory"]
                .as_str()
                .ok_or("Missing sourceDirectory")?
                .to_string();
            let target_directory = args["targetDirectory"]
                .as_str()
                .ok_or("Missing targetDirectory")?
                .to_string();
            let target_filename = args["targetFilename"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| filename.clone());
            let target_category = args["targetCategory"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| category.clone());
            let config = state.config.read().await;
            let comfyui_path = config.comfyui_path.clone();
            let extra_model_paths = config.extra_model_paths.clone();
            drop(config);

            tokio::task::spawn_blocking(move || {
                crate::commands::api::move_model_file_for_config(
                    &comfyui_path,
                    extra_model_paths.as_deref(),
                    &category,
                    &target_category,
                    &filename,
                    &source_directory,
                    &target_directory,
                    &target_filename,
                )
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "find_model_by_hash" => {
            let hash = args["hash"].as_str().ok_or("Missing hash")?.to_string();
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            if !crate::commands::api::is_safe_path_component(&category) {
                return Err("Invalid model category".into());
            }

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
            if !crate::commands::api::is_safe_path_component(&category) {
                return Err("Invalid model category".into());
            }
            if !crate::commands::api::is_safe_relative_model_path(&filename) {
                return Err("Invalid model filename".into());
            }

            let config = state.config.read().await;
            if config.comfyui_path.is_empty() {
                return Err("ComfyUI path not configured".into());
            }
            let path = std::path::Path::new(&config.comfyui_path)
                .join("models")
                .join(&category)
                .join(&filename);
            drop(config);

            if !path.is_file() {
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
            let result =
                crate::commands::api::read_modelspec_internal(&state, &category, &filename)
                    .await
                    .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }

        // --- CivitAI ---
        // TODO: refactor src-tauri/src/commands/api.rs and src-tauri/src/webserver.rs
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
        "civitai_get_model" => {
            let model_id = args
                .get("modelId")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "Missing or invalid modelId".to_string())?;
            let api_key = args
                .get("apiKey")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            crate::commands::api::civitai_get_model_internal(&state, model_id, api_key)
                .await
                .map_err(|e| e.to_string())
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
        "civitai_lookup_image" => {
            let image_ref = args["imageRef"]
                .as_str()
                .or_else(|| args["image_ref"].as_str())
                .ok_or("Missing imageRef")?
                .to_string();
            let image_id =
                commands::api::parse_civitai_image_id_pub(&image_ref).map_err(|e| e.to_string())?;
            let api_key = state.config.read().await.civitai_api_key.clone();
            let url = format!(
                "https://civitai.com/api/v1/images?imageId={}&withMeta=true",
                image_id
            );
            let mut req = state.http_client.get(&url);
            if let Some(ref key) = api_key {
                req = req.header("Authorization", format!("Bearer {}", key));
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            Ok(val)
        }
        "save_model_sidecar_thumbnail" => {
            let category = args["category"]
                .as_str()
                .ok_or("Missing category")?
                .to_string();
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let image_url = args
                .get("imageUrl")
                .or_else(|| args.get("image_url"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let gallery_filename = args
                .get("galleryFilename")
                .or_else(|| args.get("gallery_filename"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let gallery_dir = if gallery_filename.as_deref().is_some_and(|s| !s.is_empty()) {
                Some(user_gallery_dir(username).ok_or("Cannot find gallery directory")?)
            } else {
                None
            };
            commands::api::save_model_sidecar_thumbnail_inner(
                state.as_ref(),
                &category,
                &filename,
                image_url.as_deref(),
                gallery_filename.as_deref(),
                gallery_dir.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
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

            // Sidecar thumbnail (`{stem}.png` beside the model file)
            if let Some(sidecar) = crate::commands::api::read_model_sidecar_thumbnail_pub(&path) {
                result["thumbnail_url"] = serde_json::json!(sidecar);
            }
            if command == "get_checkpoint_civitai_info" {
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
            let bytes = commands::api::fetch_civitai_image_bytes(state.as_ref(), &url)
                .await
                .map_err(|e| e.to_string())?;
            use base64::{engine::general_purpose::STANDARD, Engine};
            let mime = commands::api::detect_image_mime(&bytes);
            Ok(serde_json::json!(format!(
                "data:{};base64,{}",
                mime,
                STANDARD.encode(&bytes)
            )))
        }

        // --- ComfyUI node checks ---
        "check_node_available" => {
            let node_class = args["nodeClass"]
                .as_str()
                .ok_or("Missing nodeClass")?
                .to_string();
            let required_inputs: Option<Vec<String>> = args["requiredInputs"].as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            });
            match state.api_get(&format!("/object_info/{}", node_class)).await {
                Ok(val) => Ok(serde_json::json!(crate::commands::api::node_info_matches(
                    &val,
                    &node_class,
                    required_inputs.as_deref()
                ))),
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
        "is_rife_installed" => {
            let comfyui_path = state.config.read().await.comfyui_path.clone();
            Ok(serde_json::json!(crate::comfyui::nodes::is_rife_installed(
                &comfyui_path
            )))
        }
        "install_rife" => {
            let (comfyui_path, venv_path, network_proxy, pip_index_url) = {
                let config = state.config.read().await;
                (
                    config.comfyui_path.clone(),
                    config.venv_path.clone(),
                    config.network_proxy.clone(),
                    config.pip_index_url.clone(),
                )
            };

            let emit = |step: &str, message: &str, done: bool| {
                state.broadcast(
                    "install:progress",
                    serde_json::json!({
                        "node_name": "ComfyUI-Frame-Interpolation",
                        "step": step,
                        "message": message,
                        "done": done,
                    }),
                );
            };

            let result = crate::comfyui::nodes::install_rife(
                &state.http_client,
                &comfyui_path,
                &venv_path,
                network_proxy.as_deref(),
                pip_index_url.as_deref(),
                &emit,
            )
            .await;

            if let Err(e) = &result {
                emit("error", e, true);
            }
            result.map(|_| serde_json::json!(null))
        }
        "is_h3_turbo_installed" => {
            let comfyui_path = state.config.read().await.comfyui_path.clone();
            Ok(serde_json::json!(
                crate::comfyui::nodes::is_h3_turbo_installed(&comfyui_path)
            ))
        }
        "install_h3_turbo" => {
            let (comfyui_path, venv_path, network_proxy, pip_index_url) = {
                let config = state.config.read().await;
                (
                    config.comfyui_path.clone(),
                    config.venv_path.clone(),
                    config.network_proxy.clone(),
                    config.pip_index_url.clone(),
                )
            };

            let emit = |step: &str, message: &str, done: bool| {
                state.broadcast(
                    "install:progress",
                    serde_json::json!({
                        "node_name": "ComfyUI-MiniMax-H3-Turbo",
                        "step": step,
                        "message": message,
                        "done": done,
                    }),
                );
            };

            let result = crate::comfyui::nodes::install_h3_turbo(
                &comfyui_path,
                &venv_path,
                network_proxy.as_deref(),
                pip_index_url.as_deref(),
                &emit,
            )
            .await;

            if let Err(e) = &result {
                emit("error", e, true);
            }
            result.map(|_| serde_json::json!(null))
        }
        "is_h3_teacache_installed" => {
            let comfyui_path = state.config.read().await.comfyui_path.clone();
            Ok(serde_json::json!(
                crate::comfyui::nodes::is_h3_teacache_installed(&comfyui_path)
            ))
        }
        "install_h3_teacache" => {
            let (comfyui_path, venv_path, network_proxy, pip_index_url) = {
                let config = state.config.read().await;
                (
                    config.comfyui_path.clone(),
                    config.venv_path.clone(),
                    config.network_proxy.clone(),
                    config.pip_index_url.clone(),
                )
            };

            let emit = |step: &str, message: &str, done: bool| {
                state.broadcast(
                    "install:progress",
                    serde_json::json!({
                        "node_name": "ComfyUI-MiniMaxH3-TeaCache",
                        "step": step,
                        "message": message,
                        "done": done,
                    }),
                );
            };

            let result = crate::comfyui::nodes::install_h3_teacache(
                &comfyui_path,
                &venv_path,
                network_proxy.as_deref(),
                pip_index_url.as_deref(),
                &emit,
            )
            .await;

            if let Err(e) = &result {
                emit("error", e, true);
            }
            result.map(|_| serde_json::json!(null))
        }

        // --- Config extras ---
        "set_gallery_path" => {
            let path = args["path"].as_str().unwrap_or("").to_string();
            let trimmed = path.trim().to_string();
            let resolved = if trimmed.is_empty() {
                let cfg = {
                    let mut cfg = state.config.write().await;
                    cfg.gallery_path = None;
                    cfg.clone()
                };
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
                let cfg = {
                    let mut cfg = state.config.write().await;
                    cfg.gallery_path = Some(trimmed.clone());
                    cfg.clone()
                };
                config::save_config(&cfg)?;
                trimmed
            };
            Ok(serde_json::json!(resolved))
        }

        // --- Misc ---
        "get_comfyui_version" => {
            let config = state.config.read().await;
            let comfyui_dir = std::path::Path::new(&config.comfyui_path).to_path_buf();
            drop(config);
            let info = crate::comfyui_version::comfyui_version_info(&comfyui_dir);
            Ok(serde_json::to_value(info).map_err(|e| e.to_string())?)
        }
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
            if !resp.status().is_success() {
                return Err(format!("GitHub API returned {}", resp.status()));
            }
            let releases: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            // Map to the same { version, body, published_at } shape the desktop
            // command (commands::api::fetch_release_notes) returns, so the
            // browser-mode client sees a defined `version` instead of raw GitHub
            // release objects (which carry `tag_name`, not `version`).
            let notes: Vec<serde_json::Value> = releases
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|r| {
                            let tag = r.get("tag_name")?.as_str()?;
                            Some(serde_json::json!({
                                "version": tag,
                                "body": r.get("body").and_then(|b| b.as_str()).unwrap_or(""),
                                "published_at": r
                                    .get("published_at")
                                    .and_then(|p| p.as_str())
                                    .unwrap_or(""),
                            }))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(serde_json::json!(notes))
        }
        "export_logs" => {
            // In server/browser mode there is no meaningful host filesystem path
            // for a remote browser, so build the full diagnostic log (same
            // content as the desktop command, including the llama-server log) and
            // return it as a string for the client to download.
            let frontend_logs = args
                .get("frontendLogs")
                .and_then(|v| v.as_array())
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<String>>()
                });
            let content = crate::commands::api::build_diagnostic_log(&state, frontend_logs).await;
            Ok(serde_json::json!({ "content": content }))
        }

        "cancel_download" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            state.request_download_cancel(&filename);
            Ok(serde_json::Value::Null)
        }
        "resolve_download_filename" => {
            let url = args["url"].as_str().ok_or("Missing url")?.to_string();
            let name = state
                .resolve_download_filename(&url)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!(name))
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
                    let cached_is_valid =
                        crate::comfyui::client::validate_downloaded_model_file(&dest, &filename)
                            .is_ok();
                    if !cached_is_valid {
                        let _ = std::fs::remove_file(&dest);
                    } else if let Some(ref expected_hex) = expected_sha256 {
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
            let mut req = state
                .http_client
                .get(&url)
                .header("User-Agent", "MooshieUI/1.3.0");
            if let Some(token) = crate::comfyui::client::huggingface_token_for_url(&url) {
                req = req.bearer_auth(token);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let status = resp.status();
                return Err(crate::comfyui::client::download_status_error_message(
                    &url, status,
                ));
            }
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_lowercase();
            crate::comfyui::client::reject_non_model_download_content_type(&url, &content_type)
                .map_err(|e| e.to_string())?;
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

            state.clear_download_cancel(&filename);
            progress_event(&event_tx, &filename, 0, total, false);
            let mut resp = resp;
            while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
                use std::io::Write;
                // Abort cleanly if the user cancelled this download (#399).
                if state.is_download_cancelled(&filename) {
                    drop(file);
                    let _ = std::fs::remove_file(&dest);
                    state.clear_download_cancel(&filename);
                    progress_event(&event_tx, &filename, downloaded, total, true);
                    return Err(format!("Download cancelled: {}", filename));
                }
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
            crate::comfyui::client::validate_downloaded_model_file(&dest, &filename).map_err(
                |e| {
                    let _ = std::fs::remove_file(&dest);
                    e.to_string()
                },
            )?;

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
                        // Reading an arbitrary on-disk path is a desktop-only action.
                        // Allowing it for remote LAN clients is an arbitrary file read.
                        if !caller_is_local {
                            return Err(
                                "Reading arbitrary file paths is not available in browser mode."
                                    .to_string(),
                            );
                        }
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
                        // Resolve within the caller's own gallery directory so a LAN
                        // user cannot read another user's (or the admin's) images.
                        let dir =
                            user_gallery_dir(username).ok_or("Cannot find gallery directory")?;
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

        // --- Prompt assistant ---
        #[cfg(any(feature = "desktop", feature = "server"))]
        "detect_llm_hardware" => {
            let hw = tokio::task::spawn_blocking(crate::prompt_assistant::hardware::detect)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(hw).map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "list_llm_catalog" => serde_json::to_value(crate::prompt_assistant::catalog::catalog())
            .map_err(|e| e.to_string()),
        #[cfg(any(feature = "desktop", feature = "server"))]
        "llm_status" => {
            let pa = &state.prompt_assistant;
            // `external_enabled` decides whether the frontend offers the
            // assistant at all: with an external provider configured it is
            // usable with no local model installed.
            let external_enabled = state.config.read().await.llm_external_enabled;
            Ok(serde_json::json!({
                "installed_models": pa.installed_models(),
                "active_model": pa.server.active_model(),
                "server_running": pa.server.is_running(),
                "external_enabled": external_enabled,
            }))
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "unload_llm" => {
            state.prompt_assistant.server.unload().await;
            Ok(serde_json::Value::Null)
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "enhance_prompt" | "compose_prompt" => {
            let input = if command == "enhance_prompt" {
                args["prompt"].as_str().unwrap_or("").to_string()
            } else {
                args["description"].as_str().unwrap_or("").to_string()
            };
            let family = args["family"].as_str().unwrap_or("unknown").to_string();
            let mode = if command == "enhance_prompt" {
                crate::prompt_assistant::grounding::GenMode::Enhance
            } else {
                crate::prompt_assistant::grounding::GenMode::Compose
            };
            let length = args["opts"]["length"].as_str().map(|s| s.to_string());
            let include_artists = args["opts"]["include_artists"].as_bool().unwrap_or(false);

            match args["requestId"].as_str() {
                // Browser mode delivers the result asynchronously over SSE.
                // Loading the LLM and generating can outlast the reverse proxy's
                // ~100s limit, which turns a blocking POST into a Cloudflare 524.
                // The POST returns immediately and the result/error is broadcast
                // back to the requesting user via `llm:result` / `llm:error`.
                Some(request_id) => {
                    let request_id = request_id.to_string();
                    let owner = username.map(|s| s.to_string());
                    let state = state.clone();
                    tokio::spawn(async move {
                        let (event, payload) = match run_prompt_assistant_headless(
                            &state,
                            &input,
                            &family,
                            mode,
                            length.as_deref(),
                            include_artists,
                        )
                        .await
                        {
                            Ok(text) => (
                                "llm:result",
                                serde_json::json!({
                                    "request_id": request_id,
                                    "result": text,
                                    "_target_user": owner,
                                }),
                            ),
                            Err(e) => (
                                "llm:error",
                                serde_json::json!({
                                    "request_id": request_id,
                                    "error": e,
                                    "_target_user": owner,
                                }),
                            ),
                        };
                        state.broadcast(event, payload);
                    });
                    Ok(serde_json::json!({ "queued": true }))
                }
                // Synchronous fallback for callers that don't supply a request id.
                None => {
                    let result = run_prompt_assistant_headless(
                        &state,
                        &input,
                        &family,
                        mode,
                        length.as_deref(),
                        include_artists,
                    )
                    .await?;
                    Ok(serde_json::Value::String(result))
                }
            }
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "download_llm_model" => {
            let id = args["id"].as_str().unwrap_or("").to_string();
            let variant = args["variant"].as_str().unwrap_or("").to_string();
            let state_for_progress = state.clone();
            let progress = move |filename: &str, downloaded: u64, total: u64, done: bool| {
                state_for_progress.broadcast(
                    "llm:download_progress",
                    serde_json::json!({
                        "filename": filename,
                        "downloaded": downloaded,
                        "total": total,
                        "done": done,
                    }),
                );
            };
            state
                .prompt_assistant
                .download_model(&state.http_client, &id, &variant, &progress)
                .await
                .map_err(|e| e.to_string())?;
            // Persist selected model id + mark setup done.
            {
                let mut cfg = state.config.write().await;
                cfg.prompt_assistant_model_id = Some(id.clone());
                cfg.prompt_assistant_setup_done = true;
                let _ = crate::config::save_config(&cfg);
            }
            Ok(serde_json::Value::Null)
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "delete_llm_model" => {
            let id = args["id"].as_str().unwrap_or("").to_string();
            state
                .prompt_assistant
                .delete_model(&id)
                .map_err(|e| e.to_string())?;
            Ok(serde_json::Value::Null)
        }
        // --- External LLM provider settings ---
        // Every arm returns the same key-free projection the desktop commands
        // return, so the settings UI is identical in both modes. `connect_llm_oauth`
        // has no arm on purpose: the sign-in redirect lands on a loopback port owned
        // by this process, which a browser on another machine cannot reach. Browser
        // users paste an API key instead.
        #[cfg(any(feature = "desktop", feature = "server"))]
        "get_llm_provider" => {
            let s = crate::prompt_assistant::providers::read_state(&state.config).await;
            serde_json::to_value(s).map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "set_llm_provider" => {
            let provider = args["provider"].as_str().unwrap_or("").to_string();
            let s = crate::prompt_assistant::providers::select(&state.config, &provider)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(s).map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "set_llm_api_key" => {
            let api_key = args["apiKey"].as_str().unwrap_or("").to_string();
            let s = crate::prompt_assistant::providers::store_key(&state.config, &api_key)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(s).map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "set_llm_model" => {
            let model = args["model"].as_str().unwrap_or("").to_string();
            let s = crate::prompt_assistant::providers::set_model(&state.config, &model)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(s).map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "set_llm_base_url" => {
            let base_url = args["baseUrl"].as_str().unwrap_or("").to_string();
            let s = crate::prompt_assistant::providers::set_base_url(&state.config, &base_url)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(s).map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "list_external_llm_models" => {
            let models = crate::prompt_assistant::providers::list_available_models(
                &state.http_client,
                &state.config,
            )
            .await
            .map_err(|e| e.to_string())?;
            serde_json::to_value(models).map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "call_external_llm" => {
            let system = args["system"].as_str().unwrap_or("").to_string();
            let prompt = args["prompt"].as_str().unwrap_or("").to_string();
            let max_tokens = args["maxTokens"]
                .as_u64()
                .map(|v| v as u32)
                .unwrap_or(1024)
                .clamp(64, 4096);
            // Resolved before the SSE hand-off so a browser client sees the same
            // vision behaviour as the desktop app, including the silent fallback
            // to a text-only turn when the frame cannot be read.
            let image = crate::prompt_assistant::vision::load_input_frame(
                &state,
                args["imageFilename"].as_str().map(str::to_string),
            )
            .await;

            match args["requestId"].as_str() {
                // Same SSE hand-off as enhance/compose: a long rewrite would
                // otherwise outlast the reverse proxy's ~100s limit and come back
                // as a 524 instead of an answer.
                Some(request_id) => {
                    let request_id = request_id.to_string();
                    let owner = username.map(|s| s.to_string());
                    let state = state.clone();
                    tokio::spawn(async move {
                        let (event, payload) = match chat_any_headless(
                            &state,
                            &system,
                            &prompt,
                            max_tokens,
                            image.as_ref(),
                        )
                        .await
                        {
                            Ok(text) => (
                                "llm:result",
                                serde_json::json!({
                                    "request_id": request_id,
                                    "result": text,
                                    "_target_user": owner,
                                }),
                            ),
                            Err(e) => (
                                "llm:error",
                                serde_json::json!({
                                    "request_id": request_id,
                                    "error": e,
                                    "_target_user": owner,
                                }),
                            ),
                        };
                        state.broadcast(event, payload);
                    });
                    Ok(serde_json::json!({ "queued": true }))
                }
                None => {
                    let text =
                        chat_any_headless(&state, &system, &prompt, max_tokens, image.as_ref())
                            .await?;
                    Ok(serde_json::Value::String(text))
                }
            }
        }

        // --- File operations ---
        "save_image_file" => {
            let image_bytes: Vec<u8> = serde_json::from_value(args["imageBytes"].clone())
                .map_err(|e| format!("Invalid imageBytes: {}", e))?;
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            std::fs::write(&path, &image_bytes).map_err(|e| e.to_string())?;
            Ok(serde_json::json!(null))
        }
        "export_video_animation" => {
            let filename = args["filename"].as_str().ok_or("Missing filename")?;
            let name = std::path::Path::new(filename)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .ok_or("Invalid filename")?;
            if !crate::commands::api::is_listable_gallery_file(&name) {
                return Err("Not a gallery file".into());
            }
            // Browser callers get their own gallery directory, not the root one.
            let dir = user_gallery_dir(username).ok_or("Gallery unavailable")?;
            let result = crate::commands::video_export::run_export(
                #[cfg(feature = "desktop")]
                None,
                &state,
                &dir.join(&name),
                args["format"].as_str().unwrap_or("avif"),
                args["fps"].as_u64().unwrap_or(24) as u32,
                args["width"].as_u64().unwrap_or(640) as u32,
                args["quality"].as_u64().unwrap_or(63) as u32,
                args["loopCount"].as_u64().unwrap_or(0) as u32,
                args["loopMode"].as_str().unwrap_or("auto"),
                args["crossfadeFrames"].as_u64().unwrap_or(4) as u32,
                args["keepAudio"].as_bool().unwrap_or(false),
            )
            .await
            .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "interpolate_video" => {
            let filename = args["filename"].as_str().ok_or("Missing filename")?;
            let dir = user_gallery_dir(username).ok_or("Gallery unavailable")?;
            let source = crate::commands::video_interpolate::resolve_gallery_video(&dir, filename)
                .map_err(|e| e.to_string())?;
            let settings = crate::templates::rife::RifeSettings::sanitized(
                args["multiplier"].as_u64().unwrap_or(2) as u32,
                args["scaleFactor"].as_f64().unwrap_or(1.0),
                args["fastMode"].as_bool().unwrap_or(true),
                args["ensemble"].as_bool().unwrap_or(true),
            );
            let prompt_id = crate::commands::video_interpolate::submit_interpolation(
                &state,
                &source,
                settings,
                username.map(|u| u.to_string()),
            )
            .await
            .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "prompt_id": prompt_id }))
        }
        "probe_video_export" => {
            let cap = crate::commands::video_export::probe_export_inner(&state).await;
            serde_json::to_value(cap).map_err(|e| e.to_string())
        }
        "copy_file_to_clipboard" => {
            // The server's clipboard is not the browser user's clipboard.
            // The frontend uses Download in browser mode and never calls this;
            // refusing loudly beats silently copying onto the operator's machine.
            Err("Clipboard copy is not available in browser mode".to_string())
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
            // Opening a file explorer is only meaningful when the caller shares the
            // server's desktop. For a remote LAN client this would pop a window on
            // the operator's screen, so we no-op (the directory is still ensured to
            // exist) rather than spawn a GUI the caller can't see.
            let path = args["path"].as_str().ok_or("Missing path")?.to_string();
            std::fs::create_dir_all(&path).ok();
            if !caller_is_local {
                return Ok(serde_json::json!(null));
            }
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
        "read_temp_image" => {
            let filename = args["filename"]
                .as_str()
                .ok_or("Missing filename")?
                .to_string();
            let bytes = crate::temp_images::load(&filename)
                .ok_or_else(|| format!("Temp image not found: {}", filename))?;
            Ok(serde_json::json!(bytes))
        }
        "move_installation" => {
            // Relocating the on-disk installation depends on the desktop app's
            // data-dir resolution and a running file picker; it is a desktop-only
            // maintenance action with no meaningful browser-mode equivalent.
            Err("Moving the installation is only available in the desktop app.".to_string())
        }

        // --- Clipboard ---
        // The clipboard physically belongs to the machine running the server, not
        // the remote LAN client. Restrict every clipboard command to local callers
        // so a LAN user can neither write to nor read the operator's clipboard.
        "copy_image_to_clipboard" | "copy_bytes_to_clipboard" | "read_clipboard_image"
            if !caller_is_local =>
        {
            Err("Clipboard access is only available on the local machine.".to_string())
        }
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
    // Resolve the model directory under a brief read lock, then run the
    // (multi-second, network) downloads WITHOUT holding the guard across await.
    let model_dir = { state.interrogator.read().await.model_dir() };
    if !crate::interrogator::is_model_downloaded_at(&model_dir) {
        crate::interrogator::ensure_model_downloaded_headless_at(&state.http_client, &model_dir)
            .await
            .map_err(|e| e.to_string())?;
    }
    if !crate::interrogator::is_ort_library_present_at(&model_dir) {
        crate::interrogator::ensure_ort_library_headless_at(&state.http_client, &model_dir)
            .await
            .map_err(|e| e.to_string())?;
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

/// Browser-mode twin of the desktop `chat_any`: route one system+user turn to
/// the configured external provider, else to the bundled local llama-server.
///
/// Stage updates go out over SSE instead of a Tauri emit, and there is no
/// download-progress callback because the browser model-download path has its
/// own dispatch arm.
///
/// `image` only reaches the external path; the bundled llama-server runs
/// text-only models and answers from the prompt alone.
#[cfg(any(feature = "desktop", feature = "server"))]
pub async fn chat_any_headless(
    state: &Arc<AppState>,
    system: &str,
    user: &str,
    max_tokens: u32,
    image: Option<&crate::prompt_assistant::vision::VisionImage>,
) -> Result<String, String> {
    use crate::prompt_assistant::hardware;

    let (model_id, idle_secs, ext_enabled, provider, ext_base, ext_key, ext_model) = {
        let cfg = state.config.read().await;
        (
            cfg.prompt_assistant_model_id.clone(),
            cfg.prompt_assistant_idle_timeout_secs,
            cfg.llm_external_enabled,
            cfg.llm_provider.clone(),
            cfg.llm_external_base_url.clone(),
            cfg.llm_external_api_key.clone(),
            cfg.llm_external_model.clone(),
        )
    };

    if ext_enabled {
        crate::prompt_assistant::local_llm::wake_local_server(&state.http_client, &ext_base).await;
        state.broadcast("llm:stage", serde_json::json!("generating"));
        return crate::prompt_assistant::server::chat_provider(
            &state.http_client,
            &provider,
            &ext_base,
            &ext_key,
            &ext_model,
            system,
            user,
            max_tokens,
            image,
        )
        .await
        .map_err(|e| e.to_string());
    }

    if image.is_some() {
        log::info!(
            "[prompt-assistant] the bundled local model has no vision; \
             answering from the prompt text alone"
        );
    }

    // Fall back to whatever model is already on disk when config.json carries no
    // explicit selection. This is the only path that works on read-only-config
    // deployments (e.g. a Kubernetes ConfigMap mounted at config.json), where the
    // UI's model pick can never be persisted back, leaving `prompt_assistant_model_id`
    // perpetually None despite a model sitting in the data dir.
    let model_id = match model_id {
        Some(id) => id,
        None => state
            .prompt_assistant
            .installed_models()
            .into_iter()
            .next()
            .ok_or_else(|| "prompt_assistant.no_model".to_string())?,
    };
    let hw = tokio::task::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| e.to_string())?;
    // Reclaim VRAM from idle ComfyUI workers so the LLM can load on the GPU.
    // Without this, a compose/enhance right after a generation falls back to CPU
    // (ComfyUI's model is still resident), and a 7B model on CPU overruns
    // Cloudflare's 100s proxy timeout with a 524 on the hosted deployment.
    state.free_comfyui_vram_for_llm().await;
    state.broadcast("llm:stage", serde_json::json!("loading_model"));
    let noop = |_: &str, _: u64, _: u64, _: bool| {};
    let port = state
        .prompt_assistant
        .ensure_running(
            &state.http_client,
            &model_id,
            hw.total_vram_mb,
            idle_secs,
            &noop,
        )
        .await
        .map_err(|e| e.to_string())?;
    state.broadcast("llm:stage", serde_json::json!("generating"));
    state
        .prompt_assistant
        .server
        .chat(&state.http_client, port, system, user, max_tokens)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(any(feature = "desktop", feature = "server"))]
pub async fn run_prompt_assistant_headless(
    state: &Arc<AppState>,
    input: &str,
    family: &str,
    mode: crate::prompt_assistant::grounding::GenMode,
    length: Option<&str>,
    include_artists: bool,
) -> Result<String, String> {
    use crate::prompt_assistant::grounding;
    // No active-generation guard here: `prompt_queue` is shared across every user
    // of a server/browser-mode instance, so blocking on it meant one person
    // generating locked Enhance/Compose for everyone. Contention is instead handled
    // by the free-VRAM check in `ensure_running`, which loads the LLM on CPU when a
    // GPU is already busy with ComfyUI's model rather than evicting it.
    //
    // Resolve the model purpose (drives tag-only grounding) the same way the
    // desktop path does: an external endpoint is a general-purpose chat model, so
    // it needs no local catalog entry and no installed model.
    let (model_id, ext_enabled) = {
        let cfg = state.config.read().await;
        (
            cfg.prompt_assistant_model_id.clone(),
            cfg.llm_external_enabled,
        )
    };
    // A purpose-built tag upsampler is always tag-only regardless of family.
    let purpose = if ext_enabled {
        "natural_language".to_string()
    } else {
        let resolved = match model_id {
            Some(id) => Some(id),
            None => state.prompt_assistant.installed_models().into_iter().next(),
        };
        resolved
            .and_then(|id| crate::prompt_assistant::catalog::entry(&id))
            .map(|e| e.purpose)
            .unwrap_or_else(|| "natural_language".to_string())
    };
    let tag_only = grounding::is_tag_only(&purpose, family);
    let candidates = grounding::retrieve_candidates(input, 40);
    let system = grounding::system_prompt(tag_only, mode, &candidates, include_artists);
    // Mirror the desktop token budget so browser Enhance/Compose honors the
    // user's length pick instead of always generating at the medium default.
    let max_tokens = match length {
        Some("short") => 96,
        Some("detailed") => 384,
        _ => 192,
    };
    let raw = chat_any_headless(state, &system, input, max_tokens, None).await?;
    let cleaned = grounding::repair(&raw, tag_only);
    // Enhance is additive: keep every user tag and don't let the model swap a
    // pinned attribute. No-op for Compose. The desktop path runs this too;
    // omitting it here made browser Enhance silently drop user tags.
    Ok(grounding::reconcile_enhance(input, &cleaned, mode))
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
    if let Err(e) = state.auth.check_login_allowed(&req.username) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }
    match state.auth.login(&req.username, &req.password) {
        Ok((token, must_change)) => {
            state.auth.clear_login_attempts(&req.username);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "token": token,
                    "must_change_password": must_change,
                })),
            )
                .into_response()
        }
        Err(e) => {
            state.auth.record_failed_login(&req.username);
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
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
    let mut payload = serde_json::json!({
        "auth_required": state.lan_enabled,
        "has_accounts": state.auth.has_accounts(),
        "role": role_str,
        "lan_enabled": state.lan_enabled,
        "server_mode": !cfg!(feature = "desktop"),
        "can_use_modelhub": can_use_modelhub,
        "legacy_password_deadline": state.auth.legacy_password_deadline(),
    });

    if let Some(token) = extract_token(&headers) {
        if let Some(username) = state.auth.validate_token(&token) {
            let uses_legacy = state.auth.account_uses_legacy_password(&username);
            payload["uses_legacy_password"] = serde_json::json!(uses_legacy);
            payload["legacy_password_expired"] =
                serde_json::json!(uses_legacy && state.auth.is_legacy_password_grace_expired());
        }
    }

    Json(payload)
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

/// POST /internal-api/_auth/upgrade_password_encryption — migrate a legacy SHA-256
/// hash to Argon2id using the current password (password text unchanged).
/// Accepts `{ password }` when authenticated, or `{ username, password }` on the login gate.
async fn auth_upgrade_password_encryption_handler(
    AxumState(state): AxumState<SharedState>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let (username, password, authenticated) = if let Some(token) = extract_token(&headers) {
        match state.auth.validate_token(&token) {
            Some(username) => {
                let password = match req.get("password").and_then(|v| v.as_str()) {
                    Some(p) => p.to_string(),
                    None => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({ "error": "Missing password" })),
                        )
                            .into_response();
                    }
                };
                (username, password, true)
            }
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({ "error": "Not authenticated" })),
                )
                    .into_response();
            }
        }
    } else {
        let username = match req.get("username").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Missing username" })),
                )
                    .into_response();
            }
        };
        let password = match req.get("password").and_then(|v| v.as_str()) {
            Some(p) => p.to_string(),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Missing password" })),
                )
                    .into_response();
            }
        };
        (username, password, false)
    };

    if let Err(e) = state.auth.check_login_allowed(&username) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    match state.auth.upgrade_password_encryption(&username, &password) {
        Ok(true) => {
            if authenticated {
                return (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response();
            }
            match state.auth.login(&username, &password) {
                Ok((token, must_change)) => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "ok": true,
                        "token": token,
                        "must_change_password": must_change,
                    })),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "ok": true,
                        "login_error": e,
                    })),
                )
                    .into_response(),
            }
        }
        Ok(false) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Password already uses the modern encryption format"
            })),
        )
            .into_response(),
        Err(e) => {
            state.auth.record_failed_login(&username);
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response()
        }
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
    if role == UserRole::Moderator {
        if let Some(target_role) = state.auth.get_account_role(username) {
            if target_role == "admin" || target_role == "moderator" {
                return forbidden_response("Moderators can only manage regular user accounts.");
            }
        }
        if new_role == "admin" || new_role == "moderator" {
            return forbidden_response("Only admins can grant elevated roles.");
        }
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
pub(crate) fn user_gallery_dir(username: Option<&str>) -> Option<std::path::PathBuf> {
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
    output_template: Option<&str>,
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
        Some("image_edit") => "image_edit",
        _ => "unknown",
    };

    fn sanitize_component(value: &str) -> String {
        value
            .trim()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .replace("..", "_")
            .trim_matches('_')
            .to_string()
    }

    fn parse_index(base: &str) -> String {
        let mut digits = String::new();
        for ch in base.chars().rev() {
            if ch.is_ascii_digit() {
                digits.insert(0, ch);
            } else {
                break;
            }
        }
        if digits.is_empty() {
            "0".to_string()
        } else {
            digits
        }
    }

    fn token_value(
        key: &str,
        prompt_id: &str,
        mode: &str,
        base: &str,
        metadata: Option<&std::collections::HashMap<String, String>>,
    ) -> String {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match key {
            "prompt_id" => prompt_id.to_string(),
            "mode" => mode.to_string(),
            "index" => parse_index(base),
            "date" => now_secs.to_string(),
            "time" => now_secs.to_string(),
            "model" => metadata
                .and_then(|m| {
                    m.get("checkpoint")
                        .or_else(|| m.get("model"))
                        .or_else(|| m.get("model_name"))
                })
                .cloned()
                .unwrap_or_else(|| "unknown-model".to_string()),
            "seed" => metadata
                .and_then(|m| m.get("seed"))
                .cloned()
                .unwrap_or_else(|| "0".to_string()),
            _ => String::new(),
        }
    }

    let base = std::path::Path::new(&safe_filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let mut rendered_base = String::new();
    if let Some(tpl) = output_template.map(str::trim).filter(|s| !s.is_empty()) {
        rendered_base = tpl.to_string();
        for key in [
            "prompt_id",
            "mode",
            "index",
            "date",
            "time",
            "model",
            "seed",
        ] {
            let token = format!("{{{}}}", key);
            let value = sanitize_component(&token_value(
                key,
                &safe_prompt_id,
                normalized_mode,
                base,
                metadata,
            ));
            rendered_base = rendered_base.replace(&token, &value);
        }
        rendered_base = sanitize_component(&rendered_base);
    }
    if rendered_base.is_empty() {
        rendered_base = sanitize_component(&format!(
            "{}__{}__{}",
            safe_prompt_id, normalized_mode, base
        ));
    }
    let detected_format = crate::metadata::detect_format(bytes);
    let ext = match detected_format {
        crate::metadata::ImageFormat::Jxl => "jxl",
        crate::metadata::ImageFormat::WebP => "webp",
        _ => "png",
    };
    let gallery_filename = format!("{}.{}", rendered_base, ext);
    let path = dir.join(&gallery_filename);

    let raw_mode = metadata_mode.unwrap_or("text_chunk");
    let mut embed_mode = crate::metadata::MetadataMode::from_str(raw_mode);

    if matches!(detected_format, crate::metadata::ImageFormat::Png)
        && embed_mode == crate::metadata::MetadataMode::StealthAlpha
    {
        if let Ok(true) = crate::metadata::is_png_16bit(bytes) {
            embed_mode = crate::metadata::MetadataMode::Both;
        }
    }

    let final_bytes = if let Some(meta) = metadata {
        match detected_format {
            crate::metadata::ImageFormat::Png => {
                crate::metadata::embed_png_metadata(bytes, meta, embed_mode)
                    .unwrap_or_else(|_| bytes.to_vec())
            }
            crate::metadata::ImageFormat::Jxl => {
                crate::metadata::embed_jxl_metadata(bytes, meta).unwrap_or_else(|_| bytes.to_vec())
            }
            crate::metadata::ImageFormat::WebP => {
                crate::metadata::embed_webp_metadata(bytes, meta, embed_mode)
                    .unwrap_or_else(|_| bytes.to_vec())
            }
            crate::metadata::ImageFormat::Mp4 => bytes.to_vec(),
            crate::metadata::ImageFormat::Avif => bytes.to_vec(),
            crate::metadata::ImageFormat::Gif => bytes.to_vec(),
            crate::metadata::ImageFormat::Unknown => bytes.to_vec(),
        }
    } else {
        bytes.to_vec()
    };

    std::fs::write(&path, &final_bytes).map_err(|e| e.to_string())?;
    crate::gallery_index::upsert(&path, final_bytes.len() as u64, detected_format, metadata);
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

/// The username key used for localhost / single-admin sessions, where
/// `resolve_username` returns `None`. Matches the convention documented in
/// `user_prefs.rs`.
const ADMIN_PREFS_KEY: &str = "_admin";

/// GET /internal-api/_user/prefs — fetch the current user's saved preferences.
/// Returns 204 when the user has no saved prefs yet.
async fn user_prefs_get_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if resolve_role(&state, &headers, &remote) == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }
    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| ADMIN_PREFS_KEY.to_string());
    match crate::user_prefs::load(&username).await {
        Some(prefs) => (StatusCode::OK, Json(prefs)).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// PUT /internal-api/_user/prefs — replace the current user's saved preferences.
/// The `updated_at` timestamp is set server-side, ignoring any client value.
async fn user_prefs_put_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(mut prefs): Json<crate::user_prefs::UserPrefs>,
) -> Response {
    if resolve_role(&state, &headers, &remote) == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }
    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| ADMIN_PREFS_KEY.to_string());
    prefs.updated_at = Some(chrono::Utc::now().to_rfc3339());
    match crate::user_prefs::save(&username, &prefs).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
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
        let never_expire = state.app.config.read().await.gallery_never_expire;
        let expiry = if never_expire {
            0 // 0 means never expires, same convention as the unlimited-storage case above
        } else {
            crate::auth::DEFAULT_EXPIRY_SECS
        };
        (state.auth.get_storage_limit(name), expiry)
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
                if !is_gallery_image_filename(&name) {
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

    // Moderators may only manage regular user accounts, consistent with the
    // delete / reset-password / set-role / modelhub handlers.
    if role == UserRole::Moderator {
        if let Some(target_role) = state.auth.get_account_role(username) {
            if target_role == "admin" || target_role == "moderator" {
                return forbidden_response("Moderators can only manage regular user accounts.");
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
            if !is_gallery_image_filename(&name) {
                continue;
            }
            if let Ok(meta) = file_entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = now.duration_since(modified) {
                        if age > expiry {
                            let size = meta.len();
                            let path = file_entry.path();
                            if std::fs::remove_file(&path).is_ok() {
                                expired_count += 1;
                                expired_bytes += size;
                                crate::gallery_index::remove(&path);
                                // Videos own a poster sidecar that listings
                                // never surface; expire it with its mp4.
                                if let Some(stem) = name.strip_suffix(".mp4") {
                                    let poster = user_dir.join(format!("{stem}_poster.webp"));
                                    if poster.is_file() {
                                        let _ = std::fs::remove_file(&poster);
                                        crate::gallery_index::remove(&poster);
                                    }
                                }
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

// ---------------------------------------------------------------------------
// Model request handlers
// ---------------------------------------------------------------------------

/// GET /internal-api/_model_requests — list all model requests.
/// Query params: ?status=pending|approved|denied (optional filter).
async fn model_requests_list_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let status_filter = params.get("status").and_then(|s| match s.as_str() {
        "pending" => Some(crate::model_requests::RequestStatus::Pending),
        "approved" => Some(crate::model_requests::RequestStatus::Approved),
        "denied" => Some(crate::model_requests::RequestStatus::Denied),
        _ => None,
    });

    let requests = state.app.model_requests.get_requests(status_filter);
    (
        StatusCode::OK,
        Json(serde_json::json!({ "requests": requests })),
    )
        .into_response()
}

/// POST /internal-api/_model_requests/add — submit a new model request.
/// Body: { model_id, model_name, model_type, model_url, file_name, file_url, file_size_kb, category }
async fn model_requests_add_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    let model_id = match req.get("model_id").and_then(|v| v.as_u64()) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing model_id" })),
            )
                .into_response();
        }
    };
    let model_name = req
        .get("model_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let model_type = req
        .get("model_type")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let model_url = req
        .get("model_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let file_name = req
        .get("file_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let file_url = req
        .get("file_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let file_size_kb = req
        .get("file_size_kb")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let category = req
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("checkpoints")
        .to_string();

    if file_name.is_empty() || file_url.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Missing file_name or file_url" })),
        )
            .into_response();
    }

    let request = state.app.model_requests.add_request(
        &username,
        model_id,
        &model_name,
        &model_type,
        &model_url,
        &file_name,
        &file_url,
        file_size_kb,
        &category,
    );

    // Notify mods/admins about the new request. A "global" notification would
    // leak the request (and requester's username) to every user, so target each
    // staff account individually. The localhost super-admin reads notifications
    // under the literal "admin" key, so include it explicitly.
    let mut recipients = state.auth.usernames_with_roles(&["admin", "moderator"]);
    if !recipients.iter().any(|u| u.eq_ignore_ascii_case("admin")) {
        recipients.push("admin".to_string());
    }
    for recipient in recipients {
        let _ = state.app.notifications.create_i18n(
            &recipient,
            "notifications.model_request.new_title",
            Some("notifications.model_request.new_body"),
            Some(serde_json::json!({
                "username": username,
                "model_name": model_name,
                "model_type": model_type,
            })),
            "info",
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "request": request })),
    )
        .into_response()
}

/// POST /internal-api/_model_requests/approve — approve a request (mod/admin).
/// Body: { request_id }
async fn model_requests_approve_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can approve requests.");
    }

    let handler_username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    let request_id = match req.get("request_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing request_id" })),
            )
                .into_response();
        }
    };

    match state
        .app
        .model_requests
        .approve_request(request_id, &handler_username)
    {
        Ok(request) => {
            // Notify the requester
            let _ = state.app.notifications.create_i18n(
                &request.username,
                "notifications.model_request.approved_title",
                Some("notifications.model_request.approved_body"),
                Some(serde_json::json!({
                    "model_name": request.model_name,
                    "handler": handler_username,
                })),
                "success",
            );

            (
                StatusCode::OK,
                Json(serde_json::json!({ "request": request })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /internal-api/_model_requests/deny — deny a request (mod/admin).
/// Body: { request_id, reason? }
async fn model_requests_deny_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role != UserRole::Admin && role != UserRole::Moderator {
        return forbidden_response("Only admins and moderators can deny requests.");
    }

    let handler_username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    let request_id = match req.get("request_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing request_id" })),
            )
                .into_response();
        }
    };
    let reason = req.get("reason").and_then(|v| v.as_str());

    match state
        .app
        .model_requests
        .deny_request(request_id, &handler_username, reason)
    {
        Ok(request) => {
            // Notify the requester with the reason
            let (body_key, params) = match &request.deny_reason {
                Some(r) => (
                    "notifications.model_request.denied_body_reason",
                    serde_json::json!({
                        "model_name": request.model_name,
                        "handler": handler_username,
                        "reason": r,
                    }),
                ),
                None => (
                    "notifications.model_request.denied_body",
                    serde_json::json!({
                        "model_name": request.model_name,
                        "handler": handler_username,
                    }),
                ),
            };
            let _ = state.app.notifications.create_i18n(
                &request.username,
                "notifications.model_request.denied_title",
                Some(body_key),
                Some(params),
                "warning",
            );

            (
                StatusCode::OK,
                Json(serde_json::json!({ "request": request })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Notification handlers
// ---------------------------------------------------------------------------

/// GET /internal-api/_notifications — list notifications for the current user.
async fn notifications_list_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    let notifications = state.app.notifications.get_for_user(&username);
    (
        StatusCode::OK,
        Json(serde_json::json!({ "notifications": notifications })),
    )
        .into_response()
}

/// GET /internal-api/_notifications/unread_count — get unread notification count.
async fn notifications_unread_count_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    let count = state.app.notifications.unread_count(&username);
    (
        StatusCode::OK,
        Json(serde_json::json!({ "unread_count": count })),
    )
        .into_response()
}

/// POST /internal-api/_notifications/mark_read — mark a notification as read.
/// Body: { notification_id }
async fn notifications_mark_read_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let notification_id = match req.get("notification_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing notification_id" })),
            )
                .into_response();
        }
    };

    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    match state
        .app
        .notifications
        .mark_read(&username, notification_id)
    {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /internal-api/_notifications/mark_all_read — mark all notifications as read.
async fn notifications_mark_all_read_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    state.app.notifications.mark_all_read(&username);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

/// POST /internal-api/_notifications/dismiss — dismiss a notification for this user.
/// Body: { notification_id }
async fn notifications_dismiss_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    let notification_id = match req.get("notification_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Missing notification_id" })),
            )
                .into_response();
        }
    };

    match state.app.notifications.dismiss(&username, notification_id) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /internal-api/_notifications/clear — dismiss all notifications for this user.
async fn notifications_clear_handler(
    AxumState(state): AxumState<SharedState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let role = resolve_role(&state, &headers, &remote);
    if role == UserRole::Anonymous {
        return forbidden_response("Authentication required.");
    }

    let username =
        resolve_username(&state, &headers, &remote).unwrap_or_else(|| "admin".to_string());

    state.app.notifications.clear_all(&username);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}
