use serde_json::Value;
use tauri::State;

use crate::comfyui::types::*;
use crate::error::AppError;
use crate::state::AppState;

#[tauri::command]
pub async fn get_models(
    state: State<'_, AppState>,
    category: String,
) -> Result<Vec<String>, AppError> {
    state.get_models_list(&category).await
}

#[tauri::command]
pub async fn get_samplers(state: State<'_, AppState>) -> Result<SamplerInfo, AppError> {
    state.get_samplers_and_schedulers().await
}

#[tauri::command]
pub async fn get_embeddings(state: State<'_, AppState>) -> Result<Vec<String>, AppError> {
    state.get_embeddings_list().await
}

#[tauri::command]
pub async fn get_queue(state: State<'_, AppState>) -> Result<QueueInfo, AppError> {
    state.get_queue_info().await
}

#[tauri::command]
pub async fn get_history(
    state: State<'_, AppState>,
    prompt_id: String,
) -> Result<Value, AppError> {
    state.get_history_for(&prompt_id).await
}

#[tauri::command]
pub async fn interrupt_generation(state: State<'_, AppState>) -> Result<(), AppError> {
    state.interrupt().await
}

#[tauri::command]
pub async fn delete_queue_item(
    state: State<'_, AppState>,
    prompt_id: String,
) -> Result<(), AppError> {
    state.delete_queue_items(vec![prompt_id]).await
}

#[tauri::command]
pub async fn upload_image(
    state: State<'_, AppState>,
    image_path: String,
) -> Result<UploadResponse, AppError> {
    state.upload_image_file(&image_path).await
}

#[tauri::command]
pub async fn get_output_image(
    state: State<'_, AppState>,
    filename: String,
    subfolder: String,
) -> Result<Vec<u8>, AppError> {
    state.get_output_image_bytes(&filename, &subfolder).await
}

#[tauri::command]
pub async fn get_client_id(state: State<'_, AppState>) -> Result<String, AppError> {
    Ok(state.client_id.clone())
}

#[tauri::command]
pub async fn download_model(
    state: State<'_, AppState>,
    url: String,
    category: String,
    filename: String,
) -> Result<(), AppError> {
    state.download_model_file(&url, &category, &filename).await
}
