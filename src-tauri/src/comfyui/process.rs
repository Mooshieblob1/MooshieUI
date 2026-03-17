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

    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    cmd.stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);

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
    let mut process = state.comfyui_process.lock().await;
    if let Some(ref mut child) = *process {
        child.kill().await.ok();
        *process = None;
    }
    Ok(())
}
