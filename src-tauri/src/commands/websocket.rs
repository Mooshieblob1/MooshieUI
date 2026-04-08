use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::comfyui::websocket;
use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
pub async fn connect_ws(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), AppError> {
    let event_tx = state.event_tx.clone();
    websocket::connect_websocket(app_handle, &state, event_tx).await
}

#[tauri::command]
pub async fn disconnect_ws(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    websocket::disconnect_websocket(&state).await
}
