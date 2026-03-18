use serde_json::Value;
use serde::Serialize;
use tauri::{AppHandle, State};
use sha2::{Sha256, Digest};
use std::io::Read;

use crate::comfyui::types::*;
use crate::error::AppError;
use crate::state::AppState;

/// Compute the full SHA256 hash of a file (uppercase hex).
/// Compatible with CivitAI's hash database.
/// For large model files (2-10 GB) this can take a few seconds.
fn full_sha256(path: &std::path::Path) -> Result<String, AppError> {
    const BUF_SIZE: usize = 8 * 1024 * 1024; // 8 MB read buffer
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    let result = hasher.finalize();
    Ok(format!("{:X}", result))
}

/// Return the AutoV2 hash (first 10 chars of SHA256, uppercase).
/// This is the standard format used by CivitAI, A1111, Forge, etc.
fn autov2_hash(full_hash: &str) -> String {
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
}

#[tauri::command]
pub async fn get_models(
    state: State<'_, AppState>,
    category: String,
) -> Result<Vec<String>, AppError> {
    state.get_models_list(&category).await
}

#[tauri::command]
pub async fn get_samplers(state: State<'_, AppState>) -> Result<SamplerInfo, AppError> {
    state.get_samplers_and_schedulers().await
}

#[tauri::command]
pub async fn get_embeddings(state: State<'_, AppState>) -> Result<Vec<String>, AppError> {
    state.get_embeddings_list().await
}

#[tauri::command]
pub async fn get_queue(state: State<'_, AppState>) -> Result<QueueInfo, AppError> {
    state.get_queue_info().await
}

#[tauri::command]
pub async fn get_history(
    state: State<'_, AppState>,
    prompt_id: String,
) -> Result<Value, AppError> {
    state.get_history_for(&prompt_id).await
}

#[tauri::command]
pub async fn interrupt_generation(state: State<'_, AppState>) -> Result<(), AppError> {
    state.interrupt().await
}

#[tauri::command]
pub async fn delete_queue_item(
    state: State<'_, AppState>,
    prompt_id: String,
) -> Result<(), AppError> {
    state.delete_queue_items(vec![prompt_id]).await
}

#[tauri::command]
pub async fn upload_image(
    state: State<'_, AppState>,
    image_path: String,
) -> Result<UploadResponse, AppError> {
    state.upload_image_file(&image_path).await
}

#[tauri::command]
pub async fn upload_image_bytes(
    state: State<'_, AppState>,
    image_bytes: Vec<u8>,
    filename: String,
) -> Result<UploadResponse, AppError> {
    state.upload_image_from_bytes(image_bytes, filename).await
}

#[tauri::command]
pub async fn get_output_image(
    state: State<'_, AppState>,
    filename: String,
    subfolder: String,
) -> Result<Vec<u8>, AppError> {
    state.get_output_image_bytes(&filename, &subfolder).await
}

#[tauri::command]
pub async fn get_client_id(state: State<'_, AppState>) -> Result<String, AppError> {
    Ok(state.client_id.clone())
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    category: String,
    filename: String,
) -> Result<(), AppError> {
    state
        .download_model_file(&app, &url, &category, &filename)
        .await
}

#[tauri::command]
pub async fn save_image_file(image_bytes: Vec<u8>, path: String) -> Result<(), AppError> {
    std::fs::write(&path, &image_bytes)?;
    Ok(())
}

#[tauri::command]
pub async fn save_to_gallery(
    state: State<'_, AppState>,
    filename: String,
    subfolder: String,
    prompt_id: String,
    mode: Option<String>,
) -> Result<String, AppError> {
    let bytes = state.get_output_image_bytes(&filename, &subfolder).await?;
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    std::fs::create_dir_all(&dir)?;

    let normalized_mode = match mode.as_deref() {
        Some("txt2img") => "txt2img",
        Some("img2img") => "img2img",
        Some("inpainting") => "inpainting",
        _ => "unknown",
    };

    let gallery_filename = format!("{}__{}__{}", prompt_id, normalized_mode, filename);
    let path = dir.join(&gallery_filename);
    std::fs::write(&path, &bytes)?;
    Ok(gallery_filename)
}

#[tauri::command]
pub async fn list_gallery_images() -> Result<Vec<String>, AppError> {
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg") || name.ends_with(".webp") {
                Some((entry.metadata().ok()?.modified().ok()?, name))
            } else {
                None
            }
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(files.into_iter().map(|(_, name)| name).collect())
}

#[tauri::command]
pub async fn list_gallery_image_entries() -> Result<Vec<GalleryImageEntry>, AppError> {
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.ends_with(".png")
                || name.ends_with(".jpg")
                || name.ends_with(".jpeg")
                || name.ends_with(".webp"))
            {
                return None;
            }

            let metadata = entry.metadata().ok()?;
            let modified = metadata.modified().ok()?;
            let modified_ms = modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis() as u64;

            Some(GalleryImageEntry {
                filename: name,
                size_bytes: metadata.len(),
                modified_ms,
            })
        })
        .collect();

    files.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    Ok(files)
}

#[tauri::command]
pub async fn load_gallery_image(filename: String) -> Result<Vec<u8>, AppError> {
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    let path = dir.join(&filename);
    let bytes = std::fs::read(&path)?;
    Ok(bytes)
}

