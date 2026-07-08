use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::json;

use crate::types::{AppState, ReportPayload};
use crate::{dedup, github};

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

/// Seconds since the Unix epoch (used only for rate-limit windows).
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Max characters per log comment; GitHub caps comment bodies near 65,536.
const MAX_LOG_COMMENT_CHARS: usize = 60_000;

/// Split a string into chunks of at most `max` characters (char-safe).
fn chunk_chars(s: &str, max: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return vec![s.to_string()];
    }
    chars.chunks(max).map(|c| c.iter().collect()).collect()
}

/// Attach the full diagnostics log to an issue as collapsible comment(s).
///
/// The issue body already shows a tail inline, so this only runs when the log is
/// larger than that. It is best-effort: attaching the log must never fail the
/// report, so errors are logged and swallowed. A 4-backtick fence keeps stray
/// triple-backticks inside the log from breaking out of the code block.
async fn attach_full_log(github: &github::GithubClient, number: u64, log: &str) {
    let log = log.trim();
    if number == 0 || log.chars().count() <= github::MAX_LOG_IN_ISSUE {
        return;
    }
    let chunks = chunk_chars(log, MAX_LOG_COMMENT_CHARS);
    let total = chunks.len();
    for (i, chunk) in chunks.into_iter().enumerate() {
        let header = if total > 1 {
            format!("Full diagnostics log (part {}/{})", i + 1, total)
        } else {
            "Full diagnostics log".to_string()
        };
        let body =
            format!("<details><summary>{header}</summary>\n\n````log\n{chunk}\n````\n\n</details>");
        if let Err(e) = github.comment_on(number, &body).await {
            tracing::warn!("failed to attach log comment {}/{total}: {e}", i + 1);
            break;
        }
    }
}

pub async fn report_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    // 1. App header gate.
    if headers.get("x-mooshie-app").and_then(|v| v.to_str().ok()) != Some("1") {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden" })));
    }

    // 2. Rate limit by Cloudflare-provided client IP.
    let ip = client_ip(&headers);
    if !state.limiter.check(&ip, now_secs()) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "rate limited" })),
        );
    }

    // 3. Parse and minimally validate the payload.
    let payload: ReportPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid payload: {e}") })),
            );
        }
    };
    if payload.error_code.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "errorCode is required" })),
        );
    }

    // 4. Dedup: comment on an existing open issue if we have seen this signature.
    let sig = dedup::signature(&payload.error_code, &payload.raw_message);
    match state.github.find_open_by_sig(&sig).await {
        Ok(Some(existing)) => {
            let note = format!(
                "Seen again from another user. App version `{}`, OS `{}`, arch `{}`.",
                payload.app_version, payload.os, payload.arch
            );
            let _ = state.github.comment_on(existing.number, &note).await;
            if let Some(log) = payload.logs_tail.as_deref() {
                attach_full_log(&state.github, existing.number, log).await;
            }
            return (
                StatusCode::OK,
                Json(json!({ "issueUrl": existing.html_url })),
            );
        }
        Ok(None) => {}
        Err(e) => {
            // Non-fatal: fall through and create a fresh issue.
            tracing::warn!("dedup lookup failed: {e}");
        }
    }

    // 5. Create the issue: logs + system info + the user's message, verbatim.
    let title = github::issue_title(&payload.error_code, &payload.raw_message);
    let issue_body = github::issue_body(&payload, &sig);
    match state
        .github
        .create_issue(&title, &issue_body, &["bug", "in-app-report"])
        .await
    {
        Ok((number, url)) => {
            if let Some(log) = payload.logs_tail.as_deref() {
                attach_full_log(&state.github, number, log).await;
            }
            (StatusCode::OK, Json(json!({ "issueUrl": url })))
        }
        Err(e) => {
            tracing::error!("issue creation failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "failed to create issue" })),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_log_is_a_single_chunk() {
        assert_eq!(chunk_chars("hello", 60_000), vec!["hello".to_string()]);
    }

    #[test]
    fn long_log_splits_into_bounded_chunks() {
        let log = "x".repeat(130_000);
        let chunks = chunk_chars(&log, 60_000);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.chars().count() <= 60_000));
        assert_eq!(chunks.concat().chars().count(), 130_000);
    }
}
