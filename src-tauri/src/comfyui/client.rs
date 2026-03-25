use reqwest::multipart;
use serde_json::Value;

use crate::comfyui::types::*;
use crate::error::AppError;
use crate::state::AppState;

impl AppState {
    pub async fn api_get(&self, path: &str) -> Result<Value, AppError> {
        let url = format!("{}{}", self.base_url().await, path);
        let resp = self.http_client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::ApiError {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    pub async fn api_post(&self, path: &str, body: &Value) -> Result<Value, AppError> {
        let url = format!("{}{}", self.base_url().await, path);
        let resp = self.http_client.post(&url).json(body).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::ApiError {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    pub async fn get_models_list(&self, category: &str) -> Result<Vec<String>, AppError> {
        let val = self.api_get(&format!("/models/{}", category)).await?;
        let models: Vec<String> = serde_json::from_value(val)?;

        // ComfyUI may return models from wrong categories when external model
        // directories (SwarmUI, A1111, etc.) are configured — e.g. LoRAs showing
        // up under checkpoints.  Filter out entries whose path prefix indicates
        // they belong to a different category.
        let exclude: &[&str] = match category {
            "checkpoints" => &[
                "Lora/", "Lora\\", "loras/", "loras\\",
                "LyCORIS/", "LyCORIS\\",
                "VAE/", "VAE\\", "vae/", "vae\\",
                "upscale_models/", "upscale_models\\",
                "ESRGAN/", "ESRGAN\\", "RealESRGAN/", "RealESRGAN\\",
                "embeddings/", "embeddings\\",
                "controlnet/", "controlnet\\", "ControlNet/", "ControlNet\\",
                "ultralytics/", "ultralytics\\", "yolov8/", "yolov8\\",
                "clip/", "clip\\", "unet/", "unet\\",
                "diffusion_models/", "diffusion_models\\",
                "text_encoders/", "text_encoders\\",
            ],
            "loras" => &[
                "checkpoints/", "checkpoints\\",
                "Stable-diffusion/", "Stable-diffusion\\",
                "Stable-Diffusion/", "Stable-Diffusion\\",
                "StableDiffusion/", "StableDiffusion\\",
                "VAE/", "VAE\\", "vae/", "vae\\",
                "upscale_models/", "upscale_models\\",
                "ultralytics/", "ultralytics\\", "yolov8/", "yolov8\\",
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
        self.api_post("/queue", &serde_json::json!({ "delete": ids }))
            .await?;
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
    pub async fn download_model_file(
        &self,
        app: &tauri::AppHandle,
        url: &str,
        category: &str,
        filename: &str,
    ) -> Result<(), AppError> {
        use tauri::Emitter;

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
        let models_dir = std::path::Path::new(&comfyui_path)
            .join("models")
            .join(category);

        tokio::fs::create_dir_all(&models_dir).await?;
        let dest = models_dir.join(filename);

        if dest.exists() {
            return Ok(());
        }

        let resp = self.http_client.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::ApiError {
                status: resp.status().as_u16(),
                message: format!("Failed to download {}", url),
            });
        }

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
            file.write_all(&chunk)?;
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
