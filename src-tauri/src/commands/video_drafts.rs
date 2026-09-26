//! Gallery-owned references to retained H3 data on the configured ComfyUI server.
use crate::{error::AppError, state::AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const MODEL_NAME: &str = "h3_clean_latent_upscaler_film_epoch200.safetensors";
const MODEL_BYTES: u64 = 59022848;
static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static DRAFT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize, Deserialize)]
pub(crate) struct DraftRecord {
    version: u32,
    id: String,
    server: String,
    worker: u32,
}

pub(crate) fn record_path(video: &Path) -> PathBuf {
    video.with_extension("mp4.h3draft.json")
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

async fn fingerprint(state: &AppState) -> String {
    let config = state.config.read().await;
    let identity = if matches!(config.server_mode, crate::config::ServerMode::AutoLaunch) {
        format!(
            "local:{}",
            Path::new(&config.comfyui_path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&config.comfyui_path))
                .display()
        )
    } else {
        format!("remote:{}", config.server_url.trim_end_matches('/'))
    };
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn read_record(video: &Path) -> Result<Option<DraftRecord>, AppError> {
    let path = record_path(video);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AppError::Other(e.to_string())),
    };
    if !meta.is_file() || meta.len() > 4096 || path.canonicalize()?.parent() != video.parent() {
        return Err(AppError::Other("Invalid retained draft record".into()));
    }
    let record: DraftRecord = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|_| AppError::Other("Invalid retained draft record".into()))?;
    if record.version != 1 || !valid_id(&record.id) {
        return Err(AppError::Other("Unsupported retained draft record".into()));
    }
    Ok(Some(record))
}

pub(crate) async fn attach_record(
    state: &AppState,
    video: &Path,
    id: &str,
    prompt: &str,
) -> Result<(), AppError> {
    if !valid_id(id) {
        return Err(AppError::Other("Invalid draft identifier".into()));
    }
    let record = DraftRecord {
        version: 1,
        id: id.into(),
        server: fingerprint(state).await,
        worker: state.prompt_queue.worker_of(prompt).unwrap_or(0),
    };
    std::fs::write(
        record_path(video),
        serde_json::to_vec(&record).map_err(|e| AppError::Other(e.to_string()))?,
    )?;
    Ok(())
}

pub(crate) fn move_record(from: &Path, to: &Path) -> Result<(), AppError> {
    let source = record_path(from);
    if source.is_file() {
        std::fs::copy(&source, record_path(to))?;
        std::fs::remove_file(source)?;
    }
    Ok(())
}

/// Whether a ComfyUI output `subfolder` is a relative path whose every piece is
/// one plain component (the same rule `is_single_safe_filename` applies to a
/// filename). It may nest (`video/a`, with backslashes on Windows) or be empty,
/// but never start at a root and never hold `..`, a drive prefix or NUL.
fn is_safe_output_subfolder(subfolder: &str) -> bool {
    !subfolder.starts_with(['/', '\\'])
        && subfolder
            .split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .all(crate::commands::api::is_single_safe_filename)
}

