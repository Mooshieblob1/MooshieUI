//! Image to 3D (Gaussian splat) IPC shared by desktop and browser clients.
//!
//! Follows the music pattern: the 3D page submits one graph to one worker,
//! polls that worker's history for the result, and reads the `.ply` back only
//! from the history of the caller's own prompt. The library lives in the
//! client (IndexedDB), so nothing here writes to the host except the desktop
//! export, which takes its path from the native Save dialog.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
#[cfg(feature = "desktop")]
use {std::sync::Arc, tauri::State};

use crate::{
    commands::music::model_names,
    error::AppError,
    state::AppState,
    templates::splat::{self, SplatModels, SplatParams},
};

/// One worker's view of the splat stack.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SplatWorkerCapabilities {
    pub worker_id: u32,
    pub missing_nodes: Vec<String>,
    pub models: Vec<String>,
    pub clip_visions: Vec<String>,
    pub decoders: Vec<String>,
    pub flux2_vaes: Vec<String>,
    pub bg_models: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct SplatCapabilities {
    /// Empty when at least one worker has every node.
    pub missing_nodes: Vec<String>,
    /// Union over workers that have every node.
    pub models: Vec<String>,
    pub clip_visions: Vec<String>,
    pub decoders: Vec<String>,
    pub flux2_vaes: Vec<String>,
    pub bg_models: Vec<String>,
    pub workers: Vec<SplatWorkerCapabilities>,
}

pub(crate) fn worker_capabilities_from_info(
    info: &Value,
    worker_id: u32,
) -> SplatWorkerCapabilities {
    let filtered = |node: &str, field: &str, keep: fn(&str) -> bool| -> Vec<String> {
        let mut names: Vec<String> = model_names(info, node, field)
            .into_iter()
            .filter(|n| keep(n))
            .collect();
        names.sort();
        names
    };
    SplatWorkerCapabilities {
        worker_id,
        missing_nodes: splat::REQUIRED_NODES
            .iter()
            .filter(|name| !info[**name].is_object())
            .map(|name| name.to_string())
            .collect(),
        models: filtered("UNETLoader", "unet_name", splat::is_splat_unet),
        clip_visions: filtered("CLIPVisionLoader", "clip_name", splat::is_dinov3),
        decoders: filtered("VAELoader", "vae_name", splat::is_splat_decoder),
        flux2_vaes: filtered("VAELoader", "vae_name", splat::is_flux2_vae),
        bg_models: filtered(
            "LoadBackgroundRemovalModel",
            "bg_removal_name",
            splat::is_birefnet,
        ),
    }
}

pub(crate) fn merge_capabilities(workers: Vec<SplatWorkerCapabilities>) -> SplatCapabilities {
    let complete: Vec<&SplatWorkerCapabilities> = workers
        .iter()
        .filter(|w| w.missing_nodes.is_empty())
        .collect();
    let union = |pick: fn(&SplatWorkerCapabilities) -> &Vec<String>| -> Vec<String> {
        let mut all: Vec<String> = complete.iter().flat_map(|w| pick(w).clone()).collect();
        all.sort();
        all.dedup();
        all
    };
    SplatCapabilities {
        missing_nodes: if complete.is_empty() {
            workers
                .first()
                .map(|w| w.missing_nodes.clone())
                .unwrap_or_else(|| {
                    splat::REQUIRED_NODES
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                })
        } else {
            Vec::new()
        },
        models: union(|w| &w.models),
        clip_visions: union(|w| &w.clip_visions),
        decoders: union(|w| &w.decoders),
        flux2_vaes: union(|w| &w.flux2_vaes),
        bg_models: union(|w| &w.bg_models),
        workers,
    }
}

