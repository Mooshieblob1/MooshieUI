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

/// SHA-256 of every `LLAMA_RELEASE` asset [`assets_for`] can pick. The binary
/// is executed, so a download that does not match is never extracted. Bumping
/// `LLAMA_RELEASE` means replacing every entry; an asset missing from here is
/// refused rather than installed unverified.
const LLAMA_ASSET_SHA256: &[(&str, &str)] = &[
    (
        "llama-b7100-bin-win-vulkan-x64.zip",
        "9670903b9821777f08fa4875603c872626cff8d9a37cd6a80c16cd6b22da0daa",
    ),
    (
        "llama-b7100-bin-win-cpu-x64.zip",
        "a530dfaf4928f40d08f5f5eb4e6350f37409ce5310f56240d725c3b84aca93ee",
    ),
    (
        "llama-b7100-bin-ubuntu-x64.zip",
        "a34d005333ede5f4f1c327667cce2546ef77ced1263b666199d97896b8e1e09e",
    ),
    (
        "llama-b7100-bin-macos-arm64.zip",
        "c54e7997d8dc4bc85d2166de14619cdafdd82aba34aadc15046668320e78ed63",
    ),
    (
        "llama-b7100-bin-macos-x64.zip",
        "fec57f35bf4c0bfb3bbd12e5d4a73de27d7a9938737aedb41e436adae215ab0c",
    ),
];

/// The pinned digest for a release asset, if there is one.
fn pinned_sha256(asset: &str) -> Option<&'static str> {
    LLAMA_ASSET_SHA256
        .iter()
        .find(|(name, _)| *name == asset)
        .map(|(_, digest)| *digest)
}

/// Most a remote provider may send back for one completion or model list.
/// Model lists are the big ones (a few MB at most for the aggregators).
pub(super) const RESPONSE_LIMIT: usize = 16 * 1024 * 1024;

/// How much of an error body is read. Only its first 300 characters are ever
/// shown, so there is no reason to buffer more of it.
const ERROR_BODY_LIMIT: usize = 16 * 1024;

