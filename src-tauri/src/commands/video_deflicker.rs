use serde::Serialize;
use std::path::Path;
#[cfg(feature = "desktop")]
use std::sync::Arc;
#[cfg(feature = "desktop")]
use tauri::State;

use crate::error::AppError;
use crate::state::AppState;
use crate::templates::video_deflicker::DeflickerSettings;

#[derive(Serialize)]
pub struct DeflickerVideoResponse {
    pub prompt_id: String,
}

/// Resolve an installed optical flow model filename for `OpticalFlowLoader`.
/// Explicitly requested filenames must match an installed entry; otherwise we
/// prefer `raft_large` if present, falling back to the first installed model.
pub(crate) fn resolve_optical_flow_model(
    requested: Option<&str>,
    installed: &[String],
) -> Result<String, AppError> {
    if installed.is_empty() {
        return Err(AppError::Other(
            "No optical flow model is installed in models/optical_flow. Download RAFT-large first."
                .into(),
        ));
    }

    if let Some(raw) = requested {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return installed
                .iter()
                .find(|entry| entry.as_str() == trimmed)
                .cloned()
                .ok_or_else(|| {
                    AppError::Other("Requested optical flow model is not installed".into())
                });
        }
    }

    if let Some(raft) = installed
        .iter()
        .find(|entry| entry.to_ascii_lowercase().contains("raft_large"))
    {
        return Ok(raft.clone());
    }

    Ok(installed[0].clone())
}

/// Verify ComfyUI v0.46.0+ `VideoDeflicker` availability, resolve the optical
/// flow weight, and queue the deflicker pass on the finished gallery clip.
pub(crate) async fn submit_deflicker(
    state: &AppState,
    source: &Path,
    requested_model: Option<&str>,
    settings: DeflickerSettings,
    owner: Option<String>,
) -> Result<String, AppError> {
    let object_info = match state.api_get("/object_info/VideoDeflicker").await {
        Ok(value) => value,
        Err(AppError::ConnectionFailed(msg)) => return Err(AppError::ConnectionFailed(msg)),
        Err(_) => {
            return Err(AppError::Other(
                "VideoDeflicker requires ComfyUI v0.46.0 or newer. Update ComfyUI in Settings -> Paths."
                    .into(),
            ));
        }
    };
    if object_info.get("VideoDeflicker").is_none() {
        return Err(AppError::Other(
            "VideoDeflicker requires ComfyUI v0.46.0 or newer. Update ComfyUI in Settings -> Paths."
                .into(),
        ));
    }

    let installed = state
        .get_models_list("optical_flow")
        .await
        .unwrap_or_default();
    let model = resolve_optical_flow_model(requested_model, &installed)?;

    let workflow =
        crate::templates::video_deflicker::build(&source.to_string_lossy(), &model, settings);
    log::info!(
        "deflicker_video: model={} strength={} direction={} threshold={} source={}",
        model,
        settings.strength,
        settings.direction,
        settings.threshold,
        source.display()
    );

    crate::comfyui::process::mark_legacy_worker_idle(state).await;
    state.free_llm_vram_for_generation().await;

    let timeout = std::time::Duration::from_secs(300);
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, timeout)
        .await?;

    state.prompt_queue.insert(&response.prompt_id, owner);
    state
        .prompt_queue
        .set_worker(&response.prompt_id, worker_id);
    state.broadcast_queue_positions();

    Ok(response.prompt_id)
}

/// Queue a core `VideoDeflicker` pass over a finished gallery clip.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn deflicker_video(
    state: State<'_, Arc<AppState>>,
    filename: String,
    strength: f64,
    direction: String,
    threshold: f64,
    optical_flow_model: Option<String>,
) -> Result<DeflickerVideoResponse, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let source = super::video_interpolate::resolve_gallery_video(&dir, &filename)?;
    let settings = DeflickerSettings::sanitized(strength, &direction, threshold);
    let prompt_id = submit_deflicker(
        state.inner(),
        &source,
        optical_flow_model.as_deref(),
        settings,
        None,
    )
    .await?;
    Ok(DeflickerVideoResponse { prompt_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_optical_flow_inventory() {
        assert!(resolve_optical_flow_model(None, &[]).is_err());
    }

    #[test]
    fn auto_picks_raft_large_then_first_installed() {
        let installed = vec![
            "raft_small_other.safetensors".to_string(),
            "raft_large_C_T_SKHT_V2-ff5fadd5.safetensors".to_string(),
        ];
        assert_eq!(
            resolve_optical_flow_model(None, &installed).unwrap(),
            "raft_large_C_T_SKHT_V2-ff5fadd5.safetensors"
        );
        assert_eq!(
            resolve_optical_flow_model(Some("   "), &installed).unwrap(),
            "raft_large_C_T_SKHT_V2-ff5fadd5.safetensors"
        );

        let fallback = vec!["custom_flow.safetensors".to_string()];
        assert_eq!(
            resolve_optical_flow_model(None, &fallback).unwrap(),
            "custom_flow.safetensors"
        );
    }

    #[test]
    fn validates_explicitly_requested_optical_flow_model() {
        let installed = vec!["raft_large_C_T_SKHT_V2-ff5fadd5.safetensors".to_string()];
        assert_eq!(
            resolve_optical_flow_model(
                Some("raft_large_C_T_SKHT_V2-ff5fadd5.safetensors"),
                &installed
            )
            .unwrap(),
            "raft_large_C_T_SKHT_V2-ff5fadd5.safetensors"
        );
        assert!(resolve_optical_flow_model(Some("../evil.safetensors"), &installed).is_err());
        assert!(resolve_optical_flow_model(Some("missing.safetensors"), &installed).is_err());
    }
}
