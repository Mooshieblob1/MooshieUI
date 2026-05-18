use std::sync::Arc;

use tauri::State;

use crate::comfyui::types::GenerationParams;
use crate::error::AppError;
use crate::state::AppState;
use crate::templates;

/// Response from the generate command, includes the resolved seed.
#[derive(serde::Serialize)]
pub struct GenerateResponse {
    pub prompt_id: String,
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

    let seed = if params.seed < 0 {
        (rand::random::<u64>() >> 1) as i64
    } else {
        params.seed
    };

    let workflow = templates::build_workflow(&params, seed);
    log::info!(
        "generate: output_format={}, output_bit_depth={}, mode={}",
        params.output_format,
        params.output_bit_depth,
        params.mode,
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
