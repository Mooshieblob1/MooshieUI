use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::error::AppError;
use crate::interrogator::{InterrogationResult, InterrogatorModelStatus};
use crate::state::AppState;

/// Settings for one interrogation run: model id, resolved directory, and thresholds.
struct RunSettings {
    model_id: String,
    model_dir: PathBuf,
    general_threshold: f32,
    character_threshold: f32,
}

/// Read the selected model, resolve its directory (custom or built-in), and make
/// sure the model files and the shared ONNX Runtime library are present.
///
/// The interrogator's own lock is only held long enough to clone the root path --
/// the multi-second network downloads run without a guard held across an await.
async fn prepare_run(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
) -> Result<RunSettings, AppError> {
    let (model_id, custom_models, general_threshold, character_threshold) = {
        let config = state.config.read().await;
        (
            config.interrogator_model.clone(),
            config.interrogator_custom_models.clone(),
            config.interrogator_general_threshold,
            config.interrogator_character_threshold,
        )
    };

    let root_dir = { state.interrogator.read().await.root_dir() };
    let model_dir = crate::interrogator::resolve_model_dir(&model_id, &root_dir, &custom_models)?;

    // Custom models already have their files on disk; only download for built-in ones.
    let is_custom = custom_models.iter().any(|m| m.id == model_id);
    if !is_custom {
        if !crate::interrogator::is_model_downloaded_at(&model_dir) {
            crate::interrogator::ensure_model_downloaded_at(
                app,
                &state.http_client,
                &model_dir,
                &model_id,
            )
            .await?;
        }
        if !crate::interrogator::is_ort_library_present_at(&root_dir) {
            crate::interrogator::ensure_ort_library_at(app, &state.http_client, &root_dir).await?;
        }
    }

    Ok(RunSettings {
        model_id,
        model_dir,
        general_threshold,
        character_threshold,
    })
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
        guard.set_model(&settings.model_id, settings.model_dir);
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
        guard.set_model(&settings.model_id, settings.model_dir);
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

/// Accept a file path and read it in Rust -- zero bytes over IPC.
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
    // Validate filename -- no path traversal
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

/// List the selectable taggers (built-in + custom) along with whether each is
/// already downloaded.
#[tauri::command]
pub async fn list_interrogator_models(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<InterrogatorModelStatus>, AppError> {
    let (root_dir, custom_models) = {
        let interrogator = state.interrogator.read().await;
        let config = state.config.read().await;
        (
            interrogator.root_dir(),
            config.interrogator_custom_models.clone(),
        )
    };
    Ok(crate::interrogator::model_statuses_with_custom(
        &root_dir,
        &custom_models,
    ))
}

/// Delete a downloaded built-in tagger's files to reclaim disk space. Deleting
/// the model that is currently loaded also drops the cached session, so the next
/// run re-downloads it rather than inferring against files that no longer exist.
/// Custom models are rejected -- use `remove_custom_interrogator_model` instead.
#[tauri::command]
pub async fn delete_interrogator_model(
    state: State<'_, Arc<AppState>>,
    model_id: String,
) -> Result<(), AppError> {
    let (root_dir, custom_models) = {
        let interrogator = state.interrogator.read().await;
        let config = state.config.read().await;
        (
            interrogator.root_dir(),
            config.interrogator_custom_models.clone(),
        )
    };
    crate::interrogator::delete_model_files_at(&root_dir, &model_id, &custom_models)?;

    let mut guard = state.interrogator.write().await;
    if guard.model_id() == model_id {
        guard.unload_session();
    }
    Ok(())
}

/// Register a local ONNX tagger folder as a custom model. The folder must
/// contain `model.onnx` and `selected_tags.csv` from a WD v3-family model.
/// The id is derived from the folder name with a "custom-" prefix.
/// MooshieUI never downloads or deletes files from this path.
#[tauri::command]
pub async fn add_custom_interrogator_model(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<(), AppError> {
    let dir = PathBuf::from(&path);
    // Resolve symlinks and remove any `..` traversal before doing I/O.
    let dir = dir
        .canonicalize()
        .map_err(|e| AppError::InterrogatorError(format!("Cannot access '{}': {}", path, e)))?;
    let path = dir.to_string_lossy().to_string();

    // Validate that the folder contains the required files.
    if !dir.join(crate::interrogator::MODEL_FILENAME).exists() {
        return Err(AppError::InterrogatorError(format!(
            "Folder '{}' does not contain model.onnx. Provide the folder that holds both model.onnx and selected_tags.csv from the same WD v3-family model release.",
            dir.display()
        )));
    }
    if !dir.join(crate::interrogator::TAGS_FILENAME).exists() {
        return Err(AppError::InterrogatorError(format!(
            "Folder '{}' does not contain selected_tags.csv. Provide the folder that holds both model.onnx and selected_tags.csv from the same WD v3-family model release.",
            dir.display()
        )));
    }

    let folder_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "custom".to_string());
    let id = format!("custom-{}", folder_name);
    let label = folder_name.clone();

    let mut config = state.config.write().await;

    // Avoid duplicate ids.
    if config.interrogator_custom_models.iter().any(|m| m.id == id) {
        return Err(AppError::InterrogatorError(format!(
            "A custom model with id '{}' is already registered.",
            id
        )));
    }

    config
        .interrogator_custom_models
        .push(crate::config::CustomInterrogatorModel { id, label, path });
    crate::config::save_config(&config).map_err(AppError::Other)?;
    Ok(())
}

/// Remove a custom model registration from the config. Never deletes any files
/// on disk -- the user's folder is left untouched.
#[tauri::command]
pub async fn remove_custom_interrogator_model(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), AppError> {
    let mut config = state.config.write().await;
    let before = config.interrogator_custom_models.len();
    config.interrogator_custom_models.retain(|m| m.id != id);
    if config.interrogator_custom_models.len() == before {
        return Err(AppError::InterrogatorError(format!(
            "No custom model with id '{}' found in config.",
            id
        )));
    }
    // If the removed model was active, fall back to the default.
    if config.interrogator_model == id {
        config.interrogator_model = crate::interrogator::DEFAULT_INTERROGATOR_MODEL.to_string();
    }
    crate::config::save_config(&config).map_err(AppError::Other)?;
    Ok(())
}