/// Fetch remote outputs via ComfyUI's view API; never interpret remote absolute
/// paths as files on the machine running MooshieUI.
pub(crate) async fn fetch_output(
    state: &AppState,
    payload: &Value,
    prompt: &str,
    poster: bool,
) -> Result<PathBuf, AppError> {
    use tokio::io::AsyncWriteExt;
    let filename = payload[if poster {
        "poster_filename"
    } else {
        "filename"
    }]
    .as_str()
    .filter(|name| crate::commands::api::is_single_safe_filename(name))
    .ok_or_else(|| {
        AppError::Other("Remote video output requires updated MooshieUI nodes".into())
    })?;
    let worker_id = state.prompt_queue.worker_of(prompt).unwrap_or(0);
    let server = match state.gpu_manager.workers.iter().find(|w| w.id == worker_id) {
        Some(worker) => worker.base_url(),
        None => state.base_url().await,
    };
    let subfolder = payload["subfolder"].as_str().unwrap_or("");
    if !is_safe_output_subfolder(subfolder) {
        return Err(AppError::Other("Invalid remote output folder".into()));
    }
    let mut response = state
        .http_client
        .get(format!("{server}/view"))
        .query(&[
            ("filename", filename),
            ("subfolder", subfolder),
            ("type", "output"),
        ])
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
        .error_for_status()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let root = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Application data unavailable".into()))?
        .join("video-output-cache");
    tokio::fs::create_dir_all(&root).await?;
    let dest = root.join(format!(
        "{}{}",
        uuid::Uuid::new_v4(),
        if poster { ".webp" } else { ".mp4" }
    ));
    let result = async {
        let mut output = tokio::fs::File::create(&dest).await?;
        let limit = if poster {
            16 * 1024 * 1024
        } else {
            2_u64 * 1024 * 1024 * 1024
        };
        let mut bytes = 0;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| AppError::Other(e.to_string()))?
        {
            bytes += chunk.len() as u64;
            if bytes > limit {
                return Err(AppError::Other(
                    "Remote video output exceeds download limit".into(),
                ));
            }
            output.write_all(&chunk).await?;
        }
        output.flush().await?;
        Ok::<_, AppError>(())
    }
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&dest).await;
        return Err(error);
    }
    Ok(dest)
}

async fn record_server(state: &AppState, record: &DraftRecord) -> Result<String, AppError> {
    if record.server != fingerprint(state).await {
        return Err(AppError::Other(
            "Connect to the ComfyUI installation that created this draft.".into(),
        ));
    }
    state
        .gpu_manager
        .workers
        .iter()
        .find(|worker| worker.id == record.worker)
        .map(|worker| worker.base_url())
        .ok_or_else(|| {
            AppError::Other("The draft's GPU worker is unavailable. Restore its connection.".into())
        })
}

