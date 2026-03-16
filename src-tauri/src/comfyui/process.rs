use std::time::Duration;

use crate::config::ServerMode;
use crate::error::AppError;
use crate::state::AppState;

pub async fn start_comfyui_process(state: &AppState) -> Result<(), AppError> {
    let config = state.config.read().await;

    if config.server_mode != ServerMode::AutoLaunch {
        return Ok(());
    }

    let python_path = format!("{}/bin/python", config.venv_path);
    let main_path = format!("{}/main.py", config.comfyui_path);

    let mut cmd = tokio::process::Command::new(&python_path);
    cmd.arg(&main_path)
        .arg("--listen")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(config.server_port.to_string());

    for arg in &config.extra_args {
        cmd.arg(arg);
    }

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| AppError::ProcessSpawnFailed(e.to_string()))?;

    *state.comfyui_process.lock().await = Some(child);

    // Wait for server to become ready
    let url = format!("{}/system_stats", config.server_url);
    drop(config);

    for _ in 0..120 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if state.http_client.get(&url).send().await.is_ok() {
            return Ok(());
        }
    }

    Err(AppError::ConnectionFailed(
        "ComfyUI did not start within 60 seconds".to_string(),
    ))
}

pub async fn stop_comfyui_process(state: &AppState) -> Result<(), AppError> {
    let mut process = state.comfyui_process.lock().await;
    if let Some(ref mut child) = *process {
        child.kill().await.ok();
        *process = None;
    }
    Ok(())
}
