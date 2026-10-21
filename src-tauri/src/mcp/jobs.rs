//! MCP generation jobs: submit, wait for ComfyUI, save to the gallery.
//!
//! The frontend only keeps outputs for prompts it queued itself, so nothing
//! else would save an MCP image. Each job runs in its own task and saves its
//! images even if the agent's request has already timed out or gone away.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::watch;

use crate::comfyui::types::GenerationParams;
use crate::state::AppState;

/// Finished jobs kept for `get_job`; the oldest finished ones go first.
const MAX_JOBS: usize = 100;
/// Longest a job waits for ComfyUI after submitting.
const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Waiting for a free GPU worker.
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedImage {
    /// Gallery filename, for `get_image`.
    pub filename: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub job_id: String,
    pub status: JobStatus,
    pub model: String,
    pub prompt: String,
    pub seed: String,
    pub width: u32,
    pub height: u32,
    pub images: Vec<SavedImage>,
    pub error: Option<String>,
    #[serde(skip)]
    pub comfy_prompt_id: Option<String>,
}

/// A job kind `Store` can hold.
pub trait Tracked: Clone {
    fn finished(&self) -> bool;
}

impl Tracked for Job {
    fn finished(&self) -> bool {
        self.status.finished()
    }
}

/// Jobs by id, oldest first. Finished jobs past `MAX_JOBS` are dropped.
pub struct Store<T> {
    jobs: Mutex<Vec<(String, watch::Sender<T>)>>,
}

impl<T: Tracked> Store<T> {
    pub const fn new() -> Self {
        Self {
            jobs: Mutex::new(Vec::new()),
        }
    }

    pub fn insert(&self, id: String, job: T) -> watch::Sender<T> {
        let (tx, _) = watch::channel(job);
        let mut jobs = self.jobs.lock().unwrap();
        jobs.push((id, tx.clone()));
        while jobs.len() > MAX_JOBS {
            let Some(pos) = jobs.iter().position(|(_, tx)| tx.borrow().finished()) else {
                break;
            };
            jobs.remove(pos);
        }
        tx
    }

    pub fn sender(&self, job_id: &str) -> Option<watch::Sender<T>> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .find(|(id, _)| id == job_id)
            .map(|(_, tx)| tx.clone())
    }

    pub fn get(&self, job_id: &str) -> Option<watch::Receiver<T>> {
        self.sender(job_id).map(|tx| tx.subscribe())
    }

    /// Jobs not finished yet, oldest first.
    pub fn active(&self) -> Vec<T> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .map(|(_, tx)| tx.borrow().clone())
            .filter(|j| !j.finished())
            .collect()
    }

    /// The last `n` jobs, newest first, so an agent whose client timed out
    /// can still find its job id.
    pub fn recent(&self, n: usize) -> Vec<T> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .rev()
            .take(n)
            .map(|(_, tx)| tx.borrow().clone())
            .collect()
    }
}

impl<T: Tracked> Default for Store<T> {
    fn default() -> Self {
        Self::new()
    }
}

static JOBS: Store<Job> = Store::new();

pub fn get(job_id: &str) -> Option<watch::Receiver<Job>> {
    JOBS.get(job_id)
}

/// Jobs not finished yet, oldest first.
pub fn active() -> Vec<Job> {
    JOBS.active()
}

/// The last `n` image jobs, newest first.
pub fn recent(n: usize) -> Vec<Job> {
    JOBS.recent(n)
}

/// Wait until the job finishes or `timeout` passes, then return its state.
pub async fn wait<T: Tracked>(mut rx: watch::Receiver<T>, timeout: Duration) -> T {
    let _ = tokio::time::timeout(timeout, rx.wait_for(|j| j.finished())).await;
    let job = rx.borrow().clone();
    job
}

/// Cancel a job. A queued job never reaches ComfyUI; a running one is
/// interrupted there.
pub async fn cancel(state: &Arc<AppState>, job_id: &str) -> Result<Job, String> {
    let tx = JOBS
        .sender(job_id)
        .ok_or_else(|| format!("No job '{job_id}'"))?;
    let (was, prompt_id) = {
        let job = tx.borrow();
        (job.status, job.comfy_prompt_id.clone())
    };
    if was.finished() {
        return Ok(tx.borrow().clone());
    }
    tx.send_modify(|j| {
        j.status = JobStatus::Cancelled;
        j.error = Some("Cancelled".into());
    });
    if let Some(pid) = prompt_id {
        state
            .interrupt_prompt(Some(&pid))
            .await
            .map_err(|e| e.to_string())?;
    }
    let job = tx.borrow().clone();
    Ok(job)
}

