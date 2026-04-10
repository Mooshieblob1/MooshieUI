use std::time::Duration;

use crate::config::ServerMode;
use crate::error::AppError;
use crate::state::AppState;

/// Detect whether the system has a Blackwell (compute capability 12.x) NVIDIA GPU.
/// Returns `true` if any installed GPU has compute capability >= 12.0.
fn has_blackwell_gpu() -> bool {
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader,nounits"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if let Some((major_str, _)) = trimmed.split_once('.') {
                    if let Ok(major) = major_str.parse::<u32>() {
                        if major >= 12 {
                            log::info!("Detected Blackwell GPU (compute capability {})", trimmed);
                            return true;
                        }
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// Returns true if the directory has at least one known model-category subdirectory.
/// If false, the directory is flat and needs per-category classification instead.
fn is_structured_model_dir(path: &std::path::Path) -> bool {
    const KNOWN_SUBDIRS: &[&str] = &[
        "checkpoints",
        "Stable-diffusion",
        "Stable-Diffusion",
        "StableDiffusion",
        "loras",
        "lora",
        "Lora",
        "LoRA",
        "LoRAs",
        "LORA",
        "Loras",
        "LyCORIS",
        "lycoris",
        "vae",
        "VAE",
        "upscale_models",
        "ESRGAN",
        "embeddings",
        "controlnet",
        "ControlNet",
        "clip",
        "unet",
        "diffusion_models",
        "text_encoders",
    ];
    KNOWN_SUBDIRS.iter().any(|sub| path.join(sub).is_dir())
}

/// Infer the ComfyUI model category for a flat directory (no known subdirs)
/// by inspecting the directory name. Defaults to "loras" since that is the
/// most common fringe case (users pointing at a standalone LoRA collection).
fn classify_flat_model_dir(path: &std::path::Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    if name.contains("lora") || name.contains("lycoris") {
        "loras"
    } else if name.contains("checkpoint")
        || name.contains("ckpt")
        || name.contains("stable-diffusion")
        || name.contains("stablediffusion")
    {
        "checkpoints"
    } else if name.contains("vae") {
        "vae"
    } else if name.contains("upscale") || name.contains("esrgan") {
        "upscale_models"
    } else if name.contains("controlnet") || name.contains("control_net") {
        "controlnet"
    } else if name.contains("embed") || name.contains("textual") {
        "embeddings"
    } else if name.contains("clip") {
        "clip"
    } else if name.contains("unet") || name.contains("diffusion") {
        "diffusion_models"
    } else {
        // Unknown flat directory — default to loras (most common fringe case)
        "loras"
    }
}

/// Possible outcomes of starting the ComfyUI process.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartResult {
    /// Server was already running (external instance).
    AlreadyRunning,
    /// Process was spawned; the caller should poll for readiness.
    Spawned,
    /// Server mode is Remote — nothing to do.
    Skipped,
}

/// Spawn the ComfyUI process (or detect an already-running one).
/// Returns immediately — does NOT wait for the server to become ready.
pub async fn start_comfyui_process(state: &AppState) -> Result<StartResult, AppError> {
    let config = state.config.read().await;

    // Deploy bundled custom nodes whenever we have a valid ComfyUI path,
    // regardless of server mode — the user may have started ComfyUI externally
    // but still needs our nodes installed.
    if !config.comfyui_path.is_empty() {
        let main_exists = std::path::Path::new(&config.comfyui_path)
            .join("main.py")
            .exists();
        if main_exists {
            super::nodes::ensure_mooshie_nodes(&config.comfyui_path)
                .map_err(AppError::ProcessSpawnFailed)?;
        }
    }

    if config.server_mode != ServerMode::AutoLaunch {
        return Ok(StartResult::Skipped);
    }

    // Check if something is already listening on the target port (e.g. a container)
    let health_url = format!("{}/system_stats", config.server_url);
    if state.http_client.get(&health_url).send().await.is_ok() {
        log::info!(
            "ComfyUI already running at {}, skipping spawn",
            config.server_url
        );
        return Ok(StartResult::AlreadyRunning);
    }

    #[cfg(target_os = "windows")]
    let python_path = format!("{}/Scripts/python.exe", config.venv_path);
    #[cfg(not(target_os = "windows"))]
    let python_path = format!("{}/bin/python", config.venv_path);
    let main_path = format!("{}/main.py", config.comfyui_path);

    // Detect stale venv: the uv trampoline executables and pyvenv.cfg embed
    // absolute paths to the Python installation.  When the user moves their
    // data directory (externally, not via the in-app Move feature), these
    // paths break and uv reports "entity not found (os error 2)".
    //
    // We detect this by:
    //   1. Python binary doesn't exist at all, OR
    //   2. pyvenv.cfg `home` key points to a non-existent directory
    // In either case, re-run `uv venv --allow-existing` to fix the paths.
    if !config.venv_path.is_empty() {
        let needs_repair = if !std::path::Path::new(&python_path).exists() {
            true
        } else {
            // Check pyvenv.cfg for stale `home` path
            let pyvenv_cfg = std::path::Path::new(&config.venv_path).join("pyvenv.cfg");
            if let Ok(content) = std::fs::read_to_string(&pyvenv_cfg) {
                content.lines().any(|line| {
                    if let Some(value) = line
                        .strip_prefix("home")
                        .and_then(|s| s.strip_prefix('=').or_else(|| s.strip_prefix(" =")))
                    {
                        let home_dir = value.trim();
                        !home_dir.is_empty() && !std::path::Path::new(home_dir).exists()
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        };

        if needs_repair {
            let venv_dir = std::path::Path::new(&config.venv_path);
            if venv_dir.exists() {
                log::warn!(
                    "Stale venv detected (python='{}') — attempting repair",
                    python_path
                );
                if let Some(base) = venv_dir.parent() {
                    let uv = {
                        #[cfg(target_os = "windows")]
                        {
                            base.join("bin").join("uv.exe")
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            base.join("bin").join("uv")
                        }
                    };
                    let python_dir = base.join("python");
                    if uv.exists() {
                        let python_dir_str = python_dir.to_string_lossy().to_string();
                        let venv_str = config.venv_path.clone();
                        let uv_str = uv.to_string_lossy().to_string();
                        let mut repair = tokio::process::Command::new(&uv_str);
                        repair
                            .args(["venv", &venv_str, "--python", "3.11", "--allow-existing"])
                            .env("UV_PYTHON_INSTALL_DIR", &python_dir_str)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null());
                        #[cfg(target_os = "windows")]
                        {
                            #[allow(unused_imports)]
                            use std::os::windows::process::CommandExt;
                            repair.creation_flags(0x08000000); // CREATE_NO_WINDOW
                        }
                        match repair.status().await {
                            Ok(s) if s.success() => {
                                log::info!("Venv repair succeeded");
                            }
                            Ok(s) => {
                                log::warn!("Venv repair: uv exited with {}", s);
                            }
                            Err(e) => {
                                log::warn!("Venv repair failed: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    // Validate paths before attempting to spawn
    if config.venv_path.is_empty() || !std::path::Path::new(&python_path).exists() {
        return Err(AppError::ProcessSpawnFailed(format!(
            "Python not found at '{}'. Run setup first or check your venv_path config.",
            python_path
        )));
    }
    if config.comfyui_path.is_empty() || !std::path::Path::new(&main_path).exists() {
        return Err(AppError::ProcessSpawnFailed(format!(
            "ComfyUI main.py not found at '{}'. Run setup first or check your comfyui_path config.",
            main_path
        )));
    }

    log::info!("Spawning ComfyUI: {} {}", python_path, main_path);

    let mut cmd = tokio::process::Command::new(&python_path);
    cmd.arg(&main_path)
        .arg("--listen")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(config.server_port.to_string());

    // Enable latent previews over WebSocket
    cmd.arg("--preview-method").arg("auto");

    // VRAM management flag
    match config.vram_mode.as_str() {
        "high" => {
            cmd.arg("--highvram");
        }
        "low" => {
            cmd.arg("--lowvram");
        }
        "none" => {
            cmd.arg("--novram");
        }
        // "normal" and "auto" use ComfyUI's default behavior
        _ => {}
    }

    // Auto-apply --bf16-vae for Blackwell GPUs to prevent NaN/black images
    // from fp16 VAE overflow, unless the user has already set a VAE precision flag.
    let has_vae_flag = config.extra_args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--bf16-vae" | "--fp16-vae" | "--fp32-vae" | "--cpu-vae"
        )
    });
    if !has_vae_flag && has_blackwell_gpu() {
        cmd.arg("--bf16-vae");
        log::info!("Auto-applied --bf16-vae for Blackwell GPU");
    }

    // Shared model directory support (newline-separated for multiple directories)
    // Generates a YAML config for ComfyUI's --extra-model-paths-config flag.
    //
    // Two modes per directory:
    // 1. **Structured** — directory has recognizable model subdirectories
    //    (e.g. loras/, checkpoints/).  Each category scans named subdirs only.
    // 2. **Flat** — directory contains .safetensors/.ckpt files directly with
    //    no recognizable subdirectories.  We infer the category from the
    //    directory name and add "." ONLY for that category, preventing cross-
    //    contamination that the old blanket "." caused (fixed in v0.3.4).
    if let Some(ref model_dirs_str) = config.extra_model_paths {
        let dirs: Vec<&str> = model_dirs_str
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        if !dirs.is_empty() {
            let yaml_path = std::env::temp_dir().join("mooshieui_extra_model_paths.yaml");
            let mut yaml_content = String::new();
            for (i, dir) in dirs.iter().enumerate() {
                let dir_path = std::path::Path::new(dir);
                // Escape YAML values: quote paths that contain spaces, colons,
                // backslashes, or other special characters.
                let quoted_dir = format!("\"{}\"", dir.replace('\\', "\\\\").replace('"', "\\\""));

                // Check if this directory looks structured (has known model subdirs)
                let is_structured = is_structured_model_dir(dir_path);

                if is_structured {
                    // Structured directory: scan named subdirectories per category
                    yaml_content.push_str(&format!(
                        concat!(
                            "mooshieui_{idx}:\n",
                            "  base_path: {dir}\n",
                            "  checkpoints: |\n",
                            "    checkpoints\n",
                            "    Stable-diffusion\n",
                            "    Stable-Diffusion\n",
                            "    StableDiffusion\n",
                            "    models/Stable-diffusion\n",
                            "    Models/Stable-Diffusion\n",
                            "    Models/StableDiffusion\n",
                            "    dlbackend/comfyui/models/checkpoints\n",
                            "  vae: |\n",
                            "    vae\n",
                            "    VAE\n",
                            "    models/VAE\n",
                            "    Models/VAE\n",
                            "    dlbackend/comfyui/models/vae\n",
                            "  loras: |\n",
                            "    loras\n",
                            "    lora\n",
                            "    Lora\n",
                            "    LoRA\n",
                            "    LoRAs\n",
                            "    LORA\n",
                            "    Loras\n",
                            "    LyCORIS\n",
                            "    lycoris\n",
                            "    models/Lora\n",
                            "    models/loras\n",
                            "    models/LyCORIS\n",
                            "    Models/Lora\n",
                            "    Models/loras\n",
                            "    Models/LyCORIS\n",
                            "    dlbackend/comfyui/models/loras\n",
                            "  upscale_models: |\n",
                            "    upscale_models\n",
                            "    ESRGAN\n",
                            "    models/ESRGAN\n",
                            "    models/RealESRGAN\n",
                            "    Models/ESRGAN\n",
                            "    Models/RealESRGAN\n",
                            "    dlbackend/comfyui/models/upscale_models\n",
                            "  embeddings: |\n",
                            "    embeddings\n",
                            "    models/TextualInversion\n",
                            "    Models/TextualInversion\n",
                            "    dlbackend/comfyui/models/embeddings\n",
                            "  controlnet: |\n",
                            "    controlnet\n",
                            "    ControlNet\n",
                            "    models/ControlNet\n",
                            "    Models/ControlNet\n",
                            "    dlbackend/comfyui/models/controlnet\n",
                            "  clip: |\n",
                            "    clip\n",
                            "    models/clip\n",
                            "    Models/clip\n",
                            "    dlbackend/comfyui/models/clip\n",
                            "  unet: |\n",
                            "    unet\n",
                            "    models/unet\n",
                            "    Models/unet\n",
                            "    dlbackend/comfyui/models/unet\n",
                            "  diffusion_models: |\n",
                            "    diffusion_models\n",
                            "    models/diffusion_models\n",
                            "    Models/diffusion_models\n",
                            "    dlbackend/comfyui/models/diffusion_models\n",
                            "  text_encoders: |\n",
                            "    text_encoders\n",
                            "    models/text_encoders\n",
                            "    Models/text_encoders\n",
                            "    dlbackend/comfyui/models/text_encoders\n",
                        ),
                        idx = i + 1,
                        dir = quoted_dir
                    ));
                } else {
                    // Flat directory: infer category from directory name and only
                    // expose "." for that single category to avoid cross-contamination.
                    let category = classify_flat_model_dir(dir_path);
                    log::info!(
                        "Flat model directory {:?} classified as {:?}",
                        dir,
                        category
                    );
                    yaml_content.push_str(&format!(
                        "mooshieui_{idx}:\n  base_path: {dir}\n  {cat}: \".\"\n",
                        idx = i + 1,
                        dir = quoted_dir,
                        cat = category,
                    ));
                }
            }
            if let Err(e) = std::fs::write(&yaml_path, &yaml_content) {
                log::warn!("Failed to write extra_model_paths.yaml: {}", e);
            } else {
                cmd.arg("--extra-model-paths-config").arg(&yaml_path);
                log::info!("Using {} extra model path(s)", dirs.len());
            }
        }
    }

    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    // When running inside an AppImage, the bundled LD_LIBRARY_PATH and LD_PRELOAD
    // can interfere with Python/PyTorch. Clear them for the child process so it
    // uses the system's native libraries (CUDA, ROCm, etc.).
    #[cfg(target_os = "linux")]
    {
        if std::env::var("APPIMAGE").is_ok() {
            cmd.env_remove("LD_LIBRARY_PATH");
            cmd.env_remove("LD_PRELOAD");
            cmd.env_remove("PYTHONHOME");
            cmd.env_remove("PYTHONPATH");
            cmd.env_remove("PYTHONDONTWRITEBYTECODE");
            cmd.env_remove("GDK_BACKEND");
            // Preserve the real PATH but remove AppImage-internal paths
            if let Ok(path) = std::env::var("PATH") {
                let filtered: Vec<&str> = path
                    .split(':')
                    .filter(|p| !p.contains("/tmp/.mount_"))
                    .collect();
                cmd.env("PATH", filtered.join(":"));
            }
        }
    }

    // Hide the console window on Windows so ComfyUI doesn't pop up a terminal
    #[cfg(target_os = "windows")]
    {
        #[allow(unused_imports)]
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // Log ComfyUI output to a temp file for debugging
    let log_path = std::env::temp_dir().join("comfyui-desktop-stderr.log");
    let log_file = std::fs::File::create(&log_path).ok();
    log::info!("ComfyUI log: {}", log_path.display());

    cmd.stdout(std::process::Stdio::null())
        .stderr(match log_file {
            Some(f) => std::process::Stdio::from(f),
            None => std::process::Stdio::null(),
        })
        .kill_on_drop(!config.keep_alive);

    let child = cmd
        .spawn()
        .map_err(|e| AppError::ProcessSpawnFailed(e.to_string()))?;

    *state.comfyui_process.lock().await = Some(child);

    Ok(StartResult::Spawned)
}

/// Poll until the ComfyUI HTTP server responds on `/system_stats`.
/// Returns `Ok(())` once ready, or an error after the timeout.
/// Also checks if the child process has exited early (crash), and if so,
/// reads the stderr log for diagnostic information.
pub async fn wait_for_ready(state: &AppState, timeout_secs: u64) -> Result<(), AppError> {
    let url = format!("{}/system_stats", state.base_url().await);
    let iterations = timeout_secs * 2; // 500ms per iteration

    for i in 0..iterations {
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check if the server is responding
        if state.http_client.get(&url).send().await.is_ok() {
            return Ok(());
        }

        // Every 2 seconds, check if the child process has already exited (crashed)
        if i % 4 == 3 {
            let mut process = state.comfyui_process.lock().await;
            if let Some(ref mut child) = *process {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        *process = None;
                        let log_excerpt = read_comfyui_log_tail(30);
                        let msg = if let Some(ref log) = log_excerpt {
                            // Detect PyTorch installed without GPU support
                            if log.contains("Torch not compiled with CUDA enabled")
                                || log.contains("CUDA not available")
                            {
                                "PyTorch was installed without GPU support. \
                                 The CPU-only version of PyTorch was installed instead of the GPU-accelerated version. \
                                 Go to Settings → Advanced → Reinstall PyTorch to fix this, \
                                 or re-run setup and make sure the correct GPU type is selected. \
                                 Note: AMD GPU acceleration (ROCm) is only available on Linux."
                                    .to_string()
                            } else {
                                format!(
                                    "ComfyUI process exited with {} — last log output:\n{}",
                                    status, log
                                )
                            }
                        } else {
                            format!("ComfyUI process exited with {}", status)
                        };
                        return Err(AppError::ProcessSpawnFailed(msg));
                    }
                    Ok(None) => {} // Still running, keep waiting
                    Err(e) => {
                        log::warn!("Failed to check ComfyUI process status: {}", e);
                    }
                }
            }
        }
    }

    // Timeout — read logs for diagnostics
    let log_excerpt = read_comfyui_log_tail(30);
    let msg = if let Some(log) = log_excerpt {
        format!(
            "ComfyUI did not start within {} seconds — last log output:\n{}",
            timeout_secs, log
        )
    } else {
        format!("ComfyUI did not start within {} seconds", timeout_secs)
    };
    Err(AppError::ConnectionFailed(msg))
}

/// Read the last N lines from the ComfyUI stderr log file for diagnostics.
fn read_comfyui_log_tail(lines: usize) -> Option<String> {
    let log_path = std::env::temp_dir().join("comfyui-desktop-stderr.log");
    let content = std::fs::read_to_string(&log_path).ok()?;
    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    let tail: Vec<&str> = all_lines[start..].to_vec();
    if tail.is_empty() {
        None
    } else {
        Some(tail.join("\n"))
    }
}

pub async fn stop_comfyui_process(state: &AppState) -> Result<(), AppError> {
    let port = state.config.read().await.server_port;

    // Disconnect WebSocket first
    {
        let mut ws = state.ws_handle.lock().await;
        if let Some(h) = ws.take() {
            h.abort();
        }
    }

    // Kill our child process if we have one
    {
        let mut process = state.comfyui_process.lock().await;
        if let Some(ref mut child) = *process {
            child.kill().await.ok();
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            *process = None;
        }
    }

    // If something is still listening on the port (external process or race),
    // kill it by port number
    kill_process_on_port(port).await;

    // Wait for the port to actually be free
    let health_url = format!("http://127.0.0.1:{}/system_stats", port);
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if state.http_client.get(&health_url).send().await.is_err() {
            return Ok(()); // Port is free
        }
    }

    log::warn!("Port {} still in use after stop attempts", port);
    Ok(())
}

/// Find and kill any process listening on the given port.
async fn kill_process_on_port(port: u16) {
    #[cfg(target_os = "linux")]
    {
        // fuser -k sends SIGKILL to all processes using the port
        let _ = tokio::process::Command::new("fuser")
            .args(["-k", &format!("{}/tcp", port)])
            .output()
            .await;
    }
    #[cfg(target_os = "macos")]
    {
        // lsof to find PID, then kill
        if let Ok(output) = tokio::process::Command::new("lsof")
            .args(["-ti", &format!(":{}", port)])
            .output()
            .await
        {
            let pids = String::from_utf8_lossy(&output.stdout);
            for pid in pids.lines() {
                if let Ok(pid) = pid.trim().parse::<u32>() {
                    let _ = tokio::process::Command::new("kill")
                        .args(["-9", &pid.to_string()])
                        .output()
                        .await;
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        #[allow(unused_imports)]
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        // Find PID with netstat, then taskkill
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.args([
            "/C",
            &format!("netstat -ano | findstr :{} | findstr LISTENING", port),
        ]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        if let Ok(output) = cmd.output().await {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if let Some(pid) = line.split_whitespace().last() {
                    if let Ok(_pid) = pid.parse::<u32>() {
                        let mut kill_cmd = tokio::process::Command::new("taskkill");
                        kill_cmd.args(["/F", "/PID", pid]);
                        kill_cmd.creation_flags(CREATE_NO_WINDOW);
                        let _ = kill_cmd.output().await;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-GPU worker process management
// ---------------------------------------------------------------------------

use crate::comfyui::gpu_manager::{GpuWorker, WorkerStatus};
use std::sync::Arc;

/// Start a ComfyUI process for a specific GPU worker.
/// Sets CUDA_VISIBLE_DEVICES to pin the process to the worker's GPU.
pub async fn start_worker_process(
    state: &AppState,
    worker: &Arc<GpuWorker>,
) -> Result<(), AppError> {
    let config = state.config.read().await;

    // Deploy custom nodes (same as single-process path)
    if !config.comfyui_path.is_empty() {
        let main_exists = std::path::Path::new(&config.comfyui_path)
            .join("main.py")
            .exists();
        if main_exists {
            super::nodes::ensure_mooshie_nodes(&config.comfyui_path)
                .map_err(AppError::ProcessSpawnFailed)?;
        }
    }

    if config.server_mode != ServerMode::AutoLaunch {
        return Ok(());
    }

    // Check if something is already listening on the worker's port
    let health_url = format!("{}/system_stats", worker.base_url);
    if state.http_client.get(&health_url).send().await.is_ok() {
        log::info!(
            "Worker {} (GPU {}): ComfyUI already running at {}",
            worker.id,
            worker.gpu_index,
            worker.base_url,
        );
        {
            let mut status = worker.status.write().await;
            *status = WorkerStatus::Idle;
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    let python_path = format!("{}/Scripts/python.exe", config.venv_path);
    #[cfg(not(target_os = "windows"))]
    let python_path = format!("{}/bin/python", config.venv_path);
    let main_path = format!("{}/main.py", config.comfyui_path);

    if config.venv_path.is_empty() || !std::path::Path::new(&python_path).exists() {
        return Err(AppError::ProcessSpawnFailed(format!(
            "Python not found at '{}'. Run setup first.",
            python_path
        )));
    }
    if config.comfyui_path.is_empty() || !std::path::Path::new(&main_path).exists() {
        return Err(AppError::ProcessSpawnFailed(format!(
            "ComfyUI main.py not found at '{}'.",
            main_path
        )));
    }

    log::info!(
        "Worker {} (GPU {}): Spawning ComfyUI on port {}",
        worker.id,
        worker.gpu_index,
        worker.port,
    );

    {
        let mut status = worker.status.write().await;
        *status = WorkerStatus::Starting;
    }

    let mut cmd = tokio::process::Command::new(&python_path);
    cmd.arg(&main_path)
        .arg("--listen")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(worker.port.to_string());

    cmd.arg("--preview-method").arg("auto");

    // VRAM mode: worker-specific override > global config
    let vram_mode = worker
        .vram_mode
        .as_deref()
        .unwrap_or(config.vram_mode.as_str());
    match vram_mode {
        "high" => {
            cmd.arg("--highvram");
        }
        "low" => {
            cmd.arg("--lowvram");
        }
        "none" => {
            cmd.arg("--novram");
        }
        _ => {}
    }

    // bf16 VAE for Blackwell
    let has_vae_flag = config.extra_args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--bf16-vae" | "--fp16-vae" | "--fp32-vae" | "--cpu-vae"
        )
    });
    if !has_vae_flag && has_blackwell_gpu() {
        cmd.arg("--bf16-vae");
    }

    // Extra model paths (reuse the main YAML if the single-process path wrote it)
    let main_yaml = std::env::temp_dir().join("mooshieui_extra_model_paths.yaml");
    if main_yaml.exists() {
        cmd.arg("--extra-model-paths-config").arg(&main_yaml);
    }

    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    // Pin to specific GPU
    cmd.env("CUDA_VISIBLE_DEVICES", worker.gpu_index.to_string());

    // AppImage cleanup on Linux
    #[cfg(target_os = "linux")]
    {
        if std::env::var("APPIMAGE").is_ok() {
            cmd.env_remove("LD_LIBRARY_PATH");
            cmd.env_remove("LD_PRELOAD");
            cmd.env_remove("PYTHONHOME");
            cmd.env_remove("PYTHONPATH");
            cmd.env_remove("PYTHONDONTWRITEBYTECODE");
            cmd.env_remove("GDK_BACKEND");
            if let Ok(path) = std::env::var("PATH") {
                let filtered: Vec<&str> = path
                    .split(':')
                    .filter(|p| !p.contains("/tmp/.mount_"))
                    .collect();
                cmd.env("PATH", filtered.join(":"));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        #[allow(unused_imports)]
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let log_path =
        std::env::temp_dir().join(format!("comfyui-desktop-worker{}-stderr.log", worker.id));
    let log_file = std::fs::File::create(&log_path).ok();
    log::info!("Worker {} log: {}", worker.id, log_path.display());

    cmd.stdout(std::process::Stdio::null())
        .stderr(match log_file {
            Some(f) => std::process::Stdio::from(f),
            None => std::process::Stdio::null(),
        })
        .kill_on_drop(!config.keep_alive);

    let child = cmd
        .spawn()
        .map_err(|e| AppError::ProcessSpawnFailed(e.to_string()))?;

    *worker.process.lock().await = Some(child);
    Ok(())
}

/// Wait for a worker's ComfyUI process to become ready.
pub async fn wait_for_worker_ready(
    state: &AppState,
    worker: &Arc<GpuWorker>,
    timeout_secs: u64,
) -> Result<(), AppError> {
    let url = format!("{}/system_stats", worker.base_url);
    let iterations = timeout_secs * 2;

    for i in 0..iterations {
        tokio::time::sleep(Duration::from_millis(500)).await;

        if state.http_client.get(&url).send().await.is_ok() {
            let mut status = worker.status.write().await;
            *status = WorkerStatus::Idle;
            log::info!(
                "Worker {} (GPU {}): ready on port {}",
                worker.id,
                worker.gpu_index,
                worker.port
            );
            return Ok(());
        }

        // Check for crash
        if i % 4 == 3 {
            let mut process = worker.process.lock().await;
            if let Some(ref mut child) = *process {
                match child.try_wait() {
                    Ok(Some(exit_status)) => {
                        *process = None;
                        let mut status = worker.status.write().await;
                        *status = WorkerStatus::Error;
                        let log_path = std::env::temp_dir()
                            .join(format!("comfyui-desktop-worker{}-stderr.log", worker.id));
                        let log_excerpt = std::fs::read_to_string(&log_path).ok().map(|content| {
                            let lines: Vec<&str> = content.lines().collect();
                            let start = lines.len().saturating_sub(20);
                            lines[start..].join("\n")
                        });
                        let msg = match log_excerpt {
                            Some(log) => format!(
                                "Worker {} (GPU {}): process exited with {} — {}",
                                worker.id, worker.gpu_index, exit_status, log
                            ),
                            None => format!(
                                "Worker {} (GPU {}): process exited with {}",
                                worker.id, worker.gpu_index, exit_status
                            ),
                        };
                        return Err(AppError::ProcessSpawnFailed(msg));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        log::warn!("Worker {}: Failed to check process: {}", worker.id, e);
                    }
                }
            }
        }
    }

    let mut status = worker.status.write().await;
    *status = WorkerStatus::Error;
    Err(AppError::ConnectionFailed(format!(
        "Worker {} (GPU {}): did not start within {} seconds",
        worker.id, worker.gpu_index, timeout_secs
    )))
}

/// Stop a specific worker's ComfyUI process.
pub async fn stop_worker_process(worker: &Arc<GpuWorker>) -> Result<(), AppError> {
    // Abort WebSocket task
    {
        let mut ws = worker.ws_handle.lock().await;
        if let Some(h) = ws.take() {
            h.abort();
        }
    }

    // Kill child process
    {
        let mut process = worker.process.lock().await;
        if let Some(ref mut child) = *process {
            child.kill().await.ok();
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            *process = None;
        }
    }

    // Kill anything lingering on the port
    kill_process_on_port(worker.port).await;

    let mut status = worker.status.write().await;
    *status = WorkerStatus::Stopped;

    log::info!("Worker {} (GPU {}): stopped", worker.id, worker.gpu_index);
    Ok(())
}

/// Start all enabled workers.
pub async fn start_all_workers(state: &AppState) -> Vec<(u32, Result<(), AppError>)> {
    let mut results = Vec::new();
    for worker in &state.gpu_manager.workers {
        let status = *worker.status.read().await;
        if status == WorkerStatus::Disabled {
            continue;
        }
        let res = start_worker_process(state, worker).await;
        results.push((worker.id, res));
    }
    results
}

/// Wait for all starting workers to become ready (in parallel).
pub async fn wait_all_workers_ready(state: &AppState, timeout_secs: u64) {
    let mut handles = Vec::new();

    for worker in &state.gpu_manager.workers {
        let status = *worker.status.read().await;
        if status != WorkerStatus::Starting {
            continue;
        }

        let w = Arc::clone(worker);
        let http_client = state.http_client.clone();
        let base_url = w.base_url.clone();
        let worker_id = w.id;
        let gpu_index = w.gpu_index;
        let port = w.port;

        let handle = tokio::spawn(async move {
            let url = format!("{}/system_stats", base_url);
            let iterations = timeout_secs * 2;
            for i in 0..iterations {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if http_client.get(&url).send().await.is_ok() {
                    let mut status = w.status.write().await;
                    *status = WorkerStatus::Idle;
                    log::info!(
                        "Worker {} (GPU {}): ready on port {}",
                        worker_id,
                        gpu_index,
                        port
                    );
                    return Ok(worker_id);
                }
                if i % 4 == 3 {
                    let mut process = w.process.lock().await;
                    if let Some(ref mut child) = *process {
                        if let Ok(Some(_)) = child.try_wait() {
                            *process = None;
                            let mut status = w.status.write().await;
                            *status = WorkerStatus::Error;
                            return Err(worker_id);
                        }
                    }
                }
            }
            let mut status = w.status.write().await;
            *status = WorkerStatus::Error;
            Err(worker_id)
        });
        handles.push(handle);
    }

    for handle in handles {
        match handle.await {
            Ok(Ok(id)) => log::info!("Worker {} is ready", id),
            Ok(Err(id)) => log::error!("Worker {} failed to become ready", id),
            Err(e) => log::error!("Worker ready-wait task panicked: {}", e),
        }
    }
}

/// Stop all workers.
pub async fn stop_all_workers(state: &AppState) {
    for worker in &state.gpu_manager.workers {
        if let Err(e) = stop_worker_process(worker).await {
            log::error!("Failed to stop worker {}: {}", worker.id, e);
        }
    }
}
