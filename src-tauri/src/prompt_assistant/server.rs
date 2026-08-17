use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;

use crate::comfyui::process::tokio_command_no_window;
use crate::error::AppError;

/// Pinned llama.cpp release. Update this constant to roll the binary forward.
/// b7100 is the newest release that still ships `.zip` assets on every platform
/// (b7300+ switched Linux/macOS to `.tar.gz`) and supports the `qwen3`
/// architecture, which the previous pin (b4585) could not load.
const LLAMA_RELEASE: &str = "b7100";
const LLAMA_BASE_URL: &str = "https://github.com/ggml-org/llama.cpp/releases/download";

/// Acceleration backend for the downloaded binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Vulkan,
    Metal,
    Cpu,
}

/// Pick the default backend for this platform (GPU-accelerated where possible).
pub fn default_backend() -> Backend {
    if cfg!(target_os = "macos") {
        Backend::Metal
    } else if cfg!(any(target_os = "windows", target_os = "linux")) {
        Backend::Vulkan
    } else {
        Backend::Cpu
    }
}

/// Archive asset name for a backend on this platform.
fn assets_for(backend: Backend) -> String {
    let t = LLAMA_RELEASE;
    #[cfg(target_os = "windows")]
    {
        match backend {
            Backend::Vulkan => format!("llama-{t}-bin-win-vulkan-x64.zip"),
            _ => format!("llama-{t}-bin-win-cpu-x64.zip"),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = backend;
        format!("llama-{t}-bin-ubuntu-x64.zip")
    }
    #[cfg(target_os = "macos")]
    {
        let _ = backend;
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        format!("llama-{t}-bin-macos-{arch}.zip")
    }
}

#[cfg(target_os = "windows")]
const SERVER_BIN: &str = "llama-server.exe";
#[cfg(not(target_os = "windows"))]
const SERVER_BIN: &str = "llama-server";

/// Manages a single llama-server child process and its idle lifetime.
pub struct LlamaServer {
    bin_dir: PathBuf,
    /// When true, `bin_dir` holds a binary provisioned out-of-band (e.g. baked
    /// into the Docker image) rather than one this struct downloads. The GitHub
    /// release only ships a CPU build for Linux, so a GPU-accelerated server
    /// deployment supplies its own CUDA `llama-server` and we must not clobber
    /// it with the CPU download.
    external_binary: bool,
    child: tokio::sync::Mutex<Option<Child>>,
    port: std::sync::atomic::AtomicU16,
    active_model: std::sync::Mutex<Option<String>>,
    last_used: std::sync::Mutex<Instant>,
    watchdog_started: std::sync::atomic::AtomicBool,
    /// Number of chat requests currently in flight. The idle watchdog must not
    /// unload the server while this is non-zero, otherwise a slow CPU generation
    /// (longer than the idle timeout) gets killed mid-request.
    inflight: std::sync::atomic::AtomicU32,
}

impl LlamaServer {
    pub fn new(bin_dir: PathBuf, external_binary: bool) -> Self {
        Self {
            bin_dir,
            external_binary,
            child: tokio::sync::Mutex::new(None),
            port: std::sync::atomic::AtomicU16::new(0),
            active_model: std::sync::Mutex::new(None),
            last_used: std::sync::Mutex::new(Instant::now()),
            watchdog_started: std::sync::atomic::AtomicBool::new(false),
            inflight: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn server_path(&self) -> PathBuf {
        self.bin_dir.join(SERVER_BIN)
    }

    pub fn is_binary_present(&self) -> bool {
        self.server_path().exists()
    }

    /// Marker file recording which `LLAMA_RELEASE` the on-disk binary came from.
    fn version_marker(&self) -> PathBuf {
        self.bin_dir.join(".llama-release")
    }

    /// True only when the installed binary matches the currently pinned release.
    /// A missing or mismatched marker forces a re-download, so bumping
    /// `LLAMA_RELEASE` rolls existing installs forward instead of silently
    /// reusing a stale (e.g. qwen3-incapable) binary.
    fn is_binary_current(&self) -> bool {
        self.server_path().exists()
            && std::fs::read_to_string(self.version_marker())
                .map(|s| s.trim() == LLAMA_RELEASE)
                .unwrap_or(false)
    }

    /// Path to the captured llama-server stderr log (latest spawn only).
    fn log_path(&self) -> PathBuf {
        self.bin_dir.join("llama-server.log")
    }

    /// Full contents of the captured llama-server stderr log, if it exists.
    /// Used by the diagnostics export so prompt-assistant load failures are
    /// visible to remote/server-mode users who can't reach the host filesystem.
    pub fn read_server_log(&self) -> Option<String> {
        std::fs::read_to_string(self.log_path()).ok()
    }

    /// Returns the child's exit status if it has already terminated.
    async fn child_exit_status(&self) -> Option<std::process::ExitStatus> {
        let mut guard = self.child.lock().await;
        match guard.as_mut() {
            Some(child) => child.try_wait().ok().flatten(),
            None => None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.port.load(std::sync::atomic::Ordering::Relaxed) != 0
    }

    pub fn active_model(&self) -> Option<String> {
        self.active_model.lock().unwrap().clone()
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    /// Download + extract the llama-server binary for the given backend if absent.
    pub async fn ensure_binary(
        &self,
        client: &reqwest::Client,
        backend: Backend,
        progress: &(dyn Fn(&str, u64, u64, bool) + Sync),
    ) -> Result<(), AppError> {
        // A pre-provisioned (e.g. CUDA) binary is used as-is; never download over
        // it. The release download only offers a CPU build for Linux, which would
        // silently undo GPU acceleration on a server deployment.
        if self.external_binary {
            return if self.is_binary_present() {
                Ok(())
            } else {
                Err(AppError::LlmError(format!(
                    "llama-server binary not found at {} (MOOSHIEUI_LLAMA_BIN_DIR is set but the \
                     binary is missing)",
                    self.server_path().display()
                )))
            };
        }
        if self.is_binary_current() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.bin_dir)?;
        let asset = assets_for(backend);
        let url = format!("{LLAMA_BASE_URL}/{LLAMA_RELEASE}/{asset}");
        let archive = self.bin_dir.join(&asset);
        download_with_progress(client, &url, &archive, &asset, progress).await?;
        extract_all_into(&archive, &self.bin_dir)?;
        std::fs::remove_file(&archive).ok();
        if !self.is_binary_present() {
            return Err(AppError::LlmError(format!(
                "llama-server not found after extracting {}",
                self.bin_dir.display()
            )));
        }
        // Record the installed release so a future LLAMA_RELEASE bump re-downloads.
        std::fs::write(self.version_marker(), LLAMA_RELEASE).ok();
        Ok(())
    }

    /// Ensure the server is running with `model_path` loaded. Spawns + health-polls
    /// on first use or after an idle unload / model switch.
    pub async fn ensure_running(
        &self,
        client: &reqwest::Client,
        model_path: &Path,
        model_id: &str,
        n_gpu_layers: i32,
    ) -> Result<u16, AppError> {
        // Already running with the right model?
        if self.is_running() && self.active_model().as_deref() == Some(model_id) {
            self.touch();
            return Ok(self.port.load(std::sync::atomic::Ordering::Relaxed));
        }
        // Switching models: stop the old server first.
        if self.is_running() {
            self.unload().await;
        }

        let port = pick_free_port()?;
        // Capture llama-server stderr (where it logs model-load diagnostics such as
        // "unknown model architecture") to a file so failures are debuggable instead
        // of vanishing into a null sink.
        let log_path = self.log_path();
        let log_file = std::fs::File::create(&log_path)
            .map_err(|e| AppError::LlmError(format!("Failed to create llama-server log: {e}")))?;
        let mut cmd = tokio_command_no_window(self.server_path());
        cmd.arg("-m")
            .arg(model_path)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("-ngl")
            .arg(n_gpu_layers.to_string())
            .arg("--no-webui")
            .stdout(Stdio::null())
            .stderr(Stdio::from(log_file))
            // Ensure the child dies with the app even if unload() is skipped.
            .kill_on_drop(true);
        let child = cmd
            .spawn()
            .map_err(|e| AppError::LlmError(format!("Failed to spawn llama-server: {e}")))?;

        *self.child.lock().await = Some(child);
        self.port.store(port, std::sync::atomic::Ordering::Relaxed);
        *self.active_model.lock().unwrap() = Some(model_id.to_string());

        // Health poll (up to ~180s — large models can be slow to load), but bail out
        // immediately if the child exits (e.g. an unsupported model architecture),
        // surfacing the tail of its captured stderr instead of waiting out the full
        // deadline on a process that is already dead.
        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if let Some(status) = self.child_exit_status().await {
                self.unload().await;
                return Err(AppError::LlmError(format!(
                    "llama-server exited ({status}) before becoming ready{}",
                    read_log_tail(&log_path)
                )));
            }
            if Instant::now() > deadline {
                self.unload().await;
                return Err(AppError::LlmError("llama-server health timeout".into()));
            }
            // Per-request timeout: the shared http_client has no default, so a
            // stalled /health connection would otherwise block this GET forever
            // and the deadline above could never fire (the enhance hangs).
            if let Ok(resp) = client
                .get(&health)
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                if resp.status().is_success() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        self.touch();
        Ok(port)
    }

    /// POST a single chat completion and return the assistant message content.
    pub async fn chat(
        &self,
        client: &reqwest::Client,
        port: u16,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        // Mark a request in flight so the idle watchdog leaves the server alone
        // until generation finishes. CPU inference of a 7B model routinely runs
        // longer than the idle timeout, and `last_used` is only refreshed at the
        // start and end of this call; without the guard the watchdog unloads
        // llama-server mid-generation and the request drops with a bare
        // "error sending request". The guard decrements on every return path.
        self.inflight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _inflight = InflightGuard(&self.inflight);
        self.touch();
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
        let body = json!({
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "temperature": 0.7,
            "max_tokens": max_tokens,
            "stream": false
        });
        let resp = match client
            .post(&url)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // The request never completed. If the child has died the
                // connection is simply refused and we get a bare "error sending
                // request". A crash during inference is almost always an OOM kill
                // (model + KV cache exceed the container/host RAM limit, signal 9)
                // or an illegal instruction on a CPU missing a SIMD feature the
                // prebuilt binary was compiled for (signal 4). Surface the exit
                // status and the tail of the captured stderr so the cause is
                // visible instead of an opaque connection error.
                if let Some(status) = self.child_exit_status().await {
                    let log_path = self.log_path();
                    self.unload().await;
                    return Err(AppError::LlmError(format!(
                        "llama-server died during inference ({status}){}",
                        read_log_tail(&log_path)
                    )));
                }
                return Err(AppError::LlmError(format!(
                    "llama-server request failed: {e}"
                )));
            }
        };
        if !resp.status().is_success() {
            return Err(AppError::LlmError(format!(
                "llama-server returned {}",
                resp.status()
            )));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::LlmError(format!("Bad llama-server response: {e}")))?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        self.touch();
        Ok(content)
    }

    /// Terminate the server and clear running state.
    pub async fn unload(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        self.port.store(0, std::sync::atomic::Ordering::Relaxed);
        *self.active_model.lock().unwrap() = None;
    }
}

/// Decrements the in-flight request counter when a chat call returns, including
/// on early error returns, so the idle watchdog can resume unloading the server
/// once no request is active.
struct InflightGuard<'a>(&'a std::sync::atomic::AtomicU32);

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Build the candidate chat-completions URLs for a user-configured base URL.
///
/// The primary candidate is `{base}/chat/completions` (or the base itself when
/// the user pasted the full endpoint URL). Bases without a trailing version
/// segment (e.g. a bare Ollama `http://host:11434`) serve the OpenAI-compatible
/// API under `/v1`, so `{base}/v1/chat/completions` is offered as a 404 fallback.
fn external_chat_urls(base_url: &str) -> Vec<String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        return vec![base.to_string()];
    }
    versioned_candidates(base, "chat/completions")
}

