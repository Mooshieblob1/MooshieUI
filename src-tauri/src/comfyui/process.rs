use std::time::Duration;

use crate::config::ServerMode;
use crate::error::AppError;
use crate::state::AppState;

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
        "high" => { cmd.arg("--highvram"); }
        "low" => { cmd.arg("--lowvram"); }
        "none" => { cmd.arg("--novram"); }
        // "normal" and "auto" use ComfyUI's default behavior
        _ => {}
    }

    // Shared model directory support (newline-separated for multiple directories)
    // Generates a YAML config for ComfyUI's --extra-model-paths-config flag.
    // Each category lists multiple subdirectory names to support ComfyUI, A1111,
    // Forge, and flat directory structures.
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
                // Escape YAML values: quote paths that contain spaces, colons,
                // backslashes, or other special characters.
                let quoted_dir = format!("\"{}\"", dir.replace('\\', "\\\\").replace('"', "\\\""));
                yaml_content.push_str(&format!(concat!(
                    "mooshieui_{idx}:\n",
                    "  base_path: {dir}\n",
                    "  checkpoints: |\n",
                    "    .\n",
                    "    checkpoints\n",
                    "    models/Stable-diffusion\n",
                    "    Stable-diffusion\n",
                    "  vae: |\n",
                    "    .\n",
                    "    vae\n",
                    "    models/VAE\n",
                    "    VAE\n",
                    "  loras: |\n",
                    "    .\n",
                    "    loras\n",
                    "    models/Lora\n",
                    "    models/LyCORIS\n",
                    "    Lora\n",
                    "    LyCORIS\n",
                    "  upscale_models: |\n",
                    "    .\n",
                    "    upscale_models\n",
                    "    models/ESRGAN\n",
                    "    models/RealESRGAN\n",
                    "    ESRGAN\n",
                    "  embeddings: |\n",
                    "    .\n",
                    "    embeddings\n",
                    "    models/TextualInversion\n",
                    "  controlnet: |\n",
                    "    .\n",
                    "    controlnet\n",
                    "    models/ControlNet\n",
                    "    ControlNet\n",
                    "  clip: |\n",
                    "    .\n",
                    "    clip\n",
                    "  unet: |\n",
                    "    .\n",
                    "    unet\n",
                    "  diffusion_models: |\n",
                    "    .\n",
                    "    diffusion_models\n",
                    "  text_encoders: |\n",
                    "    .\n",
                    "    text_encoders\n",
                ),
                    idx = i + 1,
                    dir = quoted_dir
                ));
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
                        let msg = if let Some(log) = log_excerpt {
                            format!(
                                "ComfyUI process exited with {} — last log output:\n{}",
                                status, log
                            )
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
        cmd.args(["/C", &format!("netstat -ano | findstr :{} | findstr LISTENING", port)]);
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
