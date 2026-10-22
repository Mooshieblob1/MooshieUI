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
//! Cloud-hosted clients (Claude's custom connectors, ChatGPT) connect from
//! their own servers, so they need a public HTTPS address: the built-in
//! Cloudflare quick tunnel (`tunnel.rs`) or the owner's own tunnel or reverse
//! proxy (`public_url`). Clients that can't send headers put the token in the
//! path instead: `POST /mcp/<token>`.
//!
//! Its settings live in `mcp.json` next to `config.json` rather than in
//! `AppConfig`: the Settings page saves the whole config object at once, and a
//! stale copy of it must not switch the server off or roll the token back.

pub mod jobs;
pub mod music;
pub mod setups;
pub mod tools;
pub mod tunnel;

use std::sync::{Arc, LazyLock, Mutex};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State as AxumState};
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

/// Longest a tool holds a request that came through a tunnel or proxy.
/// Cloudflare drops a request with no response after 100 seconds, so a
/// generation that takes longer comes back as a job id to poll instead.
const PROXIED_MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(75);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct McpSettings {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
    /// Run the built-in Cloudflare quick tunnel.
    pub tunnel: bool,
    /// The owner's own public address for the server (a named tunnel or a
    /// reverse proxy), e.g. `https://mooshie.example.com`. Its host passes
    /// the `Host`/`Origin` checks.
    pub public_url: String,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            token: String::new(),
            tunnel: false,
            public_url: String::new(),
        }
    }
}

/// What a settings change asks for. `None` keeps the current value.
#[derive(Debug, Default)]
pub struct SettingsUpdate {
    pub enabled: bool,
    pub port: u16,
    pub tunnel: Option<bool>,
    pub public_url: Option<String>,
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
    /// `url` with the token in the path, for clients that take only a URL.
    pub local_connector_url: String,
    pub tunnel: bool,
    pub tunnel_status: tunnel::TunnelStatus,
    /// The tunnel's address with the token in the path, while it runs.
    pub tunnel_connector_url: Option<String>,
    pub public_url: String,
    /// `public_url` with the token in the path.
    pub public_connector_url: Option<String>,
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

/// `base` (scheme and host, maybe a path prefix) plus `/mcp/<token>`.
fn connector_url(base: &str, token: &str) -> String {
    format!("{}/mcp/{token}", base.trim_end_matches('/'))
}

/// Check and tidy an owner-entered public address. It must be HTTPS (cloud
/// clients require it, and the token travels in it) and carry no query or
/// fragment. A trailing `/mcp` is dropped, since the server adds it.
pub fn normalize_public_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let url = reqwest::Url::parse(trimmed)
        .map_err(|_| "Enter the public address as a full URL, like https://mooshie.example.com")?;
    if url.scheme() != "https" {
        return Err("The public address must start with https://".into());
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("The public address needs a host name".into());
    }
    if url.query().is_some() || url.fragment().is_some() || !url.username().is_empty() {
        return Err("Leave out any ?query, #fragment or user name from the public address".into());
    }
    let mut out = url.as_str().trim_end_matches('/').to_string();
    if let Some(stripped) = out.strip_suffix("/mcp") {
        out = stripped.to_string();
    }
    Ok(out)
}

