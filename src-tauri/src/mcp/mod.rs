//! MCP (Model Context Protocol) server, so AI assistants can drive MooshieUI.
//!
//! Opt-in and local only: it listens on `127.0.0.1` on its own port, in every
//! mode (desktop window, browser mode, headless server), and every request
//! must carry the bearer token shown in Settings. Being on loopback is not
//! enough on its own, because any website the owner visits can reach
//! `127.0.0.1` too (see `webserver::is_trusted_local_request`).
//!
//! Transport is MCP's Streamable HTTP with plain JSON responses: one
//! `POST /mcp` per JSON-RPC message, no server-initiated stream (`GET` answers
//! 405, which the spec allows). Long tools simply hold the request open.
//!
//! Its settings live in `mcp.json` next to `config.json` rather than in
//! `AppConfig`: the Settings page saves the whole config object at once, and a
//! stale copy of it must not switch the server off or roll the token back.

pub mod jobs;
pub mod music;
pub mod setups;
pub mod tools;

use std::sync::{Arc, LazyLock, Mutex};

use axum::body::Bytes;
use axum::extract::State as AxumState;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;

use crate::state::AppState;

pub const DEFAULT_PORT: u16 = 3210;

/// Protocol revisions this server speaks, newest first. The handshake echoes
/// the client's revision when it is one of these, else offers the newest.
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct McpSettings {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            token: String::new(),
        }
    }
}

/// What the Settings panel shows.
#[derive(Debug, Clone, Serialize)]
pub struct McpStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub url: String,
    pub token: String,
    /// Why the server is enabled but not running (port taken, ...).
    pub error: Option<String>,
    /// Models with a remembered generation setup the server can reuse.
    pub setups: usize,
}

struct Running {
    port: u16,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

static SETTINGS: LazyLock<Mutex<McpSettings>> = LazyLock::new(|| Mutex::new(load_settings()));
static RUNNING: LazyLock<tokio::sync::Mutex<Option<Running>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn settings_path() -> Option<std::path::PathBuf> {
    crate::config::app_data_dir().map(|d| d.join("mcp.json"))
}

fn new_token() -> String {
    // Two v4 UUIDs: 244 random bits, hex only, so it pastes cleanly into a
    // header value or a shell command.
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn load_settings() -> McpSettings {
    let mut settings = settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<McpSettings>(&s).ok())
        .unwrap_or_default();
    if settings.port == 0 {
        settings.port = DEFAULT_PORT;
    }
    settings
}

fn save_settings(settings: &McpSettings) -> Result<(), String> {
    let path = settings_path().ok_or("Cannot find app data directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

fn current_settings() -> McpSettings {
    SETTINGS.lock().unwrap().clone()
}

pub fn url_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

pub async fn status() -> McpStatus {
    let settings = current_settings();
    let running = RUNNING.lock().await.as_ref().map(|r| r.port);
    McpStatus {
        enabled: settings.enabled,
        running: running.is_some(),
        port: running.unwrap_or(settings.port),
        url: url_for(running.unwrap_or(settings.port)),
        token: settings.token,
        error: if settings.enabled {
            LAST_ERROR.lock().unwrap().clone()
        } else {
            None
        },
        setups: setups::count(),
    }
}

/// Turn the server on or off, or move it to another port.
pub async fn set_settings(
    state: Arc<AppState>,
    enabled: bool,
    port: u16,
) -> Result<McpStatus, String> {
    if port < 1024 {
        return Err("Pick a port from 1024 to 65535.".into());
    }
    {
        let mut settings = SETTINGS.lock().unwrap();
        settings.enabled = enabled;
        settings.port = port;
        if settings.token.is_empty() {
            settings.token = new_token();
        }
        save_settings(&settings)?;
    }
    sync(state).await;
    Ok(status().await)
}

/// Replace the token. Clients holding the old one get 401 from now on; no
/// restart is needed because every request reads the current token.
pub async fn regenerate_token() -> Result<McpStatus, String> {
    {
        let mut settings = SETTINGS.lock().unwrap();
        settings.token = new_token();
        save_settings(&settings)?;
    }
    Ok(status().await)
}

/// Start, stop or move the listener to match the saved settings. Called at
/// startup and after every settings change.
pub async fn sync(state: Arc<AppState>) {
    let settings = current_settings();
    let mut running = RUNNING.lock().await;
    let wanted = settings.enabled.then_some(settings.port);
    if running.as_ref().map(|r| r.port) == wanted {
        return;
    }
    if let Some(old) = running.take() {
        let _ = old.shutdown.send(());
        log::info!("MCP server on port {} stopped", old.port);
    }
    *LAST_ERROR.lock().unwrap() = None;
    let Some(port) = wanted else {
        return;
    };
    if settings.token.is_empty() {
        // Enabled by hand-editing mcp.json without a token: never serve
        // without one.
        let mut s = SETTINGS.lock().unwrap();
        s.token = new_token();
        let _ = save_settings(&s);
    }
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            log::warn!("MCP server could not listen on 127.0.0.1:{port}: {e}");
            *LAST_ERROR.lock().unwrap() = Some(format!("Port {port} is not available: {e}"));
            return;
        }
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let app = router(state);
    tokio::spawn(async move {
        let served = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
        if let Err(e) = served {
            log::warn!("MCP server stopped with an error: {e}");
        }
    });
    log::info!("MCP server listening on {}", url_for(port));
    *running = Some(Running { port, shutdown: tx });
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/mcp",
            post(handle_post)
                .get(method_not_allowed)
                .delete(method_not_allowed),
        )
        .with_state(state)
}

async fn method_not_allowed() -> Response {
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

/// Whether an `Origin` header names this machine. The spec asks servers to
/// check it so a web page cannot talk to a local server (DNS rebinding).
fn origin_is_local(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    let host = if let Some(v6) = rest.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        rest.split(':').next().unwrap_or("")
    };
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

/// The request's bearer token matches `expected`. Constant-time, and an empty
/// expected token never matches.
fn token_matches(headers: &HeaderMap, expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    let Some(given) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };
    given.trim().as_bytes().ct_eq(expected.as_bytes()).into()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

async fn handle_post(
    AxumState(state): AxumState<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !crate::webserver::host_header_is_local(&headers) {
        return (StatusCode::FORBIDDEN, "Host not allowed").into_response();
    }
    if let Some(origin) = headers.get(axum::http::header::ORIGIN) {
        if !origin.to_str().is_ok_and(origin_is_local) {
            return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
        }
    }
    if !token_matches(&headers, &current_settings().token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Missing or wrong MooshieUI MCP token. Copy it from Settings > AI assistants (MCP)." })),
        )
            .into_response();
    }

    let message: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(rpc_error(Value::Null, -32700, &format!("Parse error: {e}"))),
            )
                .into_response();
        }
    };
    if message.is_array() {
        return (
            StatusCode::BAD_REQUEST,
            Json(rpc_error(
                Value::Null,
                -32600,
                "Batched requests are not supported",
            )),
        )
            .into_response();
    }
    // Notifications and responses get no reply body.
    let Some(id) = message.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let Some(method) = message.get("method").and_then(|m| m.as_str()) else {
        return StatusCode::ACCEPTED.into_response();
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    let reply = match dispatch(&state, method, &params).await {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, msg)) => rpc_error(id, code, &msg),
    };
    (StatusCode::OK, Json(reply)).into_response()
}

