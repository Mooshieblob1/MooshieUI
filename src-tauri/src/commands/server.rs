use tauri::{AppHandle, Emitter, Manager, State};

use crate::comfyui::process::{self, StartResult};
use crate::comfyui::types::SystemStats;
use crate::comfyui::websocket;
use crate::error::AppError;
use crate::state::AppState;

/// Start ComfyUI and return immediately with the result.
/// If the process was spawned or already running, kicks off a background task
/// that waits for the server to be ready, connects the WebSocket, and emits
/// `comfyui:server_ready`.
#[tauri::command]
pub async fn start_comfyui(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    let result = process::start_comfyui_process(&state).await?;

    match result {
        StartResult::AlreadyRunning => {
            // Server already up — connect WS and notify frontend immediately
            let app = app_handle.clone();
            tokio::spawn(async move {
                let state = app.state::<AppState>();
                if let Err(e) = websocket::connect_websocket(app.clone(), &state).await {
                    log::error!("Failed to connect WebSocket: {}", e);
                }
                let _ = app.emit("comfyui:server_ready", ());
            });
            Ok("already_running".to_string())
        }
        StartResult::Spawned => {
            // Process spawned — poll in background until ready
            let app = app_handle.clone();
            tokio::spawn(async move {
                let state = app.state::<AppState>();
                match process::wait_for_ready(&state, 120).await {
                    Ok(()) => {
                        log::info!("ComfyUI server is ready");
                        if let Err(e) = websocket::connect_websocket(app.clone(), &state).await {
                            log::error!("Failed to connect WebSocket: {}", e);
                        }
                        let _ = app.emit("comfyui:server_ready", ());
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        log::error!("ComfyUI failed to become ready: {}", err_str);
                        let _ = app.emit(
                            "comfyui:server_error",
                            serde_json::json!({
                                "error": err_str,
                                "crashed": err_str.contains("exited with"),
                            }),
                        );
                    }
                }
            });
            Ok("spawned".to_string())
        }
        StartResult::Skipped => {
            // Remote mode — just try to connect WS directly
            let app = app_handle.clone();
            tokio::spawn(async move {
                let state = app.state::<AppState>();
                if let Err(e) = websocket::connect_websocket(app.clone(), &state).await {
                    log::error!("Failed to connect WebSocket: {}", e);
                }
                let _ = app.emit("comfyui:server_ready", ());
            });
            Ok("skipped".to_string())
        }
    }
}

#[tauri::command]
pub async fn stop_comfyui(state: State<'_, AppState>) -> Result<(), AppError> {
    process::stop_comfyui_process(&state).await
}

#[tauri::command]
pub async fn check_server_health(state: State<'_, AppState>) -> Result<SystemStats, AppError> {
    state.get_system_stats_info().await
}
