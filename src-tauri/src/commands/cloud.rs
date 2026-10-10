//! Desktop commands for the cloud voice and video provider keys. The browser
//! server dispatches the same names in `webserver.rs`; both go through
//! `crate::cloud` so the per-account rules live in one place.

use std::sync::Arc;

use tauri::State;

use crate::cloud::elevenlabs::{Moderated, Subscription, VoiceSummary};
use crate::cloud::voice::{
    self, DesignRequest, DesignResponse, SpeechEstimate, TakeInfo, TakeRequest,
};
use crate::cloud::{self, CloudKeyStatus, CloudProvider};
use crate::error::AppError;
use crate::state::AppState;

/// Store a provider key. An empty string is an explicit clear. Returns
/// whether a key is now configured.
#[tauri::command]
pub async fn set_cloud_api_key(
    state: State<'_, Arc<AppState>>,
    provider: String,
    api_key: String,
) -> Result<bool, AppError> {
    let provider = CloudProvider::parse(&provider)?;
    cloud::set_api_key(state.inner(), None, provider, &api_key).await
}

/// Which cloud providers have a key saved. Never returns a key.
#[tauri::command]
pub async fn cloud_key_status(state: State<'_, Arc<AppState>>) -> Result<CloudKeyStatus, AppError> {
    Ok(cloud::key_status(state.inner(), None).await)
}

#[tauri::command]
pub async fn elevenlabs_subscription(
    state: State<'_, Arc<AppState>>,
) -> Result<Subscription, AppError> {
    voice::subscription(state.inner(), None).await
}

#[tauri::command]
pub async fn elevenlabs_list_voices(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<VoiceSummary>, AppError> {
    voice::list_voices(state.inner(), None).await
}

#[tauri::command]
pub async fn elevenlabs_design_voice(
    state: State<'_, Arc<AppState>>,
    request: DesignRequest,
) -> Result<DesignResponse, AppError> {
    voice::design(state.inner(), None, request).await
}

#[tauri::command]
pub async fn elevenlabs_save_voice(
    state: State<'_, Arc<AppState>>,
    name: String,
    description: String,
    generated_voice_id: String,
) -> Result<VoiceSummary, AppError> {
    voice::save_voice(
        state.inner(),
        None,
        &name,
        &description,
        &generated_voice_id,
    )
    .await
}

#[tauri::command]
pub async fn elevenlabs_delete_voice(
    state: State<'_, Arc<AppState>>,
    voice_id: String,
) -> Result<(), AppError> {
    voice::delete_voice(state.inner(), None, &voice_id).await
}

#[tauri::command]
pub async fn scene_estimate_takes(requests: Vec<TakeRequest>) -> Result<SpeechEstimate, AppError> {
    voice::estimate_takes(None, &requests)
}

#[tauri::command]
pub async fn scene_render_take(
    state: State<'_, Arc<AppState>>,
    request: TakeRequest,
) -> Result<Moderated<TakeInfo>, AppError> {
    voice::render_take(state.inner(), None, request).await
}

#[tauri::command]
pub async fn scene_load_take(take_id: String) -> Result<TakeInfo, AppError> {
    voice::load_take(None, &take_id)
}