/// The files one worker would use for `params`, or why it can't run them.
pub(crate) fn resolve_models(
    worker: &SplatWorkerCapabilities,
    params: &SplatParams,
) -> Result<SplatModels, String> {
    if !worker.missing_nodes.is_empty() {
        return Err(format!(
            "This ComfyUI is missing {}.",
            worker.missing_nodes.join(", ")
        ));
    }
    if !worker.models.contains(&params.model) {
        return Err("The selected 3D model is not installed on this worker.".into());
    }
    let clip_vision = worker
        .clip_visions
        .first()
        .ok_or("The DINOv3 image encoder is missing from models/clip_vision.")?;
    let decoder = splat::pick_decoder(&params.model, &worker.decoders)
        .ok_or("The splat decoder is missing from models/vae.")?;
    let flux2_vae = worker
        .flux2_vaes
        .first()
        .ok_or("The FLUX.2 VAE is missing from models/vae.")?;
    let bg_removal = if params.remove_background {
        Some(
            worker
                .bg_models
                .first()
                .ok_or("Background removal needs BiRefNet in models/background_removal. Download it, or turn background removal off.")?
                .clone(),
        )
    } else {
        None
    };
    Ok(SplatModels {
        unet: params.model.clone(),
        clip_vision: clip_vision.clone(),
        decoder: decoder.clone(),
        flux2_vae: flux2_vae.clone(),
        bg_removal,
    })
}

/// `/object_info` of every reachable worker, idle workers first so a free
/// GPU takes the job. Shared with the Pixal3D mesh page.
pub(crate) async fn worker_infos(state: &AppState) -> Result<Vec<(u32, Value)>, AppError> {
    use crate::comfyui::gpu_manager::WorkerStatus;
    crate::comfyui::process::mark_legacy_worker_idle(state).await;
    let candidates =
        futures_util::future::join_all(state.gpu_manager.workers.iter().map(|worker| async move {
            let status = *worker.status.read().await;
            if !matches!(status, WorkerStatus::Idle | WorkerStatus::Running) {
                return None;
            }
            let info: Value = state
                .http_client
                .get(format!("{}/object_info", worker.base_url()))
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?
                .json()
                .await
                .ok()?;
            Some((status == WorkerStatus::Running, worker.id, info))
        }))
        .await;
    let mut candidates: Vec<_> = candidates.into_iter().flatten().collect();
    if candidates.is_empty() {
        return Err(AppError::ConnectionFailed(
            "No ComfyUI worker is reachable.".into(),
        ));
    }
    candidates.sort_by_key(|(running, _, _)| *running);
    Ok(candidates
        .into_iter()
        .map(|(_, id, info)| (id, info))
        .collect())
}

pub(crate) async fn capabilities(state: &AppState) -> Result<SplatCapabilities, AppError> {
    Ok(merge_capabilities(
        worker_infos(state)
            .await?
            .iter()
            .map(|(id, info)| worker_capabilities_from_info(info, *id))
            .collect(),
    ))
}

const MAX_INPUT_BYTES: usize = 40 * 1024 * 1024;

/// Decode the source picture and return its bytes plus extension. Only the
/// formats ComfyUI's LoadImage reads reliably are accepted.
pub(crate) fn decode_input_image(image_base64: &str) -> Result<(Vec<u8>, &'static str), AppError> {
    if image_base64.len() > MAX_INPUT_BYTES / 3 * 4 + 4 {
        return Err(AppError::Other("The image is larger than 40 MiB.".into()));
    }
    let bytes = STANDARD
        .decode(image_base64.trim())
        .map_err(|_| AppError::Other("Invalid encoded image.".into()))?;
    let ext = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else {
        return Err(AppError::Other(
            "Use a PNG, JPEG or WebP image for 3D.".into(),
        ));
    };
    Ok((bytes, ext))
}

