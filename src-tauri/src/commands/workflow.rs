use std::sync::Arc;

use tauri::State;

use crate::comfyui::types::GenerationParams;
use crate::error::AppError;
use crate::state::AppState;
use crate::templates;

/// Response from the generate command, includes the resolved seed.
/// The seed is serialized as a string: 63-bit values exceed JavaScript's
/// 2^53 safe-integer range and would be silently rounded as a JSON number.
#[derive(serde::Serialize)]
pub struct GenerateResponse {
    pub prompt_id: String,
    #[serde(serialize_with = "crate::comfyui::types::seed_string::serialize")]
    pub seed: i64,
}

#[tauri::command]
pub async fn generate(
    state: State<'_, Arc<AppState>>,
    params: GenerationParams,
) -> Result<GenerateResponse, AppError> {
    // Clean up temp images from previous generations (> 5 min old).
    // temp_images::init() already wipes the dir on startup; this handles
    // accumulation within a long session.
    crate::temp_images::cleanup(300);

    // Validate input image is present for modes that require it. Without this
    // guard, ComfyUI's LoadImage node receives an empty filename and reports
    // `[Errno 21] Is a directory: '<input_dir>/'`, which surfaces as a generic
    // execution error far away from the actual cause.
    templates::validate_generation_params(&params).map_err(AppError::InvalidWorkflow)?;
    {
        let config = state.config.read().await;
        crate::commands::api::validate_lora_files_for_generation(
            &config.comfyui_path,
            config.extra_model_paths.as_deref(),
            &params.loras,
        )?;
    }

    // The MiniMax H3 nodes are verified here rather than at startup: they are
    // video-only and need ComfyUI >= 0.30, so an older or external server can be
    // perfectly healthy for image generation without them. Checking only in video
    // mode keeps every other generation free of the round trip, and turns what
    // would otherwise be a raw ComfyUI prompt-validation error into an actionable
    // message. mode == "video" always uses one of these nodes (native or,
    // when a timeline drives the graph, the vendored Director) — see
    // `templates::video::build`.
    if params.mode == "video" {
        let base_url = { state.config.read().await.server_url.clone() };
        crate::comfyui::nodes::verify_required_h3_nodes_for_generation(
            &state.http_client,
            &base_url,
            &params,
        )
        .await
        .map_err(AppError::InvalidWorkflow)?;
    }

    let mut params = params;
    // Misplaced model: the file lives in a folder that doesn't match what it
    // actually is (e.g. a Flux unet dropped into models/checkpoints/). ComfyUI's
    // stock loaders validate the filename against their own folder listing and
    // would reject it, so resolve an absolute path here and let the workflow use
    // the Mooshie path-based loader nodes instead.
    if let Some(source_category) = params.model_source_category.clone() {
        let active_model = if params.use_split_model {
            params.diffusion_model.clone()
        } else {
            Some(params.checkpoint.clone())
        };
        if let Some(filename) = active_model.filter(|f| !f.is_empty()) {
            let resolved = {
                let config = state.config.read().await;
                crate::commands::api::resolve_model_path(
                    &config.comfyui_path,
                    config.extra_model_paths.as_deref(),
                    &source_category,
                    &filename,
                )
            };
            match resolved {
                Some(path) => {
                    params.resolved_model_path = Some(path.to_string_lossy().to_string());
                }
                None => {
                    return Err(AppError::InvalidWorkflow(format!(
                        "Model file not found: {}/{}",
                        source_category, filename
                    )));
                }
            }
        }
    }

    let seed = if params.seed < 0 {
        (rand::random::<u64>() >> 1) as i64
    } else {
        params.seed
    };

    // Probed rather than assumed: a remote or hosted ComfyUI is not installed by
    // `ensure_mooshie_nodes()`, so its node file can predate `metadata_json`, and
    // setting an input a node does not declare fails prompt validation. This
    // mirrors the H3 Director check above, which is the same shape of problem.
    let video_metadata_supported = if params.mode == "video" {
        let base_url = state.base_url().await;
        crate::comfyui::nodes::node_declares_input(
            &state.http_client,
            &base_url,
            "MooshieSaveVideo",
            "metadata_json",
        )
        .await
    } else {
        false
    };

    let workflow = templates::build_workflow(&params, seed, video_metadata_supported);
    crate::comfyui::process::mark_legacy_worker_idle(state.inner()).await;
    log::info!(
        "generate: output_format={}, output_bit_depth={}, mode={}, architecture={}, positive_regions={}",
        params.output_format,
        params.output_bit_depth,
        params.mode,
        params.model_architecture,
        params.positive_regions.len(),
    );
    if params.controlnet.as_ref().is_some_and(|cn| cn.enabled)
        || params.facefix_enabled
        || !params.loras.is_empty()
    {
        log::info!(
            "Workflow JSON: {}",
            serde_json::to_string_pretty(&workflow).unwrap_or_default()
        );
    }

    // Release the prompt-assistant LLM's VRAM so it doesn't starve ComfyUI's
    // diffusion model (which would otherwise spill into shared system memory).
    state.free_llm_vram_for_generation().await;

    // Route through GPU manager for multi-GPU distribution
    let timeout = std::time::Duration::from_secs(300);
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, timeout)
        .await?;

    // Track the Tauri (host) prompt in the shared queue so LAN users see
    // an accurate queue position.  None = admin / host user.
    state.prompt_queue.insert(&response.prompt_id, None);
    state
        .prompt_queue
        .set_worker(&response.prompt_id, worker_id);
    state.broadcast_queue_positions();

    Ok(GenerateResponse {
        prompt_id: response.prompt_id,
        seed,
    })
}

#[derive(serde::Serialize)]
pub struct ControlNetPreprocessorPreviewResponse {
    pub prompt_id: String,
}

#[tauri::command]
pub async fn generate_controlnet_preprocessor_preview(
    state: State<'_, Arc<AppState>>,
    image: String,
    preprocessor: String,
) -> Result<ControlNetPreprocessorPreviewResponse, AppError> {
    crate::temp_images::cleanup(300);

    if image.trim().is_empty() {
        return Err(AppError::InvalidWorkflow(
            "ControlNet preprocessor preview needs a control image.".into(),
        ));
    }
    if preprocessor.trim().is_empty() {
        return Err(AppError::InvalidWorkflow(
            "ControlNet preprocessor preview needs a preprocessor.".into(),
        ));
    }

    let workflow = templates::controlnet::build_preprocessor_preview_workflow(
        image.trim(),
        preprocessor.trim(),
    );
    let timeout = std::time::Duration::from_secs(120);
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, timeout)
        .await?;

    state.prompt_queue.insert(&response.prompt_id, None);
    state
        .prompt_queue
        .set_worker(&response.prompt_id, worker_id);
    state.broadcast_queue_positions();

    Ok(ControlNetPreprocessorPreviewResponse {
        prompt_id: response.prompt_id,
    })
}
