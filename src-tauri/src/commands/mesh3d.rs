//! Image to textured 3D mesh (Pixal3D) IPC shared by desktop and browser
//! clients.
//!
//! Same job model as the splat page (`commands::splat`): one graph on one
//! worker, polled through that worker's history, with the `.glb` read back
//! only from the caller's own prompt. The library lives in the client.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use serde_json::{json, Value};
#[cfg(feature = "desktop")]
use {std::sync::Arc, tauri::State};

use crate::{
    commands::{music::model_names, splat},
    error::AppError,
    state::AppState,
    templates::mesh3d::{self, MeshModels, MeshParams},
};

/// One worker's view of the Pixal3D stack.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct MeshWorkerCapabilities {
    pub worker_id: u32,
    pub missing_nodes: Vec<String>,
    pub models: Vec<String>,
    pub clip_visions: Vec<String>,
    pub shape_vaes: Vec<String>,
    pub texture_vaes: Vec<String>,
    pub moge_models: Vec<String>,
    pub bg_models: Vec<String>,
    /// `RemeshMesh` has the "solid" sign mode (ComfyUI v0.43+).
    pub solid_remesh: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct MeshCapabilities {
    /// Empty when at least one worker has every node.
    pub missing_nodes: Vec<String>,
    /// Union over workers that have every node.
    pub models: Vec<String>,
    pub clip_visions: Vec<String>,
    pub shape_vaes: Vec<String>,
    pub texture_vaes: Vec<String>,
    pub moge_models: Vec<String>,
    pub bg_models: Vec<String>,
    pub workers: Vec<MeshWorkerCapabilities>,
}

pub(crate) fn worker_capabilities_from_info(
    info: &Value,
    worker_id: u32,
) -> MeshWorkerCapabilities {
    let filtered = |node: &str, field: &str, keep: fn(&str) -> bool| -> Vec<String> {
        let mut names: Vec<String> = model_names(info, node, field)
            .into_iter()
            .filter(|n| keep(n))
            .collect();
        names.sort();
        names
    };
    MeshWorkerCapabilities {
        worker_id,
        missing_nodes: mesh3d::REQUIRED_NODES
            .iter()
            .filter(|name| !info[**name].is_object())
            .map(|name| name.to_string())
            .collect(),
        models: filtered("UNETLoader", "unet_name", mesh3d::is_pixal3d_unet),
        clip_visions: filtered("CLIPVisionLoader", "clip_name", mesh3d::is_dinov3_naf),
        shape_vaes: filtered("VAELoader", "vae_name", mesh3d::is_trellis2_shape_vae),
        texture_vaes: filtered("VAELoader", "vae_name", mesh3d::is_trellis2_texture_vae),
        moge_models: filtered("LoadMoGeModel", "model_name", mesh3d::is_moge),
        bg_models: filtered(
            "LoadBackgroundRemovalModel",
            "bg_removal_name",
            crate::templates::splat::is_birefnet,
        ),
        solid_remesh: mesh3d::has_solid_remesh(info),
    }
}

pub(crate) fn merge_capabilities(workers: Vec<MeshWorkerCapabilities>) -> MeshCapabilities {
    let complete: Vec<&MeshWorkerCapabilities> = workers
        .iter()
        .filter(|w| w.missing_nodes.is_empty())
        .collect();
    let union = |pick: fn(&MeshWorkerCapabilities) -> &Vec<String>| -> Vec<String> {
        let mut all: Vec<String> = complete.iter().flat_map(|w| pick(w).clone()).collect();
        all.sort();
        all.dedup();
        all
    };
    MeshCapabilities {
        missing_nodes: if complete.is_empty() {
            workers
                .first()
                .map(|w| w.missing_nodes.clone())
                .unwrap_or_else(|| {
                    mesh3d::REQUIRED_NODES
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                })
        } else {
            Vec::new()
        },
        models: union(|w| &w.models),
        clip_visions: union(|w| &w.clip_visions),
        shape_vaes: union(|w| &w.shape_vaes),
        texture_vaes: union(|w| &w.texture_vaes),
        moge_models: union(|w| &w.moge_models),
        bg_models: union(|w| &w.bg_models),
        workers,
    }
}

