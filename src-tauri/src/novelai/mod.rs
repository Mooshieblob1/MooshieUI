//! NovelAI as a second generation backend.
//!
//! NovelAI generations reuse the ComfyUI event contract rather than inventing a
//! parallel one: a synthetic `nai-{uuid}` prompt id is emitted through the same
//! `comfyui:progress` / `comfyui:preview` / `comfyui:output_image` /
//! `comfyui:executing` / `comfyui:execution_error` events, so `App.svelte` and
//! the progress store need no NovelAI-specific handling.

pub mod augment;
pub mod client;
pub mod metadata;
pub mod models;
pub mod params;
pub mod payload;
pub mod prompt_syntax;
pub mod response;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};

use sha2::{Digest, Sha256};

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

/// Pick the model a request runs against.
///
/// The checkpoint field is the backend switch, but a client may also name
/// the model in the NovelAI block. The checkpoint wins.
fn resolve_model_id(params: &GenerationParams, nai: &params::NovelAiParams) -> String {
    if models::is_novelai_model(&params.checkpoint) {
        params.checkpoint.clone()
    } else {
        nai.model.clone()
    }
}

/// Vibe encodings already paid for during this run of the app.
///
/// `/ai/encode-vibe` bills 2 Anlas every time it is handed an image it has
/// not seen, so re-running the same generation must not pay again. The key
/// covers everything the token depends on: the image, how much of it NovelAI
/// is asked to extract, and the model the token is minted for.
static VIBE_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

/// Encodings are a few kilobytes each; this bounds the cache without
/// bothering with eviction order, since a cold key only costs one encode.
const VIBE_CACHE_LIMIT: usize = 64;

fn vibe_cache_key(image: &str, information_extracted: f64, model_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(image.as_bytes());
    format!(
        "{model_id}:{information_extracted:.4}:{:x}",
        hasher.finalize()
    )
}

/// Whether a vibe still owes NovelAI an encode.
///
/// A token is minted for one model at one extraction level, so it stops
/// being usable the moment either changes. A vibe that arrived as a bare
/// token with no image cannot be re-encoded here at all, and is left alone.
fn vibe_needs_encoding(vibe: &params::NovelAiVibe, model_id: &str) -> bool {
    if vibe.image.as_deref().unwrap_or_default().is_empty() {
        return false;
    }
    if vibe.encoding.as_deref().unwrap_or_default().is_empty() {
        return true;
    }
    if vibe.encoded_model.as_deref() != Some(model_id) {
        return true;
    }
    // A token minted before the client tracked these fields has no recorded
    // extraction level, so it is treated as stale and paid for once more.
    vibe.encoded_information_extracted
        .is_none_or(|level| (level - vibe.information_extracted).abs() > f64::EPSILON)
}

/// Turn every reference image whose `.naiv4vibe` token is missing or stale
/// into a fresh one.
///
/// V4 and later do not accept a raw image in `reference_image_multiple`:
/// that is the V3 shape, and sending it earns a bare 500 with no hint of
/// what went wrong. The image has to go through `/ai/encode-vibe` first, so
/// this pass runs before the payload is built.
///
/// Returns `None` when there was nothing to encode, so the caller can keep
/// using the params it already has instead of a clone.
async fn encode_pending_vibes(
    client: &NovelAiClient<'_>,
    params: &GenerationParams,
) -> Result<Option<GenerationParams>, AppError> {
    let Some(nai) = params.novelai.as_ref() else {
        return Ok(None);
    };
    let model_id = resolve_model_id(params, nai);
    // An unknown model is `build_request`'s error to report, with a better
    // message than anything this pass could give.
    if !models::find(&model_id).is_some_and(|model| model.vibe_transfer) {
        return Ok(None);
    }
    let pending: Vec<usize> = nai
        .vibes
        .iter()
        .enumerate()
        .filter(|(_, vibe)| vibe_needs_encoding(vibe, &model_id))
        .map(|(index, _)| index)
        .collect();
    if pending.is_empty() {
        return Ok(None);
    }

    let mut encoded = params.clone();
    let nai = encoded
        .novelai
        .as_mut()
        .expect("novelai block present, checked above");
    for index in pending {
        let image = nai.vibes[index].image.clone().unwrap_or_default();
        let extracted = nai.vibes[index].information_extracted;
        let key = vibe_cache_key(&image, extracted, &model_id);
        let encoding = match vibe_cache_get(&key) {
            Some(hit) => hit,
            None => {
                let fresh = client.encode_vibe(&image, &model_id, extracted).await?;
                vibe_cache_put(key, fresh.clone());
                fresh
            }
        };
        nai.vibes[index].encoding = Some(encoding);
        nai.vibes[index].encoded_model = Some(model_id.clone());
        nai.vibes[index].encoded_information_extracted = Some(extracted);
    }
    Ok(Some(encoded))
}

