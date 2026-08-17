//! Best-effort auto-start for local, self-hosted LLM servers.
//!
//! Ollama and LM Studio both ship a background app that a CLI command wakes:
//! `ollama list` launches Ollama's tray app/server if it isn't running yet, and
//! `lms server start` does the same for LM Studio. Neither costs anything when
//! the server is already up (a probe skips them), and a binary that isn't on
//! PATH just errors and is ignored — this is a nudge, not a dependency; the
//! caller's own request is what actually reports success or failure to the
//! user. vLLM has no argument-free "just start" command (it needs a model to
//! serve), so there is no safe generic way to auto-launch it here.

use std::time::Duration;

/// Whether `base_url` points at this machine — only a local server can
/// plausibly be started by a local subprocess.
fn is_loopback(base_url: &str) -> bool {
    let base = base_url.trim();
    base.contains("localhost") || base.contains("127.0.0.1") || base.contains("[::1]")
}

/// Probe whether something is already listening at `base_url`. Any response
/// (even an error status) means a server is up; only a connection-level
/// failure counts as "not running".
async fn already_running(client: &reqwest::Client, base_url: &str) -> bool {
    client
        .get(base_url.trim().trim_end_matches('/'))
        .timeout(Duration::from_millis(700))
        .send()
        .await
        .is_ok()
}

/// Nudge a local LLM server awake before a request that needs it.
///
/// Skips non-loopback bases (remote providers, or a self-hosted box elsewhere
/// on the LAN) and bases that already answer, so the common case — the server
/// is already running — costs one fast probe and nothing else.
pub async fn wake_local_server(client: &reqwest::Client, base_url: &str) {
    if !is_loopback(base_url) || already_running(client, base_url).await {
        return;
    }
    log::info!("[prompt-assistant] local LLM endpoint not responding; attempting to wake it");
    let wake = async {
        tokio::join!(
            crate::comfyui::process::tokio_command_no_window("ollama")
                .arg("list")
                .output(),
            crate::comfyui::process::tokio_command_no_window("lms")
                .args(["server", "start"])
                .output(),
        )
    };
    let _ = tokio::time::timeout(Duration::from_secs(20), wake).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_urls_are_recognized() {
        assert!(is_loopback("http://localhost:11434/v1"));
        assert!(is_loopback("http://127.0.0.1:1234/v1"));
        assert!(is_loopback("http://[::1]:11434"));
    }

    #[test]
    fn remote_urls_are_not_loopback() {
        assert!(!is_loopback("https://api.openai.com/v1"));
        assert!(!is_loopback("https://openrouter.ai/api/v1"));
        assert!(!is_loopback("http://192.168.1.50:11434/v1"));
    }
}
