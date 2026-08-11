pub mod catalog;
pub mod grounding;
pub mod hardware;
pub mod local_llm;
pub mod oauth;
pub mod providers;
pub mod server;
pub mod vision;

use std::path::PathBuf;
use std::sync::Arc;

use crate::config;
use crate::error::AppError;
use server::LlamaServer;

/// Top-level prompt-assistant state held in AppState.
pub struct PromptAssistant {
    /// {app_data}/prompt-assistant
    root: PathBuf,
    pub server: Arc<LlamaServer>,
}

impl Default for PromptAssistant {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptAssistant {
    pub fn new() -> Self {
        let root = config::app_data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("prompt-assistant");
        // A deployment can supply its own llama-server (e.g. a CUDA build baked
        // into the Docker image) via MOOSHIEUI_LLAMA_BIN_DIR. The release only
        // ships a CPU build for Linux, so this is how the server image gets
        // GPU-accelerated enhance/compose. When unset, fall back to the
        // downloaded binary under the app data dir.
        let (bin_dir, external) = match std::env::var("MOOSHIEUI_LLAMA_BIN_DIR") {
            Ok(d) if !d.trim().is_empty() => (PathBuf::from(d), true),
            _ => (root.join("bin"), false),
        };
        let server = Arc::new(LlamaServer::new(bin_dir, external));
        Self { root, server }
    }

    fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    /// Local on-disk path for a catalog variant's weight file.
    pub fn model_file_path(&self, model_id: &str, file: &str) -> PathBuf {
        self.models_dir().join(model_id).join(file)
    }

    /// Whether ANY variant of a catalog model id is installed.
    pub fn is_model_installed(&self, model_id: &str) -> bool {
        let entry = match catalog::entry(model_id) {
            Some(e) => e,
            None => return false,
        };
        entry
            .variants
            .iter()
            .any(|v| self.model_file_path(model_id, &v.file).exists())
    }

    /// Catalog model ids that are installed.
    pub fn installed_models(&self) -> Vec<String> {
        catalog::catalog()
            .into_iter()
            .filter(|e| self.is_model_installed(&e.id))
            .map(|e| e.id)
            .collect()
    }

