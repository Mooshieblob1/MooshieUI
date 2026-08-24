use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::error::AppError;
use crate::interrogator::{InterrogationResult, InterrogatorModelStatus};
use crate::state::AppState;

/// Settings for one interrogation run: which model, and the two thresholds.
struct RunSettings {
    model_id: String,
    general_threshold: f32,
    character_threshold: f32,
}

/// Read the selected model and thresholds, then make sure the model files and
/// the shared ONNX Runtime library are present.
///
/// The interrogator's own lock is only held long enough to clone the root path —
/// the multi-second network downloads run without a guard held across an await.
async fn prepare_run(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
) -> Result<RunSettings, AppError> {
    let settings = {
        let config = state.config.read().await;
        RunSettings {
            model_id: crate::interrogator::find_model(&config.interrogator_model)
                .id
                .to_string(),
            general_threshold: config.interrogator_general_threshold,
            character_threshold: config.interrogator_character_threshold,
        }
    };

    let root_dir = { state.interrogator.read().await.root_dir() };
    let model_dir = root_dir.join(&settings.model_id);

    if !crate::interrogator::is_model_downloaded_at(&model_dir) {
        crate::interrogator::ensure_model_downloaded_at(
            app,
            &state.http_client,
            &model_dir,
            &settings.model_id,
        )
        .await?;
    }
    if !crate::interrogator::is_ort_library_present_at(&root_dir) {
        crate::interrogator::ensure_ort_library_at(app, &state.http_client, &root_dir).await?;
    }

    Ok(settings)
}

/// Shared helper: ensure model downloaded, read thresholds, run inference on blocking thread.
async fn run_interrogation(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
    image_bytes: Vec<u8>,
) -> Result<InterrogationResult, AppError> {
    let settings = prepare_run(app, state).await?;

    let app2 = app.clone();
    let interrogator = state.interrogator.clone();
    tokio::task::spawn_blocking(move || {
        let mut guard = interrogator.blocking_write();
        // Switching models drops any cached session, so this must precede the
        // load check or a stale session would be reported as already loaded.
        guard.set_model(&settings.model_id);
        let is_first_load = guard.session_not_loaded();
        if is_first_load {
            app2.emit("interrogator:stage", "loading_model").ok();
        }
        guard.load_session()?;
        app2.emit("interrogator:stage", "running_inference").ok();
        guard.run_inference(
            &image_bytes,
            settings.general_threshold,
            settings.character_threshold,
        )
    })
    .await
    .map_err(|e| AppError::InterrogatorError(format!("Inference task failed: {}", e)))?
}

/// Shared helper that takes a DynamicImage directly (avoids decode round-trip).
async fn run_interrogation_from_image(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
    img: image::DynamicImage,
) -> Result<InterrogationResult, AppError> {
    let settings = prepare_run(app, state).await?;

    let app2 = app.clone();
    let interrogator = state.interrogator.clone();
    tokio::task::spawn_blocking(move || {
        let mut guard = interrogator.blocking_write();
        guard.set_model(&settings.model_id);
        let is_first_load = guard.session_not_loaded();
        if is_first_load {
            app2.emit("interrogator:stage", "loading_model").ok();
        }
        guard.load_session()?;
        app2.emit("interrogator:stage", "running_inference").ok();
        guard.run_inference_from_image(
            img,
            settings.general_threshold,
            settings.character_threshold,
        )
    })
    .await
    .map_err(|e| AppError::InterrogatorError(format!("Inference task failed: {}", e)))?
}

/// Accept image as base64-encoded string (much smaller than JSON number array).
#[tauri::command]
pub async fn interrogate_image(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
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
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<InterrogationResult, AppError> {
    let image_bytes = std::fs::read(&path)
        .map_err(|e| AppError::InterrogatorError(format!("Failed to read image file: {}", e)))?;
    run_interrogation(&app, &state, image_bytes).await
}

#[tauri::command]
pub async fn interrogate_gallery_image(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    filename: String,
) -> Result<InterrogationResult, AppError> {
    // Validate filename — no path traversal
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }

    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let path = dir.join(&filename);
    let image_bytes = std::fs::read(&path)?;
    run_interrogation(&app, &state, image_bytes).await
}

/// Read clipboard image natively (bypasses WebView clipboard restrictions) and run interrogation.
#[tauri::command]
pub async fn interrogate_clipboard(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
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

/// List the selectable taggers along with whether each is already downloaded.
#[tauri::command]
pub async fn list_interrogator_models(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<InterrogatorModelStatus>, AppError> {
    let root_dir = { state.interrogator.read().await.root_dir() };
    Ok(crate::interrogator::model_statuses_at(&root_dir))
}

/// Delete a downloaded tagger's files to reclaim disk space. Deleting the model
/// that is currently loaded also drops the cached session, so the next run
/// re-downloads it rather than inferring against files that no longer exist.
#[tauri::command]
pub async fn delete_interrogator_model(
    state: State<'_, Arc<AppState>>,
    model_id: String,
) -> Result<(), AppError> {
    let root_dir = { state.interrogator.read().await.root_dir() };
    crate::interrogator::delete_model_files_at(&root_dir, &model_id)?;

    let mut guard = state.interrogator.write().await;
    if guard.model_id() == crate::interrogator::find_model(&model_id).id {
        guard.unload_session();
    }
    Ok(())
}
