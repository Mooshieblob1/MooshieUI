pub mod catalog;
pub mod companion;
pub mod describe;
pub mod grounding;
pub mod hardware;
pub mod history;
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

/// Which bundled model answers a turn that does not go to an external provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPick {
    /// The prompt assistant model the owner set up.
    Assistant,
    /// A model that can see the turn's image ([`PromptAssistant::vision_model`]).
    Vision,
}

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
            .any(|v| self.variant_installed(model_id, v))
    }

    /// Both files of a variant are on disk: the weights, and the projector
    /// when the variant has one (a half-finished download has only one).
    fn variant_installed(&self, model_id: &str, v: &LlmVariant) -> bool {
        self.model_file_path(model_id, &v.file).exists()
            && v.mmproj
                .as_ref()
                .is_none_or(|m| self.model_file_path(model_id, m).exists())
    }

    /// Catalog model ids that are installed.
    pub fn installed_models(&self) -> Vec<String> {
        catalog::catalog()
            .into_iter()
            .filter(|e| self.is_model_installed(&e.id))
            .map(|e| e.id)
            .collect()
    }

    /// An installed bundled model that can answer when the external provider
    /// cannot (see `llm_fallback_local`), or `None` when there is none.
    pub fn fallback_model(&self, preferred: Option<&str>) -> Option<String> {
        fallback_model_among(&self.installed_models(), preferred)
    }

    /// The first installed model that can serve as the prompt assistant, for
    /// installs whose config names none. A captioner only describes images.
    pub fn default_assistant_model(&self) -> Option<String> {
        self.installed_models()
            .into_iter()
            .find(|id| catalog::entry(id).is_some_and(|e| e.purpose != "captioner"))
    }

    /// The bundled model that stands in for an unavailable external provider
    /// (`llm_fallback_local`), chosen for what the turn needs.
    pub fn stand_in(&self, configured: Option<&str>, pick: LocalPick) -> Option<String> {
        match pick {
            LocalPick::Assistant => self.fallback_model(configured),
            LocalPick::Vision => self.vision_model(configured),
        }
    }

    /// An installed model that can describe an image, or `None`. A captioner
    /// is preferred, since describing images is all it was trained for; then
    /// `preferred` (the prompt assistant model) when it sees images; then any
    /// other installed model that does.
    pub fn vision_model(&self, preferred: Option<&str>) -> Option<String> {
        vision_model_among(&self.installed_models(), preferred)
    }

    /// The installed variant of a model id: its weights, and its projector
    /// when it has one.
    fn installed_variant(&self, model_id: &str) -> Option<(PathBuf, Option<PathBuf>)> {
        let entry = catalog::entry(model_id)?;
        entry
            .variants
            .iter()
            .find(|v| self.variant_installed(model_id, v))
            .map(|v| {
                (
                    self.model_file_path(model_id, &v.file),
                    v.mmproj.as_ref().map(|m| self.model_file_path(model_id, m)),
                )
            })
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

        // The projector first: it is the small file, so a vision download that
        // fails early fails before the long one, and the weights finishing
        // last keeps "both files present" meaning "download complete".
        let files = variant.mmproj.iter().chain(std::iter::once(&variant.file));
        for file in files {
            let dest = self.model_file_path(model_id, file);
            if dest.exists() {
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // HuggingFace resolve URL (mirrors interrogator's HF_BASE_URL pattern).
            let url = format!(
                "https://huggingface.co/{}/resolve/main/{}",
                variant.repo, file
            );
            download_to(client, &url, &dest, file, progress).await?;
        }
        Ok(())
    }

    /// Delete all installed files for a model id.
    ///
    /// Only catalog ids are accepted: the id is joined onto the models dir and
    /// the result removed recursively, so `../..` or an absolute path would
    /// delete arbitrary directories.
    pub fn delete_model(&self, model_id: &str) -> Result<(), AppError> {
        if catalog::entry(model_id).is_none() {
            return Err(AppError::LlmError("Unknown model id".into()));
        }
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
        let (model_path, mmproj_path) = self
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
                    .find(|v| self.variant_installed(model_id, v))
            })
            .map(|v| v.vram_mb)
            .unwrap_or(u64::MAX)
            // The catalog sizes were measured at a 4096 context; the server
            // now runs with a larger one for persistent enhancer sessions.
            .saturating_add(catalog::context_overhead_mb(
                model_id,
                server::LOCAL_CONTEXT_TOKENS,
            ));
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
            .ensure_running(
                client,
                &model_path,
                mmproj_path.as_deref(),
                model_id,
                n_gpu_layers,
            )
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

