use serde_json::Value;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use sha2::{Sha256, Digest};
use std::io::Read;
use std::collections::BTreeSet;

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
    metadata: Option<std::collections::HashMap<String, String>>,
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

    // If metadata provided and file is PNG, embed it
    let final_bytes = if let Some(ref meta) = metadata {
        if filename.to_ascii_lowercase().ends_with(".png") {
            match crate::metadata::embed_png_metadata(&bytes, meta) {
                Ok(embedded) => embedded,
                Err(e) => {
                    log::warn!("Failed to embed metadata: {}, saving without", e);
                    bytes
                }
            }
        } else {
            bytes
        }
    } else {
        bytes
    };

    std::fs::write(&path, &final_bytes)?;
    Ok(gallery_filename)
}

#[tauri::command]
pub async fn read_image_metadata(
    filename: String,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    let path = dir.join(&filename);
    let bytes = std::fs::read(&path)?;
    crate::metadata::read_png_metadata(&bytes).map_err(|e| AppError::Other(e))
}

#[tauri::command]
pub async fn read_image_metadata_bytes(
    image_bytes: Vec<u8>,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
    crate::metadata::read_png_metadata(&image_bytes).map_err(|e| AppError::Other(e))
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
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    let path = dir.join(&filename);
    let bytes = std::fs::read(&path)?;
    Ok(bytes)
}

/// Generate a WebP thumbnail for a gallery image. Used by the `thumbnail://` protocol.
pub fn generate_thumbnail(gallery_dir: &std::path::Path, filename: &str, max_size: u32) -> Result<Vec<u8>, String> {
    // Reject path traversal attempts — filename must be a plain basename.
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err("Invalid filename".to_string());
    }
    let path = gallery_dir.join(filename);
    let bytes = std::fs::read(&path).map_err(|e| format!("Read failed: {}", e))?;

    let img = image::load_from_memory(&bytes)
        .map_err(|e| format!("Decode failed: {}", e))?;

    let thumb = img.thumbnail(max_size, max_size);

    let mut buf = std::io::Cursor::new(Vec::new());
    thumb
        .write_to(&mut buf, image::ImageFormat::WebP)
        .map_err(|e| format!("Encode failed: {}", e))?;

    Ok(buf.into_inner())
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
    use std::process::Command;
    #[cfg(target_os = "linux")]
    use std::process::Stdio;

    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(AppError::Other(format!("File not found: {}", file_path)));
    }

    let canonical = path.canonicalize()
        .map_err(|e| AppError::Other(e.to_string()))?;

    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        let image_bytes = std::fs::read(&canonical)
            .map_err(|e| AppError::Other(format!("Failed to read image file: {}", e)))?;

        let mime_type = match canonical
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            _ => "image/png",
        };

        let run_clipboard_command = |program: &str, args: &[&str]| -> Result<(), String> {
            let mut child = Command::new(program)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("{} spawn failed: {}", program, e))?;

            if let Some(ref mut stdin) = child.stdin {
                stdin
                    .write_all(&image_bytes)
                    .map_err(|e| format!("{} stdin write failed: {}", program, e))?;
            }

            let output = child
                .wait_with_output()
                .map_err(|e| format!("{} wait failed: {}", program, e))?;

            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(format!("{} exited with {}: {}", program, output.status, stderr.trim()))
            }
        };

        // Detect Wayland vs X11 and try the appropriate tool first.
        let on_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|v| v == "wayland")
                .unwrap_or(false);

        let (primary, primary_args, fallback, fallback_args): (&str, &[&str], &str, &[&str]) =
            if on_wayland {
                (
                    "wl-copy",
                    &["--type", mime_type] as &[&str],
                    "xclip",
                    &["-selection", "clipboard", "-t", mime_type, "-i"] as &[&str],
                )
            } else {
                (
                    "xclip",
                    &["-selection", "clipboard", "-t", mime_type, "-i"] as &[&str],
                    "wl-copy",
                    &["--type", mime_type] as &[&str],
                )
            };

        if let Err(primary_err) = run_clipboard_command(primary, primary_args) {
            run_clipboard_command(fallback, fallback_args).map_err(|fallback_err| {
                AppError::Other(format!(
                    "Clipboard copy failed ({} and {}). {}: {} | {}: {}",
                    primary, fallback, primary, primary_err, fallback, fallback_err
                ))
            })?;
        }
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

/// Check if a ComfyUI node class is available (used to detect custom node packages).
#[tauri::command]
pub async fn check_node_available(
    state: State<'_, AppState>,
    node_class: String,
) -> Result<bool, AppError> {
    match state.api_get(&format!("/object_info/{}", node_class)).await {
        Ok(val) => Ok(val.get(&node_class).is_some()),
        Err(_) => Ok(false),
    }
}

/// Check if a custom node package is installed on disk (directory exists in custom_nodes/).
#[tauri::command]
pub async fn is_custom_node_installed(
    state: State<'_, AppState>,
    node_name: String,
) -> Result<bool, AppError> {
    let config = state.config.read().await;
    let target_dir = std::path::Path::new(&config.comfyui_path)
        .join("custom_nodes")
        .join(&node_name);
    Ok(target_dir.exists())
}

