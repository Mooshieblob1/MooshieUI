//! Desktop commands for the cloud voice and video provider keys. The browser
//! server dispatches the same names in `webserver.rs`; both go through
//! `crate::cloud` so the per-account rules live in one place.

use std::sync::Arc;

use tauri::State;

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
