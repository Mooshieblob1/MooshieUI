//! OAuth sign-in for external LLM providers.
//!
//! Two providers are supported, for the same underlying reason: both let an
//! unaffiliated desktop app start a flow without being provisioned by hand.
//!
//! * **OpenRouter** has a user-centric PKCE flow with no client registration
//!   and no client id at all.
//! * **Nous Portal** is standards-track OAuth 2.1 -- RFC 8414 metadata, RFC
//!   7591 dynamic client registration, authorization code + PKCE S256 -- so
//!   this install registers *itself* as a public client the first time the user
//!   signs in and keeps the client id it is issued.
//!
//! * **xAI** runs the RFC 8628 device authorization grant at `auth.x.ai`, which
//!   is how a SuperGrok subscription can pay for inference on `api.x.ai/v1`
//!   instead of prepaid API credits. It publishes OIDC discovery with PKCE
//!   S256, refresh and device-code grants and public clients
//!   (`token_endpoint_auth_method: "none"`), but no dynamic registration:
//!   `/oauth2/register` is a 404 and an unknown `client_id` is refused with
//!   `invalid_client`. Client ids are allowlisted and issued by xAI by hand, so
//!   there is nothing to apply for and every third-party Grok client instead
//!   signs in with the public client id of xAI's own CLI, disclosing that the
//!   consent screen therefore names xAI's app; see [`XAI_DEFAULT_CLIENT_ID`].
//!   MooshieUI does the same, and `llm_xai_client_id` overrides it for an
//!   install that has been issued one of its own.
//!
//! Anthropic and OpenAI are API-key only here, and so is the plain `xai`
//! provider. Grok also stays reachable with no xAI credential at all through
//! OpenRouter's `x-ai/*` model ids.
//!
//! The OpenRouter and Portal redirect target is an RFC 8252 loopback listener
//! bound to `127.0.0.1`, not a custom URI scheme: it needs no scheme
//! registration, no extra Tauri plugin, and no cross-command state. That also
//! means those two flows only work where the browser and the app run on the
//! same machine, which is why browser mode never offers them. The device grant
//! has no redirect at all, so it is the one flow that survives browser mode.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::AppError;

const AUTH_URL: &str = "https://openrouter.ai/auth";
const EXCHANGE_URL: &str = "https://openrouter.ai/api/v1/auth/keys";