/// The files one worker would use for `params`, or why it can't run them.
pub(crate) fn resolve_models(
    worker: &MeshWorkerCapabilities,
    params: &MeshParams,
) -> Result<MeshModels, String> {
    if !worker.missing_nodes.is_empty() {
        return Err(format!(
            "This ComfyUI is missing {}.",
            worker.missing_nodes.join(", ")
        ));
    }
    if !worker.models.contains(&params.model) {
        return Err("The selected Pixal3D model is not installed on this worker.".into());
    }
    let first = |list: &Vec<String>, what: &str| -> Result<String, String> {
        list.first()
            .cloned()
            .ok_or_else(|| format!("{what} is missing."))
    };
    let bg_removal = if params.remove_background {
        Some(first(
            &worker.bg_models,
            "Background removal needs BiRefNet in models/background_removal. It",
        )?)
    } else {
        None
    };
    Ok(MeshModels {
        unet: params.model.clone(),
        clip_vision: first(
            &worker.clip_visions,
            "The DINOv3-L (NAF) encoder in models/clip_vision",
        )?,
        shape_vae: first(&worker.shape_vaes, "The TRELLIS.2 shape VAE in models/vae")?,
        texture_vae: first(
            &worker.texture_vaes,
            "The TRELLIS.2 texture VAE in models/vae",
        )?,
        moge: mesh3d::pick_moge(&worker.moge_models)
            .cloned()
            .ok_or("MoGe in models/geometry_estimation is missing.")?,
        bg_removal,
        solid_remesh: worker.solid_remesh,
    })
}

pub(crate) async fn capabilities(state: &AppState) -> Result<MeshCapabilities, AppError> {
    Ok(merge_capabilities(
        splat::worker_infos(state)
            .await?
            .iter()
            .map(|(id, info)| worker_capabilities_from_info(info, *id))
            .collect(),
    ))
}

pub(crate) async fn submit(
    state: &AppState,
    params: MeshParams,
    image_base64: &str,
    owner: Option<String>,
) -> Result<Value, AppError> {
    mesh3d::validate(&params).map_err(AppError::InvalidWorkflow)?;
    let (bytes, ext) = splat::decode_input_image(image_base64)?;
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
    let (worker_id, models) =
        chosen.ok_or_else(|| {
            AppError::InvalidWorkflow(last_error.unwrap_or_else(|| {
                "No ComfyUI worker can run Pixal3D. Refresh the 3D page.".into()
            }))
        })?;
    let filename = splat::upload_input(state, worker_id, bytes, ext, "mooshie_mesh_in_").await?;
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
            mesh3d::build(&params, &models, &filename, seed),
            &state.client_id,
        )
        .await?;
    // Same queue kind as the splat page: the 3D page owns both.
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

/// The saved `.glb`, if this history entry has one at the expected path.
pub(crate) fn glb_output(entry: &Value) -> Result<Option<&Value>, AppError> {
    let Some(output) = entry["outputs"][mesh3d::NODE_SAVE]["3d"]
        .as_array()
        .and_then(|a| a.first())
    else {
        return Ok(None);
    };
    let filename = output["filename"].as_str().unwrap_or("");
    if !filename.starts_with(mesh3d::OUTPUT_PREFIX)
        || !filename.ends_with(".glb")
        || !crate::commands::api::is_single_safe_filename(filename)
        || output["subfolder"] != mesh3d::OUTPUT_SUBFOLDER
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
    match glb_output(entry)? {
        Some(output) => Ok(json!({"status": "completed", "filename": output["filename"]})),
        None => Ok(json!({"status": "error", "error": "ComfyUI finished without a 3D file."})),
    }
}

pub(crate) async fn status(
    state: &AppState,
    prompt_id: &str,
    worker_id: u32,
) -> Result<Value, AppError> {
    splat::status_with(state, prompt_id, worker_id, completed_status).await
}

/// 700,000 faces with 4096 px base colour and ORM maps and a 2048 px normal
/// map is typically 60 to 120 MB.
const GLB_LIMIT: usize = 384 * 1024 * 1024;

