use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::comfyui::process::{self, StartResult};
use crate::comfyui::types::SystemStats;
use crate::comfyui::websocket;
use crate::error::AppError;
use crate::state::AppState;

/// Helper to emit to both Tauri and SSE broadcast.
fn emit_both(app: &AppHandle, state: &AppState, event: &str, payload: serde_json::Value) {
    let _ = app.emit(event, payload.clone());
    state.broadcast(event, payload);
}

/// Start ComfyUI and return immediately with the result.
/// If the process was spawned or already running, kicks off a background task
/// that waits for the server to be ready, connects the WebSocket, and emits
/// `comfyui:server_ready`.
#[tauri::command]
pub async fn start_comfyui(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<String, AppError> {
    let result = process::start_comfyui_process(&state).await?;
    let event_tx = state.event_tx.clone();

    match result {
        StartResult::AlreadyRunning => {
            // Server already up — connect WS and notify frontend immediately
            let app = app_handle.clone();
            tokio::spawn(async move {
                let state = app.state::<Arc<AppState>>();
                if let Err(e) =
                    websocket::connect_websocket(app.clone(), Arc::clone(&state), event_tx.clone())
                        .await
                {
                    log::error!("Failed to connect WebSocket: {}", e);
                }
                emit_both(
                    &app,
                    &state,
                    "comfyui:server_ready",
                    serde_json::json!(null),
                );
            });
            Ok("already_running".to_string())
        }
        StartResult::Spawned => {
            // Process spawned — poll in background until ready
            let app = app_handle.clone();
            tokio::spawn(async move {
                let state = app.state::<Arc<AppState>>();
                match process::wait_for_ready(&state, 120).await {
                    Ok(()) => {
                        log::info!("ComfyUI server is ready");
                        if let Err(e) = websocket::connect_websocket(
                            app.clone(),
                            Arc::clone(&state),
                            event_tx.clone(),
                        )
                        .await
                        {
                            log::error!("Failed to connect WebSocket: {}", e);
                        }
                        emit_both(
                            &app,
                            &state,
                            "comfyui:server_ready",
                            serde_json::json!(null),
                        );
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        log::error!("ComfyUI failed to become ready: {}", err_str);
                        emit_both(
                            &app,
                            &state,
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
                let state = app.state::<Arc<AppState>>();
                if let Err(e) =
                    websocket::connect_websocket(app.clone(), Arc::clone(&state), event_tx.clone())
                        .await
                {
                    log::error!("Failed to connect WebSocket: {}", e);
                }
                emit_both(
                    &app,
                    &state,
                    "comfyui:server_ready",
                    serde_json::json!(null),
                );
            });
            Ok("skipped".to_string())
        }
    }
}

#[tauri::command]
pub async fn stop_comfyui(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    process::stop_comfyui_process(&state).await
}

#[tauri::command]
pub async fn check_server_health(state: State<'_, Arc<AppState>>) -> Result<SystemStats, AppError> {
    state.get_system_stats_info().await
}
