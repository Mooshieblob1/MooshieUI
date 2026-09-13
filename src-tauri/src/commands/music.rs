//! Music IPC shared by desktop and browser clients. Audio is read only from
//! the history of the caller's prompt on the worker that actually ran it.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
#[cfg(feature = "desktop")]
use {std::sync::Arc, tauri::State};

use crate::{
    error::AppError,
    state::AppState,
    templates::music::{self, MusicParams},
};

#[derive(Serialize)]
pub struct MusicCapabilities {
    missing_nodes: Vec<String>,
    checkpoints: Vec<String>,
}

pub(crate) fn capabilities_from_info(info: &Value) -> MusicCapabilities {
    MusicCapabilities {
        missing_nodes: music::REQUIRED_NODES
            .iter()
            .filter(|name| !info[**name].is_object())
            .map(|name| name.to_string())
            .collect(),
        checkpoints: info["CheckpointLoaderSimple"]["input"]["required"]["ckpt_name"][0]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| name.to_lowercase().contains("yue2"))
            .map(str::to_string)
            .collect(),
    }
}

pub(crate) async fn capabilities(state: &AppState) -> Result<MusicCapabilities, AppError> {
    Ok(capabilities_from_info(
        &state.api_get("/object_info").await?,
    ))
}

pub(crate) async fn submit(
    state: &AppState,
    params: MusicParams,
    owner: Option<String>,
) -> Result<Value, AppError> {
    music::validate(&params).map_err(AppError::InvalidWorkflow)?;
    let caps = capabilities(state).await?;
    if !caps.missing_nodes.is_empty() {
        return Err(AppError::InvalidWorkflow(format!(
            "YuE2 support is missing from the running ComfyUI server: {}. Update MooshieUI's managed runtime from the Music page, or update ComfyUI on the remote host, then restart it.",
            caps.missing_nodes.join(", ")
        )));
    }
    if !caps.checkpoints.contains(&params.checkpoint) {
        return Err(AppError::InvalidWorkflow(
            "The selected YuE2 checkpoint is no longer available. Refresh the model list.".into(),
        ));
    }
    let seed = if params.seed == -1 {
        (rand::random::<u64>() >> 1) as i64
    } else {
        params.seed
    };
    crate::comfyui::process::mark_legacy_worker_idle(state).await;
    state.free_llm_vram_for_generation().await;
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt(
            music::build(&params, seed),
            &state.client_id,
            Duration::from_secs(300),
        )
        .await?;
    state.prompt_queue.mark_music(&response.prompt_id);
    state.prompt_queue.insert(&response.prompt_id, owner);
    state
        .prompt_queue
        .set_worker(&response.prompt_id, worker_id);
    state.broadcast_queue_positions();
    Ok(json!({"prompt_id": response.prompt_id, "worker_id": worker_id, "seed": seed.to_string()}))
}

fn worker_url(state: &AppState, worker_id: u32, prompt_id: &str) -> Result<String, AppError> {
    if prompt_id.is_empty()
        || prompt_id.len() > 128
        || !prompt_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(AppError::Other("Invalid music prompt ID".into()));
    }
    state
        .gpu_manager
        .workers
        .iter()
        .find(|worker| worker.id == worker_id)
        .map(|worker| worker.base_url.trim_end_matches('/').to_string())
        .ok_or_else(|| AppError::Other("The music GPU worker is no longer configured.".into()))
}

