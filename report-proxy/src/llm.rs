use std::time::Duration;

use crate::catalog::CatalogEntry;
use crate::types::Config;

const MAX_LOG_CHARS: usize = 4000; // ~1-1.3k tokens; the model has only 3072 ctx

/// Build the summarization prompt. `/no_think` disables Qwen3 reasoning so the
/// CPU-bound model spends its budget on the answer, not the thinking pass.
pub fn build_prompt(
    entry: Option<CatalogEntry>,
    error_code: &str,
    raw_message: &str,
    logs_tail: Option<&str>,
) -> String {
    let mut ctx = String::new();
    ctx.push_str("/no_think\n");
    ctx.push_str(
        "You are triaging a bug report for an image-generation desktop app. \
Write ONE short paragraph (2-4 sentences), plain English, no lists, describing the \
likely problem and the most probable cause. Do not restate the logs verbatim.\n\n",
    );
    ctx.push_str(&format!("Error code: {error_code}\n"));
    if let Some(e) = entry {
        ctx.push_str(&format!("Known meaning: {} - {}\n", e.title, e.what));
        ctx.push_str(&format!("Typical cause: {}\n", e.why));
        ctx.push_str(&format!("Known fixes: {}\n", e.fixes));
    }
    ctx.push_str(&format!("Raw error: {raw_message}\n"));
    if let Some(logs) = logs_tail {
        let char_count = logs.chars().count();
        let tail: String = if char_count > MAX_LOG_CHARS {
            logs.chars().skip(char_count - MAX_LOG_CHARS).collect()
        } else {
            logs.to_string()
        };
        ctx.push_str("\nRecent log tail:\n");
        ctx.push_str(&tail);
    }
    ctx
}

/// Extract the assistant's final text, ignoring empty/thinking-only responses.
pub fn parse_content(json: &serde_json::Value) -> Option<String> {
    let content = json
        .get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()?
        .trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

/// Best-effort one-paragraph summary. Returns None on any failure or timeout.
pub async fn summarize(client: &reqwest::Client, cfg: &Config, prompt: &str) -> Option<String> {
    if cfg.llm_base_url.trim().is_empty() {
        return None;
    }
    let url = format!(
        "{}/v1/chat/completions",
        cfg.llm_base_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "model": cfg.llm_model,
        "messages": [ { "role": "user", "content": prompt } ],
        "max_tokens": 300,
        "temperature": 0.3,
        "stream": false
    });
    let resp = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(cfg.llm_timeout_secs))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!("llm returned {}", resp.status());
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    parse_content(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    #[test]
    fn prompt_disables_thinking_and_includes_context() {
        let entry = catalog::lookup("out_of_memory");
        let prompt = build_prompt(entry, "out_of_memory", "CUDA OOM", Some("log line here"));
        assert!(prompt.contains("/no_think"));
        assert!(prompt.contains("out_of_memory"));
        assert!(prompt.contains("CUDA OOM"));
        assert!(prompt.contains("log line here"));
    }

    #[test]
    fn prompt_truncates_long_logs() {
        let big = "z".repeat(20_000);
        let prompt = build_prompt(None, "generic", "err", Some(&big));
        assert!(prompt.chars().filter(|c| *c == 'z').count() <= 4000);
    }

    #[test]
    fn parse_reads_message_content() {
        let json = serde_json::json!({
            "choices": [ { "message": { "content": "  Hello.  " } } ]
        });
        assert_eq!(parse_content(&json), Some("Hello.".to_string()));
    }

    #[test]
    fn parse_none_when_content_empty_or_thinking_only() {
        let json = serde_json::json!({
            "choices": [ { "message": { "content": "", "reasoning_content": "thinking..." } } ]
        });
        assert_eq!(parse_content(&json), None);
    }
}
