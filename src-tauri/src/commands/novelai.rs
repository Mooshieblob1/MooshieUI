//! Tauri commands for the NovelAI generation backend.
//!
//! Desktop-only by module gate (`commands/mod.rs`), so `tauri::*` is free to
//! use here. The browser-mode equivalents live in `webserver.rs` and call the
//! same `crate::novelai` entry points.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::comfyui::types::GenerationParams;
use crate::error::AppError;
use crate::novelai::{self, response::Subscription};
use crate::state::AppState;

/// Mirrors `commands::workflow::GenerateResponse`: the resolved seed is a
/// string because 63-bit values exceed JavaScript's safe-integer range.
#[derive(serde::Serialize)]
pub struct NovelAiGenerateResponse {
    pub prompt_id: String,
    #[serde(serialize_with = "crate::comfyui::types::seed_string::serialize")]
    pub seed: i64,
}

/// Director Tools have no seed to report, so the response carries only the id
/// the events will arrive under.
#[derive(serde::Serialize)]
pub struct NovelAiAugmentResponse {
    pub prompt_id: String,
}

/// Start a NovelAI generation and return its synthetic prompt id immediately.
///
/// The request itself runs in a spawned task so the caller's
/// `progress.pendingPrompts` is populated before the first `comfyui:progress`
/// event lands, exactly like the ComfyUI `generate` command.
#[tauri::command]
pub async fn novelai_generate(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    params: GenerationParams,
) -> Result<NovelAiGenerateResponse, AppError> {
    // Same 5-minute sweep the ComfyUI path does: NovelAI previews land in the
    // same temp dir and would otherwise accumulate across a long session.
    crate::temp_images::cleanup(300);

    let mut params = params;
    params.seed = novelai::resolve_seed(params.seed);

    // Built before spawning so a bad request (no key, unknown model, missing
    // NovelAI block) surfaces as a command error the caller can show inline,
    // rather than as an async execution_error against a prompt id that the
    // frontend has already committed to.
    novelai::build_request(&params)?;

    let prompt_id = novelai::new_prompt_id();
    let seed = params.seed;

    // NovelAI runs off-box, so it takes no local GPU slot, but it is still
    // inserted into the queue so cancellation and the queue readout see it.
    state.prompt_queue.insert(&prompt_id, None);
    state.broadcast_queue_positions();

    let bg_state = Arc::clone(state.inner());
    let bg_prompt_id = prompt_id.clone();
    tokio::spawn(async move {
        let sink = novelai::EventSink::new(Arc::clone(&bg_state), Some(app));
        let result = novelai::run(Arc::clone(&bg_state), sink, bg_prompt_id.clone(), params).await;
        if let Err(err) = &result {
            log::error!("NovelAI generation {bg_prompt_id} failed: {err}");
        }
        // A handed-off generation is still running as a local ComfyUI prompt
        // under this same id; the websocket finishes and removes it.
        if !matches!(result, Ok(novelai::RunOutcome::HandedOff)) {
            bg_state.prompt_queue.cancel_and_remove(&bg_prompt_id);
            bg_state.broadcast_queue_positions();
        }
    });

    Ok(NovelAiGenerateResponse { prompt_id, seed })
}

/// Run a Director Tool over an image and return its synthetic prompt id.
///
/// Structured exactly like `novelai_generate`: validate up front so a bad tool
/// name or an unreadable image is a command error, then spawn so the caller has
/// a prompt id before the first event arrives. The results come back as
/// `comfyui:output_image` events, which is why nothing is returned here.
#[tauri::command]
pub async fn novelai_augment(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    params: novelai::augment::AugmentParams,
) -> Result<NovelAiAugmentResponse, AppError> {
    crate::temp_images::cleanup(300);

    let prepared = novelai::augment::PreparedAugment::prepare(params)?;
    let prompt_id = novelai::new_prompt_id();

    state.prompt_queue.insert(&prompt_id, None);
    state.broadcast_queue_positions();

    let bg_state = Arc::clone(state.inner());
    let bg_prompt_id = prompt_id.clone();
    tokio::spawn(async move {
        let sink = novelai::EventSink::new(Arc::clone(&bg_state), Some(app));
        let result =
            novelai::augment::run(Arc::clone(&bg_state), sink, bg_prompt_id.clone(), prepared)
                .await;
        if let Err(err) = &result {
            log::error!("NovelAI Director Tools {bg_prompt_id} failed: {err}");
        }
        bg_state.prompt_queue.cancel_and_remove(&bg_prompt_id);
        bg_state.broadcast_queue_positions();
    });

    Ok(NovelAiAugmentResponse { prompt_id })
}

/// Anlas balance and Opus status for the configured key.
#[tauri::command]
pub async fn novelai_subscription(
    state: State<'_, Arc<AppState>>,
) -> Result<Subscription, AppError> {
    novelai::fetch_subscription(state.inner()).await
}

/// Store the NovelAI API key. An empty string is an explicit clear.
///
/// A dedicated command rather than a field on `update_config`, because
/// `preserve_secrets` treats a blank incoming key as a stale echo from a client
/// that only ever saw the redacted config.
#[tauri::command]
pub async fn set_novelai_api_key(
    state: State<'_, Arc<AppState>>,
    api_key: String,
) -> Result<bool, AppError> {
    let trimmed = api_key.trim().to_string();
    let configured = !trimmed.is_empty();

    let snapshot = {
        let mut config = state.config.write().await;
        config.novelai_api_key = if configured { Some(trimmed) } else { None };
        config.clone()
    };
    crate::config::save_config(&snapshot).map_err(AppError::Other)?;

    Ok(configured)
}