/// How long to wait for the user to finish signing in before giving up.
/// OpenRouter's authorization codes expire after 10 minutes anyway.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Run the OpenRouter PKCE flow and return the issued API key.
///
/// Opens the system browser, waits for the loopback redirect, then exchanges
/// the one-time code for a key the user controls and can revoke.
pub async fn connect_openrouter(client: &reqwest::Client) -> Result<String, AppError> {
    let verifier = random_b64url(32);
    let challenge = s256_challenge(&verifier);
    // A random callback path stands in for an OAuth `state` parameter: a code
    // delivered to any other path is not ours. Appending it to the path rather
    // than the query keeps it clear of the `?code=` the provider adds.
    let nonce = random_b64url(12);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| AppError::LlmError(format!("Could not open a sign-in callback port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::LlmError(format!("Could not read the callback port: {e}")))?
        .port();
    let callback_path = format!("/cb/{nonce}");
    let callback_url = format!("http://localhost:{port}{callback_path}");

    let auth_url = url::Url::parse_with_params(
        AUTH_URL,
        &[
            ("callback_url", callback_url.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ],
    )
    .map_err(|e| AppError::LlmError(format!("Could not build the sign-in URL: {e}")))?;

    open::that(auth_url.as_str())
        .map_err(|e| AppError::LlmError(format!("Could not open the browser: {e}")))?;

    let code = tokio::time::timeout(
        CALLBACK_TIMEOUT,
        await_code(listener, &callback_path, "OpenRouter", None),
    )
    .await
    .map_err(|_| AppError::LlmError("Sign-in timed out after 5 minutes.".into()))??;

    exchange_code(client, &code, &verifier).await
}

/// An authorization server's refusal, carrying the machine-readable `error`
/// code so callers can branch on it. `invalid_scope` in particular is worth
/// retrying with a narrower request rather than surfacing to the user.
#[derive(Debug)]
pub struct AuthError {
    pub code: String,
    pub description: Option<String>,
}

impl From<AuthError> for AppError {
    fn from(e: AuthError) -> Self {
        let detail = match &e.description {
            Some(d) => format!("{}: {d}", e.code),
            None => e.code.clone(),
        };
        AppError::LlmError(format!("Sign-in failed: {detail}"))
    }
}

/// Accept loopback connections until the provider redirects to our callback
/// path. Anything else (a favicon probe, a stray request) gets a 404 and the
/// listener keeps waiting.
///
/// `expected_state` is checked when the server echoes an OAuth `state`
/// parameter. The random callback path already scopes the listener to one
/// flow, so `state` is belt-and-braces for servers that mandate it.
async fn await_code(
    listener: TcpListener,
    callback_path: &str,
    provider_label: &str,
    expected_state: Option<&str>,
) -> Result<String, AuthError> {
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|e| AuthError {
            code: "callback_failed".into(),
            description: Some(e.to_string()),
        })?;

        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        // "GET /cb/abc?code=... HTTP/1.1"
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("");
        let parsed = url::Url::parse(&format!("http://localhost{target}")).ok();
        let (path, params) = match parsed {
            Some(u) => {
                let params: HashMap<String, String> = u.query_pairs().into_owned().collect();
                (u.path().to_string(), params)
            }
            None => (String::new(), HashMap::new()),
        };

        if path != callback_path {
            respond(&mut stream, "404 Not Found", "text/plain", "Not found").await;
            continue;
        }
        // A mismatched `state` means this redirect belongs to some other flow,
        // so the code in it is not ours to redeem.
        if let (Some(expected), Some(got)) = (expected_state, params.get("state")) {
            if expected != got {
                respond(&mut stream, "400 Bad Request", "text/plain", "Bad state").await;
                return Err(AuthError {
                    code: "state_mismatch".into(),
                    description: Some("the redirect did not match this sign-in attempt".into()),
                });
            }
        }
        if let Some(code) = params.get("code") {
            let page = done_page(provider_label);
            respond(&mut stream, "200 OK", "text/html; charset=utf-8", &page).await;
            return Ok(code.clone());
        }
        let code = params
            .get("error")
            .cloned()
            .unwrap_or_else(|| "no_authorization_code".to_string());
        respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            FAILED_PAGE,
        )
        .await;
        return Err(AuthError {
            code,
            description: params.get("error_description").cloned(),
        });
    }
}

/// Trade the one-time code for an API key. The verifier never left this
/// process, so a code obtained by anyone else cannot be redeemed here.
async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    verifier: &str,
) -> Result<String, AppError> {
    let resp = client
        .post(EXCHANGE_URL)
        .json(&serde_json::json!({
            "code": code,
            "code_verifier": verifier,
            "code_challenge_method": "S256",
        }))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| AppError::LlmError(format!("Key exchange request failed: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail: String = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        return Err(AppError::LlmError(format!(
            "Key exchange returned {status}: {detail}"
        )));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::LlmError(format!("Bad key exchange response: {e}")))?;
    let key = v["key"].as_str().unwrap_or("").to_string();
    if key.is_empty() {
        return Err(AppError::LlmError(
            "Key exchange returned no key. Try again, or paste an API key instead.".into(),
        ));
    }
    Ok(key)
}