/// Queue `params` and return a receiver that tracks the job.
pub fn start(state: Arc<AppState>, params: GenerationParams) -> watch::Receiver<Job> {
    let job = Job {
        job_id: format!("mcp-{}", uuid::Uuid::new_v4().simple()),
        status: JobStatus::Queued,
        model: super::setups::model_key(&params),
        prompt: params.positive_prompt.clone(),
        seed: String::new(),
        width: params.width,
        height: params.height,
        images: Vec::new(),
        error: None,
        comfy_prompt_id: None,
    };
    let tx = JOBS.insert(job.job_id.clone(), job);
    let rx = tx.subscribe();
    tokio::spawn(async move {
        if let Err(e) = run(&state, &tx, params).await {
            tx.send_if_modified(|j| {
                if j.status.finished() {
                    return false;
                }
                j.status = JobStatus::Failed;
                j.error = Some(e.clone());
                true
            });
            log::warn!("[mcp] job {} failed: {e}", tx.borrow().job_id);
        }
    });
    rx
}

fn cancelled(tx: &watch::Sender<Job>) -> bool {
    tx.borrow().status == JobStatus::Cancelled
}

/// One output image event for our prompt.
struct Output {
    temp_filename: String,
    width: Option<u32>,
    height: Option<u32>,
}

enum Outcome {
    Done,
    Failed(String),
    Cancelled,
}

async fn run(
    state: &Arc<AppState>,
    tx: &watch::Sender<Job>,
    mut params: GenerationParams,
) -> Result<(), String> {
    crate::temp_images::cleanup(300);
    crate::templates::validate_generation_params(&params)?;
    {
        let config = state.config.read().await;
        crate::commands::api::validate_generation_loras(&config, &params.loras)
            .map_err(|e| e.to_string())?;
    }
    let seed = if params.seed < 0 {
        (rand::random::<u64>() >> 1) as i64
    } else {
        params.seed
    };
    params.seed = seed;
    tx.send_modify(|j| j.seed = seed.to_string());

    let workflow = crate::templates::build_workflow(&params, seed, false);
    crate::comfyui::process::mark_legacy_worker_idle(state).await;
    state.free_llm_vram_for_generation().await;

    // Subscribed before submitting, so no event for this prompt is missed.
    let mut events = state.event_tx.subscribe();
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, Duration::from_secs(300))
        .await
        .map_err(|e| e.to_string())?;
    let prompt_id = response.prompt_id;
    // Owner `None`: the MCP caller is the machine owner, like the desktop app.
    state.prompt_queue.insert(&prompt_id, None);
    state.prompt_queue.set_worker(&prompt_id, worker_id);
    state.broadcast_queue_positions();
    tx.send_modify(|j| j.comfy_prompt_id = Some(prompt_id.clone()));
    if cancelled(tx) {
        let _ = state.interrupt_prompt(Some(&prompt_id)).await;
        return Ok(());
    }
    tx.send_modify(|j| {
        if j.status == JobStatus::Queued {
            j.status = JobStatus::Running;
        }
    });
    log::info!(
        "[mcp] job {} running as prompt {prompt_id}",
        tx.borrow().job_id
    );

    let mut outputs: Vec<Output> = Vec::new();
    let deadline = tokio::time::Instant::now() + RUN_TIMEOUT;
    let outcome = loop {
        if cancelled(tx) {
            break Outcome::Cancelled;
        }
        let evt = match tokio::time::timeout_at(
            deadline.min(tokio::time::Instant::now() + Duration::from_secs(2)),
            events.recv(),
        )
        .await
        {
            Err(_) if tokio::time::Instant::now() >= deadline => {
                break Outcome::Failed("Timed out waiting for ComfyUI".into());
            }
            Err(_) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                break Outcome::Failed("Event channel closed".into());
            }
            Ok(Ok(evt)) => evt,
        };
        let Some(raw) = evt.payload.get("prompt_id").and_then(|v| v.as_str()) else {
            continue;
        };
        if raw != prompt_id && state.prompt_queue.resolve_alias(raw) != prompt_id {
            continue;
        }
        match evt.event.as_str() {
            "comfyui:output_image" => {
                if let Some(name) = evt.payload.get("temp_filename").and_then(|v| v.as_str()) {
                    if !outputs.iter().any(|o| o.temp_filename == name) {
                        outputs.push(Output {
                            temp_filename: name.to_string(),
                            width: evt.payload["width"].as_u64().map(|v| v as u32),
                            height: evt.payload["height"].as_u64().map(|v| v as u32),
                        });
                    }
                }
            }
            "comfyui:executing" if evt.payload.get("node").is_some_and(|n| n.is_null()) => {
                break Outcome::Done;
            }
            "comfyui:execution_success" => break Outcome::Done,
            "comfyui:execution_interrupted" => break Outcome::Cancelled,
            "comfyui:execution_error" => {
                let msg = evt.payload["exception_message"]
                    .as_str()
                    .or_else(|| evt.payload["error"].as_str())
                    .unwrap_or("ComfyUI reported an error")
                    .trim()
                    .to_string();
                break Outcome::Failed(msg);
            }
            _ => {}
        }
    };

    match outcome {
        Outcome::Cancelled => {
            tx.send_modify(|j| {
                j.status = JobStatus::Cancelled;
                j.error.get_or_insert_with(|| "Cancelled".into());
            });
            return Ok(());
        }
        Outcome::Failed(msg) => return Err(msg),
        Outcome::Done => {}
    }

    // Outputs whose events were dropped (receiver lagged) are still cached.
    let cached: Vec<String> = state
        .output_image_cache
        .read()
        .unwrap()
        .get(&prompt_id)
        .cloned()
        .unwrap_or_default();
    for name in cached {
        if !outputs.iter().any(|o| o.temp_filename == name) {
            outputs.push(Output {
                temp_filename: name,
                width: None,
                height: None,
            });
        }
    }
    if outputs.is_empty() {
        return Err("ComfyUI finished without returning an image".into());
    }

    let saved = save_outputs(state, &params, seed, &prompt_id, &outputs).await?;
    tx.send_modify(|j| {
        j.status = JobStatus::Done;
        j.images = saved;
    });
    Ok(())
}

