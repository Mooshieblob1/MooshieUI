use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::error::AppError;
use crate::prompt_assistant::grounding::{self, GenMode};
use crate::prompt_assistant::{catalog, hardware, LlmCatalogEntry, LlmHardware};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct LlmStatus {
    pub installed_models: Vec<String>,
    pub active_model: Option<String>,
    pub server_running: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct PromptAssistantOpts {
    /// "short" | "medium" | "detailed"
    pub length: Option<String>,
    #[serde(default)]
    pub include_artists: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    filename: String,
    downloaded: u64,
    total: u64,
    done: bool,
}

#[tauri::command]
pub async fn detect_llm_hardware() -> Result<LlmHardware, AppError> {
    tokio::task::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| AppError::LlmError(format!("hardware detect failed: {e}")))
}

#[tauri::command]
pub async fn list_llm_catalog() -> Result<Vec<LlmCatalogEntry>, AppError> {
    Ok(catalog::catalog())
}

#[tauri::command]
pub async fn llm_status(state: State<'_, Arc<AppState>>) -> Result<LlmStatus, AppError> {
    let pa = &state.prompt_assistant;
    Ok(LlmStatus {
        installed_models: pa.installed_models(),
        active_model: pa.server.active_model(),
        server_running: pa.server.is_running(),
    })
}

#[tauri::command]
pub async fn download_llm_model(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
    variant: String,
) -> Result<(), AppError> {
    let pa = state.prompt_assistant.clone();
    let app2 = app.clone();
    let progress = move |filename: &str, downloaded: u64, total: u64, done: bool| {
        app2.emit(
            "llm:download_progress",
            DownloadProgress {
                filename: filename.to_string(),
                downloaded,
                total,
                done,
            },
        )
        .ok();
    };
    pa.download_model(&state.http_client, &id, &variant, &progress)
        .await?;
    // Persist selected model id + mark setup done.
    {
        let mut cfg = state.config.write().await;
        cfg.prompt_assistant_model_id = Some(id.clone());
        cfg.prompt_assistant_setup_done = true;
        let _ = crate::config::save_config(&cfg);
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_llm_model(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    state.prompt_assistant.delete_model(&id)?;
    Ok(())
}

#[tauri::command]
pub async fn unload_llm(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    state.prompt_assistant.server.unload().await;
    Ok(())
}

/// Shared core for enhance/compose: guard, ensure server, ground, generate, repair.
async fn run_generation(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
    input: &str,
    family: &str,
    mode: GenMode,
    opts: &PromptAssistantOpts,
) -> Result<String, AppError> {
    // Generation guard: do not contend with an active ComfyUI generation.
    if !state.prompt_queue.is_empty() {
        return Err(AppError::LlmError(
            "prompt_assistant.busy_generation".into(),
        ));
    }

    let (model_id, idle_secs) = {
        let cfg = state.config.read().await;
        (
            cfg.prompt_assistant_model_id.clone(),
            cfg.prompt_assistant_idle_timeout_secs,
        )
    };
    let model_id =
        model_id.ok_or_else(|| AppError::LlmError("prompt_assistant.no_model".into()))?;

    let hw = tokio::task::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| AppError::LlmError(e.to_string()))?;

    app.emit("llm:stage", "loading_model").ok();
    let pa = state.prompt_assistant.clone();
    let app2 = app.clone();
    let progress = move |filename: &str, downloaded: u64, total: u64, done: bool| {
        app2.emit(
            "llm:download_progress",
            DownloadProgress {
                filename: filename.to_string(),
                downloaded,
                total,
                done,
            },
        )
        .ok();
    };
    let port = pa
        .ensure_running(
            &state.http_client,
            &model_id,
            hw.total_vram_mb,
            idle_secs,
            &progress,
        )
        .await?;

    app.emit("llm:stage", "generating").ok();
    // A purpose-built tag upsampler is always tag-only regardless of family.
    let purpose = catalog::entry(&model_id)
        .map(|e| e.purpose)
        .unwrap_or_else(|| "natural_language".to_string());
    let tag_only = grounding::is_tag_only(&purpose, family);
    let candidates = grounding::retrieve_candidates(input, 40);
    let system = grounding::system_prompt(tag_only, mode, &candidates);
    let max_tokens = match opts.length.as_deref() {
        Some("short") => 96,
        Some("detailed") => 384,
        _ => 192,
    };
    let raw = pa
        .server
        .chat(&state.http_client, port, &system, input, max_tokens)
        .await?;
    let cleaned = grounding::repair(&raw, tag_only);
    // Enhance is additive: keep every user tag (named characters included) and don't
    // let the model switch a pinned attribute (a 1boy on a 1girl prompt, red hair on a
    // "blue hair" prompt). No-op for Compose.
    let cleaned = grounding::reconcile_enhance(input, &cleaned, mode);
    Ok(cleaned)
}

#[tauri::command]
pub async fn enhance_prompt(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    prompt: String,
    family: String,
    opts: Option<PromptAssistantOpts>,
) -> Result<String, AppError> {
    let opts = opts.unwrap_or_default();
    run_generation(&app, &state, &prompt, &family, GenMode::Enhance, &opts).await
}

#[tauri::command]
pub async fn compose_prompt(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    description: String,
    family: String,
    opts: Option<PromptAssistantOpts>,
) -> Result<String, AppError> {
    let opts = opts.unwrap_or_default();
    run_generation(&app, &state, &description, &family, GenMode::Compose, &opts).await
}