async fn respond(stream: &mut tokio::net::TcpStream, status: &str, content_type: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// `n` random bytes, base64url-encoded without padding. 32 bytes yields the
/// 43-character verifier RFC 7636 asks for.
fn random_b64url(n: usize) -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..n).map(|_| rng.random::<u8>()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE S256 challenge: base64url-no-pad of the SHA-256 of the verifier.
fn s256_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// The tab the user is left looking at once the redirect lands. `provider` is
/// one of our own literals, never anything the remote server sent, so it needs
/// no escaping.
fn done_page(provider: &str) -> String {
    format!(
        "<!doctype html><meta charset=utf-8><title>Signed in</title>\
<body style=\"font-family:system-ui;background:#171717;color:#e5e5e5;display:flex;\
align-items:center;justify-content:center;height:100vh;margin:0\">\
<div style=\"text-align:center\"><h1 style=\"font-weight:500\">Signed in to {provider}</h1>\
<p>You can close this tab and go back to MooshieUI.</p></div>"
    )
}

const FAILED_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Sign-in failed</title>\
<body style=\"font-family:system-ui;background:#171717;color:#e5e5e5;display:flex;\
align-items:center;justify-content:center;height:100vh;margin:0\">\
<div style=\"text-align:center\"><h1 style=\"font-weight:500\">Sign-in failed</h1>\
<p>Close this tab and try again, or paste an API key in MooshieUI instead.</p></div>";

// ---------------------------------------------------------------------------
// Nous Portal
// ---------------------------------------------------------------------------

/// Nous Portal's OAuth issuer. Sign-in and token minting live here; inference
/// itself is a different host (`inference-api.nousresearch.com`), which is why
/// the provider registry carries the base URL separately.
const NOUS_ISSUER: &str = "https://portal.nousresearch.com";

/// What the first authorization attempt asks for.
///
/// Portal's metadata advertises only `mcp:manage_agents`, but its inference API
/// is scoped `inference:invoke`, and a refresh token needs `offline_access`.
/// Asking for the union is the only way to end up with a token that can
/// actually run a completion; a server that dislikes the list says so with
/// `invalid_scope`, which [`connect_nous`] handles by retrying with just the
/// advertised set rather than surfacing an error the user cannot act on.
const NOUS_PREFERRED_SCOPES: &[&str] = &["offline_access", "inference:invoke", "mcp:manage_agents"];

/// A signed-in session for a provider that issues *expiring* tokens.
///
/// Unlike OpenRouter, whose flow ends in a durable API key, Portal hands back a
/// short-lived access token plus the means to mint the next one. All four
/// fields have to be persisted together: the refresh token is useless without
/// the client id it was issued to, and the expiry is what tells us when to
/// spend it.
#[derive(Debug, Clone)]
pub struct OauthSession {
    pub access_token: String,
    pub refresh_token: String,
    pub client_id: String,
    /// Unix seconds, or `0` when the server did not say the token expires.
    pub expires_at: i64,
}

/// The parts of an RFC 8414 authorization server metadata document we act on.
#[derive(Debug, Clone, serde::Deserialize)]
struct ServerMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    /// RFC 8628. Present on servers that offer the device grant, absent on the
    /// ones that only do redirect flows.
    device_authorization_endpoint: Option<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

impl ServerMetadata {
    /// The endpoints as they stood when this was written. Discovery is the
    /// source of truth, but a Portal outage on the well-known path should not
    /// take sign-in down with it when the endpoints have not actually moved.
    fn nous_fallback() -> Self {
        Self {
            authorization_endpoint: format!("{NOUS_ISSUER}/oauth/authorize"),
            token_endpoint: format!("{NOUS_ISSUER}/api/oauth/token"),
            registration_endpoint: Some(format!("{NOUS_ISSUER}/api/oauth/register")),
            device_authorization_endpoint: None,
            scopes_supported: vec!["mcp:manage_agents".to_string()],
        }
    }

    /// The same idea for xAI, whose discovery document lives on the OIDC path.
    fn xai_fallback() -> Self {
        Self {
            authorization_endpoint: format!("{XAI_ISSUER}/oauth2/authorize"),
            token_endpoint: format!("{XAI_ISSUER}/oauth2/token"),
            registration_endpoint: None,
            device_authorization_endpoint: Some(format!("{XAI_ISSUER}/oauth2/device/code")),
            scopes_supported: Vec::new(),
        }
    }
}

/// Run the Nous Portal sign-in and return the session it produces.
///
/// Discovery, then RFC 7591 dynamic client registration, then authorization
/// code + PKCE S256 on a loopback redirect. Registration happens on every
/// sign-in rather than once per install: the redirect URI has to name the port
/// the listener actually got, and that port is different every time.
pub async fn connect_nous(client: &reqwest::Client) -> Result<OauthSession, AppError> {
    let meta = match discover(client, NOUS_ISSUER).await {
        Ok(m) => m,
        Err(e) => {
            log::warn!("Nous Portal metadata discovery failed ({e}); using the built-in endpoints");
            ServerMetadata::nous_fallback()
        }
    };

    let preferred: Vec<String> = NOUS_PREFERRED_SCOPES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    match nous_attempt(client, &meta, &preferred).await {
        Err(e) if e.code == "invalid_scope" && meta.scopes_supported != preferred => {
            // The server told us which scopes it will grant, so take it at its
            // word and go round once more. Inference may still be refused
            // later, but a refused completion is a far clearer failure than a
            // sign-in that never gets past the consent screen.
            log::warn!(
                "Nous Portal rejected the requested scopes; retrying with {:?}",
                meta.scopes_supported
            );
            let fallback = meta.scopes_supported.clone();
            Ok(nous_attempt(client, &meta, &fallback).await?)
        }
        other => Ok(other?),
    }
}

/// One full browser round trip: register, authorize, redeem.
async fn nous_attempt(
    client: &reqwest::Client,
    meta: &ServerMetadata,
    scopes: &[String],
) -> Result<OauthSession, AuthError> {
    let verifier = random_b64url(32);
    let challenge = s256_challenge(&verifier);
    let state = random_b64url(16);
    let nonce = random_b64url(12);

    let bind_failed = |e: std::io::Error| AuthError {
        code: "callback_bind_failed".into(),
        description: Some(e.to_string()),
    };
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(bind_failed)?;
    let port = listener.local_addr().map_err(bind_failed)?.port();
    let callback_path = format!("/cb/{nonce}");
    // RFC 8252 asks for the literal loopback address rather than `localhost`,
    // which can resolve to an address the listener is not bound to.
    let redirect_uri = format!("http://127.0.0.1:{port}{callback_path}");
    let scope = scopes.join(" ");

    let client_id = register_client(client, meta, &redirect_uri, &scope)
        .await
        .map_err(|e| AuthError {
            code: "registration_failed".into(),
            description: Some(e.to_string()),
        })?;

    let mut params = vec![
        ("response_type", "code"),
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
    ];
    if !scope.is_empty() {
        params.push(("scope", scope.as_str()));
    }
    let auth_url =
        url::Url::parse_with_params(&meta.authorization_endpoint, &params).map_err(|e| {
            AuthError {
                code: "bad_authorization_endpoint".into(),
                description: Some(e.to_string()),
            }
        })?;

    open::that(auth_url.as_str()).map_err(|e| AuthError {
        code: "browser_launch_failed".into(),
        description: Some(e.to_string()),
    })?;

    let code = tokio::time::timeout(
        CALLBACK_TIMEOUT,
        await_code(listener, &callback_path, "Nous Research", Some(&state)),
    )
    .await
    .map_err(|_| AuthError {
        code: "timeout".into(),
        description: Some("sign-in was not completed within 5 minutes".into()),
    })??;

    let form = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("code".to_string(), code),
        ("redirect_uri".to_string(), redirect_uri),
        ("client_id".to_string(), client_id.clone()),
        ("code_verifier".to_string(), verifier),
    ];
    redeem(client, &meta.token_endpoint, &client_id, form, "")
        .await
        .map_err(|e| AuthError {
            code: "token_exchange_failed".into(),
            description: Some(e.to_string()),
        })
}