/// `{base}/{path}`, plus `{base}/v1/{path}` as a 404 fallback when the base
/// carries no trailing version segment.
fn versioned_candidates(base: &str, path: &str) -> Vec<String> {
    let mut urls = vec![format!("{base}/{path}")];
    let last_segment = base.rsplit('/').next().unwrap_or("");
    let versioned = last_segment.len() >= 2
        && last_segment.starts_with('v')
        && last_segment[1..].chars().all(|c| c.is_ascii_digit());
    if !versioned {
        urls.push(format!("{base}/v1/{path}"));
    }
    urls
}

/// Candidate model-list URLs. Users who pasted a full chat-completions endpoint
/// as their base URL still get a working `/models` lookup.
fn external_models_urls(base_url: &str) -> Vec<String> {
    let base = base_url.trim().trim_end_matches('/');
    let base = base.strip_suffix("/chat/completions").unwrap_or(base);
    if base.ends_with("/models") {
        return vec![base.to_string()];
    }
    versioned_candidates(base, "models")
}

async fn send_external_chat(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response, AppError> {
    let mut req = client
        .post(url)
        .json(body)
        .timeout(Duration::from_secs(120));
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    req.send()
        .await
        .map_err(|e| AppError::LlmError(format!("External LLM request failed: {e}")))
}

/// Send a chat completion to an external OpenAI-compatible endpoint (LM Studio,
/// OpenAI, OpenRouter, Ollama, ...) instead of the bundled local llama-server.
/// `base_url` is the API root (e.g. `http://localhost:1234/v1` or
/// `https://api.openai.com/v1`); `/chat/completions` is appended. Base URLs
/// entered without the `/v1` segment are retried at `/v1/chat/completions` on
/// 404. Bearer auth is added when `api_key` is non-empty.
///
/// With an `image`, the user turn becomes a content-block array carrying a
/// `data:` URI alongside the text. Endpoints that ignore images still see the
/// text block, so the worst case is a text-only answer rather than an error.
// One argument over the lint's threshold, and the caller already passes them
// individually; a struct would only move the same fields somewhere else.
#[allow(clippy::too_many_arguments)]
pub async fn chat_external(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
    image: Option<&super::vision::VisionImage>,
) -> Result<String, AppError> {
    let user_content = match image {
        Some(img) => json!([
            { "type": "text", "text": user },
            {
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{};base64,{}", img.media_type, img.base64)
                }
            }
        ]),
        None => json!(user),
    };
    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user_content }
        ],
        "temperature": 0.7,
        "max_tokens": max_tokens,
        "stream": false
    });
    let urls = external_chat_urls(base_url);
    let mut resp = send_external_chat(client, &urls[0], api_key, &body).await?;
    if resp.status().as_u16() == 404 && urls.len() > 1 {
        log::info!(
            "[prompt-assistant] {} returned 404, retrying at {}",
            urls[0],
            urls[1]
        );
        resp = send_external_chat(client, &urls[1], api_key, &body).await?;
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let detail: String = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        let hint = if status.as_u16() == 404 {
            " (check that the base URL is an OpenAI-compatible API root, e.g. http://localhost:11434/v1)"
        } else {
            ""
        };
        return Err(AppError::LlmError(format!(
            "External LLM returned {status}: {detail}{hint}"
        )));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::LlmError(format!("Bad external LLM response: {e}")))?;
    Ok(v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

/// `anthropic-version` header required on every Messages API request.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic's API root, used when the config carries no base URL.
const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";

fn anthropic_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let base = if base.is_empty() {
        ANTHROPIC_BASE_URL
    } else {
        base
    };
    format!("{base}/{path}")
}