#[tauri::command]
pub async fn get_gallery_image_path(filename: String) -> Result<String, AppError> {
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    let path = dir.join(&filename);
    if !path.exists() {
        return Err(AppError::Other(format!("Gallery image not found: {}", filename)));
    }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn delete_gallery_image(filename: String) -> Result<(), AppError> {
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    let path = dir.join(&filename);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn rename_gallery_image(old_filename: String, new_filename: String) -> Result<String, AppError> {
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");

    let old_path = dir.join(&old_filename);
    if !old_path.exists() {
        return Err(AppError::Other(format!("Gallery image not found: {}", old_filename)));
    }

    let new_path = dir.join(&new_filename);
    if new_path.exists() {
        return Err(AppError::Other(format!("Target gallery filename already exists: {}", new_filename)));
    }

    std::fs::rename(&old_path, &new_path)?;
    Ok(new_filename)
}

#[tauri::command]
pub async fn copy_image_to_clipboard(file_path: String) -> Result<(), AppError> {
    use std::process::{Command, Stdio};

    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(AppError::Other(format!("File not found: {}", file_path)));
    }

    let canonical = path.canonicalize()
        .map_err(|e| AppError::Other(e.to_string()))?;

    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        let uri = format!("file://{}\n", canonical.display());

        // Try xclip (X11), fall back to wl-copy (Wayland)
        let result = Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "text/uri-list"])
            .stdin(Stdio::piped())
            .spawn();

        let mut child = match result {
            Ok(child) => child,
            Err(_) => {
                Command::new("wl-copy")
                    .args(["--type", "text/uri-list"])
                    .stdin(Stdio::piped())
                    .spawn()
                    .map_err(|e| AppError::Other(format!(
                        "No clipboard tool found (tried xclip, wl-copy): {}", e
                    )))?
            }
        };

        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(uri.as_bytes())?;
        }
        child.wait().map_err(|e| AppError::Other(format!("Clipboard tool failed: {}", e)))?;
    }

    #[cfg(target_os = "macos")]
    {
        // Use osascript to copy file to clipboard (preserves format + metadata)
        let script = format!(
            "set the clipboard to (POSIX file \"{}\")",
            canonical.display()
        );
        let status = Command::new("osascript")
            .args(["-e", &script])
            .status()
            .map_err(|e| AppError::Other(format!("osascript failed: {}", e)))?;
        if !status.success() {
            return Err(AppError::Other("Failed to copy file to clipboard via osascript".into()));
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Use PowerShell Set-Clipboard with file list
        let ps_cmd = format!(
            "Set-Clipboard -Path '{}'",
            canonical.display()
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .status()
            .map_err(|e| AppError::Other(format!("PowerShell failed: {}", e)))?;
        if !status.success() {
            return Err(AppError::Other("Failed to copy file to clipboard via PowerShell".into()));
        }
    }

    Ok(())
}

/// Search for a model file by SHA256 hash (full or AutoV2) within a model category directory.
/// Returns the filename if found, or null if no match.
/// Note: this hashes each file in the directory, so it may take a while for large collections.
#[tauri::command]
pub async fn find_model_by_hash(
    state: State<'_, AppState>,
    category: String,
    hash: String,
) -> Result<Option<String>, AppError> {
    let config = state.config.read().await;
    if config.comfyui_path.is_empty() {
        return Ok(None);
    }
    let models_dir = std::path::Path::new(&config.comfyui_path)
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
#[tauri::command]
pub async fn hash_model_file(
    state: State<'_, AppState>,
    category: String,
    filename: String,
) -> Result<ModelHashResult, AppError> {
    let config = state.config.read().await;
    if config.comfyui_path.is_empty() {
        return Err(AppError::Other("ComfyUI path not configured".into()));
    }
    let path = std::path::Path::new(&config.comfyui_path)
        .join("models")
        .join(&category)
        .join(&filename);

    if !path.exists() {
        return Err(AppError::Other(format!("File not found: {}", filename)));
    }
    let sha256 = full_sha256(&path)?;
    let autov2 = autov2_hash(&sha256);
    Ok(ModelHashResult { sha256, autov2 })
}

/// Look up a model on CivitAI by its hash (SHA256 or AutoV2).
/// Returns the CivitAI model version info if found.
#[tauri::command]
pub async fn civitai_lookup_hash(hash: String) -> Result<Value, AppError> {
    let url = format!("https://civitai.com/api/v1/model-versions/by-hash/{}", hash);
    let resp = reqwest::Client::new()
        .get(&url)
        .header("User-Agent", "MooshieUI/0.2.2")
        .send()
        .await
        .map_err(|e| AppError::Other(format!("CivitAI request failed: {}", e)))?;

    if resp.status() == 404 {
        return Err(AppError::Other("Model not found on CivitAI".into()));
    }
    if !resp.status().is_success() {
        return Err(AppError::Other(format!("CivitAI returned status {}", resp.status())));
    }

    let data: Value = resp.json().await
        .map_err(|e| AppError::Other(format!("Failed to parse CivitAI response: {}", e)))?;
    Ok(data)
}
