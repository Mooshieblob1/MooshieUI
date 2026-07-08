use crate::dedup::marker;
use crate::types::ReportPayload;

/// Inline log kept in the issue body (a quick-glance tail). The full log is
/// attached separately as collapsible comments when it exceeds this.
pub const MAX_LOG_IN_ISSUE: usize = 60_000; // keep issue bodies well under GitHub's limit

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub fn issue_title(error_code: &str, raw_message: &str) -> String {
    format!(
        "[in-app] {}: {}",
        error_code,
        truncate_chars(raw_message, 80)
    )
}

pub fn issue_body(payload: &ReportPayload, sig: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("### What happened".to_string());
    lines.push(
        payload
            .user_note
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "(no description provided)".to_string()),
    );
    lines.push(String::new());
    lines.push("### Error".to_string());
    lines.push("```".to_string());
    lines.push(if payload.raw_message.is_empty() {
        "(empty)".to_string()
    } else {
        payload.raw_message.clone()
    });
    lines.push("```".to_string());
    lines.push(String::new());
    lines.push("### Environment".to_string());
    lines.push(format!("- App version: `{}`", payload.app_version));
    lines.push(format!("- OS: `{}`", payload.os));
    lines.push(format!("- Arch: `{}`", payload.arch));
    lines.push(format!("- Mode: {}", payload.mode));
    lines.push(format!("- Error code: {}", payload.error_code));
    lines.push(format!("- When: {}", payload.timestamp));
    lines.push(String::new());
    if let Some(logs) = payload.logs_tail.as_ref().filter(|l| !l.trim().is_empty()) {
        lines.push("### Diagnostics".to_string());
        lines.push("```".to_string());
        lines.push(truncate_chars(logs, MAX_LOG_IN_ISSUE));
        lines.push("```".to_string());
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
    fn body_contains_env_note_logs_and_marker() {
        let body = issue_body(&sample(), "abc123def456aaaa");
        assert!(body.contains("### What happened"));
        assert!(body.contains("was generating a batch"));
        assert!(body.contains("CUDA out of memory"));
        assert!(body.contains("- App version: `1.4.35`"));
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