/// Mint a fresh access token from a stored refresh token.
///
/// Re-runs discovery rather than persisting a token endpoint, so an endpoint
/// that moves between releases fixes itself; the cost is one small GET against
/// a URL that is almost certainly cached. `refresh_token` is carried forward
/// when the response omits a new one, because a server that returns no
/// `refresh_token` means "keep using the one you have", not "you no longer have
/// one".
pub async fn refresh_nous(
    client: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> Result<OauthSession, AppError> {
    if client_id.is_empty() {
        return Err(AppError::LlmError(
            "No OAuth client id is stored for this session. Sign in again.".into(),
        ));
    }
    let meta = match discover(client, NOUS_ISSUER).await {
        Ok(m) => m,
        Err(_) => ServerMetadata::nous_fallback(),
    };
    let form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token.to_string()),
        ("client_id".to_string(), client_id.to_string()),
    ];
    redeem(client, &meta.token_endpoint, client_id, form, refresh_token).await
}

/// POST a token request and read the session out of the response.
async fn redeem(
    client: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    form: Vec<(String, String)>,
    previous_refresh: &str,
) -> Result<OauthSession, AppError> {
    let resp = client
        .post(token_endpoint)
        .form(&form)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| AppError::LlmError(format!("Token request failed: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail: String = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        return Err(AppError::LlmError(format!(
            "Token request returned {status}: {detail}"
        )));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::LlmError(format!("Bad token response: {e}")))?;

    session_from(&v, client_id, previous_refresh)
}

