use tauri::State;

use crate::comfyui::types::GenerationParams;
use crate::error::AppError;
use crate::state::AppState;
use crate::templates;

#[tauri::command]
pub async fn generate(
    state: State<'_, AppState>,
    params: GenerationParams,
) -> Result<String, AppError> {
    let seed = if params.seed < 0 {
        (rand::random::<u64>() >> 1) as i64
    } else {
        params.seed
    };

    let workflow = templates::build_workflow(&params, seed);
    let response = state
        .queue_prompt_request(workflow, &state.client_id)
        .await?;
    Ok(response.prompt_id)
}
