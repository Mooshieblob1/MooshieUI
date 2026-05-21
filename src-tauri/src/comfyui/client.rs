use reqwest::multipart;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::comfyui::types::*;
use crate::error::AppError;
use crate::state::AppState;

fn is_optional_model_category(category: &str) -> bool {
    matches!(
        category,
        "diffusion_models" | "unet" | "text_encoders" | "clip" | "controlnet" | "ultralytics"
    )
}

fn is_huggingface_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| host == "huggingface.co" || host.ends_with(".huggingface.co"))
}

pub fn huggingface_token_for_url(url: &str) -> Option<String> {
    if !is_huggingface_url(url) {
        return None;
    }

    ["HF_TOKEN", "HUGGINGFACE_HUB_TOKEN", "HUGGINGFACE_TOKEN"]
        .iter()
        .find_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

pub fn download_status_error_message(url: &str, status: reqwest::StatusCode) -> String {
    if is_huggingface_url(url) && matches!(status.as_u16(), 401 | 403) {
        format!(
            "Failed to download {url}: HTTP {status}. This Hugging Face file requires access; set HF_TOKEN, HUGGINGFACE_HUB_TOKEN, or HUGGINGFACE_TOKEN, or install the file manually."
        )
    } else {
        format!("Failed to download {url}: HTTP {status}")
    }
}

pub fn reject_non_model_download_content_type(
    url: &str,
    content_type: &str,
) -> Result<(), AppError> {
    let content_type = content_type.to_ascii_lowercase();
    let looks_like_error_body = [
        "text/html",
        "text/plain",
        "application/json",
        "application/problem+json",
        "application/xml",
        "text/xml",
    ]
    .iter()
    .any(|needle| content_type.contains(needle));

    if looks_like_error_body {
        return Err(AppError::ApiError {
            status: 200,
            message: format!(
                "The URL returned '{}' instead of model bytes for {}. This usually means the download requires authentication, a direct file URL, or a different token.",
                content_type,
                url
            ),
        });
    }

    Ok(())
}

pub fn validate_downloaded_model_file(
    path: &std::path::Path,
    filename: &str,
) -> Result<(), AppError> {
    let size = std::fs::metadata(path)?.len();
    if size == 0 {
        return Err(AppError::Other(format!(
            "Downloaded model '{}' is empty",
            filename
        )));
    }

    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();

    if ext == "safetensors" || ext == "sft" {
        validate_safetensors_file(path, filename, size)?;
    }

    Ok(())
}

fn validate_safetensors_file(
    path: &std::path::Path,
    filename: &str,
    file_size: u64,
) -> Result<(), AppError> {
    use std::io::Read;

    if file_size < 9 {
        return Err(AppError::Other(format!(
            "'{}' is too small to be a valid safetensors file",
            filename
        )));
    }

    let mut file = std::fs::File::open(path)?;
    let mut size_buf = [0u8; 8];
    file.read_exact(&mut size_buf)?;
    let header_size = u64::from_le_bytes(size_buf);

    if header_size == 0 || header_size > 100 * 1024 * 1024 {
        return Err(AppError::Other(format!(
            "'{}' has an invalid safetensors header size ({})",
            filename, header_size
        )));
    }
    if 8 + header_size > file_size {
        return Err(AppError::Other(format!(
            "'{}' is incomplete: safetensors header declares {} bytes but the file is only {} bytes",
            filename, header_size, file_size
        )));
    }

    let mut header_buf = vec![0u8; header_size as usize];
    file.read_exact(&mut header_buf)?;
    let header: serde_json::Value = serde_json::from_slice(&header_buf).map_err(|e| {
        AppError::Other(format!(
            "'{}' is not a valid safetensors file: {}. The server may have saved an HTML/JSON error page instead of the model.",
            filename, e
        ))
    })?;
    let header = header.as_object().ok_or_else(|| {
        AppError::Other(format!("'{}' has an invalid safetensors header", filename))
    })?;

    let mut tensor_count = 0usize;
    let mut max_end = 0u64;
    for (key, value) in header {
        if key == "__metadata__" {
            continue;
        }
        tensor_count += 1;
        if let Some(offsets) = value.get("data_offsets").and_then(|v| v.as_array()) {
            if let Some(end) = offsets.get(1).and_then(|v| v.as_u64()) {
                max_end = max_end.max(end);
            }
        }
    }

    if tensor_count == 0 {
        return Err(AppError::Other(format!(
            "'{}' has no tensors in its safetensors header",
            filename
        )));
    }

    let required_size = 8 + header_size + max_end;
    if required_size > file_size {
        return Err(AppError::Other(format!(
            "'{}' is incomplete: tensor data ends at byte {} but the file is only {} bytes",
            filename, required_size, file_size
        )));
    }

    Ok(())
}

/// Compute SHA256 of a file. Returns lowercase hex. Used to verify downloaded model files.
pub fn sha256_file(path: &std::path::Path) -> Result<String, AppError> {
    use std::io::Read;
    const BUF_SIZE: usize = 8 * 1024 * 1024;
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
    Ok(format!("{:x}", hasher.finalize()))
}

impl AppState {
    pub async fn api_get(&self, path: &str) -> Result<Value, AppError> {
        // Delegate to the GPU manager so the request is dispatched to a real
        // worker port. The legacy single-`server_url` path was broken in
        // multi-GPU server-mode deployments where `server_url` (default
        // 127.0.0.1:8188) doesn't actually host ComfyUI — workers run on
        // 8188, 8189, … per `gpu_workers` config — causing /models, /samplers
        // and /embeddings to 500 with connection-refused.
        self.gpu_manager.api_get(path).await
    }

    pub async fn api_post(&self, path: &str, body: &Value) -> Result<Value, AppError> {
        self.gpu_manager.api_post(path, body).await
    }

    pub async fn get_models_list(&self, category: &str) -> Result<Vec<String>, AppError> {
        let val = match self.api_get(&format!("/models/{}", category)).await {
            Ok(value) => value,
            Err(e) if is_optional_model_category(category) => {
                log::debug!(
                    "Optional ComfyUI model category '{}' unavailable: {}",
                    category,
                    e
                );
                return Ok(Vec::new());
            }
            Err(e) => return Err(e),
        };
        let models: Vec<String> = serde_json::from_value(val)?;

        // ComfyUI may return models from wrong categories when external model
        // directories (SwarmUI, A1111, etc.) are configured — e.g. LoRAs showing
        // up under checkpoints.  Filter out entries whose path prefix indicates
        // they belong to a different category.
        let exclude: &[&str] = match category {
            "checkpoints" => &[
                "Lora/",
                "Lora\\",
                "loras/",
                "loras\\",
                "LyCORIS/",
                "LyCORIS\\",
                "VAE/",
                "VAE\\",
                "vae/",
                "vae\\",
                "upscale_models/",
                "upscale_models\\",
                "ESRGAN/",
                "ESRGAN\\",
                "RealESRGAN/",
                "RealESRGAN\\",
                "embeddings/",
                "embeddings\\",
                "controlnet/",
                "controlnet\\",
                "ControlNet/",
                "ControlNet\\",
                "ultralytics/",
                "ultralytics\\",
                "yolov8/",
                "yolov8\\",
                "clip/",
                "clip\\",
                "unet/",
                "unet\\",
                "diffusion_models/",
                "diffusion_models\\",
                "text_encoders/",
                "text_encoders\\",
            ],
            "loras" => &[
                "checkpoints/",
                "checkpoints\\",
                "Stable-diffusion/",
                "Stable-diffusion\\",
                "Stable-Diffusion/",
                "Stable-Diffusion\\",
                "StableDiffusion/",
                "StableDiffusion\\",
                "VAE/",
                "VAE\\",
                "vae/",
                "vae\\",
                "upscale_models/",
                "upscale_models\\",
                "ultralytics/",
                "ultralytics\\",
                "yolov8/",
                "yolov8\\",
            ],
            _ => &[],
        };

        if exclude.is_empty() {
            Ok(models)
        } else {
            Ok(models
                .into_iter()
                .filter(|m| !exclude.iter().any(|pfx| m.starts_with(pfx)))
                .collect())
        }
    }

    pub async fn get_samplers_and_schedulers(&self) -> Result<SamplerInfo, AppError> {
        let val = self.api_get("/object_info/KSampler").await?;

        let samplers = val["KSampler"]["input"]["required"]["sampler_name"][0]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let schedulers = val["KSampler"]["input"]["required"]["scheduler"][0]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(SamplerInfo {
            samplers,
            schedulers,
        })
    }

    pub async fn get_embeddings_list(&self) -> Result<Vec<String>, AppError> {
        let val = self.api_get("/embeddings").await?;
        let embeddings: Vec<String> = serde_json::from_value(val)?;
        Ok(embeddings)
    }

    pub async fn queue_prompt_request(
        &self,
        workflow: Value,
        client_id: &str,
    ) -> Result<PromptResponse, AppError> {
        let body = serde_json::json!({
            "prompt": workflow,
            "client_id": client_id,
        });
        let val = self.api_post("/prompt", &body).await?;
        let resp: PromptResponse = serde_json::from_value(val)?;
        Ok(resp)
    }

    pub async fn get_history_for(&self, prompt_id: &str) -> Result<Value, AppError> {
        self.api_get(&format!("/history/{}", prompt_id)).await
    }

    pub async fn get_queue_info(&self) -> Result<QueueInfo, AppError> {
        let val = self.api_get("/queue").await?;
        let info: QueueInfo = serde_json::from_value(val)?;
        Ok(info)
    }

    pub async fn interrupt(&self) -> Result<(), AppError> {
        self.api_post("/interrupt", &serde_json::json!({})).await?;
        // Flush execution cache and free VRAM after interruption.
        // Rapid interrupts on Blackwell GPUs with cudaMallocAsync can leave
        // VRAM in an inconsistent state, causing subsequent gens to produce
        // all-black images from corrupted model weights.
        let _ = self
            .api_post(
                "/free",
                &serde_json::json!({ "unload_models": true, "free_memory": true }),
            )
            .await;
        Ok(())
    }

    pub async fn delete_queue_items(&self, ids: Vec<String>) -> Result<(), AppError> {
        let mut ids_to_delete: Vec<String> = Vec::new();
        for id in ids {
            for related_id in self.prompt_queue.cancel_and_remove(&id) {
                if !ids_to_delete.iter().any(|existing| existing == &related_id) {
                    ids_to_delete.push(related_id);
                }
            }
        }

        for hp in self.prompt_queue.take_held_related_to(&ids_to_delete) {
            {
                let mut result = hp.result.lock().await;
                *result = Some(Err("generation.error_cancelled".to_string()));
            }
            hp.submitted.notify_one();
        }

        if !ids_to_delete.is_empty() {
            for worker in &self.gpu_manager.workers {
                let _ = self
                    .http_client
                    .post(format!("{}/queue", worker.base_url))
                    .json(&serde_json::json!({ "delete": ids_to_delete }))
                    .send()
                    .await;
            }
        }

        self.broadcast_queue_positions();
        self.prompt_queue.drain_notify.notify_one();
        Ok(())
    }

    pub async fn get_system_stats_info(&self) -> Result<SystemStats, AppError> {
        let val = self.api_get("/system_stats").await?;
        let stats: SystemStats = serde_json::from_value(val)?;
        Ok(stats)
    }

    pub async fn upload_image_file(&self, file_path: &str) -> Result<UploadResponse, AppError> {
        let url = format!("{}/upload/image", self.base_url().await);
        let file_bytes = tokio::fs::read(file_path).await?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let part = multipart::Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str("image/png")
            .unwrap();

        let form = multipart::Form::new()
            .part("image", part)
            .text("type", "input")
            .text("overwrite", "true");

        let resp = self.http_client.post(&url).multipart(form).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::ApiError {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        let upload_resp: UploadResponse = resp.json().await?;
        Ok(upload_resp)
    }

    pub async fn upload_image_from_bytes(
        &self,
        bytes: Vec<u8>,
        filename: String,
    ) -> Result<UploadResponse, AppError> {
        let url = format!("{}/upload/image", self.base_url().await);

        let part = multipart::Part::bytes(bytes)
            .file_name(filename)
            .mime_str("image/png")
            .unwrap();

        let form = multipart::Form::new()
            .part("image", part)
            .text("type", "input")
            .text("overwrite", "true");

        let resp = self.http_client.post(&url).multipart(form).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::ApiError {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        let upload_resp: UploadResponse = resp.json().await?;
        Ok(upload_resp)
    }

    /// Downloads a file from a URL to the models/<category> directory,
    /// emitting `download:progress` events with byte-level progress.
    /// If `dest_dir_override` is provided, the file is written there instead
    /// of the default `{comfyui_path}/models/{category}` path.
    #[cfg(feature = "desktop")]
    pub async fn download_model_file(
        &self,
        app: &tauri::AppHandle,
        url: &str,
        category: &str,
        filename: &str,
        dest_dir_override: Option<&str>,
        expected_sha256: Option<&str>,
    ) -> Result<(), AppError> {
        use tauri::Emitter;

        let models_dir = if let Some(dir) = dest_dir_override {
            std::path::PathBuf::from(dir)
        } else {
            let config = self.config.read().await;
            let comfyui_path = if config.comfyui_path.is_empty() {
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()));
                if let Some(dir) = exe_dir {
                    dir.to_string_lossy().to_string()
                } else {
                    ".".to_string()
                }
            } else {
                config.comfyui_path.clone()
            };
            std::path::Path::new(&comfyui_path)
                .join("models")
                .join(category)
        };

        tokio::fs::create_dir_all(&models_dir).await?;
        let dest = models_dir.join(filename);

        // Skip if the file already exists and is non-empty. If an expected hash is
        // supplied, verify the cached file before trusting it — a tampered file
        // is re-downloaded rather than silently accepted.
        if dest.exists() {
            let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            if size > 0 {
                let cached_is_valid = validate_downloaded_model_file(&dest, filename).is_ok();
                if !cached_is_valid {
                    let _ = std::fs::remove_file(&dest);
                } else if let Some(expected_hex) = expected_sha256 {
                    let dest_clone = dest.clone();
                    let computed = tokio::task::spawn_blocking(move || sha256_file(&dest_clone))
                        .await
                        .map_err(|e| AppError::Other(format!("Hash task failed: {}", e)))??;
                    if computed == expected_hex.to_lowercase() {
                        return Ok(()); // Trusted cache hit.
                    }
                    // Hash mismatch — fall through to re-download.
                } else {
                    return Ok(()); // No verification requested.
                }
            }
            // Zero-byte leftover or hash mismatch — remove before re-downloading.
            let _ = std::fs::remove_file(&dest);
        }

        let mut req = self.http_client.get(url);
        if let Some(token) = huggingface_token_for_url(url) {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(AppError::ApiError {
                status: status.as_u16(),
                message: download_status_error_message(url, status),
            });
        }

        // Reject HTML responses — they indicate a web page URL was used instead
        // of a direct file URL (e.g. a HuggingFace model page instead of a /resolve/ URL).
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        reject_non_model_download_content_type(url, &content_type)?;

        let total = resp.content_length().unwrap_or(0);
        let mut downloaded: u64 = 0;
        let mut file = std::fs::File::create(&dest)?;
        let mut last_emit: u64 = 0;

        app.emit(
            "download:progress",
            crate::setup::DownloadProgress {
                filename: filename.to_string(),
                downloaded: 0,
                total,
                done: false,
            },
        )
        .ok();

        let mut resp = resp;
        while let Some(chunk) = resp.chunk().await? {
            use std::io::Write;
            if let Err(e) = file.write_all(&chunk) {
                drop(file);
                let _ = std::fs::remove_file(&dest);
                return Err(e.into());
            }
            downloaded += chunk.len() as u64;

            if downloaded - last_emit > 256 * 1024 || downloaded == total {
                last_emit = downloaded;
                app.emit(
                    "download:progress",
                    crate::setup::DownloadProgress {
                        filename: filename.to_string(),
                        downloaded,
                        total,
                        done: false,
                    },
                )
                .ok();
            }
        }

        app.emit(
            "download:progress",
            crate::setup::DownloadProgress {
                filename: filename.to_string(),
                downloaded,
                total,
                done: true,
            },
        )
        .ok();

        // Verify the downloaded file matches the expected SHA256 if supplied.
        if let Some(expected_hex) = expected_sha256 {
            let dest_clone = dest.clone();
            let computed = tokio::task::spawn_blocking(move || sha256_file(&dest_clone))
                .await
                .map_err(|e| AppError::Other(format!("Hash task failed: {}", e)))??;
            if computed != expected_hex.to_lowercase() {
                let _ = std::fs::remove_file(&dest);
                return Err(AppError::Other(format!(
                    "SHA256 mismatch for '{}': expected {}, got {}",
                    filename,
                    expected_hex.to_lowercase(),
                    computed
                )));
            }
        }

        validate_downloaded_model_file(&dest, filename)?;

        Ok(())
    }

    pub async fn get_output_image_bytes(
        &self,
        filename: &str,
        subfolder: &str,
    ) -> Result<Vec<u8>, AppError> {
        let url = format!(
            "{}/view?filename={}&subfolder={}&type=output",
            self.base_url().await,
            filename,
            subfolder
        );
        let resp = self.http_client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::ApiError {
                status: resp.status().as_u16(),
                message: "Failed to fetch image".to_string(),
            });
        }
        Ok(resp.bytes().await?.to_vec())
    }
}
