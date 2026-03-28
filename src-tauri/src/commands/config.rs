use tauri::State;

use crate::config::{save_config, AppConfig};
use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, AppError> {
    let config = state.config.read().await;
    Ok(config.clone())
}

#[tauri::command]
pub async fn update_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), AppError> {
    save_config(&config).map_err(AppError::Other)?;
    let mut current = state.config.write().await;
    *current = config;
    Ok(())
}
