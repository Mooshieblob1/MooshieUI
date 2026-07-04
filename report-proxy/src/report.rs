use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::json;

use crate::types::{AppState, ReportPayload};
use crate::{catalog, dedup, github, llm};

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

    // 5. Best-effort LLM summary (never blocks issue creation).
    let entry = catalog::lookup(&payload.error_code);
    let prompt = llm::build_prompt(
        entry,
        &payload.error_code,
        &payload.raw_message,
        payload.logs_tail.as_deref(),
    );
    let summary = llm::summarize(&state.http, &state.cfg, &prompt).await;

    // 6. Create the issue.
    let title = github::issue_title(&payload.error_code, &payload.raw_message);
    let issue_body = github::issue_body(&payload, &sig, summary.as_deref());
    match state
        .github
        .create_issue(&title, &issue_body, &["bug", "in-app-report"])
        .await
    {
        Ok(url) => (StatusCode::OK, Json(json!({ "issueUrl": url }))),
        Err(e) => {
            tracing::error!("issue creation failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "failed to create issue" })),
            )
        }
    }
}