/// First 300 characters of an error response body, for surfacing in messages.
async fn error_detail(resp: reqwest::Response) -> String {
    resp.text()
        .await
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect()
}

/// Send a chat completion to Anthropic's Messages API.
///
/// Anthropic is not OpenAI-compatible: the path is `/messages`, auth is the
/// `x-api-key` header rather than a bearer token, `anthropic-version` is
/// mandatory, the system prompt is a top-level field instead of a message, and
/// the answer arrives as a list of content blocks.
///
/// An `image` leads the user turn, which is what Anthropic recommends: the
/// model reads the picture before the instruction that refers to it.
// One argument over the lint's threshold; see `chat_external`.
#[allow(clippy::too_many_arguments)]
pub async fn chat_anthropic(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
    image: Option<&super::vision::VisionImage>,
) -> Result<String, AppError> {
    if api_key.trim().is_empty() {
        return Err(AppError::LlmError(
            "Anthropic requires an API key. Add one in Settings > Prompt Assistant.".into(),
        ));
    }
    let user_content = match image {
        Some(img) => json!([
            {
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": img.media_type,
                    "data": img.base64
                }
            },
            { "type": "text", "text": user }
        ]),
        None => json!(user),
    };
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "temperature": 0.7,
        "messages": [ { "role": "user", "content": user_content } ]
    });
    let resp = client
        .post(anthropic_url(base_url, "messages"))
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| AppError::LlmError(format!("Anthropic request failed: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = error_detail(resp).await;
        return Err(AppError::LlmError(format!(
            "Anthropic returned {status}: {detail}"
        )));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::LlmError(format!("Bad Anthropic response: {e}")))?;
    // Concatenate every text block: a response can lead with a non-text block.
    let text = v["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    Ok(text)
}

/// Send a chat completion to whichever external provider is configured,
/// picking the wire format from the provider registry.
// One more argument than the per-wire helpers it dispatches to, which are
// themselves at the limit. Bundling them into a struct would only move the
// same fields somewhere else.
#[allow(clippy::too_many_arguments)]
pub async fn chat_provider(
    client: &reqwest::Client,
    provider_id: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
    image: Option<&super::vision::VisionImage>,
) -> Result<String, AppError> {
    let base = super::providers::effective_base_url(provider_id, base_url);
    match super::providers::wire_for(provider_id) {
        super::providers::Wire::Anthropic => {
            chat_anthropic(
                client, &base, api_key, model, system, user, max_tokens, image,
            )
            .await
        }
        super::providers::Wire::OpenAiCompatible => {
            if base.is_empty() {
                return Err(AppError::LlmError(
                    "No API base URL is set for the external LLM (Settings > Prompt Assistant)."
                        .into(),
                ));
            }
            chat_external(
                client, &base, api_key, model, system, user, max_tokens, image,
            )
            .await
        }
    }
}

/// List the model ids a provider exposes.
///
/// Both wire formats answer `GET {root}/models` with `{"data": [{"id": ...}]}`;
/// only the auth headers differ. Doubles as the credential round-trip check in
/// the settings UI: a bad key fails here with the provider's own message.
pub async fn list_models(
    client: &reqwest::Client,
    provider_id: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, AppError> {
    let anthropic = super::providers::wire_for(provider_id) == super::providers::Wire::Anthropic;
    let base = super::providers::effective_base_url(provider_id, base_url);
    let urls = if anthropic {
        vec![anthropic_url(&base, "models?limit=1000")]
    } else {
        if base.is_empty() {
            return Err(AppError::LlmError(
                "No API base URL is set for the external LLM (Settings > Prompt Assistant).".into(),
            ));
        }
        external_models_urls(&base)
    };
    let send = |url: String| {
        let mut req = client.get(url).timeout(Duration::from_secs(30));
        if anthropic {
            req = req
                .header("x-api-key", api_key)
                .header("anthropic-version", ANTHROPIC_VERSION);
        } else if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }
        req.send()
    };
    let mut resp = send(urls[0].clone())
        .await
        .map_err(|e| AppError::LlmError(format!("Model list request failed: {e}")))?;
    if resp.status().as_u16() == 404 && urls.len() > 1 {
        resp = send(urls[1].clone())
            .await
            .map_err(|e| AppError::LlmError(format!("Model list request failed: {e}")))?;
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = error_detail(resp).await;
        return Err(AppError::LlmError(format!(
            "Model list returned {status}: {detail}"
        )));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::LlmError(format!("Bad model list response: {e}")))?;
    let mut ids: Vec<String> = v["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    // Ollama serves this list with no modality information, so a text-only model
    // looks exactly like a VLM here. Its own API does know, and every job the
    // prompt assistant does can involve an image, so narrow the picker to models
    // that can actually see one. Keep the full list when the probe finds nothing:
    // an empty picker reads as a broken request, and the field is free text, so a
    // user who wants a text-only model can still type its id.
    if !anthropic {
        if let Some(vision) = super::vision::ollama_vision_models(client, &base).await {
            let kept: Vec<String> = ids
                .iter()
                .filter(|id| {
                    vision
                        .iter()
                        .any(|name| super::vision::model_id_matches(id, name))
                })
                .cloned()
                .collect();
            if kept.is_empty() {
                log::info!(
                    "[prompt-assistant] Ollama reported no vision-capable models; \
                     keeping the unfiltered list"
                );
            } else {
                log::info!(
                    "[prompt-assistant] Ollama: {} of {} models are vision-capable",
                    kept.len(),
                    ids.len()
                );
                return Ok(kept);
            }
        }
    }
    Ok(ids)
}

/// Idle watchdog implemented as a free function so it can hold an Arc clone.
pub fn start_idle_watchdog(server: std::sync::Arc<LlamaServer>, idle_secs: u64) {
    if server
        .watchdog_started
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return; // already running
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            if !server.is_running() {
                continue;
            }
            // Never unload while a request is in flight (a slow CPU generation can
            // outlast the idle timeout); refresh the idle clock so the countdown
            // restarts only once the request completes.
            if server.inflight.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                server.touch();
                continue;
            }
            let idle = server.last_used.lock().unwrap().elapsed().as_secs();
            if idle >= idle_secs {
                log::info!("[prompt-assistant] idle {idle}s, unloading llama-server");
                server.unload().await;
            }
        }
    });
}

