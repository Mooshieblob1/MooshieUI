//! OAuth sign-in for external LLM providers.
//!
//! Only OpenRouter is supported. Its PKCE flow is user-centric: there is no
//! client registration and no client id, so the app can start it without being
//! provisioned by the provider. Anthropic, OpenAI and xAI have no equivalent
//! public flow for third-party desktop apps, so they take an API key instead.
//!
//! The redirect target is an RFC 8252 loopback listener bound to `127.0.0.1`,
//! not a custom URI scheme: it needs no scheme registration, no extra Tauri
//! plugin, and no cross-command state. That also means the flow only works
//! where the browser and the app run on the same machine, which is why browser
//! mode never offers it.

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

    let code = tokio::time::timeout(CALLBACK_TIMEOUT, await_code(listener, &callback_path))
        .await
        .map_err(|_| AppError::LlmError("Sign-in timed out after 5 minutes.".into()))??;

    exchange_code(client, &code, &verifier).await
}

/// Accept loopback connections until the provider redirects to our callback
/// path. Anything else (a favicon probe, a stray request) gets a 404 and the
/// listener keeps waiting.
async fn await_code(listener: TcpListener, callback_path: &str) -> Result<String, AppError> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|e| AppError::LlmError(format!("Sign-in callback failed: {e}")))?;

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
        if let Some(code) = params.get("code") {
            respond(&mut stream, "200 OK", "text/html; charset=utf-8", DONE_PAGE).await;
            return Ok(code.clone());
        }
        let error = params
            .get("error")
            .cloned()
            .unwrap_or_else(|| "no authorization code returned".to_string());
        respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            FAILED_PAGE,
        )
        .await;
        return Err(AppError::LlmError(format!(
            "OpenRouter sign-in failed: {error}"
        )));
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

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Signed in</title>\
<body style=\"font-family:system-ui;background:#171717;color:#e5e5e5;display:flex;\
align-items:center;justify-content:center;height:100vh;margin:0\">\
<div style=\"text-align:center\"><h1 style=\"font-weight:500\">Signed in to OpenRouter</h1>\
<p>You can close this tab and go back to MooshieUI.</p></div>";

const FAILED_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Sign-in failed</title>\
<body style=\"font-family:system-ui;background:#171717;color:#e5e5e5;display:flex;\
align-items:center;justify-content:center;height:100vh;margin:0\">\
<div style=\"text-align:center\"><h1 style=\"font-weight:500\">Sign-in failed</h1>\
<p>Close this tab and try again, or paste an API key in MooshieUI instead.</p></div>";

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
}