/// How long a model switch waits for requests already sent to the old server
/// to finish before stopping it anyway. Just over `LlamaServer::chat`'s own
/// request timeout, so a switch never cuts off a request that could still
/// succeed.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(125);

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
    /// Serialises `ensure_running`, so two first requests cannot each spawn a
    /// server (the second replacing the first's child, whose health loop then
    /// kills the survivor) and a model switch cannot interleave with a start.
    start_lock: tokio::sync::Mutex<()>,
    /// Serialises `ensure_binary`: concurrent first uses would otherwise
    /// download and extract over the same archive path at once.
    binary_lock: tokio::sync::Mutex<()>,
    /// Random bearer key of the running server, fresh for every launch.
    /// llama-server listens on loopback with permissive CORS, so without one
    /// any local process, or any website probing localhost ports, could use
    /// the model and read `/slots`.
    api_key: std::sync::Mutex<String>,
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
            start_lock: tokio::sync::Mutex::new(()),
            binary_lock: tokio::sync::Mutex::new(()),
            api_key: std::sync::Mutex::new(String::new()),
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

    /// Whether a server has passed its health check and is serving requests.
    /// A server that is still loading does not count: its port is only
    /// published once `/health` answers.
    pub fn is_running(&self) -> bool {
        self.port.load(std::sync::atomic::Ordering::SeqCst) != 0
    }

    /// Why the child spawned by `ensure_running` is no longer usable: it
    /// exited, or someone unloaded it while it was still loading. `None`
    /// while it is alive.
    async fn child_gone(&self) -> Option<String> {
        let mut guard = self.child.lock().await;
        match guard.as_mut() {
            None => Some("was stopped while the model was loading".to_string()),
            Some(child) => child
                .try_wait()
                .ok()
                .flatten()
                .map(|status| format!("exited ({status}) before becoming ready")),
        }
    }

    /// Stop the current server for a model switch without cutting off
    /// requests already sent to it.
    ///
    /// Clearing the port first makes every `chat` that has not yet counted
    /// itself in flight fail fast instead of reaching a server about to die;
    /// the ones that already did are waited for (bounded by `DRAIN_TIMEOUT`).
    /// `chat` increments `inflight` before it re-checks the port, and both
    /// sides use `SeqCst`, so no request can slip between the two.
    async fn retire(&self) {
        self.port.store(0, std::sync::atomic::Ordering::SeqCst);
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        while self.inflight.load(std::sync::atomic::Ordering::SeqCst) > 0
            && Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.unload().await;
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
        // Re-checked under the lock: whoever held it may just have installed
        // the binary, and downloading again would extract over files a
        // freshly started server is executing.
        let _installing = self.binary_lock.lock().await;
        if self.is_binary_current() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.bin_dir)?;
        let asset = assets_for(backend);
        let expected = pinned_sha256(&asset).ok_or_else(|| {
            AppError::LlmError(format!(
                "No pinned checksum for {asset}; refusing to install an unverified llama-server."
            ))
        })?;
        let url = format!("{LLAMA_BASE_URL}/{LLAMA_RELEASE}/{asset}");
        let archive = self.bin_dir.join(&asset);
        let digest = download_with_progress(client, &url, &archive, &asset, progress).await?;
        if !digest.eq_ignore_ascii_case(expected) {
            std::fs::remove_file(&archive).ok();
            return Err(AppError::LlmError(format!(
                "Downloaded {asset} failed checksum verification (expected {expected}, got {digest}); \
                 not installing it."
            )));
        }
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
        // One start at a time; see `start_lock`. A second caller waits here and
        // then finds the server the first one started.
        let _starting = self.start_lock.lock().await;
        // Already running with the right model?
        if self.is_running() && self.active_model().as_deref() == Some(model_id) {
            self.touch();
            return Ok(self.port.load(std::sync::atomic::Ordering::SeqCst));
        }
        // Switching models: stop the old server once its requests are done.
        if self.is_running() {
            self.retire().await;
        } else if self.child.lock().await.is_some() {
            // A child with no published port is left over from a start whose
            // caller went away mid-load; nothing can be using it.
            self.unload().await;
        }

        let api_key = random_api_key();
        *self.api_key.lock().unwrap() = api_key.clone();
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
            // Passed through the environment rather than `--api-key`: another
            // local account can read a process's command line, but not its
            // environment. llama-server reads `LLAMA_API_KEY` as `--api-key`.
            .env("LLAMA_API_KEY", &api_key)
            .stdout(Stdio::null())
            .stderr(Stdio::from(log_file))
            // Ensure the child dies with the app even if unload() is skipped.
            .kill_on_drop(true);
        let child = cmd
            .spawn()
            .map_err(|e| AppError::LlmError(format!("Failed to spawn llama-server: {e}")))?;

        *self.child.lock().await = Some(child);

        // Health poll (up to ~180s — large models can be slow to load), but bail out
        // immediately if the child exits (e.g. an unsupported model architecture),
        // surfacing the tail of its captured stderr instead of waiting out the full
        // deadline on a process that is already dead.
        //
        // The port stays unpublished until this passes, so `is_running` is false
        // and no request is sent to a server that is still loading.
        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if let Some(reason) = self.child_gone().await {
                self.unload().await;
                return Err(AppError::LlmError(format!(
                    "llama-server {reason}{}",
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
                .bearer_auth(&api_key)
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

        // Publish under the child lock: an `unload` that took the child since
        // the last check above must not be followed by a port for a server that
        // no longer exists.
        let child = self.child.lock().await;
        if child.is_none() {
            return Err(AppError::LlmError(
                "llama-server was stopped while the model was loading".into(),
            ));
        }
        *self.active_model.lock().unwrap() = Some(model_id.to_string());
        self.touch();
        self.port.store(port, std::sync::atomic::Ordering::SeqCst);
        drop(child);
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
        // Counted in flight *before* this check (see `retire`): a server that
        // was switched or unloaded since `ensure_running` handed out `port` is
        // not sent anything, and one that is still current cannot be retired
        // until this request finishes.
        if self.port.load(std::sync::atomic::Ordering::SeqCst) != port {
            return Err(AppError::LlmError(
                "The local model was stopped or switched by another request. Please retry.".into(),
            ));
        }
        let api_key = self.api_key.lock().unwrap().clone();
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
            .bearer_auth(&api_key)
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
        let v = bounded_json(resp, RESPONSE_LIMIT, "llama-server response").await?;
        let content = completion_content(&v, "The local model")?;
        self.touch();
        Ok(content)
    }

    /// Terminate the server and clear running state.
    pub async fn unload(&self) {
        // Unpublish together with taking the child, under its lock, so this
        // can never interleave with `ensure_running` publishing a new server.
        let child = {
            let mut guard = self.child.lock().await;
            self.port.store(0, std::sync::atomic::Ordering::SeqCst);
            *self.active_model.lock().unwrap() = None;
            guard.take()
        };
        if let Some(mut child) = child {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
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

/// Per-request ceiling for a remote chat completion.
///
/// A vision turn ships up to four ~1 MP JPEGs and asks for a long answer, and
/// hosted models routinely take well over two minutes on that; a text-only
/// turn that has not answered in two minutes is not going to.
fn chat_timeout(has_images: bool) -> Duration {
    if has_images {
        Duration::from_secs(300)
    } else {
        Duration::from_secs(120)
    }
}

/// The whole cause chain of an error, outermost first.
///
/// reqwest's `Display` prints only its own kind and the URL ("error sending
/// request for url (...)"), hiding the TLS, DNS or timeout detail underneath
/// that is the only part worth reading in a log.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut cur = err.source();
    while let Some(src) = cur {
        let text = src.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        cur = src.source();
    }
    out
}

/// Human-readable reason a request never produced a response.
///
/// A timeout is named as such (with the budget it blew) instead of being
/// buried under reqwest's generic "error sending request" wording.
fn describe_request_error(err: &reqwest::Error, timeout: Duration) -> String {
    if err.is_timeout() {
        return format!(
            "timed out after {}s waiting for the model to answer",
            timeout.as_secs()
        );
    }
    error_chain(err)
}

async fn send_external_chat(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &serde_json::Value,
    timeout: Duration,
) -> Result<reqwest::Response, AppError> {
    let mut req = client.post(url).json(body).timeout(timeout);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    req.send().await.map_err(|e| {
        AppError::LlmError(format!(
            "External LLM request failed: {}",
            describe_request_error(&e, timeout)
        ))
    })
}

/// Send a chat completion to an external OpenAI-compatible endpoint (LM Studio,
/// OpenAI, OpenRouter, Ollama, ...) instead of the bundled local llama-server.
/// `base_url` is the API root (e.g. `http://localhost:1234/v1` or
/// `https://api.openai.com/v1`); `/chat/completions` is appended. Base URLs
/// entered without the `/v1` segment are retried at `/v1/chat/completions` on
/// 404. Bearer auth is added when `api_key` is non-empty.
///
/// With `images`, the user turn becomes a content-block array carrying one
/// `data:` URI per image alongside the text, in the order given. Endpoints that
/// ignore images still see the text block, so the worst case is a text-only
/// answer rather than an error.
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
    images: &[super::vision::VisionImage],
) -> Result<String, AppError> {
    // A bare string when there is nothing to look at: the array form is legal
    // either way, but some OpenAI-compatible servers only accept the string.
    let user_content = if images.is_empty() {
        json!(user)
    } else {
        // Text first, then the images in the order the caller listed them --
        // the user turn refers to them by position ("image 2"), so the order
        // here is part of the contract rather than an implementation detail.
        let mut parts = vec![json!({ "type": "text", "text": user })];
        parts.extend(images.iter().map(|img| {
            json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{};base64,{}", img.media_type, img.base64)
                }
            })
        }));
        json!(parts)
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
    let timeout = chat_timeout(!images.is_empty());
    let mut resp = send_external_chat(client, &urls[0], api_key, &body, timeout).await?;
    if resp.status().as_u16() == 404 && urls.len() > 1 {
        // A user-entered base URL can carry credentials (`user:pass@`) or a
        // `?key=` query, and this log ends up in exported diagnostics.
        log::info!(
            "[prompt-assistant] {} returned 404, retrying at {}",
            crate::commands::api::redact_url_secrets(&urls[0]),
            crate::commands::api::redact_url_secrets(&urls[1])
        );
        resp = send_external_chat(client, &urls[1], api_key, &body, timeout).await?;
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = error_detail(resp).await;
        let hint = if status.as_u16() == 404 {
            " (check that the base URL is an OpenAI-compatible API root, e.g. http://localhost:11434/v1)"
        } else {
            ""
        };
        return Err(AppError::LlmError(format!(
            "External LLM returned {status}{detail}{hint}"
        )));
    }
    let v = bounded_json(resp, RESPONSE_LIMIT, "external LLM response").await?;
    completion_content(&v, "The external model")
}

/// Pull the assistant text out of a chat completion, naming why it can be missing.
///
/// A reasoning model streams its thinking into a separate `reasoning` (Ollama)
/// or `reasoning_content` (DeepSeek, vLLM) field that is billed against the
/// same `max_tokens` budget as the answer. Give one a budget too small and it
/// returns HTTP 200 carrying thousands of tokens of thinking and an empty
/// `content`. Returning that empty string as success pushed the failure
/// downstream, where every caller had to guess at it: the prompt assistant
/// cached "" as this model's considered answer and later callers saw an
/// unexplained blank rather than a limit that wants raising.
fn completion_content(v: &serde_json::Value, source: &str) -> Result<String, AppError> {
    let choice = &v["choices"][0];
    let content = choice["message"]["content"].as_str().unwrap_or("");
    if !content.trim().is_empty() {
        return Ok(content.to_string());
    }
    let reasoned = ["reasoning", "reasoning_content"].iter().any(|k| {
        !choice["message"][*k]
            .as_str()
            .unwrap_or("")
            .trim()
            .is_empty()
    });
    if reasoned && choice["finish_reason"].as_str() == Some("length") {
        return Err(AppError::LlmError(format!(
            "{source} spent its entire token budget on reasoning and returned no answer.              Raise the token limit or choose a model that does not reason."
        )));
    }
    Err(AppError::LlmError(format!(
        "{source} returned an empty response."
    )))
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

/// The start of an error response body, formatted to follow the status in a
/// message (`": <first 300 characters>"`), or empty when there is nothing to
/// show or it must not be shown.
///
/// A custom provider's base URL is chosen by a moderator, and quoting whatever
/// answered would turn the error message into a way to read internal HTTP
/// services (routers, cloud metadata, admin panels). So bodies from private
/// and link-local addresses are withheld; the status code still is reported.
/// Loopback stays quotable: that is where self-hosted LLM servers live and
/// their error text is what makes them debuggable.
async fn error_detail(mut resp: reqwest::Response) -> String {
    if !may_quote_body(&resp) {
        return " (response body withheld: the server is on a private network address)".to_string();
    }
    let body = match read_limited(&mut resp, ERROR_BODY_LIMIT).await {
        Ok((bytes, _)) => bytes,
        Err(_) => return String::new(),
    };
    let detail: String = String::from_utf8_lossy(&body).chars().take(300).collect();
    if detail.trim().is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    }
}

/// Whether the body of `resp` may be quoted back in an error message. Checks
/// both the host the final (post-redirect) URL names and, when known, the
/// address actually connected to, so neither a redirect nor a public name
/// that resolves to a private address gets around it.
fn may_quote_body(resp: &reqwest::Response) -> bool {
    host_may_quote(resp.url()) && resp.remote_addr().is_none_or(|a| ip_may_quote(a.ip()))
}

/// Host-name half of [`may_quote_body`]. Names that only resolve inside a
/// network (single-label hosts and private-use suffixes) count as private.
fn host_may_quote(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip_may_quote(std::net::IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => ip_may_quote(std::net::IpAddr::V6(ip)),
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            if name == "localhost" || name.ends_with(".localhost") {
                return true;
            }
            const PRIVATE_SUFFIXES: &[&str] = &[
                ".local",
                ".localdomain",
                ".internal",
                ".intranet",
                ".lan",
                ".home",
                ".home.arpa",
                ".corp",
            ];
            name.contains('.') && !PRIVATE_SUFFIXES.iter().any(|s| name.ends_with(s))
        }
        None => false,
    }
}