/// Upload the source picture to the worker that will run the job, under a
/// fresh name with `prefix`, and return that name.
pub(crate) async fn upload_input(
    state: &AppState,
    worker_id: u32,
    bytes: Vec<u8>,
    ext: &str,
    prefix: &str,
) -> Result<String, AppError> {
    let endpoint = state
        .gpu_manager
        .workers
        .iter()
        .find(|w| w.id == worker_id)
        .ok_or_else(|| AppError::ConnectionFailed("The 3D worker is unavailable.".into()))?
        .base_url();
    let filename = format!("{prefix}{}.{ext}", uuid::Uuid::new_v4());
    let mime = match ext {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        _ => "image/webp",
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
        || !uploaded["subfolder"].as_str().unwrap_or("").is_empty()
        || uploaded["type"] != "input"
    {
        return Err(AppError::Other(
            "Unexpected 3D source image upload path.".into(),
        ));
    }
    Ok(filename)
}

pub(crate) async fn submit(
    state: &AppState,
    params: SplatParams,
    image_base64: &str,
    owner: Option<String>,
) -> Result<Value, AppError> {
    splat::validate(&params).map_err(AppError::InvalidWorkflow)?;
    let (bytes, ext) = decode_input_image(image_base64)?;
    let caps = capabilities(state).await?;
    let mut last_error = None;
    let mut chosen = None;
    for worker in &caps.workers {
        match resolve_models(worker, &params) {
            Ok(models) => {
                chosen = Some((worker.worker_id, models));
                break;
            }
            Err(e) => last_error = Some(e),
        }
    }
    let (worker_id, models) = chosen.ok_or_else(|| {
        AppError::InvalidWorkflow(last_error.unwrap_or_else(|| {
            "No ComfyUI worker can run image to 3D. Refresh the 3D page.".into()
        }))
    })?;
    let filename = upload_input(state, worker_id, bytes, ext, "mooshie_splat_in_").await?;
    let seed = if params.seed < 0 {
        (rand::random::<u64>() >> 1) as i64
    } else {
        params.seed
    };
    state.free_llm_vram_for_generation().await;
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt_to_worker(
            worker_id,
            splat::build(&params, &models, &filename, seed),
            &state.client_id,
        )
        .await?;
    state.prompt_queue.mark_splat(&response.prompt_id);
    state.prompt_queue.insert(&response.prompt_id, owner);
    state
        .prompt_queue
        .set_worker(&response.prompt_id, worker_id);
    state.broadcast_queue_positions();
    Ok(json!({
        "prompt_id": response.prompt_id,
        "worker_id": worker_id,
        "seed": seed.to_string(),
        "model": models.unet,
    }))
}

pub(crate) fn worker_url(
    state: &AppState,
    worker_id: u32,
    prompt_id: &str,
) -> Result<String, AppError> {
    if prompt_id.is_empty()
        || prompt_id.len() > 128
        || !prompt_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(AppError::Other("Invalid 3D prompt ID".into()));
    }
    state
        .gpu_manager
        .workers
        .iter()
        .find(|worker| worker.id == worker_id)
        .map(|worker| worker.base_url().trim_end_matches('/').to_string())
        .ok_or_else(|| AppError::Other("The 3D GPU worker is no longer configured.".into()))
}