    /// Find the installed variant file path for a model id.
    fn installed_variant(&self, model_id: &str) -> Option<PathBuf> {
        let entry = catalog::entry(model_id)?;
        for v in &entry.variants {
            let p = self.model_file_path(model_id, &v.file);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    /// Download a specific variant (by format key: "gguf:Q4_K_M") of a model.
    pub async fn download_model(
        &self,
        client: &reqwest::Client,
        model_id: &str,
        variant_key: &str,
        progress: &(dyn Fn(&str, u64, u64, bool) + Sync),
    ) -> Result<(), AppError> {
        let entry = catalog::entry(model_id)
            .ok_or_else(|| AppError::LlmError("Unknown model id".into()))?;
        let variant = entry
            .variants
            .iter()
            .find(|v| variant_matches(v, variant_key))
            .ok_or_else(|| AppError::LlmError("Unknown variant".into()))?;

        let dest = self.model_file_path(model_id, &variant.file);
        if dest.exists() {
            return Ok(());
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // HuggingFace resolve URL (mirrors interrogator's HF_BASE_URL pattern).
        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            variant.repo, variant.file
        );
        download_to(client, &url, &dest, &variant.file, progress).await
    }

    /// Delete all installed files for a model id.
    pub fn delete_model(&self, model_id: &str) -> Result<(), AppError> {
        let dir = self.models_dir().join(model_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Ensure the server is up and the model loaded, starting the idle watchdog.
    /// `total_vram_mb` drives GPU-layer offload.
    pub async fn ensure_running(
        &self,
        client: &reqwest::Client,
        model_id: &str,
        total_vram_mb: u64,
        idle_timeout_secs: u64,
        progress: &(dyn Fn(&str, u64, u64, bool) + Sync),
    ) -> Result<u16, AppError> {
        let model_path = self
            .installed_variant(model_id)
            .ok_or_else(|| AppError::LlmError("Model not installed".into()))?;

        let backend = server::default_backend();
        self.server.ensure_binary(client, backend, progress).await?;

        // Decide GPU offload. The model must (a) physically fit in the card and
        // (b) fit in the VRAM that is *currently free*. Gating on free (not just
        // total) VRAM is what keeps a later image generation fast: loading the
        // LLM fully onto a GPU that ComfyUI's diffusion model already occupies
        // makes WDDM demote those weights into shared system memory, and it is
        // never migrated back — so the next generation samples from RAM and
        // crawls. When the GPU is busy we fall back to CPU inference: a slower
        // enhance, but generation stays fast.
        let variant_vram = catalog::entry(model_id)
            .and_then(|e| {
                e.variants
                    .into_iter()
                    .find(|v| self.model_file_path(model_id, &v.file).exists())
            })
            .map(|v| v.vram_mb)
            .unwrap_or(u64::MAX);
        // Free VRAM is NVIDIA-only (nvidia-smi). When unknown (AMD/Intel/Apple,
        // or no GPU) fall back to the total-VRAM decision so those hosts are not
        // needlessly forced onto CPU.
        let free_vram_mb =
            tokio::task::spawn_blocking(crate::comfyui::gpu_manager::detect_free_vram_mb)
                .await
                .ok()
                .flatten();
        let n_gpu_layers = if total_vram_mb < variant_vram {
            0
        } else {
            match free_vram_mb {
                Some(free) if free < variant_vram => {
                    log::info!(
                        "[prompt-assistant] GPU busy ({free} MB free < {variant_vram} MB needed); \
                         loading LLM on CPU to avoid evicting ComfyUI's model"
                    );
                    0
                }
                _ => 999,
            }
        };

        let port = self
            .server
            .ensure_running(client, &model_path, model_id, n_gpu_layers)
            .await?;
        server::start_idle_watchdog(self.server.clone(), idle_timeout_secs);
        Ok(port)
    }
}

/// Match a catalog variant against a frontend variant key.
/// Keys: "gguf:<QUANT>" (e.g. "gguf:Q4_K_M").
fn variant_matches(v: &catalog::LlmVariant, key: &str) -> bool {
    match key.strip_prefix("gguf:") {
        Some(q) => v.quant.as_deref() == Some(q),
        None => false,
    }
}

async fn download_to(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
    label: &str,
    progress: &(dyn Fn(&str, u64, u64, bool) + Sync),
) -> Result<(), AppError> {
    use tokio::io::AsyncWriteExt;
    let resp = tokio::time::timeout(std::time::Duration::from_secs(30), client.get(url).send())
        .await
        .map_err(|_| AppError::LlmError("Download timed out while connecting".into()))?
        .map_err(|e| AppError::LlmError(format!("Download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::LlmError(format!(
            "Download returned {}",
            resp.status()
        )));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(dest).await?;
    progress(label, 0, total, false);
    // Stream to disk; on any error remove the partial file so a retry starts clean.
    let stream_result: Result<u64, AppError> = async {
        let mut downloaded = 0u64;
        let mut last_emit = 0u64;
        let mut resp = resp;
        // A stalled connection (no bytes for 60s) errors out rather than hanging
        // forever — the shared http_client has no read timeout of its own.
        loop {
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(60), resp.chunk())
                .await
                .map_err(|_| AppError::LlmError("Download stalled (no data for 60s)".into()))?
                .map_err(|e| AppError::LlmError(format!("Download read error: {e}")))?;
            let Some(chunk) = chunk else { break };
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if downloaded - last_emit > 1024 * 1024 || downloaded == total {
                last_emit = downloaded;
                progress(label, downloaded, total, false);
            }
        }
        file.flush().await?;
        Ok(downloaded)
    }
    .await;
    match stream_result {
        Ok(downloaded) => {
            progress(label, downloaded, total, true);
            Ok(())
        }
        Err(e) => {
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            Err(e)
        }
    }
}

pub use catalog::{LlmCatalogEntry, LlmVariant};
pub use hardware::{LlmGpu, LlmHardware};
