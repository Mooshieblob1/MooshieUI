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

    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    cmd.stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(!config.keep_alive);

    let child = cmd
        .spawn()
        .map_err(|e| AppError::ProcessSpawnFailed(e.to_string()))?;

    *state.comfyui_process.lock().await = Some(child);

    Ok(StartResult::Spawned)
}

/// Poll until the ComfyUI HTTP server responds on `/system_stats`.
/// Returns `Ok(())` once ready, or an error after the timeout.
pub async fn wait_for_ready(state: &AppState, timeout_secs: u64) -> Result<(), AppError> {
    let url = format!("{}/system_stats", state.base_url().await);
    let iterations = timeout_secs * 2; // 500ms per iteration

    for _ in 0..iterations {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if state.http_client.get(&url).send().await.is_ok() {
            return Ok(());
        }
    }

    Err(AppError::ConnectionFailed(format!(
        "ComfyUI did not start within {} seconds",
        timeout_secs
    )))
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
        // Find PID with netstat, then taskkill
        if let Ok(output) = tokio::process::Command::new("cmd")
            .args(["/C", &format!("netstat -ano | findstr :{} | findstr LISTENING", port)])
            .output()
            .await
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if let Some(pid) = line.split_whitespace().last() {
                    if let Ok(_pid) = pid.parse::<u32>() {
                        let _ = tokio::process::Command::new("taskkill")
                            .args(["/F", "/PID", pid])
                            .output()
                            .await;
                    }
                }
            }
        }
    }
}
