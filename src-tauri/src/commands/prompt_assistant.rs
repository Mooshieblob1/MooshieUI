use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::error::AppError;
use crate::prompt_assistant::grounding::{self, GenMode};
use crate::prompt_assistant::{catalog, hardware, oauth, providers, LlmCatalogEntry, LlmHardware};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct LlmStatus {
    pub installed_models: Vec<String>,
    pub active_model: Option<String>,
    pub server_running: bool,
    /// True when an external OpenAI-compatible endpoint is configured — the
    /// assistant is usable even with no local model installed.
    pub external_enabled: bool,
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
    let external_enabled = state.config.read().await.llm_external_enabled;
    Ok(LlmStatus {
        installed_models: pa.installed_models(),
        active_model: pa.server.active_model(),
        server_running: pa.server.is_running(),
        external_enabled,
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

/// Route one system+user turn to whichever backend is configured: the external
/// provider when it is enabled, otherwise the bundled local llama-server.
///
/// Every caller that just needs an answer goes through here, so a feature built
/// on the external providers keeps working for users who only have a local
/// model installed.
///
/// `image` only reaches the external path. The bundled llama-server runs
/// text-only models, so it drops the image and answers from the prompt alone
/// rather than failing.
async fn chat_any(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
    system: &str,
    user: &str,
    max_tokens: u32,
    image: Option<&crate::prompt_assistant::vision::VisionImage>,
) -> Result<String, AppError> {
    let (model_id, idle_secs, ext_enabled, provider, ext_base, ext_key, ext_model) = {
        let cfg = state.config.read().await;
        (
            cfg.prompt_assistant_model_id.clone(),
            cfg.prompt_assistant_idle_timeout_secs,
            cfg.llm_external_enabled,
            cfg.llm_provider.clone(),
            cfg.llm_external_base_url.clone(),
            cfg.llm_external_api_key.clone(),
            cfg.llm_external_model.clone(),
        )
    };

    if ext_enabled {
        crate::prompt_assistant::local_llm::wake_local_server(&state.http_client, &ext_base).await;
        app.emit("llm:stage", "generating").ok();
        return crate::prompt_assistant::server::chat_provider(
            &state.http_client,
            &provider,
            &ext_base,
            &ext_key,
            &ext_model,
            system,
            user,
            max_tokens,
            image,
        )
        .await;
    }

    if image.is_some() {
        log::info!(
            "[prompt-assistant] the bundled local model has no vision; \
             answering from the prompt text alone"
        );
    }

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
    pa.server
        .chat(&state.http_client, port, system, user, max_tokens)
        .await
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
    // No active-generation guard: contention with an in-flight ComfyUI generation
    // is handled by the free-VRAM check in `ensure_running` (it loads the LLM on
    // CPU when the GPU is busy rather than evicting the diffusion model), so there
    // is no need to hard-block enhance/compose while a generation is queued. In
    // server mode `prompt_queue` is shared across users, where blocking meant one
    // person's generation locked the feature for everyone.
    let (model_id, ext_enabled) = {
        let cfg = state.config.read().await;
        (
            cfg.prompt_assistant_model_id.clone(),
            cfg.llm_external_enabled,
        )
    };

    // Resolve the model purpose (drives tag-only grounding). External endpoints are
    // general-purpose chat models, so no local catalog entry / model id is required.
    let purpose = if ext_enabled {
        "natural_language".to_string()
    } else {
        let mid = model_id
            .as_deref()
            .ok_or_else(|| AppError::LlmError("prompt_assistant.no_model".into()))?;
        catalog::entry(mid)
            .map(|e| e.purpose)
            .unwrap_or_else(|| "natural_language".to_string())
    };
    let tag_only = grounding::is_tag_only(&purpose, family);
    let candidates = grounding::retrieve_candidates(input, 40);
    let system = grounding::system_prompt(tag_only, mode, &candidates, opts.include_artists);
    let max_tokens = match opts.length.as_deref() {
        Some("short") => 96,
        Some("detailed") => 384,
        _ => 192,
    };

    let raw = chat_any(app, state, &system, input, max_tokens, None).await?;
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

/// Read the current external-provider settings without exposing the key.
#[tauri::command]
pub async fn get_llm_provider(
    state: State<'_, Arc<AppState>>,
) -> Result<providers::LlmProviderState, AppError> {
    Ok(providers::read_state(&state.config).await)
}

/// Switch the external LLM provider, prefilling its API root and model.
#[tauri::command]
pub async fn set_llm_provider(
    state: State<'_, Arc<AppState>>,
    provider: String,
) -> Result<providers::LlmProviderState, AppError> {
    providers::select(&state.config, &provider).await
}

/// Store the API key for the current provider. An empty key clears it.
#[tauri::command]
pub async fn set_llm_api_key(
    state: State<'_, Arc<AppState>>,
    api_key: String,
) -> Result<providers::LlmProviderState, AppError> {
    providers::store_key(&state.config, &api_key).await
}

/// Set the model to use with the current provider.
#[tauri::command]
pub async fn set_llm_model(
    state: State<'_, Arc<AppState>>,
    model: String,
) -> Result<providers::LlmProviderState, AppError> {
    providers::set_model(&state.config, &model).await
}

/// Point a self-hosted provider at its OpenAI-compatible API root.
#[tauri::command]
pub async fn set_llm_base_url(
    state: State<'_, Arc<AppState>>,
    base_url: String,
) -> Result<providers::LlmProviderState, AppError> {
    providers::set_base_url(&state.config, &base_url).await
}

/// Ask the provider which models the stored key can actually use.
#[tauri::command]
pub async fn list_external_llm_models(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, AppError> {
    providers::list_available_models(&state.http_client, &state.config).await
}

/// Sign in to OpenRouter and store the API key the flow issues.
///
/// Desktop only: the redirect lands on a loopback port owned by this process,
/// which is unreachable from a browser on another machine. Browser mode offers
/// API-key entry instead.
#[tauri::command]
pub async fn connect_llm_oauth(
    state: State<'_, Arc<AppState>>,
    provider: String,
) -> Result<providers::LlmProviderState, AppError> {
    let known = providers::provider(&provider)
        .ok_or_else(|| AppError::LlmError(format!("Unknown LLM provider: {provider}")))?;
    if !known.oauth {
        return Err(AppError::LlmError(format!(
            "{provider} has no sign-in flow; paste an API key instead."
        )));
    }

    let key = oauth::connect_openrouter(&state.http_client).await?;
    providers::store_oauth_key(&state.config, &provider, key).await
}

/// Run one system+user turn through the configured backend and return it raw.
///
/// Deliberately skips the danbooru grounding that `enhance_prompt` applies: the
/// callers here (the H3 rewrite) want prose in a structured format, and the
/// tag-repair pass would mangle it. The system prompt comes from the client
/// because it belongs to the feature asking for the rewrite, not to the
/// assistant; that is no more privileged than `enhance_prompt`, which already
/// spends the same key on client-supplied text, and `max_tokens` is clamped so
/// a bad caller cannot run up an unbounded bill.
///
/// `image_filename` names an image already uploaded to ComfyUI's input folder
/// (a video first frame, say). The client passes the filename rather than the
/// bytes: the bytes are the one thing it does not keep - an upload leaves only a
/// name behind - and routing multiple megabytes of base64 back through IPC to
/// send it straight out again would be pointless. Anything that goes wrong while
/// fetching or encoding it degrades to a text-only turn, because a rewrite
/// written from the prompt alone beats an error dialog.
#[tauri::command]
pub async fn call_external_llm(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    system: String,
    prompt: String,
    max_tokens: Option<u32>,
    image_filename: Option<String>,
) -> Result<String, AppError> {
    let max_tokens = max_tokens.unwrap_or(1024).clamp(64, 4096);
    let image = crate::prompt_assistant::vision::load_input_frame(&state, image_filename).await;
    chat_any(&app, &state, &system, &prompt, max_tokens, image.as_ref()).await
}