/// Address half of [`may_quote_body`]: loopback and public addresses may be
/// quoted; private, link-local, CGNAT and unspecified ones may not.
fn ip_may_quote(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            // 100.64.0.0/10, shared address space (carrier NAT, tailnets).
            let shared = a == 100 && (64..128).contains(&b);
            v4.is_loopback()
                || !(v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast()
                    || shared)
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return ip_may_quote(std::net::IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            let unique_local = first & 0xfe00 == 0xfc00;
            let link_local = first & 0xffc0 == 0xfe80;
            v6.is_loopback() || !(v6.is_unspecified() || unique_local || link_local)
        }
    }
}

/// Read at most `limit` bytes of a body. The flag reports whether more was
/// left unread. The request's own timeout still bounds how long this takes.
async fn read_limited(
    resp: &mut reqwest::Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), reqwest::Error> {
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        let room = limit - body.len();
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}

/// Parse a JSON body of at most `limit` bytes. A provider (or whatever a
/// custom base URL points at) can otherwise stream an unbounded body into
/// memory. `what` names the response in error messages.
pub(super) async fn bounded_json(
    mut resp: reqwest::Response,
    limit: usize,
    what: &str,
) -> Result<serde_json::Value, AppError> {
    let (body, truncated) = read_limited(&mut resp, limit)
        .await
        .map_err(|e| AppError::LlmError(format!("Bad {what}: {e}")))?;
    if truncated {
        return Err(AppError::LlmError(format!(
            "Bad {what}: larger than {} MB",
            limit / (1024 * 1024)
        )));
    }
    serde_json::from_slice(&body).map_err(|e| AppError::LlmError(format!("Bad {what}: {e}")))
}