async fn fetch_json(state: &AppState, url: &str) -> Result<Value, AppError> {
    let mut response = state
        .http_client
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Other(
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                "Retained draft data or updated MooshieUI nodes are missing on this ComfyUI server."
                    .into()
            } else {
                format!("ComfyUI draft request failed ({})", response.status())
            },
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    {
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
            return Err(AppError::Other("Draft metadata is too large".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|e| AppError::Other(e.to_string()))
}

pub(crate) async fn upscaler_status(state: &AppState) -> Value {
    let capabilities = fetch_json(
        state,
        &format!("{}/mooshie/h3/capabilities", state.base_url().await),
    )
    .await
    .unwrap_or(Value::Null);
    let config = state.config.read().await;
    json!({"ready": capabilities["upscaler_ready"].as_bool().unwrap_or(false),
        "nodes_ready": capabilities["version"] == 1,
        "can_install": matches!(config.server_mode, crate::config::ServerMode::AutoLaunch) && !config.comfyui_path.trim().is_empty(),
        "download_bytes": MODEL_BYTES})
}

pub(crate) async fn install_upscaler(state: &AppState) -> Result<Value, AppError> {
    let _guard = INSTALL_LOCK
        .try_lock()
        .map_err(|_| AppError::Other("Upscaler installation is already running".into()))?;
    let config = state.config.read().await;
    if !matches!(config.server_mode, crate::config::ServerMode::AutoLaunch)
        || config.comfyui_path.trim().is_empty()
    {
        return Err(AppError::Other(
            "Install the H3 upscaler on the remote ComfyUI server.".into(),
        ));
    }
    let dest = Path::new(&config.comfyui_path)
        .join("models/h3_latent_upscalers")
        .join(MODEL_NAME);
    drop(config);
    let spec = crate::comfyui::h3_vdn::CheckpointFile {
        path: MODEL_NAME,
        bytes: MODEL_BYTES,
        sha256: Some("984afb58f11d01274b90d880596ce2f93bc9c512db66fdeecd4e2d99b371d3e4"),
    };
    let url = format!("https://huggingface.co/Tridae/H3LatentUpscaler/resolve/5c87ab7cf8425a2cfbc3d21da1bffbd686ce67a6/{MODEL_NAME}");
    crate::comfyui::h3_vdn::download_file(&state.http_client, &url, &dest, &spec, &|_, _, _| {})
        .await
        .map_err(AppError::Other)?;
    Ok(upscaler_status(state).await)
}

pub(crate) async fn draft_status(
    state: &AppState,
    dir: &Path,
    filename: &str,
) -> Result<Value, AppError> {
    let video = super::video_interpolate::resolve_gallery_video(dir, filename)?;
    let Some(record) = read_record(&video)? else {
        return Ok(json!({"retained": false}));
    };
    let result = async {
        let server = record_server(state, &record).await?;
        let mut meta = fetch_json(state, &format!("{server}/mooshie/h3/drafts/{}", record.id)).await?;
        meta.as_object_mut().map(|object| object.remove("params"));
        let cap = fetch_json(state, &format!("{server}/mooshie/h3/capabilities")).await?;
        Ok::<_, AppError>(json!({"retained": true, "available": true, "draft": meta, "upscaler_ready": cap["upscaler_ready"]}))
    }.await;
    Ok(result.unwrap_or_else(
        |error| json!({"retained": true, "available": false, "error": error.to_string()}),
    ))
}

pub(crate) async fn delete_draft(
    state: &AppState,
    dir: &Path,
    filename: &str,
) -> Result<Value, AppError> {
    let _guard = DRAFT_LOCK.lock().await;
    let video = super::video_interpolate::resolve_gallery_video(dir, filename)?;
    if let Some(record) = read_record(&video)? {
        let server = record_server(state, &record).await?;
        let response = state
            .http_client
            .delete(format!("{server}/mooshie/h3/drafts/{}", record.id))
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| AppError::Other(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AppError::Other(format!(
                "Cannot delete retained data ({}). Finish queued refinements and retry.",
                response.status()
            )));
        }
        std::fs::remove_file(record_path(&video))?;
    }
    Ok(json!({"deleted": true}))
}

pub(crate) async fn refine_draft(
    state: &AppState,
    dir: &Path,
    filename: &str,
    steps: u32,
    sigma: f64,
    owner: Option<String>,
) -> Result<Value, AppError> {
    let _guard = DRAFT_LOCK.lock().await;
    if !(4..=20).contains(&steps) || !sigma.is_finite() || !(0.15..=0.6).contains(&sigma) {
        return Err(AppError::Other(
            "Refinement requires 4–20 steps and strength 0.15–0.60".into(),
        ));
    }
    let video = super::video_interpolate::resolve_gallery_video(dir, filename)?;
    let record = read_record(&video)?
        .ok_or_else(|| AppError::Other("This clip has no retained draft".into()))?;
    let server = record_server(state, &record).await?;
    let meta = fetch_json(state, &format!("{server}/mooshie/h3/drafts/{}", record.id)).await?;
    let width = meta["width"].as_u64().unwrap_or(0);
    let height = meta["height"].as_u64().unwrap_or(0);
    let frames = meta["frames"].as_u64().unwrap_or(0);
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || width * height * 4 > 2_200_000
        || frames == 0
        || frames > 365
    {
        return Err(AppError::Other(
            "Draft dimensions exceed the 2× refinement limits".into(),
        ));
    }
    let params: crate::comfyui::types::GenerationParams =
        serde_json::from_value(meta["params"].clone())
            .map_err(|_| AppError::Other("Draft generation settings are invalid".into()))?;
    let cap = fetch_json(state, &format!("{server}/mooshie/h3/capabilities")).await?;
    if cap["upscaler_ready"] != true {
        return Err(AppError::Other(
            "Install the H3 latent upscaler first".into(),
        ));
    }
    let workflow = crate::templates::video_refine::build(
        &params,
        &record.id,
        filename,
        width as u32,
        height as u32,
        steps,
        sigma,
    );
    crate::comfyui::process::mark_legacy_worker_idle(state).await;
    state.free_llm_vram_for_generation().await;
    let (worker, response) = state
        .gpu_manager
        .submit_prompt_to_worker(record.worker, workflow, &state.client_id)
        .await?;
    state.prompt_queue.insert(&response.prompt_id, owner);
    state.prompt_queue.set_worker(&response.prompt_id, worker);
    state.broadcast_queue_positions();
    Ok(json!({"prompt_id": response.prompt_id}))
}

#[cfg(feature = "desktop")]
mod desktop {
    use super::*;
    use std::sync::Arc;
    use tauri::State;
    fn gallery() -> Result<PathBuf, AppError> {
        crate::config::gallery_dir().ok_or_else(|| AppError::Other("Gallery unavailable".into()))
    }
    #[tauri::command]
    pub async fn get_h3_upscaler_status(
        state: State<'_, Arc<AppState>>,
    ) -> Result<Value, AppError> {
        Ok(upscaler_status(state.inner()).await)
    }
    #[tauri::command]
    pub async fn install_h3_upscaler(state: State<'_, Arc<AppState>>) -> Result<Value, AppError> {
        install_upscaler(state.inner()).await
    }
    #[tauri::command]
    pub async fn get_video_draft_status(
        state: State<'_, Arc<AppState>>,
        filename: String,
    ) -> Result<Value, AppError> {
        draft_status(state.inner(), &gallery()?, &filename).await
    }
    #[tauri::command]
    pub async fn delete_video_draft(
        state: State<'_, Arc<AppState>>,
        filename: String,
    ) -> Result<Value, AppError> {
        delete_draft(state.inner(), &gallery()?, &filename).await
    }
    #[tauri::command]
    pub async fn refine_video_draft(
        state: State<'_, Arc<AppState>>,
        filename: String,
        steps: u32,
        sigma: f64,
    ) -> Result<Value, AppError> {
        refine_draft(state.inner(), &gallery()?, &filename, steps, sigma, None).await
    }
}
#[cfg(feature = "desktop")]
pub use desktop::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_records_follow_clip_moves_without_changing_ids() {
        let temp = std::env::temp_dir().join(format!("draft-record-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&temp).unwrap();
        let root = temp.canonicalize().unwrap();
        let from = root.join("old.mp4");
        let to = root.join("new.mp4");
        std::fs::write(&from, b"synthetic mp4").unwrap();
        assert!(read_record(&from).unwrap().is_none());
        let record = DraftRecord {
            version: 1,
            id: "a".repeat(32),
            server: "server-hash".into(),
            worker: 2,
        };
        std::fs::write(record_path(&from), serde_json::to_vec(&record).unwrap()).unwrap();
        std::fs::rename(&from, &to).unwrap();
        move_record(&from, &to).unwrap();
        assert!(!record_path(&from).exists());
        let restored = read_record(&to).unwrap().unwrap();
        assert_eq!(restored.id, record.id);
        assert_eq!(restored.worker, 2);
        std::fs::write(record_path(&to), b"{}").unwrap();
        assert!(read_record(&to).is_err());
        std::fs::write(record_path(&to), vec![b'x'; 4097]).unwrap();
        assert!(read_record(&to).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_opaque_lowercase_draft_ids_are_accepted() {
        assert!(valid_id(&"0123456789abcdef".repeat(2)));
        for id in ["../other", "", "ABCDEF", "c:\\private"] {
            assert!(!valid_id(id));
        }
        assert!(!valid_id(&"a".repeat(33)));
        assert!(!valid_id(&"z".repeat(32)));
    }

    #[test]
    fn output_subfolders_nest_but_never_escape() {
        for ok in ["", "video", "video/a", "video\\a", "a//b", "a/"] {
            assert!(is_safe_output_subfolder(ok), "{ok:?}");
        }
        for bad in [
            "/abs",
            "\\abs",
            "..",
            "a/../b",
            "a\\..\\b",
            "D:x",
            "video/C:evil",
            "a/./b",
            "nul\0byte",
        ] {
            assert!(!is_safe_output_subfolder(bad), "{bad:?}");
        }
    }
}
