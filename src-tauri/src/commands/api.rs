use std::collections::BTreeSet;
use std::io::Read;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter, State};

use crate::comfyui::process::tokio_command_no_window;
use crate::comfyui::types::*;
use crate::error::AppError;
use crate::state::AppState;

/// Compute the full SHA256 hash of a file (uppercase hex).
/// Compatible with CivitAI's hash database.
/// For large model files (2-10 GB) this can take a few seconds.
pub(crate) fn full_sha256(path: &std::path::Path) -> Result<String, AppError> {
    const BUF_SIZE: usize = 8 * 1024 * 1024; // 8 MB read buffer
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let result = hasher.finalize();
    Ok(format!("{:X}", result))
}

/// Return the AutoV2 hash (first 10 chars of SHA256, uppercase).
/// This is the standard format used by CivitAI, A1111, Forge, etc.
pub(crate) fn autov2_hash(full_hash: &str) -> String {
    full_hash[..10].to_string()
}

#[derive(Debug, Serialize)]
pub struct ModelHashResult {
    pub sha256: String,
    pub autov2: String,
}

#[derive(Debug, Serialize)]
pub struct GalleryImageEntry {
    pub filename: String,
    pub size_bytes: u64,
    pub modified_ms: u64,
    /// Playback length for `.mp4` entries, read from the gallery index.
    /// `None` for images and for videos the index does not know about.
    pub duration_seconds: Option<f64>,
    /// Frame rate for `.mp4` entries. `None` for images and for rows that
    /// predate the `fps` column; the player falls back to 24.
    pub fps: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct CivitaiSearchParams {
    pub query: Option<String>,
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    #[serde(rename = "baseModel")]
    pub base_model: Option<String>,
    #[serde(rename = "fileFormat")]
    pub file_format: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
    pub period: Option<String>,
    pub nsfw: Option<bool>,
    pub page: Option<u32>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_models(
    state: State<'_, Arc<AppState>>,
    category: String,
) -> Result<Vec<String>, AppError> {
    state.get_models_list(&category).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_samplers(state: State<'_, Arc<AppState>>) -> Result<SamplerInfo, AppError> {
    state.get_samplers_and_schedulers().await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_embeddings(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, AppError> {
    state.get_embeddings_list().await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_queue(state: State<'_, Arc<AppState>>) -> Result<QueueInfo, AppError> {
    let mut info = state.get_queue_info().await?;
    // Augment with internal fair-queue positions so the Settings Queue section
    // works in Tauri desktop mode (raw ComfyUI /queue has no queue_positions).
    let queue_positions: Vec<serde_json::Value> = {
        let queue = state.prompt_queue.queue.read().unwrap();
        let total = queue.len();
        queue
            .iter()
            .enumerate()
            .map(|(pos, (id, _owner))| {
                serde_json::json!({
                    "prompt_id": id,
                    "position": pos,
                    "total": total,
                })
            })
            .collect()
    };
    info.queue_positions = queue_positions;
    Ok(info)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_history(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
) -> Result<Value, AppError> {
    state.get_history_for(&prompt_id).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn recover_prompt_outputs(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
) -> Result<Value, AppError> {
    let ids = state.prompt_queue.related_ids(&prompt_id);
    let mut cached = Vec::new();
    {
        let mut outputs = state.output_image_cache.write().unwrap();
        for id in &ids {
            if let Some(files) = outputs.remove(id) {
                cached.extend(files);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    cached.retain(|f| seen.insert(f.clone()));
    let mut images: Vec<serde_json::Value> = cached
        .into_iter()
        .map(|f| serde_json::json!({ "temp_filename": f }))
        .collect();
    // Regional inpaint chains may cache multiple outputs per prompt; use the latest.
    if images.len() > 1 {
        let last = images.pop().unwrap();
        images = vec![last];
    }
    Ok(serde_json::json!({ "images": images }))
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn interrupt_generation(
    state: State<'_, Arc<AppState>>,
    prompt_id: Option<String>,
) -> Result<(), AppError> {
    if let Some(prompt_id) = prompt_id {
        state.interrupt_prompt(Some(prompt_id.as_str())).await
    } else {
        state.interrupt_user_prompts(None).await
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn clear_all_queues(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    state.interrupt_user_prompts(None).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn delete_queue_item(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
) -> Result<(), AppError> {
    state.delete_queue_items(vec![prompt_id]).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn upload_image(
    state: State<'_, Arc<AppState>>,
    image_path: String,
) -> Result<UploadResponse, AppError> {
    state.upload_image_file(&image_path).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn upload_image_bytes(
    state: State<'_, Arc<AppState>>,
    image_bytes: Vec<u8>,
    filename: String,
) -> Result<UploadResponse, AppError> {
    state.upload_image_from_bytes(image_bytes, filename).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_output_image(
    state: State<'_, Arc<AppState>>,
    filename: String,
    subfolder: String,
) -> Result<Vec<u8>, AppError> {
    state.get_output_image_bytes(&filename, &subfolder).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_client_id(state: State<'_, Arc<AppState>>) -> Result<String, AppError> {
    Ok(state.client_id.clone())
}

#[derive(Clone, serde::Serialize)]
pub struct ModelInstallDir {
    pub path: String,
    pub label: String,
}

#[derive(serde::Serialize)]
pub struct ManagedModelFile {
    pub category: String,
    pub filename: String,
    pub directory: String,
    pub directory_label: String,
    pub path: String,
    pub size_bytes: u64,
    pub modified_ms: u64,
}

#[derive(serde::Serialize)]
pub struct ManagedModelFolder {
    pub category: String,
    pub path: String,
    pub directory: String,
    pub directory_label: String,
}

const MANAGED_MODEL_EXTENSIONS: &[&str] = &[
    "safetensors",
    "ckpt",
    "pt",
    "pth",
    "bin",
    "gguf",
    "onnx",
    "sft",
];

pub(crate) fn is_safe_path_component(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value {
        return false;
    }

    let path = std::path::Path::new(trimmed);
    let mut components = path.components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

pub(crate) fn is_safe_relative_model_path(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value {
        return false;
    }

    let path = std::path::Path::new(trimmed);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn is_managed_model_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext = ext.to_ascii_lowercase();
            MANAGED_MODEL_EXTENSIONS.contains(&ext.as_str())
        })
        .unwrap_or(false)
}

fn known_model_subdirs() -> Vec<&'static str> {
    [
        "checkpoints",
        "loras",
        "vae",
        "upscale_models",
        "embeddings",
        "controlnet",
        "clip",
        "unet",
        "diffusion_models",
        "text_encoders",
        "ultralytics",
        "model_patches",
    ]
    .iter()
    .flat_map(|category| category_subdirs(category).iter().copied())
    .collect()
}

fn is_structured_model_dir(path: &std::path::Path) -> bool {
    known_model_subdirs()
        .iter()
        .any(|subdir| path.join(subdir).is_dir())
}

/// When the user points at a ComfyUI install root (with `models/checkpoints`
/// etc. nested one level down), normalize to the `models` folder so structured
/// category subdirs resolve correctly.
pub(crate) fn resolve_extra_model_root(path: &std::path::Path) -> std::path::PathBuf {
    if is_structured_model_dir(path) {
        return path.to_path_buf();
    }
    let nested = path.join("models");
    if nested.is_dir() && is_structured_model_dir(&nested) {
        return nested;
    }
    path.to_path_buf()
}

fn classify_flat_model_dir(path: &std::path::Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    if name.contains("lora") || name.contains("lycoris") {
        "loras"
    } else if name.contains("checkpoint")
        || name.contains("ckpt")
        || name.contains("stable-diffusion")
        || name.contains("stablediffusion")
    {
        "checkpoints"
    } else if name.contains("vae") {
        "vae"
    } else if name.contains("upscale") || name.contains("esrgan") {
        "upscale_models"
    } else if name.contains("controlnet") || name.contains("control_net") {
        "controlnet"
    } else if name.contains("embed") || name.contains("textual") {
        "embeddings"
    } else if name.contains("ultralytic") || name.contains("face") {
        "ultralytics"
    } else if name.contains("clip")
        || name.contains("text_encoder")
        || name.contains("text-encoder")
        || name.contains("textencoder")
    // StabilityMatrix
    {
        "text_encoders"
    } else if name.contains("unet") || name.contains("diffusion") {
        "diffusion_models"
    } else {
        "loras"
    }
}

fn push_model_install_dir(
    dirs: &mut Vec<ModelInstallDir>,
    seen: &mut BTreeSet<String>,
    path: std::path::PathBuf,
    label: String,
) {
    let display_path = path.to_string_lossy().to_string();
    let key = display_path.to_lowercase();
    if seen.insert(key) {
        dirs.push(ModelInstallDir {
            path: display_path,
            label,
        });
    }
}

pub(crate) fn model_install_dirs_for_config(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    category: &str,
) -> Result<Vec<ModelInstallDir>, AppError> {
    if !is_safe_path_component(category) || category_subdirs(category).is_empty() {
        return Err(AppError::Other("Invalid model category".into()));
    }

    let mut dirs: Vec<ModelInstallDir> = Vec::new();
    let mut seen = BTreeSet::new();

    if !comfyui_path.is_empty() {
        let primary = std::path::Path::new(comfyui_path)
            .join("models")
            .join(category);
        let label = std::path::Path::new(comfyui_path)
            .file_name()
            .map(|n| format!("App ({})", n.to_string_lossy()))
            .unwrap_or_else(|| "App".to_string());
        push_model_install_dir(&mut dirs, &mut seen, primary, label);
    }

    if let Some(extra) = extra_model_paths {
        for line in extra.lines().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let base = resolve_extra_model_root(std::path::Path::new(line));
            let base_label = base
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| line.to_string());

            if is_structured_model_dir(&base) {
                for subdir in category_subdirs(category) {
                    let candidate = base.join(subdir);
                    if candidate.is_dir() {
                        let label = if category_subdirs(category).len() > 1 {
                            format!("{} / {}", base_label, subdir)
                        } else {
                            base_label.clone()
                        };
                        push_model_install_dir(&mut dirs, &mut seen, candidate, label);
                    }
                }
            } else if classify_flat_model_dir(&base) == category {
                push_model_install_dir(&mut dirs, &mut seen, base.to_path_buf(), base_label);
            }
        }
    }

    Ok(dirs)
}

fn relative_model_filename(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn modified_ms(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn collect_model_files_from_dir(
    category: &str,
    dir: &ModelInstallDir,
    root: &std::path::Path,
    current: &std::path::Path,
    files: &mut Vec<ManagedModelFile>,
) -> Result<(), AppError> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let metadata = entry.metadata()?;
        if file_type.is_dir() {
            collect_model_files_from_dir(category, dir, root, &path, files)?;
        } else if file_type.is_file() && is_managed_model_file(&path) {
            let filename = relative_model_filename(root, &path);
            if !is_safe_relative_model_path(&filename) {
                continue;
            }
            files.push(ManagedModelFile {
                category: category.to_string(),
                filename,
                directory: dir.path.clone(),
                directory_label: dir.label.clone(),
                path: path.to_string_lossy().to_string(),
                size_bytes: metadata.len(),
                modified_ms: modified_ms(&metadata),
            });
        }
    }
    Ok(())
}

pub(crate) fn list_model_files_for_config(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    category: &str,
) -> Result<Vec<ManagedModelFile>, AppError> {
    let dirs = model_install_dirs_for_config(comfyui_path, extra_model_paths, category)?;
    let mut files = Vec::new();
    for dir in &dirs {
        let root = std::path::Path::new(&dir.path);
        if root.is_dir() {
            collect_model_files_from_dir(category, dir, root, root, &mut files)?;
        }
    }
    files.sort_by(|a, b| {
        a.filename
            .to_lowercase()
            .cmp(&b.filename.to_lowercase())
            .then_with(|| a.directory_label.cmp(&b.directory_label))
    });
    Ok(files)
}

fn collect_model_folders_from_dir(
    category: &str,
    dir: &ModelInstallDir,
    root: &std::path::Path,
    current: &std::path::Path,
    folders: &mut Vec<ManagedModelFolder>,
) -> Result<(), AppError> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let relative = relative_model_filename(root, &path);
            if is_safe_relative_model_path(&relative) {
                folders.push(ManagedModelFolder {
                    category: category.to_string(),
                    path: relative,
                    directory: dir.path.clone(),
                    directory_label: dir.label.clone(),
                });
            }
            collect_model_folders_from_dir(category, dir, root, &path, folders)?;
        }
    }
    Ok(())
}

/// Lists every subfolder (at any depth) under each known install directory for
/// a category, including empty ones — used to populate the folder tree and
/// move-destination picker in the Model Manager UI.
pub(crate) fn list_model_folders_for_config(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    category: &str,
) -> Result<Vec<ManagedModelFolder>, AppError> {
    let dirs = model_install_dirs_for_config(comfyui_path, extra_model_paths, category)?;
    let mut folders = Vec::new();
    for dir in &dirs {
        let root = std::path::Path::new(&dir.path);
        if root.is_dir() {
            collect_model_folders_from_dir(category, dir, root, root, &mut folders)?;
        }
    }
    folders.sort_by(|a, b| {
        a.directory_label
            .cmp(&b.directory_label)
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
    Ok(folders)
}

/// Creates a (possibly nested) subfolder under a known install directory so
/// users can organize model files before moving anything into it.
pub(crate) fn create_model_folder_for_config(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    category: &str,
    directory: &str,
    folder_path: &str,
) -> Result<(), AppError> {
    let dir = find_known_model_dir(comfyui_path, extra_model_paths, category, directory)?;
    if !is_safe_relative_model_path(folder_path) {
        return Err(AppError::Other("Invalid folder name".into()));
    }
    let target = std::path::Path::new(&dir.path).join(folder_path);
    if target.is_file() {
        return Err(AppError::Other(
            "A file already exists with that name".into(),
        ));
    }
    std::fs::create_dir_all(target)?;
    Ok(())
}

fn find_known_model_dir(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    category: &str,
    directory: &str,
) -> Result<ModelInstallDir, AppError> {
    model_install_dirs_for_config(comfyui_path, extra_model_paths, category)?
        .into_iter()
        .find(|dir| dir.path == directory)
        .ok_or_else(|| AppError::Other("Unknown model directory".into()))
}

fn model_file_path_in_dir(directory: &str, filename: &str) -> Result<std::path::PathBuf, AppError> {
    if !is_safe_relative_model_path(filename) {
        return Err(AppError::Other("Invalid model filename".into()));
    }
    Ok(std::path::Path::new(directory).join(std::path::Path::new(filename)))
}

pub(crate) fn delete_model_file_for_config(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    category: &str,
    filename: &str,
    directory: &str,
) -> Result<(), AppError> {
    let dir = find_known_model_dir(comfyui_path, extra_model_paths, category, directory)?;
    let path = model_file_path_in_dir(&dir.path, filename)?;
    if !is_managed_model_file(&path) {
        return Err(AppError::Other("Unsupported model file type".into()));
    }
    if !path.is_file() {
        return Err(AppError::Other("Model file not found".into()));
    }
    std::fs::remove_file(path)?;
    Ok(())
}

pub(crate) fn move_model_file_for_config(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    category: &str,
    target_category: &str,
    filename: &str,
    source_directory: &str,
    target_directory: &str,
    target_filename: &str,
) -> Result<(), AppError> {
    let source_dir =
        find_known_model_dir(comfyui_path, extra_model_paths, category, source_directory)?;
    let target_dir = find_known_model_dir(
        comfyui_path,
        extra_model_paths,
        target_category,
        target_directory,
    )?;
    let source = model_file_path_in_dir(&source_dir.path, filename)?;
    let target = model_file_path_in_dir(&target_dir.path, target_filename)?;

    if source == target {
        return Err(AppError::Other(
            "Choose a different destination directory".into(),
        ));
    }
    if !is_managed_model_file(&source) {
        return Err(AppError::Other("Unsupported model file type".into()));
    }
    if !source.is_file() {
        return Err(AppError::Other("Model file not found".into()));
    }
    if target.exists() {
        return Err(AppError::Other(
            "A model with that name already exists in the destination".into(),
        ));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    match std::fs::rename(&source, &target) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            std::fs::copy(&source, &target).map_err(|copy_error| {
                AppError::Other(format!(
                    "Failed to move model: {}; copy fallback failed: {}",
                    rename_error, copy_error
                ))
            })?;
            std::fs::remove_file(&source)?;
            Ok(())
        }
    }
}

/// Returns all directories where a model of the given category can be installed.
/// Always includes the primary app directory; also includes any extra_model_paths
/// subdirectories for the category that already exist on disk.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_model_install_dirs(
    state: State<'_, Arc<AppState>>,
    category: String,
) -> Result<Vec<ModelInstallDir>, AppError> {
    let config = state.config.read().await;
    let comfyui_path = config.comfyui_path.clone();
    let extra_model_paths = config.extra_model_paths.clone();
    drop(config);

    model_install_dirs_for_config(&comfyui_path, extra_model_paths.as_deref(), &category)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn list_model_files(
    state: State<'_, Arc<AppState>>,
    category: String,
) -> Result<Vec<ManagedModelFile>, AppError> {
    let config = state.config.read().await;
    let comfyui_path = config.comfyui_path.clone();
    let extra_model_paths = config.extra_model_paths.clone();
    drop(config);

    tokio::task::spawn_blocking(move || {
        list_model_files_for_config(&comfyui_path, extra_model_paths.as_deref(), &category)
    })
    .await
    .map_err(|e| AppError::Other(format!("Model list task failed: {}", e)))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn list_model_folders(
    state: State<'_, Arc<AppState>>,
    category: String,
) -> Result<Vec<ManagedModelFolder>, AppError> {
    let config = state.config.read().await;
    let comfyui_path = config.comfyui_path.clone();
    let extra_model_paths = config.extra_model_paths.clone();
    drop(config);

    tokio::task::spawn_blocking(move || {
        list_model_folders_for_config(&comfyui_path, extra_model_paths.as_deref(), &category)
    })
    .await
    .map_err(|e| AppError::Other(format!("Model folder list task failed: {}", e)))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn create_model_folder(
    state: State<'_, Arc<AppState>>,
    category: String,
    directory: String,
    folder_path: String,
) -> Result<(), AppError> {
    let config = state.config.read().await;
    let comfyui_path = config.comfyui_path.clone();
    let extra_model_paths = config.extra_model_paths.clone();
    drop(config);

    tokio::task::spawn_blocking(move || {
        create_model_folder_for_config(
            &comfyui_path,
            extra_model_paths.as_deref(),
            &category,
            &directory,
            &folder_path,
        )
    })
    .await
    .map_err(|e| AppError::Other(format!("Model folder create task failed: {}", e)))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn delete_model_file(
    state: State<'_, Arc<AppState>>,
    category: String,
    filename: String,
    directory: String,
) -> Result<(), AppError> {
    let config = state.config.read().await;
    let comfyui_path = config.comfyui_path.clone();
    let extra_model_paths = config.extra_model_paths.clone();
    drop(config);

    tokio::task::spawn_blocking(move || {
        delete_model_file_for_config(
            &comfyui_path,
            extra_model_paths.as_deref(),
            &category,
            &filename,
            &directory,
        )
    })
    .await
    .map_err(|e| AppError::Other(format!("Model delete task failed: {}", e)))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn move_model_file(
    state: State<'_, Arc<AppState>>,
    category: String,
    filename: String,
    source_directory: String,
    target_directory: String,
    target_filename: Option<String>,
    target_category: Option<String>,
) -> Result<(), AppError> {
    let config = state.config.read().await;
    let comfyui_path = config.comfyui_path.clone();
    let extra_model_paths = config.extra_model_paths.clone();
    drop(config);

    let target_filename = target_filename.unwrap_or_else(|| filename.clone());
    // Cross-category moves (e.g. a diffusion-only checkpoint mistakenly placed in
    // `checkpoints/` relocated to `diffusion_models/`) pass a distinct target
    // category; omitting it keeps the move within the source category.
    let target_category = target_category.unwrap_or_else(|| category.clone());
    tokio::task::spawn_blocking(move || {
        move_model_file_for_config(
            &comfyui_path,
            extra_model_paths.as_deref(),
            &category,
            &target_category,
            &filename,
            &source_directory,
            &target_directory,
            &target_filename,
        )
    })
    .await
    .map_err(|e| AppError::Other(format!("Model move task failed: {}", e)))?
}

/// Opens a directory in the OS file explorer.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn open_directory(path: String) -> Result<(), AppError> {
    let dir = std::path::Path::new(&path);
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    let path_str = dir
        .canonicalize()
        .unwrap_or_else(|_| dir.to_path_buf())
        .to_string_lossy()
        .to_string();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path_str)
            .spawn()
            .map_err(|e| AppError::Other(format!("Failed to open directory: {}", e)))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| AppError::Other(format!("Failed to open directory: {}", e)))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| AppError::Other(format!("Failed to open directory: {}", e)))?;
    }
    Ok(())
}

/// Proxy a GET request to the Mooshieblob CDN and return the response body as
/// text. Used by the Tauri desktop app for JSON fetches (artist gallery
/// manifest, shards, search index) that would otherwise be blocked by the
/// webview's CORS enforcement. Only the hardcoded CDN origin is reachable —
/// this is NOT an open proxy.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cdn_proxy_fetch(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<String, AppError> {
    // Strip any leading slashes to keep the joined URL well-formed.
    let clean = path.trim_start_matches('/');
    let url = format!("https://cdn.mooshieblob.com/{}", clean);
    let resp = state
        .http_client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("CDN fetch failed: {}", e)))?;
    if !resp.status().is_success() {
        return Err(AppError::ApiError {
            status: resp.status().as_u16(),
            message: format!("CDN returned {} for {}", resp.status(), clean),
        });
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(format!("CDN body read failed: {}", e)))?;
    Ok(body)
}

/// Proxy a GET request to the Mooshieblob CDN and return the response body as
/// base64. Same hardcoded-origin restriction as [`cdn_proxy_fetch`]; this
/// variant exists for binary payloads (artist gallery AVIF images), which
/// `resp.text()` would corrupt. Base64 rather than a byte array because the IPC
/// transport is JSON on both desktop and browser mode.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cdn_proxy_fetch_bytes(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<String, AppError> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let clean = path.trim_start_matches('/');
    let url = format!("https://cdn.mooshieblob.com/{}", clean);
    let resp = state
        .http_client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("CDN fetch failed: {}", e)))?;
    if !resp.status().is_success() {
        return Err(AppError::ApiError {
            status: resp.status().as_u16(),
            message: format!("CDN returned {} for {}", resp.status(), clean),
        });
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Other(format!("CDN body read failed: {}", e)))?;
    Ok(STANDARD.encode(&bytes))
}

/// Proxy a GET request to animadex.net (characters API only). Used by the Tauri
/// desktop app for JSON fetches that would otherwise be blocked by CORS.
/// Only paths under `api/characters/` are allowed — not an open proxy.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn animadex_proxy_fetch(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<String, AppError> {
    let clean = path.trim_start_matches('/');
    if !clean.starts_with("api/characters/") {
        return Err(AppError::Other(
            "animadex proxy: path must start with api/characters/".into(),
        ));
    }
    let url = format!("https://animadex.net/{}", clean);
    let resp = state
        .http_client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("Animadex fetch failed: {}", e)))?;
    if !resp.status().is_success() {
        return Err(AppError::ApiError {
            status: resp.status().as_u16(),
            message: format!("Animadex returned {} for {}", resp.status(), clean),
        });
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(format!("Animadex body read failed: {}", e)))?;
    Ok(body)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    url: String,
    category: String,
    filename: String,
    install_dir: Option<String>,
    expected_sha256: Option<String>,
) -> Result<(), AppError> {
    state
        .download_model_file(
            &app,
            &url,
            &category,
            &filename,
            install_dir.as_deref(),
            expected_sha256.as_deref(),
        )
        .await
}

/// Resolve the real filename a download URL points to (read from the server's
/// `Content-Disposition` header) without downloading the file. Returns `None`
/// when the server reports no usable name, so the Model Hub can fall back to
/// URL-based inference. Used to autopopulate the direct-download filename field,
/// including for CivitAI links whose filename is not present in the URL.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn resolve_download_filename(
    state: State<'_, Arc<AppState>>,
    url: String,
) -> Result<Option<String>, AppError> {
    state.resolve_download_filename(&url).await
}

/// Request cancellation of an in-progress model download by filename. The running
/// download loop checks this flag each chunk, deletes the partial file and stops (#399).
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cancel_download(
    state: State<'_, Arc<AppState>>,
    filename: String,
) -> Result<(), AppError> {
    state.request_download_cancel(&filename);
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_image_file(image_bytes: Vec<u8>, path: String) -> Result<(), AppError> {
    std::fs::write(&path, &image_bytes)?;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_text_file(content: String, path: String) -> Result<(), AppError> {
    tokio::fs::write(&path, content).await?;
    Ok(())
}

/// Embed metadata into raw image bytes and return the result — no disk save.
/// Used when copying or exporting a freshly-generated image before it has been
/// persisted to gallery. The output keeps the input's container format (PNG or
/// WebP), so callers must not assume PNG.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn embed_image_metadata_bytes(
    image_bytes: Vec<u8>,
    metadata: std::collections::HashMap<String, String>,
    metadata_mode: Option<String>,
) -> Result<Vec<u8>, AppError> {
    let mode =
        crate::metadata::MetadataMode::from_str(metadata_mode.as_deref().unwrap_or("text_chunk"));
    crate::metadata::embed_image_metadata(&image_bytes, &metadata, mode).map_err(AppError::Other)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_to_gallery(
    state: State<'_, Arc<AppState>>,
    filename: String,
    subfolder: String,
    prompt_id: String,
    mode: Option<String>,
    metadata: Option<std::collections::HashMap<String, String>>,
    metadata_mode: Option<String>,
) -> Result<String, AppError> {
    let bytes = state.get_output_image_bytes(&filename, &subfolder).await?;
    let saved = save_to_gallery_inner(
        &bytes,
        &filename,
        &prompt_id,
        mode.as_deref(),
        metadata.as_ref(),
        metadata_mode.as_deref(),
    )?;
    let payload = serde_json::json!({
        "filename": saved,
        "prompt_id": prompt_id,
        "mode": mode,
        "source_filename": filename,
        "metadata": metadata,
    });
    state.broadcast("mooshie:image_saved", payload.clone());
    let _ = state.dispatch_webhook_event("image_saved", payload).await;
    Ok(saved)
}

/// Save raw image bytes (from WebSocket) directly to the gallery with optional embedded metadata.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_to_gallery_bytes(
    state: State<'_, Arc<AppState>>,
    image_bytes: Vec<u8>,
    filename: String,
    prompt_id: String,
    mode: Option<String>,
    metadata: Option<std::collections::HashMap<String, String>>,
    metadata_mode: Option<String>,
) -> Result<String, AppError> {
    let saved = save_to_gallery_inner(
        &image_bytes,
        &filename,
        &prompt_id,
        mode.as_deref(),
        metadata.as_ref(),
        metadata_mode.as_deref(),
    )?;
    let payload = serde_json::json!({
        "filename": saved,
        "prompt_id": prompt_id,
        "mode": mode,
        "source_filename": filename,
        "metadata": metadata,
    });
    state.broadcast("mooshie:image_saved", payload.clone());
    let _ = state.dispatch_webhook_event("image_saved", payload).await;
    Ok(saved)
}

/// Save a temp image to the gallery (avoids re-serialising large byte arrays
/// through the IPC bridge — the temp file is already on disk).
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_to_gallery_temp(
    state: State<'_, Arc<AppState>>,
    temp_filename: String,
    filename: String,
    prompt_id: String,
    mode: Option<String>,
    metadata: Option<std::collections::HashMap<String, String>>,
    metadata_mode: Option<String>,
) -> Result<String, AppError> {
    let bytes = crate::temp_images::load(&temp_filename)
        .ok_or_else(|| AppError::Other(format!("Temp image not found: {}", temp_filename)))?;
    let saved = save_to_gallery_inner(
        &bytes,
        &filename,
        &prompt_id,
        mode.as_deref(),
        metadata.as_ref(),
        metadata_mode.as_deref(),
    )?;
    let payload = serde_json::json!({
        "filename": saved,
        "prompt_id": prompt_id,
        "mode": mode,
        "source_filename": filename,
        "metadata": metadata,
    });
    state.broadcast("mooshie:image_saved", payload.clone());
    let _ = state.dispatch_webhook_event("image_saved", payload).await;
    Ok(saved)
}

fn sanitize_filename_component(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .replace("..", "_")
        .trim_matches('_')
        .to_string()
}

fn parse_index_from_base(base: &str) -> String {
    let mut digits = String::new();
    for ch in base.chars().rev() {
        if ch.is_ascii_digit() {
            digits.insert(0, ch);
        } else {
            break;
        }
    }
    if digits.is_empty() {
        "0".to_string()
    } else {
        digits
    }
}

fn template_value(
    key: &str,
    prompt_id: &str,
    mode: &str,
    base: &str,
    metadata: Option<&std::collections::HashMap<String, String>>,
) -> String {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match key {
        "prompt_id" => prompt_id.to_string(),
        "mode" => mode.to_string(),
        "index" => parse_index_from_base(base),
        // Kept simple and dependency-free; Unix timestamp is stable and sortable.
        "date" => now_secs.to_string(),
        "time" => now_secs.to_string(),
        "model" => metadata
            .and_then(|m| {
                m.get("checkpoint")
                    .or_else(|| m.get("model"))
                    .or_else(|| m.get("model_name"))
            })
            .cloned()
            .unwrap_or_else(|| "unknown-model".to_string()),
        "seed" => metadata
            .and_then(|m| m.get("seed"))
            .cloned()
            .unwrap_or_else(|| "0".to_string()),
        _ => String::new(),
    }
}

fn render_output_filename_base(
    template: Option<&str>,
    prompt_id: &str,
    mode: &str,
    base: &str,
    metadata: Option<&std::collections::HashMap<String, String>>,
) -> String {
    if let Some(tpl) = template.map(str::trim).filter(|s| !s.is_empty()) {
        let mut out = tpl.to_string();
        for key in [
            "prompt_id",
            "mode",
            "index",
            "date",
            "time",
            "model",
            "seed",
        ] {
            let token = format!("{{{}}}", key);
            let value =
                sanitize_filename_component(&template_value(key, prompt_id, mode, base, metadata));
            out = out.replace(&token, &value);
        }
        let cleaned = sanitize_filename_component(&out);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    sanitize_filename_component(&format!("{}__{}__{}", prompt_id, mode, base))
}

pub fn save_to_gallery_inner(
    bytes: &[u8],
    filename: &str,
    prompt_id: &str,
    mode: Option<&str>,
    metadata: Option<&std::collections::HashMap<String, String>>,
    metadata_mode: Option<&str>,
) -> Result<String, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    std::fs::create_dir_all(&dir)?;

    let normalized_mode = match mode {
        Some("txt2img") => "txt2img",
        Some("img2img") => "img2img",
        Some("inpainting") => "inpainting",
        Some("image_edit") => "image_edit",
        _ => "unknown",
    };

    // Gallery filename uses the detected container extension, not whatever
    // the frontend guessed. This keeps PNG / JXL gallery files honest even if
    // the caller passed a stale filename.
    let detected_format = crate::metadata::detect_format(bytes);
    let base = match filename.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => filename,
    };
    let ext = match detected_format {
        crate::metadata::ImageFormat::Jxl => "jxl",
        crate::metadata::ImageFormat::WebP => "webp",
        _ => "png",
    };
    let cfg = crate::config::load_persisted_config();
    let rendered_base = render_output_filename_base(
        cfg.output_filename_template.as_deref(),
        prompt_id,
        normalized_mode,
        base,
        metadata,
    );
    let gallery_filename = format!("{}.{}", rendered_base, ext);
    let path = dir.join(&gallery_filename);

    let raw_mode = metadata_mode.unwrap_or("text_chunk");
    let mut embed_mode = crate::metadata::MetadataMode::from_str(raw_mode);

    if matches!(detected_format, crate::metadata::ImageFormat::Png)
        && embed_mode == crate::metadata::MetadataMode::StealthAlpha
    {
        match crate::metadata::is_png_16bit(bytes) {
            Ok(true) => {
                embed_mode = crate::metadata::MetadataMode::Both;
                log::info!(
                    "save_to_gallery_inner: forcing metadata mode to Both for 16-bit PNG (requested=stealth) to improve compatibility"
                );
            }
            Ok(false) => {}
            Err(e) => {
                log::warn!(
                    "save_to_gallery_inner: failed to detect PNG bit depth for metadata mode policy: {}",
                    e
                );
            }
        }
    }

    log::info!(
        "save_to_gallery_inner: format={:?}, metadata_mode={:?}, effective_embed_mode={:?}, has_metadata={}",
        detected_format,
        raw_mode,
        embed_mode,
        metadata.is_some()
    );

    // If metadata provided, embed it using the format-appropriate mechanism.
    let final_bytes = if let Some(meta) = metadata {
        match detected_format {
            crate::metadata::ImageFormat::Png => {
                match crate::metadata::embed_png_metadata(bytes, meta, embed_mode) {
                    Ok(embedded) => embedded,
                    Err(e) => {
                        log::warn!("Failed to embed PNG metadata: {}, saving without", e);
                        bytes.to_vec()
                    }
                }
            }
            crate::metadata::ImageFormat::Jxl => {
                match crate::metadata::embed_jxl_metadata(bytes, meta) {
                    Ok(embedded) => embedded,
                    Err(e) => {
                        log::warn!("Failed to embed JXL metadata: {}, saving without", e);
                        bytes.to_vec()
                    }
                }
            }
            crate::metadata::ImageFormat::WebP => {
                match crate::metadata::embed_webp_metadata(bytes, meta, embed_mode) {
                    Ok(embedded) => embedded,
                    Err(e) => {
                        log::warn!("Failed to embed WebP metadata: {}, saving without", e);
                        bytes.to_vec()
                    }
                }
            }
            crate::metadata::ImageFormat::Mp4 => bytes.to_vec(),
            crate::metadata::ImageFormat::Avif => bytes.to_vec(),
            crate::metadata::ImageFormat::Gif => bytes.to_vec(),
            crate::metadata::ImageFormat::Unknown => bytes.to_vec(),
        }
    } else {
        bytes.to_vec()
    };

    std::fs::write(&path, &final_bytes)?;
    crate::gallery_index::upsert(&path, final_bytes.len() as u64, detected_format, metadata);
    Ok(gallery_filename)
}

/// Result of moving a finished video (and its poster sidecar) into the gallery.
pub struct SavedVideo {
    pub video_filename: String,
    pub poster_filename: Option<String>,
}

/// Rename where possible; fall back to copy+delete when ComfyUI's output dir
/// and the gallery live on different filesystems.
fn move_gallery_file(src: &std::path::Path, dst: &std::path::Path) -> Result<(), AppError> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dst)?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

/// Move a fresh mp4 (and optional poster) out of ComfyUI's output directory
/// into `gallery_dir`, applying the user's output-filename template, then
/// index it. Rust-side counterpart of the frontend-initiated image save path:
/// the video is already encoded on disk, so files are moved instead of
/// shuttling bytes through the WebSocket.
#[allow(clippy::too_many_arguments)]
pub fn save_video_to_gallery(
    video_path: &std::path::Path,
    poster_path: Option<&std::path::Path>,
    gallery_dir: &std::path::Path,
    prompt_id: &str,
    fps: f64,
    frame_count: u64,
    width: u32,
    height: u32,
) -> Result<SavedVideo, AppError> {
    if !video_path.is_file() {
        return Err(AppError::from(format!(
            "Video not found at {}",
            video_path.display()
        )));
    }
    std::fs::create_dir_all(gallery_dir)?;

    let cfg = crate::config::load_persisted_config();
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let rendered = render_output_filename_base(
        cfg.output_filename_template.as_deref(),
        prompt_id,
        "video",
        stem,
        None,
    );

    // The template can collapse distinct outputs onto one name; never
    // overwrite an existing gallery file.
    let mut video_filename = format!("{rendered}.mp4");
    let mut n = 1;
    while gallery_dir.join(&video_filename).exists() {
        video_filename = format!("{rendered}_{n}.mp4");
        n += 1;
    }
    let dest_video = gallery_dir.join(&video_filename);
    move_gallery_file(video_path, &dest_video)?;
    // Mirror the mp4's own metadata into a top-level uuid box. Best-effort: the
    // node writes the container-native copy and this only adds the sidecar that
    // survives a chat-client upload, so a failure is worth a log line and
    // nothing more.
    if crate::metadata::mirror_uuid_sidecar(&dest_video) {
        log::debug!("[video] mirrored metadata into a uuid box for {video_filename}");
    }

    let video_stem = video_filename.trim_end_matches(".mp4").to_string();
    let mut poster_filename = None;
    let mut poster_dest_str = None;
    if let Some(src_poster) = poster_path {
        if src_poster.is_file() {
            let name = format!("{video_stem}_poster.webp");
            let dest_poster = gallery_dir.join(&name);
            match move_gallery_file(src_poster, &dest_poster) {
                Ok(()) => {
                    poster_dest_str = Some(dest_poster.to_string_lossy().to_string());
                    poster_filename = Some(name);
                }
                Err(e) => log::warn!("[video] poster move failed: {e}"),
            }
        }
    }

    let file_size = std::fs::metadata(&dest_video).map(|m| m.len()).unwrap_or(0);
    let duration_seconds = if fps > 0.0 {
        frame_count as f64 / fps
    } else {
        0.0
    };
    crate::gallery_index::upsert_video(
        &dest_video,
        file_size,
        &crate::gallery_index::VideoIndexMeta {
            duration_seconds,
            fps,
            width,
            height,
            poster_path: poster_dest_str,
        },
    );
    Ok(SavedVideo {
        video_filename,
        poster_filename,
    })
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn read_image_metadata(
    filename: String,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let path = resolve_gallery_image_path(&dir, &filename)
        .map_err(|e| AppError::Other(format!("{}: {}", e, filename)))?;
    let bytes = std::fs::read(&path)?;
    crate::metadata::read_image_metadata(&bytes).map_err(AppError::Other)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn read_image_metadata_bytes(
    image_bytes: Vec<u8>,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    crate::metadata::read_image_metadata(&image_bytes).map_err(AppError::Other)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn read_image_metadata_path(
    path: String,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    let bytes = std::fs::read(&path)?;
    crate::metadata::read_image_metadata(&bytes).map_err(AppError::Other)
}

/// Whether a gallery directory entry should appear in gallery listings and
/// count toward storage quotas/expiry. Poster sidecars (`{stem}_poster.webp`)
/// are internal to their video and never listed. Must stay in sync with the
/// formats the save pipeline can write (images + mp4 video).
pub(crate) fn is_listable_gallery_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with("_poster.webp") {
        return false;
    }
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".jxl")
        || lower.ends_with(".mp4")
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn list_gallery_images() -> Result<Vec<String>, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_listable_gallery_file(&name) {
                Some((entry.metadata().ok()?.modified().ok()?, name))
            } else {
                None
            }
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(files.into_iter().map(|(_, name)| name).collect())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn list_gallery_image_entries() -> Result<Vec<GalleryImageEntry>, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    if !dir.exists() {
        return Ok(vec![]);
    }

    // One query for the whole video table, not one per directory entry.
    let meta = crate::gallery_index::video_meta();

    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_listable_gallery_file(&name) {
                return None;
            }

            let metadata = entry.metadata().ok()?;
            let modified = metadata.modified().ok()?;
            let modified_ms = modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis() as u64;

            let entry_meta = meta.get(&name);

            Some(GalleryImageEntry {
                filename: name,
                size_bytes: metadata.len(),
                modified_ms,
                duration_seconds: entry_meta.map(|m| m.duration_seconds),
                fps: entry_meta.and_then(|m| m.fps),
            })
        })
        .collect();