/// A binary glTF 2.0 file whose header length matches its size.
pub(crate) fn is_glb(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return false;
    }
    let word =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    word(4) == 2 && word(8) as usize == bytes.len() && &bytes[16..20] == b"JSON"
}

pub(crate) async fn glb(
    state: &AppState,
    prompt_id: &str,
    worker_id: u32,
) -> Result<String, AppError> {
    let base = splat::worker_url(state, worker_id, prompt_id)?;
    let history = splat::history(state, &base, prompt_id).await?;
    let output = glb_output(&history[prompt_id])?.ok_or_else(|| {
        AppError::Other("The 3D file is not available in ComfyUI history.".into())
    })?;
    let bytes = splat::fetch_output(
        state,
        &base,
        output["filename"].as_str().unwrap_or(""),
        mesh3d::OUTPUT_SUBFOLDER,
        GLB_LIMIT,
    )
    .await?;
    if !is_glb(&bytes) {
        return Err(AppError::Other(
            "ComfyUI did not return a binary glTF (.glb) file.".into(),
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
    if extension != "glb" || data_base64.len() > GLB_LIMIT / 3 * 4 + 4 {
        return Err(AppError::Other(
            "Choose a GLB file, at most 384 MiB.".into(),
        ));
    }
    let bytes = STANDARD
        .decode(data_base64)
        .map_err(|_| AppError::Other("Invalid encoded 3D file.".into()))?;
    if !is_glb(&bytes) {
        return Err(AppError::Other("The 3D file is not a valid GLB.".into()));
    }
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_mesh_capabilities(
    state: State<'_, Arc<AppState>>,
) -> Result<MeshCapabilities, AppError> {
    capabilities(&state).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn generate_mesh(
    state: State<'_, Arc<AppState>>,
    params: MeshParams,
    image_base64: String,
) -> Result<Value, AppError> {
    submit(&state, params, &image_base64, None).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_mesh_status(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
    worker_id: u32,
) -> Result<Value, AppError> {
    status(&state, &prompt_id, worker_id).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn load_mesh_glb(
    state: State<'_, Arc<AppState>>,
    prompt_id: String,
    worker_id: u32,
) -> Result<String, AppError> {
    glb(&state, &prompt_id, worker_id).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_mesh_file(data_base64: String, path: String) -> Result<(), AppError> {
    save_file(&data_base64, &path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::mesh3d::MeshDetail;

    fn object_info() -> Value {
        let mut info = json!({});
        for node in mesh3d::REQUIRED_NODES {
            info[*node] = json!({"input": {"required": {}}});
        }
        info["UNETLoader"]["input"]["required"]["unet_name"] = json!([
            [
                "idol_fp16.safetensors",
                "pixal3d_int8_convrot.safetensors",
                "pixal3d_multiview_int8_convrot.safetensors"
            ],
            {}
        ]);
        info["CLIPVisionLoader"]["input"]["required"]["clip_name"] = json!([
            [
                "dino_v3_L_naf_fp32.safetensors",
                "dino_v3_vit_h_bf16.safetensors"
            ],
            {}
        ]);
        info["VAELoader"]["input"]["required"]["vae_name"] = json!([
            [
                "flux2-vae.safetensors",
                "trellis_2_shape_vae_bf16.safetensors",
                "trellis_2_texture_vae_bf16.safetensors"
            ],
            {}
        ]);
        info["LoadMoGeModel"]["input"]["required"]["model_name"] =
            json!(["COMBO", {"options": ["moge_2_vitl_normal_fp16.safetensors"]}]);
        info["LoadBackgroundRemovalModel"]["input"]["required"]["bg_removal_name"] =
            json!(["COMBO", {"options": ["birefnet.safetensors"]}]);
        info
    }

    fn params() -> MeshParams {
        MeshParams {
            model: "pixal3d_int8_convrot.safetensors".into(),
            remove_background: true,
            detail: MeshDetail::High,
            seed: -1,
        }
    }

    #[test]
    fn capabilities_pick_the_pixal3d_stack() {
        let w = worker_capabilities_from_info(&object_info(), 0);
        assert!(w.missing_nodes.is_empty());
        assert_eq!(w.models, vec!["pixal3d_int8_convrot.safetensors"]);
        assert_eq!(w.clip_visions, vec!["dino_v3_L_naf_fp32.safetensors"]);
        assert_eq!(w.shape_vaes, vec!["trellis_2_shape_vae_bf16.safetensors"]);
        assert_eq!(
            w.texture_vaes,
            vec!["trellis_2_texture_vae_bf16.safetensors"]
        );
        assert_eq!(w.moge_models, vec!["moge_2_vitl_normal_fp16.safetensors"]);
        let models = resolve_models(&w, &params()).unwrap();
        assert_eq!(models.bg_removal.as_deref(), Some("birefnet.safetensors"));
    }

    #[test]
    fn missing_pieces_are_named() {
        let mut info = object_info();
        info["LoadMoGeModel"]["input"]["required"]["model_name"] =
            json!(["COMBO", {"options": []}]);
        let w = worker_capabilities_from_info(&info, 0);
        assert!(resolve_models(&w, &params()).unwrap_err().contains("MoGe"));
        let mut info = object_info();
        info.as_object_mut().unwrap().remove("Pixal3DConditioning");
        let caps = merge_capabilities(vec![worker_capabilities_from_info(&info, 0)]);
        assert_eq!(caps.missing_nodes, vec!["Pixal3DConditioning"]);
        assert!(caps.models.is_empty());
    }

    #[test]
    fn background_removal_is_optional() {
        let mut info = object_info();
        info["LoadBackgroundRemovalModel"]["input"]["required"]["bg_removal_name"] =
            json!(["COMBO", {"options": []}]);
        let w = worker_capabilities_from_info(&info, 0);
        assert!(resolve_models(&w, &params()).is_err());
        let p = MeshParams {
            remove_background: false,
            ..params()
        };
        assert!(resolve_models(&w, &p).unwrap().bg_removal.is_none());
    }

    #[test]
    fn output_path_is_checked() {
        let ok = json!({"outputs": {"39": {"3d": [{"filename": "mooshie_mesh_00001_.glb", "subfolder": "3d", "type": "output"}]}}});
        assert!(glb_output(&ok).unwrap().is_some());
        assert_eq!(completed_status(&ok).unwrap()["status"], "completed");
        for bad in [
            json!({"filename": "other_00001_.glb", "subfolder": "3d", "type": "output"}),
            json!({"filename": "mooshie_mesh_00001_.ply", "subfolder": "3d", "type": "output"}),
            json!({"filename": "mooshie_mesh_00001_.glb", "subfolder": "..", "type": "output"}),
            json!({"filename": "mooshie_mesh_00001_.glb", "subfolder": "3d", "type": "temp"}),
        ] {
            let entry = json!({"outputs": {"39": {"3d": [bad]}}});
            assert!(glb_output(&entry).is_err());
        }
        let failed = json!({"status": {"status_str": "error", "messages": [["execution_error", {"exception_message": "CUDA out of memory"}]]}});
        assert_eq!(
            completed_status(&failed).unwrap()["error"],
            "CUDA out of memory"
        );
        let empty = json!({"outputs": {}});
        assert_eq!(completed_status(&empty).unwrap()["status"], "error");
    }

    fn tiny_glb() -> Vec<u8> {
        let json = br#"{"asset":{"version":"2.0"}}  "#; // padded to 4 bytes
        let total = 12 + 8 + json.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(json);
        out
    }

    #[test]
    fn glb_header_is_checked() {
        let glb = tiny_glb();
        assert!(is_glb(&glb));
        let mut wrong_len = glb.clone();
        wrong_len.push(0);
        assert!(!is_glb(&wrong_len));
        assert!(!is_glb(b"ply\nformat binary_little_endian 1.0\n"));
    }

    #[tokio::test]
    async fn export_only_writes_glb() {
        let dir = std::env::temp_dir().join(format!("mooshie-mesh-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let data = STANDARD.encode(tiny_glb());
        let path = dir.join("model.glb");
        save_file(&data, path.to_str().unwrap()).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), tiny_glb());
        assert!(save_file(&data, dir.join("model.ply").to_str().unwrap())
            .await
            .is_err());
        assert!(save_file(&STANDARD.encode(b"nope"), path.to_str().unwrap())
            .await
            .is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
