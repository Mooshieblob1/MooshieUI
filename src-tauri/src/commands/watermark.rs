//! Invisible watermark setup (see `crate::watermark`).

use std::sync::Arc;

use crate::error::AppError;
use crate::state::AppState;
use tauri::State;

/// Whether the watermark models and ONNX Runtime are on disk.
#[tauri::command]
pub async fn get_watermark_status() -> Result<bool, AppError> {
    Ok(crate::watermark::is_ready())
}

/// Download (about 65 MB, once) and load what the watermark needs.
#[tauri::command]
pub async fn prepare_watermark(state: State<'_, Arc<AppState>>) -> Result<bool, AppError> {
    crate::watermark::prepare(&state.http_client).await?;
    Ok(crate::watermark::is_ready())
}
