//! The MCP tools: definitions for `tools/list` and their handlers.
//!
//! Every handler acts as the machine owner (the root gallery, admin queue
//! slot), which is who holds the token. Tool failures come back as a normal
//! result with `isError: true` so the agent sees the message.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use super::jobs::{self, Job};
use super::music::{self, MusicJob};
use super::setups;
use crate::state::AppState;

/// How long `generate_image` holds the request before handing back a job id.
const DEFAULT_WAIT_SECS: u64 = 600;
/// Largest song `include_audio` embeds in a result.
const MAX_INLINE_AUDIO: u64 = 16 * 1024 * 1024;
const PREVIEW_SIZE: u32 = 768;
/// Newest gallery files a search looks at when filling in missing metadata.
const SEARCH_WINDOW: usize = 5000;
/// Files whose metadata a single search reads from disk at most.
const BACKFILL_PER_CALL: usize = 200;

pub fn definitions() -> Value {
    json!([
        {
            "name": "get_status",
            "title": "MooshieUI status",
            "description": "Check whether ComfyUI is running, what is queued, recent image and music jobs, and which models have a saved generation setup (only those can be used by generate_image). Call this first.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "list_models",
            "title": "List models",
            "description": "List installed model files. Checkpoints and diffusion models marked has_setup can be passed to generate_image; LoRAs can be added to any generation. music lists the YuE2 checkpoints generate_music can use.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string", "enum": ["checkpoints", "diffusion_models", "loras", "music"], "description": "Default checkpoints." },
                    "filter": { "type": "string", "description": "Case-insensitive substring to match." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Default 200." }
                }
            },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "generate_image",
            "title": "Generate image",
            "description": "Generate images with MooshieUI (txt2img). Reuses the owner's last setup for the model (loaders, upscale, face fix, guidance); you change the prompt and any of the optional fields. Images are saved to the MooshieUI gallery and returned as previews. Waits up to 10 minutes for the result unless wait is false; then poll get_job (get_status lists recent job ids).",
            "inputSchema": {
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "prompt": { "type": "string", "description": "Positive prompt, in the style the model expects (tags for anime SDXL models, sentences for Flux and similar)." },
                    "negative_prompt": { "type": "string", "description": "Replaces the setup's negative prompt." },
                    "model": { "type": "string", "description": "Model with a saved setup (see get_status). Default: the most recently used one." },
                    "width": { "type": "integer", "minimum": 256, "maximum": 4096 },
                    "height": { "type": "integer", "minimum": 256, "maximum": 4096 },
                    "steps": { "type": "integer", "minimum": 1, "maximum": 150 },
                    "cfg": { "type": "number", "minimum": 0, "maximum": 30 },
                    "seed": { "type": "integer", "description": "Omit or -1 for random." },
                    "batch_size": { "type": "integer", "minimum": 1, "maximum": 8 },
                    "sampler": { "type": "string", "description": "ComfyUI sampler name, e.g. euler_ancestral." },
                    "scheduler": { "type": "string", "description": "ComfyUI scheduler name, e.g. karras." },
                    "loras": {
                        "type": "array",
                        "maxItems": 10,
                        "description": "Replaces the setup's LoRAs. [] removes them.",
                        "items": {
                            "type": "object",
                            "required": ["name"],
                            "properties": {
                                "name": { "type": "string", "description": "LoRA filename from list_models." },
                                "strength": { "type": "number", "description": "Default 1." },
                                "strength_clip": { "type": "number", "description": "Default: same as strength." }
                            }
                        }
                    },
                    "wait": { "type": "boolean", "description": "Default true." },
                    "include_images": { "type": "boolean", "description": "Return image previews. Default true." }
                }
            }
        },
        {
            "name": "generate_music",
            "title": "Generate music",
            "description": "Generate a song with YuE2 (the MooshieUI Music page backend). Describe the style; add lyrics for vocals or leave them out for instrumental music. The FLAC (and the ABC score YuE2 planned) is saved on the MooshieUI machine and its path returned. Songs take minutes: this waits up to 10 minutes unless wait is false; then poll get_job.",
            "inputSchema": {
                "type": "object",
                "required": ["style"],
                "properties": {
                    "style": { "type": "string", "description": "Genre, instruments, mood, tempo and vocal type, e.g. \"upbeat synthpop, female vocals, 120 bpm, bright analog synths\"." },
                    "lyrics": { "type": "string", "description": "Song lyrics. Section tags such as [verse] and [chorus] on their own lines help structure. Omit for instrumental music." },
                    "duration_seconds": { "type": "number", "minimum": 1, "maximum": 360, "description": "Maximum length. Default 120." },
                    "checkpoint": { "type": "string", "description": "YuE2 checkpoint from list_models (category music). Default: the first installed." },
                    "planning": { "type": "string", "enum": ["full", "melody", "off"], "description": "full plans melody and chords as an ABC score first (default), melody plans only the melody, off skips planning." },
                    "score": { "type": "string", "description": "ABC notation score for the song to follow (needs planning full or melody)." },
                    "steps": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 32." },
                    "seed": { "type": "integer", "description": "Omit or -1 for random." },
                    "wait": { "type": "boolean", "description": "Default true." },
                    "include_audio": { "type": "boolean", "description": "Embed the FLAC in the result (up to 16 MB). Default false." }
                }
            }
        },
        {
            "name": "get_job",
            "title": "Generation job",
            "description": "Status and results of a generate_image or generate_music job.",
            "inputSchema": {
                "type": "object",
                "required": ["job_id"],
                "properties": {
                    "job_id": { "type": "string" },
                    "wait_seconds": { "type": "integer", "minimum": 0, "maximum": 600, "description": "Wait up to this long for the job to finish. Default 0." },
                    "include_images": { "type": "boolean", "description": "Image jobs: return previews. Default true." },
                    "include_audio": { "type": "boolean", "description": "Music jobs: embed the FLAC in the result (up to 16 MB). Default false; the file path is always returned." }
                }
            },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "cancel_generation",
            "title": "Cancel generation",
            "description": "Cancel a generate_image or generate_music job, or every unfinished one when job_id is omitted. Generations started in the MooshieUI window are not touched.",
            "inputSchema": {
                "type": "object",
                "properties": { "job_id": { "type": "string" } }
            },
            "annotations": { "destructiveHint": false }
        },
        {
            "name": "search_gallery",
            "title": "Search gallery",
            "description": "Find images in the MooshieUI gallery by words in their prompt, negative prompt or model name, or list the newest when query is empty.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20." }
                }
            },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "get_image",
            "title": "Get gallery image",
            "description": "Look at a gallery image and read its generation settings.",
            "inputSchema": {
                "type": "object",
                "required": ["filename"],
                "properties": {
                    "filename": { "type": "string", "description": "Gallery filename from search_gallery or generate_image." },
                    "max_size": { "type": "integer", "minimum": 128, "maximum": 2048, "description": "Longest side of the returned preview. Default 1024." }
                }
            },
            "annotations": { "readOnlyHint": true }
        }
    ])
}

