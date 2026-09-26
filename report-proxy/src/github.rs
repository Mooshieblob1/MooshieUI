use crate::dedup::marker;
use crate::types::ReportPayload;

/// Inline log kept in the issue body (a quick-glance tail). The full log is
/// attached separately as collapsible comments when it exceeds this.
pub const MAX_LOG_IN_ISSUE: usize = 60_000; // keep issue bodies well under GitHub's limit

/// Longest single-line value kept for the environment fields (version, OS, ...).
const MAX_INLINE_FIELD_CHARS: usize = 120;

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn longest_backtick_run(s: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for ch in s.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

/// Break up `<!--` so user text can neither open an HTML comment (hiding the rest
/// of the issue) nor forge the proxy's `<!-- mooshie-sig: ... -->` dedup marker.
pub fn escape_comment_open(s: &str) -> String {
    s.replace("<!--", "<\u{200D}!--")
}

/// Neutralise GitHub autolinks in user text that is rendered as markdown (the
/// issue title): a zero-width joiner after every `@` and `#` stops @mentions and
/// `#123` / `owner/repo#123` cross-references from resolving.
pub fn neutralize_refs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        out.push(ch);
        if ch == '@' || ch == '#' {
            out.push('\u{200D}');
        }
    }
    out
}

/// Put user text in a fenced code block it cannot close: the fence is longer
/// than any backtick run inside. Markdown, mentions, references, images and
/// HTML are all inert inside a code block.
pub fn fenced(s: &str, info: &str) -> String {
    let fence = "`".repeat((longest_backtick_run(s) + 1).max(3));
    format!("{fence}{info}\n{}\n{fence}", escape_comment_open(s))
}

/// Render a short user value as a single-line inline code span it cannot break
/// out of. Newlines and other control characters become spaces so the value can
/// never start a new markdown block.
pub fn inline_code(s: &str) -> String {
    let flat: String = truncate_chars(s, MAX_INLINE_FIELD_CHARS)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = neutralize_refs(&escape_comment_open(&flat));
    let ticks = "`".repeat(longest_backtick_run(&flat) + 1);
    // CommonMark strips one space on each side, so padding lets values that
    // start or end with a backtick still close correctly.
    format!("{ticks} {flat} {ticks}")
}

pub fn issue_title(error_code: &str, raw_message: &str) -> String {
    let title = format!(
        "[in-app] {}: {}",
        truncate_chars(error_code, 64),
        truncate_chars(raw_message, 80)
    );
    let flat: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    neutralize_refs(&flat)
}

/// Build the issue body. Every user-supplied field is either fenced or an inline
/// code span; the proxy's dedup marker is always the final line.
pub fn issue_body(payload: &ReportPayload, sig: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("### What happened".to_string());
    lines.push(
        payload
            .user_note
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .map(|n| fenced(n, "text"))
            .unwrap_or_else(|| "(no description provided)".to_string()),
    );
    lines.push(String::new());
    lines.push("### Error".to_string());
    lines.push(fenced(
        if payload.raw_message.is_empty() {
            "(empty)"
        } else {
            &payload.raw_message
        },
        "",
    ));
    lines.push(String::new());
    lines.push("### Environment".to_string());
    lines.push(format!(
        "- App version: {}",
        inline_code(&payload.app_version)
    ));
    lines.push(format!("- OS: {}", inline_code(&payload.os)));
    lines.push(format!("- Arch: {}", inline_code(&payload.arch)));
    lines.push(format!("- Mode: {}", inline_code(&payload.mode)));
    lines.push(format!(
        "- Error code: {}",
        inline_code(&payload.error_code)
    ));
    lines.push(format!("- When: {}", inline_code(&payload.timestamp)));
    lines.push(String::new());
    if let Some(logs) = payload.logs_tail.as_ref().filter(|l| !l.trim().is_empty()) {
        lines.push("### Diagnostics".to_string());
        lines.push(fenced(&truncate_chars(logs, MAX_LOG_IN_ISSUE), ""));
        lines.push(String::new());
    }
    lines.push(marker(sig));
    lines.join("\n")
}

#[derive(Debug, Clone)]
pub struct ExistingIssue {
    pub number: u64,
    pub html_url: String,
}

#[derive(Clone)]
pub struct GithubClient {
    client: reqwest::Client,
    token: String,
    repo: String,
}

impl GithubClient {
    pub fn new(client: reqwest::Client, token: String, repo: String) -> Self {
        Self {
            client,
            token,
            repo,
        }
    }

