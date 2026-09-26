use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::json;

use crate::types::{AppState, ReportPayload};
use crate::{dedup, github, ratelimit};

fn client_ip(headers: &HeaderMap) -> String {
    ratelimit::rate_limit_key(
        headers
            .get("cf-connecting-ip")
            .and_then(|v| v.to_str().ok()),
    )
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

/// Most comments a single report may add to GitHub, including the "seen again"
/// note on a duplicate. Together with the log cap below this bounds what one
/// request can make the maintainer's token do.
const MAX_COMMENTS_PER_REPORT: usize = 2;

/// Longest diagnostics log kept from a report (the tail, since the newest lines
/// matter most): exactly what fits in `MAX_COMMENTS_PER_REPORT` log comments.
pub const MAX_LOG_TAIL_CHARS: usize = MAX_COMMENTS_PER_REPORT * MAX_LOG_COMMENT_CHARS;

/// Room reserved at the top of a truncated log for the truncation notice.
const TRUNCATION_NOTE_RESERVE: usize = 160;

/// Keep only the last `max_chars` characters of a log, starting with a clear
/// notice when anything was dropped. The result never exceeds `max_chars`.
fn cap_log_tail(log: &str, max_chars: usize) -> String {
    let total = log.chars().count();
    if total <= max_chars {
        return log.to_string();
    }
    let keep = max_chars.saturating_sub(TRUNCATION_NOTE_RESERVE);
    let dropped = total - keep;
    let tail: String = log.chars().skip(dropped).collect();
    format!(
        "[report-proxy: log truncated, the first {dropped} characters were dropped; \
         showing the last {keep}]\n{tail}"
    )
}

/// Split a string into chunks of at most `max` characters (char-safe).
fn chunk_chars(s: &str, max: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return vec![s.to_string()];
    }
    chars.chunks(max).map(|c| c.iter().collect()).collect()
}

/// Comment bodies to post for one report, never more than
/// `MAX_COMMENTS_PER_REPORT`.
///
/// The issue body already shows the log inline, so the full log is attached as
/// collapsible comment(s) only when it is longer than that. `note` (the "seen
/// again" line for a duplicate) is folded into the first log comment rather
/// than costing a comment of its own.
fn comment_bodies(note: Option<&str>, log: Option<&str>) -> Vec<String> {
    let mut bodies: Vec<String> = Vec::new();
    if let Some(log) = log
        .map(str::trim)
        .filter(|l| l.chars().count() > github::MAX_LOG_IN_ISSUE)
    {
        let chunks = chunk_chars(log, MAX_LOG_COMMENT_CHARS);
        let total = chunks.len().min(MAX_COMMENTS_PER_REPORT);
        for (i, chunk) in chunks.iter().take(total).enumerate() {
            let header = if total > 1 {
                format!("Full diagnostics log (part {}/{})", i + 1, total)
            } else {
                "Full diagnostics log".to_string()
            };
            bodies.push(format!(
                "<details><summary>{header}</summary>\n\n{}\n\n</details>",
                github::fenced(chunk, "log")
            ));
        }
    }
    if let Some(note) = note {
        match bodies.first_mut() {
            Some(first) => *first = format!("{note}\n\n{first}"),
            None => bodies.push(note.to_string()),
        }
    }
    bodies
}

