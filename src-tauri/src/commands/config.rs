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

/// Return the resolved gallery directory path.
/// If a custom `gallery_path` is set, returns that; otherwise returns the default.
#[tauri::command]
pub async fn get_gallery_path(state: State<'_, AppState>) -> Result<String, AppError> {
    let cfg = state.config.read().await;
    if let Some(ref custom) = cfg.gallery_path {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    Ok(dir.to_string_lossy().into_owned())
}

/// Set a custom gallery directory. Pass an empty string to reset to default.
/// Validates the path is a writable directory (or can be created).
#[tauri::command]
pub async fn set_gallery_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<String, AppError> {
    let trimmed = path.trim().to_string();

    let resolved = if trimmed.is_empty() {
        // Reset to default
        let mut cfg = state.config.write().await;
        cfg.gallery_path = None;
        save_config(&cfg).map_err(AppError::Other)?;
        let dir = crate::config::app_data_dir()
            .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
            .join("gallery");
        std::fs::create_dir_all(&dir)?;
        dir.to_string_lossy().into_owned()
    } else {
        let p = std::path::Path::new(&trimmed);
        // Create if it doesn't exist
        std::fs::create_dir_all(p)
            .map_err(|e| AppError::Other(format!("Cannot create gallery directory: {}", e)))?;
        // Verify writable by creating a temp file
        let test_file = p.join(".mooshie_write_test");
        std::fs::write(&test_file, b"test")
            .map_err(|e| AppError::Other(format!("Directory is not writable: {}", e)))?;
        let _ = std::fs::remove_file(&test_file);

        let mut cfg = state.config.write().await;
        cfg.gallery_path = Some(trimmed.clone());
        save_config(&cfg).map_err(AppError::Other)?;
        trimmed
    };

    Ok(resolved)
}