/// Send a chat completion to Anthropic's Messages API.
///
/// Anthropic is not OpenAI-compatible: the path is `/messages`, auth is the
/// `x-api-key` header rather than a bearer token, `anthropic-version` is
/// mandatory, the system prompt is a top-level field instead of a message, and
/// the answer arrives as a list of content blocks.
///
/// `images` lead the user turn, which is what Anthropic recommends: the model
/// reads the pictures before the instruction that refers to them, and in the
/// order the caller listed them so that "image 2" resolves.
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
    images: &[super::vision::VisionImage],
) -> Result<String, AppError> {
    if api_key.trim().is_empty() {
        return Err(AppError::LlmError(
            "Anthropic requires an API key. Add one in Settings > Prompt Assistant.".into(),
        ));
    }
    let user_content = if images.is_empty() {
        json!(user)
    } else {
        // Images before the text, which is what Anthropic's own guidance asks
        // for, and in the order the caller listed them: the user turn names
        // them by position.
        let mut parts: Vec<serde_json::Value> = images
            .iter()
            .map(|img| {
                json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": img.media_type,
                        "data": img.base64
                    }
                })
            })
            .collect();
        parts.push(json!({ "type": "text", "text": user }));
        json!(parts)
    };
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "temperature": 0.7,
        "messages": [ { "role": "user", "content": user_content } ]
    });
    let timeout = chat_timeout(!images.is_empty());
    let resp = client
        .post(anthropic_url(base_url, "messages"))
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| {
            AppError::LlmError(format!(
                "Anthropic request failed: {}",
                describe_request_error(&e, timeout)
            ))
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = error_detail(resp).await;
        return Err(AppError::LlmError(format!(
            "Anthropic returned {status}{detail}"
        )));
    }
    let v = bounded_json(resp, RESPONSE_LIMIT, "Anthropic response").await?;
    anthropic_text(&v)
}