/// Post comment bodies in order. When `first_reserved`, the caller already took
/// the first write from the global budget; every other comment takes its own
/// and is skipped once the budget runs out. Best-effort: failures are logged,
/// never surfaced, since the report itself already succeeded.
async fn post_comments(state: &AppState, number: u64, bodies: Vec<String>, first_reserved: bool) {
    if number == 0 {
        return;
    }
    let total = bodies.len();
    for (i, body) in bodies.into_iter().enumerate() {
        if !(i == 0 && first_reserved) && !state.budget.try_take(now_secs()) {
            tracing::warn!(
                "global write budget exhausted; skipped comment {}/{total}",
                i + 1
            );
            break;
        }
        if let Err(e) = state.github.comment_on(number, &body).await {
            tracing::warn!("failed to post comment {}/{total}: {e}", i + 1);
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
    let mut payload: ReportPayload = match serde_json::from_slice(&body) {
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
    // Keep only the newest part of the log; this is all that gets posted.
    payload.logs_tail = payload
        .logs_tail
        .map(|l| cap_log_tail(&l, MAX_LOG_TAIL_CHARS));

    // 4. Global GitHub write budget, shared by every client. Taken before the
    //    dedup lookup so reads are bounded by it too. This covers the issue (or
    //    the first comment on a duplicate); further comments take their own.
    if !state.budget.try_take(now_secs()) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "report service busy, try again later" })),
        );
    }

    // 5. Dedup: comment on an existing open issue if we have seen this signature.
    let sig = dedup::signature(&payload.error_code, &payload.raw_message);
    match state.github.find_open_by_sig(&sig).await {
        Ok(Some(existing)) => {
            let note = format!(
                "Seen again from another user. App version {}, OS {}, arch {}.",
                github::inline_code(&payload.app_version),
                github::inline_code(&payload.os),
                github::inline_code(&payload.arch)
            );
            let bodies = comment_bodies(Some(&note), payload.logs_tail.as_deref());
            post_comments(&state, existing.number, bodies, true).await;
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

    // 6. Create the issue: logs + system info + the user's message, fenced.
    let title = github::issue_title(&payload.error_code, &payload.raw_message);
    let issue_body = github::issue_body(&payload, &sig);
    match state
        .github
        .create_issue(&title, &issue_body, &["bug", "in-app-report"])
        .await
    {
        Ok((number, url)) => {
            let bodies = comment_bodies(None, payload.logs_tail.as_deref());
            post_comments(&state, number, bodies, false).await;
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

    #[test]
    fn short_logs_are_not_capped() {
        assert_eq!(cap_log_tail("abc", 10), "abc");
    }

    #[test]
    fn huge_log_keeps_a_marked_tail_within_the_cap() {
        let log = format!("{}END", "a".repeat(4 * 1024 * 1024));
        let capped = cap_log_tail(&log, MAX_LOG_TAIL_CHARS);
        assert!(capped.chars().count() <= MAX_LOG_TAIL_CHARS);
        assert!(capped.starts_with("[report-proxy: log truncated"));
        assert!(capped.ends_with("END"), "the newest lines are kept");
    }

    #[test]
    fn a_maximal_report_yields_at_most_the_comment_cap() {
        // A 4 MB log used to become ~70 comments; now it is capped first.
        let log = cap_log_tail(&"x".repeat(4 * 1024 * 1024), MAX_LOG_TAIL_CHARS);
        let bodies = comment_bodies(None, Some(&log));
        assert_eq!(bodies.len(), MAX_COMMENTS_PER_REPORT);
        assert!(bodies.iter().all(|b| b.chars().count() < 65_536));
        // Even an uncapped log is cut at the comment cap.
        let raw = "y".repeat(1_000_000);
        assert_eq!(
            comment_bodies(None, Some(&raw)).len(),
            MAX_COMMENTS_PER_REPORT
        );
    }

    #[test]
    fn duplicate_note_folds_into_the_first_log_comment() {
        let log = "z".repeat(github::MAX_LOG_IN_ISSUE + 10);
        let bodies = comment_bodies(Some("Seen again."), Some(&log));
        assert_eq!(
            bodies.len(),
            2,
            "note + two log parts still fit in two comments"
        );
        assert!(bodies[0].starts_with("Seen again.\n\n<details>"));
        assert!(bodies[1].starts_with("<details>"));
        // Short logs are already inline in the issue: just the note.
        assert_eq!(
            comment_bodies(Some("Seen again."), Some("short")),
            vec!["Seen again."]
        );
        assert!(comment_bodies(None, Some("short")).is_empty());
    }

    #[test]
    fn log_comments_are_fenced_against_backticks() {
        let log = format!("`````\n@owner\n{}", "l".repeat(github::MAX_LOG_IN_ISSUE));
        let bodies = comment_bodies(None, Some(&log));
        assert_eq!(bodies.len(), 2);
        assert!(bodies[0].contains("\n``````log\n`````\n@owner"));
        assert!(bodies[0].contains("part 1/2"));
    }
}