async fn save_outputs(
    state: &Arc<AppState>,
    params: &GenerationParams,
    seed: i64,
    prompt_id: &str,
    outputs: &[Output],
) -> Result<Vec<SavedImage>, String> {
    let dir = crate::config::gallery_dir().ok_or("Cannot find gallery directory")?;
    let template = state.config.read().await.output_filename_template.clone();
    let metadata: HashMap<String, String> = super::setups::image_metadata(params, seed);
    let mut saved = Vec::new();
    for (i, out) in outputs.iter().enumerate() {
        let Some(bytes) = crate::temp_images::load(&out.temp_filename) else {
            log::warn!(
                "[mcp] temp image {} expired before saving",
                out.temp_filename
            );
            continue;
        };
        let ext = out
            .temp_filename
            .rsplit_once('.')
            .map(|(_, e)| e)
            .unwrap_or("png");
        let filename = crate::webserver::save_to_gallery_in_dir(
            &dir,
            &bytes,
            &format!("{prompt_id}_{i}.{ext}"),
            prompt_id,
            Some("txt2img"),
            Some(&metadata),
            Some("both"),
            template.as_deref(),
        )?;
        let payload = serde_json::json!({
            "filename": filename,
            "mode": "txt2img",
            "source": "mcp",
            // Admin connection only; see the SSE `_target_user` filter.
            "_target_user": null,
        });
        state.broadcast("mcp:image_saved", payload.clone());
        emit_to_window(state, "mcp:image_saved", payload).await;
        saved.push(SavedImage {
            filename,
            width: out.width,
            height: out.height,
        });
    }
    if saved.is_empty() {
        return Err("The generated images expired before they could be saved".into());
    }
    Ok(saved)
}

/// Desktop window events go through Tauri, not the SSE broadcast.
#[cfg(feature = "desktop")]
async fn emit_to_window(state: &AppState, event: &str, payload: serde_json::Value) {
    use tauri::Emitter;
    if let Some(app) = state.app_handle.lock().await.as_ref() {
        let _ = app.emit(event, payload);
    }
}

#[cfg(not(feature = "desktop"))]
async fn emit_to_window(_state: &AppState, _event: &str, _payload: serde_json::Value) {}