/// Pull the answer out of a Messages API response.
///
/// Every text block is concatenated, since a response can lead with a
/// non-text block. A reply with no text at all is an error, for the same
/// reason [`completion_content`] gives: an empty "answer" returned as success
/// gets cached and shown downstream as if the model had meant it.
fn anthropic_text(v: &serde_json::Value) -> Result<String, AppError> {
    let blocks = v["content"].as_array();
    let text = blocks
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    if !text.trim().is_empty() {
        return Ok(text);
    }
    let stop_reason = v["stop_reason"].as_str();
    let thought = blocks.is_some_and(|blocks| {
        blocks
            .iter()
            .any(|b| b["type"] == "thinking" || b["type"] == "redacted_thinking")
    });
    if thought && stop_reason == Some("max_tokens") {
        return Err(AppError::LlmError(
            "Anthropic spent its entire token budget on thinking and returned no answer. \
             Raise the token limit or choose a model without extended thinking."
                .into(),
        ));
    }
    Err(AppError::LlmError(match stop_reason {
        Some(reason) => format!("Anthropic returned an empty response (stop reason: {reason})."),
        None => "Anthropic returned an empty response.".to_string(),
    }))
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
    images: &[super::vision::VisionImage],
) -> Result<String, AppError> {
    let base = super::providers::effective_base_url(provider_id, base_url);
    match super::providers::wire_for(provider_id) {
        super::providers::Wire::Companion => {
            super::companion::chat(provider_id, model, system, user, images, None).await
        }
        super::providers::Wire::Anthropic => {
            chat_anthropic(
                client, &base, api_key, model, system, user, max_tokens, images,
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
                client, &base, api_key, model, system, user, max_tokens, images,
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
    if super::companion::is_companion(provider_id) {
        return super::companion::models(provider_id).await;
    }
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
    let mut resp = send(urls[0].clone()).await.map_err(|e| {
        AppError::LlmError(format!(
            "Model list request failed: {}",
            describe_request_error(&e, Duration::from_secs(30))
        ))
    })?;
    if resp.status().as_u16() == 404 && urls.len() > 1 {
        resp = send(urls[1].clone()).await.map_err(|e| {
            AppError::LlmError(format!(
                "Model list request failed: {}",
                describe_request_error(&e, Duration::from_secs(30))
            ))
        })?;
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = error_detail(resp).await;
        return Err(AppError::LlmError(format!(
            "Model list returned {status}{detail}"
        )));
    }
    let v = bounded_json(resp, RESPONSE_LIMIT, "model list response").await?;
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

/// A fresh bearer key for one llama-server launch: 32 random bytes, hex.
fn random_api_key() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let bytes: [u8; 32] = std::array::from_fn(|_| rng.random::<u8>());
    hex::encode(bytes)
}

/// Stream `url` to `dest` and return the SHA-256 of what was written, as
/// lowercase hex, so the caller can check it before trusting the file.
async fn download_with_progress(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
    progress: &(dyn Fn(&str, u64, u64, bool) + Sync),
) -> Result<String, AppError> {
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
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
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
            sha2::Digest::update(&mut hasher, &chunk);
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
            Ok(hex::encode(sha2::Digest::finalize(hasher)))
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

#[cfg(test)]
mod tests {
    use super::{chat_timeout, completion_content, error_chain};
    use serde_json::json;
    use std::time::Duration;

    #[derive(Debug)]
    struct Outer(std::io::Error);

    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "error sending request for url (https://api.x.ai/v1)")
        }
    }

    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn error_chain_appends_the_hidden_cause() {
        let e = Outer(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset by peer",
        ));
        assert_eq!(
            error_chain(&e),
            "error sending request for url (https://api.x.ai/v1): connection reset by peer"
        );
    }

    #[test]
    fn error_chain_without_a_source_is_just_the_message() {
        let e = std::io::Error::other("plain");
        assert_eq!(error_chain(&e), "plain");
    }

    #[test]
    fn vision_requests_get_a_longer_budget() {
        assert!(chat_timeout(true) > chat_timeout(false));
        assert_eq!(chat_timeout(false), Duration::from_secs(120));
    }

    #[test]
    fn returns_the_message_content() {
        let v = json!({ "choices": [{ "message": { "content": "BASE: a girl" } }] });
        assert_eq!(completion_content(&v, "The model").unwrap(), "BASE: a girl");
    }

    #[test]
    fn content_wins_even_when_the_model_also_reasoned() {
        let v = json!({
            "choices": [{
                "finish_reason": "stop",
                "message": { "content": "BASE: a girl", "reasoning": "let me think" }
            }]
        });
        assert_eq!(completion_content(&v, "The model").unwrap(), "BASE: a girl");
    }

    #[test]
    fn budget_spent_on_reasoning_names_the_token_limit() {
        let v = json!({
            "choices": [{
                "finish_reason": "length",
                "message": { "content": "", "reasoning": "thinking at length" }
            }]
        });
        let err = completion_content(&v, "The model").unwrap_err().to_string();
        assert!(err.contains("token budget on reasoning"), "{err}");
    }

    #[test]
    fn reasoning_content_is_recognised_too() {
        let v = json!({
            "choices": [{
                "finish_reason": "length",
                "message": { "content": "", "reasoning_content": "thinking at length" }
            }]
        });
        let err = completion_content(&v, "The model").unwrap_err().to_string();
        assert!(err.contains("token budget on reasoning"), "{err}");
    }

    #[test]
    fn empty_without_reasoning_is_a_plain_empty_response() {
        let v = json!({ "choices": [{ "finish_reason": "stop", "message": { "content": "  " } }] });
        let err = completion_content(&v, "The model").unwrap_err().to_string();
        assert!(err.contains("empty response"), "{err}");
    }

    #[test]
    fn a_response_with_no_choices_is_an_error_not_a_panic() {
        let err = completion_content(&json!({}), "The model")
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty response"), "{err}");
    }
}

