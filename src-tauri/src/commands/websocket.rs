use tauri::{AppHandle, State};

use crate::comfyui::websocket;
use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
pub async fn connect_ws(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    websocket::connect_websocket(app_handle, &state).await
}

#[tauri::command]
pub async fn disconnect_ws(state: State<'_, AppState>) -> Result<(), AppError> {
    websocket::disconnect_websocket(&state).await
}