async fn history(state: &AppState, base: &str, prompt_id: &str) -> Result<Value, AppError> {
    Ok(state
        .http_client
        .get(format!("{base}/history/{prompt_id}"))
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

fn audio_output(entry: &Value) -> Result<Option<&Value>, AppError> {
    let Some(output) = entry["outputs"]["8"]["audio"]
        .as_array()
        .and_then(|a| a.first())
    else {
        return Ok(None);
    };
    let filename = output["filename"].as_str().unwrap_or("");
    if !filename.starts_with("mooshie_yue2_")
        || !filename.ends_with(".flac")
        || filename.contains(['/', '\\', ':'])
        || filename.contains("..")
        || output["subfolder"] != "audio"
        || output["type"] != "output"
    {
        return Err(AppError::Other("Unexpected YuE2 audio output path".into()));
    }
    Ok(Some(output))
}

fn completed_status(entry: &Value) -> Result<Value, AppError> {
    if entry["status"]["status_str"] == "error" {
        let error = entry["status"]["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|message| message[1]["exception_message"].as_str())
            .unwrap_or("Music generation was interrupted or failed.");
        return Ok(json!({"status": "error", "error": error}));
    }
    let Some(audio) = audio_output(entry)? else {
        return Ok(
            json!({"status": "error", "error": "ComfyUI finished without a YuE2 audio file."}),
        );
    };
    let abc = entry["outputs"]["9"]["text"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(json!({"status": "completed", "filename": audio["filename"], "abc": abc}))
}

pub(crate) async fn status(
    state: &AppState,
    prompt_id: &str,
    worker_id: u32,
) -> Result<Value, AppError> {
    let base = worker_url(state, worker_id, prompt_id)?;
    let history = history(state, &base, prompt_id).await?;
    if let Some(entry) = history.get(prompt_id) {
        // Cached runs can finish before submit() records their worker, and a
        // websocket disconnect can lose completion entirely. History is final;
        // reconcile here so a finished song cannot keep its GPU reserved.
        if state.prompt_queue.is_music(prompt_id) {
            if let Some(worker) = state.prompt_queue.finish(prompt_id) {
                state.gpu_manager.mark_worker_idle(worker).await;
                state.broadcast_queue_positions();
                state.prompt_queue.drain_notify.notify_one();
            }
        }
        return completed_status(entry);
    }
    let queue: Value = state
        .http_client
        .get(format!("{base}/queue"))
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    for (field, status) in [("queue_running", "running"), ("queue_pending", "queued")] {
        if queue[field]
            .as_array()
            .into_iter()
            .flatten()
            .any(|entry| entry[1] == prompt_id)
        {
            return Ok(json!({"status": status}));
        }
    }
    // Allow the client to retry the short transition between queue and history.
    Ok(json!({"status": "missing"}))
}

pub(crate) async fn audio(
    state: &AppState,
    prompt_id: &str,
    worker_id: u32,
) -> Result<String, AppError> {
    let base = worker_url(state, worker_id, prompt_id)?;
    let history = history(state, &base, prompt_id).await?;
    let output = audio_output(&history[prompt_id])?.ok_or_else(|| {
        AppError::Other("The YuE2 audio is not available in ComfyUI history.".into())
    })?;
    let mut response = state
        .http_client
        .get(format!("{base}/view"))
        .query(&[
            ("filename", output["filename"].as_str().unwrap_or("")),
            ("subfolder", "audio"),
            ("type", "output"),
        ])
        .timeout(Duration::from_secs(120))
        .send()
        .await?
        .error_for_status()?;
    const LIMIT: usize = 192 * 1024 * 1024;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > LIMIT {
            return Err(AppError::Other("Audio exceeds the 192 MiB playback limit. Retrieve it from ComfyUI's output/audio folder.".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !bytes.starts_with(b"fLaC") {
        return Err(AppError::Other(
            "ComfyUI did not return a FLAC audio file.".into(),
        ));
    }
    Ok(STANDARD.encode(bytes))
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_music_capabilities(
    state: State<'_, Arc<AppState>>,
) -> Result<MusicCapabilities, AppError> {
    capabilities(&state).await
}
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn generate_music(
    state: State<'_, Arc<AppState>>,
    params: MusicParams,
) -> Result<Value, AppError> {
    submit(&state, params, None).await
}
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_music_status(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
    worker_id: u32,
) -> Result<Value, AppError> {
    status(&state, &prompt_id, worker_id).await
}
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn load_music_audio(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
    worker_id: u32,
) -> Result<String, AppError> {
    audio(&state, &prompt_id, worker_id).await
}

// Desktop saves the already-loaded original, so exports also work after a
// ComfyUI restart. Browser clients download their Blob locally instead.
#[cfg(any(feature = "desktop", test))]
#[cfg_attr(feature = "desktop", tauri::command)]
pub async fn save_music_audio(audio_base64: String, path: String) -> Result<(), AppError> {
    if audio_base64.len() > 192 * 1024 * 1024 / 3 * 4 {
        return Err(AppError::Other(
            "Audio exceeds the 192 MiB export limit.".into(),
        ));
    }
    let bytes = STANDARD
        .decode(audio_base64)
        .map_err(|_| AppError::Other("Invalid encoded FLAC audio.".into()))?;
    if !bytes.starts_with(b"fLaC") {
        return Err(AppError::Other("The audio is not a FLAC file.".into()));
    }
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn flac_export_preserves_bytes_and_rejects_invalid_data_before_writing() {
        let path =
            std::env::temp_dir().join(format!("mooshie-audio-{}.flac", uuid::Uuid::new_v4()));
        let target = path.to_string_lossy().into_owned();
        let audio = b"fLaC\0\x80\xff\n\r\x01";
        save_music_audio(STANDARD.encode(audio), target.clone())
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), audio);
        for invalid in ["!invalid!".to_string(), STANDARD.encode(b"not audio")] {
            assert!(save_music_audio(invalid, target.clone()).await.is_err());
            assert_eq!(tokio::fs::read(&path).await.unwrap(), audio);
        }
        tokio::fs::remove_file(path).await.unwrap();
    }
    #[test]
    fn detects_old_servers_and_filters_checkpoints() {
        let info = json!({"CheckpointLoaderSimple": {"input": {"required": {"ckpt_name": [["sdxl.safetensors", "YuE2/model.safetensors"]]}}}});
        let caps = capabilities_from_info(&info);
        assert!(caps.missing_nodes.contains(&"YuE2GenerateMusic".into()));
        assert_eq!(caps.checkpoints, vec!["YuE2/model.safetensors"]);
    }
    #[test]
    fn reads_audio_and_score_and_rejects_path_traversal() {
        let mut entry = json!({"status": {"status_str": "success"}, "outputs": {
            "8": {"audio": [{"filename": "mooshie_yue2_00001_.flac", "subfolder": "audio", "type": "output"}]},
            "9": {"text": ["X:1\nK:C"]}
        }});
        let result = completed_status(&entry).unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["abc"], "X:1\nK:C");
        for name in [
            "../secret.flac",
            "mooshie_yue2_../secret.flac",
            "mooshie_yue2_:secret.flac",
        ] {
            entry["outputs"]["8"]["audio"][0]["filename"] = json!(name);
            assert!(audio_output(&entry).is_err());
        }
    }
    #[test]
    fn reports_errors_and_missing_outputs() {
        assert_eq!(completed_status(&json!({})).unwrap()["status"], "error");
        let entry = json!({"status": {"status_str": "error", "messages": [["execution_error", {"exception_message": "out of memory"}]]}});
        assert_eq!(completed_status(&entry).unwrap()["error"], "out of memory");
    }

    #[tokio::test]
    async fn status_and_audio_use_the_original_worker_after_queue_cleanup() {
        use axum::{routing::get, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = Router::new()
            .route("/history/music-test", get(|| async {
                Json(json!({"music-test": {"status": {"status_str": "success"}, "outputs": {
                    "8": {"audio": [{"filename": "mooshie_yue2_00001_.flac", "subfolder": "audio", "type": "output"}]}
                }}}))
            }))
            .route("/view", get(|axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>| async move {
                assert_eq!(query.get("filename").unwrap(), "mooshie_yue2_00001_.flac");
                assert_eq!(query.get("subfolder").unwrap(), "audio");
                b"fLaCtest".to_vec()
            }));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut config = crate::config::AppConfig::default();
        config.gpu_workers = [1, port]
            .into_iter()
            .map(|port| crate::config::GpuWorkerConfig {
                gpu_index: 0,
                port: Some(port),
                enabled: true,
                label: None,
                vram_mode: None,
            })
            .collect();
        let state = AppState::new(config);
        // No worker_map entry remains after the normal completion reactor runs.
        assert_eq!(state.prompt_queue.worker_of("music-test"), None);
        let result = status(&state, "music-test", 1).await.unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(
            audio(&state, "music-test", 1).await.unwrap(),
            STANDARD.encode(b"fLaCtest")
        );
        state.prompt_queue.mark_music("music-test");
        state.prompt_queue.insert("music-test", None);
        state.prompt_queue.set_worker("music-test", 1);
        assert!(state.gpu_manager.workers[1].try_reserve());
        status(&state, "music-test", 1).await.unwrap();
        assert_eq!(state.prompt_queue.worker_of("music-test"), None);
        assert!(!state.prompt_queue.is_music("music-test"));
        assert!(!state.gpu_manager.workers[1]
            .reserved
            .load(std::sync::atomic::Ordering::Acquire));
        assert!(status(&state, "../secret", 1).await.is_err());
        assert!(status(&state, "music-test", 10).await.is_err());
        server.abort();
    }

    #[tokio::test]
    async fn queue_updates_identify_music_without_restoring_image_progress() {
        let state = AppState::new(crate::config::AppConfig::default());
        let mut events = state.event_tx.subscribe();
        state.prompt_queue.mark_music("song");
        state.prompt_queue.insert("song", Some("alice".into()));
        state.prompt_queue.insert("image", Some("bob".into()));
        state.broadcast_queue_positions();
        assert_eq!(events.recv().await.unwrap().payload["kind"], "music");
        assert_eq!(events.recv().await.unwrap().payload["kind"], "generation");
        assert!(!state.prompt_queue.is_owned_by("song", &Some("bob".into())));
        assert!(state
            .prompt_queue
            .is_owned_by("song", &Some("alice".into())));
    }
}