    files.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    Ok(files)
}

/// Copy a gallery file to a caller-chosen destination path.
///
/// Used by save-video-as. Videos are far too large to marshal through IPC as a
/// byte array: Tauri v2 serializes `Vec<u8>` as a JSON number array, so a few
/// hundred MB of mp4 would balloon into gigabytes of JSON. The copy happens in
/// Rust instead. Browser mode never calls this (it downloads straight from the
/// `/internal-api/_gallery/` URL), so there is no webserver dispatch arm.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn copy_gallery_file_to(filename: String, dest_path: String) -> Result<(), AppError> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let src = resolve_gallery_image_path(&dir, &filename)
        .map_err(|e| AppError::Other(format!("Read failed: {}", e)))?;
    tokio::fs::copy(&src, &dest_path).await?;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn load_gallery_image(filename: String) -> Result<Vec<u8>, AppError> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let path = resolve_gallery_image_path(&dir, &filename)
        .map_err(|e| AppError::Other(format!("{}: {}", e, filename)))?;
    let bytes = std::fs::read(&path)?;
    Ok(bytes)
}

/// Load a gallery image and encode as PNG. JXL files are decoded via jxl-oxide
/// and re-encoded as PNG. Used when copying/saving/downloading — PNG is the
/// portable export format that supports metadata embedding.
pub(crate) async fn load_gallery_image_png_inner(filename: String) -> Result<Vec<u8>, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    load_gallery_image_png_from_dir(&dir, &filename).await
}

pub(crate) async fn load_gallery_image_png_from_dir(
    gallery_dir: &std::path::Path,
    filename: &str,
) -> Result<Vec<u8>, AppError> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }
    let path = resolve_gallery_image_path(gallery_dir, filename)
        .map_err(|e| AppError::Other(format!("{}: {}", e, filename)))?;
    let bytes = std::fs::read(&path)?;
    if filename.ends_with(".jxl") || filename.ends_with(".webp") {
        let png = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            let img = decode_gallery_image(&bytes)?;
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png)
                .map_err(|e| format!("PNG encode failed: {}", e))?;
            let png = buf.into_inner();
            // The source may carry embedded generation metadata (JXL box or WebP
            // EXIF chunk); the freshly encoded PNG does not. Re-embed it as a
            // standard PNG text chunk so the exported file stays portable and
            // metadata-complete.
            match crate::metadata::read_image_metadata(&bytes) {
                Ok(Some(meta)) => Ok(crate::metadata::embed_png_metadata(
                    &png,
                    &meta,
                    crate::metadata::MetadataMode::TextChunk,
                )
                .unwrap_or(png)),
                _ => Ok(png),
            }
        })
        .await
        .map_err(|e| AppError::Other(format!("Task panicked: {}", e)))?
        .map_err(AppError::Other)?;
        Ok(png)
    } else {
        Ok(bytes)
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn load_gallery_image_png(filename: String) -> Result<Vec<u8>, AppError> {
    load_gallery_image_png_inner(filename).await
}

/// Load a gallery image for display in the UI. JXL files are transcoded to WebP
/// so WebView2 (which cannot decode JXL natively) can render them.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn load_gallery_image_display(filename: String) -> Result<Vec<u8>, AppError> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let path = resolve_gallery_image_path(&dir, &filename)
        .map_err(|e| AppError::Other(format!("{}: {}", e, filename)))?;
    let bytes = std::fs::read(&path)?;
    if filename.ends_with(".jxl") {
        let webp = tokio::task::spawn_blocking(move || transcode_jxl_to_webp(&bytes))
            .await
            .map_err(|e| AppError::Other(format!("Task panicked: {}", e)))?
            .map_err(AppError::Other)?;
        Ok(webp)
    } else {
        Ok(bytes)
    }
}

/// Read a file from the temp_images directory.  Used by Tauri desktop mode to
/// fetch JXL and display-copy (WebP/PNG) bytes after generation — avoids
/// embedding multi-MB base64 blobs in Tauri events, which silently drops large
/// payloads.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn read_temp_image(filename: String) -> Result<Vec<u8>, AppError> {
    // Reuse the same path-traversal guard from temp_images::load.
    crate::temp_images::load(&filename)
        .ok_or_else(|| AppError::Other(format!("Temp image not found: {}", filename)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryPathResolveError {
    InvalidFilename,
    NotFound,
    Ambiguous,
}

impl std::fmt::Display for GalleryPathResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFilename => write!(f, "Invalid filename"),
            Self::NotFound => write!(f, "Gallery image not found"),
            Self::Ambiguous => write!(f, "Ambiguous gallery filename"),
        }
    }
}

pub fn validate_gallery_filename(filename: &str) -> Result<(), GalleryPathResolveError> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(GalleryPathResolveError::InvalidFilename);
    }
    Ok(())
}

/// Locate a gallery file under the root gallery dir or `users/{username}/` subdirs.
///
/// Desktop IPC does not carry a username, so a basename that appears in more
/// than one gallery location is unsafe to resolve. Fail closed instead of
/// picking a filesystem-dependent match that could load or delete another
/// user's image.
pub fn resolve_gallery_image_path(
    base_dir: &std::path::Path,
    filename: &str,
) -> Result<std::path::PathBuf, GalleryPathResolveError> {
    validate_gallery_filename(filename)?;

    let mut matches = Vec::new();
    let direct = base_dir.join(filename);
    if direct.is_file() {
        matches.push(direct);
    }

    let users_dir = base_dir.join("users");
    if users_dir.is_dir() {
        let mut user_dirs: Vec<_> = std::fs::read_dir(&users_dir)
            .map_err(|_| GalleryPathResolveError::NotFound)?
            .flatten()
            .filter_map(|entry| {
                if entry.file_type().ok().is_some_and(|ft| ft.is_dir()) {
                    Some(entry.path())
                } else {
                    None
                }
            })
            .collect();
        user_dirs.sort();

        for user_dir in user_dirs {
            let candidate = user_dir.join(filename);
            if candidate.is_file() {
                matches.push(candidate);
            }
        }
    }

    match matches.len() {
        0 => Err(GalleryPathResolveError::NotFound),
        1 => Ok(matches.remove(0)),
        _ => Err(GalleryPathResolveError::Ambiguous),
    }
}

#[cfg(test)]
mod gallery_path_tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn temp_gallery_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "mooshieui-gallery-path-{}-{}-{}",
            name,
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).expect("create temp gallery dir");
        dir
    }

    #[test]
    fn resolves_unique_user_gallery_file() {
        let dir = temp_gallery_dir("unique-user");
        let user_dir = dir.join("users").join("alice");
        fs::create_dir_all(&user_dir).expect("create user dir");
        let image = user_dir.join("image.png");
        fs::write(&image, b"png").expect("write image");

        let resolved = resolve_gallery_image_path(&dir, "image.png").expect("resolve image");

        assert_eq!(resolved, image);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_duplicate_user_gallery_basenames() {
        let dir = temp_gallery_dir("duplicate-users");
        let alice = dir.join("users").join("alice");
        let bob = dir.join("users").join("bob");
        fs::create_dir_all(&alice).expect("create alice dir");
        fs::create_dir_all(&bob).expect("create bob dir");
        fs::write(alice.join("image.png"), b"alice").expect("write alice image");
        fs::write(bob.join("image.png"), b"bob").expect("write bob image");

        let err = resolve_gallery_image_path(&dir, "image.png").expect_err("should be ambiguous");

        assert_eq!(err, GalleryPathResolveError::Ambiguous);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_root_and_user_gallery_basename_collision() {
        let dir = temp_gallery_dir("root-user-collision");
        let user_dir = dir.join("users").join("alice");
        fs::create_dir_all(&user_dir).expect("create user dir");
        fs::write(dir.join("image.png"), b"root").expect("write root image");
        fs::write(user_dir.join("image.png"), b"alice").expect("write user image");

        let err = resolve_gallery_image_path(&dir, "image.png").expect_err("should be ambiguous");

        assert_eq!(err, GalleryPathResolveError::Ambiguous);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_gallery_filenames_with_path_segments() {
        assert_eq!(
            validate_gallery_filename("../image.png"),
            Err(GalleryPathResolveError::InvalidFilename)
        );
        assert_eq!(
            validate_gallery_filename("alice/image.png"),
            Err(GalleryPathResolveError::InvalidFilename)
        );
        assert_eq!(
            validate_gallery_filename("alice\\image.png"),
            Err(GalleryPathResolveError::InvalidFilename)
        );
    }
}

/// Generate a WebP thumbnail for a gallery image. Used by the `thumbnail://` protocol.
pub fn generate_thumbnail(
    gallery_dir: &std::path::Path,
    filename: &str,
    max_size: u32,
) -> Result<Vec<u8>, String> {
    // Videos can't be decoded by the image crate; their thumbnail is the
    // poster sidecar written at save time.
    let filename: String = if filename.to_ascii_lowercase().ends_with(".mp4") {
        format!("{}_poster.webp", &filename[..filename.len() - 4])
    } else {
        filename.to_string()
    };
    let filename = filename.as_str();
    let path = resolve_gallery_image_path(gallery_dir, filename)
        .map_err(|e| format!("Read failed: {}", e))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("Read failed: {}", e))?;

    let img = decode_gallery_image(&bytes)?;
    let thumb = img.thumbnail(max_size, max_size);

    let mut buf = std::io::Cursor::new(Vec::new());
    thumb
        .write_to(&mut buf, image::ImageFormat::WebP)
        .map_err(|e| format!("Encode failed: {}", e))?;

    Ok(buf.into_inner())
}

/// Decode a gallery image (PNG or JXL) into an in-memory `DynamicImage`.
/// PNGs go through the `image` crate; JXL is decoded by `jxl-oxide` and
/// promoted to RGBA8 before being wrapped as a `DynamicImage`.
pub fn decode_gallery_image(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    match crate::metadata::detect_format(bytes) {
        crate::metadata::ImageFormat::Jxl => {
            let decoded =
                crate::jxl::decode_to_rgba8(bytes).map_err(|e| format!("JXL decode: {}", e))?;
            let buf = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba)
                .ok_or_else(|| "JXL decode produced mismatched buffer size".to_string())?;
            Ok(image::DynamicImage::ImageRgba8(buf))
        }
        _ => image::load_from_memory(bytes).map_err(|e| format!("Decode failed: {}", e)),
    }
}

/// Transcode JXL bytes to a lossless WebP suitable for serving to non-JXL
/// browsers. Returns the WebP bytes.
pub fn transcode_jxl_to_webp(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = decode_gallery_image(bytes)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::WebP)
        .map_err(|e| format!("WebP encode failed: {}", e))?;
    Ok(buf.into_inner())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_gallery_image_path(filename: String) -> Result<String, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let path = resolve_gallery_image_path(&dir, &filename)
        .map_err(|e| AppError::Other(format!("{}: {}", e, filename)))?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn delete_gallery_image(filename: String) -> Result<(), AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    match resolve_gallery_image_path(&dir, &filename) {
        Ok(path) => {
            std::fs::remove_file(&path)?;
            crate::gallery_index::remove(&path);
            // Videos own a poster sidecar that listings never surface; delete it
            // together with its mp4.
            if let Some(stem) = filename.strip_suffix(".mp4") {
                let poster = path.with_file_name(format!("{stem}_poster.webp"));
                if poster.is_file() {
                    let _ = std::fs::remove_file(&poster);
                    crate::gallery_index::remove(&poster);
                }
            }
        }
        Err(GalleryPathResolveError::NotFound) => {}
        Err(e) => return Err(AppError::Other(format!("{}: {}", e, filename))),
    }
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn rename_gallery_image(
    old_filename: String,
    new_filename: String,
) -> Result<String, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;

    let old_path = resolve_gallery_image_path(&dir, &old_filename)
        .map_err(|e| AppError::Other(format!("{}: {}", e, old_filename)))?;
    validate_gallery_filename(&new_filename)
        .map_err(|e| AppError::Other(format!("{}: {}", e, new_filename)))?;

    // Disallow path traversal / directory injection in rename target.
    let new_name_path = std::path::Path::new(&new_filename);
    let is_single_component = new_name_path.components().count() == 1;
    let exact_file_name =
        new_name_path.file_name().and_then(|n| n.to_str()) == Some(new_filename.as_str());
    if new_filename.trim().is_empty() || !is_single_component || !exact_file_name {
        return Err(AppError::Other(format!(
            "Invalid gallery filename for rename: {}",
            new_filename
        )));
    }

    let old_is_video = old_filename.to_ascii_lowercase().ends_with(".mp4");
    if old_is_video && !new_filename.to_ascii_lowercase().ends_with(".mp4") {
        return Err(AppError::from(
            "Videos must keep the .mp4 extension".to_string(),
        ));
    }

    let new_path = old_path
        .parent()
        .ok_or_else(|| AppError::Other("Invalid gallery path".into()))?
        .join(&new_filename);
    if new_path.exists() {
        return Err(AppError::Other(format!(
            "Target gallery filename already exists: {}",
            new_filename
        )));
    }

    std::fs::rename(&old_path, &new_path)?;
    crate::gallery_index::rename(&old_path, &new_path);

    if old_is_video {
        let old_stem = old_filename.strip_suffix(".mp4").unwrap_or(&old_filename);
        let new_stem = new_filename.strip_suffix(".mp4").unwrap_or(&new_filename);
        let old_poster = old_path.with_file_name(format!("{old_stem}_poster.webp"));
        if old_poster.is_file() {
            let new_poster = old_path.with_file_name(format!("{new_stem}_poster.webp"));
            match std::fs::rename(&old_poster, &new_poster) {
                Ok(()) => {
                    crate::gallery_index::rename(&old_poster, &new_poster);
                    crate::gallery_index::update_poster_path(&new_path, &new_poster);
                }
                Err(e) => {
                    log::warn!(
                        "[gallery] poster rename failed ({} -> ...): {e}",
                        old_poster.display()
                    );
                }
            }
        }
    }

    Ok(new_filename)
}