/// One line per vibe-carrying request, read back off the body that is
/// actually sent.
///
/// Normalising happens on NovelAI side, so nothing about it is observable
/// locally and this is the only confirmation that the flag left the machine.
/// Reading the built body rather than the params also keeps the pre-flight
/// validation build in `commands::novelai` from doubling every line. The
/// tokens themselves are never logged.
fn log_vibe_summary(body: &serde_json::Value) {
    let Some(parameters) = body.get("parameters") else {
        return;
    };
    let Some(refs) = parameters
        .get("reference_image_multiple")
        .and_then(|v| v.as_array())
    else {
        return;
    };
    let null = serde_json::Value::Null;
    log::info!(
        "NovelAI vibe transfer: {} reference(s), strengths {}, normalize {}",
        refs.len(),
        parameters
            .get("reference_strength_multiple")
            .unwrap_or(&null),
        parameters
            .get("normalize_reference_strength_multiple")
            .unwrap_or(&null)
    );
}
/// Hand the freshly minted tokens back to the client that asked for them.
///
/// The client stores them next to the image it sent, so the same vibe costs
/// nothing on the next generation or after a restart. Only clients that own
/// the prompt apply the payload, the way they already filter previews.
fn emit_vibe_encodings(sink: &EventSink, prompt_id: &str, params: &GenerationParams) {
    let Some(nai) = params.novelai.as_ref() else {
        return;
    };
    let vibes: Vec<serde_json::Value> = nai
        .vibes
        .iter()
        .enumerate()
        .filter_map(|(index, vibe)| {
            let encoding = vibe.encoding.as_deref().filter(|s| !s.is_empty())?;
            Some(serde_json::json!({
                "index": index,
                "encoding": encoding,
                "encoded_model": vibe.encoded_model,
                "encoded_information_extracted": vibe.encoded_information_extracted,
            }))
        })
        .collect();
    if vibes.is_empty() {
        return;
    }
    sink.emit(
        "novelai:vibes_encoded",
        serde_json::json!({ "prompt_id": prompt_id, "vibes": vibes }),
    );
}

fn vibe_cache_get(key: &str) -> Option<String> {
    let cache = VIBE_CACHE.get_or_init(Default::default).lock().ok()?;
    cache.get(key).cloned()
}

