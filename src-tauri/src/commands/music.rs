//! Music IPC shared by desktop and browser clients. Audio is read only from
//! the history of the caller's prompt on the worker that actually ran it.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
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
    extended: bool,
    cover_missing_nodes: Vec<String>,
    audio_encoders: Vec<String>,
    workers: Vec<MusicWorkerCapabilities>,
}

#[derive(Clone, Serialize)]
pub struct MusicWorkerCapabilities {
    worker_id: u32,
    missing_nodes: Vec<String>,
    checkpoints: Vec<String>,
    extended: bool,
    cover_missing_nodes: Vec<String>,
    audio_encoders: Vec<String>,
}

fn model_names(info: &Value, node: &str, field: &str) -> Vec<String> {
    let input = &info[node]["input"]["required"][field];
    // Legacy nodes put choices in slot 0. New schema nodes (including
    // AudioEncoderLoader) expose ["COMBO", {"options": [...]}] instead.
    input[0]
        .as_array()
        .or_else(|| {
            (input[0].as_str() == Some("COMBO"))
                .then(|| input[1]["options"].as_array())
                .flatten()
        })
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

pub(crate) fn capabilities_from_info(info: &Value) -> MusicCapabilities {
    MusicCapabilities {
        missing_nodes: music::REQUIRED_NODES
            .iter()
            .filter(|name| !info[**name].is_object())
            .map(|name| name.to_string())
            .collect(),
        checkpoints: model_names(info, "CheckpointLoaderSimple", "ckpt_name")
            .into_iter()
            .filter(|name| name.to_lowercase().contains("yue2"))
            .collect(),
        extended: music::EXTENDED_NODES
            .iter()
            .all(|node| info[*node].is_object()),
        cover_missing_nodes: music::COVER_NODES
            .iter()
            .filter(|node| !info[**node].is_object())
            .map(|s| s.to_string())
            .collect(),
        audio_encoders: model_names(info, "AudioEncoderLoader", "audio_encoder_name")
            .into_iter()
            .filter(|name| name.to_lowercase().contains("sheetsage2"))
            .collect(),
        workers: Vec::new(),
    }
}

pub(crate) async fn capabilities(state: &AppState) -> Result<MusicCapabilities, AppError> {
    use crate::comfyui::gpu_manager::WorkerStatus;
    crate::comfyui::process::mark_legacy_worker_idle(state).await;
    let candidates =
        futures_util::future::join_all(state.gpu_manager.workers.iter().map(|worker| async move {
            let status = *worker.status.read().await;
            if !matches!(status, WorkerStatus::Idle | WorkerStatus::Running) {
                return None;
            }
            let response = state
                .http_client
                .get(format!("{}/object_info", worker.base_url()))
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?;
            let info: Value = response.json().await.ok()?;
            let caps = capabilities_from_info(&info);
            Some((
                status == WorkerStatus::Running,
                MusicWorkerCapabilities {
                    worker_id: worker.id,
                    missing_nodes: caps.missing_nodes,
                    checkpoints: caps.checkpoints,
                    extended: caps.extended,
                    cover_missing_nodes: caps.cover_missing_nodes,
                    audio_encoders: caps.audio_encoders,
                },
            ))
        }))
        .await;
    let mut candidates: Vec<_> = candidates.into_iter().flatten().collect();
    candidates.sort_by_key(|(running, _)| *running);
    if candidates.is_empty() {
        return Err(AppError::ConnectionFailed(
            "No music GPU worker is reachable.".into(),
        ));
    }
    let mut caps = capabilities_from_info(&json!({}));
    caps.workers = candidates.into_iter().map(|(_, caps)| caps).collect();
    if caps.workers.iter().any(|w| w.missing_nodes.is_empty()) {
        caps.missing_nodes.clear();
    }
    if caps
        .workers
        .iter()
        .any(|w| w.cover_missing_nodes.is_empty())
    {
        caps.cover_missing_nodes.clear();
    }
    caps.extended = caps
        .workers
        .iter()
        .any(|w| w.extended && w.missing_nodes.is_empty());
    caps.checkpoints = caps
        .workers
        .iter()
        .filter(|w| w.missing_nodes.is_empty())
        .flat_map(|w| w.checkpoints.clone())
        .collect();
    caps.audio_encoders = caps
        .workers
        .iter()
        .filter(|w| w.cover_missing_nodes.is_empty())
        .flat_map(|w| w.audio_encoders.clone())
        .collect();
    caps.checkpoints.sort();
    caps.checkpoints.dedup();
    caps.audio_encoders.sort();
    caps.audio_encoders.dedup();
    Ok(caps)
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
    let worker = caps.workers.iter().find(|w| w.missing_nodes.is_empty()
        && w.checkpoints.contains(&params.checkpoint)
        && (params.sampling.cfg_scale == -1.0 || w.extended))
        .ok_or_else(|| AppError::InvalidWorkflow("No worker has this checkpoint and the requested music controls. Update MooshieUI's nodes or use automatic guidance.".into()))?;
    state.free_llm_vram_for_generation().await;
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt_to_worker(
            worker.worker_id,
            music::build_with_capabilities(&params, seed, worker.extended),
            &state.client_id,
        )
        .await?;
    state.prompt_queue.mark_music(&response.prompt_id);
    state.prompt_queue.insert(&response.prompt_id, owner);
    state
        .prompt_queue
        .set_worker(&response.prompt_id, worker_id);
    state.broadcast_queue_positions();
    Ok(
        json!({"prompt_id": response.prompt_id, "worker_id": worker_id, "seed": seed.to_string(),
        "kind": if params.task == music::MusicTask::Plan { "plan" } else { "audio" },
        "backend": if worker.extended { "mooshie-yue2-1" } else { "native-comfyui" }}),
    )
}

#[derive(Deserialize)]
pub struct NativeCoverRequest {
    pub audio_base64: String,
    pub filename: String,
    pub encoder: String,
    pub mode: String,
}

pub(crate) async fn transcribe_native(
    state: &AppState,
    request: NativeCoverRequest,
    owner: Option<String>,
) -> Result<Value, AppError> {
    if !matches!(request.mode.as_str(), "melody" | "full") {
        return Err(AppError::InvalidWorkflow(
            "Choose melody or full transcription.".into(),
        ));
    }
    let (bytes, extension) =
        super::music_cover::decode_audio(&request.audio_base64, &request.filename)?;
    let caps = capabilities(state).await?;
    let worker = caps.workers.iter().find(|w| w.cover_missing_nodes.is_empty() && w.audio_encoders.contains(&request.encoder))
        .ok_or_else(|| AppError::InvalidWorkflow("Install the SheetSage2 audio encoder and current MooshieUI nodes on the same ComfyUI worker, then refresh.".into()))?;
    let endpoint = state
        .gpu_manager
        .workers
        .iter()
        .find(|w| w.id == worker.worker_id)
        .ok_or_else(|| AppError::ConnectionFailed("Cover worker is unavailable.".into()))?
        .base_url();
    let filename = format!("mooshie_cover_{}.{}", uuid::Uuid::new_v4(), extension);
    let mime = match extension.as_str() {
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "aif" | "aiff" => "audio/aiff",
        _ => "audio/ogg",
    };
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.clone())
        .mime_str(mime)?;
    let form = reqwest::multipart::Form::new()
        .part("image", part)
        .text("type", "input")
        .text("overwrite", "false");
    let uploaded: Value = state
        .http_client
        .post(format!("{endpoint}/upload/image"))
        .multipart(form)
        .timeout(Duration::from_secs(120))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if uploaded["name"] != filename
        || uploaded["subfolder"].as_str().unwrap_or("") != ""
        || uploaded["type"] != "input"
    {
        return Err(AppError::Other(
            "Unexpected source recording upload path.".into(),
        ));
    }
    state.free_llm_vram_for_generation().await;
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt_to_worker(
            worker.worker_id,
            music::build_transcription(&request.encoder, &filename, &request.mode),
            &state.client_id,
        )
        .await?;
    state.prompt_queue.mark_music(&response.prompt_id);
    state.prompt_queue.insert(&response.prompt_id, owner);
    state
        .prompt_queue
        .set_worker(&response.prompt_id, worker_id);
    state.broadcast_queue_positions();
    Ok(
        json!({"prompt_id": response.prompt_id, "worker_id": worker_id, "seed": "0", "kind": "transcription", "backend": "native-sheetsage2"}),
    )
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
        .map(|worker| worker.base_url().trim_end_matches('/').to_string())
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
        || !crate::commands::api::is_single_safe_filename(filename)
        || output["subfolder"] != "audio"
        || output["type"] != "output"
    {
        return Err(AppError::Other("Unexpected YuE2 audio output path".into()));
    }
    Ok(Some(output))
}