pub(crate) async fn history(
    state: &AppState,
    base: &str,
    prompt_id: &str,
) -> Result<Value, AppError> {
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

/// The saved `.ply`, if this history entry has one at the expected path.
pub(crate) fn ply_output(entry: &Value) -> Result<Option<&Value>, AppError> {
    let Some(output) = entry["outputs"][splat::NODE_SAVE]["3d"]
        .as_array()
        .and_then(|a| a.first())
    else {
        return Ok(None);
    };
    let filename = output["filename"].as_str().unwrap_or("");
    if !filename.starts_with(splat::OUTPUT_PREFIX)
        || !filename.ends_with(".ply")
        || !crate::commands::api::is_single_safe_filename(filename)
        || output["subfolder"] != splat::OUTPUT_SUBFOLDER
        || output["type"] != "output"
    {
        return Err(AppError::Other("Unexpected 3D output path".into()));
    }
    Ok(Some(output))
}

pub(crate) fn completed_status(entry: &Value) -> Result<Value, AppError> {
    if entry["status"]["status_str"] == "error" {
        let error = entry["status"]["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|message| message[1]["exception_message"].as_str())
            .unwrap_or("3D generation was interrupted or failed.");
        return Ok(json!({"status": "error", "error": error}));
    }
    match ply_output(entry)? {
        Some(output) => Ok(json!({"status": "completed", "filename": output["filename"]})),
        None => Ok(json!({"status": "error", "error": "ComfyUI finished without a 3D file."})),
    }
}

pub(crate) async fn status(
    state: &AppState,
    prompt_id: &str,
    worker_id: u32,
) -> Result<Value, AppError> {
    status_with(state, prompt_id, worker_id, completed_status).await
}

/// Job status from the worker's history (final) or queue. `complete` turns a
/// finished history entry into the reply. Shared with the mesh page.
pub(crate) async fn status_with(
    state: &AppState,
    prompt_id: &str,
    worker_id: u32,
    complete: fn(&Value) -> Result<Value, AppError>,
) -> Result<Value, AppError> {
    let base = worker_url(state, worker_id, prompt_id)?;
    let history = history(state, &base, prompt_id).await?;
    if let Some(entry) = history.get(prompt_id) {
        // History is final. Reconcile here too, so a lost websocket event
        // cannot keep the worker reserved after the job is done.
        if state.prompt_queue.is_splat(prompt_id) {
            if let Some(worker) = state.prompt_queue.finish(prompt_id) {
                state.gpu_manager.mark_worker_idle(worker).await;
                state.broadcast_queue_positions();
                state.prompt_queue.drain_notify.notify_one();
            }
        }
        return complete(entry);
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
    Ok(json!({"status": "missing"}))
}

/// Download one output file from the worker, refusing anything over `limit`.
pub(crate) async fn fetch_output(
    state: &AppState,
    base: &str,
    filename: &str,
    subfolder: &str,
    limit: usize,
) -> Result<Vec<u8>, AppError> {
    let mut response = state
        .http_client
        .get(format!("{base}/view"))
        .query(&[
            ("filename", filename),
            ("subfolder", subfolder),
            ("type", "output"),
        ])
        .timeout(Duration::from_secs(300))
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > limit {
            return Err(AppError::Other(format!(
                "The 3D file exceeds {} MiB. Retrieve it from ComfyUI's output/3d folder.",
                limit / (1024 * 1024)
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// 1,048,576 Gaussians at SH degree 0 is about 71 MB of PLY.
const PLY_LIMIT: usize = 128 * 1024 * 1024;

/// A binary little-endian PLY with a vertex element, the only kind
/// `SplatToFile3D` writes.
pub(crate) fn is_splat_ply(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let Some(end) = head.windows(11).position(|w| w == b"end_header\n") else {
        return false;
    };
    let header = String::from_utf8_lossy(&head[..end]);
    bytes.starts_with(b"ply\n")
        && header.contains("format binary_little_endian 1.0")
        && header.contains("element vertex ")
}

pub(crate) async fn ply(
    state: &AppState,
    prompt_id: &str,
    worker_id: u32,
) -> Result<String, AppError> {
    let base = worker_url(state, worker_id, prompt_id)?;
    let history = history(state, &base, prompt_id).await?;
    let output = ply_output(&history[prompt_id])?.ok_or_else(|| {
        AppError::Other("The 3D file is not available in ComfyUI history.".into())
    })?;
    let bytes = fetch_output(
        state,
        &base,
        output["filename"].as_str().unwrap_or(""),
        splat::OUTPUT_SUBFOLDER,
        PLY_LIMIT,
    )
    .await?;
    if !is_splat_ply(&bytes) {
        return Err(AppError::Other(
            "ComfyUI did not return a Gaussian splat PLY file.".into(),
        ));
    }
    Ok(STANDARD.encode(bytes))
}

/// Desktop export. The path comes from the native Save dialog; browser
/// clients download their Blob locally instead.
#[cfg(any(feature = "desktop", test))]
pub(crate) async fn save_file(data_base64: &str, path: &str) -> Result<(), AppError> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["ply", "splat"].contains(&extension.as_str()) || data_base64.len() > PLY_LIMIT / 3 * 4 + 4
    {
        return Err(AppError::Other(
            "Choose a PLY or SPLAT file, at most 128 MiB.".into(),
        ));
    }
    let bytes = STANDARD
        .decode(data_base64)
        .map_err(|_| AppError::Other("Invalid encoded 3D file.".into()))?;
    let valid = match extension.as_str() {
        "ply" => is_splat_ply(&bytes),
        // antimatter15 .splat: 32 bytes per Gaussian, no header.
        _ => !bytes.is_empty() && bytes.len() % 32 == 0,
    };
    if !valid {
        return Err(AppError::Other(
            "3D file contents do not match the selected extension.".into(),
        ));
    }
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_splat_capabilities(
    state: State<'_, Arc<AppState>>,
) -> Result<SplatCapabilities, AppError> {
    capabilities(&state).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn generate_splat(
    state: State<'_, Arc<AppState>>,
    params: SplatParams,
    image_base64: String,
) -> Result<Value, AppError> {
    submit(&state, params, &image_base64, None).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_splat_status(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
    worker_id: u32,
) -> Result<Value, AppError> {
    status(&state, &prompt_id, worker_id).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn load_splat_ply(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
    worker_id: u32,
) -> Result<String, AppError> {
    ply(&state, &prompt_id, worker_id).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_splat_file(data_base64: String, path: String) -> Result<(), AppError> {
    save_file(&data_base64, &path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_info() -> Value {
        let mut info = json!({});
        for node in splat::REQUIRED_NODES {
            info[*node] = json!({"input": {"required": {}}});
        }
        info["UNETLoader"]["input"]["required"]["unet_name"] = json!([
            [
                "idol_fp16.safetensors",
                "flux2_klein_9b.safetensors",
                "triposplat_fp16.safetensors"
            ],
            {}
        ]);
        info["CLIPVisionLoader"]["input"]["required"]["clip_name"] = json!([
            [
                "clip_vision_g.safetensors",
                "dino_v3_vit_h_bf16.safetensors"
            ],
            {}
        ]);
        info["VAELoader"]["input"]["required"]["vae_name"] = json!([
            [
                "ae.safetensors",
                "flux2-vae.safetensors",
                "idol_decoder_fp16.safetensors"
            ],
            {}
        ]);
        // New-schema combo, like the real node.
        info["LoadBackgroundRemovalModel"]["input"]["required"]["bg_removal_name"] =
            json!(["COMBO", {"options": ["birefnet.safetensors"]}]);
        info
    }

    fn params() -> SplatParams {
        serde_json::from_value(json!({"model": "idol_fp16.safetensors", "seed": "-1"})).unwrap()
    }

    #[test]
    fn reads_models_from_object_info() {
        let w = worker_capabilities_from_info(&object_info(), 3);
        assert!(w.missing_nodes.is_empty());
        assert_eq!(
            w.models,
            vec!["idol_fp16.safetensors", "triposplat_fp16.safetensors"]
        );
        assert_eq!(w.clip_visions, vec!["dino_v3_vit_h_bf16.safetensors"]);
        assert_eq!(w.decoders, vec!["idol_decoder_fp16.safetensors"]);
        assert_eq!(w.flux2_vaes, vec!["flux2-vae.safetensors"]);
        assert_eq!(w.bg_models, vec!["birefnet.safetensors"]);
        let models = resolve_models(&w, &params()).unwrap();
        assert_eq!(models.decoder, "idol_decoder_fp16.safetensors");
        assert_eq!(models.bg_removal.as_deref(), Some("birefnet.safetensors"));
    }

    #[test]
    fn reports_missing_nodes_and_files() {
        let w = worker_capabilities_from_info(&json!({}), 0);
        assert_eq!(w.missing_nodes.len(), splat::REQUIRED_NODES.len());
        let caps = merge_capabilities(vec![w.clone()]);
        assert!(!caps.missing_nodes.is_empty());
        assert!(caps.models.is_empty());
        assert!(resolve_models(&w, &params()).is_err());

        let mut info = object_info();
        info["LoadBackgroundRemovalModel"]["input"]["required"]["bg_removal_name"] =
            json!(["COMBO", {"options": []}]);
        let w = worker_capabilities_from_info(&info, 0);
        let err = resolve_models(&w, &params()).unwrap_err();
        assert!(err.contains("BiRefNet"), "{err}");
        let no_bg = SplatParams {
            remove_background: false,
            ..params()
        };
        assert!(resolve_models(&w, &no_bg).unwrap().bg_removal.is_none());
    }

    #[test]
    fn merge_ignores_files_on_workers_without_the_nodes() {
        let full = worker_capabilities_from_info(&object_info(), 0);
        let mut partial = worker_capabilities_from_info(&object_info(), 1);
        partial.missing_nodes = vec!["TripoSplatConditioning".into()];
        partial.models = vec!["idol_bf16.safetensors".into()];
        let caps = merge_capabilities(vec![partial, full]);
        assert!(caps.missing_nodes.is_empty());
        assert!(!caps.models.contains(&"idol_bf16.safetensors".to_string()));
        assert_eq!(caps.workers.len(), 2);
    }

    #[test]
    fn accepts_only_image_inputs() {
        let png = STANDARD.encode(b"\x89PNG\r\n\x1a\nrest");
        assert_eq!(decode_input_image(&png).unwrap().1, "png");
        let jpg = STANDARD.encode([0xFF, 0xD8, 0xFF, 0xE0, 0, 0]);
        assert_eq!(decode_input_image(&jpg).unwrap().1, "jpg");
        let webp = STANDARD.encode(b"RIFF\0\0\0\0WEBPVP8 ");
        assert_eq!(decode_input_image(&webp).unwrap().1, "webp");
        assert!(decode_input_image(&STANDARD.encode(b"GIF89a....")).is_err());
        assert!(decode_input_image("not base64!").is_err());
    }

    #[test]
    fn reads_the_ply_output_and_rejects_other_paths() {
        let ok = json!({"outputs": {"14": {"3d": [
            {"filename": "mooshie_splat_00001_.ply", "subfolder": "3d", "type": "output"}]}}});
        assert_eq!(
            completed_status(&ok).unwrap(),
            json!({"status": "completed", "filename": "mooshie_splat_00001_.ply"})
        );
        for bad in [
            json!({"filename": "../secret.ply", "subfolder": "3d", "type": "output"}),
            json!({"filename": "mooshie_splat_00001_.ply", "subfolder": "..", "type": "output"}),
            json!({"filename": "mooshie_splat_00001_.glb", "subfolder": "3d", "type": "output"}),
            json!({"filename": "other_00001_.ply", "subfolder": "3d", "type": "output"}),
            json!({"filename": "mooshie_splat_00001_.ply", "subfolder": "3d", "type": "temp"}),
        ] {
            let entry = json!({"outputs": {"14": {"3d": [bad]}}});
            assert!(completed_status(&entry).is_err());
        }
        let missing = completed_status(&json!({"outputs": {}})).unwrap();
        assert_eq!(missing["status"], "error");
        let failed = json!({"status": {"status_str": "error", "messages": [
            ["execution_error", {"exception_message": "CUDA out of memory"}]]}});
        assert_eq!(
            completed_status(&failed).unwrap()["error"],
            "CUDA out of memory"
        );
    }

    fn sample_ply() -> Vec<u8> {
        let mut bytes = b"ply\nformat binary_little_endian 1.0\nelement vertex 1\nproperty float x\nend_header\n".to_vec();
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes
    }

    #[test]
    fn recognises_splat_plys() {
        assert!(is_splat_ply(&sample_ply()));
        assert!(!is_splat_ply(
            b"ply\nformat ascii 1.0\nelement vertex 1\nend_header\n"
        ));
        assert!(!is_splat_ply(b"glTF"));
    }

    #[tokio::test]
    async fn export_validates_extension_and_contents() {
        let dir = std::env::temp_dir().join(format!("mooshie-splat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let ply = dir.join("a.ply");
        save_file(&STANDARD.encode(sample_ply()), ply.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(std::fs::read(&ply).unwrap(), sample_ply());
        let splat = dir.join("a.splat");
        save_file(&STANDARD.encode([0u8; 64]), splat.to_str().unwrap())
            .await
            .unwrap();
        assert!(
            save_file(&STANDARD.encode([0u8; 33]), splat.to_str().unwrap())
                .await
                .is_err()
        );
        assert!(save_file(&STANDARD.encode(b"hello"), ply.to_str().unwrap())
            .await
            .is_err());
        let exe = dir.join("a.exe");
        assert!(
            save_file(&STANDARD.encode(sample_ply()), exe.to_str().unwrap())
                .await
                .is_err()
        );
        assert!(!exe.exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