/// Infer MIME type from image bytes (magic bytes) or file extension.
fn infer_image_mime(bytes: &[u8], ext_hint: Option<&str>) -> &'static str {
    if bytes.len() >= 4 {
        if bytes[0..4] == [0xFF, 0xD8, 0xFF, 0xE0] || bytes[0..4] == [0xFF, 0xD8, 0xFF, 0xE1] {
            return "image/jpeg";
        }
        if bytes.len() >= 4 && bytes[0..4] == [0x52, 0x49, 0x46, 0x46] {
            // RIFF header — likely WebP
            return "image/webp";
        }
    }
    match ext_hint {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

/// Put a file on the Windows clipboard as a file-drop (like right-click → Copy in Explorer).
/// Much faster than decoding PNGs and preserves all metadata.
#[cfg(target_os = "windows")]
pub(crate) fn clipboard_set_file_drop_win(path: &std::path::Path) -> Result<(), AppError> {
    let path_str = path.to_string_lossy().into_owned();
    // Guard must stay alive for the write; DoClear empties the clipboard first
    // (the crate's default FileList setter leaves stale formats behind).
    let _clip = clipboard_win::Clipboard::new_attempts(10)
        .map_err(|e| AppError::Other(format!("Failed to open clipboard: {}", e)))?;
    clipboard_win::raw::set_file_list_with(&[path_str.as_str()], clipboard_win::options::DoClear)
        .map_err(|e| AppError::Other(format!("Failed to set clipboard file list: {}", e)))?;
    Ok(())
}

/// Copy image bytes to the system clipboard using native platform tools.
fn native_clipboard_write(image_bytes: &[u8], mime_type: &str) -> Result<(), AppError> {
    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let run_clipboard_command = |program: &str, args: &[&str]| -> Result<(), String> {
            let mut child = Command::new(program)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                // stderr must be null — xclip/wl-copy fork a background daemon
                // that inherits piped fds, causing wait_with_output to hang forever.
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("{} spawn failed: {}", program, e))?;

            if let Some(ref mut stdin) = child.stdin {
                stdin
                    .write_all(image_bytes)
                    .map_err(|e| format!("{} stdin write failed: {}", program, e))?;
            }
            // Close stdin so the clipboard tool knows we're done writing.
            drop(child.stdin.take());

            let status = child
                .wait()
                .map_err(|e| format!("{} wait failed: {}", program, e))?;

            if status.success() {
                Ok(())
            } else {
                Err(format!("{} exited with {}", program, status))
            }
        };

        // Detect Wayland vs X11 and try the appropriate tool first.
        let on_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|v| v == "wayland")
                .unwrap_or(false);

        let (primary, primary_args, fallback, fallback_args): (&str, Vec<&str>, &str, Vec<&str>) =
            if on_wayland {
                (
                    "wl-copy",
                    vec!["--type", mime_type],
                    "xclip",
                    vec!["-selection", "clipboard", "-t", mime_type, "-i"],
                )
            } else {
                (
                    "xclip",
                    vec!["-selection", "clipboard", "-t", mime_type, "-i"],
                    "wl-copy",
                    vec!["--type", mime_type],
                )
            };

        if let Err(primary_err) = run_clipboard_command(primary, &primary_args) {
            run_clipboard_command(fallback, &fallback_args).map_err(|fallback_err| {
                AppError::Other(format!(
                    "Clipboard copy failed ({} and {}). {}: {} | {}: {}",
                    primary, fallback, primary, primary_err, fallback, fallback_err
                ))
            })?;
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};

        // Write bytes to pasteboard using osascript + temp approach,
        // or pipe PNG data via pbcopy alternative. For reliability,
        // write to a temp file and use osascript.
        let tmp_dir = std::env::temp_dir();
        let ext = match mime_type {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };
        let tmp_path = tmp_dir.join(format!("mooshie_clipboard.{}", ext));
        std::fs::write(&tmp_path, image_bytes)
            .map_err(|e| AppError::Other(format!("Failed to write temp file: {}", e)))?;

        let script = format!(
            "set the clipboard to (read (POSIX file \"{}\") as «class PNGf»)",
            tmp_path.display()
        );
        let status = Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map_err(|e| AppError::Other(format!("osascript failed: {}", e)))?;
        let _ = std::fs::remove_file(&tmp_path);
        if !status.success() {
            return Err(AppError::Other(
                "Failed to copy image to clipboard via osascript".into(),
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Write to temp file, then put file reference on clipboard.
        // Using SetFileDropList instead of SetImage avoids decoding the
        // image (much faster for large PNGs) and preserves metadata.
        let tmp_dir = std::env::temp_dir();
        let ext = match mime_type {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };
        let tmp_path = tmp_dir.join(format!("mooshie_clipboard.{}", ext));
        std::fs::write(&tmp_path, image_bytes)
            .map_err(|e| AppError::Other(format!("Failed to write temp file: {}", e)))?;
        clipboard_set_file_drop_win(&tmp_path)?;
        // Don't delete temp file — it must exist when the user pastes.
    }

    Ok(())
}

/// Public wrapper for `native_clipboard_write` — used by the web server.
pub fn native_clipboard_write_pub(image_bytes: &[u8], mime_type: &str) -> Result<(), AppError> {
    native_clipboard_write(image_bytes, mime_type)
}

/// Read image data from the native OS clipboard as PNG bytes.
/// Uses wl-paste/xclip on Linux, pbpaste on macOS, PowerShell on Windows.
pub fn native_clipboard_read_pub() -> Result<Vec<u8>, AppError> {
    native_clipboard_read()
}

fn native_clipboard_read() -> Result<Vec<u8>, AppError> {
    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};

        let on_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|v| v == "wayland")
                .unwrap_or(false);

        let try_read = |program: &str, args: &[&str]| -> Result<Vec<u8>, String> {
            let output = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| format!("{} spawn failed: {}", program, e))?;

            if output.status.success() && !output.stdout.is_empty() {
                Ok(output.stdout)
            } else {
                Err(format!("{} failed or returned no data", program))
            }
        };

        let (primary, primary_args, fallback, fallback_args): (&str, Vec<&str>, &str, Vec<&str>) =
            if on_wayland {
                (
                    "wl-paste",
                    vec!["--type", "image/png"],
                    "xclip",
                    vec!["-selection", "clipboard", "-t", "image/png", "-o"],
                )
            } else {
                (
                    "xclip",
                    vec!["-selection", "clipboard", "-t", "image/png", "-o"],
                    "wl-paste",
                    vec!["--type", "image/png"],
                )
            };

        match try_read(primary, &primary_args) {
            Ok(bytes) => return Ok(bytes),
            Err(primary_err) => match try_read(fallback, &fallback_args) {
                Ok(bytes) => return Ok(bytes),
                Err(fallback_err) => {
                    return Err(AppError::Other(format!(
                        "No image in clipboard ({}: {} | {}: {})",
                        primary, primary_err, fallback, fallback_err
                    )));
                }
            },
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::{Command, Stdio};

        // Use osascript to check if clipboard has an image and write it to temp
        let tmp_path = std::env::temp_dir().join("mooshie_clipboard_read.png");
        let script = format!(
            "set imgData to the clipboard as «class PNGf»\nset f to open for access POSIX file \"{}\" with write permission\nwrite imgData to f\nclose access f",
            tmp_path.display()
        );
        let status = Command::new("osascript")
            .args(["-e", &script])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .map_err(|e| AppError::Other(format!("osascript failed: {}", e)))?;

        if status.success() {
            let bytes = std::fs::read(&tmp_path).map_err(|e| {
                AppError::Other(format!("Failed to read temp clipboard file: {}", e))
            })?;
            let _ = std::fs::remove_file(&tmp_path);
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::Other("No image in clipboard".into()));
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        let tmp_path = std::env::temp_dir().join("mooshie_clipboard_read.png");
        let script = format!(
            "$img = Get-Clipboard -Format Image; if ($img) {{ $img.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png) }} else {{ exit 1 }}",
            tmp_path.display()
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .status()
            .map_err(|e| AppError::Other(format!("PowerShell clipboard read failed: {}", e)))?;

        if status.success() {
            let bytes = std::fs::read(&tmp_path).map_err(|e| {
                AppError::Other(format!("Failed to read temp clipboard file: {}", e))
            })?;
            let _ = std::fs::remove_file(&tmp_path);
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
        let _ = std::fs::remove_file(&tmp_path);
        Err(AppError::Other("No image in clipboard".into()))
    }
}

/// Public wrapper for `infer_image_mime` — used by the web server.
pub fn infer_image_mime_pub(bytes: &[u8], ext_hint: Option<&str>) -> &'static str {
    infer_image_mime(bytes, ext_hint)
}

/// Copy raw image bytes (PNG/JPEG/WebP) to the system clipboard.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn copy_bytes_to_clipboard(bytes: Vec<u8>, ext: String) -> Result<(), AppError> {
    let mime = infer_image_mime(&bytes, Some(&ext));
    native_clipboard_write(&bytes, mime)
}

/// Copy an image file to the system clipboard.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn copy_image_to_clipboard(file_path: String) -> Result<(), AppError> {
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(AppError::Other(format!("File not found: {}", file_path)));
    }

    let canonical = path
        .canonicalize()
        .map_err(|e| AppError::Other(e.to_string()))?;

    // On Windows, put the actual file on the clipboard as a file drop.
    // This is instant (no image decoding) and preserves PNG metadata.
    #[cfg(target_os = "windows")]
    {
        clipboard_set_file_drop_win(&canonical)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let image_bytes = std::fs::read(&canonical)
            .map_err(|e| AppError::Other(format!("Failed to read image file: {}", e)))?;

        let ext_str = canonical
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());
        let mime = infer_image_mime(&image_bytes, ext_str.as_deref());

        native_clipboard_write(&image_bytes, mime)
    }
}

/// Copy a gallery image to the system clipboard entirely on the Rust side.
/// JXL and WebP images are decoded once and encoded to a single
/// metadata-bearing PNG — no image bytes ever cross the IPC boundary (the old
/// flow shipped a multi-megabyte PNG over IPC four times), and PNG is the only
/// bitmap format every paste target understands. PNGs are put on the clipboard
/// straight from disk.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn copy_gallery_image_to_clipboard(
    filename: String,
    metadata: Option<std::collections::HashMap<String, String>>,
    metadata_mode: Option<String>,
) -> Result<(), AppError> {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let path = resolve_gallery_image_path(&dir, &filename)
        .map_err(|e| AppError::Other(format!("{}: {}", e, filename)))?;

    if filename.ends_with(".jxl") || filename.ends_with(".webp") {
        let src_bytes = std::fs::read(&path)?;
        let mode = crate::metadata::MetadataMode::from_str(
            metadata_mode.as_deref().unwrap_or("text_chunk"),
        );
        let params = metadata.filter(|m| !m.is_empty());
        let png_bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            let decoded = decode_gallery_image(&src_bytes)?.into_rgba8();
            let (width, height) = (decoded.width(), decoded.height());
            let rgba = decoded.into_raw();
            // Prefer the metadata the frontend already holds; fall back to
            // whatever is embedded in the source file itself.
            let params = params.or_else(|| {
                crate::metadata::read_image_metadata(&src_bytes)
                    .ok()
                    .flatten()
            });
            match params {
                Some(p) => {
                    crate::metadata::encode_png_with_metadata_rgba8(&rgba, width, height, &p, mode)
                }
                None => {
                    crate::jxl::encode_rgba8_png(&rgba, width, height).map_err(|e| e.to_string())
                }
            }
        })
        .await
        .map_err(|e| AppError::Other(format!("Task panicked: {}", e)))?
        .map_err(AppError::Other)?;

        native_clipboard_write(&png_bytes, "image/png")
    } else {
        let canonical = path
            .canonicalize()
            .map_err(|e| AppError::Other(e.to_string()))?;

        #[cfg(target_os = "windows")]
        {
            clipboard_set_file_drop_win(&canonical)
        }

        #[cfg(not(target_os = "windows"))]
        {
            let image_bytes = std::fs::read(&canonical)
                .map_err(|e| AppError::Other(format!("Failed to read image file: {}", e)))?;

            let ext_str = canonical
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase());
            let mime = infer_image_mime(&image_bytes, ext_str.as_deref());

            native_clipboard_write(&image_bytes, mime)
        }
    }
}

/// Does an `/object_info/{class}` payload describe `node_class` with every name
/// in `required_inputs` present in its input spec?
///
/// A class name alone is not proof the node is the one we emit for: ComfyUI lets
/// two packages claim the same name (core wins), so `AnimaLLLiteApply` may exist
/// while taking a completely different input shape (#522). Checking the inputs
/// distinguishes them.
pub(crate) fn node_info_matches(
    info: &Value,
    node_class: &str,
    required_inputs: Option<&[String]>,
) -> bool {
    let Some(node) = info.get(node_class) else {
        return false;
    };
    let Some(required_inputs) = required_inputs else {
        return true;
    };
    let input = node.get("input");
    required_inputs.iter().all(|name| {
        ["required", "optional"].iter().any(|group| {
            input
                .and_then(|input| input.get(group))
                .and_then(|group| group.get(name))
                .is_some()
        })
    })
}

/// Check if a ComfyUI node class is available (used to detect custom node packages).
/// Pass `required_inputs` to also assert the node's input signature.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn check_node_available(
    state: State<'_, Arc<AppState>>,
    node_class: String,
    required_inputs: Option<Vec<String>>,
) -> Result<bool, AppError> {
    match state.api_get(&format!("/object_info/{}", node_class)).await {
        Ok(val) => Ok(node_info_matches(
            &val,
            &node_class,
            required_inputs.as_deref(),
        )),
        Err(_) => Ok(false),
    }
}

/// Resolve the uv binary path from the venv path.
/// Layout: {base}/bin/uv.exe and {base}/venv/ — so base = parent of venv_path.
fn resolve_uv_bin(venv_path: &str) -> std::path::PathBuf {
    let base = std::path::Path::new(venv_path)
        .parent()
        .unwrap_or(std::path::Path::new(venv_path));
    #[cfg(target_os = "windows")]
    {
        base.join("bin").join("uv.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        base.join("bin").join("uv")
    }
}

/// Public accessor for `resolve_uv_bin` — used by the browser-mode web server.
pub fn resolve_uv_bin_pub(venv_path: &str) -> std::path::PathBuf {
    resolve_uv_bin(venv_path)
}

/// Resolve the Python executable path from the venv path.
pub fn resolve_venv_python_bin(venv_path: &str) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::path::Path::new(venv_path)
            .join("Scripts")
            .join("python.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::path::Path::new(venv_path).join("bin").join("python")
    }
}

/// Validate a Python module name for `python -c "import <module>"`.
pub fn is_valid_python_module_name(module: &str) -> bool {
    let module = module.trim();
    !module.is_empty()
        && !module.starts_with('.')
        && !module.ends_with('.')
        && !module.contains("..")
        && module
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// Check if a custom node package is installed on disk (directory exists in custom_nodes/).
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn is_custom_node_installed(
    state: State<'_, Arc<AppState>>,
    node_name: String,
) -> Result<bool, AppError> {
    let config = state.config.read().await;
    let target_dir = std::path::Path::new(&config.comfyui_path)
        .join("custom_nodes")
        .join(&node_name);
    Ok(target_dir.exists())
}

/// Whether the RIFE frame-interpolation pack and its `rife49.pth` checkpoint
/// are both present on disk.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn is_rife_installed(state: State<'_, Arc<AppState>>) -> Result<bool, AppError> {
    let comfyui_path = state.config.read().await.comfyui_path.clone();
    Ok(crate::comfyui::nodes::is_rife_installed(&comfyui_path))
}

/// Install the RIFE frame-interpolation pack and its checkpoint, driven by the
/// video settings panel the first time the user enables 2x interpolation.
/// Emits `install:progress` events with the same shape as `install_custom_node`.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn install_rife(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    let (comfyui_path, venv_path, network_proxy, pip_index_url) = {
        let config = state.config.read().await;
        (
            config.comfyui_path.clone(),
            config.venv_path.clone(),
            config.network_proxy.clone(),
            config.pip_index_url.clone(),
        )
    };

    let emit_progress = |step: &str, message: &str, done: bool| {
        let _ = app.emit(
            "install:progress",
            serde_json::json!({
                "node_name": "ComfyUI-Frame-Interpolation",
                "step": step,
                "message": message,
                "done": done,
            }),
        );
    };

    let result = crate::comfyui::nodes::install_rife(
        &state.http_client,
        &comfyui_path,
        &venv_path,
        network_proxy.as_deref(),
        pip_index_url.as_deref(),
        &emit_progress,
    )
    .await;

    if let Err(e) = &result {
        emit_progress("error", e, true);
    }
    result.map_err(AppError::Other)
}

/// Whether the MiniMax-H3 Turbo node pack is present. The adapter file itself
/// is a regular LoRA download, so the panel checks the model list for it.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn is_h3_turbo_installed(state: State<'_, Arc<AppState>>) -> Result<bool, AppError> {
    let comfyui_path = state.config.read().await.comfyui_path.clone();
    Ok(crate::comfyui::nodes::is_h3_turbo_installed(&comfyui_path))
}

/// Install the MiniMax-H3 Turbo node pack, driven by the video settings panel
/// the first time the user ticks the Turbo LoRA. Emits `install:progress`
/// events with the same shape as `install_custom_node`.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn install_h3_turbo(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), AppError> {
    let (comfyui_path, venv_path, network_proxy, pip_index_url) = {
        let config = state.config.read().await;
        (
            config.comfyui_path.clone(),
            config.venv_path.clone(),
            config.network_proxy.clone(),
            config.pip_index_url.clone(),
        )
    };

    let emit_progress = |step: &str, message: &str, done: bool| {
        let _ = app.emit(
            "install:progress",
            serde_json::json!({
                "node_name": "ComfyUI-MiniMax-H3-Turbo",
                "step": step,
                "message": message,
                "done": done,
            }),
        );
    };

    let result = crate::comfyui::nodes::install_h3_turbo(
        &comfyui_path,
        &venv_path,
        network_proxy.as_deref(),
        pip_index_url.as_deref(),
        &emit_progress,
    )
    .await;

    if let Err(e) = &result {
        emit_progress("error", e, true);
    }
    result.map_err(AppError::Other)
}

/// Whether the MiniMax-H3 TeaCache node pack is present.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn is_h3_teacache_installed(state: State<'_, Arc<AppState>>) -> Result<bool, AppError> {
    let comfyui_path = state.config.read().await.comfyui_path.clone();
    Ok(crate::comfyui::nodes::is_h3_teacache_installed(
        &comfyui_path,
    ))
}

/// Install the MiniMax-H3 TeaCache node pack, driven by the video settings
/// panel the first time the user enables the toggle. Emits `install:progress`
/// events with the same shape as `install_custom_node`.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn install_h3_teacache(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), AppError> {
    let (comfyui_path, venv_path, network_proxy, pip_index_url) = {
        let config = state.config.read().await;
        (
            config.comfyui_path.clone(),
            config.venv_path.clone(),
            config.network_proxy.clone(),
            config.pip_index_url.clone(),
        )
    };

    let emit_progress = |step: &str, message: &str, done: bool| {
        let _ = app.emit(
            "install:progress",
            serde_json::json!({
                "node_name": "ComfyUI-MiniMaxH3-TeaCache",
                "step": step,
                "message": message,
                "done": done,
            }),
        );
    };

    let result = crate::comfyui::nodes::install_h3_teacache(
        &comfyui_path,
        &venv_path,
        network_proxy.as_deref(),
        pip_index_url.as_deref(),
        &emit_progress,
    )
    .await;

    if let Err(e) = &result {
        emit_progress("error", e, true);
    }
    result.map_err(AppError::Other)
}

/// Install a custom node from a git repository into ComfyUI's custom_nodes directory.
/// Emits `install:progress` events with { node_name, step, message, done } for live progress.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn install_custom_node(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    git_url: String,
    node_name: String,
) -> Result<(), AppError> {
    let (comfyui_path, venv_path, network_proxy, pip_index_url) = {
        let config = state.config.read().await;
        (
            config.comfyui_path.clone(),
            config.venv_path.clone(),
            config.network_proxy.clone(),
            config.pip_index_url.clone(),
        )
    };
    let network_proxy = network_proxy.as_deref();
    let pip_index_url = pip_index_url.as_deref();
    let custom_nodes_dir = std::path::Path::new(&comfyui_path).join("custom_nodes");
    let target_dir = custom_nodes_dir.join(&node_name);

    let emit_progress = |step: &str, message: &str, done: bool| {
        let _ = app.emit(
            "install:progress",
            serde_json::json!({
                "node_name": node_name,
                "step": step,
                "message": message,
                "done": done,
            }),
        );
    };

    if target_dir.exists() {
        emit_progress("done", "Already installed", true);
        return Ok(());
    }

    // git clone — stream stderr for progress (git writes progress to stderr)
    emit_progress("clone", &format!("Cloning {}...", node_name), false);

    let mut git_cmd = tokio_command_no_window("git");
    git_cmd
        .args([
            "clone",
            "--progress",
            &git_url,
            target_dir.to_string_lossy().as_ref(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    crate::comfyui::nodes::apply_network_proxy(&mut git_cmd, network_proxy);
    let mut child = git_cmd
        .spawn()
        .map_err(|e| AppError::Other(format!("git clone failed to start: {}", e)))?;

    // Read stderr in background for progress lines
    if let Some(stderr) = child.stderr.take() {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let app_clone = app.clone();
        let node_name_clone = node_name.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() {
                    let _ = app_clone.emit(
                        "install:progress",
                        serde_json::json!({
                            "node_name": node_name_clone,
                            "step": "clone",
                            "message": trimmed,
                            "done": false,
                        }),
                    );
                }
            }
        });
    }

    let status = child
        .wait()
        .await
        .map_err(|e| AppError::Other(format!("git clone failed: {}", e)))?;

    if !status.success() {
        emit_progress("error", "git clone failed", true);
        return Err(AppError::Other("git clone failed".to_string()));
    }

    // pip install -r requirements.txt if it exists
    let req_file = target_dir.join("requirements.txt");
    if req_file.exists() {
        emit_progress("pip", "Installing Python dependencies...", false);

        let uv_path = resolve_uv_bin(&venv_path);

        let mut pip_child = if uv_path.exists() {
            let mut cmd = tokio_command_no_window(&uv_path);
            cmd.args(["pip", "install", "-r", &req_file.to_string_lossy()])
                .env("VIRTUAL_ENV", &venv_path)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            crate::comfyui::nodes::apply_pip_install_options(
                &mut cmd,
                true,
                network_proxy,
                pip_index_url,
            );
            cmd.spawn()
                .map_err(|e| AppError::Other(format!("uv pip install failed to start: {}", e)))?
        } else {
            let venv_base = std::path::Path::new(&venv_path);
            #[cfg(target_os = "windows")]
            let pip_path = venv_base.join("Scripts").join("pip.exe");
            #[cfg(not(target_os = "windows"))]
            let pip_path = venv_base.join("bin").join("pip");

            let mut cmd = tokio_command_no_window(&pip_path);
            cmd.args(["install", "-r", &req_file.to_string_lossy()])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            crate::comfyui::nodes::apply_pip_install_options(
                &mut cmd,
                false,
                network_proxy,
                pip_index_url,
            );
            cmd.spawn()
                .map_err(|e| AppError::Other(format!("pip install failed to start: {}", e)))?
        };

        // Stream pip stdout for progress
        if let Some(stdout) = pip_child.stdout.take() {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let app_clone = app.clone();
            let node_name_clone = node_name.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        let _ = app_clone.emit(
                            "install:progress",
                            serde_json::json!({
                                "node_name": node_name_clone,
                                "step": "pip",
                                "message": trimmed,
                                "done": false,
                            }),
                        );
                    }
                }
            });
        }

        let pip_status = pip_child
            .wait()
            .await
            .map_err(|e| AppError::Other(format!("pip install failed: {}", e)))?;

        if !pip_status.success() {
            emit_progress(
                "error",
                "pip install failed (some features may not work)",
                false,
            );
            log::warn!("pip install requirements failed for {}", node_name);
        }
    }

    emit_progress(
        "done",
        &format!("{} installed successfully", node_name),
        true,
    );

    // Emit event so frontend knows to restart ComfyUI
    let _ = app.emit("custom_node:installed", &node_name);
    Ok(())
}

/// Install a pip package into the ComfyUI virtual environment.
/// Used to lazily install dependencies that are only needed for optional features
/// (e.g. `ultralytics` for face fix).
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn install_pip_package(
    state: State<'_, Arc<AppState>>,
    package: String,
) -> Result<(), AppError> {
    let (venv_path, network_proxy, pip_index_url) = {
        let config = state.config.read().await;
        (
            config.venv_path.clone(),
            config.network_proxy.clone(),
            config.pip_index_url.clone(),
        )
    };
    let network_proxy = network_proxy.as_deref();
    let pip_index_url = pip_index_url.as_deref();

    let uv_path = resolve_uv_bin(&venv_path);

    let output = if uv_path.exists() {
        let mut cmd = tokio_command_no_window(&uv_path);
        cmd.args(["pip", "install", &package])
            .env("VIRTUAL_ENV", &venv_path);
        crate::comfyui::nodes::apply_pip_install_options(
            &mut cmd,
            true,
            network_proxy,
            pip_index_url,
        );
        cmd.output()
            .await
            .map_err(|e| AppError::Other(format!("uv pip install failed to start: {}", e)))?
    } else {
        // Fallback to venv pip
        let venv_base = std::path::Path::new(&venv_path);
        #[cfg(target_os = "windows")]
        let pip_path = venv_base.join("Scripts").join("pip.exe");
        #[cfg(not(target_os = "windows"))]
        let pip_path = venv_base.join("bin").join("pip");

        let mut cmd = tokio_command_no_window(&pip_path);
        cmd.args(["install", &package]);
        crate::comfyui::nodes::apply_pip_install_options(
            &mut cmd,
            false,
            network_proxy,
            pip_index_url,
        );
        cmd.output()
            .await
            .map_err(|e| AppError::Other(format!("pip install failed to start: {}", e)))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Other(format!(
            "pip install {} failed: {}",
            package, stderr
        )));
    }

    log::info!("Installed pip package: {}", package);
    Ok(())
}

/// Verify that a Python module can be imported inside the ComfyUI virtual environment.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn check_python_import(
    state: State<'_, Arc<AppState>>,
    module: String,
) -> Result<bool, AppError> {
    let module = module.trim().to_string();
    if !is_valid_python_module_name(&module) {
        return Err(AppError::Other("Invalid module name".into()));
    }

    let venv_path = {
        let config = state.config.read().await;
        config.venv_path.clone()
    };

    let python_path = resolve_venv_python_bin(&venv_path);
    let output = tokio_command_no_window(&python_path)
        .args(["-c", &format!("import {}", module)])
        .output()
        .await
        .map_err(|e| AppError::Other(format!("python import check failed to start: {}", e)))?;

    Ok(output.status.success())
}

/// Search for a model file by SHA256 hash (full or AutoV2) within a model category directory.
/// Returns the filename if found, or null if no match.
/// Note: this hashes each file in the directory, so it may take a while for large collections.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn find_model_by_hash(
    state: State<'_, Arc<AppState>>,
    category: String,
    hash: String,
) -> Result<Option<String>, AppError> {
    if !is_safe_path_component(&category) {
        return Err(AppError::Other("Invalid model category".into()));
    }

    let comfyui_path = {
        let config = state.config.read().await;
        config.comfyui_path.clone()
    };
    if comfyui_path.is_empty() {
        return Ok(None);
    }
    let models_dir = std::path::Path::new(&comfyui_path)
        .join("models")
        .join(&category);

    if !models_dir.exists() {
        return Ok(None);
    }

    let needle = hash.to_uppercase();
    let is_autov2 = needle.len() == 10;

    let entries = std::fs::read_dir(&models_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !(name.ends_with(".safetensors") || name.ends_with(".ckpt")) {
            continue;
        }
        if let Ok(h) = full_sha256(&path) {
            let matches = if is_autov2 {
                autov2_hash(&h) == needle
            } else {
                h == needle
            };
            if matches {
                return Ok(Some(name));
            }
        }
    }
    Ok(None)
}

/// Compute the full SHA256 hash of a model file (uppercase hex, CivitAI-compatible).
/// Also returns the AutoV2 hash (first 10 chars).
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn hash_model_file(
    state: State<'_, Arc<AppState>>,
    category: String,
    filename: String,
) -> Result<ModelHashResult, AppError> {
    if !is_safe_path_component(&category) {
        return Err(AppError::Other("Invalid model category".into()));
    }
    if !is_safe_relative_model_path(&filename) {
        return Err(AppError::Other("Invalid model filename".into()));
    }

    let comfyui_path = {
        let config = state.config.read().await;
        config.comfyui_path.clone()
    };
    if comfyui_path.is_empty() {
        return Err(AppError::Other("ComfyUI path not configured".into()));
    }
    let path = std::path::Path::new(&comfyui_path)
        .join("models")
        .join(&category)
        .join(&filename);

    if !path.is_file() {
        return Err(AppError::Other(format!("File not found: {}", filename)));
    }
    let sha256 = full_sha256(&path)?;
    let autov2 = autov2_hash(&sha256);
    Ok(ModelHashResult { sha256, autov2 })
}

async fn civitai_lookup_hash_value(state: &Arc<AppState>, hash: &str) -> Result<Value, AppError> {
    let api_key = state.config.read().await.civitai_api_key.clone();
    let url = format!("https://civitai.com/api/v1/model-versions/by-hash/{}", hash);
    let mut req = state
        .http_client
        .get(&url)
        .header("User-Agent", "MooshieUI/0.3.9");
    if let Some(key) = api_key.filter(|v| !v.trim().is_empty()) {
        req = req.bearer_auth(key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Other(format!("CivitAI request failed: {}", e)))?;

    if resp.status() == 404 {
        return Err(AppError::Other("Model not found on CivitAI".into()));
    }
    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "CivitAI returned status {}",
            resp.status()
        )));
    }

    let data: Value = resp
        .json()
        .await
        .map_err(|e| AppError::Other(format!("Failed to parse CivitAI response: {}", e)))?;
    Ok(data)
}

/// Parse a CivitAI image id from a numeric string or `https://civitai.com/images/{id}` URL.
pub(crate) fn parse_civitai_image_id_pub(image_ref: &str) -> Result<u64, AppError> {
    parse_civitai_image_id(image_ref)
}

fn parse_civitai_image_id(image_ref: &str) -> Result<u64, AppError> {
    let trimmed = image_ref.trim();
    if let Ok(id) = trimmed.parse::<u64>() {
        return Ok(id);
    }
    const MARKER: &str = "/images/";
    if let Some(pos) = trimmed.find(MARKER) {
        let rest = &trimmed[pos + MARKER.len()..];
        let id_str = rest.split(&['/', '?', '#'][..]).next().unwrap_or("").trim();
        if let Ok(id) = id_str.parse::<u64>() {
            return Ok(id);
        }
    }
    Err(AppError::Other(format!(
        "Could not parse CivitAI image id from {:?}",
        image_ref
    )))
}

fn is_allowed_civitai_image_host(host: &str) -> bool {
    // Accept civitai.com and any of its subdomains (image.civitai.com,
    // cdn.civitai.com, and any future image host CivitAI introduces). The
    // trailing-dot match keeps look-alikes like "civitai.com.evil.test" out.
    let host = host.to_ascii_lowercase();
    host == "civitai.com" || host.ends_with(".civitai.com")
}