/// Install a custom node from a git repository into ComfyUI's custom_nodes directory.
/// Emits `install:progress` events with { node_name, step, message, done } for live progress.
#[tauri::command]
pub async fn install_custom_node(
    app: AppHandle,
    state: State<'_, AppState>,
    git_url: String,
    node_name: String,
) -> Result<(), AppError> {
    let config = state.config.read().await;
    let custom_nodes_dir = std::path::Path::new(&config.comfyui_path).join("custom_nodes");
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

    let mut child = tokio::process::Command::new("git")
        .args(["clone", "--progress", &git_url, &target_dir.to_string_lossy().as_ref()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
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

        #[cfg(target_os = "windows")]
        let pip_path = format!("{}/Scripts/pip.exe", config.venv_path);
        #[cfg(not(target_os = "windows"))]
        let pip_path = format!("{}/bin/pip", config.venv_path);

        let mut pip_child = tokio::process::Command::new(&pip_path)
            .args(["install", "-r", &req_file.to_string_lossy()])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AppError::Other(format!("pip install failed to start: {}", e)))?;

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
            emit_progress("error", "pip install failed (some features may not work)", false);
            log::warn!("pip install requirements failed for {}", node_name);
        }
    }

    emit_progress("done", &format!("{} installed successfully", node_name), true);

    // Emit event so frontend knows to restart ComfyUI
    let _ = app.emit("custom_node:installed", &node_name);
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
        .header("User-Agent", "MooshieUI/0.2.9")
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

#[tauri::command]
pub async fn civitai_search_models(
    state: State<'_, AppState>,
    params: CivitaiSearchParams,
) -> Result<Value, AppError> {
    // Build query string manually because reqwest percent-encodes brackets in
    // parameter names (baseModels[] → baseModels%5B%5D) which CivitAI ignores.
    let encode_val = |v: &str| -> String {
        url::form_urlencoded::byte_serialize(v.as_bytes()).collect()
    };

    let mut parts: Vec<String> = vec![
        format!("sort={}", encode_val(&params.sort.unwrap_or_else(|| "Most Downloaded".to_string()))),
        format!("period={}", encode_val(&params.period.unwrap_or_else(|| "AllTime".to_string()))),
        format!("nsfw={}", params.nsfw.unwrap_or(false)),
        format!("limit={}", params.limit.unwrap_or(20)),
    ];

    let has_query = params.query.as_ref().filter(|v| !v.trim().is_empty()).is_some();

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
        .header("User-Agent", "MooshieUI/0.2.9");

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

#[tauri::command]
pub async fn civitai_list_architectures(
    state: State<'_, AppState>,
    api_key: Option<String>,
) -> Result<Vec<String>, AppError> {
    let mut architectures = BTreeSet::<String>::new();
    
    // Add common architectures first to guarantee they're present
    let common = vec![
        // Stable Diffusion 1.x
        "SD 1.4", "SD 1.5", "SD 1.5 LCM", "SD 1.5 Hyper",
        // Stable Diffusion 2.x
        "SD 2.0", "SD 2.0 768", "SD 2.1", "SD 2.1 768", "SD 2.1 Unclip",
        // Stable Diffusion 3.x
        "SD 3", "SD 3.5", "SD 3.5 Large", "SD 3.5 Large Turbo", "SD 3.5 Medium",
        // SDXL
        "SDXL 0.9", "SDXL 1.0", "SDXL 1.0 LCM", "SDXL Distilled", "SDXL Turbo", "SDXL Lightning", "SDXL Hyper",
        // Anime / Illustrious / NoobAI / Pony
        "Illustrious", "NoobAI", "Pony",
        // Flux
        "Flux.1 S", "Flux.1 D", "Flux.1 S Turbo",
        // Other popular architectures
        "AuraFlow", "Hunyuan 1", "HunyuanDiT", "Hunyuan Video",
        "Lumina", "Kolors", "PixArt-a", "PixArt-E",
        "Stable Cascade", "SVD", "SVD XT",
        "PlaygroundV2.5", "CogVideoX",
        // Misc
        "Illusion", "MoDi", "ODOR", "Other",
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
            .header("User-Agent", "MooshieUI/0.2.9")
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
                        if let Some(base_model) = version.get("baseModel").and_then(|v| v.as_str()) {
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

/// Read the ModelSpec metadata from a safetensors file header.
/// Returns a map of modelspec fields (without the "modelspec." prefix) if present,
/// or null if the file has no ModelSpec metadata.
#[tauri::command]
pub async fn read_modelspec(
    state: State<'_, AppState>,
    category: String,
    filename: String,
) -> Result<Option<std::collections::HashMap<String, String>>, AppError> {
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

    // Only process .safetensors files
    if !filename.ends_with(".safetensors") {
        return Ok(None);
    }

    read_safetensors_modelspec(&path)
}

/// Parse the safetensors JSON header and extract modelspec.* fields.
fn read_safetensors_modelspec(
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
        _ => return Ok(None),
    };

    let mut result = std::collections::HashMap::new();
    for (key, value) in metadata {
        if let Some(field) = key.strip_prefix("modelspec.") {
            if let Some(s) = value.as_str() {
                result.insert(field.to_string(), s.to_string());
            }
        }
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}
