use tauri::State;

use crate::comfyui::process;
use crate::comfyui::types::SystemStats;
use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
pub async fn start_comfyui(state: State<'_, AppState>) -> Result<(), AppError> {
    process::start_comfyui_process(&state).await
}

#[tauri::command]
pub async fn stop_comfyui(state: State<'_, AppState>) -> Result<(), AppError> {
    process::stop_comfyui_process(&state).await
}

#[tauri::command]
pub async fn check_server_health(state: State<'_, AppState>) -> Result<SystemStats, AppError> {
    state.get_system_stats_info().await
}