pub(crate) fn parse_civitai_image_url(url: &str) -> Result<reqwest::Url, AppError> {
    let parsed = reqwest::Url::parse(url.trim())
        .map_err(|e| AppError::Other(format!("Invalid CivitAI image URL: {}", e)))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Other("CivitAI image URL must use HTTPS".into()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::Other("CivitAI image URL is missing a host".into()))?;
    if !is_allowed_civitai_image_host(host) {
        return Err(AppError::Other(
            "Only CivitAI image URLs can be used as sidecar thumbnails".into(),
        ));
    }
    Ok(parsed)
}

pub(crate) async fn fetch_civitai_image_bytes(
    state: &AppState,
    url: &str,
) -> Result<Vec<u8>, AppError> {
    // Reuse the shared no-redirect client so redirects stay manual (the token is
    // gated per-host below) while still benefiting from connection pooling.
    let client = &state.http_client_no_redirect;
    // The initial URL must be a CivitAI host so this command can't be turned into
    // a generic server-side fetch primitive (it is reachable through browser-mode
    // LAN auth and carries the user's CivitAI token).
    let mut current = parse_civitai_image_url(url)?;
    let civitai_api_key = state.config.read().await.civitai_api_key.clone();

    for _ in 0..5 {
        // CivitAI redirects image requests to its CDN, which is not on a
        // civitai.com host. Only attach the user's token while we are still on a
        // CivitAI host so it can never leak to the CDN or any other origin.
        let on_civitai_host = current
            .host_str()
            .map(is_allowed_civitai_image_host)
            .unwrap_or(false);
        let mut req = client
            .get(current.clone())
            .header("User-Agent", "MooshieUI/0.5.7");
        if on_civitai_host {
            if let Some(key) = civitai_api_key.as_ref().filter(|v| !v.trim().is_empty()) {
                req = req.bearer_auth(key);
            }
        }

        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Other(format!("Image fetch failed: {}", e)))?;
        if resp.status().is_redirection() {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .ok_or_else(|| AppError::Other("Image fetch redirect missing Location".into()))?
                .to_str()
                .map_err(|_| AppError::Other("Image fetch redirect Location is invalid".into()))?;
            current = current
                .join(location)
                .map_err(|e| AppError::Other(format!("Image fetch redirect invalid: {}", e)))?;
            // Follow the redirect to CivitAI's CDN, but keep it HTTPS-only so a
            // redirect can never reach an internal http service. The token is
            // gated on the host check above, not on the target being CivitAI.
            if current.scheme() != "https" {
                return Err(AppError::Other(
                    "CivitAI image redirect must use HTTPS".into(),
                ));
            }
            continue;
        }
        if !resp.status().is_success() {
            return Err(AppError::Other(format!(
                "Image fetch returned HTTP {}",
                resp.status()
            )));
        }
        return resp
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|e| AppError::Other(format!("Failed to read image bytes: {}", e)));
    }

    Err(AppError::Other("Image fetch had too many redirects".into()))
}

#[cfg(test)]
mod sidecar_thumbnail_tests {
    use super::*;

    #[test]
    fn sidecar_image_url_allows_civitai_image_hosts() {
        assert!(parse_civitai_image_url("https://image.civitai.com/example.jpeg").is_ok());
        assert!(parse_civitai_image_url("https://cdn.civitai.com/example.png").is_ok());
        assert!(parse_civitai_image_url("https://civitai.com/images/123").is_ok());
        // Any civitai.com subdomain is accepted so new image hosts keep working.
        assert!(parse_civitai_image_url("https://imagecache.civitai.com/example.jpeg").is_ok());
    }

    #[test]
    fn sidecar_image_url_rejects_non_civitai_and_non_https_targets() {
        assert!(parse_civitai_image_url("http://image.civitai.com/example.jpeg").is_err());
        assert!(parse_civitai_image_url("http://127.0.0.1:8188/view").is_err());
        assert!(parse_civitai_image_url("https://169.254.169.254/latest/meta-data").is_err());
        assert!(parse_civitai_image_url("https://civitai.com.evil.test/example.png").is_err());
    }

    #[test]
    fn cached_image_fetch_uses_same_civitai_only_url_policy() {
        // `fetch_cached_image` is exposed through browser-mode LAN auth; this
        // policy must keep it from becoming a server-side request primitive.
        assert!(parse_civitai_image_url("https://www.civitai.com/images/123").is_ok());
        assert!(parse_civitai_image_url("https://localhost/admin").is_err());
        assert!(parse_civitai_image_url("file:///etc/passwd").is_err());
    }
}

/// Look up a CivitAI image by id (or image page URL) and return generation metadata when available.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn civitai_lookup_image(
    state: State<'_, Arc<AppState>>,
    image_ref: String,
) -> Result<Value, AppError> {
    let image_id = parse_civitai_image_id(&image_ref)?;
    let api_key = state.config.read().await.civitai_api_key.clone();
    let url = format!(
        "https://civitai.com/api/v1/images?imageId={}&withMeta=true",
        image_id
    );
    let mut req = state
        .http_client
        .get(&url)
        .header("User-Agent", "MooshieUI/0.5.7");
    if let Some(key) = api_key.filter(|v| !v.trim().is_empty()) {
        req = req.bearer_auth(key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Other(format!("CivitAI image lookup failed: {}", e)))?;

    if resp.status() == 404 {
        return Err(AppError::Other("Image not found on CivitAI".into()));
    }
    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "CivitAI returned status {}",
            resp.status()
        )));
    }

    let data: Value = resp
        .json()
        .await
        .map_err(|e| AppError::Other(format!("Failed to parse CivitAI response: {}", e)))?;
    Ok(data)
}

/// Write a PNG sidecar preview next to a checkpoint or LoRA file (`{stem}.png`).
pub(crate) async fn save_model_sidecar_thumbnail_inner(
    state: &AppState,
    category: &str,
    filename: &str,
    image_url: Option<&str>,
    gallery_filename: Option<&str>,
    gallery_dir: Option<&std::path::Path>,
) -> Result<(), AppError> {
    let bytes = if let Some(gf) = gallery_filename.filter(|s| !s.is_empty()) {
        if let Some(dir) = gallery_dir {
            load_gallery_image_png_from_dir(dir, gf).await?
        } else {
            load_gallery_image_png_inner(gf.to_string()).await?
        }
    } else if let Some(url) = image_url.filter(|s| !s.is_empty()) {
        fetch_civitai_image_bytes(state, url).await?
    } else {
        return Err(AppError::Other(
            "Provide image_url or gallery_filename".into(),
        ));
    };

    let (comfyui_path, extra_model_paths) = {
        let config = state.config.read().await;
        if config.comfyui_path.is_empty() {
            return Err(AppError::Other("ComfyUI path not configured".into()));
        }
        (
            config.comfyui_path.clone(),
            config.extra_model_paths.clone(),
        )
    };

    let path = resolve_model_path(
        &comfyui_path,
        extra_model_paths.as_deref(),
        category,
        filename,
    )
    .ok_or_else(|| AppError::Other(format!("Model file not found: {}", filename)))?;

    let model_dir = path
        .parent()
        .ok_or_else(|| AppError::Other("Model path has no parent directory".into()))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::Other("Invalid model filename".into()))?;

    let out_path = model_dir.join(format!("{}.png", stem));
    let img = image::load_from_memory(&bytes)
        .map_err(|e| AppError::Other(format!("Failed to decode image: {}", e)))?;
    img.save(&out_path)
        .map_err(|e| AppError::Other(format!("Failed to write sidecar thumbnail: {}", e)))?;

    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_model_sidecar_thumbnail(
    state: State<'_, Arc<AppState>>,
    category: String,
    filename: String,
    image_url: Option<String>,
    gallery_filename: Option<String>,
) -> Result<(), AppError> {
    save_model_sidecar_thumbnail_inner(
        state.inner(),
        &category,
        &filename,
        image_url.as_deref(),
        gallery_filename.as_deref(),
        None,
    )
    .await
}

/// Look up a model on CivitAI by its hash (SHA256 or AutoV2).
/// Returns the CivitAI model version info if found.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn civitai_lookup_hash(
    state: State<'_, Arc<AppState>>,
    hash: String,
) -> Result<Value, AppError> {
    civitai_lookup_hash_value(state.inner(), &hash).await
}

fn sidecar_metadata_path(path: &std::path::Path, suffix: &str) -> Option<std::path::PathBuf> {
    let parent = path.parent()?;
    let stem = path.file_stem()?.to_str()?;
    Some(parent.join(format!("{}{}", stem, suffix)))
}

fn is_sdxl_like_family(family: &str) -> bool {
    matches!(family, "sdxl" | "illustrious" | "pony")
}

/// Anima / Wan2.1 fine-tunes (e.g. animayume) generally ship without ModelSpec
/// or sidecar metadata — keep the legacy filename heuristics for them so they
/// never depend on sidecar or hash lookups.
fn text_has_marker_token(text: &str, marker: &str) -> bool {
    text.match_indices(marker).any(|(idx, _)| {
        let before_is_boundary = text[..idx]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        let after_idx = idx + marker.len();
        let after_is_boundary = text[after_idx..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        before_is_boundary && after_is_boundary
    })
}

fn text_indicates_anima(text: &str) -> bool {
    let name = text.trim().to_lowercase();
    if name.contains("nanosaur") || name.contains("mugen") {
        return false;
    }
    name.contains("animayume")
        || text_has_marker_token(&name, "anima")
        || text_has_marker_token(&name, "yume")
}

fn filename_indicates_anima(filename: &str) -> bool {
    text_indicates_anima(filename)
}

fn model_family_from_base_model(base_model: &str) -> &'static str {
    let bm = base_model.trim().to_lowercase();
    if bm.is_empty() {
        return "unknown";
    }

    if bm.contains("nanosaur") {
        return "nanosaur";
    }
    if bm == "flux.2 klein 9b-base" {
        return "flux2klein9bbase";
    }
    if bm == "flux.2 klein 9b" {
        return "flux2klein9b";
    }
    if bm == "flux.2 klein 4b-base" {
        return "flux2klein4bbase";
    }
    if bm == "flux.2 klein 4b" {
        return "flux2klein4b";
    }
    if bm == "flux.2 d" {
        return "flux2d";
    }
    if bm == "flux.1 krea" {
        return "flux1krea";
    }
    if bm == "flux.1 s" {
        return "flux1s";
    }
    if bm == "flux.1 d" {
        return "flux1d";
    }
    if bm == "zimageturbo" {
        return "zit";
    }
    if bm == "zimagebase" {
        return "zib";
    }
    // Qwen Image Edit variants must be checked before the generic "qwen" bucket.
    // Plus (2509/2511, multi-image) is the more specific match.
    if bm.contains("qwen")
        && (bm.contains("edit plus")
            || bm.contains("edit-plus")
            || bm.contains("edit_plus")
            || bm.contains("editplus")
            || bm.contains("2509")
            || bm.contains("2511"))
    {
        return "qwen_edit_plus";
    }
    if bm.contains("qwen") && bm.contains("edit") {
        return "qwen_edit";
    }
    if bm.contains("qwen") {
        return "qwen";
    }
    if bm == "ideogram 4.0" {
        return "ideogram4";
    }
    if bm == "krea 2" {
        return "krea2";
    }
    // Wan Video bases are Anima fine-tunes in practice (animayume etc.) — keep
    // the legacy CivitAI baseModel → anima mapping.
    if bm.contains("wan video") || bm.contains("wan 2") || bm.contains("wan2") || bm == "wan" {
        return "anima";
    }
    if text_indicates_anima(&bm) {
        return "anima";
    }
    if bm.contains("illustrious") || bm.contains("noobai") {
        return "illustrious";
    }
    if bm.contains("pony") {
        return "pony";
    }
    if bm.contains("stable diffusion 3")
        || bm.contains("stable-diffusion-3")
        || bm.contains("stable_diffusion_3")
        || bm.contains("sd3")
        || bm.contains("sd 3")
    {
        return "sd3";
    }
    if bm.contains("chroma") {
        return "chroma";
    }
    if bm.contains("kontext") {
        return "flux1kontext";
    }
    if bm.contains("flux") {
        return "flux";
    }
    if bm.contains("auraflow") || bm.contains("aura flow") {
        return "auraflow";
    }
    if bm.contains("pixart") {
        return "pixart";
    }
    if bm.contains("hunyuan") {
        return "hunyuandit";
    }
    if bm.contains("cascade") {
        return "cascade";
    }
    if bm.contains("kolors") {
        return "kolors";
    }
    if bm.contains("mugen") {
        return "mugen";
    }
    if bm.contains("stable diffusion xl") || bm.contains("sdxl") || bm.contains("xl 1.0") {
        return "sdxl";
    }
    if bm.contains("stable diffusion 1.5")
        || bm.contains("sd 1.5")
        || bm.contains("sd15")
        || bm.contains("sd_15")
        || bm.contains("1.5")
    {
        return "sd15";
    }

    "unknown"
}

fn model_family_from_filename(filename: &str) -> Option<&'static str> {
    let name = filename.trim().to_lowercase();
    if name.is_empty() {
        return None;
    }

    if name.contains("nanosaur") {
        return Some("nanosaur");
    }
    if name.contains("flux.2 klein 9b-base")
        || name.contains("flux2klein9bbase")
        || (name.contains("flux")
            && name.contains("klein")
            && name.contains("9b")
            && name.contains("base"))
    {
        return Some("flux2klein9bbase");
    }
    if name.contains("flux.2 klein 9b")
        || name.contains("flux2klein9b")
        || (name.contains("flux") && name.contains("klein") && name.contains("9b"))
    {
        return Some("flux2klein9b");
    }
    if name.contains("flux.2 klein 4b-base")
        || name.contains("flux2klein4bbase")
        || (name.contains("flux")
            && name.contains("klein")
            && name.contains("4b")
            && name.contains("base"))
    {
        return Some("flux2klein4bbase");
    }
    if name.contains("flux.2 klein 4b")
        || name.contains("flux2klein4b")
        || (name.contains("flux") && name.contains("klein") && name.contains("4b"))
    {
        return Some("flux2klein4b");
    }
    if name.contains("flux.2 d")
        || name.contains("flux2d")
        || (name.contains("flux") && name.contains("2") && name.contains("d"))
    {
        return Some("flux2d");
    }
    // Flux.1 Kontext (image edit). Must precede the generic flux1d matcher, which
    // would otherwise claim "flux1-kontext-dev" via the flux+1+d token test.
    if name.contains("kontext") {
        return Some("flux1kontext");
    }
    if name.contains("flux.1 krea")
        || name.contains("flux1krea")
        || (name.contains("flux") && name.contains("krea"))
    {
        return Some("flux1krea");
    }
    if name.contains("flux.1 s")
        || name.contains("flux1s")
        || name.contains("schnell")
        || (name.contains("flux") && name.contains("1") && name.contains("s"))
    {
        return Some("flux1s");
    }
    if name.contains("flux.1 d")
        || name.contains("flux1d")
        || (name.contains("flux") && name.contains("1") && name.contains("d"))
    {
        return Some("flux1d");
    }
    if name.contains("zimageturbo")
        || name.contains("zimage_turbo")
        || name.contains("/zit/")
        || name.contains("\\zit\\")
        || name.contains("_zit")
        || name.contains("-zit")
        || name.contains(" zit")
        || name.starts_with("zit")
    {
        return Some("zit");
    }
    if name.contains("zimagebase")
        || name.contains("zimage_base")
        || name.contains("/zib/")
        || name.contains("\\zib\\")
        || name.contains("_zib")
        || name.contains("-zib")
        || name.contains(" zib")
        || name.starts_with("zib")
        || (name.contains("zimage") && name.contains("base"))
    {
        return Some("zib");
    }
    // Qwen Image Edit variants precede the generic "qwen" bucket; Plus (multi-image,
    // 2509/2511) is the more specific match and is tested first.
    if name.contains("qwen")
        && (name.contains("edit_plus")
            || name.contains("edit-plus")
            || name.contains("editplus")
            || name.contains("2509")
            || name.contains("2511"))
    {
        return Some("qwen_edit_plus");
    }
    if name.contains("qwen") && name.contains("edit") {
        return Some("qwen_edit");
    }
    if name.contains("qwen") {
        return Some("qwen");
    }
    if name.contains("ideogram4") {
        return Some("ideogram4");
    }
    if name.contains("krea2")
        || name.contains("krea-2")
        || name.contains("krea_2")
        || name.contains("krea 2")
    {
        return Some("krea2");
    }
    if name == "wan"
        || name.contains("wan video")
        || name.contains("wan 2")
        || name.contains("wan2")
    {
        return Some("wan");
    }
    // Fine-tunes such as animayume_v05.safetensors; use token matching so
    // unrelated names like Animagine or AnimationMix do not become Anima.
    if filename_indicates_anima(&name) {
        return Some("anima");
    }
    if name.contains("noobai") || name.contains("illustrious") {
        return Some("illustrious");
    }
    if name.contains("pony") {
        return Some("pony");
    }
    if name.contains("stable diffusion 3")
        || name.contains("stable-diffusion-3")
        || name.contains("stable_diffusion_3")
        || name.contains("sd3")
        || name.contains("sd 3")
    {
        return Some("sd3");
    }
    if name.contains("chroma") {
        return Some("chroma");
    }
    if name.contains("flux") {
        return Some("flux");
    }
    if name.contains("auraflow") || name.contains("aura flow") {
        return Some("auraflow");
    }
    if name.contains("pixart") {
        return Some("pixart");
    }
    if name.contains("hunyuan") {
        return Some("hunyuandit");
    }
    if name.contains("cascade") {
        return Some("cascade");
    }
    if name.contains("kolors") {
        return Some("kolors");
    }
    if name.contains("mugen") {
        return Some("mugen");
    }
    if name.contains("stable diffusion xl") || name.contains("sdxl") || name.contains("xl 1.0") {
        return Some("sdxl");
    }
    if name.contains("stable diffusion 1.5")
        || name.contains("sd 1.5")
        || name.contains("sd15")
        || name.contains("sd_15")
        || name.contains("1.5")
    {
        return Some("sd15");
    }

    None
}

fn turbo_model_variant_from_filename(filename: &str) -> &'static str {
    let name = filename.trim().to_lowercase();
    if name.is_empty() {
        return "none";
    }
    if name.contains("zimageturbo")
        || name.contains("z-image-turbo")
        || name.contains("/zit/")
        || name.contains("\\zit\\")
        || name.contains("_zit")
        || name.contains("-zit")
        || name.contains(" zit")
        || name.starts_with("zit")
    {
        return "turbo";
    }
    if name.contains("dmd2") {
        return "dmd2";
    }
    if name.contains("dmd") {
        return "dmd";
    }
    if name.contains("turbo") {
        return "turbo";
    }
    if name.contains("lightning") {
        return "lightning";
    }
    if name.contains("lcm") {
        return "lcm";
    }
    if name.contains("hyper") {
        return "hyper";
    }
    "none"
}

fn find_first_vae_matching(vaes: &[String], markers: &[&str]) -> Option<String> {
    vaes.iter().find_map(|vae| {
        let lower = vae.to_lowercase();
        if markers.iter().any(|marker| lower.contains(marker)) {
            Some(vae.clone())
        } else {
            None
        }
    })
}

fn find_first_text_encoder_matching(encoders: &[String], markers: &[&str]) -> Option<String> {
    encoders.iter().find_map(|encoder| {
        let lower = encoder.to_lowercase();
        if !lower.ends_with(".safetensors") {
            return None;
        }
        if markers.iter().any(|marker| lower.contains(marker)) {
            Some(encoder.clone())
        } else {
            None
        }
    })
}

/// Families that are never distributed as a full single-file checkpoint: their
/// text encoder always ships separately. Loading one through
/// `CheckpointLoaderSimple` yields a `None` CLIP and fails at conditioning, so
/// the family alone is enough to conclude the file is a bare diffusion model.
///
/// Rust mirror of `SPLIT_ONLY_FAMILIES` in
/// [`src/lib/utils/modelFamily.ts`](../../../src/lib/utils/modelFamily.ts) — keep
/// the two in sync.
pub(crate) fn family_requires_separate_clip(family: &str) -> bool {
    matches!(
        family,
        "anima"
            | "wan"
            | "qwen"
            | "qwen_edit"
            | "qwen_edit_plus"
            | "flux"
            | "flux1d"
            | "flux1s"
            | "flux1krea"
            | "flux1kontext"
            | "flux2d"
            | "flux2klein9b"
            | "flux2klein9bbase"
            | "flux2klein4b"
            | "flux2klein4bbase"
            | "chroma"
            | "ideogram4"
            | "krea2"
    )
}

fn recommended_vae_from_available(category: &str, family: &str, vaes: &[String]) -> Option<String> {
    if category != "diffusion_models" || vaes.is_empty() {
        return None;
    }

    if matches!(
        family,
        "anima" | "qwen" | "qwen_edit" | "qwen_edit_plus" | "wan"
    ) {
        return find_first_vae_matching(vaes, &["qwen"]).or_else(|| vaes.first().cloned());
    }

    if family == "krea2" {
        return find_first_vae_matching(vaes, &["qwen"]).or_else(|| vaes.first().cloned());
    }

    if family == "ideogram4" {
        return find_first_vae_matching(vaes, &["flux2-vae", "flux2_vae"])
            .or_else(|| vaes.first().cloned());
    }

    if matches!(
        family,
        "flux2d" | "flux2klein9b" | "flux2klein9bbase" | "flux2klein4b" | "flux2klein4bbase"
    ) {
        return find_first_vae_matching(vaes, &["flux2-vae", "flux2_vae"])
            .or_else(|| vaes.first().cloned());
    }

    if matches!(
        family,
        "flux" | "flux1d" | "flux1s" | "flux1krea" | "flux1kontext" | "chroma" | "zib" | "zit"
    ) {
        return find_first_vae_matching(vaes, &["flux"]).or_else(|| vaes.first().cloned());
    }

    find_first_vae_matching(vaes, &["sdxl"]).or_else(|| vaes.first().cloned())
}

/// Text-encoder filename markers accepted for Krea 2 (Qwen3-VL 4B, 30720-dim
/// conditioning). Shared with the generate-time guard in templates/mod.rs.
pub const KREA2_TEXT_ENCODER_MARKERS: [&str; 5] = [
    "qwen3vl-4b",
    "qwen3vl_4b",
    "qwen3-vl-4b",
    "qwen3_vl_4b",
    "qwen3vl4b",
];

/// Returns (recommended encoder filename if a compatible one exists, CLIP type).
/// A `None` filename with `Some` type means: the family's CLIP type is known but
/// no installed encoder is compatible — callers must NOT silently substitute an
/// arbitrary encoder (a mismatched encoder fails deep inside ComfyUI sampling).
fn recommended_clip_from_available(
    category: &str,
    family: &str,
    encoders: &[String],
) -> Option<(Option<String>, &'static str)> {
    if category != "diffusion_models" || encoders.is_empty() {
        return None;
    }

    if family == "anima" {
        // Strict: Anima's llm_adapter is trained against Qwen3-0.6B, so any other
        // encoder produces garbage rather than an error. Omit the model instead of
        // substituting one and let the frontend offer the download.
        // Matches the curated Anima recommended-model entries (CLIPLoader type "wan").
        let preferred =
            find_first_text_encoder_matching(encoders, &["qwen_3_06b_base", "qwen_3_06b"]);
        return Some((preferred, "wan"));
    }

    if matches!(family, "qwen" | "qwen_edit" | "qwen_edit_plus" | "wan") {
        let preferred = find_first_text_encoder_matching(encoders, &["qwen2.5-vl", "qwen_2.5_vl"])
            .or_else(|| encoders.first().cloned());
        return Some((preferred, "qwen_image"));
    }

    if family == "flux2d" {
        let preferred = encoders
            .iter()
            .find_map(|encoder| {
                let lower = encoder.to_lowercase();
                if lower.contains("cow-mistral3-small") {
                    Some(encoder.clone())
                } else {
                    None
                }
            })
            .or_else(|| encoders.first().cloned());
        return Some((preferred, "flux2"));
    }

    if matches!(family, "flux2klein9b" | "flux2klein9bbase") {
        let preferred = find_first_text_encoder_matching(encoders, &["qwen3_8b", "qwen_3_8b"])
            .or_else(|| encoders.first().cloned());
        return Some((preferred, "flux2"));
    }

    if matches!(family, "flux2klein4b" | "flux2klein4bbase") {
        let preferred =
            find_first_text_encoder_matching(encoders, &["zimage", "qwen3-4b", "qwen34b"])
                .or_else(|| encoders.first().cloned());
        return Some((preferred, "flux2"));
    }

    if matches!(family, "zib" | "zit") {
        let preferred =
            find_first_text_encoder_matching(encoders, &["zimage", "qwen3-4b", "qwen34b"])
                .or_else(|| encoders.first().cloned());
        return Some((preferred, "lumina2"));
    }

    if matches!(
        family,
        "flux" | "flux1d" | "flux1s" | "flux1krea" | "flux1kontext" | "chroma"
    ) {
        let preferred = find_first_text_encoder_matching(encoders, &["flan_t5_xxl", "t5_xxl"])
            .or_else(|| encoders.first().cloned());
        return Some((preferred, "chroma"));
    }

    if family == "ideogram4" {
        // Strict: Ideogram 4 only works with a Qwen3-VL 8B encoder. Never fall
        // back to an unrelated encoder — that fails deep inside sampling.
        let preferred = find_first_text_encoder_matching(
            encoders,
            &[
                "qwen3vl-8b",
                "qwen3vl_8b",
                "qwen3-vl-8b",
                "qwen3_vl_8b",
                "qwen3vl8b",
            ],
        );
        return Some((preferred, "ideogram4"));
    }

    if family == "krea2" {
        // Strict: Krea 2 expects 12x2560=30720-dim conditioning from Qwen3-VL 4B.
        // Recommending any other encoder produces a confusing ComfyUI error.
        let preferred = find_first_text_encoder_matching(encoders, &KREA2_TEXT_ENCODER_MARKERS);
        return Some((preferred, "krea2"));
    }

    Some((Some(encoders.first()?.clone()), "wan"))
}

fn read_json_sidecar(path: &std::path::Path) -> Result<Option<Value>, AppError> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    Ok(Some(value))
}