fn text(value: Value) -> Value {
    json!({ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() })
}

fn ok(content: Vec<Value>) -> Value {
    json!({ "content": content, "isError": false })
}

fn error(message: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": message.into() }], "isError": true })
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Run a tool. `None` for an unknown tool name.
pub async fn call(state: &Arc<AppState>, name: &str, args: &Value) -> Option<Value> {
    let result = match name {
        "get_status" => get_status(state).await,
        "list_models" => list_models(state, args).await,
        "generate_image" => generate_image(state, args).await,
        "generate_music" => generate_music(state, args).await,
        "get_job" => get_job(args).await,
        "cancel_generation" => cancel_generation(state, args).await,
        "search_gallery" => search_gallery(args).await,
        "get_image" => get_image(args).await,
        _ => return None,
    };
    Some(result.unwrap_or_else(error))
}

fn setup_summary(s: &setups::Setup) -> Value {
    let p = &s.params;
    json!({
        "model": s.model,
        "family": p.model_architecture,
        "width": p.width,
        "height": p.height,
        "steps": p.steps,
        "cfg": p.cfg,
        "sampler": p.sampler_name,
        "scheduler": p.scheduler,
        "negative_prompt": p.negative_prompt,
        "loras": p.loras.iter().map(|l| json!({ "name": l.name, "strength": l.strength_model })).collect::<Vec<_>>(),
        "upscale": p.upscale_enabled.then(|| json!({ "scale": p.upscale_scale, "model": p.upscale_model })),
        "face_fix": p.facefix_enabled,
    })
}

async fn get_status(state: &Arc<AppState>) -> Result<Value, String> {
    let comfyui = match state.get_system_stats_info().await {
        Ok(stats) => json!({
            "running": true,
            "version": stats.system.comfyui_version,
            "gpus": stats.devices.iter().map(|d| json!({
                "name": d.name,
                "vram_total_gb": (d.vram_total as f64 / 1e9 * 10.0).round() / 10.0,
                "vram_free_gb": (d.vram_free as f64 / 1e9 * 10.0).round() / 10.0,
            })).collect::<Vec<_>>(),
        }),
        Err(e) => json!({ "running": false, "error": e.to_string() }),
    };
    let setups: Vec<Value> = setups::list().iter().map(setup_summary).collect();
    let recent: Vec<Value> = jobs::recent(5)
        .iter()
        .map(|j| {
            json!({
                "job_id": j.job_id,
                "status": j.status,
                "prompt": j.prompt.chars().take(120).collect::<String>(),
                "images": j.images.iter().map(|i| &i.filename).collect::<Vec<_>>(),
                "error": j.error,
            })
        })
        .collect();
    let recent_music: Vec<Value> = music::recent(5)
        .iter()
        .map(|j| {
            json!({
                "job_id": j.job_id,
                "status": j.status,
                "style": j.style.chars().take(120).collect::<String>(),
                "file": j.file,
                "error": j.error,
            })
        })
        .collect();
    Ok(ok(vec![text(json!({
        "mooshieui_version": env!("CARGO_PKG_VERSION"),
        "comfyui": comfyui,
        "queue_length": state.prompt_queue.len(),
        "recent_jobs": recent,
        "recent_music_jobs": recent_music,
        "setups": setups,
        "note": if setups.is_empty() {
            "No saved setups yet: generate one txt2img image in MooshieUI with a model to make it usable here."
        } else {
            "generate_image can use any model in setups."
        },
    }))]))
}

async fn list_models(state: &Arc<AppState>, args: &Value) -> Result<Value, String> {
    let category = arg_str(args, "category").unwrap_or("checkpoints");
    if category == "music" {
        return list_music_models(state).await;
    }
    if !matches!(category, "checkpoints" | "diffusion_models" | "loras") {
        return Err("category must be checkpoints, diffusion_models, loras or music".into());
    }
    let filter = arg_str(args, "filter").map(str::to_ascii_lowercase);
    let limit = args["limit"].as_u64().unwrap_or(200).clamp(1, 1000) as usize;
    let all = state
        .get_models_list(category)
        .await
        .map_err(|e| format!("Could not list models (is ComfyUI running?): {e}"))?;
    let with_setup: HashSet<String> = setups::list().into_iter().map(|s| s.model).collect();
    let matching: Vec<&String> = all
        .iter()
        .filter(|m| {
            filter
                .as_deref()
                .is_none_or(|f| m.to_ascii_lowercase().contains(f))
        })
        .collect();
    let models: Vec<Value> = matching
        .iter()
        .take(limit)
        .map(|m| {
            if category == "loras" {
                json!({ "name": m })
            } else {
                json!({ "name": m, "has_setup": with_setup.contains(m.as_str()) })
            }
        })
        .collect();
    Ok(ok(vec![text(json!({
        "category": category,
        "total": matching.len(),
        "models": models,
    }))]))
}

async fn generate_image(state: &Arc<AppState>, args: &Value) -> Result<Value, String> {
    let overrides: setups::Overrides =
        serde_json::from_value(args.clone()).map_err(|e| format!("Invalid arguments: {e}"))?;
    let setup = setups::find(&setups::list(), arg_str(args, "model"))?;
    let params = setups::apply(&setup.params, &overrides)?;
    let rx = jobs::start(state.clone(), params);
    if args["wait"].as_bool() == Some(false) {
        let job = rx.borrow().clone();
        return Ok(ok(vec![text(job_json(&job))]));
    }
    let job = jobs::wait(rx, Duration::from_secs(DEFAULT_WAIT_SECS)).await;
    job_result(&job, args["include_images"].as_bool().unwrap_or(true)).await
}

async fn list_music_models(state: &Arc<AppState>) -> Result<Value, String> {
    let caps = crate::commands::music::capabilities(state)
        .await
        .map_err(|e| format!("Could not check music support (is ComfyUI running?): {e}"))?;
    let note = if !caps.missing_nodes.is_empty() {
        format!(
            "YuE2 nodes are missing from ComfyUI ({}). Update the managed runtime from MooshieUI's Music page.",
            caps.missing_nodes.join(", ")
        )
    } else if caps.checkpoints.is_empty() {
        "No YuE2 checkpoint is installed. Download one from MooshieUI's Music page.".into()
    } else {
        "generate_music can use any of these.".into()
    };
    Ok(ok(vec![text(json!({
        "category": "music",
        "available": caps.missing_nodes.is_empty() && !caps.checkpoints.is_empty(),
        "models": caps.checkpoints.iter().map(|m| json!({ "name": m })).collect::<Vec<_>>(),
        "note": note,
    }))]))
}

async fn generate_music(state: &Arc<AppState>, args: &Value) -> Result<Value, String> {
    let req: music::MusicRequest =
        serde_json::from_value(args.clone()).map_err(|e| format!("Invalid arguments: {e}"))?;
    if req.style.trim().is_empty() {
        return Err("style is required".into());
    }
    let rx = music::start(state.clone(), req);
    if args["wait"].as_bool() == Some(false) {
        let job = rx.borrow().clone();
        return Ok(ok(vec![text(music_job_json(&job))]));
    }
    let job = jobs::wait(rx, Duration::from_secs(DEFAULT_WAIT_SECS)).await;
    music_job_result(&job, args["include_audio"].as_bool().unwrap_or(false)).await
}

fn music_job_json(job: &MusicJob) -> Value {
    let mut v = serde_json::to_value(job).unwrap_or_default();
    if !job.status.finished() {
        v["hint"] = json!("Still working. Call get_job with this job_id (wait_seconds up to 600).");
    }
    v
}

async fn music_job_result(job: &MusicJob, include_audio: bool) -> Result<Value, String> {
    let mut content = vec![text(music_job_json(job))];
    if let (true, Some(file)) = (
        include_audio && job.status == jobs::JobStatus::Done,
        &job.file,
    ) {
        match tokio::fs::metadata(file).await {
            Ok(m) if m.len() <= MAX_INLINE_AUDIO => {
                let bytes = tokio::fs::read(file).await.map_err(|e| e.to_string())?;
                content.push(json!({
                    "type": "audio",
                    "mimeType": "audio/flac",
                    "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                }));
            }
            Ok(_) => content.push(json!({
                "type": "text",
                "text": "The song is over 16 MB, so it isn't embedded. Use the file path.",
            })),
            Err(e) => log::warn!("[mcp] reading {file} failed: {e}"),
        }
    }
    Ok(json!({
        "content": content,
        "isError": job.status == jobs::JobStatus::Failed,
    }))
}

fn job_json(job: &Job) -> Value {
    let mut v = serde_json::to_value(job).unwrap_or_default();
    if !job.status.finished() {
        v["hint"] = json!("Still working. Call get_job with this job_id (wait_seconds up to 600).");
    }
    v
}

async fn job_result(job: &Job, include_images: bool) -> Result<Value, String> {
    let mut content = vec![text(job_json(job))];
    if include_images && job.status == jobs::JobStatus::Done {
        for img in &job.images {
            match preview(&img.filename, PREVIEW_SIZE).await {
                Ok((data, _, _)) => content.push(image_content(data)),
                Err(e) => log::warn!("[mcp] preview of {} failed: {e}", img.filename),
            }
        }
    }
    Ok(json!({
        "content": content,
        "isError": job.status == jobs::JobStatus::Failed,
    }))
}

async fn get_job(args: &Value) -> Result<Value, String> {
    let id = arg_str(args, "job_id").ok_or("job_id is required")?;
    let wait = args["wait_seconds"].as_u64().unwrap_or(0).min(600);
    if music::is_music_job(id) {
        let rx = music::get(id)
            .ok_or_else(|| format!("No job '{id}' (jobs are forgotten when MooshieUI restarts)"))?;
        let job = jobs::wait(rx, Duration::from_secs(wait)).await;
        return music_job_result(&job, args["include_audio"].as_bool().unwrap_or(false)).await;
    }
    let rx = jobs::get(id)
        .ok_or_else(|| format!("No job '{id}' (jobs are forgotten when MooshieUI restarts)"))?;
    let job = jobs::wait(rx, Duration::from_secs(wait)).await;
    job_result(&job, args["include_images"].as_bool().unwrap_or(true)).await
}

async fn cancel_generation(state: &Arc<AppState>, args: &Value) -> Result<Value, String> {
    let ids: Vec<String> = match arg_str(args, "job_id") {
        Some(id) => vec![id.to_string()],
        None => jobs::active()
            .into_iter()
            .map(|j| j.job_id)
            .chain(music::active().into_iter().map(|j| j.job_id))
            .collect(),
    };
    let mut cancelled = Vec::new();
    for id in ids {
        let (job_id, status) = if music::is_music_job(&id) {
            let job = music::cancel(state, &id).await?;
            (job.job_id, job.status)
        } else {
            let job = jobs::cancel(state, &id).await?;
            (job.job_id, job.status)
        };
        cancelled.push(json!({ "job_id": job_id, "status": status }));
    }
    Ok(ok(vec![text(json!({ "jobs": cancelled }))]))
}

fn image_content(jpeg: Vec<u8>) -> Value {
    json!({
        "type": "image",
        "mimeType": "image/jpeg",
        "data": base64::engine::general_purpose::STANDARD.encode(jpeg),
    })
}

fn gallery_dir() -> Result<PathBuf, String> {
    crate::config::gallery_dir().ok_or_else(|| "Cannot find gallery directory".to_string())
}

/// A gallery image file directly in `dir` (the owner's gallery), or an error.
fn gallery_image_path(dir: &Path, filename: &str) -> Result<PathBuf, String> {
    crate::commands::api::validate_gallery_filename(filename)
        .map_err(|_| format!("Invalid filename '{filename}'"))?;
    if !is_image_file(filename) {
        return Err(format!("'{filename}' is not a gallery image"));
    }
    let path = dir.join(filename);
    if !path.is_file() {
        return Err(format!("No gallery image '{filename}'"));
    }
    Ok(path)
}

fn is_image_file(name: &str) -> bool {
    crate::commands::api::is_listable_gallery_file(name)
        && !name.to_ascii_lowercase().ends_with(".mp4")
}

/// A JPEG preview of a gallery image, plus the original size.
async fn preview(filename: &str, max_size: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let path = gallery_image_path(&gallery_dir()?, filename)?;
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let img = crate::commands::api::decode_gallery_image(&bytes)?;
        let (w, h) = (img.width(), img.height());
        let thumb = if w.max(h) > max_size {
            img.thumbnail(max_size, max_size)
        } else {
            img
        };
        let rgb = image::DynamicImage::ImageRgb8(thumb.to_rgb8());
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .encode_image(&rgb)
            .map_err(|e| format!("JPEG encode failed: {e}"))?;
        Ok((out, w, h))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Metadata keys worth showing an agent, in the order shown.
const METADATA_KEYS: &[&str] = &[
    "positive_prompt",
    "negative_prompt",
    "model",
    "seed",
    "steps",
    "cfg",
    "sampler",
    "scheduler",
    "size",
    "loras",
    "denoise",
    "mode",
    "upscale_model",
    "upscale_scale",
    "date",
];

async fn get_image(args: &Value) -> Result<Value, String> {
    let filename = arg_str(args, "filename").ok_or("filename is required")?;
    let max_size = args["max_size"].as_u64().unwrap_or(1024).clamp(128, 2048) as u32;
    let (jpeg, width, height) = preview(filename, max_size).await?;
    let path = gallery_image_path(&gallery_dir()?, filename)?;
    let metadata = tokio::task::spawn_blocking(move || {
        std::fs::read(&path)
            .ok()
            .and_then(|b| crate::metadata::read_image_metadata(&b).ok().flatten())
    })
    .await
    .map_err(|e| e.to_string())?
    .unwrap_or_default();
    let settings: serde_json::Map<String, Value> = METADATA_KEYS
        .iter()
        .filter_map(|k| {
            metadata
                .get(*k)
                .filter(|v| !v.is_empty())
                .map(|v| (k.to_string(), json!(v)))
        })
        .collect();
    Ok(ok(vec![
        text(json!({
            "filename": filename,
            "width": width,
            "height": height,
            "settings": settings,
        })),
        image_content(jpeg),
    ]))
}

/// Files whose metadata was already read this session, so images without any
/// are not re-read on every search.
static CHECKED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Gallery images directly in `dir`, newest first.
fn gallery_images(dir: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, u64)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| is_image_file(&e.file_name().to_string_lossy()))
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            (e.path(), modified)
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    files
}

/// Read metadata from disk into the index for files whose row has no prompt
/// (images saved before the index stored prompts, or imported ones). Returns
/// how many of `paths` still lack one afterwards.
fn backfill(paths: &[PathBuf], budget: usize) -> usize {
    let rows = crate::gallery_index::rows_for(paths);
    let mut checked = CHECKED.lock().unwrap();
    let missing: Vec<&PathBuf> = paths
        .iter()
        .filter(|p| {
            rows.get(p.to_string_lossy().as_ref())
                .is_none_or(|r| r.prompt.is_none())
        })
        .filter(|p| !checked.contains(*p))
        .collect();
    let remaining = missing.len().saturating_sub(budget);
    for path in missing.into_iter().take(budget) {
        checked.insert(path.clone());
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let meta: Option<HashMap<String, String>> =
            crate::metadata::read_image_metadata(&bytes).ok().flatten();
        if meta.is_some() {
            crate::gallery_index::upsert(
                path,
                bytes.len() as u64,
                crate::metadata::detect_format(&bytes),
                meta.as_ref(),
            );
        }
    }
    remaining
}

async fn search_gallery(args: &Value) -> Result<Value, String> {
    let query = arg_str(args, "query").unwrap_or("").to_string();
    let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as usize;
    let dir = gallery_dir()?;
    tokio::task::spawn_blocking(move || {
        let files = gallery_images(&dir);
        let total = files.len();
        if query.is_empty() {
            let newest: Vec<PathBuf> = files.into_iter().take(limit).map(|(p, _)| p).collect();
            backfill(&newest, newest.len());
            let rows = crate::gallery_index::rows_for(&newest);
            let images: Vec<Value> = newest
                .iter()
                .map(|p| {
                    let key = p.to_string_lossy();
                    match rows.get(key.as_ref()) {
                        Some(row) => json!(row),
                        None => json!({ "filename": p.file_name().map(|n| n.to_string_lossy().into_owned()) }),
                    }
                })
                .collect();
            return ok(vec![text(json!({ "total_images": total, "images": images }))]);
        }
        let window: Vec<PathBuf> = files
            .into_iter()
            .take(SEARCH_WINDOW)
            .map(|(p, _)| p)
            .collect();
        let unindexed = backfill(&window, BACKFILL_PER_CALL);
        let hits = crate::gallery_index::search(&dir, &query, limit);
        let mut result = json!({ "total_images": total, "query": query, "images": hits });
        if unindexed > 0 {
            result["note"] = json!(format!(
                "{unindexed} older images were not searched yet. Repeat the search to cover more."
            ));
        }
        ok(vec![text(result)])
    })
    .await
    .map_err(|e| e.to_string())
}