fn output_text(entry: &Value, node: &str) -> String {
    entry["outputs"][node]["text"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n")
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
    let abc = output_text(entry, "9");
    if abc.len() > 131072 {
        return Err(AppError::Other("Returned score exceeds 128 KiB.".into()));
    }
    let receipt: Value = serde_json::from_str(&output_text(entry, "12")).unwrap_or(Value::Null);
    let planner: Value = serde_json::from_str(&output_text(entry, "11")).unwrap_or(Value::Null);
    let semantic: Value = serde_json::from_str(&output_text(entry, "10")).unwrap_or(Value::Null);
    let metadata = json!({"abc_truncated": planner["abc_truncated"].as_bool(),
        "semantic_truncated": semantic["semantic_truncated"].as_bool(),
        "score_source": receipt["score_source"].as_str(),
        "abc_tokens": planner["abc_tokens"].as_u64(),
        "semantic_frames": semantic["semantic_frames"].as_u64(),
        "generated_seconds": semantic["generated_seconds"].as_f64(),
        "cfg_scale": semantic["cfg_scale"].as_f64()});
    if matches!(receipt["kind"].as_str(), Some("plan" | "transcription")) {
        if abc.trim().is_empty() {
            return Ok(json!({"status": "error", "error": "The worker returned no score."}));
        }
        return Ok(
            json!({"status": "completed", "kind": receipt["kind"], "abc": abc, "metadata": metadata}),
        );
    }
    let Some(audio) = audio_output(entry)? else {
        return Ok(
            json!({"status": "error", "error": "ComfyUI finished without a YuE2 audio file."}),
        );
    };
    Ok(
        json!({"status": "completed", "kind": "audio", "filename": audio["filename"], "abc": abc, "metadata": metadata}),
    )
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
pub async fn transcribe_music_native(
    state: State<'_, Arc<AppState>>,
    request: NativeCoverRequest,
) -> Result<Value, AppError> {
    transcribe_native(&state, request, None).await
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
pub async fn save_music_file(data_base64: String, path: String) -> Result<(), AppError> {
    let extension = std::path::Path::new(&path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["abc", "mid", "svg", "zip"].contains(&extension.as_str())
        || data_base64.len() > 256 * 1024 * 1024 / 3 * 4 + 4
    {
        return Err(AppError::Other(
            "Choose an ABC, MIDI, SVG or ZIP file, at most 256 MiB.".into(),
        ));
    }
    let bytes = STANDARD
        .decode(data_base64)
        .map_err(|_| AppError::Other("Invalid encoded music file.".into()))?;
    let valid = match extension.as_str() {
        "mid" => bytes.starts_with(b"MThd"),
        "zip" => bytes.starts_with(b"PK\x03\x04"),
        "svg" => bytes.starts_with(b"<svg "),
        "abc" => std::str::from_utf8(&bytes).is_ok(),
        _ => false,
    };
    if !valid {
        return Err(AppError::Other(
            "Music file contents do not match the selected extension.".into(),
        ));
    }
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

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
    #[test]
    fn discovers_audio_encoders_from_current_and_legacy_combos() {
        // Current AudioEncoderLoader response, verified against a running
        // ComfyUI with the downloaded SheetSage2 encoder installed.
        for field in [
            json!(["COMBO", {"multiselect": false, "options": [
                "sheetsage2_bf16.safetensors", "other_encoder.safetensors"
            ]}]),
            json!([
                ["sheetsage2_bf16.safetensors", "other_encoder.safetensors"],
                {}
            ]),
        ] {
            let mut info = json!({"AudioEncoderLoader": {"input": {"required": {
                "audio_encoder_name": field
            }}}});
            for node in music::COVER_NODES {
                if info[*node].is_null() {
                    info[*node] = json!({});
                }
            }
            let caps = capabilities_from_info(&info);
            assert!(caps.cover_missing_nodes.is_empty());
            assert_eq!(caps.audio_encoders, vec!["sheetsage2_bf16.safetensors"]);
        }
    }

    #[test]
    fn model_choices_reject_missing_or_non_combo_options() {
        for field in [
            Value::Null,
            json!(["COMBO", {}]),
            json!(["COMBO", {"options": "sheetsage2_bf16.safetensors"}]),
            json!(["STRING", {"options": ["sheetsage2_bf16.safetensors"]}]),
        ] {
            let info = json!({"AudioEncoderLoader": {"input": {"required": {
                "audio_encoder_name": field
            }}}});
            assert!(capabilities_from_info(&info).audio_encoders.is_empty());
        }
    }

    #[test]
    fn plan_transcription_and_audio_receipts_keep_unknown_flags_unknown() {
        for kind in ["plan", "transcription"] {
            let mut entry = json!({"status":{"status_str":"success"},"outputs":{
                "9":{"text":["X:1\nK:C\nCDEF"]},"12":{"text":[json!({"kind":kind,"score_source":"generated"}).to_string()]},
                "11":{"text":["{\"abc_truncated\":true,\"abc_tokens\":123}"]}
            }});
            let result = completed_status(&entry).unwrap();
            assert_eq!(result["status"], "completed");
            assert_eq!(result["kind"], kind);
            assert_eq!(result["metadata"]["abc_truncated"], true);
            assert!(result["metadata"]["semantic_truncated"].is_null());
            entry["outputs"]["9"]["text"] = json!([""]);
            assert_eq!(completed_status(&entry).unwrap()["status"], "error");
        }
        let entry = json!({"outputs":{
            "8":{"audio":[{"filename":"mooshie_yue2_test.flac","subfolder":"audio","type":"output"}]},
            "10":{"text":["{\"semantic_truncated\":false,\"generated_seconds\":31.2,\"cfg_scale\":1.01}"]}
        }});
        let result = completed_status(&entry).unwrap();
        assert_eq!(result["metadata"]["semantic_truncated"], false);
        assert!(result["metadata"]["abc_truncated"].is_null());
        assert_eq!(result["metadata"]["generated_seconds"], 31.2);
    }

    #[tokio::test]
    async fn music_project_exports_validate_extension_before_writing() {
        let root = std::env::temp_dir().join(format!("music-export-{}", uuid::Uuid::new_v4()));
        let path = root.with_extension("mid");
        save_music_file(STANDARD.encode(b"MThdtest"), path.to_string_lossy().into())
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"MThdtest");
        assert!(
            save_music_file(STANDARD.encode(b"wrong"), path.to_string_lossy().into())
                .await
                .is_err()
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"MThdtest");
        assert!(save_music_file(
            STANDARD.encode(b"bad"),
            root.with_extension("exe").to_string_lossy().into()
        )
        .await
        .is_err());
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn exact_music_worker_never_falls_back_and_preserves_running_queue_on_error() {
        use crate::comfyui::gpu_manager::WorkerStatus;
        use axum::{routing::post, Json, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = Router::new().route(
            "/prompt",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["prompt"]["test"], true);
                "invalid JSON"
            }),
        );
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
        let worker = &state.gpu_manager.workers[1];
        *worker.status.write().await = WorkerStatus::Idle;
        assert!(state
            .gpu_manager
            .submit_prompt_to_worker(1, json!({"test":true}), "test")
            .await
            .is_err());
        assert!(!worker.reserved.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(*worker.status.read().await, WorkerStatus::Idle);
        *worker.status.write().await = WorkerStatus::Running;
        assert!(worker.try_reserve());
        assert!(state
            .gpu_manager
            .submit_prompt_to_worker(1, json!({"test":true}), "test")
            .await
            .is_err());
        assert!(worker.reserved.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(*worker.status.read().await, WorkerStatus::Running);
        *worker.status.write().await = WorkerStatus::Error;
        assert!(state
            .gpu_manager
            .submit_prompt_to_worker(1, json!({"test":true}), "test")
            .await
            .is_err());
        assert!(state
            .gpu_manager
            .submit_prompt_to_worker(99, json!({"test":true}), "test")
            .await
            .is_err());
        server.abort();
    }

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
            "mooshie_yue2_\\..\\secret.flac",
            "mooshie_yue2_\0.flac",
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