/// https://github.com/willmiao/ComfyUI-Lora-Manager
fn read_comfyui_lora_manager_metadata(
    model_path: &std::path::Path,
) -> Result<Option<Value>, AppError> {
    let Some(path) = sidecar_metadata_path(model_path, ".metadata.json") else {
        return Ok(None);
    };
    let Some(json) = read_json_sidecar(&path)? else {
        return Ok(None);
    };
    Ok(Some(json!({
        "baseModel": json
            .get("base_model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("unknown"))
            .or_else(|| {
                json.get("civitai")
                    .and_then(|v| v.get("baseModel"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("unknown"))
            }),
        "name": json
            .get("model_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .or_else(|| {
                json.get("civitai")
                    .and_then(|v| v.get("model"))
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
            }),
        "versionName": json
            .get("civitai")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        "modelId": json
            .get("civitai")
            .and_then(|v| v.get("modelId"))
            .and_then(|v| v.as_u64()),
        "createdAt": json
            .get("civitai")
            .and_then(|v| v.get("createdAt"))
            .and_then(|v| v.as_str()),
        "updatedAt": json
            .get("civitai")
            .and_then(|v| v.get("updatedAt"))
            .and_then(|v| v.as_str()),
        "publishedAt": json
            .get("civitai")
            .and_then(|v| v.get("publishedAt"))
            .and_then(|v| v.as_str()),
        "creatorUsername": json
            .get("civitai")
            .and_then(|v| v.get("creator"))
            .and_then(|v| v.get("username"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        "trainedWords": json
            .get("civitai")
            .and_then(|v| v.get("trainedWords"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "images": json
            .get("civitai")
            .and_then(|v| v.get("images"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "tags": json
            .get("tags")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "modelDescription": json
            .get("modelDescription")
            .cloned()
            .unwrap_or(Value::Null),
    })))
}

/// - https://github.com/AUTOMATIC1111/st/able-diffusion-webui
/// - https://github.com/lllyasviel/st/able-diffusion-webui-forge
/// - https://github.com/Haoming02/sd/-webui-forge-classic/tree/neo
/// via https://github.com/butaixianran/Stable-Diffusion-Webui-Civitai-Helper
fn read_forge_metadata(model_path: &std::path::Path) -> Result<Option<Value>, AppError> {
    let Some(path) = sidecar_metadata_path(model_path, ".civitai.info") else {
        return Ok(None);
    };
    let Some(json) = read_json_sidecar(&path)? else {
        return Ok(None);
    };
    Ok(Some(json!({
        "baseModel": json
            .get("baseModel")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        "name": json
            .get("model")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        "versionName": json
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        "modelId": json.get("modelId").and_then(|v| v.as_u64()),
        "createdAt": json.get("createdAt").and_then(|v| v.as_str()),
        "updatedAt": json.get("updatedAt").and_then(|v| v.as_str()),
        "publishedAt": json.get("publishedAt").and_then(|v| v.as_str()),
        "creatorUsername": json
            .get("creator")
            .and_then(|v| v.get("username"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        "trainedWords": json
            .get("trainedWords")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "images": json
            .get("images")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "tags": json
            .get("model")
            .and_then(|v| v.get("tags"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "modelDescription": json
            .get("model")
            .and_then(|v| v.get("description"))
            .cloned()
            .unwrap_or(Value::Null),
    })))
}

/// https://github.com/LykosAI/StabilityMatrix
fn read_stability_matrix_metadata(model_path: &std::path::Path) -> Result<Option<Value>, AppError> {
    let Some(path) = sidecar_metadata_path(model_path, ".cm-info.json") else {
        return Ok(None);
    };
    let Some(json) = read_json_sidecar(&path)? else {
        return Ok(None);
    };
    Ok(Some(json!({
        "baseModel": json
            .get("BaseModel")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("other")),
        "name": json
            .get("ModelName")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        "versionName": json
            .get("VersionName")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        "modelId": json.get("ModelId").and_then(|v| v.as_u64()),
        "createdAt": Value::Null,
        "updatedAt": Value::Null,
        "publishedAt": Value::Null,
        "trainedWords": json
            .get("TrainedWords")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "images": Value::Array(Vec::new()),
        "tags": json
            .get("Tags")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "modelDescription": json
            .get("ModelDescription")
            .cloned()
            .unwrap_or(Value::Null),
    })))
}

async fn lookup_civitai_base_model_by_hash(
    state: &Arc<AppState>,
    hash: &str,
) -> Result<Option<String>, AppError> {
    let data = match civitai_lookup_hash_value(state, hash).await {
        Ok(data) => data,
        Err(AppError::Other(message)) if message == "Model not found on CivitAI" => {
            return Ok(None)
        }
        Err(err) => return Err(err),
    };
    Ok(data
        .get("baseModel")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string))
}

#[cfg(feature = "desktop")]
#[tauri::command]
// TODO: refactor src-tauri/src/commands/api.rs and src-tauri/src/webserver.rs
pub async fn civitai_search_models(
    state: State<'_, Arc<AppState>>,
    params: CivitaiSearchParams,
) -> Result<Value, AppError> {
    // Build query string manually because reqwest percent-encodes brackets in
    // parameter names (baseModels[] → baseModels%5B%5D) which CivitAI ignores.
    let encode_val =
        |v: &str| -> String { url::form_urlencoded::byte_serialize(v.as_bytes()).collect() };

    let mut parts: Vec<String> = vec![
        format!(
            "sort={}",
            encode_val(&params.sort.unwrap_or_else(|| "Most Downloaded".to_string()))
        ),
        format!(
            "period={}",
            encode_val(&params.period.unwrap_or_else(|| "AllTime".to_string()))
        ),
        format!("nsfw={}", params.nsfw.unwrap_or(false)),
        format!("limit={}", params.limit.unwrap_or(20)),
    ];

    let has_query = params
        .query
        .as_ref()
        .filter(|v| !v.trim().is_empty())
        .is_some();

    if !has_query {
        parts.push(format!("page={}", params.page.unwrap_or(1)));
    }

    if let Some(cursor) = params.cursor.filter(|v| !v.trim().is_empty()) {
        parts.push(format!("cursor={}", encode_val(&cursor)));
    }

    if let Some(q) = params.query.filter(|v| !v.trim().is_empty()) {
        parts.push(format!("query={}", encode_val(&q)));
    }
    if let Some(t) = params.model_type.filter(|v| !v.trim().is_empty()) {
        parts.push(format!("types[]={}", encode_val(&t)));
    }
    if let Some(base_model) = params.base_model.filter(|v| !v.trim().is_empty()) {
        parts.push(format!("baseModels[]={}", encode_val(&base_model)));
    }
    if let Some(file_format) = params.file_format.filter(|v| !v.trim().is_empty()) {
        parts.push(format!("fileFormats[]={}", encode_val(&file_format)));
    }
    // Note: CivitAI public API does not support a "status" query parameter.

    let url = format!("https://civitai.com/api/v1/models?{}", parts.join("&"));
    log::debug!("CivitAI search URL: {}", url);

    let mut req = state
        .http_client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "MooshieUI/0.3.9");

    if let Some(key) = params.api_key.filter(|v| !v.trim().is_empty()) {
        req = req.bearer_auth(key);
    }

    let resp = req.send().await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(AppError::ApiError {
            status: status.as_u16(),
            message: if body.is_empty() {
                status.to_string()
            } else {
                body
            },
        });
    }

    let data: Value = serde_json::from_str(&body)?;
    Ok(data)
}

/// Fetch a single CivitAI model (all versions and files) by numeric ID.
/// Shared by the Tauri command and the LAN web server route.
pub async fn civitai_get_model_internal(
    state: &Arc<AppState>,
    model_id: u64,
    api_key: Option<String>,
) -> Result<Value, AppError> {
    let url = format!("https://civitai.com/api/v1/models/{}", model_id);

    let mut req = state
        .http_client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "MooshieUI/0.3.9");

    if let Some(key) = api_key.filter(|v| !v.trim().is_empty()) {
        req = req.bearer_auth(key);
    }

    let resp = req.send().await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(AppError::ApiError {
            status: status.as_u16(),
            message: if body.is_empty() {
                status.to_string()
            } else {
                body
            },
        });
    }

    let data: Value = serde_json::from_str(&body)?;
    Ok(data)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn civitai_get_model(
    state: State<'_, Arc<AppState>>,
    model_id: u64,
    api_key: Option<String>,
) -> Result<Value, AppError> {
    civitai_get_model_internal(&state, model_id, api_key).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn civitai_list_architectures(
    state: State<'_, Arc<AppState>>,
    api_key: Option<String>,
) -> Result<Vec<String>, AppError> {
    let mut architectures = BTreeSet::<String>::new();

    // Add common architectures first to guarantee they're present
    let common = vec![
        // Stable Diffusion 1.x
        "SD 1.4",
        "SD 1.5",
        "SD 1.5 LCM",
        "SD 1.5 Hyper",
        // Stable Diffusion 2.x
        "SD 2.0",
        "SD 2.0 768",
        "SD 2.1",
        "SD 2.1 768",
        "SD 2.1 Unclip",
        // Stable Diffusion 3.x
        "SD 3",
        "SD 3.5",
        "SD 3.5 Large",
        "SD 3.5 Large Turbo",
        "SD 3.5 Medium",
        // SDXL
        "SDXL 0.9",
        "SDXL 1.0",
        "SDXL 1.0 LCM",
        "SDXL Distilled",
        "SDXL Turbo",
        "SDXL Lightning",
        "SDXL Hyper",
        // Anime / Illustrious / NoobAI / Pony
        "Illustrious",
        "NoobAI",
        "Pony",
        // Flux
        "Flux.1 S",
        "Flux.1 D",
        "Flux.1 S Turbo",
        // Other popular architectures
        "AuraFlow",
        "Hunyuan 1",
        "HunyuanDiT",
        "Hunyuan Video",
        "Lumina",
        "Kolors",
        "PixArt-a",
        "PixArt-E",
        "Stable Cascade",
        "SVD",
        "SVD XT",
        "PlaygroundV2.5",
        "CogVideoX",
        // Misc
        "Illusion",
        "MoDi",
        "ODOR",
        "Other",
    ];
    for &arch in &common {
        architectures.insert(arch.to_string());
    }

    let mut cursor: Option<String> = None;

    for _ in 0..8 {
        let mut req = state
            .http_client
            .get("https://civitai.com/api/v1/models")
            .header("Accept", "application/json")
            .header("User-Agent", "MooshieUI/0.3.9")
            .query(&[("limit", "100")]);

        if let Some(ref c) = cursor {
            req = req.query(&[("cursor", c)]);
        }

        req = req.timeout(std::time::Duration::from_secs(3));

        if let Some(key) = api_key.as_ref().filter(|v| !v.trim().is_empty()) {
            req = req.bearer_auth(key);
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(_) => break,
        };

        if !resp.status().is_success() {
            break;
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(_) => break,
        };

        let data = match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => v,
            Err(_) => break,
        };

        if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
            for item in items {
                if let Some(versions) = item.get("modelVersions").and_then(|v| v.as_array()) {
                    for version in versions {
                        if let Some(base_model) = version.get("baseModel").and_then(|v| v.as_str())
                        {
                            let normalized = base_model.trim();
                            if !normalized.is_empty() {
                                architectures.insert(normalized.to_string());
                            }
                        }
                    }
                }
            }
        }

        cursor = data
            .get("metadata")
            .and_then(|m| m.get("nextCursor"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());

        if cursor.is_none() {
            break;
        }
    }

    Ok(architectures.into_iter().collect())
}

/// Read the small subset of metadata needed at runtime.
/// `base_model` is resolved from local sidecars first:
/// 1. `{filename}.metadata.json` (ComfyUI LoRA Manager)
/// 2. `{filename}.civitai.info` (Forge / A1111 Civitai Helper)
/// 3. `{filename}.cm-info.json` (Stability Matrix)
/// 4. fallback to filename-based family detection
/// 5. fallback to `hash + CivitAI baseModel`
/// Prediction-related header fields are read only for SDXL-like families
/// (`sdxl`, `illustrious`/`noobai`, `pony`).
/// Shared by the desktop `read_modelspec` command and the browser-mode
/// webserver dispatch — must compile in both build flavors (no desktop cfg).
pub(crate) async fn read_modelspec_internal(
    state: &Arc<AppState>,
    category: &str,
    filename: &str,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    if !is_safe_path_component(category) {
        return Err(AppError::Other("Invalid model category".into()));
    }
    if !is_safe_relative_model_path(filename) {
        return Err(AppError::Other("Invalid model filename".into()));
    }

    let config = state.config.read().await;
    if config.comfyui_path.is_empty() {
        return Err(AppError::Other("ComfyUI path not configured".into()));
    }
    let comfyui_path = config.comfyui_path.clone();
    let extra_model_paths = config.extra_model_paths.clone();
    drop(config);

    let path = resolve_model_path(
        &comfyui_path,
        extra_model_paths.as_deref(),
        category,
        filename,
    )
    .ok_or_else(|| AppError::Other(format!("File not found: {}", filename)))?;

    // GGUF files have no safetensors header, but filename heuristics, sidecar
    // metadata, and hash lookups still apply. Without them a GGUF diffusion
    // model (e.g. Krea-2-Turbo-Q5_K_S.gguf) stays family "unknown" and the
    // split-model text-encoder type is never resolved.
    let is_safetensors = filename.ends_with(".safetensors");
    let is_gguf = filename.to_ascii_lowercase().ends_with(".gguf");
    if !is_safetensors && !is_gguf {
        return Ok(None);
    }

    let mut result = std::collections::HashMap::new();
    result.insert(
        "turbo_model_variant".to_string(),
        turbo_model_variant_from_filename(filename).to_string(),
    );
    if filename_indicates_anima(filename) {
        // Anima models lack ModelSpec/sidecar metadata — resolve from the
        // filename first (legacy detection) instead of sidecar/hash lookups.
        result.insert("family".to_string(), "anima".to_string());
        result.insert("is_sdxl_like".to_string(), "false".to_string());
    } else if let Some(base_model) = read_comfyui_lora_manager_metadata(&path)?
        .as_ref()
        .and_then(|v| v.get("baseModel"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
    {
        result.insert("base_model".to_string(), base_model);
    } else if let Some(base_model) = read_forge_metadata(&path)
        .ok()
        .flatten()
        .as_ref()
        .and_then(|v| v.get("baseModel"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
    {
        result.insert("base_model".to_string(), base_model);
    } else if let Some(base_model) = read_stability_matrix_metadata(&path)
        .ok()
        .flatten()
        .as_ref()
        .and_then(|v| v.get("baseModel"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
    {
        result.insert("base_model".to_string(), base_model);
    } else if let Some(family) = model_family_from_filename(filename) {
        result.insert("family".to_string(), family.to_string());
        result.insert(
            "is_sdxl_like".to_string(),
            if is_sdxl_like_family(family) {
                "true".to_string()
            } else {
                "false".to_string()
            },
        );
    } else {
        let hash_path = path.clone();
        let hash = tokio::task::spawn_blocking(move || full_sha256(&hash_path))
            .await
            .map_err(|e| AppError::Other(format!("Hash task failed: {}", e)))??;
        let autov2 = autov2_hash(&hash);
        result.insert("hash".to_string(), autov2.clone());
        if let Some(base_model) = lookup_civitai_base_model_by_hash(state, &autov2)
            .await
            .ok()
            .flatten()
        {
            result.insert("base_model".to_string(), base_model);
        }
    }

    if is_safetensors
        && (result
            .get("is_sdxl_like")
            .is_some_and(|value| value == "true")
            || result.get("base_model").is_some_and(|base_model| {
                is_sdxl_like_family(model_family_from_base_model(base_model))
            }))
    {
        if let Some(runtime_meta) = read_safetensors_runtime_metadata(&path)? {
            result.extend(runtime_meta);
        }
    }

    if !result.contains_key("family") {
        let family = result
            .get("base_model")
            .map(|base_model| model_family_from_base_model(base_model))
            .unwrap_or("unknown");
        result.insert("family".to_string(), family.to_string());
        result.insert(
            "is_sdxl_like".to_string(),
            if is_sdxl_like_family(family) {
                "true".to_string()
            } else {
                "false".to_string()
            },
        );
    }

    // Merge the full ModelSpec display fields (title, author, description,
    // trigger phrase, ...) for the model info panel. Header-only read — no
    // hashing. Runtime keys resolved above take precedence. This also populates
    // an inferred `architecture` (from tensor-key patterns) when the file has no
    // declared modelspec.architecture. GGUF has no safetensors header, but its
    // binary header still carries `general.architecture` plus the tensor-name
    // table, so it feeds the same structural detection below.
    let display_meta = if is_safetensors {
        read_safetensors_modelspec(&path).ok().flatten()
    } else if is_gguf {
        read_gguf_architecture_meta(&path).ok().flatten()
    } else {
        None
    };
    if let Some(display_meta) = display_meta {
        for (key, value) in display_meta {
            result.entry(key).or_insert(value);
        }
    }

    // Tensor keys are structural ground truth: they describe the weights that are
    // actually in the file, unlike a filename or a sidecar, which can be renamed
    // or missing. Anima ships with an empty metadata block, so a renamed Anima
    // checkpoint has nothing else to identify it and its text encoder would
    // otherwise degrade to whatever encoder happens to be installed first.
    //
    // Resolution rule (structural vs. the softer filename/sidecar/CivitAI family):
    //   - Anima always wins: it is a DiT, not a classic UNet, so standard
    //     ControlNet cannot apply and only the Anima LLLite path works. A wrong
    //     guess is unrunnable, not merely suboptimal.
    //   - An unknown soft family is filled in from the structural result.
    //   - Otherwise the two are compared by architecture *class*. When both
    //     classes are known and differ, the filename named a structurally
    //     incompatible family (e.g. a flux-named file that is really SDXL) and
    //     the weights win, correcting the family and recording the mismatch.
    //   - Same class, or either side without a structural signature, keeps the
    //     softer, more specific family (`illustrious` over `sdxl`, `flux1krea`
    //     over `flux`, `krea2` untouched) — a structural result never downgrades
    //     a compatible sub-family, and never overrides a family it cannot classify.
    if let Some(structural) = result
        .get("architecture")
        .map(String::as_str)
        .and_then(family_from_architecture)
    {
        let current = result.get("family").map(String::as_str);
        let (should_override, is_mismatch) = if structural == "anima" {
            let mismatch = current.is_some_and(|f| f != "unknown" && f != structural);
            (true, mismatch)
        } else {
            match current {
                None | Some("unknown") => (true, false),
                Some(soft) => match (architecture_class(soft), architecture_class(structural)) {
                    (Some(sc), Some(tc)) if sc != tc => (true, true),
                    _ => (false, false),
                },
            }
        };
        if is_mismatch {
            if let Some(soft) = result.get("family").cloned() {
                result.insert("filename_family_mismatch".to_string(), soft);
            }
        }
        if should_override {
            result.insert("family".to_string(), structural.to_string());
            result.insert(
                "is_sdxl_like".to_string(),
                is_sdxl_like_family(structural).to_string(),
            );
        }
    }

    // Last-resort kind detection: a file whose weights were unreadable or whose
    // tensor names matched nothing still declares a family, and some families are
    // never shipped as a full checkpoint at all.
    if !result.contains_key("model_kind") {
        if let Some(family) = result.get("family") {
            if family_requires_separate_clip(family) {
                result.insert("model_kind".to_string(), "diffusion_model".to_string());
                result.insert("model_kind_source".to_string(), "family".to_string());
            }
        }
    }

    // Recommendations are a split-model concern: a full checkpoint bakes its own
    // CLIP and VAE. Keyed on what the file *is* rather than the folder it sits in,
    // so a diffusion model in `checkpoints/` still gets its encoder and VAE picked
    // out. Restricted to the two model categories a generation can load from —
    // other categories (loras, ...) have no loader to recommend for.
    let effective_is_split = matches!(category, "checkpoints" | "diffusion_models")
        && (category == "diffusion_models"
            || result.get("model_kind").map(String::as_str) == Some("diffusion_model"));
    if effective_is_split {
        // The helpers gate on this category internally; a reclassified checkpoint
        // must take the same path as a correctly-placed diffusion model.
        let category = "diffusion_models";
        let family = result
            .get("family")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        if let Ok(vaes) = state.get_models_list("vae").await {
            if let Some(recommended_vae) = recommended_vae_from_available(category, &family, &vaes)
            {
                result.insert("recommended_vae".to_string(), recommended_vae);
            }
        }
        if let Ok(encoders) = state.get_models_list("text_encoders").await {
            if let Some((recommended_clip_model, recommended_clip_type)) =
                recommended_clip_from_available(category, &family, &encoders)
            {
                // The model key is omitted (not defaulted) when no installed
                // encoder is compatible, so the frontend can offer a download
                // instead of silently loading a mismatched encoder.
                if let Some(recommended_clip_model) = recommended_clip_model {
                    result.insert("recommended_clip_model".to_string(), recommended_clip_model);
                }
                result.insert(
                    "recommended_clip_type".to_string(),
                    recommended_clip_type.to_string(),
                );
            }
        }
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn read_modelspec(
    state: State<'_, Arc<AppState>>,
    category: String,
    filename: String,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    read_modelspec_internal(state.inner(), &category, &filename).await
}

/// Parse the safetensors JSON header and extract only the prediction-related
/// fields used by the frontend runtime path.
pub(crate) fn read_safetensors_runtime_metadata(
    path: &std::path::Path,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    let mut file = std::fs::File::open(path)?;

    let mut size_buf = [0u8; 8];
    file.read_exact(&mut size_buf)?;
    let header_size = u64::from_le_bytes(size_buf) as usize;

    if header_size > 100 * 1024 * 1024 {
        return Err(AppError::Other("Safetensors header too large".into()));
    }

    let mut header_buf = vec![0u8; header_size];
    file.read_exact(&mut header_buf)?;

    let header: Value = serde_json::from_slice(&header_buf)?;

    let metadata = match header.get("__metadata__") {
        Some(Value::Object(m)) => m,
        _ => &serde_json::Map::new(),
    };

    let mut result = std::collections::HashMap::new();
    for (field, output_key) in [
        ("modelspec.prediction_type", "prediction_type"),
        ("modelspec.predict_key", "predict_key"),
    ] {
        if let Some(value) = metadata.get(field).and_then(|v| v.as_str()) {
            result.insert(output_key.to_string(), value.to_string());
        }
    }

    if header.get("v_pred").is_some() {
        result.insert("header_v_pred".to_string(), "true".to_string());
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

/// Ceiling on a single `__metadata__` value shipped to the frontend. A ModelSpec
/// thumbnail is a base64 data URI and is normally well under a megabyte; without
/// a cap a pathological blob would cross IPC on every model selection.
const MAX_METADATA_VALUE_BYTES: usize = 4 * 1024 * 1024;

/// Parse the safetensors JSON header and extract every `modelspec.*` field.
///
/// All fields are returned with the `modelspec.` prefix stripped, plus:
/// - `modelspec_keys`: comma-joined list of the field names that were actually
///   declared in the file, so the frontend can render unrecognised ModelSpec
///   fields without also surfacing the derived keys added by
///   [`read_modelspec_internal`].
/// - `architecture_inferred`: `"true"` when `architecture` came from tensor-key
///   inference rather than a declared `modelspec.architecture`.
pub(crate) fn read_safetensors_modelspec(
    path: &std::path::Path,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    let mut file = std::fs::File::open(path)?;

    // First 8 bytes: little-endian u64 header size
    let mut size_buf = [0u8; 8];
    file.read_exact(&mut size_buf)?;
    let header_size = u64::from_le_bytes(size_buf) as usize;

    // Sanity check: headers shouldn't be larger than 100 MB
    if header_size > 100 * 1024 * 1024 {
        return Err(AppError::Other("Safetensors header too large".into()));
    }

    // Read the JSON header
    let mut header_buf = vec![0u8; header_size];
    file.read_exact(&mut header_buf)?;

    let header: Value = serde_json::from_slice(&header_buf)?;

    let metadata = match header.get("__metadata__") {
        Some(Value::Object(m)) => m,
        _ => &serde_json::Map::new(),
    };

    let mut result = std::collections::HashMap::new();
    let mut declared_fields: Vec<String> = Vec::new();
    for (key, value) in metadata {
        // `__metadata__` is specified as string→string, but some trainers write
        // raw numbers and bools. Stringify those instead of dropping the field.
        let text = match value {
            Value::Null => continue,
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if let Some(field) = key.strip_prefix("modelspec.") {
            if text.len() > MAX_METADATA_VALUE_BYTES {
                log::warn!(
                    "Skipping oversized ModelSpec field '{}' ({} bytes) in {}",
                    field,
                    text.len(),
                    path.display()
                );
                continue;
            }
            declared_fields.push(field.to_string());
            result.insert(field.to_string(), text);
        } else if key == "prediction_type" && !result.contains_key("prediction_type") {
            result.insert("prediction_type".to_string(), text);
        }
    }

    if !declared_fields.is_empty() {
        declared_fields.sort();
        result.insert("modelspec_keys".to_string(), declared_fields.join(","));
    }

    if header.get("v_pred").is_some() {
        result.insert("header_v_pred".to_string(), "true".to_string());
    }

    // If no modelspec.architecture, infer from tensor key patterns in the header
    if let Some(Value::Object(top)) = Some(&header) {
        if !result.contains_key("architecture") {
            if let Some(arch) = infer_architecture_from_keys(top) {
                result.insert("architecture".to_string(), arch);
                result.insert("architecture_inferred".to_string(), "true".to_string());
            }
        }
        // Which loader the weights actually need, independent of the folder the
        // file sits in. `__metadata__` is not a tensor and cannot match any of
        // the structural prefixes, so it is left in place.
        let names: Vec<&str> = top.keys().map(String::as_str).collect();
        if let Some(kind) = infer_model_kind_from_key_names(&names) {
            result.insert("model_kind".to_string(), kind.to_string());
            result.insert("model_kind_source".to_string(), "tensor_keys".to_string());
        }
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

// --- GGUF header parsing ---------------------------------------------------
//
// GGUF files have no safetensors JSON header, but their binary header still
// carries a `general.architecture` string and the full tensor-name table, so
// the same structural detection used for safetensors applies. Only the header
// (metadata KV block + tensor-info block) is read, never the tensor data. The
// layout and value-type tags follow the GGUF v2/v3 spec as implemented by
// city96/ComfyUI-GGUF, which is what actually loads these files in the app.

const GGUF_MAGIC: u32 = 0x4655_4747; // "GGUF" little-endian
const GGUF_TYPE_UINT8: u32 = 0;
const GGUF_TYPE_INT8: u32 = 1;
const GGUF_TYPE_UINT16: u32 = 2;
const GGUF_TYPE_INT16: u32 = 3;
const GGUF_TYPE_UINT32: u32 = 4;
const GGUF_TYPE_INT32: u32 = 5;
const GGUF_TYPE_FLOAT32: u32 = 6;
const GGUF_TYPE_BOOL: u32 = 7;
const GGUF_TYPE_STRING: u32 = 8;
const GGUF_TYPE_ARRAY: u32 = 9;
const GGUF_TYPE_UINT64: u32 = 10;
const GGUF_TYPE_INT64: u32 = 11;
const GGUF_TYPE_FLOAT64: u32 = 12;

const GGUF_MAX_STRING_LEN: u64 = 1024 * 1024; // 1 MB
const GGUF_MAX_KV_COUNT: u64 = 10_000;
const GGUF_MAX_TENSOR_COUNT: u64 = 200_000;
const GGUF_MAX_ARRAY_COUNT: u64 = 1_000_000_000;
const GGUF_MAX_NDIMS: u32 = 8;

fn read_u32_le<R: Read>(reader: &mut R) -> Result<u32, AppError> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64_le<R: Read>(reader: &mut R) -> Result<u64, AppError> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Skip `n` bytes without allocating a buffer for them.
fn skip_bytes<R: Read>(reader: &mut R, mut n: u64) -> Result<(), AppError> {
    let mut chunk = [0u8; 4096];
    while n > 0 {
        let take = n.min(chunk.len() as u64) as usize;
        reader.read_exact(&mut chunk[..take])?;
        n -= take as u64;
    }
    Ok(())
}

/// Byte size of a fixed-width GGUF scalar value type, or `None` for
/// variable-length types (string, array).
fn gguf_scalar_size(vtype: u32) -> Option<u64> {
    match vtype {
        GGUF_TYPE_UINT8 | GGUF_TYPE_INT8 | GGUF_TYPE_BOOL => Some(1),
        GGUF_TYPE_UINT16 | GGUF_TYPE_INT16 => Some(2),
        GGUF_TYPE_UINT32 | GGUF_TYPE_INT32 | GGUF_TYPE_FLOAT32 => Some(4),
        GGUF_TYPE_UINT64 | GGUF_TYPE_INT64 | GGUF_TYPE_FLOAT64 => Some(8),
        _ => None,
    }
}

fn read_gguf_string<R: Read>(reader: &mut R) -> Result<String, AppError> {
    let len = read_u64_le(reader)?;
    if len > GGUF_MAX_STRING_LEN {
        return Err(AppError::Other("GGUF string too long".into()));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|_| AppError::Other("Invalid UTF-8 in GGUF string".into()))
}

fn skip_gguf_string<R: Read>(reader: &mut R) -> Result<(), AppError> {
    let len = read_u64_le(reader)?;
    if len > GGUF_MAX_STRING_LEN {
        return Err(AppError::Other("GGUF string too long".into()));
    }
    skip_bytes(reader, len)
}

/// Skip a single GGUF metadata value of the given type (only the value body —
/// the type tag has already been read). Nested arrays are rejected; unknown
/// types are an error so a malformed header fails cleanly rather than desyncing.
fn skip_gguf_value<R: Read>(reader: &mut R, vtype: u32) -> Result<(), AppError> {
    if let Some(size) = gguf_scalar_size(vtype) {
        return skip_bytes(reader, size);
    }
    match vtype {
        GGUF_TYPE_STRING => skip_gguf_string(reader),
        GGUF_TYPE_ARRAY => {
            let elem_type = read_u32_le(reader)?;
            if elem_type == GGUF_TYPE_ARRAY {
                return Err(AppError::Other("Nested GGUF arrays unsupported".into()));
            }
            let count = read_u64_le(reader)?;
            if count > GGUF_MAX_ARRAY_COUNT {
                return Err(AppError::Other("GGUF array too long".into()));
            }
            if let Some(size) = gguf_scalar_size(elem_type) {
                skip_bytes(reader, size.saturating_mul(count))
            } else if elem_type == GGUF_TYPE_STRING {
                for _ in 0..count {
                    skip_gguf_string(reader)?;
                }
                Ok(())
            } else {
                Err(AppError::Other("Unknown GGUF array element type".into()))
            }
        }
        _ => Err(AppError::Other("Unknown GGUF value type".into())),
    }
}

/// Map a city96/ComfyUI-GGUF `general.architecture` value onto the app's
/// internal architecture string (the same space `infer_architecture_from_keys`
/// produces, so `family_from_architecture` consumes it unchanged).
fn gguf_arch_to_app_arch(gguf_arch: &str) -> Option<&'static str> {
    match gguf_arch.to_ascii_lowercase().as_str() {
        "flux" => Some("flux"),
        "sd1" => Some("stable-diffusion-v1-5"),
        "sdxl" => Some("stable-diffusion-xl-v1-base"),
        "sd3" => Some("stable-diffusion-3-medium"),
        "aura" | "auraflow" => Some("auraflow"),
        "wan" | "wan21" | "wan2.1" => Some("wan2.1"),
        "cosmos" => Some("cosmos_predict2"),
        "qwen_image" | "qwenimage" | "qwen-image" => Some("qwen_image"),
        "pixart" => Some("pixart"),
        "hunyuan_dit" | "hunyuandit" => Some("hunyuandit"),
        "kolors" => Some("kolors"),
        _ => None,
    }
}

/// Parse a GGUF header for structural architecture detection.
///
/// Returns `{"architecture": ..., "gguf_architecture": <raw>}` when the file's
/// architecture resolves from its tensor names or `general.architecture` key.
/// Any parse failure (truncated, non-GGUF, unexpected type) logs a warning and
/// returns `Ok(None)`: a malformed GGUF must degrade to filename detection,
/// never fail the whole modelspec read.
fn read_gguf_architecture_meta(
    path: &std::path::Path,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    match read_gguf_architecture_meta_inner(path) {
        Ok(meta) => Ok(meta),
        Err(e) => {
            log::warn!("GGUF header parse failed for {}: {}", path.display(), e);
            Ok(None)
        }
    }
}

fn read_gguf_architecture_meta_inner(
    path: &std::path::Path,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    // BufReader is essential: the header is walked with many tiny read_exact
    // calls (one per string length, per tensor name, per dim), so an unbuffered
    // File would issue thousands of syscalls.
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);

    let magic = read_u32_le(&mut reader)?;
    if magic != GGUF_MAGIC {
        // Not a GGUF container; nothing to detect.
        return Ok(None);
    }
    let version = read_u32_le(&mut reader)?;
    if !(2..=3).contains(&version) {
        // v1 used 32-bit counts; attempting the 64-bit layout below on it would
        // misparse, but the caps catch the garbage and fall back to Ok(None).
        log::warn!("Unexpected GGUF version {} for {}", version, path.display());
    }

    let tensor_count = read_u64_le(&mut reader)?;
    if tensor_count > GGUF_MAX_TENSOR_COUNT {
        return Err(AppError::Other("GGUF tensor count too large".into()));
    }
    let kv_count = read_u64_le(&mut reader)?;
    if kv_count > GGUF_MAX_KV_COUNT {
        return Err(AppError::Other("GGUF KV count too large".into()));
    }

    // Metadata KV block: capture general.architecture, skip everything else.
    let mut general_arch: Option<String> = None;
    for _ in 0..kv_count {
        let key = read_gguf_string(&mut reader)?;
        let vtype = read_u32_le(&mut reader)?;
        if key == "general.architecture" && vtype == GGUF_TYPE_STRING {
            general_arch = Some(read_gguf_string(&mut reader)?);
        } else {
            skip_gguf_value(&mut reader, vtype)?;
        }
    }

    // Tensor-info block: name, dims, ggml type, offset. Collect names only.
    let mut names: Vec<String> = Vec::new();
    for _ in 0..tensor_count {
        let name = read_gguf_string(&mut reader)?;
        let n_dims = read_u32_le(&mut reader)?;
        if n_dims > GGUF_MAX_NDIMS {
            return Err(AppError::Other(
                "GGUF tensor has too many dimensions".into(),
            ));
        }
        skip_bytes(&mut reader, (n_dims as u64) * 8)?; // dims: n_dims * u64
        let _ggml_type = read_u32_le(&mut reader)?;
        let _offset = read_u64_le(&mut reader)?;
        names.push(name);
    }

    // Tensor names are ground truth and are checked first: they catch Anima's
    // `llm_adapter`, which a `general.architecture` of "cosmos" cannot tell from
    // plain Cosmos. Fall back to the declared architecture key otherwise.
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let architecture = infer_architecture_from_key_names(&name_refs).or_else(|| {
        general_arch
            .as_deref()
            .and_then(gguf_arch_to_app_arch)
            .map(str::to_string)
    });

    let mut result = std::collections::HashMap::new();
    // GGUF quantisation is only ever applied to a single component, so a GGUF in
    // a model folder is always a bare diffusion model — there is no such thing as
    // a GGUF full checkpoint with baked CLIP and VAE. Recorded even when the
    // architecture is unknown, since the kind is certain either way.
    result.insert("model_kind".to_string(), "diffusion_model".to_string());
    result.insert("model_kind_source".to_string(), "gguf".to_string());
    if let Some(architecture) = architecture {
        result.insert("architecture".to_string(), architecture);
    }
    if let Some(raw) = general_arch {
        result.insert("gguf_architecture".to_string(), raw);
    }
    Ok(Some(result))
}

/// Container prefix that the bulk of a checkpoint's tensors sit under.
///
/// Diffusion weights are rarely stored under bare names: ldm/sgm checkpoints
/// wrap them in `model.diffusion_model.`, the Cosmos family (including Anima)
/// uses `net.`, and audio models use `model.model.`. Architecture patterns only
/// match once the wrapper is stripped. Mirrors ComfyUI's
/// `unet_prefix_from_state_dict`, which is what actually has to load the file.
fn dominant_tensor_key_prefix(names: &[&str]) -> &'static str {
    const CANDIDATES: [&str; 4] = [
        "model.diffusion_model.",
        "diffusion_model.",
        "model.model.",
        "net.",
    ];
    let mut best = "";
    let mut best_count = 0usize;
    for candidate in CANDIDATES {
        let count = names.iter().filter(|k| k.starts_with(candidate)).count();
        if count > best_count {
            best_count = count;
            best = candidate;
        }
    }
    // A few stray matches are noise; a real container prefix covers most of the file.
    if best_count > 5 {
        best
    } else {
        ""
    }
}

/// Infer model architecture from safetensors header keys.
///
/// Thin wrapper over [`infer_architecture_from_key_names`] that projects the
/// JSON header's object keys into `&str`s. GGUF detection calls the key-names
/// form directly with its tensor-name table.
fn infer_architecture_from_keys(header: &serde_json::Map<String, Value>) -> Option<String> {
    let names: Vec<&str> = header.keys().map(String::as_str).collect();
    infer_architecture_from_key_names(&names)
}

/// Infer model architecture by examining tensor key name patterns.
///
/// Patterns are matched against names with the container prefix stripped, so a
/// checkpoint saved as `model.diffusion_model.double_blocks.0…` is recognised
/// the same as a bare `double_blocks.0…` dump. The markers themselves follow
/// ComfyUI's `detect_unet_config`. Shared by the safetensors header reader and
/// the GGUF header reader — both hand it their tensor-name list.
fn infer_architecture_from_key_names(names: &[&str]) -> Option<String> {
    let prefix = dominant_tensor_key_prefix(names);
    let bare: Vec<&str> = if prefix.is_empty() {
        names.to_vec()
    } else {
        names
            .iter()
            .filter_map(|k| k.strip_prefix(prefix))
            .collect()
    };

    let starts_with = |p: &str| bare.iter().any(|k| k.starts_with(p));
    let contains = |s: &str| bare.iter().any(|k| k.contains(s));
    let has_key = |name: &str| bare.contains(&name);

    // Anima: a Cosmos-Predict2 backbone plus a Qwen3 LLM adapter. Checked before
    // plain Cosmos because it shares that backbone — the adapter is what makes
    // it Anima, and what makes it require the Qwen3-0.6B text encoder.
    if starts_with("llm_adapter.") && contains("blocks.0.mlp.layer1.") {
        return Some("anima".to_string());
    }
    if starts_with("blocks.0.mlp.layer1.") {
        return Some("cosmos_predict2".to_string());
    }

    // Wan 2.1 (DiT video model)
    if has_key("head.modulation") {
        return Some("wan2.1".to_string());
    }

    // Flux: uses double_blocks / single_blocks (DiT architecture)
    if starts_with("double_blocks.") && starts_with("single_blocks.") {
        return Some("flux".to_string());
    }

    // Qwen-Image: MMDiT with a dedicated text-stream norm
    if has_key("txt_norm.weight") && starts_with("transformer_blocks.") {
        return Some("qwen_image".to_string());
    }

    // SD3 / SD3.5: uses joint_blocks (MMDiT architecture)
    if starts_with("joint_blocks.") || starts_with("transformer_blocks.0.attn.add_q_proj.") {
        return Some("stable-diffusion-3-medium".to_string());
    }

    // AuraFlow: has single_transformer_blocks + double_transformer_blocks
    if starts_with("double_transformer_blocks.") && starts_with("single_transformer_blocks.") {
        return Some("auraflow".to_string());
    }

    // HunyuanDiT: mlp_t5 + pooler patterns
    if contains("mlp_t5") {
        return Some("hunyuandit".to_string());
    }

    // Stable Cascade: has "down_blocks" + "up_blocks" with "clip_txt_mapper"
    if contains("clip_txt_mapper") {
        return Some("stable_cascade".to_string());
    }

    // PixArt: alpha/sigma share the adaln_single conditioning stack
    if starts_with("adaln_single.") {
        return Some("pixart".to_string());
    }

    // Kolors: ChatGLM-based text encoder
    if contains("chatglm") {
        return Some("kolors".to_string());
    }

    // Classic UNet architectures (SD 1.5 / SDXL)
    let is_unet_prefix = prefix == "model.diffusion_model." || prefix == "diffusion_model.";
    if is_unet_prefix || starts_with("input_blocks.") {
        // SDXL check: has label_emb (y-embedding for SDXL's 2816-dim vector) or
        // conditioner.embedders.1 (dual text encoder in a full checkpoint, which
        // sits outside the diffusion-model prefix so it is matched unstripped).
        let is_sdxl = contains("label_emb")
            || names
                .iter()
                .any(|k| k.starts_with("conditioner.embedders.1."));
        if is_sdxl {
            return Some("stable-diffusion-xl-v1-base".to_string());
        }
        return Some("stable-diffusion-v1-5".to_string());
    }

    None
}

/// Tensor trees that only a full single-file checkpoint carries. `first_stage_model.`
/// is the ldm/sgm VAE, the others are baked text encoders (`text_encoders.` is
/// ComfyUI's all-in-one packing used by the fp8 Flux/SD3 releases). Presence of
/// any of these is what makes `CheckpointLoaderSimple` able to return a CLIP and
/// a VAE instead of `None`.
const FULL_CHECKPOINT_KEY_PREFIXES: [&str; 4] = [
    "cond_stage_model.",
    "conditioner.embedders.",
    "first_stage_model.",
    "text_encoders.",
];

/// Containers that hold diffusion weights. A full checkpoint carries these too,
/// so they are only conclusive once the prefixes above are known to be absent.
const DIFFUSION_KEY_PREFIXES: [&str; 7] = [
    "model.diffusion_model.",
    "diffusion_model.",
    "double_blocks.",
    "single_blocks.",
    "joint_blocks.",
    "transformer_blocks.",
    "net.",
];

/// A handful of matches is noise (a stray buffer, a merge artefact); a real
/// tensor tree covers dozens of keys. Same reasoning as
/// [`dominant_tensor_key_prefix`].
const MIN_TENSOR_TREE_KEYS: usize = 5;

/// Infer whether a file is a full checkpoint or a bare diffusion model from its
/// tensor names.
///
/// This is the structural answer to "which loader does this file need", which is
/// otherwise guessed from whichever folder the file happens to sit in. A Flux
/// unet dropped into `checkpoints/` and an SDXL checkpoint dropped into
/// `diffusion_models/` both load wrong (or not at all) on the folder-based guess,
/// and the weights themselves are the only signal that cannot be renamed away.
///
/// Returns `None` when neither signature is present (a LoRA, an embedding, an
/// unrecognised container) so callers keep their folder-based default rather than
/// acting on a coin flip.
fn infer_model_kind_from_key_names(names: &[&str]) -> Option<&'static str> {
    let has_tree = |prefix: &str| {
        names.iter().filter(|k| k.starts_with(prefix)).count() > MIN_TENSOR_TREE_KEYS
    };
    if FULL_CHECKPOINT_KEY_PREFIXES.iter().copied().any(has_tree) {
        return Some("checkpoint");
    }
    if DIFFUSION_KEY_PREFIXES.iter().copied().any(has_tree) {
        return Some("diffusion_model");
    }
    None
}

/// Map a structurally inferred architecture onto the app's model family.
///
/// Only architectures that map unambiguously onto a single family are listed:
/// tensor keys cannot tell a Flux dev from a schnell, or an Illustrious from a
/// stock SDXL, so those resolve to the generic family and are only ever used to
/// fill in a family that other detection left unknown.
fn family_from_architecture(architecture: &str) -> Option<&'static str> {
    match architecture {
        "anima" => Some("anima"),
        "wan2.1" => Some("wan"),
        "flux" => Some("flux"),
        "qwen_image" => Some("qwen"),
        "stable-diffusion-3-medium" => Some("sd3"),
        "auraflow" => Some("auraflow"),
        "pixart" => Some("pixart"),
        "hunyuandit" => Some("hunyuandit"),
        "stable_cascade" => Some("cascade"),
        "kolors" => Some("kolors"),
        "stable-diffusion-xl-v1-base" => Some("sdxl"),
        "stable-diffusion-v1-5" => Some("sd15"),
        _ => None,
    }
}

/// Structural class of a model family. Families that share tensor-key topology
/// share a class, because structural detection can prove which class a file
/// belongs to but not which sub-family within it (tensor keys cannot tell an
/// Illustrious from a stock SDXL, or a Flux dev from a schnell).
///
/// `None` means the family has no implemented structural signature, so a
/// structural guess must never override it (e.g. Krea 2, Z-Image, Flux 2
/// variants). This keeps the veto conservative: it only fires when the file's
/// weights prove a *different, known* class than the filename claimed.
fn architecture_class(family: &str) -> Option<&'static str> {
    match family {
        // Classic SDXL-shaped UNets. Mugen is SDXL-shaped too (it is driven with
        // SDXL-style sampling in templates/mod.rs), so a mugen-by-filename tag is
        // a compatible refinement of a structural SDXL result, not a conflict.
        "sdxl" | "illustrious" | "pony" | "mugen" => Some("sdxl"),
        "sd15" => Some("sd15"),
        "flux" | "flux1d" | "flux1s" | "flux1krea" | "chroma" => Some("flux"),
        "anima" => Some("anima"),
        "wan" => Some("wan"),
        "qwen" => Some("qwen_image"),
        "sd3" => Some("sd3"),
        "auraflow" => Some("auraflow"),
        "pixart" => Some("pixart"),
        "hunyuandit" => Some("hunyuandit"),
        "cascade" => Some("cascade"),
        "kolors" => Some("kolors"),
        _ => None,
    }
}

/// Read `{stem}.png` / `.jpg` sidecar preview next to a model file, if present.
pub(crate) fn read_model_sidecar_thumbnail_pub(path: &std::path::Path) -> Option<String> {
    read_model_sidecar_thumbnail(path)
}

fn read_model_sidecar_thumbnail(path: &std::path::Path) -> Option<String> {
    let model_dir = path.parent()?;
    let stem = path.file_stem()?.to_str()?;
    let candidates = [
        model_dir.join(format!("{}.png", stem)),
        model_dir.join(format!("{}.jpg", stem)),
        model_dir.join(format!("{}.jpeg", stem)),
        model_dir.join(format!("{}.preview.png", stem)),
        model_dir.join(format!("{}.preview.jpg", stem)),
    ];
    for candidate in &candidates {
        if candidate.exists() {
            if let Ok(bytes) = std::fs::read(candidate) {
                use base64::Engine as _;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let mime = match candidate.extension().and_then(|e| e.to_str()).unwrap_or("") {
                    "jpg" | "jpeg" => "image/jpeg",
                    _ => "image/png",
                };
                return Some(format!("data:{};base64,{}", mime, b64));
            }
            break;
        }
    }
    None
}

/// Combined LoRA information from ModelSpec + CivitAI.
#[derive(Debug, Serialize)]
pub struct LoraCivitaiInfo {
    pub filename: String,
    pub hash: Option<String>,
    pub family: Option<String>,
    /// "data:<mime>;base64,..." for local sidecar, "https://..." for CivitAI, or None.
    pub thumbnail_url: Option<String>,
    pub civitai_name: Option<String>,
    pub civitai_description: Option<String>,
    pub civitai_model_id: Option<u64>,
    pub civitai_version_id: Option<u64>,
    pub civitai_base_model: Option<String>,
    pub civitai_images: Vec<LoraCivitaiImage>,
    pub civitai_trigger_words: Vec<String>,
    pub civitai_download_count: Option<u64>,
    pub civitai_thumbs_up_count: Option<u64>,
    pub civitai_creator: Option<String>,
    pub modelspec_title: Option<String>,
    pub modelspec_author: Option<String>,
    pub modelspec_architecture: Option<String>,
    pub modelspec_trigger_phrase: Option<String>,
    pub modelspec_description: Option<String>,
    pub modelspec_tags: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoraCivitaiImage {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub nsfw: Option<String>,
}

/// Combined checkpoint information from ModelSpec + local sidecar thumbnail + CivitAI hash lookup.
#[derive(Debug, Serialize)]
pub struct CheckpointCivitaiInfo {
    pub filename: String,
    pub hash: Option<String>,
    pub display_name: Option<String>,
    pub base_model: Option<String>,
    pub family: Option<String>,
    /// "data:<mime>;base64,..." for local sidecar, "https://..." for CivitAI, or None.
    pub thumbnail_url: Option<String>,
    pub civitai_model_id: Option<u64>,
    pub civitai_version_id: Option<u64>,
    pub civitai_description: Option<String>,
    /// All sample images returned by CivitAI (independent of whether a sidecar exists).
    pub civitai_images: Vec<LoraCivitaiImage>,
    pub civitai_download_count: Option<u64>,
    pub civitai_thumbs_up_count: Option<u64>,
    pub civitai_creator: Option<String>,
    pub modelspec_title: Option<String>,
    pub modelspec_author: Option<String>,
    pub modelspec_architecture: Option<String>,
    pub modelspec_description: Option<String>,
    pub modelspec_tags: Option<String>,
}

/// All known subdirectory names that map to a given ComfyUI model category.
/// Must stay in sync with the YAML generated in `process.rs`.
pub(crate) fn category_subdirs(category: &str) -> &'static [&'static str] {
    match category {
        "checkpoints" => &[
            "checkpoints",
            "Stable-diffusion",
            "Stable-Diffusion",
            "StableDiffusion",
            "models/Stable-diffusion",
            "Models/Stable-Diffusion",
            "Models/StableDiffusion",
        ],
        "loras" => &[
            "loras",
            "lora",
            "Lora",
            "LoRA",
            "LoRAs",
            "LORA",
            "Loras",
            "LyCORIS",
            "lycoris",
            "models/Lora",
            "models/loras",
            "models/LyCORIS",
            "Models/Lora",
            "Models/loras",
            "Models/LyCORIS",
        ],
        "vae" => &["vae", "VAE", "models/VAE", "Models/VAE"],
        "upscale_models" => &[
            "upscale_models",
            "ESRGAN",
            "models/ESRGAN",
            "models/RealESRGAN",
            "Models/ESRGAN",
            "Models/RealESRGAN",
        ],
        "embeddings" => &[
            "embeddings",
            "models/TextualInversion",
            "Models/TextualInversion",
        ],
        "controlnet" => &[
            "controlnet",
            "ControlNet",
            "models/ControlNet",
            "Models/ControlNet",
        ],
        "clip" => &["clip", "models/clip", "Models/clip"],
        "unet" => &["unet", "models/unet", "Models/unet"],
        "diffusion_models" => &[
            "diffusion_models",
            "DiffusionModels",
            "models/diffusion_models",
            "models/DiffusionModels",
            "Models/diffusion_models",
            "Models/DiffusionModels",
        ],
        "text_encoders" => &[
            "text_encoders",
            "TextEncoders",
            "models/text_encoders",
            "models/TextEncoders",
            "Models/text_encoders",
            "Models/TextEncoders",
        ],
        "ultralytics" => &["ultralytics", "models/ultralytics", "Models/ultralytics"],
        "model_patches" => &[
            "model_patches",
            "ModelPatches",
            "models/model_patches",
            "Models/model_patches",
        ],
        _ => &[],
    }
}

/// Resolve a model file path by searching the primary ComfyUI models directory
/// and then any extra_model_paths directories (newline-separated).
/// For extra paths, tries all known subdirectory variants for the category
/// (matching the YAML config given to ComfyUI) and also tries the file
/// directly in the root (flat directory case).
pub(crate) fn resolve_model_path(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    category: &str,
    filename: &str,
) -> Option<std::path::PathBuf> {
    if !is_safe_path_component(category) || !is_safe_relative_model_path(filename) {
        return None;
    }

    // Primary ComfyUI directory always uses the canonical category name
    let primary = std::path::Path::new(comfyui_path)
        .join("models")
        .join(category)
        .join(filename);
    if primary.exists() {
        return Some(primary);
    }

    if let Some(extra) = extra_model_paths {
        let subdirs = category_subdirs(category);
        for dir in extra
            .split('\n')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let base = resolve_extra_model_root(std::path::Path::new(dir));
            // Try all known subdirectory variants for this category
            for subdir in subdirs {
                let candidate = base.join(subdir).join(filename);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            // Flat directory: file directly in the root
            let flat = base.join(filename);
            if flat.exists() {
                return Some(flat);
            }
        }
    }
    None
}

pub(crate) fn validate_lora_files_for_generation(
    comfyui_path: &str,
    extra_model_paths: Option<&str>,
    loras: &[crate::comfyui::types::LoraParam],
) -> Result<(), AppError> {
    for lora in loras {
        let name = lora.name.trim();
        if name.is_empty() {
            continue;
        }
        let path = resolve_model_path(comfyui_path, extra_model_paths, "loras", name)
            .ok_or_else(|| AppError::InvalidWorkflow(format!("LoRA file not found: '{}'", name)))?;
        crate::comfyui::client::validate_downloaded_model_file(&path, name).map_err(|e| {
            AppError::InvalidWorkflow(format!(
                "LoRA file '{}' is invalid and needs to be re-downloaded: {}",
                name, e
            ))
        })?;
    }
    Ok(())
}

/// Fetch combined LoRA info: hash the file, look up on CivitAI, read ModelSpec.
/// Returns structured info for the LoRA gallery panel.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_lora_civitai_info(
    state: State<'_, Arc<AppState>>,
    filename: String,
) -> Result<LoraCivitaiInfo, AppError> {
    let (comfyui_path, extra_model_paths, civitai_api_key) = {
        let config = state.config.read().await;
        if config.comfyui_path.is_empty() {
            return Err(AppError::Other("ComfyUI path not configured".into()));
        }
        (
            config.comfyui_path.clone(),
            config.extra_model_paths.clone(),
            config.civitai_api_key.clone(),
        )
    };

    let path = resolve_model_path(
        &comfyui_path,
        extra_model_paths.as_deref(),
        "loras",
        &filename,
    )
    .ok_or_else(|| {
        log::warn!(
            "LoRA file not found: '{}' (comfyui_path='{}', extra_model_paths={:?})",
            filename,
            comfyui_path,
            extra_model_paths
        );
        AppError::Other(format!("LoRA file not found: {}", filename))
    })?;

    log::debug!("Resolved LoRA '{}' → {:?}", filename, path);

    let sidecar_thumbnail = read_model_sidecar_thumbnail(&path);
    let resolved_modelspec = read_modelspec_internal(state.inner(), "loras", &filename).await?;

    // Read modelspec in parallel-friendly manner (sync I/O in blocking task)
    let modelspec = if filename.ends_with(".safetensors") {
        read_safetensors_modelspec(&path).ok().flatten()
    } else {
        None
    };

    // Hash the file in a blocking task (can take seconds for large files)
    let path_clone = path.clone();
    let sha256 = tokio::task::spawn_blocking(move || full_sha256(&path_clone))
        .await
        .map_err(|e| AppError::Other(format!("Hash task failed: {}", e)))??;
    let autov2 = autov2_hash(&sha256);

    // Look up on CivitAI by hash
    let civitai_url = format!(
        "https://civitai.com/api/v1/model-versions/by-hash/{}",
        autov2
    );
    let mut civitai_req = state
        .http_client
        .get(&civitai_url)
        .header("User-Agent", "MooshieUI/0.3.9");
    if let Some(key) = civitai_api_key.filter(|v| !v.trim().is_empty()) {
        civitai_req = civitai_req.bearer_auth(key);
    }
    let civitai_resp = civitai_req.send().await;

    let mut info = LoraCivitaiInfo {
        filename: filename.clone(),
        hash: Some(autov2),
        family: resolved_modelspec
            .as_ref()
            .and_then(|m| m.get("family").cloned()),
        thumbnail_url: sidecar_thumbnail,
        civitai_name: None,
        civitai_description: None,
        civitai_model_id: None,
        civitai_version_id: None,
        civitai_base_model: None,
        civitai_images: Vec::new(),
        civitai_trigger_words: Vec::new(),
        civitai_download_count: None,
        civitai_thumbs_up_count: None,
        civitai_creator: None,
        modelspec_title: modelspec.as_ref().and_then(|m| m.get("title").cloned()),
        modelspec_author: modelspec.as_ref().and_then(|m| m.get("author").cloned()),
        modelspec_architecture: modelspec
            .as_ref()
            .and_then(|m| m.get("architecture").cloned()),
        modelspec_trigger_phrase: modelspec
            .as_ref()
            .and_then(|m| m.get("trigger_phrase").cloned()),
        modelspec_description: modelspec
            .as_ref()
            .and_then(|m| m.get("description").cloned()),
        modelspec_tags: modelspec.as_ref().and_then(|m| m.get("tags").cloned()),
    };

    // Parse CivitAI response if successful
    match &civitai_resp {
        Ok(resp) if !resp.status().is_success() => {
            log::warn!(
                "CivitAI hash lookup for lora '{}' returned status {}",
                filename,
                resp.status()
            );
        }
        Err(e) => {
            log::warn!("CivitAI hash lookup for lora '{}' failed: {}", filename, e);
        }
        _ => {}
    }
    if let Ok(resp) = civitai_resp {
        if resp.status().is_success() {
            if let Ok(data) = resp.json::<Value>().await {
                // Version-level fields
                info.civitai_version_id = data.get("id").and_then(|v| v.as_u64());
                info.civitai_base_model = data
                    .get("baseModel")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                info.civitai_name = data
                    .get("model")
                    .and_then(|m| m.get("name"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                info.civitai_model_id = data.get("modelId").and_then(|v| v.as_u64());

                // Trigger words
                if let Some(words) = data.get("trainedWords").and_then(|v| v.as_array()) {
                    info.civitai_trigger_words = words
                        .iter()
                        .filter_map(|w| w.as_str().map(String::from))
                        .collect();
                }

                // Images
                if let Some(images) = data.get("images").and_then(|v| v.as_array()) {
                    info.civitai_images = images
                        .iter()
                        .filter_map(|img| {
                            img.get("url")
                                .and_then(|u| u.as_str())
                                .map(|url| LoraCivitaiImage {
                                    url: url.to_string(),
                                    width: img
                                        .get("width")
                                        .and_then(|w| w.as_u64())
                                        .map(|w| w as u32),
                                    height: img
                                        .get("height")
                                        .and_then(|h| h.as_u64())
                                        .map(|h| h as u32),
                                    nsfw: img.get("nsfwLevel").and_then(|n| n.as_u64()).map(|n| {
                                        if n <= 1 {
                                            "None".to_string()
                                        } else {
                                            format!("Level{}", n)
                                        }
                                    }),
                                })
                        })
                        .collect();

                    if info.thumbnail_url.is_none() {
                        info.thumbnail_url = info.civitai_images.first().map(|i| i.url.clone());
                    }
                }

                // Stats from parent model
                if let Some(stats) = data.get("stats") {
                    info.civitai_download_count =
                        stats.get("downloadCount").and_then(|v| v.as_u64());
                    info.civitai_thumbs_up_count =
                        stats.get("thumbsUpCount").and_then(|v| v.as_u64());
                }

                // Creator
                if let Some(model) = data.get("model") {
                    if let Some(desc) = model.get("description").and_then(|v| v.as_str()) {
                        // CivitAI returns HTML descriptions; store raw for now
                        info.civitai_description = Some(desc.to_string());
                    }
                }
            }
        }
    }

    Ok(info)
}

/// Fetch combined checkpoint info: ModelSpec metadata + local sidecar thumbnail + CivitAI hash lookup.
/// Always hashes the file and queries CivitAI so name, base architecture, stats, and sample images
/// are populated even when a local sidecar preview exists.
/// Sidecar search order: `{stem}.png`, `{stem}.jpg`, `{stem}.jpeg`,
/// `{stem}.preview.png`, `{stem}.preview.jpg` (same directory as the model file).
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_checkpoint_civitai_info(
    state: State<'_, Arc<AppState>>,
    filename: String,
) -> Result<CheckpointCivitaiInfo, AppError> {
    let (comfyui_path, extra_model_paths, civitai_api_key) = {
        let config = state.config.read().await;
        if config.comfyui_path.is_empty() {
            return Err(AppError::Other("ComfyUI path not configured".into()));
        }
        (
            config.comfyui_path.clone(),
            config.extra_model_paths.clone(),
            config.civitai_api_key.clone(),
        )
    };

    let path = resolve_model_path(
        &comfyui_path,
        extra_model_paths.as_deref(),
        "checkpoints",
        &filename,
    )
    .ok_or_else(|| AppError::Other(format!("Checkpoint file not found: {}", filename)))?;

    // Read all modelspec fields (safetensors only, fast)
    let modelspec = if filename.ends_with(".safetensors") {
        read_safetensors_modelspec(&path).ok().flatten()
    } else {
        None
    };

    let sidecar_thumbnail = read_model_sidecar_thumbnail(&path);
    let resolved_modelspec =
        read_modelspec_internal(state.inner(), "checkpoints", &filename).await?;

    let mut info = CheckpointCivitaiInfo {
        filename: filename.clone(),
        hash: None,
        display_name: modelspec.as_ref().and_then(|m| m.get("title").cloned()),
        base_model: resolved_modelspec
            .as_ref()
            .and_then(|m| m.get("base_model").cloned())
            .or_else(|| {
                modelspec
                    .as_ref()
                    .and_then(|m| m.get("architecture").cloned())
            }),
        family: resolved_modelspec
            .as_ref()
            .and_then(|m| m.get("family").cloned()),
        thumbnail_url: sidecar_thumbnail,
        civitai_model_id: None,
        civitai_version_id: None,
        civitai_description: None,
        civitai_images: Vec::new(),
        civitai_download_count: None,
        civitai_thumbs_up_count: None,
        civitai_creator: None,
        modelspec_title: modelspec.as_ref().and_then(|m| m.get("title").cloned()),
        modelspec_author: modelspec.as_ref().and_then(|m| m.get("author").cloned()),
        modelspec_architecture: modelspec
            .as_ref()
            .and_then(|m| m.get("architecture").cloned()),
        modelspec_description: modelspec
            .as_ref()
            .and_then(|m| m.get("description").cloned()),
        modelspec_tags: modelspec.as_ref().and_then(|m| m.get("tags").cloned()),
    };

    // Hash via spawn_blocking — checkpoints can be 5–20 GB so this runs on a thread pool.
    let path_clone = path.clone();
    let sha256_result = tokio::task::spawn_blocking(move || full_sha256(&path_clone))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    let sha256 = match sha256_result {
        Ok(h) => h,
        Err(e) => {
            log::warn!("Failed to hash checkpoint {}: {}", filename, e);
            return Ok(info); // return partial info (modelspec + sidecar if any)
        }
    };
    let autov2 = autov2_hash(&sha256);
    info.hash = Some(autov2.clone());

    // CivitAI lookup by AutoV2 hash
    let civitai_url = format!(
        "https://civitai.com/api/v1/model-versions/by-hash/{}",
        autov2
    );
    let mut civitai_req = state
        .http_client
        .get(&civitai_url)
        .header("User-Agent", "MooshieUI/0.3.9");
    if let Some(key) = civitai_api_key.filter(|v| !v.trim().is_empty()) {
        civitai_req = civitai_req.bearer_auth(key);
    }
    let civitai_resp = civitai_req.send().await;

    match &civitai_resp {
        Ok(resp) if !resp.status().is_success() => {
            log::warn!(
                "CivitAI hash lookup for checkpoint '{}' returned status {}",
                filename,
                resp.status()
            );
        }
        Err(e) => {
            log::warn!(
                "CivitAI hash lookup for checkpoint '{}' failed: {}",
                filename,
                e
            );
        }
        _ => {}
    }
    if let Ok(resp) = civitai_resp {
        if resp.status().is_success() {
            if let Ok(data) = resp.json::<Value>().await {
                info.civitai_version_id = data.get("id").and_then(|v| v.as_u64());
                info.civitai_model_id = data.get("modelId").and_then(|v| v.as_u64());

                // Prefer CivitAI base model over modelspec architecture
                if let Some(bm) = data.get("baseModel").and_then(|v| v.as_str()) {
                    info.base_model = Some(bm.to_string());
                }

                if info.display_name.is_none() {
                    info.display_name = data
                        .get("model")
                        .and_then(|m| m.get("name"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }

                // Description + creator from parent model object
                if let Some(model) = data.get("model") {
                    info.civitai_description = model
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    info.civitai_creator = model
                        .get("creator")
                        .and_then(|c| c.get("username"))
                        .or_else(|| model.get("user").and_then(|u| u.get("username")))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }

                // Stats
                if let Some(stats) = data.get("stats") {
                    info.civitai_download_count =
                        stats.get("downloadCount").and_then(|v| v.as_u64());
                    info.civitai_thumbs_up_count =
                        stats.get("thumbsUpCount").and_then(|v| v.as_u64());
                }

                // All sample images
                if let Some(images) = data.get("images").and_then(|v| v.as_array()) {
                    info.civitai_images = images
                        .iter()
                        .filter_map(|img| {
                            img.get("url")
                                .and_then(|u| u.as_str())
                                .map(|url| LoraCivitaiImage {
                                    url: url.to_string(),
                                    width: img
                                        .get("width")
                                        .and_then(|w| w.as_u64())
                                        .map(|w| w as u32),
                                    height: img
                                        .get("height")
                                        .and_then(|h| h.as_u64())
                                        .map(|h| h as u32),
                                    nsfw: img.get("nsfwLevel").and_then(|n| n.as_u64()).map(|n| {
                                        if n <= 1 {
                                            "None".to_string()
                                        } else {
                                            format!("Level{}", n)
                                        }
                                    }),
                                })
                        })
                        .collect();

                    // Use first CivitAI image as thumbnail only if no local sidecar
                    if info.thumbnail_url.is_none() {
                        info.thumbnail_url = info.civitai_images.first().map(|i| i.url.clone());
                    }
                }
            }
        }
    }

    Ok(info)
}

#[derive(Serialize)]
pub struct ReleaseNote {
    pub version: String,
    pub body: String,
    pub published_at: String,
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn fetch_release_notes(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ReleaseNote>, AppError> {
    let resp = state
        .http_client
        .get("https://api.github.com/repos/Mooshieblob1/MooshieUI/releases")
        .query(&[("per_page", "20")])
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "MooshieUI")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "GitHub API returned {}",
            resp.status()
        )));
    }

    let releases: Vec<Value> = resp.json().await?;
    let notes: Vec<ReleaseNote> = releases
        .into_iter()
        .filter_map(|r| {
            let tag = r.get("tag_name")?.as_str()?.to_string();
            let body = r
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string();
            let published = r
                .get("published_at")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            Some(ReleaseNote {
                version: tag,
                body,
                published_at: published,
            })
        })
        .collect();

    Ok(notes)
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub imported: u32,
    pub skipped: u32,
    pub failed: u32,
}

/// Import images from an external directory into the gallery.
/// Copies each image file (PNG/JPG/WebP) into the gallery directory,
/// preserving file modification time in the gallery filename for sorting.
/// Skips files that already exist in the gallery (by original filename).
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn import_image_directory(
    directory: String,
    app: AppHandle,
) -> Result<ImportResult, AppError> {
    let src_dir = std::path::Path::new(&directory);
    if !src_dir.is_dir() {
        return Err(AppError::Other(format!("Not a directory: {}", directory)));
    }

    let gallery_dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    std::fs::create_dir_all(&gallery_dir)?;

    // Collect existing gallery filenames to avoid duplicates
    let existing: std::collections::HashSet<String> = if gallery_dir.exists() {
        std::fs::read_dir(&gallery_dir)?
            .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    // Walk the directory recursively
    let entries = collect_image_files(src_dir)?;

    let total = entries.len() as u32;
    for (i, path) in entries.iter().enumerate() {
        let original_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => {
                failed += 1;
                continue;
            }
        };

        // Gallery filename: imported__{original_name}
        let gallery_name = format!("imported__imported__{}", original_name);

        if existing.contains(&gallery_name) {
            skipped += 1;
            continue;
        }

        // Check if there's already a file with the same original name (any prefix)
        let already_imported = existing
            .iter()
            .any(|e| e.ends_with(&format!("__{}", original_name)));
        if already_imported {
            skipped += 1;
            continue;
        }

        match std::fs::copy(path, gallery_dir.join(&gallery_name)) {
            Ok(_) => imported += 1,
            Err(e) => {
                log::warn!("Failed to import {}: {}", path.display(), e);
                failed += 1;
            }
        }

        // Emit progress every 50 files or on last file
        if imported.is_multiple_of(50) || i as u32 + 1 == total {
            let _ = app.emit(
                "import_progress",
                serde_json::json!({
                    "current": i + 1,
                    "total": total,
                    "imported": imported,
                }),
            );
        }
    }

    Ok(ImportResult {
        imported,
        skipped,
        failed,
    })
}

/// Recursively collect all image files (PNG, JPG, WebP) from a directory.
fn collect_image_files(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, AppError> {
    let mut files = Vec::new();
    collect_image_files_recursive(dir, &mut files)?;
    // Sort by modification time (newest first) for consistent import order
    files.sort_by(|a, b| {
        let ma = a.metadata().and_then(|m| m.modified()).ok();
        let mb = b.metadata().and_then(|m| m.modified()).ok();
        mb.cmp(&ma)
    });
    Ok(files)
}

fn collect_image_files_recursive(
    dir: &std::path::Path,
    files: &mut Vec<std::path::PathBuf>,
) -> Result<(), AppError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_image_files_recursive(&path, files)?;
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext.to_ascii_lowercase().as_str() {
                "png" | "jpg" | "jpeg" | "webp" => files.push(path),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Export application logs and system information for troubleshooting. Collects:
/// - ComfyUI subprocess stderr log
/// - App config (sanitized)
/// - Basic system/platform info
/// - Rust-side log path references
///
/// With `destination`, writes the report to that path and returns an empty
/// object. Without it, returns `{ content }` so callers that only want the text
/// (diagnostics copy, error reports) get the Rust-side logs too. The browser
/// branch in `webserver.rs` has the same contract.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn export_logs(
    state: State<'_, Arc<AppState>>,
    destination: Option<String>,
    frontend_logs: Option<Vec<String>>,
) -> Result<Value, AppError> {
    let output = build_diagnostic_log(&state, frontend_logs).await;
    match destination {
        Some(path) => {
            std::fs::write(&path, &output)?;
            Ok(json!({}))
        }
        None => Ok(json!({ "content": output })),
    }
}

/// Shallow-recursive walk collecting managed model files (`.safetensors`,
/// `.gguf`, ...) under `base`, recording each file's path relative to `base` and
/// its size in bytes. Deduped by absolute path so a file surfaced through more
/// than one category subdir is counted once. Depth-capped to avoid pathological
/// trees or symlink loops (Docker symlinks `models` to `/data/models`).
#[cfg(any(feature = "desktop", feature = "server"))]
fn collect_managed_models(
    base: &std::path::Path,
    dir: &std::path::Path,
    depth: usize,
    out: &mut Vec<(String, u64)>,
    seen: &mut BTreeSet<String>,
) {
    if depth > 6 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_managed_models(base, &path, depth + 1, out, seen);
        } else if is_managed_model_file(&path) {
            let key = path.to_string_lossy().to_lowercase();
            if !seen.insert(key) {
                continue;
            }
            let rel = path.strip_prefix(base).unwrap_or(&path);
            let name = rel.to_string_lossy().replace('\\', "/");
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((name, size));
        }
    }
}

/// Append an inventory of installed models per category to the diagnostic log,
/// so a bug report shows which checkpoints/LoRAs/etc. the user was running.
/// Records file names and sizes only (never contents) and caps the list per
/// category to keep the log bounded for large libraries.
#[cfg(any(feature = "desktop", feature = "server"))]
fn append_models_section(output: &mut String, comfyui_path: &str, extra_model_paths: Option<&str>) {
    use std::fmt::Write;

    const CATEGORIES: &[&str] = &[
        "checkpoints",
        "loras",
        "vae",
        "upscale_models",
        "embeddings",
        "controlnet",
        "clip",
        "unet",
        "diffusion_models",
        "text_encoders",
        "ultralytics",
        "model_patches",
    ];
    const MAX_FILES_PER_CATEGORY: usize = 200;

    let _ = writeln!(output, "=== Installed Models ===");
    let mut any = false;

    for category in CATEGORIES {
        let dirs = match model_install_dirs_for_config(comfyui_path, extra_model_paths, category) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let mut files: Vec<(String, u64)> = Vec::new();
        let mut seen = BTreeSet::new();
        for dir in &dirs {
            let base = std::path::Path::new(&dir.path);
            collect_managed_models(base, base, 0, &mut files, &mut seen);
        }

        if files.is_empty() {
            continue;
        }
        any = true;

        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        let total = files.len();
        let total_mb: u64 = files.iter().map(|(_, s)| *s).sum::<u64>() / 1024 / 1024;
        let _ = writeln!(
            output,
            "{} ({} file{}, {} MB):",
            category,
            total,
            if total == 1 { "" } else { "s" },
            total_mb
        );
        for (name, size) in files.iter().take(MAX_FILES_PER_CATEGORY) {
            let _ = writeln!(output, "  {} ({} MB)", name, size / 1024 / 1024);
        }
        if total > MAX_FILES_PER_CATEGORY {
            let _ = writeln!(output, "  ... and {} more", total - MAX_FILES_PER_CATEGORY);
        }
    }

    if !any {
        let _ = writeln!(output, "(no models found)");
    }
    let _ = writeln!(output);
}

/// Append mounted-disk capacity (total/available) to the diagnostic log so a
/// "failed to save / out of space" report shows the actual free space on the
/// volumes holding ComfyUI, models, and the gallery. Non-invasive: mount points
/// and free space only, no file listings.
#[cfg(any(feature = "desktop", feature = "server"))]
fn append_disks_section(output: &mut String) {
    use std::fmt::Write;
    use sysinfo::Disks;

    let _ = writeln!(output, "=== Disks ===");
    let disks = Disks::new_with_refreshed_list();
    if disks.is_empty() {
        let _ = writeln!(output, "(no disks reported)");
    } else {
        let gb = |bytes: u64| bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        for disk in &disks {
            let _ = writeln!(
                output,
                "{} [{}]: {:.1} GB free of {:.1} GB",
                disk.mount_point().display(),
                disk.file_system().to_string_lossy(),
                gb(disk.available_space()),
                gb(disk.total_space()),
            );
        }
    }
    let _ = writeln!(output);
}

/// Resolve a git checkout's short commit hash by reading `.git` directly (no
/// subprocess), so custom-node versions can be reported without spawning one
/// `git` per node. Returns `None` for non-git folders or unreadable refs.
#[cfg(any(feature = "desktop", feature = "server"))]
fn read_git_short_rev(repo: &std::path::Path) -> Option<String> {
    let git_dir = repo.join(".git");
    if !git_dir.is_dir() {
        return None;
    }
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let short = |sha: &str| sha.trim().chars().take(9).collect::<String>();
    match head.strip_prefix("ref: ") {
        Some(ref_path) => {
            if let Ok(sha) = std::fs::read_to_string(git_dir.join(ref_path)) {
                return Some(short(&sha));
            }
            // Packed-refs fallback for repos that have gc'd loose refs.
            let packed = std::fs::read_to_string(git_dir.join("packed-refs")).ok()?;
            packed.lines().find_map(|line| {
                line.split_once(' ')
                    .filter(|(_, name)| name.trim() == ref_path)
                    .map(|(sha, _)| short(sha))
            })
        }
        // Detached HEAD stores the sha directly.
        None => Some(short(head)),
    }
}

/// Append the installed ComfyUI custom nodes (the single biggest source of
/// ComfyUI breakage) — folder name, git short-rev when available, and disabled
/// state. Names only, no contents; capped for large installs.
#[cfg(any(feature = "desktop", feature = "server"))]
fn append_custom_nodes_section(output: &mut String, comfyui_path: &str) {
    use std::fmt::Write;

    let _ = writeln!(output, "=== Custom Nodes ===");
    if comfyui_path.is_empty() {
        let _ = writeln!(output, "(ComfyUI path not configured)");
        let _ = writeln!(output);
        return;
    }

    let dir = std::path::Path::new(comfyui_path).join("custom_nodes");
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            let _ = writeln!(output, "(could not read {}: {})", dir.display(), e);
            let _ = writeln!(output);
            return;
        }
    };

    let mut nodes: Vec<(String, Option<String>)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Skip Python bytecode cache; keep .disabled so we can flag it.
        if name == "__pycache__" {
            continue;
        }
        nodes.push((name, read_git_short_rev(&path)));
    }

    if nodes.is_empty() {
        let _ = writeln!(output, "(none installed)");
    } else {
        nodes.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        let total = nodes.len();
        let _ = writeln!(output, "{} installed:", total);
        const MAX_NODES: usize = 300;
        for (name, rev) in nodes.iter().take(MAX_NODES) {
            let disabled = name.ends_with(".disabled");
            let _ = writeln!(
                output,
                "  {}{}{}",
                name,
                rev.as_deref()
                    .map(|r| format!(" @ {}", r))
                    .unwrap_or_default(),
                if disabled { " (disabled)" } else { "" }
            );
        }
        if total > MAX_NODES {
            let _ = writeln!(output, "  ... and {} more", total - MAX_NODES);
        }
    }
    let _ = writeln!(output);
}

/// Append an allowlisted set of GPU/ML-relevant environment variables. Only
/// variables known to affect ComfyUI/PyTorch behaviour are read, so no secrets
/// or unrelated environment leak into the log.
#[cfg(any(feature = "desktop", feature = "server"))]
fn append_env_section(output: &mut String) {
    use std::fmt::Write;

    // Allowlist: device selection, allocator tuning, ROCm overrides, HF cache,
    // proxies (host only), and ComfyUI arg passthrough. No API keys or tokens.
    const VARS: &[&str] = &[
        "CUDA_VISIBLE_DEVICES",
        "HIP_VISIBLE_DEVICES",
        "ROCR_VISIBLE_DEVICES",
        "HSA_OVERRIDE_GFX_VERSION",
        "PYTORCH_CUDA_ALLOC_CONF",
        "PYTORCH_HIP_ALLOC_CONF",
        "PYTORCH_ENABLE_MPS_FALLBACK",
        "HF_HOME",
        "HF_HUB_OFFLINE",
        "HF_HUB_ENABLE_HF_TRANSFER",
        "COMMANDLINE_ARGS",
        "MOOSHIEUI_LLAMA_BIN_DIR",
        "HTTP_PROXY",
        "HTTPS_PROXY",
    ];

    let mut found = false;
    let mut section = String::new();
    for var in VARS {
        if let Ok(val) = std::env::var(var) {
            if val.is_empty() {
                continue;
            }
            found = true;
            // Proxy vars can embed credentials (user:pass@host); redact them.
            let shown = if var.ends_with("PROXY") {
                redact_proxy_credentials(&val)
            } else {
                val
            };
            let _ = writeln!(section, "{}={}", var, shown);
        }
    }

    let _ = writeln!(output, "=== Environment (relevant) ===");
    if found {
        let _ = write!(output, "{}", section);
    } else {
        let _ = writeln!(output, "(none of the tracked variables are set)");
    }
    let _ = writeln!(output);
}

/// Redact any `user:pass@` credentials from a proxy URL so the log never carries
/// proxy secrets while still showing the host being used.
#[cfg(any(feature = "desktop", feature = "server"))]
fn redact_proxy_credentials(url: &str) -> String {
    match (url.find("://"), url.find('@')) {
        (Some(scheme_end), Some(at)) if at > scheme_end + 3 => {
            format!("{}://***@{}", &url[..scheme_end], &url[at + 1..])
        }
        _ => url.to_string(),
    }
}

/// Build the full diagnostic log text (versions, system, disks, config, models,
/// GPU, custom nodes, Python/ComfyUI, prompt-assistant, runtime, and the
/// ComfyUI/llama-server/Rust/frontend logs). Shared by the desktop `export_logs`
/// command (which writes it to a file) and the browser/server-mode handler
/// (which returns it for a client-side download).
#[cfg(any(feature = "desktop", feature = "server"))]
pub async fn build_diagnostic_log(state: &AppState, frontend_logs: Option<Vec<String>>) -> String {
    use std::fmt::Write;

    // Fold any newly-submitted frontend logs into the ring buffer so repeat
    // exports include previous captures too.
    if let Some(lines) = frontend_logs {
        crate::log_buffer::push_frontend_lines(lines);
    }

    let mut output = String::with_capacity(16 * 1024);

    // Header
    let _ = writeln!(output, "=== MooshieUI Diagnostic Log ===");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = writeln!(output, "Exported: {} (unix timestamp)", now);
    let _ = writeln!(output, "MooshieUI: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        output,
        "Build: {} / {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        if cfg!(feature = "server") {
            "server"
        } else {
            "desktop"
        }
    );
    let _ = writeln!(output);

    // System specs (OS, CPU, memory) — collected via `sysinfo` so bug reports
    // carry the hardware/OS context needed to reproduce driver- or VRAM-related
    // issues without a back-and-forth.
    {
        use sysinfo::System;

        let _ = writeln!(output, "=== System ===");
        let _ = writeln!(
            output,
            "OS: {} {} (kernel {})",
            System::name().unwrap_or_else(|| std::env::consts::OS.to_string()),
            System::os_version().unwrap_or_else(|| "unknown".into()),
            System::kernel_version().unwrap_or_else(|| "unknown".into()),
        );
        if let Some(long) = System::long_os_version() {
            let _ = writeln!(output, "OS (full): {}", long);
        }
        let _ = writeln!(output, "Arch: {}", std::env::consts::ARCH);
        if let Some(host) = System::host_name() {
            let _ = writeln!(output, "Host: {}", host);
        }

        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();

        if let Some(cpu) = sys.cpus().first() {
            let brand = cpu.brand().trim();
            let _ = writeln!(
                output,
                "CPU: {} ({} logical cores{})",
                if brand.is_empty() { "unknown" } else { brand },
                sys.cpus().len(),
                match System::physical_core_count() {
                    Some(n) => format!(", {} physical", n),
                    None => String::new(),
                }
            );
        }

        let mb = |bytes: u64| bytes / 1024 / 1024;
        let _ = writeln!(
            output,
            "Memory: {} MB total, {} MB available",
            mb(sys.total_memory()),
            mb(sys.available_memory())
        );
        if sys.total_swap() > 0 {
            let _ = writeln!(output, "Swap: {} MB total", mb(sys.total_swap()));
        }
        let _ = writeln!(output);
    }

    // Disk capacity (out-of-space is a common save/generation failure)
    append_disks_section(&mut output);

    // Presence flag for secret-bearing fields — never log the value itself.
    let cfgd = |s: &str| if s.is_empty() { "no" } else { "yes" };

    // App config (sanitized — no secrets, just relevant settings)
    {
        let config = state.config.read().await;
        let _ = writeln!(output, "=== App Configuration ===");
        let _ = writeln!(
            output,
            "UI mode: {}",
            if config.browser_mode {
                "Browser"
            } else {
                "App"
            }
        );
        let _ = writeln!(output, "Server mode: {:?}", config.server_mode);
        let _ = writeln!(output, "Server URL: {}", config.server_url);
        let _ = writeln!(output, "Server port: {}", config.server_port);
        let _ = writeln!(output, "VRAM mode: {}", config.vram_mode);
        let _ = writeln!(output, "Attention backend: {}", config.attention_backend);
        let _ = writeln!(output, "Keep alive: {}", config.keep_alive);
        let _ = writeln!(output, "Auto start: {}", config.auto_start);
        let _ = writeln!(output, "Extra args: {:?}", config.extra_args);
        let _ = writeln!(output, "ComfyUI path: {}", config.comfyui_path);
        let cv = crate::comfyui_version::comfyui_version_info(std::path::Path::new(
            &config.comfyui_path,
        ));
        let _ = writeln!(
            output,
            "ComfyUI version: {} (target {}{})",
            cv.installed.as_deref().unwrap_or("unknown"),
            cv.target,
            if cv.update_available {
                ", update available"
            } else {
                ""
            }
        );
        let _ = writeln!(output, "Venv path: {}", config.venv_path);
        let _ = writeln!(
            output,
            "Extra model paths: {}",
            config.extra_model_paths.as_deref().unwrap_or("(none)")
        );
        let _ = writeln!(
            output,
            "Gallery path: {}",
            config.gallery_path.as_deref().unwrap_or("(default)")
        );
        let _ = writeln!(
            output,
            "App data dir: {}",
            crate::config::app_data_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(unknown)".into())
        );

        // Multi-GPU workers
        if config.gpu_workers.is_empty() {
            let _ = writeln!(output, "GPU workers: single-worker (default)");
        } else {
            let _ = writeln!(output, "GPU workers: {}", config.gpu_workers.len());
            for w in &config.gpu_workers {
                let _ = writeln!(
                    output,
                    "  GPU {} -> port {} (vram {})",
                    w.gpu_index,
                    w.port
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "auto".into()),
                    w.vram_mode.as_deref().unwrap_or("default"),
                );
            }
        }

        // Default generation parameters
        let _ = writeln!(
            output,
            "Defaults: checkpoint={}, sampler={}, scheduler={}, steps={}, cfg={}, {}x{}",
            config.default_checkpoint.as_deref().unwrap_or("(none)"),
            config.default_sampler,
            config.default_scheduler,
            config.default_steps,
            config.default_cfg,
            config.default_width,
            config.default_height,
        );
        let _ = writeln!(
            output,
            "Interrogator thresholds: general={}, character={}",
            config.interrogator_general_threshold, config.interrogator_character_threshold
        );

        // UI
        let _ = writeln!(
            output,
            "UI: theme={}, palette={}, font_scale={}, custom_profile={}",
            config.theme,
            config.theme_palette,
            config.font_scale,
            config.theme_profile_id.as_deref().unwrap_or("(none)")
        );

        // Networking / browser mode
        let _ = writeln!(
            output,
            "Browser mode: enabled={}, ui_port={}, lan={}, tls_cert={}, tls_key={}",
            config.browser_mode,
            config.ui_server_port,
            config.lan_enabled,
            cfgd(config.tls_cert_path.as_deref().unwrap_or("")),
            cfgd(config.tls_key_path.as_deref().unwrap_or("")),
        );
        let _ = writeln!(
            output,
            "Network proxy: {}, pip index: {}",
            cfgd(config.network_proxy.as_deref().unwrap_or("")),
            cfgd(config.pip_index_url.as_deref().unwrap_or("")),
        );

        // Integrations (presence only for anything secret-bearing)
        let _ = writeln!(
            output,
            "CivitAI API key: {}",
            cfgd(config.civitai_api_key.as_deref().unwrap_or(""))
        );
        let _ = writeln!(
            output,
            "Webhook: url={}, events={:?}, include_sensitive={}, allow_private={}",
            cfgd(config.webhook_url.as_deref().unwrap_or("")),
            config.webhook_events,
            config.webhook_include_sensitive,
            config.webhook_allow_private_targets,
        );
        let _ = writeln!(
            output,
            "Output filename template: {}",
            config
                .output_filename_template
                .as_deref()
                .unwrap_or("(default)")
        );

        let _ = writeln!(output, "Setup complete: {}", config.setup_complete);

        // Prompt assistant (LLM) — local llama-server vs external endpoint.
        if config.llm_external_enabled {
            let _ = writeln!(
                output,
                "Prompt assistant: external endpoint, base_url={}, model={}, api_key={}",
                if config.llm_external_base_url.is_empty() {
                    "(unset)"
                } else {
                    &config.llm_external_base_url
                },
                if config.llm_external_model.is_empty() {
                    "(unset)"
                } else {
                    &config.llm_external_model
                },
                cfgd(&config.llm_external_api_key),
            );
        } else {
            let _ =
                writeln!(
                output,
                "Prompt assistant: local llama-server, model={}, idle_timeout={}s, setup_done={}",
                config.prompt_assistant_model_id.as_deref().unwrap_or("(none)"),
                config.prompt_assistant_idle_timeout_secs,
                config.prompt_assistant_setup_done,
            );
        }
        let _ = writeln!(output);

        // Installed models inventory (names + sizes) — held under the same lock
        // so it reflects the exact ComfyUI + extra-model paths reported above.
        append_models_section(
            &mut output,
            &config.comfyui_path,
            config.extra_model_paths.as_deref(),
        );

        // Custom nodes (top cause of ComfyUI breakage) — names + git rev.
        append_custom_nodes_section(&mut output, &config.comfyui_path);
    }

    // Relevant environment variables (GPU/ML tuning; allowlisted, no secrets)
    append_env_section(&mut output);

    // Runtime status — is the managed ComfyUI child alive, is the web server up,
    // is the prompt-assistant llama-server loaded right now?
    {
        let _ = writeln!(output, "=== Runtime Status ===");
        let comfyui_pid = {
            let guard = state.comfyui_process.lock().await;
            guard.as_ref().and_then(|c| c.id())
        };
        match comfyui_pid {
            Some(pid) => {
                let _ = writeln!(output, "Managed ComfyUI process: running (pid {})", pid);
            }
            None => {
                let _ = writeln!(
                    output,
                    "Managed ComfyUI process: not running (remote or stopped)"
                );
            }
        }
        let _ = writeln!(
            output,
            "Web server running: {}",
            state
                .web_server_running
                .load(std::sync::atomic::Ordering::Relaxed)
        );
        let _ = writeln!(
            output,
            "Prompt assistant loaded: {} (active model: {})",
            state.prompt_assistant.server.is_running(),
            state
                .prompt_assistant
                .server
                .active_model()
                .unwrap_or_else(|| "(none)".into())
        );
        let _ = writeln!(output);
    }

    // GPU info (NVIDIA) — static specs plus current utilization/VRAM/temperature
    // so out-of-memory and thermal-throttle reports carry live state.
    let _ = writeln!(output, "=== GPU Info ===");
    let nvidia_smi_out = {
        let mut cmd = std::process::Command::new("nvidia-smi");
        cmd.args([
            "--query-gpu=name,driver_version,memory.total,memory.used,memory.free,utilization.gpu,temperature.gpu,compute_cap",
            "--format=csv,noheader",
        ]);
        // Force English / POSIX locale so diagnostics read the same regardless
        // of the user's system language.
        cmd.env("LC_ALL", "C").env("LANG", "C");
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd.output()
    };
    match nvidia_smi_out {
        Ok(o) if o.status.success() => {
            let _ = write!(output, "{}", String::from_utf8_lossy(&o.stdout));
        }
        _ => {
            let _ = writeln!(output, "(nvidia-smi not available or no NVIDIA GPU)");
        }
    }
    let _ = writeln!(output);

    // Python / ComfyUI version info
    {
        let config = state.config.read().await;
        if !config.venv_path.is_empty() {
            let _ = writeln!(output, "=== Python Environment ===");
            let python_path = {
                let venv = std::path::Path::new(&config.venv_path);
                if cfg!(target_os = "windows") {
                    venv.join("Scripts").join("python.exe")
                } else {
                    venv.join("bin").join("python")
                }
            };
            if python_path.exists() {
                #[cfg(target_os = "windows")]
                let hide: u32 = 0x08000000; // CREATE_NO_WINDOW

                let mut py_ver_cmd = std::process::Command::new(&python_path);
                py_ver_cmd.args(["--version"]);
                py_ver_cmd.env("LC_ALL", "C").env("LANG", "C");
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    py_ver_cmd.creation_flags(hide);
                }
                if let Ok(o) = py_ver_cmd.output() {
                    let _ = write!(output, "Python: {}", String::from_utf8_lossy(&o.stdout));
                    if !o.stderr.is_empty() {
                        let _ = write!(output, "{}", String::from_utf8_lossy(&o.stderr));
                    }
                }
                // Get torch version
                let mut torch_cmd = std::process::Command::new(&python_path);
                torch_cmd.args(["-c", "import torch; print(f'PyTorch: {torch.__version__}'); print(f'CUDA available: {torch.cuda.is_available()}'); print(f'CUDA version: {torch.version.cuda}') if torch.cuda.is_available() else None"]);
                torch_cmd.env("LC_ALL", "C").env("LANG", "C");
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    torch_cmd.creation_flags(hide);
                }
                if let Ok(o) = torch_cmd.output() {
                    if o.status.success() {
                        let _ = write!(output, "{}", String::from_utf8_lossy(&o.stdout));
                    }
                }
            } else {
                let _ = writeln!(output, "Python not found at: {}", python_path.display());
            }
            let _ = writeln!(output);
        }
    }

    // ComfyUI stderr log
    let _ = writeln!(output, "=== ComfyUI Log ===");
    let log_path = std::env::temp_dir().join("comfyui-desktop-stderr.log");
    let _ = writeln!(output, "(Source: {})", log_path.display());
    // This file is truncated only when MooshieUI spawns ComfyUI itself. When it
    // attached to an already-running server instead, the contents can be from a
    // much earlier launch with a different ComfyUI/PyTorch version, so stamp the
    // age rather than let a stale log be read as the current session's.
    if let Ok(age) = std::fs::metadata(&log_path)
        .and_then(|m| m.modified())
        .and_then(|t| {
            std::time::SystemTime::now()
                .duration_since(t)
                .map_err(|e| std::io::Error::other(e.to_string()))
        })
    {
        let secs = age.as_secs();
        let _ = writeln!(
            output,
            "(Last written {}h {}m ago)",
            secs / 3600,
            (secs % 3600) / 60
        );
    }
    match std::fs::read_to_string(&log_path) {
        Ok(content) => {
            if content.is_empty() {
                let _ = writeln!(output, "(log file is empty)");
            } else {
                let _ = write!(output, "{}", content);
            }
        }
        Err(e) => {
            let _ = writeln!(output, "(Could not read log: {})", e);
        }
    }

    // Prompt-assistant llama-server stderr log (model-load diagnostics such as
    // missing shared libraries, unsupported architectures, health timeouts).
    let _ = writeln!(output);
    let _ = writeln!(output, "=== Prompt Assistant (llama-server) Log ===");
    match state.prompt_assistant.server.read_server_log() {
        Some(content) if !content.trim().is_empty() => {
            let _ = write!(output, "{}", content);
        }
        Some(_) => {
            let _ = writeln!(output, "(log file is empty)");
        }
        None => {
            let _ = writeln!(output, "(no llama-server log yet)");
        }
    }

    // Rust-side log ring buffer (captured by log_buffer::RingLogger)
    let rust_lines = crate::log_buffer::snapshot_rust();
    if !rust_lines.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "=== Rust Log (last {} lines) ===", rust_lines.len());
        for line in &rust_lines {
            let _ = writeln!(output, "{}", line);
        }
    }

    // Frontend console ring buffer (captured by src/lib/utils/log-buffer.ts)
    let frontend_lines = crate::log_buffer::snapshot_frontend();
    if !frontend_lines.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(
            output,
            "=== Frontend Console (last {} lines) ===",
            frontend_lines.len()
        );
        for line in &frontend_lines {
            let _ = writeln!(output, "{}", line);
        }
    }

    output
}

/// Append a batch of frontend console log lines to the in-memory diagnostics
/// ring buffer. Called opportunistically by the UI so that exported logs
/// include frontend state even when a crash prevents exporting normally.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn append_frontend_logs(lines: Vec<String>) -> Result<(), AppError> {
    crate::log_buffer::push_frontend_lines(lines);
    Ok(())
}

/// Detect the MIME type of image bytes from magic bytes.
pub(crate) fn detect_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF") {
        "image/gif"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

/// Fetch a remote image URL through the Rust backend (with CivitAI auth headers if
/// configured), caching the raw bytes to `{app_data_dir}/image_cache/{url_sha256}`.
///
/// Returns the image as a `"data:<mime>;base64,..."` string so the WebView can
/// display it without making its own unauthenticated request to CivitAI.
/// Cache TTL is 7 days; stale or missing entries are refreshed transparently.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn fetch_cached_image(
    state: State<'_, Arc<AppState>>,
    url: String,
) -> Result<String, AppError> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    // This backend fetch carries the user's CivitAI token, so keep it scoped to
    // CivitAI image hosts and validate redirects before touching the cache.
    parse_civitai_image_url(&url)?;

    // Build a stable cache filename from the URL hash.
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, url.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    let cache_dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot determine app data directory".into()))?
        .join("image_cache");
    std::fs::create_dir_all(&cache_dir)?;

    let cache_path = cache_dir.join(&hash);
    const CACHE_TTL_SECS: u64 = 7 * 24 * 60 * 60; // 7 days

    // Return cached bytes if they exist and are fresh.
    if let Ok(meta) = std::fs::metadata(&cache_path) {
        if meta
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|e| e.as_secs() < CACHE_TTL_SECS)
            .unwrap_or(false)
        {
            if let Ok(bytes) = std::fs::read(&cache_path) {
                if !bytes.is_empty() {
                    let mime = detect_image_mime(&bytes);
                    return Ok(format!("data:{};base64,{}", mime, STANDARD.encode(&bytes)));
                }
            }
        }
    }

    // Cache miss — fetch through the backend so auth headers are applied.
    let bytes = fetch_civitai_image_bytes(state.inner().as_ref(), &url).await?;

    // Persist to disk cache (best-effort; ignore write errors).
    let _ = std::fs::write(&cache_path, &bytes);

    let mime = detect_image_mime(&bytes);
    Ok(format!("data:{};base64,{}", mime, STANDARD.encode(&bytes)))
}

