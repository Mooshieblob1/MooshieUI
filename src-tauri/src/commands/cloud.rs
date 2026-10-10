//! Desktop commands for the cloud voice and video provider keys. The browser
//! server dispatches the same names in `webserver.rs`; both go through
//! `crate::cloud` so the per-account rules live in one place.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::cloud::elevenlabs::{Moderated, Subscription, VoiceSummary};
use crate::cloud::scene::job::{self, ScenePlan, SceneRequest};
use crate::cloud::video::VideoCapabilities;
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

#[tauri::command]
pub async fn scene_video_capabilities() -> Result<Vec<VideoCapabilities>, AppError> {
    Ok(job::capabilities())
}

/// Timings, prompt and price for a scene. Spends nothing.
#[tauri::command]
pub async fn scene_plan(
    state: State<'_, Arc<AppState>>,
    request: SceneRequest,
) -> Result<ScenePlan, AppError> {
    job::plan(state.inner(), None, &request).await
}

/// Start a paid scene render and return its prompt id. The page shows the
/// itemized confirmation before calling this.
#[tauri::command]
pub async fn scene_generate(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    request: SceneRequest,
) -> Result<String, AppError> {
    let sink = crate::novelai::EventSink::new(Arc::clone(state.inner()), Some(app));
    job::start(state.inner(), None, &request, sink).await
}

/// Pick up unfinished scenes after a restart. Costs nothing.
#[tauri::command]
pub async fn scene_resume_jobs(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, AppError> {
    let shared = Arc::clone(state.inner());
    job::resume(state.inner(), None, || {
        crate::novelai::EventSink::new(Arc::clone(&shared), Some(app.clone()))
    })
    .await
}

/// Start a NovelAI keyframe for one shot; returns its prompt id. The page
/// shows the Anlas estimate before calling this.
#[tauri::command]
pub async fn scene_keyframe(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    request: crate::cloud::scene::keyframe::KeyframeRequest,
) -> Result<String, AppError> {
    let sink = crate::novelai::EventSink::new(Arc::clone(state.inner()), Some(app));
    crate::cloud::scene::keyframe::start(state.inner(), None, &request, sink).await
}

/// What measuring mouth timing for these takes would cost. Calls no provider.
#[tauri::command]
pub async fn scene_estimate_alignment(
    state: State<'_, Arc<AppState>>,
    take_ids: Vec<String>,
) -> Result<voice::AlignEstimate, AppError> {
    voice::estimate_alignment(state.inner(), None, &take_ids).await
}

/// Measure the mouth timing of takes not measured yet. Paid (ElevenLabs);
/// the page shows the estimate first. Returns how many were measured.
#[tauri::command]
pub async fn scene_align_takes(
    state: State<'_, Arc<AppState>>,
    take_ids: Vec<String>,
) -> Result<usize, AppError> {
    voice::align_takes(state.inner(), None, &take_ids).await
}

/// What can still be done with a finished scene clip.
#[tauri::command]
pub async fn scene_clip_info(filename: String) -> Result<job::ClipInfo, AppError> {
    job::clip_info(None, &filename).await
}

/// Complete a draft clip at 1080p; returns the prompt id. Paid; the page
/// shows the itemized price first.
#[tauri::command]
pub async fn scene_upgrade_draft(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    filename: String,
) -> Result<String, AppError> {
    let sink = crate::novelai::EventSink::new(Arc::clone(state.inner()), Some(app));
    job::start_upgrade(state.inner(), None, &filename, sink).await
}

/// Replace a clip's sound with the user's own voice track, as a new gallery
/// clip. Free and local.
#[tauri::command]
pub async fn scene_exact_voice(
    state: State<'_, Arc<AppState>>,
    filename: String,
) -> Result<serde_json::Value, AppError> {
    job::exact_voice(state.inner(), None, &filename).await
}