const INSTRUCTIONS: &str = "MooshieUI is a local image and music generator built on ComfyUI. \
generate_image reuses the owner's last txt2img setup for a model (loaders, upscale, face fix, guidance), \
so only models listed with a saved setup in get_status can be used; generating once with a model in \
MooshieUI saves its setup. Images land in the MooshieUI gallery. generate_music makes songs with YuE2 \
(list_models category music shows whether it is installed) and saves FLAC files on the MooshieUI machine. \
Check get_status first.";

async fn dispatch(
    state: &Arc<AppState>,
    method: &str,
    params: &Value,
) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == requested)
                .copied()
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "mooshieui", "title": "MooshieUI", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::definitions() })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or((-32602, "Missing tool name".to_string()))?;
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            tools::call(state, name, &args)
                .await
                .ok_or((-32602, format!("Unknown tool: {name}")))
        }
        _ => Err((-32601, format!("Method not found: {method}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    #[test]
    fn only_local_origins_pass() {
        for ok in [
            "http://localhost:3210",
            "http://127.0.0.1",
            "https://LOCALHOST",
            "http://[::1]:3210",
        ] {
            assert!(origin_is_local(ok), "{ok}");
        }
        for bad in [
            "https://evil.example",
            "http://localhost.evil.example",
            "null",
            "file://",
            "http://192.168.1.5:3210",
        ] {
            assert!(!origin_is_local(bad), "{bad}");
        }
    }

    #[test]
    fn token_must_match_exactly() {
        let good = headers(&[("authorization", "Bearer abc123")]);
        assert!(token_matches(&good, "abc123"));
        assert!(!token_matches(&good, "abc1234"));
        assert!(!token_matches(&good, ""));
        assert!(!token_matches(
            &headers(&[("authorization", "abc123")]),
            "abc123"
        ));
        assert!(!token_matches(&HeaderMap::new(), "abc123"));
    }

    #[test]
    fn new_tokens_are_long_hex() {
        let t = new_token();
        assert_eq!(t.len(), 64);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(t, new_token());
    }

    #[tokio::test]
    async fn handshake_negotiates_a_known_version() {
        let state = Arc::new(AppState::new(crate::config::AppConfig::default()));
        let init = dispatch(
            &state,
            "initialize",
            &json!({ "protocolVersion": "2025-03-26" }),
        )
        .await
        .unwrap();
        assert_eq!(init["protocolVersion"], "2025-03-26");
        let init = dispatch(
            &state,
            "initialize",
            &json!({ "protocolVersion": "1999-01-01" }),
        )
        .await
        .unwrap();
        assert_eq!(init["protocolVersion"], PROTOCOL_VERSIONS[0]);
        assert!(dispatch(&state, "nope", &json!({})).await.is_err());
        let listed = dispatch(&state, "tools/list", &json!({})).await.unwrap();
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"generate_image") && names.contains(&"generate_music"));
    }
}
