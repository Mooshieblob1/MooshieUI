//! NovelAI as a second generation backend.
//!
//! NovelAI generations reuse the ComfyUI event contract rather than inventing a
//! parallel one: a synthetic `nai-{uuid}` prompt id is emitted through the same
//! `comfyui:progress` / `comfyui:preview` / `comfyui:output_image` /
//! `comfyui:executing` / `comfyui:execution_error` events, so `App.svelte` and
//! the progress store need no NovelAI-specific handling.

pub mod client;
pub mod models;
pub mod params;
pub mod payload;
pub mod prompt_syntax;
pub mod response;

use std::sync::Arc;

#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};

use crate::comfyui::types::GenerationParams;
use crate::error::AppError;
use crate::state::AppState;

pub use client::NovelAiClient;
pub use models::is_novelai_model;
pub use response::{StreamEvent, Subscription};

/// Mint the synthetic prompt id a NovelAI generation reports under.
pub fn new_prompt_id() -> String {
    format!("nai-{}", uuid::Uuid::new_v4())
}

/// NovelAI's largest accepted seed. Seeds are a uint32 in its API, so the
/// app's 63-bit seed space has to be folded down rather than passed through.
pub const MAX_SEED: i64 = u32::MAX as i64;

/// Resolve the app's seed convention (negative means "randomise") into a value
/// NovelAI will accept.
///
/// Folding rather than clamping: a clamp would make every seed above the u32
/// ceiling collapse onto the same image, which silently breaks "same seed, same
/// picture" for anyone who pasted a ComfyUI seed in.
pub fn resolve_seed(seed: i64) -> i64 {
    if seed < 0 {
        (rand::random::<u32>()) as i64
    } else {
        seed % (MAX_SEED + 1)
    }
}

/// Emits generation events to whichever transports this build has.
///
/// Desktop emits to the Tauri window *and* the SSE bus (the desktop binary can
/// also be serving browser clients); the server build has only the bus.
pub struct EventSink {
    state: Arc<AppState>,
    #[cfg(feature = "desktop")]
    app: Option<AppHandle>,
}

impl EventSink {
    pub fn new(state: Arc<AppState>, #[cfg(feature = "desktop")] app: Option<AppHandle>) -> Self {
        Self {
            state,
            #[cfg(feature = "desktop")]
            app,
        }
    }

    pub fn emit(&self, event: &str, payload: serde_json::Value) {
        #[cfg(feature = "desktop")]
        if let Some(app) = &self.app {
            let _ = app.emit(event, payload.clone());
        }
        if let Some(prompt_id) = payload.get("prompt_id").and_then(|v| v.as_str()) {
            crate::comfyui::websocket::cache_temp_event(&self.state, event, prompt_id, &payload);
        }
        self.state.broadcast(event, payload);
    }
}

/// Build the NovelAI request body for a generation.
///
/// Separated from [`run`] so a caller can inspect or price the payload without
/// spending Anlas.
pub fn build_request(params: &GenerationParams) -> Result<serde_json::Value, AppError> {
    let nai = params
        .novelai
        .as_ref()
        .ok_or_else(|| AppError::Other("NovelAI parameters missing from the request".into()))?;

    // The checkpoint field is the backend switch, but a client may also name
    // the model in the NovelAI block. The checkpoint wins.
    let model_id = if models::is_novelai_model(&params.checkpoint) {
        params.checkpoint.as_str()
    } else {
        nai.model.as_str()
    };
    let model = models::find(model_id)
        .ok_or_else(|| AppError::Other(format!("Unknown NovelAI model: {model_id}")))?;

    // Weight syntax is rewritten here rather than in `payload.rs` so that
    // module stays a pure description of NovelAI's request shape, and so the
    // rewrite covers character prompts too (they reach `payload::build`
    // through `nai`, not through `PayloadInput`).
    let nai = with_novelai_prompt_syntax(nai);

    let input = payload::PayloadInput {
        positive_prompt: prompt_syntax::to_novelai(&params.positive_prompt),
        negative_prompt: prompt_syntax::to_novelai(&params.negative_prompt),
        width: params.width,
        height: params.height,
        steps: params.steps,
        cfg: params.cfg,
        seed: params.seed,
        // From the NovelAI block, not `params.sampler_name`: that one still
        // names the ComfyUI sampler used by the free local post-process.
        sampler: if nai.sampler.trim().is_empty() {
            "k_euler_ancestral".to_string()
        } else {
            nai.sampler.clone()
        },
        n_samples: params.batch_size,
        input_image: params.input_image.clone(),
        mask_image: params.mask_image.clone(),
    };

    payload::build(&input, &nai, model).map_err(AppError::Other)
}