#[cfg(test)]
mod hardening_tests {
    use super::*;
    use std::net::IpAddr;

    fn response(body: &'static str, url: &str) -> reqwest::Response {
        use reqwest::ResponseBuilderExt;
        let http = axum::http::Response::builder()
            .url(url::Url::parse(url).unwrap())
            .body(body)
            .unwrap();
        reqwest::Response::from(http)
    }

    #[test]
    fn the_asset_for_this_platform_has_a_pinned_checksum() {
        for backend in [Backend::Vulkan, Backend::Metal, Backend::Cpu] {
            let asset = assets_for(backend);
            assert!(pinned_sha256(&asset).is_some(), "{asset} is not pinned");
        }
    }

    #[test]
    fn pinned_checksums_belong_to_the_pinned_release_and_are_sha256() {
        for (asset, digest) in LLAMA_ASSET_SHA256 {
            // A release bump that forgets this table fails here rather than
            // refusing every download at runtime.
            assert!(
                asset.starts_with(&format!("llama-{LLAMA_RELEASE}-")),
                "{asset}"
            );
            assert_eq!(digest.len(), 64, "{asset}");
            assert!(digest.chars().all(|c| c.is_ascii_hexdigit()), "{asset}");
        }
        assert!(pinned_sha256("llama-b7100-bin-evil.zip").is_none());
    }

