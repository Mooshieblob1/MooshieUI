use tauri::{AppHandle, Emitter, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::error::AppError;
use crate::interrogator::InterrogationResult;
use crate::state::AppState;

/// Shared helper: ensure model downloaded, read thresholds, run inference on blocking thread.
async fn run_interrogation(
    app: &AppHandle,
    state: &State<'_, AppState>,
    image_bytes: Vec<u8>,
) -> Result<InterrogationResult, AppError> {
    // Ensure model is downloaded
    {
        let interrogator = state.interrogator.read().await;
        if !interrogator.is_model_downloaded() {
            drop(interrogator);
            let interrogator = state.interrogator.read().await;
            interrogator
                .ensure_model_downloaded(app, &state.http_client)
                .await?;
        }
    }

    let (general_threshold, character_threshold) = {
        let config = state.config.read().await;
        (
            config.interrogator_general_threshold,
            config.interrogator_character_threshold,
        )
    };

    let app2 = app.clone();
    let interrogator = state.interrogator.clone();
    tokio::task::spawn_blocking(move || {
        let mut guard = interrogator.blocking_write();
        let is_first_load = guard.session_not_loaded();
        if is_first_load {
            app2.emit("interrogator:stage", "loading_model").ok();
        }
        guard.load_session()?;
        app2.emit("interrogator:stage", "running_inference").ok();
        guard.run_inference(&image_bytes, general_threshold, character_threshold)
    })
    .await
    .map_err(|e| AppError::InterrogatorError(format!("Inference task failed: {}", e)))?
}

/// Shared helper that takes a DynamicImage directly (avoids decode round-trip).
async fn run_interrogation_from_image(
    app: &AppHandle,
    state: &State<'_, AppState>,
    img: image::DynamicImage,
) -> Result<InterrogationResult, AppError> {
    {
        let interrogator = state.interrogator.read().await;
        if !interrogator.is_model_downloaded() {
            drop(interrogator);
            let interrogator = state.interrogator.read().await;
            interrogator
                .ensure_model_downloaded(app, &state.http_client)
                .await?;
        }
    }

    let (general_threshold, character_threshold) = {
        let config = state.config.read().await;
        (
            config.interrogator_general_threshold,
            config.interrogator_character_threshold,
        )
    };

    let app2 = app.clone();
    let interrogator = state.interrogator.clone();
    tokio::task::spawn_blocking(move || {
        let mut guard = interrogator.blocking_write();
        let is_first_load = guard.session_not_loaded();
        if is_first_load {
            app2.emit("interrogator:stage", "loading_model").ok();
        }
        guard.load_session()?;
        app2.emit("interrogator:stage", "running_inference").ok();
        guard.run_inference_from_image(img, general_threshold, character_threshold)
    })
    .await
    .map_err(|e| AppError::InterrogatorError(format!("Inference task failed: {}", e)))?
}

/// Accept image as base64-encoded string (much smaller than JSON number array).
#[tauri::command]
pub async fn interrogate_image(
    app: AppHandle,
    state: State<'_, AppState>,
    image_base64: String,
) -> Result<InterrogationResult, AppError> {
    use base64::Engine;
    let image_bytes = base64::engine::general_purpose::STANDARD
        .decode(&image_base64)
        .map_err(|e| AppError::InterrogatorError(format!("Invalid base64: {}", e)))?;
    run_interrogation(&app, &state, image_bytes).await
}

/// Accept a file path and read it in Rust — zero bytes over IPC.
#[tauri::command]
pub async fn interrogate_image_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<InterrogationResult, AppError> {
    let image_bytes = std::fs::read(&path)
        .map_err(|e| AppError::InterrogatorError(format!("Failed to read image file: {}", e)))?;
    run_interrogation(&app, &state, image_bytes).await
}

#[tauri::command]
pub async fn interrogate_gallery_image(
    app: AppHandle,
    state: State<'_, AppState>,
    filename: String,
) -> Result<InterrogationResult, AppError> {
    // Validate filename — no path traversal
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }

    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    let path = dir.join(&filename);
    let image_bytes = std::fs::read(&path)?;
    run_interrogation(&app, &state, image_bytes).await
}

/// Read clipboard image natively (bypasses WebView clipboard restrictions) and run interrogation.
#[tauri::command]
pub async fn interrogate_clipboard(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<InterrogationResult, AppError> {
    let clipboard_image = app
        .clipboard()
        .read_image()
        .map_err(|e| AppError::InterrogatorError(format!("No image in clipboard: {}", e)))?;

    let rgba = clipboard_image.rgba().to_vec();
    let w = clipboard_image.width();
    let h = clipboard_image.height();

    let rgba_img = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| AppError::InterrogatorError("Invalid clipboard image data".into()))?;

    let dynamic = image::DynamicImage::from(rgba_img);
    run_interrogation_from_image(&app, &state, dynamic).await
}
