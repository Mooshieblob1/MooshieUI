use serde_json::Value;
use tauri::State;

use crate::comfyui::types::*;
use crate::error::AppError;
use crate::state::AppState;

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
    state: State<'_, AppState>,
    url: String,
    category: String,
    filename: String,
) -> Result<(), AppError> {
    state.download_model_file(&url, &category, &filename).await
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
) -> Result<String, AppError> {
    let bytes = state.get_output_image_bytes(&filename, &subfolder).await?;
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    std::fs::create_dir_all(&dir)?;

    let gallery_filename = format!("{}_{}", prompt_id, filename);
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