/// Read a session out of a successful token response.
///
/// `previous_refresh` is what to keep when the server rotates nothing back to
/// us: dropping it would strand the session at the next refresh.
fn session_from(
    v: &serde_json::Value,
    client_id: &str,
    previous_refresh: &str,
) -> Result<OauthSession, AppError> {
    let access_token = v["access_token"].as_str().unwrap_or_default().to_string();
    if access_token.is_empty() {
        return Err(AppError::LlmError(
            "The provider returned no access token. Try signing in again.".into(),
        ));
    }
    let refresh_token = v["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(previous_refresh)
        .to_string();

    Ok(OauthSession {
        access_token,
        refresh_token,
        client_id: client_id.to_string(),
        expires_at: expires_at_from(v["expires_in"].as_i64(), chrono::Utc::now().timestamp()),
    })
}

/// Turn an `expires_in` lifetime into the absolute deadline we persist. A
/// missing lifetime means the token does not expire, which the config encodes
/// as `0`; a non-positive one is nonsense and gets the same treatment rather
/// than producing a deadline that is already in the past.
fn expires_at_from(expires_in: Option<i64>, now: i64) -> i64 {
    match expires_in {
        Some(secs) if secs > 0 => now + secs,
        _ => 0,
    }
}

/// Fetch RFC 8414 authorization server metadata for an issuer.
async fn discover(client: &reqwest::Client, issuer: &str) -> Result<ServerMetadata, AppError> {
    // Portal answers on the RFC 8414 path and xAI on the OIDC one, and neither
    // serves the other, so try both before deciding discovery is down.
    let mut last = AppError::LlmError("Discovery was not attempted".into());
    for path in [
        ".well-known/oauth-authorization-server",
        ".well-known/openid-configuration",
    ] {
        match discover_at(client, &format!("{issuer}/{path}")).await {
            Ok(meta) => return Ok(meta),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Fetch and parse one metadata document.
async fn discover_at(client: &reqwest::Client, url: &str) -> Result<ServerMetadata, AppError> {
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| AppError::LlmError(format!("Discovery request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::LlmError(format!(
            "Discovery returned {}",
            resp.status()
        )));
    }
    resp.json::<ServerMetadata>()
        .await
        .map_err(|e| AppError::LlmError(format!("Bad discovery document: {e}")))
}

/// Register this install as a public OAuth client (RFC 7591).
///
/// Portal accepts `token_endpoint_auth_method: "none"`, which is what makes any
/// of this possible: there is no client secret to ship, so MooshieUI never has
/// to embed a credential in the binary or borrow another application's.
async fn register_client(
    client: &reqwest::Client,
    meta: &ServerMetadata,
    redirect_uri: &str,
    scope: &str,
) -> Result<String, AppError> {
    let endpoint = meta.registration_endpoint.as_deref().ok_or_else(|| {
        AppError::LlmError(
            "This provider does not support automatic sign-in. Paste an API key instead.".into(),
        )
    })?;

    let mut body = serde_json::json!({
        "client_name": "MooshieUI",
        "client_uri": "https://github.com/Mooshieblob1/MooshieUI",
        "application_type": "native",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    if !scope.is_empty() {
        body["scope"] = serde_json::Value::String(scope.to_string());
    }

    let resp = client
        .post(endpoint)
        .json(&body)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| AppError::LlmError(format!("Client registration failed: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail: String = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        return Err(AppError::LlmError(format!(
            "Client registration returned {status}: {detail}"
        )));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::LlmError(format!("Bad registration response: {e}")))?;
    let client_id = v["client_id"].as_str().unwrap_or_default().to_string();
    if client_id.is_empty() {
        return Err(AppError::LlmError(
            "Client registration returned no client id.".into(),
        ));
    }
    Ok(client_id)
}

// ---------------------------------------------------------------------------
// xAI
// ---------------------------------------------------------------------------

/// xAI's authorization server. Inference is a different host (`api.x.ai`),
/// which the provider registry carries separately as the base URL.
const XAI_ISSUER: &str = "https://auth.x.ai";

/// The public client id third-party Grok clients sign in with.
///
/// xAI runs no registration endpoint and issues client ids by hand, so there is
/// no application to file; the tools in this space (Hermes Agent, OpenClaw, Pi)
/// all present this same id, which belongs to xAI's own CLI, and each documents
/// that the consent screen names xAI's app rather than theirs. It is a public
/// client id and not a secret. `llm_xai_client_id` overrides it.
///
/// Which accounts may actually mint a token is decided per account and not per
/// client, so a client id of one's own would not widen it: xAI gates that on the
/// subscription and refuses some accounts outright, which is what [`xai_hint`]
/// explains when a 403 comes back.
pub const XAI_DEFAULT_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";

/// What sign-in asks for when the operator has not overridden the scope.
///
/// `openid profile email` identify the account, `offline_access` is what makes
/// a refresh token possible, and `api:access` is what lets the token call the
/// inference API. `grok-cli:access` goes with the shared client id above -- it
/// is the grant that client exists to issue, and this is the set known to work
/// for a SuperGrok account. The conversation and workspace scopes xAI also
/// advertises are left out: nothing here reads them, and every extra scope is
/// another line on the user's consent screen.
pub const XAI_DEFAULT_SCOPE: &str =
    "openid profile email offline_access grok-cli:access api:access";

/// Backstop for a device response that names no lifetime of its own.
const XAI_DEVICE_TIMEOUT: Duration = Duration::from_secs(600);

/// An RFC 8628 device authorization response, trimmed to what the flow uses.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DeviceAuth {
    pub device_code: String,
    /// The short code the user types on the verification page.
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub interval: Option<u64>,
}

impl DeviceAuth {
    /// Where to send the user: the pre-filled URL when the server offers one,
    /// otherwise the bare page where they enter the code by hand.
    pub fn best_uri(&self) -> &str {
        self.verification_uri_complete
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.verification_uri)
    }
}

