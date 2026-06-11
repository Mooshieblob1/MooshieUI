use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::comfyui::gpu_manager::WorkerStatus;
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

async fn wait_for_configured_workers_ready(
    app: AppHandle,
    state: Arc<AppState>,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
) -> Result<(), AppError> {
    process::wait_all_workers_ready(&state, 120).await;

    let mut ready_any = false;
    for worker in &state.gpu_manager.workers {
        let status = *worker.status.read().await;
        if status != WorkerStatus::Idle {
            continue;
        }
        ready_any = true;
        if let Err(e) = websocket::connect_websocket_for_worker_desktop(
            app.clone(),
            &state,
            worker,
            event_tx.clone(),
        )
        .await
        {
            log::error!("Worker {} WebSocket failed: {}", worker.id, e);
        }
    }

    if !ready_any {
        return Err(AppError::ConnectionFailed(
            "No configured GPU workers became ready".into(),
        ));
    }

    emit_both(
        &app,
        &state,
        "comfyui:server_ready",
        serde_json::json!(null),
    );
    Ok(())
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
                let configured_worker_mode = {
                    let config = state.config.read().await;
                    process::uses_configured_gpu_workers(&config)
                };
                if configured_worker_mode {
                    if let Err(e) = wait_for_configured_workers_ready(
                        app.clone(),
                        Arc::clone(&state),
                        event_tx.clone(),
                    )
                    .await
                    {
                        let err_str = e.to_string();
                        log::error!("Configured GPU workers failed to become ready: {}", err_str);
                        let port = state.config.read().await.server_port;
                        emit_both(
                            &app,
                            &state,
                            "comfyui:server_error",
                            crate::comfyui::nodes::server_error_payload(&err_str, port),
                        );
                    }
                    return;
                }
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
                        let port = state.config.read().await.server_port;
                        emit_both(
                            &app,
                            &state,
                            "comfyui:server_error",
                            crate::comfyui::nodes::server_error_payload(&err_str, port),
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

/// Kill whatever process is currently listening on the configured ComfyUI port.
/// Used by the "external instance" modal so the user can free port 8188 and retry.
#[tauri::command]
pub async fn kill_port_process(state: State<'_, Arc<AppState>>) -> Result<u16, AppError> {
    let port = state.config.read().await.server_port;
    process::kill_process_on_port(port).await;
    Ok(port)
}

#[tauri::command]
pub async fn check_server_health(state: State<'_, Arc<AppState>>) -> Result<SystemStats, AppError> {
    state.get_system_stats_info().await
}
