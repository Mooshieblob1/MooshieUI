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
    if params.controlnet.as_ref().is_some_and(|cn| cn.enabled) || params.facefix_enabled {
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