    #[test]
    fn api_keys_are_random_and_long() {
        let a = random_api_key();
        assert_eq!(a.len(), 64);
        assert_ne!(a, random_api_key());
    }

    #[tokio::test]
    async fn a_chat_against_a_retired_port_is_refused_without_a_request() {
        let server = LlamaServer::new(std::env::temp_dir(), true);
        // Nothing is listening and no port is published: the check must fail
        // before any connection is attempted, and release its in-flight slot.
        let err = server
            .chat(&reqwest::Client::new(), 9, "s", "u", 16)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("stopped or switched"), "{err}");
        assert_eq!(server.inflight.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn private_and_link_local_addresses_are_not_quoted() {
        for ip in [
            "10.0.0.5",
            "172.16.1.1",
            "192.168.1.50",
            "169.254.169.254",
            "100.100.100.100",
            "0.0.0.0",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
            "::",
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(!ip_may_quote(ip), "{ip}");
        }
        for ip in ["127.0.0.1", "::1", "8.8.8.8", "2606:4700::1111"] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(ip_may_quote(ip), "{ip}");
        }
    }

    #[test]
    fn internal_host_names_are_not_quoted() {
        for url in [
            "http://192.168.1.10:11434/v1",
            "http://[fe80::1]:80/",
            "http://metadata.google.internal/computeMetadata/v1/",
            "http://router.lan/",
            "http://nas.local:8080/",
            "http://ollama:11434/v1",
        ] {
            assert!(!host_may_quote(&url::Url::parse(url).unwrap()), "{url}");
        }
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:1234/v1",
            "https://api.openai.com/v1",
        ] {
            assert!(host_may_quote(&url::Url::parse(url).unwrap()), "{url}");
        }
    }

    #[tokio::test]
    async fn error_bodies_from_private_targets_are_withheld() {
        let detail = error_detail(response("secret admin page", "http://10.0.0.1/x")).await;
        assert!(!detail.contains("secret"), "{detail}");
        let detail = error_detail(response("model not found", "http://127.0.0.1:1/x")).await;
        assert_eq!(detail, ": model not found");
    }

    #[tokio::test]
    async fn error_detail_reads_only_a_bounded_prefix() {
        let big: &'static str = Box::leak("x".repeat(ERROR_BODY_LIMIT * 4).into_boxed_str());
        let detail = error_detail(response(big, "https://api.example.com/v1")).await;
        assert_eq!(detail.len(), ": ".len() + 300);
    }

    #[tokio::test]
    async fn oversized_json_is_refused() {
        let ok = bounded_json(response(r#"{"a":1}"#, "https://x.example/"), 64, "test")
            .await
            .unwrap();
        assert_eq!(ok["a"], 1);
        let err = bounded_json(
            response(r#"{"a":"0123456789"}"#, "https://x.example/"),
            8,
            "test",
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("larger than"), "{err}");
    }

    #[test]
    fn anthropic_text_joins_text_blocks() {
        let v = serde_json::json!({
            "content": [
                { "type": "thinking", "thinking": "hmm" },
                { "type": "text", "text": "BASE: " },
                { "type": "text", "text": "a girl" }
            ]
        });
        assert_eq!(anthropic_text(&v).unwrap(), "BASE: a girl");
    }

    #[test]
    fn anthropic_without_text_is_an_error() {
        let err = anthropic_text(&serde_json::json!({ "content": [], "stop_reason": "refusal" }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty response"), "{err}");
        assert!(err.contains("refusal"), "{err}");
        let err = anthropic_text(&serde_json::json!({
            "content": [{ "type": "thinking", "thinking": "long" }],
            "stop_reason": "max_tokens"
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("token budget"), "{err}");
        assert!(anthropic_text(&serde_json::json!({})).is_err());
    }
}