/// Read the last few non-empty lines of the llama-server log, formatted for
/// appending to an error message. Returns an empty string if the log is missing
/// or blank.
fn read_log_tail(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
            if lines.is_empty() {
                return String::new();
            }
            let start = lines.len().saturating_sub(6);
            format!(":\n{}", lines[start..].join("\n"))
        }
        Err(_) => String::new(),
    }
}

fn pick_free_port() -> Result<u16, AppError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| AppError::LlmError(format!("No free port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::LlmError(e.to_string()))?
        .port();
    Ok(port)
}

async fn download_with_progress(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
    progress: &(dyn Fn(&str, u64, u64, bool) + Sync),
) -> Result<(), AppError> {
    let resp = tokio::time::timeout(Duration::from_secs(30), client.get(url).send())
        .await
        .map_err(|_| AppError::LlmError("Download timed out while connecting".into()))?
        .map_err(|e| AppError::LlmError(format!("Download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::LlmError(format!(
            "Download returned {}",
            resp.status()
        )));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(dest).await?;
    progress(label, 0, total, false);
    // Stream to disk; on any error remove the partial file so a retry starts clean.
    let stream_result: Result<u64, AppError> = async {
        let mut downloaded: u64 = 0;
        let mut last_emit = 0u64;
        let mut resp = resp;
        // A stalled connection (no bytes for 60s) errors out rather than hanging
        // forever — the shared http_client has no read timeout of its own.
        loop {
            let chunk = tokio::time::timeout(Duration::from_secs(60), resp.chunk())
                .await
                .map_err(|_| AppError::LlmError("Download stalled (no data for 60s)".into()))?
                .map_err(|e| AppError::LlmError(format!("Download read error: {e}")))?;
            let Some(chunk) = chunk else { break };
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if downloaded - last_emit > 1024 * 1024 || downloaded == total {
                last_emit = downloaded;
                progress(label, downloaded, total, false);
            }
        }
        file.flush().await?;
        Ok(downloaded)
    }
    .await;
    match stream_result {
        Ok(downloaded) => {
            progress(label, downloaded, total, true);
            Ok(())
        }
        Err(e) => {
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            Err(e)
        }
    }
}

/// Extract every file from a zip archive flatly into `dir` (strip directories),
/// preserving executable bits on unix.
fn extract_all_into(archive_path: &Path, dir: &Path) -> Result<(), AppError> {
    let file = std::fs::File::open(archive_path)?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| AppError::LlmError(format!("Bad zip: {e}")))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::LlmError(format!("Zip entry error: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = match entry
            .enclosed_name()
            .and_then(|p| p.file_name().map(|f| f.to_owned()))
        {
            Some(n) => n,
            None => continue,
        };
        let out_path = dir.join(name);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if out_path.file_name().and_then(|n| n.to_str()) == Some(SERVER_BIN) {
                std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    Ok(())
}