/// Copy the NovelAI block with every character prompt rewritten into NovelAI
/// weight syntax.
///
/// Character prompts come from the same textareas as the main prompt, so they
/// carry the same ComfyUI-syntax weights and need the same treatment.
fn with_novelai_prompt_syntax(nai: &params::NovelAiParams) -> params::NovelAiParams {
    let mut out = nai.clone();
    for character in &mut out.characters {
        character.prompt = prompt_syntax::to_novelai(&character.prompt);
        character.negative_prompt = prompt_syntax::to_novelai(&character.negative_prompt);
    }
    out
}

/// What a finished [`run`] leaves behind for its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The generation is over. The caller owns queue cleanup.
    Completed,
    /// A free local ComfyUI post-process is now in flight under the *same*
    /// prompt id. The ComfyUI websocket will emit the remaining progress and
    /// the terminal `executing { node: null }`, and finishes the queue entry
    /// itself, so the caller must leave the queue alone.
    HandedOff,
}

/// Run a NovelAI generation to completion, emitting progress as it goes.
///
/// Errors are emitted as `comfyui:execution_error` *and* returned, so a caller
/// that spawned this can log while the frontend clears its pending state.
pub async fn run(
    state: Arc<AppState>,
    sink: EventSink,
    prompt_id: String,
    params: GenerationParams,
) -> Result<RunOutcome, AppError> {
    match run_inner(&state, &sink, &prompt_id, &params).await {
        Ok(outcome) => Ok(outcome),
        Err(err) => {
            sink.emit(
                "comfyui:execution_error",
                serde_json::json!({
                    "prompt_id": prompt_id,
                    "error": err.to_string(),
                    "exception_message": err.to_string(),
                    "node_type": "NovelAI",
                }),
            );
            Err(err)
        }
    }
}

async fn run_inner(
    state: &Arc<AppState>,
    sink: &EventSink,
    prompt_id: &str,
    params: &GenerationParams,
) -> Result<RunOutcome, AppError> {
    let body = build_request(params)?;
    let steps = params.steps.max(1);

    let api_key = {
        let config = state.config.read().await;
        config.novelai_api_key.clone().unwrap_or_default()
    };
    let client = NovelAiClient::new(&state.http_client, &api_key)?;

    sink.emit(
        "comfyui:progress",
        serde_json::json!({ "prompt_id": prompt_id, "value": 0, "max": steps, "node": "NovelAI" }),
    );

    // The streaming endpoint costs the same as the batch one and gives the
    // preview frames the UI already knows how to render.
    let images = client
        .generate_stream(&body, |event| {
            if state.prompt_queue.is_cancelled(prompt_id) {
                return;
            }
            match event {
                StreamEvent::Intermediate { image, step, .. } => {
                    if let Some(temp) = crate::temp_images::save(&image, "png") {
                        sink.emit(
                            "comfyui:preview",
                            serde_json::json!({
                                "temp_filename": temp,
                                "format": "png",
                                "prompt_id": prompt_id,
                            }),
                        );
                    }
                    sink.emit(
                        "comfyui:progress",
                        serde_json::json!({
                            "prompt_id": prompt_id,
                            "value": (step + 1).min(steps),
                            "max": steps,
                            "node": "NovelAI",
                        }),
                    );
                }
                StreamEvent::Final { .. } | StreamEvent::Error { .. } => {}
            }
        })
        .await?;

    if state.prompt_queue.is_cancelled(prompt_id) {
        state.prompt_queue.cleanup_alias(prompt_id);
        return Ok(RunOutcome::Completed);
    }

    // The free local pass. NovelAI has already been paid for these pixels, so
    // any failure here falls back to delivering the image untouched rather
    // than surfacing an error the user would read as "my Anlas bought nothing".
    if crate::templates::upscale_standalone::is_requested(params) {
        if let [png] = images.as_slice() {
            match run_local_post_process(state, sink, prompt_id, params, png).await {
                Ok(()) => return Ok(RunOutcome::HandedOff),
                Err(err) => log::warn!(
                    "NovelAI {prompt_id}: local post-process could not start ({err});                      delivering the unmodified image"
                ),
            }
        } else {
            // One ComfyUI prompt maps to one alias and one GPU worker, so a
            // multi-image NovelAI batch has no safe single-prompt post-process
            // to hand off to. Delivering the batch untouched beats leaking a
            // worker or ending the frontend's progress after the first image.
            log::warn!(
                "NovelAI {prompt_id}: local post-process skipped, it runs on                  single-image generations only ({} returned)",
                images.len()
            );
        }
    }

    for image in &images {
        deliver_image(sink, prompt_id, image).await;
    }

    // `node: null` is the frontend's completion signal.
    sink.emit(
        "comfyui:executing",
        serde_json::json!({ "prompt_id": prompt_id, "node": serde_json::Value::Null }),
    );
    state.prompt_queue.cleanup_alias(prompt_id);
    Ok(RunOutcome::Completed)
}