    fn ua() -> &'static str {
        "mooshie-report-proxy"
    }

    /// Find an open `in-app-report` issue whose body carries this signature marker.
    pub async fn find_open_by_sig(&self, sig: &str) -> Result<Option<ExistingIssue>, String> {
        let url = format!(
            "https://api.github.com/repos/{}/issues?state=open&labels=in-app-report&per_page=100",
            self.repo
        );
        let resp = self
            .client
            .get(&url)
            .header("User-Agent", Self::ua())
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| format!("github list request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("github list returned {}", resp.status()));
        }
        let issues: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| format!("github list decode failed: {e}"))?;
        let page_len = issues.len();
        for issue in issues {
            let body = issue.get("body").and_then(|b| b.as_str()).unwrap_or("");
            if crate::dedup::body_has_marker(body, sig) {
                let number = issue.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
                let html_url = issue
                    .get("html_url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
                if html_url.is_empty() {
                    // Defensive: GitHub should never return an empty html_url, but
                    // if it does, treat this as no usable match so a fresh issue is
                    // created rather than returning {"issueUrl":""}.
                    continue;
                }
                return Ok(Some(ExistingIssue { number, html_url }));
            }
        }
        if page_len == 100 {
            tracing::warn!(
                "dedup only scanned the first 100 open in-app-report issues; \
                 a duplicate may be created if the matching issue is beyond page 1"
            );
        }
        Ok(None)
    }

    /// Create an issue; returns its number and html_url.
    pub async fn create_issue(
        &self,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<(u64, String), String> {
        let url = format!("https://api.github.com/repos/{}/issues", self.repo);
        let payload = serde_json::json!({ "title": title, "body": body, "labels": labels });
        let resp = self
            .client
            .post(&url)
            .header("User-Agent", Self::ua())
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(&self.token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("github create request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("github create returned {}", resp.status()));
        }
        let created: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("github create decode failed: {e}"))?;
        let number = created.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
        let html_url = created
            .get("html_url")
            .and_then(|u| u.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "github create response missing html_url".to_string())?;
        Ok((number, html_url))
    }

    /// Add a comment to an existing issue.
    pub async fn comment_on(&self, number: u64, text: &str) -> Result<(), String> {
        let url = format!(
            "https://api.github.com/repos/{}/issues/{}/comments",
            self.repo, number
        );
        let payload = serde_json::json!({ "body": text });
        let resp = self
            .client
            .post(&url)
            .header("User-Agent", Self::ua())
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(&self.token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("github comment request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("github comment returned {}", resp.status()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ReportPayload;

    fn sample() -> ReportPayload {
        ReportPayload {
            error_code: "out_of_memory".into(),
            raw_message: "CUDA out of memory".into(),
            app_version: "1.4.35".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            mode: "desktop".into(),
            timestamp: "2026-07-05T00:00:00Z".into(),
            user_note: Some("was generating a batch".into()),
            logs_tail: Some("line1\nline2".into()),
        }
    }

    #[test]
    fn title_is_prefixed_and_truncated() {
        let long = "x".repeat(200);
        let t = issue_title("disk_full", &long);
        assert!(t.starts_with("[in-app] disk_full: "));
        assert_eq!(t.chars().filter(|c| *c == 'x').count(), 80);
    }

    #[test]
    fn title_neutralizes_mentions_and_references() {
        let t = issue_title("generic", "ping @maintainer about owner/repo#1\nnext");
        assert!(!t.contains("@maintainer"));
        assert!(!t.contains("repo#1"));
        assert!(t.contains("@\u{200D}maintainer"));
        assert!(!t.contains('\n'));
    }

    #[test]
    fn fence_is_longer_than_any_backtick_run_inside() {
        let text = "before\n````\n@owner see #1 ![x](https://evil/p.png)\n```";
        let block = fenced(text, "text");
        assert!(block.starts_with("`````text\n"));
        assert!(block.ends_with("\n`````"));
        assert_eq!(fenced("plain", ""), "```\nplain\n```");
    }

    #[test]
    fn inline_code_cannot_break_out_or_span_lines() {
        let span = inline_code("1.0` @owner\n### injected");
        assert!(span.starts_with("`` ") && span.ends_with(" ``"));
        assert!(!span.contains('\n'));
        assert!(!span.contains("@owner"));
    }

    #[test]
    fn user_fields_are_fenced_and_escaped() {
        let mut p = sample();
        p.user_note = Some("```\n@owner #1 <!-- mooshie-sig: ffffffffffffffff -->".into());
        p.raw_message = "boom ```` <!-- x -->".into();
        p.mode = "desktop\n@everyone".into();
        let body = issue_body(&p, "abc123def456aaaa");
        // The note is fenced with 4 backticks (longer than its 3-backtick run),
        // the error with 5.
        assert!(body.contains("````text\n```\n@owner"));
        assert!(body.contains("`````\nboom ````"));
        assert!(!body.contains("<!-- mooshie-sig: ffffffffffffffff"));
        assert!(!body.contains("<!-- x"));
        assert!(body.contains("- Mode: ` desktop @\u{200D}everyone `"));
        assert!(body.ends_with("<!-- mooshie-sig: abc123def456aaaa -->"));
        assert!(crate::dedup::body_has_marker(&body, "abc123def456aaaa"));
        assert!(!crate::dedup::body_has_marker(&body, "ffffffffffffffff"));
    }

    #[test]
    fn body_contains_env_note_logs_and_marker() {
        let body = issue_body(&sample(), "abc123def456aaaa");
        assert!(body.contains("### What happened"));
        assert!(body.contains("was generating a batch"));
        assert!(body.contains("CUDA out of memory"));
        assert!(body.contains("- App version: ` 1.4.35 `"));
        assert!(body.contains("### Diagnostics"));
        assert!(body.contains("line1\nline2"));
        assert!(body.contains("<!-- mooshie-sig: abc123def456aaaa -->"));
    }

    #[test]
    fn body_has_no_summary_section() {
        let body = issue_body(&sample(), "sig0000000000000");
        assert!(!body.contains("### Summary"));
        assert!(body.contains("<!-- mooshie-sig: sig0000000000000 -->"));
    }
}