/// Read an image from the native clipboard and return PNG bytes.
/// Bypasses WebView clipboard restrictions that prevent `navigator.clipboard.read()` from working.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn read_clipboard_image(app: AppHandle) -> Result<Vec<u8>, AppError> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let clipboard_image = app
        .clipboard()
        .read_image()
        .map_err(|e| AppError::Other(format!("No image in clipboard: {}", e)))?;

    let rgba = clipboard_image.rgba().to_vec();
    let w = clipboard_image.width();
    let h = clipboard_image.height();

    let rgba_img = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| AppError::Other("Invalid clipboard image data".into()))?;

    let dynamic = image::DynamicImage::from(rgba_img);
    let mut png_bytes: Vec<u8> = Vec::new();
    dynamic
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| AppError::Other(format!("Failed to encode clipboard image: {}", e)))?;

    Ok(png_bytes)
}

// ---------------------------------------------------------------------------
// GPU stats — live nvidia-smi data + worker status
// ---------------------------------------------------------------------------

/// Per-GPU stats returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct GpuStats {
    /// GPU index (matches CUDA device order)
    pub index: u32,
    /// GPU name (e.g. "NVIDIA RTX 3090 Ti")
    pub name: String,
    /// VRAM used in MiB
    pub vram_used_mb: u64,
    /// VRAM total in MiB
    pub vram_total_mb: u64,
    /// GPU utilization percentage (0–100)
    pub gpu_util: u32,
    /// GPU temperature in Celsius
    pub temperature: u32,
    /// Power draw in watts
    pub power_draw_w: u32,
    /// Worker status (if a MooshieUI worker is assigned to this GPU)
    pub worker: Option<GpuWorkerInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuWorkerInfo {
    pub worker_id: u32,
    pub port: u16,
    pub status: String,
    pub reserved: bool,
    pub label: String,
}

