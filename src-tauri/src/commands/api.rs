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
pub async fn copy_image_to_clipboard(image_bytes: Vec<u8>) -> Result<(), AppError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Try xclip first, then xsel, then wl-copy (Wayland)
    let result = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "image/png"])
        .stdin(Stdio::piped())
        .spawn();

    let mut child = match result {
        Ok(child) => child,
        Err(_) => {
            // Try xsel
            let result = Command::new("xsel")
                .args(["--clipboard", "--input"])
                .stdin(Stdio::piped())
                .spawn();
            match result {
                Ok(child) => child,
                Err(_) => {
                    // Try wl-copy (Wayland)
                    Command::new("wl-copy")
                        .args(["--type", "image/png"])
                        .stdin(Stdio::piped())
                        .spawn()
                        .map_err(|e| AppError::Other(format!(
                            "No clipboard tool found (tried xclip, xsel, wl-copy): {}", e
                        )))?
                }
            }
        }
    };

    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(&image_bytes)?;
    }
    child.wait().map_err(|e| AppError::Other(format!("Clipboard tool failed: {}", e)))?;
    Ok(())
}