/// Sign in to xAI with the device authorization grant.
///
/// `client_id` is the operator override; empty means [`XAI_DEFAULT_CLIENT_ID`],
/// which is the ordinary case. `on_prompt` fires once, as soon as the code and
/// before the browser is touched, so the caller can put the code in front of
/// the user: on the desktop it belongs beside the page that opens, and in
/// browser mode it is the only way the code is ever seen, because the browser
/// opened here is the server's rather than theirs.
pub async fn connect_xai(
    client: &reqwest::Client,
    client_id: &str,
    scope: &str,
    on_prompt: impl Fn(&DeviceAuth),
) -> Result<OauthSession, AppError> {
    let client_id = match client_id.trim() {
        "" => XAI_DEFAULT_CLIENT_ID,
        s => s,
    };
    let scope = match scope.trim() {
        "" => XAI_DEFAULT_SCOPE,
        s => s,
    };
    let meta = discover(client, XAI_ISSUER)
        .await
        .unwrap_or_else(|_| ServerMetadata::xai_fallback());
    let device_endpoint = meta
        .device_authorization_endpoint
        .clone()
        .unwrap_or_else(|| format!("{XAI_ISSUER}/oauth2/device/code"));

    let resp = client
        .post(&device_endpoint)
        .form(&[("client_id", client_id), ("scope", scope)])
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| AppError::LlmError(format!("Device authorization request failed: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail: String = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        return Err(AppError::LlmError(format!(
            "Device authorization returned {status}: {detail}{}",
            xai_hint(status.as_u16())
        )));
    }
    let auth: DeviceAuth = resp
        .json()
        .await
        .map_err(|e| AppError::LlmError(format!("Bad device authorization response: {e}")))?;

    on_prompt(&auth);
    // A browser that will not open is not fatal here: the caller has already
    // shown the URL and the code, so the user can open it themselves. Only the
    // desktop build opens one at all -- in the headless server binary the
    // browser here would be the host's rather than the person signing in.
    #[cfg(feature = "desktop")]
    let _ = open::that(auth.best_uri());

    poll_device_token(client, &meta.token_endpoint, client_id, &auth).await
}