/// Query live GPU stats from nvidia-smi.
fn query_nvidia_smi_stats() -> Result<Vec<GpuStats>, AppError> {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=index,name,memory.used,memory.total,utilization.gpu,temperature.gpu,power.draw",
        "--format=csv,noheader,nounits",
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd
        .output()
        .map_err(|e| AppError::Other(format!("nvidia-smi not found: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Other(format!("nvidia-smi failed: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut gpus = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 7 {
            continue;
        }
        let index = parts[0].parse::<u32>().unwrap_or(0);
        let name = parts[1].to_string();
        let vram_used = parts[2].parse::<f64>().unwrap_or(0.0) as u64;
        let vram_total = parts[3].parse::<f64>().unwrap_or(0.0) as u64;
        let gpu_util = parts[4].parse::<u32>().unwrap_or(0);
        let temperature = parts[5].parse::<u32>().unwrap_or(0);
        let power_draw = parts[6].parse::<f64>().unwrap_or(0.0) as u32;

        gpus.push(GpuStats {
            index,
            name,
            vram_used_mb: vram_used,
            vram_total_mb: vram_total,
            gpu_util,
            temperature,
            power_draw_w: power_draw,
            worker: None,
        });
    }

    Ok(gpus)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_gpu_stats(state: State<'_, Arc<AppState>>) -> Result<Vec<GpuStats>, AppError> {
    get_gpu_stats_inner(&state).await
}

/// Shared implementation used by both Tauri command and REST handler.
pub async fn get_gpu_stats_inner(state: &AppState) -> Result<Vec<GpuStats>, AppError> {
    let mut gpus = query_nvidia_smi_stats()?;

    // Merge worker status info
    let statuses = state.gpu_manager.worker_statuses().await;
    for ws in &statuses {
        if let Some(gpu) = gpus.iter_mut().find(|g| g.index == ws.gpu_index) {
            gpu.worker = Some(GpuWorkerInfo {
                worker_id: ws.id,
                port: ws.port,
                status: format!("{:?}", ws.status).to_lowercase(),
                reserved: ws.reserved,
                label: ws.label.clone(),
            });
        }
    }

    Ok(gpus)
}

// ─── Attention Backend Commands ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AttentionBackendStatus {
    pub current: String,
    pub venv_packages: Vec<String>,
    pub compute_capability: Option<f32>,
    /// OS family ("windows" | "linux" | "macos").
    pub os: String,
    /// Whether the CUDA toolkit (`nvcc`) is on PATH — required for backends
    /// that compile from source on this platform (flash_v1 / flash_v2 on
    /// Windows, sage_v2 on Linux).
    pub nvcc_available: bool,
    /// Per-backend support, so the UI can disable options the machine can't use.
    pub support: Vec<BackendSupport>,
}

/// Whether a given attention backend can be installed on this machine, and why not.
#[derive(Debug, Serialize)]
pub struct BackendSupport {
    pub backend: String,
    pub supported: bool,
    /// Machine-readable reason code when unsupported:
    /// `"no_nvidia_gpu" | "compute_capability" | "nvcc_missing"`.
    pub reason: Option<String>,
    pub min_cc: Option<f32>,
}

/// Resolve the venv's Python interpreter path (platform-specific layout).
fn venv_python_bin(venv_path: &str) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::path::Path::new(venv_path)
            .join("Scripts")
            .join("python.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::path::Path::new(venv_path).join("bin").join("python")
    }
}

/// Probe whether the CUDA compiler (`nvcc`) is available on PATH.
fn nvcc_available() -> bool {
    let mut cmd = std::process::Command::new("nvcc");
    cmd.arg("--version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Compute per-backend support from detected capabilities. Hard-block rules:
/// no NVIDIA GPU → all non-default blocked; compute cap below the backend's
/// minimum → blocked; backends that compile from source on this platform need
/// `nvcc` → blocked when missing. Everything else is attempt-install-and-verify.
fn compute_backend_support(
    compute_capability: Option<f32>,
    nvcc: bool,
    is_windows: bool,
) -> Vec<BackendSupport> {
    // (backend, min compute capability, needs nvcc on Windows, needs nvcc elsewhere)
    const MATRIX: [(&str, f32, bool, bool); 4] = [
        ("sage_v1", 8.0, false, false), // wheel; triton at runtime (triton-windows on Windows)
        ("sage_v2", 8.0, false, true),  // prebuilt wheel on Windows; CUDA source build on Linux
        ("flash_v1", 7.5, true, false), // source build on Windows
        ("flash_v2", 8.0, true, false), // source build on Windows; Linux sdist may fetch a wheel
    ];
    MATRIX
        .iter()
        .map(|(backend, min_cc, nvcc_windows, nvcc_other)| {
            let needs_nvcc = if is_windows {
                *nvcc_windows
            } else {
                *nvcc_other
            };
            let (supported, reason) = match compute_capability {
                None => (false, Some("no_nvidia_gpu".to_string())),
                Some(cc) if cc < *min_cc => (false, Some("compute_capability".to_string())),
                Some(_) if needs_nvcc && !nvcc => (false, Some("nvcc_missing".to_string())),
                Some(_) => (true, None),
            };
            BackendSupport {
                backend: backend.to_string(),
                supported,
                reason,
                min_cc: Some(*min_cc),
            }
        })
        .collect()
}

/// Keep only the last ~20 lines of a stderr blob, so surfaced install errors are
/// informative without dumping an entire build log into the UI.
fn stderr_tail(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let tail = if lines.len() > 20 {
        &lines[lines.len() - 20..]
    } else {
        &lines[..]
    };
    tail.join("\n").trim().to_string()
}

/// Check which attention backend packages are installed in the venv and detect GPU compute capability.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn check_attention_backend(
    state: State<'_, Arc<AppState>>,
) -> Result<AttentionBackendStatus, AppError> {
    check_attention_backend_core(&state).await
}

/// Lightweight probe for just the GPU compute capability (single `nvidia-smi` shell-out).
/// The gen page calls this on mount and after every generation, so it must stay cheap —
/// unlike `check_attention_backend`, it does not shell out to `uv`/`nvcc`.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_compute_capability() -> Result<Option<f32>, AppError> {
    Ok(detect_compute_capability())
}

/// Core of `check_attention_backend`, shared by the desktop Tauri command and the
/// browser-mode dispatch arm. Lists the attention packages present in the venv and
/// computes per-backend support from GPU compute capability, OS, and `nvcc`.
pub async fn check_attention_backend_core(
    state: &Arc<AppState>,
) -> Result<AttentionBackendStatus, AppError> {
    let (venv_path, current) = {
        let config = state.config.read().await;
        (config.venv_path.clone(), config.attention_backend.clone())
    };

    let uv = resolve_uv_bin(&venv_path);
    let venv_python = venv_python_bin(&venv_path);

    // List installed attention packages in the venv
    let mut venv_packages = Vec::new();
    if uv.exists() {
        let mut cmd = tokio_command_no_window(&uv);
        cmd.args(["pip", "list", "--python", &venv_python.to_string_lossy()]);
        let output = cmd.output().await.ok();

        if let Some(o) = output {
            if o.status.success() {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let known = ["sageattention", "flash-attn", "triton", "triton-windows"];
                for line in stdout.lines() {
                    let mut cols = line.split_whitespace();
                    let pkg = cols.next().unwrap_or("").to_lowercase();
                    if known.iter().any(|k| pkg == *k) {
                        // Keep `uv pip list`'s version column. SageAttention v1 and v2
                        // both install under the package name `sageattention`, so the
                        // bare name cannot tell a user which one is active — and the
                        // v2 wheel's local version (e.g. "2.2.0+cu128torch2.9.1.post6")
                        // also shows whether it matches the venv's torch build.
                        match cols.next() {
                            Some(version) => venv_packages.push(format!("{} {}", pkg, version)),
                            None => venv_packages.push(pkg),
                        }
                    }
                }
            }
        }
    }

    let compute_capability = detect_compute_capability();
    let nvcc = nvcc_available();
    let support = compute_backend_support(compute_capability, nvcc, cfg!(target_os = "windows"));

    Ok(AttentionBackendStatus {
        current,
        venv_packages,
        compute_capability,
        os: std::env::consts::OS.to_string(),
        nvcc_available: nvcc,
        support,
    })
}

/// Detect the highest NVIDIA GPU compute capability (e.g. 8.6 for RTX 3080).
fn detect_compute_capability() -> Option<f32> {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args(["--query-gpu=compute_cap", "--format=csv,noheader,nounits"]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| line.trim().parse::<f32>().ok())
        .reduce(f32::max)
}

/// Public accessor for browser-mode command dispatch.
pub fn detect_compute_capability_pub() -> Option<f32> {
    detect_compute_capability()
}

/// Install (or uninstall) an attention backend package in the venv.
/// Accepts: "default", "sage_v1", "sage_v2", "flash_v1", "flash_v2".
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn install_attention_backend(
    state: State<'_, Arc<AppState>>,
    app_handle: AppHandle,
    backend: String,
) -> Result<(), AppError> {
    install_attention_backend_core(&state, backend, |msg| {
        app_handle.emit("attention:install_progress", msg).ok();
    })
    .await
}

/// Core of `install_attention_backend`, shared by the desktop Tauri command and
/// the browser-mode dispatch arm. `emit` reports progress: desktop forwards it to
/// `attention:install_progress` via the WebView, browser mode via the SSE broadcast.
pub async fn install_attention_backend_core(
    state: &Arc<AppState>,
    backend: String,
    emit: impl Fn(&str),
) -> Result<(), AppError> {
    let valid = ["default", "sage_v1", "sage_v2", "flash_v1", "flash_v2"];
    if !valid.contains(&backend.as_str()) {
        return Err(AppError::Other(format!(
            "Invalid attention backend: '{}'. Valid: {:?}",
            backend, valid
        )));
    }

    let (venv_path, network_proxy, pip_index_url) = {
        let config = state.config.read().await;
        (
            config.venv_path.clone(),
            config.network_proxy.clone(),
            config.pip_index_url.clone(),
        )
    };

    let uv = resolve_uv_bin(&venv_path);
    if !uv.exists() {
        return Err(AppError::Other("uv not found. Run setup first.".into()));
    }

    let venv_python = venv_python_bin(&venv_path);
    let python_str = venv_python.to_string_lossy().to_string();
    let is_windows = cfg!(target_os = "windows");

    // Preflight: reject hard-blocked backends before touching any packages, so a
    // doomed source build never even starts.
    if backend != "default" {
        let support =
            compute_backend_support(detect_compute_capability(), nvcc_available(), is_windows);
        if let Some(s) = support.iter().find(|s| s.backend == backend) {
            if !s.supported {
                let reason = match s.reason.as_deref() {
                    Some("no_nvidia_gpu") => "no NVIDIA GPU was detected".to_string(),
                    Some("compute_capability") => format!(
                        "the GPU's compute capability is below the required {} for this backend",
                        s.min_cc
                            .map(|c| format!("{:.1}", c))
                            .unwrap_or_else(|| "minimum".into())
                    ),
                    Some("nvcc_missing") => "the CUDA toolkit (nvcc) is required to build this backend from source but was not found on PATH".to_string(),
                    _ => "it is not supported on this system".to_string(),
                };
                return Err(AppError::Other(format!(
                    "Cannot install the {} attention backend: {}.",
                    backend, reason
                )));
            }
        }
    }

    // SageAttention 2.x is not on PyPI: on Windows resolve a prebuilt wheel
    // matched to the venv's torch/CUDA/Python build. Resolution runs before the
    // uninstall step so a failed lookup leaves the venv untouched.
    let sage2_wheel: Option<crate::attention::ResolvedWheel> = if backend == "sage_v2" && is_windows
    {
        emit("Detecting the venv's PyTorch build...");
        let build = crate::attention::probe_torch_build(&venv_python)
            .await
            .map_err(|e| AppError::Other(format!("Cannot install sage_v2: {}", e)))?;
        emit(&format!(
            "Looking up a prebuilt SageAttention v2 wheel for {}...",
            build.describe()
        ));
        let wheel = crate::attention::resolve_sage2_windows_wheel(&state.http_client, &build)
            .await
            .map_err(|e| AppError::Other(format!("Cannot install sage_v2: {}", e)))?;
        emit(&format!("Found wheel: {}", wheel.file_name));
        Some(wheel)
    } else {
        None
    };

    // Package names (no version specifier) backing each backend, for uninstall/rollback.
    // triton-windows is deliberately absent: it is also the fast-kernel path for
    // comfy-kitchen (int4/int8 convrot quantized models), so rolling back a failed
    // SageAttention install must not strip it and silently halve quant throughput.
    let base_names: Vec<&str> = match backend.as_str() {
        "sage_v1" | "sage_v2" => vec!["sageattention"],
        "flash_v1" | "flash_v2" => vec!["flash-attn"],
        _ => vec![],
    };

    // Step 1: Uninstall any existing attention packages (ignore errors).
    // triton-windows is intentionally left installed: comfy-kitchen needs it for
    // fast convrot quant kernels and falls back to slow eager PyTorch without it,
    // so switching attention backend must not regress quantized-model speed.
    emit("Removing old attention packages...");
    let uninstall_old: Vec<&str> = vec![
        "pip",
        "uninstall",
        "--python",
        &python_str,
        "sageattention",
        "flash-attn",
    ];
    let _ = tokio_command_no_window(&uv)
        .args(&uninstall_old)
        .output()
        .await;

    // Step 2: Install the requested backend
    if backend != "default" {
        // Packages plus extra pip flags for the requested backend.
        let (packages, extra_flags): (Vec<String>, Vec<String>) = match backend.as_str() {
            "sage_v1" => {
                emit("Installing SageAttention v1...");
                if is_windows {
                    (
                        vec!["sageattention==1.0.6".into(), "triton-windows".into()],
                        vec![],
                    )
                } else {
                    (vec!["sageattention==1.0.6".into()], vec![])
                }
            }
            "sage_v2" => match &sage2_wheel {
                Some(wheel) => {
                    emit("Installing the prebuilt SageAttention v2 wheel...");
                    (vec![wheel.url.clone(), "triton-windows".into()], vec![])
                }
                None => {
                    emit("Building SageAttention v2 from source (CUDA kernels — this can take 10+ minutes)...");
                    (
                        vec![crate::attention::SAGE2_LINUX_GIT_SPEC.into()],
                        vec!["--no-build-isolation".into()],
                    )
                }
            },
            "flash_v1" => {
                emit("Installing FlashAttention v1...");
                (
                    vec!["flash-attn<2.0".into()],
                    vec!["--no-build-isolation".into()],
                )
            }
            "flash_v2" => {
                emit("Installing FlashAttention v2 (may compile from source — this can take 10+ minutes)...");
                (
                    vec!["flash-attn".into()],
                    vec!["--no-build-isolation".into()],
                )
            }
            _ => unreachable!(),
        };

        let mut cmd = tokio_command_no_window(&uv);
        cmd.arg("pip")
            .arg("install")
            .arg("--python")
            .arg(&python_str)
            .args(&packages)
            .args(&extra_flags);
        // Mirror/proxy parity with setup.rs::uv_pip (fixes silent mirror bypass).
        crate::comfyui::nodes::apply_pip_install_options(
            &mut cmd,
            true,
            network_proxy.as_deref(),
            pip_index_url.as_deref(),
        );

        let output = cmd
            .output()
            .await
            .map_err(|e| AppError::Other(format!("Failed to run uv: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Other(format!(
                "Failed to install {} attention backend: {}",
                backend,
                stderr_tail(&stderr)
            )));
        }

        // Import verification: a pip "success" that can't actually import (missing
        // triton, CUDA mismatch) is not acceptance. On failure, roll back what we
        // just installed and leave config on the previous backend.
        emit("Verifying installation...");
        let import_name = if backend.starts_with("sage") {
            "sageattention"
        } else {
            "flash_attn"
        };
        let verify = tokio_command_no_window(&venv_python)
            .args(["-c", &format!("import {}", import_name)])
            .output()
            .await
            .map_err(|e| AppError::Other(format!("Failed to run python: {}", e)))?;

        if !verify.status.success() {
            let stderr = String::from_utf8_lossy(&verify.stderr);
            emit("Verification failed — rolling back...");
            let mut rollback: Vec<&str> = vec!["pip", "uninstall", "--python", &python_str];
            rollback.extend(base_names.iter().copied());
            let _ = tokio_command_no_window(&uv).args(&rollback).output().await;
            return Err(AppError::Other(format!(
                "Installed {} but could not import it: {}. Rolled back; keeping the previous backend.",
                backend,
                stderr_tail(&stderr)
            )));
        }
    }

    // Step 3: Update config (only after a verified install)
    {
        let mut config = state.config.write().await;
        config.attention_backend = backend.clone();
        crate::config::save_config(&config)
            .map_err(|e| AppError::Other(format!("Failed to save config: {}", e)))?;
    }

    emit("Attention backend updated. Restart ComfyUI to apply.");
    log::info!("Attention backend set to: {}", backend);
    Ok(())
}
