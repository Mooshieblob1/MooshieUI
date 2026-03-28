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
    state: State<'_, AppState>,
    params: GenerationParams,
) -> Result<GenerateResponse, AppError> {
    let seed = if params.seed < 0 {
        (rand::random::<u64>() >> 1) as i64
    } else {
        params.seed
    };

    let workflow = templates::build_workflow(&params, seed);
    if params.controlnet.as_ref().map_or(false, |cn| cn.enabled) || params.facefix_enabled {
        log::info!(
            "Workflow JSON: {}",
            serde_json::to_string_pretty(&workflow).unwrap_or_default()
        );
    }
    let response = state
        .queue_prompt_request(workflow, &state.client_id)
        .await?;
    Ok(GenerateResponse {
        prompt_id: response.prompt_id,
        seed,
    })
}