/// Poll the token endpoint until the user approves, denies, or the code dies.
async fn poll_device_token(
    client: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    auth: &DeviceAuth,
) -> Result<OauthSession, AppError> {
    let mut interval = Duration::from_secs(auth.interval.unwrap_or(5).clamp(1, 60));
    let lifetime = auth
        .expires_in
        .filter(|s| *s > 0)
        .map(|s| Duration::from_secs(s as u64))
        .unwrap_or(XAI_DEVICE_TIMEOUT);
    let deadline = tokio::time::Instant::now() + lifetime;

    loop {
        tokio::time::sleep(interval).await;
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::LlmError(
                "The sign-in code expired before it was approved. Try again.".into(),
            ));
        }
        let resp = client
            .post(token_endpoint)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", auth.device_code.as_str()),
                ("client_id", client_id),
            ])
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| AppError::LlmError(format!("Token request failed: {e}")))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body: serde_json::Value =
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);

        if status.is_success() {
            return session_from(&body, client_id, "");
        }
        match body["error"].as_str().unwrap_or_default() {
            // The user has not finished yet, so keep the same cadence.
            "authorization_pending" => {}
            // RFC 8628 wants the slowdown to be permanent, so this never resets.
            "slow_down" => interval += Duration::from_secs(5),
            "expired_token" => {
                return Err(AppError::LlmError(
                    "The sign-in code expired before it was approved. Try again.".into(),
                ))
            }
            "access_denied" => {
                return Err(AppError::LlmError(
                    "Sign-in was denied on the xAI page.".into(),
                ))
            }
            _ => {
                let detail = body["error_description"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| text.chars().take(300).collect());
                return Err(AppError::LlmError(format!(
                    "Token request returned {status}: {detail}{}",
                    xai_hint(status.as_u16())
                )));
            }
        }
    }
}

/// Mint a fresh xAI access token from a stored refresh token.
pub async fn refresh_xai(
    client: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> Result<OauthSession, AppError> {
    if client_id.is_empty() {
        return Err(AppError::LlmError(
            "No OAuth client id is stored for this session. Sign in again.".into(),
        ));
    }
    let meta = discover(client, XAI_ISSUER)
        .await
        .unwrap_or_else(|_| ServerMetadata::xai_fallback());
    let form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token.to_string()),
        ("client_id".to_string(), client_id.to_string()),
    ];
    redeem(client, &meta.token_endpoint, client_id, form, refresh_token).await
}

/// Turn xAI's two characteristic rejections into something actionable.
///
/// A 403 here does not mean the credentials are wrong: xAI allowlists accounts
/// on this surface and has refused subscribers who are otherwise in good
/// standing, so the useful advice is to fall back rather than to retry.
fn xai_hint(status: u16) -> &'static str {
    match status {
        403 => {
            " -- xAI restricts which accounts may use OAuth on this endpoint. If this keeps \
             happening, use an xAI API key, or reach Grok through OpenRouter instead."
        }
        400 | 401 => {
            " -- check the xAI OAuth client id in the provider settings; xAI rejects client ids \
             it has not issued."
        }
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_is_the_rfc_length_and_charset() {
        let v = random_b64url(32);
        assert_eq!(v.len(), 43);
        assert!(
            v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "verifier must be unreserved characters only: {v}"
        );
    }

    #[test]
    fn verifiers_are_not_reused() {
        assert_ne!(random_b64url(32), random_b64url(32));
    }

    #[test]
    fn s256_challenge_matches_the_rfc_7636_appendix_b_vector() {
        // RFC 7636 Appendix B: the canonical verifier/challenge pair.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            s256_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn expires_at_is_absolute_and_zero_when_unbounded() {
        assert_eq!(expires_at_from(Some(3600), 1_000), 4_600);
        // No lifetime, or a nonsensical one, means "does not expire" rather
        // than a deadline in the past that would refresh on every request.
        assert_eq!(expires_at_from(None, 1_000), 0);
        assert_eq!(expires_at_from(Some(0), 1_000), 0);
        assert_eq!(expires_at_from(Some(-5), 1_000), 0);
    }

    #[test]
    fn metadata_parses_and_tolerates_a_missing_scope_list() {
        let m: ServerMetadata = serde_json::from_str(
            r#"{"issuer":"https://portal.nousresearch.com",
                "authorization_endpoint":"https://portal.nousresearch.com/oauth/authorize",
                "token_endpoint":"https://portal.nousresearch.com/api/oauth/token",
                "registration_endpoint":"https://portal.nousresearch.com/api/oauth/register"}"#,
        )
        .expect("discovery documents carry extra fields we must ignore");
        assert_eq!(
            m.token_endpoint,
            "https://portal.nousresearch.com/api/oauth/token"
        );
        assert!(m.scopes_supported.is_empty());
    }

    #[test]
    fn the_fallback_names_a_registration_endpoint() {
        // Without one, `register_client` has nothing to POST to and sign-in
        // degrades to "paste an API key", so the fallback must carry it.
        let m = ServerMetadata::nous_fallback();
        assert!(m.registration_endpoint.is_some());
        assert!(m.authorization_endpoint.starts_with(NOUS_ISSUER));
    }
}