/// Hand the finished NovelAI image to the local ComfyUI upscale/face-fix chain.
///
/// On success the generation continues under the *same* prompt id: the
/// ComfyUI prompt is alias-bound to it, so the websocket re-emits its progress,
/// previews, output image and terminal `executing { node: null }` against the
/// id the frontend is already tracking.
///
/// Returning `Err` means nothing was submitted and the caller should deliver
/// the NovelAI image as-is.
async fn run_local_post_process(
    state: &Arc<AppState>,
    sink: &EventSink,
    prompt_id: &str,
    params: &GenerationParams,
    png: &[u8],
) -> Result<(), AppError> {
    // Paid work first. If the user asked to keep the pre-upscale image, it is
    // delivered before the local pass is even submitted, so a crash, a GPU
    // OOM or a closed app between here and ComfyUI's output still leaves the
    // image they paid for in the gallery.
    if params.save_pre_upscale_image {
        deliver_image(sink, prompt_id, png).await;
    }

    let filename = format!("{prompt_id}.png");
    let upload = state
        .upload_image_from_bytes(png.to_vec(), filename)
        .await?;

    let workflow = crate::templates::upscale_standalone::build(params, &upload.name, params.seed)
        .ok_or_else(|| AppError::Other("Local post-process is not applicable".into()))?;

    let timeout = std::time::Duration::from_secs(300);
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, timeout)
        .await?;

    // From here the ComfyUI websocket owns the prompt: it resolves the alias
    // back to `prompt_id`, finishes the queue entry and releases the worker.
    if state
        .prompt_queue
        .bind_alias(prompt_id, &response.prompt_id)
    {
        // Completion or error beat the bind. The queue entry is already gone,
        // so the worker has to be released here.
        state
            .gpu_manager
            .mark_worker_error_then_idle(worker_id)
            .await;
        let alias_state = Arc::clone(state);
        let alias_pid = prompt_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            alias_state.prompt_queue.cleanup_alias(&alias_pid);
        });
    } else {
        state.prompt_queue.set_worker(prompt_id, worker_id);
    }
    state.broadcast_queue_positions();

    Ok(())
}

/// Feed NovelAI PNG bytes through the existing output-image pipeline.
///
/// `process_output_image` expects a `MooshieSaveImage` binary frame: a 4-byte
/// event id followed by a 4-byte format tag. Tag 1 means "8-bit PNG follows",
/// which is exactly what NovelAI returns, so the frame is synthesised rather
/// than duplicating the temp-file, JXL and SSE-payload logic.
async fn deliver_image(sink: &EventSink, prompt_id: &str, png: &[u8]) {
    let mut frame = Vec::with_capacity(png.len() + 8);
    frame.extend_from_slice(&100u32.to_be_bytes()); // MOOSHIE_OUTPUT_IMAGE
    frame.extend_from_slice(&1u32.to_be_bytes()); // format tag: 8-bit PNG
    frame.extend_from_slice(png);

    match crate::comfyui::websocket::process_output_image(&frame).await {
        Some(img) => {
            let payload = crate::comfyui::websocket::build_sse_payload(&img, prompt_id);
            sink.emit("comfyui:output_image", payload);
        }
        None => log::warn!("NovelAI image failed to process for prompt {prompt_id}"),
    }
}

/// Fetch the subscription record backing the Anlas and Opus readouts.
pub async fn fetch_subscription(state: &Arc<AppState>) -> Result<Subscription, AppError> {
    let api_key = {
        let config = state.config.read().await;
        config.novelai_api_key.clone().unwrap_or_default()
    };
    let client = NovelAiClient::new(&state.http_client, &api_key)?;
    let mut sub = client.subscription().await?;
    sub.derive_opus_allowance();

    // Log unrecognised keys so a field NovelAI adds later can be wired up from
    // an observed response rather than guessed at.
    if !sub.extra.is_empty() {
        log::debug!(
            "NovelAI subscription carried unmapped fields: {:?}",
            sub.extra.keys().collect::<Vec<_>>()
        );
    }
    Ok(sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_ids_are_namespaced_and_unique() {
        let a = new_prompt_id();
        let b = new_prompt_id();
        assert!(a.starts_with("nai-"));
        assert_ne!(a, b);
    }

    #[test]
    fn a_negative_seed_randomises_within_range() {
        for _ in 0..64 {
            let seed = resolve_seed(-1);
            assert!((0..=MAX_SEED).contains(&seed), "out of range: {seed}");
        }
    }

    #[test]
    fn an_in_range_seed_is_untouched() {
        assert_eq!(resolve_seed(0), 0);
        assert_eq!(resolve_seed(42), 42);
        assert_eq!(resolve_seed(MAX_SEED), MAX_SEED);
    }

    #[test]
    fn an_oversized_seed_folds_instead_of_collapsing() {
        // Two distinct 63-bit seeds must not land on the same NovelAI seed just
        // because both exceed the ceiling, which is what a clamp would do.
        let a = resolve_seed(MAX_SEED + 1);
        let b = resolve_seed(MAX_SEED + 2);
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        assert_ne!(a, b);
    }
}