/// The configured local model when it writes natural language, else the first
/// installed one that does. Tag upsamplers are never picked: the request was
/// written for an external chat model and asks for prose they cannot write.
fn fallback_model_among(installed: &[String], preferred: Option<&str>) -> Option<String> {
    let writes_prose =
        |id: &str| catalog::entry(id).is_some_and(|e| e.purpose == "natural_language");
    preferred
        .filter(|p| installed.iter().any(|i| i == p) && writes_prose(p))
        .map(str::to_string)
        .or_else(|| installed.iter().find(|i| writes_prose(i)).cloned())
}

/// The images a bundled model is sent: all of them when it was loaded with a
/// projector, none when it is text-only (which answers from the text alone).
pub fn images_for<'a>(
    model_id: &str,
    images: &'a [vision::VisionImage],
) -> &'a [vision::VisionImage] {
    if images.is_empty() || catalog::entry(model_id).is_some_and(|e| e.has_vision()) {
        return images;
    }
    log::info!(
        "[prompt-assistant] the bundled model {model_id} has no vision; \
         answering from the prompt text alone"
    );
    &[]
}

/// See [`PromptAssistant::vision_model`].
fn vision_model_among(installed: &[String], preferred: Option<&str>) -> Option<String> {
    let entries: Vec<_> = installed
        .iter()
        .filter_map(|id| catalog::entry(id))
        .filter(|e| e.has_vision())
        .collect();
    entries
        .iter()
        .find(|e| e.purpose == "captioner")
        .or_else(|| entries.iter().find(|e| Some(e.id.as_str()) == preferred))
        .or_else(|| entries.first())
        .map(|e| e.id.clone())
}

#[cfg(test)]
mod tests {
    use super::PromptAssistant;

    #[test]
    fn delete_model_rejects_ids_outside_the_catalog() {
        // Paths that don't exist, so a missing check fails the test (Ok) without
        // deleting anything.
        let assistant = PromptAssistant::new();
        for id in [
            "../mooshie-test-missing",
            "/mooshie-test-missing",
            "not-a-catalog-model",
            "",
        ] {
            assert!(assistant.delete_model(id).is_err(), "{id:?}");
        }
    }
}

#[cfg(test)]
mod fallback_model_tests {
    use super::fallback_model_among;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_configured_prose_model_is_preferred() {
        let installed = ids(&["qwen3-4b-instruct", "qwen25-7b-instruct"]);
        assert_eq!(
            fallback_model_among(&installed, Some("qwen25-7b-instruct")).as_deref(),
            Some("qwen25-7b-instruct")
        );
    }

    #[test]
    fn a_tag_upsampler_is_never_the_fallback() {
        let installed = ids(&["dantaggen-l", "qwen3-4b-instruct"]);
        assert_eq!(
            fallback_model_among(&installed, Some("dantaggen-l")).as_deref(),
            Some("qwen3-4b-instruct")
        );
        assert_eq!(fallback_model_among(&ids(&["dantaggen-l"]), None), None);
    }

    #[test]
    fn a_configured_model_that_is_not_installed_is_skipped() {
        let installed = ids(&["qwen3-4b-instruct"]);
        assert_eq!(
            fallback_model_among(&installed, Some("qwen25-7b-instruct")).as_deref(),
            Some("qwen3-4b-instruct")
        );
        assert_eq!(fallback_model_among(&[], Some("qwen3-4b-instruct")), None);
    }
}