/// The host (and port, if not the default) of the owner's public address,
/// lowercased, as it appears in a `Host` header.
fn public_host(public_url: &str) -> Option<String> {
    let url = reqwest::Url::parse(public_url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// Whether the request's `Host` header is the owner's public address.
fn host_is_public(headers: &HeaderMap, public_url: &str) -> bool {
    let (Some(expected), Some(host)) = (
        public_host(public_url),
        headers
            .get(axum::http::header::HOST)
            .and_then(|v| v.to_str().ok()),
    ) else {
        return false;
    };
    let host = host.trim().to_ascii_lowercase();
    host == expected || host.strip_suffix(":443") == Some(expected.as_str())
}

/// Whether `origin` is the owner's public address.
fn origin_is_public(origin: &str, public_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(public_url) else {
        return false;
    };
    origin
        .trim_end_matches('/')
        .eq_ignore_ascii_case(&url.origin().ascii_serialization())
}

pub async fn status() -> McpStatus {
    let settings = current_settings();
    let running = RUNNING.lock().await.as_ref().map(|r| r.port);
    let port = running.unwrap_or(settings.port);
    let tunnel_status = tunnel::status();
    let tunnel_connector_url = (tunnel_status.state == tunnel::TunnelState::Running)
        .then(|| {
            tunnel_status
                .url
                .as_deref()
                .map(|u| connector_url(u, &settings.token))
        })
        .flatten();
    McpStatus {
        enabled: settings.enabled,
        running: running.is_some(),
        port,
        url: url_for(port),
        local_connector_url: connector_url(&format!("http://127.0.0.1:{port}"), &settings.token),
        error: if settings.enabled {
            LAST_ERROR.lock().unwrap().clone()
        } else {
            None
        },
        setups: setups::count(),
        tunnel: settings.tunnel,
        tunnel_status,
        tunnel_connector_url,
        public_connector_url: (!settings.public_url.is_empty())
            .then(|| connector_url(&settings.public_url, &settings.token)),
        public_url: settings.public_url,
        token: settings.token,
    }
}

/// Turn the server or the tunnel on or off, move the server to another port,
/// or change the public address.
pub async fn set_settings(
    state: Arc<AppState>,
    update: SettingsUpdate,
) -> Result<McpStatus, String> {
    if update.port < 1024 {
        return Err("Pick a port from 1024 to 65535.".into());
    }
    let public_url = update
        .public_url
        .as_deref()
        .map(normalize_public_url)
        .transpose()?;
    {
        let mut settings = SETTINGS.lock().unwrap();
        settings.enabled = update.enabled;
        settings.port = update.port;
        if let Some(tunnel) = update.tunnel {
            settings.tunnel = tunnel;
        }
        if let Some(public_url) = public_url {
            settings.public_url = public_url;
        }
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
        drop(running);
        sync_tunnel(&state, &settings, wanted).await;
        return;
    }
    if let Some(old) = running.take() {
        let _ = old.shutdown.send(());
        log::info!("MCP server on port {} stopped", old.port);
    }
    *LAST_ERROR.lock().unwrap() = None;
    let Some(port) = wanted else {
        drop(running);
        sync_tunnel(&state, &settings, None).await;
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
            drop(running);
            sync_tunnel(&state, &settings, None).await;
            return;
        }
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let app = router(state.clone());
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
    drop(running);
    sync_tunnel(&state, &settings, Some(port)).await;
}

/// Run the tunnel only while the server listens and the owner wants it.
async fn sync_tunnel(state: &AppState, settings: &McpSettings, listening: Option<u16>) {
    let port = listening.filter(|_| settings.tunnel);
    tunnel::sync(port, state.http_client.clone()).await;
}

/// Stop the tunnel on app exit.
pub async fn shutdown() {
    tunnel::shutdown().await;
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/mcp",
            post(handle_post)
                .get(method_not_allowed)
                .delete(method_not_allowed),
        )
        .route(
            "/mcp/{token}",
            post(handle_post_with_path_token)
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
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|given| token_equals(given.trim(), expected))
}

/// Constant-time token comparison; an empty expected token never matches.
fn token_equals(given: &str, expected: &str) -> bool {
    !expected.is_empty() && bool::from(given.as_bytes().ct_eq(expected.as_bytes()))
}

/// Whether a request came through the tunnel or a reverse proxy rather than
/// straight from an app on this machine.
fn is_proxied(headers: &HeaderMap, public_url: &str) -> bool {
    headers.contains_key("cf-ray")
        || headers.contains_key("x-forwarded-for")
        || host_is_public(headers, public_url)
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

async fn handle_post(
    AxumState(state): AxumState<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let settings = current_settings();
    let authorized = token_matches(&headers, &settings.token);
    serve(&state, &settings, &headers, authorized, body).await
}

/// `POST /mcp/<token>`, for clients that can only be given a URL. A bearer
/// header, if also sent, is ignored.
async fn handle_post_with_path_token(
    AxumState(state): AxumState<Arc<AppState>>,
    AxumPath(token): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let settings = current_settings();
    let authorized = token_equals(&token, &settings.token);
    serve(&state, &settings, &headers, authorized, body).await
}

async fn serve(
    state: &Arc<AppState>,
    settings: &McpSettings,
    headers: &HeaderMap,
    authorized: bool,
    body: Bytes,
) -> Response {
    let public = &settings.public_url;
    if !crate::webserver::host_header_is_local(headers) && !host_is_public(headers, public) {
        return (StatusCode::FORBIDDEN, "Host not allowed").into_response();
    }
    if let Some(origin) = headers.get(axum::http::header::ORIGIN) {
        let ok = origin
            .to_str()
            .is_ok_and(|o| origin_is_local(o) || origin_is_public(o, public));
        if !ok {
            return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
        }
    }
    if !authorized {
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

    let max_wait = if is_proxied(headers, public) {
        PROXIED_MAX_WAIT
    } else {
        tools::MAX_WAIT
    };
    let reply = match dispatch(state, method, &params, max_wait).await {
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
    max_wait: std::time::Duration,
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
            tools::call(state, name, &args, max_wait)
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
    fn path_tokens_compare_exactly() {
        assert!(token_equals("abc123", "abc123"));
        assert!(!token_equals("abc12", "abc123"));
        assert!(!token_equals("ABC123", "abc123"));
        assert!(!token_equals("", ""));
    }

    #[test]
    fn public_addresses_are_checked_and_tidied() {
        for (input, want) in [
            ("", ""),
            (
                "  https://Mooshie.Example.com/  ",
                "https://mooshie.example.com",
            ),
            (
                "https://mooshie.example.com/mcp",
                "https://mooshie.example.com",
            ),
            (
                "https://example.com:8443/ai/mcp/",
                "https://example.com:8443/ai",
            ),
        ] {
            assert_eq!(normalize_public_url(input).unwrap(), want, "{input}");
        }
        for bad in [
            "http://mooshie.example.com",
            "mooshie.example.com",
            "https://mooshie.example.com/?x=1",
            "https://user@mooshie.example.com",
            "ftp://mooshie.example.com",
        ] {
            assert!(normalize_public_url(bad).is_err(), "{bad}");
        }
        assert_eq!(
            connector_url("https://a.example/", "tok"),
            "https://a.example/mcp/tok"
        );
    }

    #[test]
    fn only_the_configured_public_host_passes() {
        let public = "https://mooshie.example.com";
        let host = |h: &str| headers(&[("host", h)]);
        assert!(host_is_public(&host("mooshie.example.com"), public));
        assert!(host_is_public(&host("MOOSHIE.example.com:443"), public));
        assert!(!host_is_public(
            &host("mooshie.example.com.evil.example"),
            public
        ));
        assert!(!host_is_public(&host("evil.example"), public));
        assert!(!host_is_public(&host("mooshie.example.com"), ""));
        assert!(!host_is_public(&HeaderMap::new(), public));
        let ported = "https://example.com:8443/ai";
        assert!(host_is_public(&host("example.com:8443"), ported));
        assert!(!host_is_public(&host("example.com"), ported));

        assert!(origin_is_public("https://mooshie.example.com", public));
        assert!(origin_is_public("https://mooshie.example.com/", public));
        assert!(!origin_is_public("http://mooshie.example.com", public));
        assert!(!origin_is_public("https://evil.example", public));
        assert!(!origin_is_public("https://mooshie.example.com", ""));
    }

    #[test]
    fn tunnelled_and_proxied_requests_are_told_apart() {
        let public = "https://mooshie.example.com";
        assert!(!is_proxied(&headers(&[("host", "127.0.0.1:3210")]), public));
        assert!(is_proxied(
            &headers(&[("host", "127.0.0.1:3210"), ("cf-ray", "8a1b")]),
            public
        ));
        assert!(is_proxied(
            &headers(&[("x-forwarded-for", "203.0.113.5")]),
            ""
        ));
        assert!(is_proxied(
            &headers(&[("host", "mooshie.example.com")]),
            public
        ));
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
            tools::MAX_WAIT,
        )
        .await
        .unwrap();
        assert_eq!(init["protocolVersion"], "2025-03-26");
        let init = dispatch(
            &state,
            "initialize",
            &json!({ "protocolVersion": "1999-01-01" }),
            tools::MAX_WAIT,
        )
        .await
        .unwrap();
        assert_eq!(init["protocolVersion"], PROTOCOL_VERSIONS[0]);
        assert!(dispatch(&state, "nope", &json!({}), tools::MAX_WAIT)
            .await
            .is_err());
        let listed = dispatch(&state, "tools/list", &json!({}), tools::MAX_WAIT)
            .await
            .unwrap();
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"generate_image") && names.contains(&"generate_music"));
    }
}
