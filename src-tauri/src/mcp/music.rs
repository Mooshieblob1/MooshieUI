//! MCP music jobs: YuE2 songs through the same backend the Music page uses.
//!
//! Unlike images, the YuE2 graph is built in Rust from a few fields, so there
//! is no setup to reuse. Finished songs are copied out of ComfyUI into
//! `<app data>/music`, so the agent gets a local file even when the worker is
//! remote. They don't appear in the Music page library, which lives in the
//! browser.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::jobs::{JobStatus, Store, Tracked};
use crate::commands::music as music_cmd;
use crate::state::AppState;
use crate::templates::music::{self, MusicParams, MusicSampling, MusicTask};

/// Songs run long: up to six minutes of audio, plus the score plan.
const RUN_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// Same limit as the Music page before it reports the song as lost.
const MAX_MISSING_POLLS: u32 = 5;
const MAX_FAILED_POLLS: u32 = 10;
pub const DEFAULT_DURATION: f64 = 120.0;
pub const DEFAULT_STEPS: u32 = 32;

#[derive(Debug, Default, Deserialize)]
pub struct MusicRequest {
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub lyrics: String,
    pub checkpoint: Option<String>,
    pub duration_seconds: Option<f64>,
    pub steps: Option<u32>,
    pub seed: Option<i64>,
    pub planning: Option<String>,
    /// ABC score for the song to follow.
    pub score: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MusicJob {
    pub job_id: String,
    pub status: JobStatus,
    pub checkpoint: String,
    pub style: String,
    pub seed: String,
    /// The saved FLAC on the MooshieUI machine.
    pub file: Option<String>,
    /// The ABC score YuE2 planned or followed, saved next to the FLAC.
    pub score_file: Option<String>,
    pub generated_seconds: Option<f64>,
    pub error: Option<String>,
    #[serde(skip)]
    pub comfy_prompt_id: Option<String>,
}

impl Tracked for MusicJob {
    fn finished(&self) -> bool {
        self.status.finished()
    }
}

static JOBS: Store<MusicJob> = Store::new();

pub fn is_music_job(job_id: &str) -> bool {
    job_id.starts_with("mcp-music-")
}

pub fn get(job_id: &str) -> Option<watch::Receiver<MusicJob>> {
    JOBS.get(job_id)
}

pub fn active() -> Vec<MusicJob> {
    JOBS.active()
}

pub fn recent(n: usize) -> Vec<MusicJob> {
    JOBS.recent(n)
}

/// Pick a YuE2 checkpoint: exact, case-insensitive, file stem, or a unique
/// substring. Default: the first one installed.
pub fn pick_checkpoint(wanted: Option<&str>, installed: &[String]) -> Result<String, String> {
    if installed.is_empty() {
        return Err(
            "No YuE2 checkpoint is installed. Download one from MooshieUI's Music page.".into(),
        );
    }
    let Some(wanted) = wanted.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(installed[0].clone());
    };
    let lower = wanted.to_lowercase();
    let stem = |s: &str| {
        let s = s.rsplit(['/', '\\']).next().unwrap_or(s);
        s.rsplit_once('.').map_or(s, |(a, _)| a).to_lowercase()
    };
    if let Some(found) = installed.iter().find(|c| *c == wanted) {
        return Ok(found.clone());
    }
    if let Some(found) = installed
        .iter()
        .find(|c| c.to_lowercase() == lower || stem(c) == stem(wanted))
    {
        return Ok(found.clone());
    }
    let partial: Vec<&String> = installed
        .iter()
        .filter(|c| c.to_lowercase().contains(&lower))
        .collect();
    match partial.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!(
            "No YuE2 checkpoint matches '{wanted}'. Installed: {}",
            installed.join(", ")
        )),
        many => Err(format!(
            "'{wanted}' matches several checkpoints: {}",
            many.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Turn the agent's request into Music page params, with the page's defaults.
pub fn to_params(req: &MusicRequest, installed: &[String]) -> Result<MusicParams, String> {
    let score = req.score.as_deref().unwrap_or("").trim().to_string();
    let planning = match req.planning.as_deref().map(str::trim) {
        None | Some("") => "full".to_string(),
        Some(p) => p.to_lowercase(),
    };
    let params = MusicParams {
        checkpoint: pick_checkpoint(req.checkpoint.as_deref(), installed)?,
        style: req.style.trim().to_string(),
        lyrics: req.lyrics.trim().to_string(),
        planning,
        abc: score,
        cover: false,
        max_duration: req.duration_seconds.unwrap_or(DEFAULT_DURATION),
        steps: req.steps.unwrap_or(DEFAULT_STEPS),
        seed: req.seed.unwrap_or(-1),
        task: MusicTask::Audio,
        sampling: MusicSampling::default(),
    };
    music::validate(&params)?;
    Ok(params)
}

/// Queue a song and return a receiver that tracks it.
pub fn start(state: Arc<AppState>, req: MusicRequest) -> watch::Receiver<MusicJob> {
    let job = MusicJob {
        job_id: format!("mcp-music-{}", uuid::Uuid::new_v4().simple()),
        status: JobStatus::Queued,
        checkpoint: req.checkpoint.clone().unwrap_or_default(),
        style: req.style.trim().to_string(),
        seed: String::new(),
        file: None,
        score_file: None,
        generated_seconds: None,
        error: None,
        comfy_prompt_id: None,
    };
    let tx = JOBS.insert(job.job_id.clone(), job);
    let rx = tx.subscribe();
    tokio::spawn(async move {
        if let Err(e) = run(&state, &tx, req).await {
            tx.send_if_modified(|j| {
                if j.status.finished() {
                    return false;
                }
                j.status = JobStatus::Failed;
                j.error = Some(e.clone());
                true
            });
            log::warn!("[mcp] music job {} failed: {e}", tx.borrow().job_id);
        }
    });
    rx
}

/// Cancel a song. Interrupting goes through the shared queue, which also
/// frees the worker, the same way the Music page cancels.
pub async fn cancel(state: &Arc<AppState>, job_id: &str) -> Result<MusicJob, String> {
    let tx = JOBS
        .sender(job_id)
        .ok_or_else(|| format!("No job '{job_id}'"))?;
    let (was, prompt_id) = {
        let job = tx.borrow();
        (job.status, job.comfy_prompt_id.clone())
    };
    if !was.finished() {
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
    }
    let job = tx.borrow().clone();
    Ok(job)
}

fn cancelled(tx: &watch::Sender<MusicJob>) -> bool {
    tx.borrow().status == JobStatus::Cancelled
}

async fn run(
    state: &Arc<AppState>,
    tx: &watch::Sender<MusicJob>,
    req: MusicRequest,
) -> Result<(), String> {
    let caps = music_cmd::capabilities(state)
        .await
        .map_err(|e| format!("Music generation is unavailable: {e}"))?;
    if !caps.missing_nodes.is_empty() {
        return Err(format!(
            "YuE2 nodes are missing from ComfyUI: {}. Update the managed runtime from MooshieUI's Music page.",
            caps.missing_nodes.join(", ")
        ));
    }
    let params = to_params(&req, &caps.checkpoints)?;
    tx.send_modify(|j| j.checkpoint = params.checkpoint.clone());
    if cancelled(tx) {
        return Ok(());
    }
    let submitted = music_cmd::submit(state, params, None)
        .await
        .map_err(|e| e.to_string())?;
    let prompt_id = submitted["prompt_id"]
        .as_str()
        .ok_or("ComfyUI returned no prompt id")?
        .to_string();
    let worker_id = submitted["worker_id"]
        .as_u64()
        .ok_or("ComfyUI returned no worker id")? as u32;
    let seed = submitted["seed"].as_str().unwrap_or("").to_string();
    tx.send_modify(|j| {
        j.comfy_prompt_id = Some(prompt_id.clone());
        j.seed = seed.clone();
    });
    if cancelled(tx) {
        let _ = state.interrupt_prompt(Some(&prompt_id)).await;
        return Ok(());
    }
    log::info!(
        "[mcp] music job {} running as prompt {prompt_id}",
        tx.borrow().job_id
    );

    let deadline = tokio::time::Instant::now() + RUN_TIMEOUT;
    let (mut missing, mut failed) = (0, 0);
    let done = loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if cancelled(tx) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = state.interrupt_prompt(Some(&prompt_id)).await;
            return Err("Timed out waiting for ComfyUI".into());
        }
        let status = match music_cmd::status(state, &prompt_id, worker_id).await {
            Ok(s) => {
                failed = 0;
                s
            }
            Err(e) => {
                failed += 1;
                if failed >= MAX_FAILED_POLLS {
                    return Err(format!("Lost contact with ComfyUI: {e}"));
                }
                continue;
            }
        };
        match status["status"].as_str().unwrap_or("") {
            "completed" => break status,
            "error" => {
                return Err(status["error"]
                    .as_str()
                    .unwrap_or("Music generation failed")
                    .to_string())
            }
            "missing" => {
                missing += 1;
                if missing >= MAX_MISSING_POLLS {
                    return Err("ComfyUI no longer knows this song (was it restarted?)".into());
                }
            }
            s => {
                missing = 0;
                if s == "running" {
                    tx.send_if_modified(|j| {
                        let change = j.status == JobStatus::Queued;
                        if change {
                            j.status = JobStatus::Running;
                        }
                        change
                    });
                }
            }
        }
    };
    if done["filename"].as_str().is_none() {
        return Err("ComfyUI finished without a YuE2 audio file".into());
    }

    let audio = music_cmd::audio(state, &prompt_id, worker_id)
        .await
        .map_err(|e| e.to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(audio)
        .map_err(|e| format!("Invalid audio from ComfyUI: {e}"))?;
    let dir = music_dir().ok_or("Cannot find the app data directory")?;
    let abc = done["abc"].as_str().unwrap_or("").trim().to_string();
    let (file, score_file) = save(&dir, &file_stem(&seed), &bytes, &abc)
        .await
        .map_err(|e| format!("Could not save the song: {e}"))?;
    tx.send_modify(|j| {
        j.status = JobStatus::Done;
        j.file = Some(file.to_string_lossy().into_owned());
        j.score_file = score_file.map(|p| p.to_string_lossy().into_owned());
        j.generated_seconds = done["metadata"]["generated_seconds"].as_f64();
    });
    Ok(())
}

pub fn music_dir() -> Option<PathBuf> {
    crate::config::app_data_dir().map(|d| d.join("music"))
}

fn file_stem(seed: &str) -> String {
    let seed: String = seed.chars().filter(char::is_ascii_digit).take(20).collect();
    format!(
        "mcp_{}_{}",
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        if seed.is_empty() { "0" } else { &seed }
    )
}

async fn save(
    dir: &std::path::Path,
    stem: &str,
    flac: &[u8],
    abc: &str,
) -> std::io::Result<(PathBuf, Option<PathBuf>)> {
    tokio::fs::create_dir_all(dir).await?;
    let mut name = stem.to_string();
    let mut n = 1;
    while tokio::fs::try_exists(dir.join(format!("{name}.flac"))).await? {
        n += 1;
        name = format!("{stem}_{n}");
    }
    let file = dir.join(format!("{name}.flac"));
    tokio::fs::write(&file, flac).await?;
    let score = if abc.is_empty() {
        None
    } else {
        let path = dir.join(format!("{name}.abc"));
        tokio::fs::write(&path, abc).await?;
        Some(path)
    };
    Ok((file, score))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed() -> Vec<String> {
        vec![
            "yue2_base_bf16.safetensors".to_string(),
            "music/YuE2-large.safetensors".to_string(),
        ]
    }

    #[test]
    fn picks_checkpoints_like_the_image_setups() {
        let all = installed();
        assert_eq!(pick_checkpoint(None, &all).unwrap(), all[0]);
        assert_eq!(pick_checkpoint(Some(" "), &all).unwrap(), all[0]);
        assert_eq!(pick_checkpoint(Some("yue2-large"), &all).unwrap(), all[1]);
        assert_eq!(pick_checkpoint(Some("base"), &all).unwrap(), all[0]);
        assert!(pick_checkpoint(Some("yue2"), &all)
            .unwrap_err()
            .contains("several"));
        assert!(pick_checkpoint(Some("ace"), &all)
            .unwrap_err()
            .contains("Installed"));
        assert!(pick_checkpoint(None, &[])
            .unwrap_err()
            .contains("Music page"));
    }

    #[test]
    fn requests_get_music_page_defaults_and_validation() {
        let req = MusicRequest {
            style: " lo-fi hip hop, warm piano ".into(),
            ..Default::default()
        };
        let p = to_params(&req, &installed()).unwrap();
        assert_eq!(p.style, "lo-fi hip hop, warm piano");
        assert_eq!(p.planning, "full");
        assert_eq!(p.max_duration, DEFAULT_DURATION);
        assert_eq!(p.steps, DEFAULT_STEPS);
        assert_eq!(p.seed, -1);
        assert!(!p.cover);
        assert_eq!(p.task, MusicTask::Audio);

        let empty = MusicRequest::default();
        assert!(to_params(&empty, &installed()).is_err());
        let long = MusicRequest {
            style: "x".into(),
            duration_seconds: Some(400.0),
            ..Default::default()
        };
        assert!(to_params(&long, &installed()).is_err());
        let score_without_plan = MusicRequest {
            style: "x".into(),
            planning: Some("off".into()),
            score: Some("X:1\nK:C\nCDEF|".into()),
            ..Default::default()
        };
        assert!(to_params(&score_without_plan, &installed()).is_err());
        let melody = MusicRequest {
            style: "x".into(),
            planning: Some("Melody".into()),
            ..Default::default()
        };
        assert_eq!(to_params(&melody, &installed()).unwrap().planning, "melody");
    }

    #[test]
    fn music_job_ids_are_told_apart() {
        assert!(is_music_job("mcp-music-abc"));
        assert!(!is_music_job("mcp-abc"));
    }

    #[tokio::test]
    async fn saves_never_overwrite_and_write_the_score() {
        let dir = std::env::temp_dir().join(format!("mcp-music-{}", uuid::Uuid::new_v4()));
        let (a, sa) = save(&dir, "song", b"fLaC1", "X:1\nK:C\nC|").await.unwrap();
        let (b, sb) = save(&dir, "song", b"fLaC2", "").await.unwrap();
        assert_ne!(a, b);
        assert!(sa.is_some() && sb.is_none());
        assert_eq!(std::fs::read(&a).unwrap(), b"fLaC1");
        assert_eq!(std::fs::read(&b).unwrap(), b"fLaC2");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn file_stems_keep_only_digits_of_the_seed() {
        assert!(file_stem("12/../3").ends_with("_123"));
        assert!(file_stem("").ends_with("_0"));
    }
}