fn vibe_cache_put(key: String, encoding: String) {
    let Ok(mut cache) = VIBE_CACHE.get_or_init(Default::default).lock() else {
        return;
    };
    if cache.len() >= VIBE_CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(key, encoding);
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

    let model_id = resolve_model_id(params, nai);
    let model = models::find(&model_id)
        .ok_or_else(|| AppError::Other(format!("Unknown NovelAI model: {model_id}")))?;

    // Weight syntax is rewritten here rather than in `payload.rs` so that
    // module stays a pure description of NovelAI's request shape, and so the
    // rewrite covers character prompts too (they reach `payload::build`
    // through `nai`, not through `PayloadInput`).
    let nai = with_novelai_prompt_syntax(nai);

    let input = payload::PayloadInput {
        positive_prompt: prompt_syntax::to_novelai(&params.positive_prompt),
        negative_prompt: prompt_syntax::to_novelai(&params.negative_prompt),
        // NovelAI rejects any dimension that is not a multiple of 64. The UI
        // already snaps, so this is the backstop for a preset or a restored
        // gallery setting that predates that.
        width: snap_dimension(params.width),
        height: snap_dimension(params.height),
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

/// NovelAI's dimension grid. Its own UI steps 1024 -> 1088 -> 1152.
const DIMENSION_STEP: u32 = 64;

/// Round a pixel dimension onto NovelAI's grid, never below one full step.
fn snap_dimension(px: u32) -> u32 {
    let snapped = ((px + DIMENSION_STEP / 2) / DIMENSION_STEP) * DIMENSION_STEP;
    snapped.max(DIMENSION_STEP)
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
    let api_key = {
        let config = state.config.read().await;
        config.novelai_api_key.clone().unwrap_or_default()
    };
    let client = NovelAiClient::new(&state.http_client, &api_key)?;

    // The client has to exist before the payload does: vibe references are
    // encoded over the network, and V4 will not take the raw images.
    let encoded = encode_pending_vibes(&client, params).await?;
    if let Some(encoded) = encoded.as_ref() {
        emit_vibe_encodings(sink, prompt_id, encoded);
    }
    let body = build_request(encoded.as_ref().unwrap_or(params))?;
    log_vibe_summary(&body);
    let steps = params.steps.max(1);

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

    let mut derived = crate::templates::upscale_standalone::build_params(params, &upload.name)
        .ok_or_else(|| AppError::Other("Local post-process is not applicable".into()))?;
    // The local model lives in a folder that does not match what it is (a
    // split-file model in checkpoints/, or a full checkpoint in
    // diffusion_models/), so the path loaders take over and need an absolute
    // path. Anything correctly filed skips this entirely.
    if let Some(category) = derived.model_source_category.clone() {
        let filename = if derived.use_split_model {
            derived.diffusion_model.clone().unwrap_or_default()
        } else {
            derived.checkpoint.clone()
        };
        let resolved = {
            let config = state.config.read().await;
            crate::commands::api::resolve_model_path(
                &config.comfyui_path,
                config.extra_model_paths.as_deref(),
                &category,
                &filename,
            )
        };
        match resolved {
            Some(path) => {
                derived.resolved_model_path = Some(path.to_string_lossy().to_string());
            }
            None => {
                return Err(AppError::Other(format!(
                    "Local post-process model not found: {category}/{filename}"
                )));
            }
        }
    }
    // A split-file model needs all three names. An unresolved companion reaches
    // ComfyUI as `clip_name: ""`, which is rejected during graph validation with
    // no mention of the model that caused it, so refuse it here where the
    // message can say which half is missing.
    if derived.use_split_model {
        let missing: Vec<&str> = [
            ("text encoder", derived.clip_model.as_deref()),
            ("VAE", derived.vae.as_deref()),
        ]
        .into_iter()
        .filter(|(_, name)| name.unwrap_or("").trim().is_empty())
        .map(|(label, _)| label)
        .collect();
        if !missing.is_empty() {
            return Err(AppError::Other(format!(
                "Local post-process model {} is a split-file model and no {} is installed for it; pick a full checkpoint or install the companion file",
                derived.checkpoint,
                missing.join(" or "),
            )));
        }
    }
    // What the pass will actually load. The failure modes here are all "the
    // wrong file name reached a loader", and the graph is derived rather than
    // user-authored, so the names are worth one line in the log.
    log::info!(
        "NovelAI {prompt_id}: local pass model={} split={} clip={:?} clip_type={:?} vae={:?} source_category={:?} path={:?}",
        derived.checkpoint,
        derived.use_split_model,
        derived.clip_model,
        derived.clip_type,
        derived.vae,
        derived.model_source_category,
        derived.resolved_model_path,
    );
    // Every knob the local pass reads comes from the upscale and face-fix
    // panels rather than from the NovelAI panel, so when a slider looks like it
    // did nothing this line is what says whether it reached the graph.
    log::info!(
        "NovelAI {prompt_id}: local pass upscale={} method={} model={:?} scale={} downscale={} steps={} denoise={} cfg={} sampler={}/{} tiling={} tile_size={} facefix={} facefix_steps={} facefix_denoise={}",
        derived.upscale_enabled,
        derived.upscale_method,
        derived.upscale_model,
        derived.upscale_scale,
        derived.upscale_model_downscale_ratio,
        derived.upscale_steps,
        derived.upscale_denoise,
        derived.cfg,
        derived.sampler_name,
        derived.scheduler,
        derived.upscale_tiling || derived.use_split_model,
        derived.upscale_tile_size,
        derived.facefix_enabled,
        derived.facefix_steps,
        derived.facefix_denoise,
    );
    let workflow = crate::templates::build_workflow(&derived, params.seed, false);

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

    /// A vibe carrying a token minted for this exact model and extraction
    /// level is the only case that costs nothing.
    #[test]
    fn a_vibe_is_re_encoded_when_its_token_no_longer_matches() {
        let good = params::NovelAiVibe {
            image: Some("png".into()),
            encoding: Some("token".into()),
            encoded_model: Some("nai-diffusion-4-5-full".into()),
            encoded_information_extracted: Some(1.0),
            information_extracted: 1.0,
            ..Default::default()
        };
        assert!(!vibe_needs_encoding(&good, "nai-diffusion-4-5-full"));

        // Switching model invalidates the token.
        assert!(vibe_needs_encoding(&good, "nai-diffusion-4-full"));

        // So does moving the extraction slider.
        let moved = params::NovelAiVibe {
            information_extracted: 0.7,
            ..good.clone()
        };
        assert!(vibe_needs_encoding(&moved, "nai-diffusion-4-5-full"));

        // A token from before these fields were tracked is treated as stale.
        let legacy = params::NovelAiVibe {
            encoded_model: None,
            encoded_information_extracted: None,
            ..good.clone()
        };
        assert!(vibe_needs_encoding(&legacy, "nai-diffusion-4-5-full"));

        // No image means nothing to re-encode from, whatever the model is.
        let token_only = params::NovelAiVibe {
            image: None,
            encoded_model: None,
            encoded_information_extracted: None,
            ..good.clone()
        };
        assert!(!vibe_needs_encoding(&token_only, "nai-diffusion-4-full"));
    }

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
    fn dimensions_snap_onto_novelais_64px_grid() {
        assert_eq!(snap_dimension(1024), 1024);
        assert_eq!(snap_dimension(1088), 1088);
        assert_eq!(snap_dimension(832), 832);
        // An 8px-grid value from a local model rounds to the nearest legal one.
        assert_eq!(snap_dimension(1080), 1088);
        assert_eq!(snap_dimension(1352), 1344);
        // Never zero, whatever comes in.
        assert_eq!(snap_dimension(0), 64);
        assert_eq!(snap_dimension(8), 64);
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
