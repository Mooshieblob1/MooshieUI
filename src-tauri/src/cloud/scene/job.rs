//! The scene job: everything is checked and assembled before an id exists,
//! then a background task submits to the video provider, polls, downloads
//! and ingests the clip into the gallery (research doc 5.3).
//!
//! It speaks the same event contract as a NovelAI generation (`comfyui:*`
//! with a synthetic prompt id), so the queue, progress and gallery work
//! unchanged. Paid-work rules: the provider job is written to disk as soon as
//! it exists so a restart resumes polling instead of paying again, and a
//! downloaded clip is kept until it is safely in the gallery.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::cloud::elevenlabs::Moderated;
use crate::cloud::takes::{self, TakeStore};
use crate::cloud::video::fal::{self, FalClient, QueueHandle, QueueStatus};
use crate::cloud::video::{VideoCapabilities, VideoModel};
use crate::cloud::{resolve_credential, CloudCredential};
use crate::error::AppError;
use crate::novelai::EventSink;
use crate::state::AppState;

use super::audio::{self, LineWindow, VideoInfo};
use super::mouth;
use super::price::VideoEstimate;
use super::prompt::{self, ImageRole, SceneSpec};
use super::timeline::{self, LineInput, ShotInput};

const JOB_FILE: &str = "job.json";
const JOB_VERSION: u32 = 1;
/// Longest side of a reference image sent to the provider. Larger adds
/// upload size, not detail the model uses.
const MAX_REFERENCE_SIDE: u32 = 2048;
const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Consecutive failed polls before the job is parked for a later resume.
const MAX_POLL_ERRORS: u32 = 12;
/// How long one session keeps polling before parking the job.
const MAX_POLL_TIME: Duration = Duration::from_secs(3 * 60 * 60);
const NODE_LABEL: &str = "Seedance";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneImage {
    pub filename: String,
    pub role: ImageRole,
}

/// Everything the Scenes page sends to plan or generate a scene.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneRequest {
    pub model: VideoModel,
    pub resolution: String,
    pub draft: bool,
    pub aspect: String,
    pub seed: Option<u32>,
    #[serde(default)]
    pub continuous: bool,
    #[serde(default)]
    pub character_name: String,
    #[serde(default)]
    pub character_traits: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub starting_state: String,
    #[serde(default)]
    pub ending_state: String,
    #[serde(default)]
    pub ambience: String,
    #[serde(default)]
    pub minor: bool,
    #[serde(default)]
    pub voice_id: String,
    pub lines: Vec<LineInput>,
    pub shots: Vec<ShotInput>,
    #[serde(default)]
    pub tail: f64,
    pub images: Vec<SceneImage>,
    /// The user's edit of the built prompt (research doc 7, requirement 5).
    #[serde(default)]
    pub prompt_override: Option<String>,
    /// Use the measured mouth timing of each take (rule S10) where it exists.
    #[serde(default)]
    pub mouth_map: bool,
}

/// Longest prompt accepted from the editor.
const MAX_PROMPT_CHARS: usize = 8000;

/// The user may edit the built prompt, but a minor-flagged scene keeps its
/// age-appropriate constraints whatever the edit says.
fn final_prompt(built: String, edited: Option<&str>, minor: bool) -> Result<String, String> {
    let Some(edited) = edited.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(built);
    };
    if edited.chars().count() > MAX_PROMPT_CHARS {
        return Err(format!(
            "The prompt can be at most {MAX_PROMPT_CHARS} characters."
        ));
    }
    let mut prompt = edited.to_string();
    if minor && !prompt.contains(prompt::MINOR_CONSTRAINTS) {
        prompt.push_str(&format!("\n\nConstraints: {}.", prompt::MINOR_CONSTRAINTS));
    }
    Ok(prompt)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ShotSpan {
    pub start: f64,
    pub end: f64,
}

/// What the confirmation dialog and prompt preview show.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScenePlan {
    pub prompt: String,
    pub builder_version: u32,
    pub seconds: u32,
    pub track_seconds: f64,
    pub windows: Vec<LineWindow>,
    pub shots: Vec<ShotSpan>,
    pub estimate: VideoEstimate,
    pub model_label: &'static str,
    /// Lines whose mouth timing is in the prompt.
    pub mouth_measured: usize,
    /// Lines whose take has not been measured yet.
    pub mouth_missing: usize,
}

/// What is written into the clip's metadata and the job file, so the scene
/// can be rebuilt. Never holds a key or a signed URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneRecipe {
    pub builder_version: u32,
    pub model: VideoModel,
    pub resolution: String,
    pub draft: bool,
    pub seconds: u32,
    pub aspect: String,
    pub seed: Option<u32>,
    pub prompt: String,
    pub voice_id: String,
    pub character_name: String,
    pub minor: bool,
    pub lines: Vec<LineInput>,
    pub images: Vec<SceneImage>,
    pub estimate_usd: f64,
    /// The provider request id of the draft this clip completes.
    #[serde(default)]
    pub upgraded_from: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum JobStage {
    /// The provider has the job; poll it.
    Submitted,
    /// The clip is on disk in the job folder; ingest it.
    Downloaded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    version: u32,
    prompt_id: String,
    handle: QueueHandle,
    stage: JobStage,
    created_unix: i64,
    provider_seed: Option<i64>,
    draft_id: Option<String>,
    recipe: SceneRecipe,
    /// Id of the voice track kept in `tracks/` for the exact-voice option.
    #[serde(default)]
    voice_track: Option<String>,
}

impl JobRecord {
    pub fn prompt_id(&self) -> &str {
        &self.prompt_id
    }
}

/// A scene ready to submit: the body is built, nothing has been paid.
pub struct PreparedScene {
    prompt_id: String,
    dir: PathBuf,
    endpoint: &'static str,
    body: serde_json::Value,
    recipe: SceneRecipe,
    credential: CloudCredential,
    voice_track: Option<String>,
}

impl PreparedScene {
    pub fn prompt_id(&self) -> &str {
        &self.prompt_id
    }
}

/// What a background run starts from.
pub enum JobStart {
    New(PreparedScene),
    Resume(JobRecord, CloudCredential),
}

impl JobStart {
    pub fn prompt_id(&self) -> &str {
        match self {
            JobStart::New(p) => &p.prompt_id,
            JobStart::Resume(r, _) => &r.prompt_id,
        }
    }
}

/// Scene jobs running in this process, so a resume never starts a second
/// poller for a job that is already being followed.
fn active() -> &'static Mutex<HashSet<String>> {
    static ACTIVE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn jobs_dir(username: Option<&str>) -> Result<PathBuf, AppError> {
    takes::scene_assets_dir(username)
        .map(|d| d.join("jobs"))
        .ok_or_else(|| AppError::Other("Cannot locate the scene asset folder.".into()))
}

/// Voice tracks kept after upload, named by the scene that made them.
fn tracks_dir(username: Option<&str>) -> Result<PathBuf, AppError> {
    takes::scene_assets_dir(username)
        .map(|d| d.join("tracks"))
        .ok_or_else(|| AppError::Other("Cannot locate the scene asset folder.".into()))
}

fn valid_job_id(id: &str) -> bool {
    id.strip_prefix("scene-")
        .and_then(|u| uuid::Uuid::parse_str(u).ok())
        .is_some()
}

fn ffmpeg_path(state: &AppState) -> Result<PathBuf, AppError> {
    state.media_tools.path("ffmpeg").ok_or_else(|| {
        AppError::Other(
            "Scenes need FFmpeg to build the voice track. Set up the music tools on the Music page first."
                .into(),
        )
    })
}

pub fn capabilities() -> Vec<VideoCapabilities> {
    crate::cloud::video::all_capabilities()
}

/// Checks that need no file or network access.
fn validate_request(request: &SceneRequest, caps: &VideoCapabilities) -> Result<(), String> {
    if !caps.resolutions.contains(&request.resolution.as_str()) {
        return Err("Pick a resolution the video model supports.".into());
    }
    if !caps.aspect_ratios.contains(&request.aspect.as_str()) {
        return Err("Pick an aspect ratio the video model supports.".into());
    }
    if request.draft && !caps.draft {
        return Err("This video model has no draft mode.".into());
    }
    if request.images.is_empty() {
        return Err("Add at least one reference image of the character.".into());
    }
    if request.images.len() > caps.max_images {
        return Err(format!("Use at most {} reference images.", caps.max_images));
    }
    if !request
        .images
        .iter()
        .any(|i| i.role == ImageRole::Character)
    {
        return Err("Mark one reference image as the character.".into());
    }
    for line in &request.lines {
        if line.text.trim().is_empty() {
            return Err("A line in the scene has no text.".into());
        }
        if !takes::valid_take_id(&line.take_id) {
            return Err("A line in the scene has no recorded take.".into());
        }
    }
    Ok(())
}

struct Planned {
    plan: ScenePlan,
    timeline: timeline::Timeline,
    caps: VideoCapabilities,
    store: TakeStore,
}

/// Measure the takes, lay out the timeline and build the prompt. Spends
/// nothing and needs no provider key.
async fn plan_inner(
    state: &Arc<AppState>,
    username: Option<&str>,
    request: &SceneRequest,
) -> Result<Planned, AppError> {
    let caps = request.model.capabilities();
    validate_request(request, &caps).map_err(AppError::Other)?;
    let store = TakeStore::for_account(username)
        .ok_or_else(|| AppError::Other("Cannot locate the scene asset folder.".into()))?;
    let ffmpeg = ffmpeg_path(state)?;

    let used: HashSet<&str> = request
        .shots
        .iter()
        .filter_map(|s| s.line_id.as_deref())
        .collect();
    let mut durations = HashMap::new();
    for line in request
        .lines
        .iter()
        .filter(|l| used.contains(l.line_id.as_str()))
    {
        let path = store.path_of(&line.take_id).ok_or_else(|| {
            AppError::Other("A chosen take is no longer on disk. Record it again.".into())
        })?;
        let seconds = audio::probe_duration(&ffmpeg, &path)
            .await
            .map_err(AppError::Other)?;
        durations.insert(line.line_id.clone(), seconds);
    }

    let mut timeline = timeline::build(
        &request.shots,
        &request.lines,
        &durations,
        request.tail,
        caps.min_seconds,
        caps.max_seconds,
    )
    .map_err(AppError::Other)?;
    if timeline.track.total > caps.audio_max_seconds {
        return Err(AppError::Other(format!(
            "The voice track is longer than the {} seconds the video model accepts.",
            caps.audio_max_seconds
        )));
    }

    let (mouth_measured, mouth_missing) = if request.mouth_map {
        apply_mouth_map(&mut timeline, request, &store)
    } else {
        (0, 0)
    };

    let spec = SceneSpec {
        seconds: timeline.seconds,
        aspect: request.aspect.clone(),
        continuous: request.continuous,
        character_name: request.character_name.clone(),
        character_traits: request.character_traits.clone(),
        images: request.images.iter().map(|i| i.role.clone()).collect(),
        has_voice_track: true,
        language: request.language.clone(),
        starting_state: request.starting_state.clone(),
        shots: timeline.shots.clone(),
        ambience: request.ambience.clone(),
        ending_state: request.ending_state.clone(),
        minor: request.minor,
    };
    let prompt = final_prompt(
        prompt::build(&spec).map_err(AppError::Other)?,
        request.prompt_override.as_deref(),
        request.minor,
    )
    .map_err(AppError::Other)?;
    let estimate = request
        .model
        .estimate(&request.resolution, timeline.seconds, request.draft)
        .ok_or_else(|| AppError::Other("No price is known for this resolution.".into()))?;

    let plan = ScenePlan {
        prompt,
        builder_version: prompt::BUILDER_VERSION,
        seconds: timeline.seconds,
        track_seconds: timeline.track.total,
        windows: timeline.track.windows.clone(),
        shots: timeline
            .shots
            .iter()
            .map(|s| ShotSpan {
                start: s.start,
                end: s.end,
            })
            .collect(),
        estimate,
        model_label: caps.label,
        mouth_measured,
        mouth_missing,
    };
    Ok(Planned {
        plan,
        timeline,
        caps,
        store,
    })
}

/// Put each measured take's talking stretches into its line (rule S10).
/// Returns how many lines got a map and how many takes are not measured.
fn apply_mouth_map(
    timeline: &mut timeline::Timeline,
    request: &SceneRequest,
    store: &TakeStore,
) -> (usize, usize) {
    let take_of: HashMap<&str, &str> = request
        .lines
        .iter()
        .map(|l| (l.line_id.as_str(), l.take_id.as_str()))
        .collect();
    let (mut measured, mut missing) = (0, 0);
    // The timeline keeps the request's shot order.
    for (shot, input) in timeline.shots.iter_mut().zip(&request.shots) {
        let (Some(line), Some(line_id)) = (shot.line.as_mut(), input.line_id.as_deref()) else {
            continue;
        };
        match take_of.get(line_id).and_then(|take| store.mouth(take)) {
            Some(map) => {
                line.talking = mouth::place(&map.spans, line.start, line.end);
                measured += 1;
            }
            None => missing += 1,
        }
    }
    (measured, missing)
}

pub async fn plan(
    state: &Arc<AppState>,
    username: Option<&str>,
    request: &SceneRequest,
) -> Result<ScenePlan, AppError> {
    Ok(plan_inner(state, username, request).await?.plan)
}

fn data_uri(media_type: &str, bytes: &[u8]) -> String {
    format!(
        "data:{media_type};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// A gallery image as the provider gets it: decoded, shrunk to a sane size
/// and re-encoded as JPEG, which also drops every embedded metadata chunk
/// (prompts and settings stay on this machine).
fn reference_jpeg(png: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory(png)
        .map_err(|e| format!("Could not read a reference image: {e}"))?;
    let image = if image.width().max(image.height()) > MAX_REFERENCE_SIDE {
        image.resize(
            MAX_REFERENCE_SIDE,
            MAX_REFERENCE_SIDE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        image
    };
    let rgb = image.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92)
        .encode_image(&rgb)
        .map_err(|e| format!("Could not encode a reference image: {e}"))?;
    Ok(out)
}

/// Build everything the provider needs: the voice track, the reference
/// images and the request body. Spends nothing; the id is not in the queue
/// yet, so a failure here is an ordinary command error.
pub async fn prepare(
    state: &Arc<AppState>,
    username: Option<&str>,
    request: &SceneRequest,
) -> Result<PreparedScene, AppError> {
    let planned = plan_inner(state, username, request).await?;
    let credential = resolve_credential(state, username, planned.caps.provider).await?;
    let ffmpeg = ffmpeg_path(state)?;
    let gallery = crate::webserver::user_gallery_dir(username)
        .ok_or_else(|| AppError::Other("Cannot find the gallery folder.".into()))?;

    let mut images = Vec::with_capacity(request.images.len());
    for image in &request.images {
        let png = crate::commands::api::load_gallery_image_png_from_dir(&gallery, &image.filename)
            .await?;
        let jpeg = tokio::task::spawn_blocking(move || reference_jpeg(&png))
            .await
            .map_err(|e| AppError::Other(format!("Task failed: {e}")))?
            .map_err(AppError::Other)?;
        if jpeg.len() > planned.caps.max_image_bytes {
            return Err(AppError::Other(
                "A reference image is too large for the video model.".into(),
            ));
        }
        images.push(data_uri("image/jpeg", &jpeg));
    }

    let prompt_id = format!("scene-{}", uuid::Uuid::new_v4());
    let dir = jobs_dir(username)?.join(&prompt_id);
    std::fs::create_dir_all(&dir)?;
    let result = build_track(&ffmpeg, &planned, request, &dir).await;
    let track = match result {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    if track.len() > planned.caps.max_audio_bytes {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(AppError::Other(
            "The voice track is too large for the video model.".into(),
        ));
    }

    let body = match request.model {
        VideoModel::FalSeedance25 => fal::seedance_25_body(
            &planned.plan.prompt,
            &images,
            Some(&data_uri("audio/wav", &track)),
            &request.resolution,
            request.draft,
            planned.timeline.seconds,
            &request.aspect,
            request.seed,
        ),
    };
    let used: HashSet<&str> = planned
        .timeline
        .planned
        .iter()
        .map(|l| l.line_id.as_str())
        .collect();
    let recipe = SceneRecipe {
        builder_version: prompt::BUILDER_VERSION,
        model: request.model,
        resolution: request.resolution.clone(),
        draft: request.draft,
        seconds: planned.timeline.seconds,
        aspect: request.aspect.clone(),
        seed: request.seed,
        prompt: planned.plan.prompt.clone(),
        voice_id: request.voice_id.clone(),
        character_name: request.character_name.clone(),
        minor: request.minor,
        lines: request
            .lines
            .iter()
            .filter(|l| used.contains(l.line_id.as_str()))
            .cloned()
            .collect(),
        images: request.images.clone(),
        estimate_usd: planned.plan.estimate.usd,
        upgraded_from: None,
    };
    Ok(PreparedScene {
        voice_track: Some(prompt_id.clone()),
        prompt_id,
        dir,
        endpoint: fal::SEEDANCE_25_REFERENCE,
        body,
        recipe,
        credential,
    })
}

async fn build_track(
    ffmpeg: &Path,
    planned: &Planned,
    request: &SceneRequest,
    dir: &Path,
) -> Result<Vec<u8>, AppError> {
    let take_of: HashMap<&str, &str> = request
        .lines
        .iter()
        .map(|l| (l.line_id.as_str(), l.take_id.as_str()))
        .collect();
    let mut inputs = Vec::new();
    for line in &planned.timeline.planned {
        let path = take_of
            .get(line.line_id.as_str())
            .and_then(|id| planned.store.path_of(id))
            .ok_or_else(|| AppError::Other("A chosen take is no longer on disk.".into()))?;
        inputs.push((path, line.silence_before));
    }
    let out = dir.join("voice.wav");
    let args = audio::track_args(&inputs, planned.timeline.tail, &out);
    audio::run_ffmpeg(ffmpeg, &args, false)
        .await
        .map_err(AppError::Other)?;
    Ok(std::fs::read(&out)?)
}

/// Unfinished jobs on disk for this account, oldest first, that are not
/// already being followed in this process.
pub fn resumable(username: Option<&str>) -> Vec<JobRecord> {
    let running = active().lock().map(|a| a.clone()).unwrap_or_default();
    let mut records: Vec<JobRecord> = job_records(username)
        .into_iter()
        .filter(|r| !running.contains(&r.prompt_id))
        .collect();
    records.sort_by_key(|r| r.created_unix);
    records
}

/// Every submitted job still on disk for this account, running or not.
fn job_records(username: Option<&str>) -> Vec<JobRecord> {
    let Ok(dir) = jobs_dir(username) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(valid_job_id))
        .filter_map(|e| std::fs::read(e.path().join(JOB_FILE)).ok())
        .filter_map(|bytes| serde_json::from_slice::<JobRecord>(&bytes).ok())
        .filter(|r| r.version == JOB_VERSION && valid_job_id(&r.prompt_id))
        .collect()
}

/// Claim a job for this process. False when it is already being followed.
pub fn claim(prompt_id: &str) -> bool {
    active()
        .lock()
        .map(|mut a| a.insert(prompt_id.to_string()))
        .unwrap_or(false)
}

fn release(prompt_id: &str) {
    if let Ok(mut a) = active().lock() {
        a.remove(prompt_id);
    }
}

/// Put a claimed job in the prompt queue and run it in the background.
/// Returns its prompt id.
pub fn spawn(
    state: &Arc<AppState>,
    username: Option<String>,
    start: JobStart,
    sink: EventSink,
) -> String {
    let prompt_id = start.prompt_id().to_string();
    // Inserted before spawning: the frontend reconciler drops prompts it
    // cannot find in the queue after 30 s.
    state.prompt_queue.insert(&prompt_id, username.clone());
    state.broadcast_queue_positions();
    let bg_state = Arc::clone(state);
    let bg_id = prompt_id.clone();
    tokio::spawn(async move {
        if let Err(e) = run(Arc::clone(&bg_state), sink, username, start).await {
            log::error!("[scene] job {bg_id} failed: {e}");
        }
        bg_state.prompt_queue.cancel_and_remove(&bg_id);
        bg_state.broadcast_queue_positions();
    });
    prompt_id
}

/// Prepare and start a new scene. Everything that can fail without spending
/// fails here, before the id reaches the queue.
pub async fn start(
    state: &Arc<AppState>,
    username: Option<&str>,
    request: &SceneRequest,
    sink: EventSink,
) -> Result<String, AppError> {
    let prepared = prepare(state, username, request).await?;
    claim(&prepared.prompt_id);
    Ok(spawn(
        state,
        username.map(str::to_string),
        JobStart::New(prepared),
        sink,
    ))
}

/// Pick up this account's unfinished scenes: poll a submitted job, or add an
/// already downloaded clip to the gallery. Never submits anything new, so it
/// costs nothing. Returns the prompt ids now running.
pub async fn resume(
    state: &Arc<AppState>,
    username: Option<&str>,
    make_sink: impl Fn() -> EventSink,
) -> Result<Vec<String>, AppError> {
    let records = resumable(username);
    let mut ids = Vec::new();
    for record in records {
        let provider = record.recipe.model.capabilities().provider;
        let credential = match resolve_credential(state, username, provider).await {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[scene] cannot resume {}: {e}", record.prompt_id);
                continue;
            }
        };
        if !claim(&record.prompt_id) {
            continue;
        }
        ids.push(spawn(
            state,
            username.map(str::to_string),
            JobStart::Resume(record, credential),
            make_sink(),
        ));
    }
    Ok(ids)
}

fn write_record(dir: &Path, record: &JobRecord) -> Result<(), AppError> {
    let tmp = dir.join(format!("{JOB_FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(record)?)?;
    std::fs::rename(&tmp, dir.join(JOB_FILE))?;
    Ok(())
}

fn progress(sink: &EventSink, prompt_id: &str, value: u32) {
    sink.emit(
        "comfyui:progress",
        serde_json::json!({ "prompt_id": prompt_id, "value": value, "max": 100, "node": NODE_LABEL }),
    );
}

/// Rough progress from elapsed time, since the provider reports none. A
/// draft usually takes a few minutes; this never claims to be done.
fn time_progress(elapsed: Duration, seconds: u32) -> u32 {
    let expected = 60.0 + f64::from(seconds) * 8.0;
    (5.0 + 90.0 * (elapsed.as_secs_f64() / expected).min(1.0)) as u32
}

/// Run a scene job in the background. Errors are emitted as
/// `comfyui:execution_error` and returned for the caller to log. The caller
/// must have claimed the id and inserted it into the prompt queue.
pub async fn run(
    state: Arc<AppState>,
    sink: EventSink,
    username: Option<String>,
    start: JobStart,
) -> Result<(), AppError> {
    let prompt_id = start.prompt_id().to_string();
    let result = run_inner(&state, &sink, username.as_deref(), start).await;
    release(&prompt_id);
    if let Err(err) = &result {
        sink.emit(
            "comfyui:execution_error",
            serde_json::json!({
                "prompt_id": prompt_id,
                "error": err.to_string(),
                "exception_message": err.to_string(),
                "node_type": NODE_LABEL,
            }),
        );
    }
    result
}

async fn run_inner(
    state: &Arc<AppState>,
    sink: &EventSink,
    username: Option<&str>,
    start: JobStart,
) -> Result<(), AppError> {
    let (dir, mut record, credential) = match start {
        JobStart::New(prepared) => {
            progress(sink, &prepared.prompt_id, 0);
            let client = FalClient::new(&state.http_client, &prepared.credential);
            let submitted = client.submit(prepared.endpoint, &prepared.body).await;
            let handle = match submitted {
                Ok(Moderated::Ok { value }) => value,
                Ok(Moderated::Blocked { message }) => {
                    let _ = std::fs::remove_dir_all(&prepared.dir);
                    return Err(AppError::Other(message));
                }
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&prepared.dir);
                    return Err(e);
                }
            };
            let record = JobRecord {
                version: JOB_VERSION,
                prompt_id: prepared.prompt_id.clone(),
                handle,
                stage: JobStage::Submitted,
                created_unix: chrono::Utc::now().timestamp(),
                provider_seed: None,
                draft_id: None,
                recipe: prepared.recipe,
                voice_track: prepared.voice_track,
            };
            // Written before anything else can fail: from here on the job is
            // paid for, and only this file lets a restart find it again.
            if let Err(e) = write_record(&prepared.dir, &record) {
                log::error!("[scene] could not save job {}: {e}", record.prompt_id);
            }
            // voice.wav stays: ingest keeps it for the exact-voice option.
            (prepared.dir, record, prepared.credential)
        }
        JobStart::Resume(record, credential) => {
            let dir = jobs_dir(username)?.join(&record.prompt_id);
            (dir, record, credential)
        }
    };
    let prompt_id = record.prompt_id.clone();
    let client = FalClient::new(&state.http_client, &credential);

    if record.stage == JobStage::Submitted {
        match poll(state, sink, &client, &record).await? {
            PollOutcome::Cancelled => {
                client.cancel(&record.handle).await;
                let _ = std::fs::remove_dir_all(&dir);
                return Ok(());
            }
            PollOutcome::Failed(message) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(AppError::Other(message));
            }
            PollOutcome::Completed => {}
        }
        let result = match client.result(&record.handle).await? {
            Moderated::Ok { value } => value,
            Moderated::Blocked { message } => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(AppError::Other(message));
            }
        };
        client.download(&result.url, &dir.join("raw.mp4")).await?;
        record.stage = JobStage::Downloaded;
        record.provider_seed = result.seed;
        record.draft_id = result.draft_id;
        if let Err(e) = write_record(&dir, &record) {
            log::error!("[scene] could not update job {prompt_id}: {e}");
        }
    }

    // A clip that is already downloaded is paid for, so it goes to the
    // gallery even if the job was cancelled in the last moments; the
    // frontend has dropped the prompt by then and ignores the events.
    progress(sink, &prompt_id, 97);
    let payload = ingest(state, username, &dir, &record).await.map_err(|e| {
        AppError::Other(format!(
            "The video was made but could not be added to the gallery ({e}). It is kept in {} and the Scenes page offers to retry.",
            dir.display()
        ))
    })?;
    let _ = std::fs::remove_dir_all(&dir);
    sink.emit("comfyui:output_video", payload);
    sink.emit(
        "comfyui:executing",
        serde_json::json!({ "prompt_id": prompt_id, "node": serde_json::Value::Null }),
    );
    Ok(())
}

enum PollOutcome {
    Completed,
    Cancelled,
    Failed(String),
}

async fn poll(
    state: &Arc<AppState>,
    sink: &EventSink,
    client: &FalClient<'_>,
    record: &JobRecord,
) -> Result<PollOutcome, AppError> {
    let prompt_id = &record.prompt_id;
    let started = Instant::now();
    let mut errors = 0;
    let mut running_since: Option<Instant> = None;
    loop {
        if state.prompt_queue.is_cancelled(prompt_id) {
            return Ok(PollOutcome::Cancelled);
        }
        match client.status(&record.handle).await {
            Ok(QueueStatus::Completed) => return Ok(PollOutcome::Completed),
            Ok(QueueStatus::Failed { message, .. }) => return Ok(PollOutcome::Failed(message)),
            Ok(QueueStatus::Queued { .. }) => {
                errors = 0;
                progress(sink, prompt_id, 2);
            }
            Ok(QueueStatus::Running) => {
                errors = 0;
                let since = *running_since.get_or_insert_with(Instant::now);
                progress(
                    sink,
                    prompt_id,
                    time_progress(since.elapsed(), record.recipe.seconds),
                );
            }
            Err(e) => {
                errors += 1;
                log::warn!("[scene] polling {prompt_id} failed ({errors}): {e}");
                if errors >= MAX_POLL_ERRORS {
                    return Err(parked(e));
                }
            }
        }
        if started.elapsed() > MAX_POLL_TIME {
            return Err(AppError::Other(
                "fal.ai has not finished this scene yet. It is saved; open the Scenes page later to pick it up.".into(),
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn parked(error: AppError) -> AppError {
    AppError::Other(format!(
        "Lost contact with fal.ai ({error}). The scene is saved; open the Scenes page later to pick it up without paying again."
    ))
}

/// The JSON written into the clip, in the SwarmUI shape the gallery reads,
/// plus the full scene recipe.
fn metadata_json(record: &JobRecord, width: u32, height: u32, voice_track: Option<&str>) -> String {
    let recipe = &record.recipe;
    let seed = record
        .provider_seed
        .map(|s| s.to_string())
        .or_else(|| recipe.seed.map(|s| s.to_string()));
    serde_json::json!({
        "sui_image_params": {
            "prompt": recipe.prompt,
            "model": recipe.model.capabilities().label,
            "seed": seed,
            "width": width,
            "height": height,
            "generationmode": "anime_scene",
        },
        "sui_extra_data": {
            "date": chrono::Local::now().format("%Y-%m-%d").to_string(),
        },
        "mooshie_scene": {
            "recipe": recipe,
            "provider_seed": record.provider_seed,
            "draft_id": record.draft_id,
            "request_id": record.handle.request_id,
            "created_unix": record.created_unix,
            "voice_track": voice_track,
            "audio": "model",
        },
    })
    .to_string()
}

/// Copy the streams untouched and write the metadata as an mdta `comment`,
/// the shape the gallery's reader and `mirror_uuid_sidecar` expect.
fn tag_args(raw: &Path, out: &Path, comment: &str) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-y".into(),
        "-i".into(),
        raw.to_string_lossy().into_owned(),
        "-map".into(),
        "0".into(),
        "-c".into(),
        "copy".into(),
        "-metadata".into(),
        format!("comment={comment}"),
        "-movflags".into(),
        "+faststart+use_metadata_tags".into(),
        out.to_string_lossy().into_owned(),
    ]
}

/// Keep the clip's picture and replace its sound with the user's own voice
/// track (research doc 5.4). The video stream is copied, never re-encoded.
fn exact_voice_args(video: &Path, track: &Path, out: &Path, comment: &str) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-y".into(),
        "-i".into(),
        video.to_string_lossy().into_owned(),
        "-i".into(),
        track.to_string_lossy().into_owned(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "1:a:0".into(),
        "-c:v".into(),
        "copy".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "192k".into(),
        "-metadata".into(),
        format!("comment={comment}"),
        "-movflags".into(),
        "+faststart+use_metadata_tags".into(),
        out.to_string_lossy().into_owned(),
    ]
}

fn poster_args(video: &Path, frame: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-y".into(),
        "-i".into(),
        video.to_string_lossy().into_owned(),
        "-frames:v".into(),
        "1".into(),
        frame.to_string_lossy().into_owned(),
    ]
}

async fn probe_video(ffmpeg: &Path, video: &Path) -> Result<VideoInfo, AppError> {
    let banner = audio::run_ffmpeg(
        ffmpeg,
        &[
            "-hide_banner".into(),
            "-i".into(),
            video.to_string_lossy().into_owned(),
        ],
        true,
    )
    .await
    .map_err(AppError::Other)?;
    audio::parse_video_info(&banner)
        .ok_or_else(|| AppError::Other("The file is not a readable video.".into()))
}

/// Move the voice track out of the job folder into `tracks/`, so it outlives
/// the job. Returns the track id when the track is kept.
fn keep_track(username: Option<&str>, dir: &Path, record: &JobRecord) -> Option<String> {
    let id = record
        .voice_track
        .as_deref()
        .filter(|id| valid_job_id(id))?;
    let tracks = tracks_dir(username).ok()?;
    let dest = tracks.join(format!("{id}.wav"));
    let src = dir.join("voice.wav");
    if src.is_file() && !dest.is_file() {
        let moved = std::fs::create_dir_all(&tracks).and_then(|_| {
            std::fs::rename(&src, &dest).or_else(|_| std::fs::copy(&src, &dest).map(|_| ()))
        });
        if let Err(e) = moved {
            log::warn!("[scene] could not keep the voice track of {id}: {e}");
        }
    }
    dest.is_file().then(|| id.to_string())
}

/// Grab a poster and move a finished clip into the account's gallery.
/// Returns the `comfyui:output_video` payload.
async fn save_clip(
    ffmpeg: &Path,
    username: Option<&str>,
    work: &Path,
    video: PathBuf,
    info: VideoInfo,
    prompt_id: &str,
) -> Result<serde_json::Value, AppError> {
    let frame = work.join("poster.png");
    let poster = work.join("poster.webp");
    let poster_ok = match audio::run_ffmpeg(ffmpeg, &poster_args(&video, &frame), false).await {
        Ok(_) => {
            let (frame, poster) = (frame.clone(), poster.clone());
            tokio::task::spawn_blocking(move || -> Result<(), String> {
                let img = image::open(&frame).map_err(|e| e.to_string())?;
                img.save_with_format(&poster, image::ImageFormat::WebP)
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r)
            .map_err(|e| log::warn!("[scene] poster encode failed: {e}"))
            .is_ok()
        }
        Err(e) => {
            log::warn!("[scene] poster frame grab failed: {e}");
            false
        }
    };

    let gallery = crate::webserver::user_gallery_dir(username)
        .ok_or_else(|| AppError::Other("Cannot find the gallery folder.".into()))?;
    let id = prompt_id.to_string();
    let frame_count = (info.duration * info.fps).round().max(0.0) as u64;
    let poster_path = poster_ok.then_some(poster);
    // A paid clip always goes to the gallery, even in manual save mode:
    // a held-back clip that is never saved would be paid work thrown away.
    let saved = tokio::task::spawn_blocking(move || {
        crate::commands::api::save_video_to_gallery(
            &video,
            poster_path.as_deref(),
            &gallery,
            &id,
            info.fps,
            frame_count,
            info.width,
            info.height,
        )
    })
    .await
    .map_err(|e| AppError::Other(format!("Task failed: {e}")))??;

    Ok(serde_json::json!({
        "type": "video",
        "prompt_id": prompt_id,
        "persisted": true,
        "video_filename": saved.video_filename,
        "poster_filename": saved.poster_filename,
        "duration_seconds": info.duration,
        "fps": info.fps,
        "frame_count": frame_count,
        "width": info.width,
        "height": info.height,
    }))
}

/// Remux the downloaded clip with its metadata and add it to the gallery.
async fn ingest(
    state: &Arc<AppState>,
    username: Option<&str>,
    dir: &Path,
    record: &JobRecord,
) -> Result<serde_json::Value, AppError> {
    let ffmpeg = ffmpeg_path(state)?;
    let raw = dir.join("raw.mp4");
    let info = probe_video(&ffmpeg, &raw).await?;
    let voice_track = keep_track(username, dir, record);
    let tagged = dir.join("scene.mp4");
    let args = tag_args(
        &raw,
        &tagged,
        &metadata_json(record, info.width, info.height, voice_track.as_deref()),
    );
    audio::run_ffmpeg(&ffmpeg, &args, false)
        .await
        .map_err(AppError::Other)?;
    save_clip(&ffmpeg, username, dir, tagged, info, &record.prompt_id).await
}

/// What the scene metadata of a gallery clip says.
struct ClipMeta {
    root: serde_json::Value,
    recipe: SceneRecipe,
    draft_id: Option<String>,
    request_id: Option<String>,
    created_unix: Option<i64>,
    voice_track: Option<String>,
    exact_voice: bool,
}

fn parse_clip_meta(root: serde_json::Value) -> Option<ClipMeta> {
    let scene = root.get("mooshie_scene")?;
    let recipe: SceneRecipe = serde_json::from_value(scene.get("recipe")?.clone()).ok()?;
    let text = |key: &str| {
        scene
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(ClipMeta {
        draft_id: text("draft_id"),
        request_id: text("request_id"),
        created_unix: scene
            .get("created_unix")
            .and_then(serde_json::Value::as_i64),
        voice_track: text("voice_track").filter(|id| valid_job_id(id)),
        exact_voice: text("audio").as_deref() == Some("exact_voice"),
        recipe,
        root,
    })
}

/// A gallery file name from the page: a bare `.mp4` name, never a path.
fn clip_path(username: Option<&str>, filename: &str) -> Result<PathBuf, AppError> {
    let name = filename.trim();
    if name.is_empty()
        || name.contains(['/', '\\'])
        || name.contains("..")
        || !name.to_ascii_lowercase().ends_with(".mp4")
    {
        return Err(AppError::Other(
            "Pick a scene clip from the gallery.".into(),
        ));
    }
    let gallery = crate::webserver::user_gallery_dir(username)
        .ok_or_else(|| AppError::Other("Cannot find the gallery folder.".into()))?;
    let path = gallery.join(name);
    if !path.is_file() {
        return Err(AppError::Other(
            "That clip is no longer in the gallery.".into(),
        ));
    }
    Ok(path)
}

async fn read_clip(
    username: Option<&str>,
    filename: &str,
) -> Result<(PathBuf, ClipMeta), AppError> {
    let path = clip_path(username, filename)?;
    let read = path.clone();
    let root = tokio::task::spawn_blocking(move || crate::metadata::read_video_comment_json(&read))
        .await
        .map_err(|e| AppError::Other(format!("Task failed: {e}")))?;
    let meta = root
        .and_then(parse_clip_meta)
        .ok_or_else(|| AppError::Other("That clip was not made on the Scenes page.".into()))?;
    Ok((path, meta))
}

/// What can still be done with a finished scene clip.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ClipInfo {
    pub model_label: &'static str,
    pub seconds: u32,
    pub resolution: String,
    pub draft: bool,
    /// When fal stops completing this draft.
    pub draft_expires_unix: Option<i64>,
    /// The price of completing the draft at 1080p, while it can be.
    pub upgrade: Option<VideoEstimate>,
    /// The user's own voice track was kept, so it can replace the clip's sound.
    pub exact_voice: bool,
}

/// A draft is offered for completion until ten minutes before fal's limit,
/// so a slow submit never lands on an expired draft.
const DRAFT_MARGIN_SECONDS: i64 = 10 * 60;

fn clip_info_from(meta: &ClipMeta, track_kept: bool, now_unix: i64) -> ClipInfo {
    let caps = meta.recipe.model.capabilities();
    let expires = meta
        .created_unix
        .filter(|_| meta.recipe.draft && meta.draft_id.is_some() && caps.draft)
        .map(|c| c + fal::DRAFT_LIFETIME_SECONDS);
    let upgrade = expires
        .filter(|e| now_unix < e - DRAFT_MARGIN_SECONDS)
        .and_then(|_| {
            meta.recipe
                .model
                .estimate("1080p", meta.recipe.seconds, false)
        });
    ClipInfo {
        model_label: caps.label,
        seconds: meta.recipe.seconds,
        resolution: meta.recipe.resolution.clone(),
        draft: meta.recipe.draft,
        draft_expires_unix: expires,
        upgrade,
        exact_voice: track_kept && !meta.exact_voice,
    }
}

fn kept_track(username: Option<&str>, meta: &ClipMeta) -> Option<PathBuf> {
    let id = meta.voice_track.as_deref()?;
    let path = tracks_dir(username).ok()?.join(format!("{id}.wav"));
    path.is_file().then_some(path)
}

pub async fn clip_info(username: Option<&str>, filename: &str) -> Result<ClipInfo, AppError> {
    let (_, meta) = read_clip(username, filename).await?;
    let kept = kept_track(username, &meta).is_some();
    Ok(clip_info_from(&meta, kept, chrono::Utc::now().timestamp()))
}

/// Complete a draft at 1080p. Paid; the page shows the itemized price first.
/// Runs as an ordinary scene job, so it resumes after a restart like one.
pub async fn start_upgrade(
    state: &Arc<AppState>,
    username: Option<&str>,
    filename: &str,
    sink: EventSink,
) -> Result<String, AppError> {
    let (_, meta) = read_clip(username, filename).await?;
    let kept = kept_track(username, &meta).is_some();
    let info = clip_info_from(&meta, kept, chrono::Utc::now().timestamp());
    let (Some(estimate), Some(draft_id)) = (info.upgrade, meta.draft_id.as_deref()) else {
        return Err(AppError::Other(
            "This clip is not a draft fal.ai can still complete. Drafts can be completed for seven days.".into(),
        ));
    };
    // A second click must not pay for the same completion twice.
    if job_records(username)
        .iter()
        .any(|r| r.recipe.upgraded_from.is_some() && r.recipe.upgraded_from == meta.request_id)
    {
        return Err(AppError::Other(
            "This draft is already being completed. It appears in the gallery when it is done."
                .into(),
        ));
    }
    let endpoint = match meta.recipe.model {
        VideoModel::FalSeedance25 => fal::SEEDANCE_25_DRAFT_COMPLETE,
    };
    let credential =
        resolve_credential(state, username, meta.recipe.model.capabilities().provider).await?;
    let mut recipe = meta.recipe.clone();
    recipe.draft = false;
    recipe.resolution = "1080p".into();
    recipe.estimate_usd = estimate.usd;
    recipe.upgraded_from = meta.request_id.clone();

    let prompt_id = format!("scene-{}", uuid::Uuid::new_v4());
    let dir = jobs_dir(username)?.join(&prompt_id);
    std::fs::create_dir_all(&dir)?;
    let prepared = PreparedScene {
        prompt_id,
        dir,
        endpoint,
        body: fal::seedance_25_complete_body(draft_id),
        recipe,
        credential,
        voice_track: meta.voice_track.filter(|_| kept),
    };
    claim(&prepared.prompt_id);
    Ok(spawn(
        state,
        username.map(str::to_string),
        JobStart::New(prepared),
        sink,
    ))
}

/// Replace a clip's sound with the voice track it was made from, as a new
/// gallery clip. Free and local. Returns the `comfyui:output_video` payload.
pub async fn exact_voice(
    state: &Arc<AppState>,
    username: Option<&str>,
    filename: &str,
) -> Result<serde_json::Value, AppError> {
    let (source, meta) = read_clip(username, filename).await?;
    if meta.exact_voice {
        return Err(AppError::Other(
            "This clip already uses your exact voice.".into(),
        ));
    }
    let track = kept_track(username, &meta).ok_or_else(|| {
        AppError::Other(
            "The voice track for this scene was not kept, so its sound cannot be replaced.".into(),
        )
    })?;
    let ffmpeg = ffmpeg_path(state)?;
    let work = takes::scene_assets_dir(username)
        .ok_or_else(|| AppError::Other("Cannot locate the scene asset folder.".into()))?
        .join("work")
        .join(uuid::Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&work)?;
    let result = async {
        let mut root = meta.root.clone();
        root["mooshie_scene"]["audio"] = serde_json::json!("exact_voice");
        let out = work.join("scene.mp4");
        let args = exact_voice_args(&source, &track, &out, &root.to_string());
        audio::run_ffmpeg(&ffmpeg, &args, false)
            .await
            .map_err(AppError::Other)?;
        let info = probe_video(&ffmpeg, &out).await?;
        let prompt_id = format!("scene-{}", uuid::Uuid::new_v4());
        save_clip(&ffmpeg, username, &work, out, info, &prompt_id).await
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> SceneRequest {
        SceneRequest {
            model: VideoModel::FalSeedance25,
            resolution: "720p".into(),
            draft: true,
            aspect: "16:9".into(),
            seed: None,
            continuous: false,
            character_name: "Aoi".into(),
            character_traits: String::new(),
            language: "Japanese".into(),
            starting_state: String::new(),
            ending_state: String::new(),
            ambience: String::new(),
            minor: false,
            voice_id: "v1".into(),
            lines: vec![LineInput {
                line_id: "l1".into(),
                take_id: "a".repeat(64),
                text: "hello".into(),
                delivery: String::new(),
            }],
            shots: vec![],
            tail: 1.0,
            images: vec![SceneImage {
                filename: "a.jxl".into(),
                role: ImageRole::Character,
            }],
            prompt_override: None,
            mouth_map: false,
        }
    }

    #[test]
    fn an_edited_prompt_keeps_a_minors_constraints() {
        let built = "built".to_string();
        assert_eq!(final_prompt(built.clone(), None, true).unwrap(), "built");
        assert_eq!(
            final_prompt(built.clone(), Some("  "), false).unwrap(),
            "built"
        );
        assert_eq!(
            final_prompt(built.clone(), Some("mine"), false).unwrap(),
            "mine"
        );
        let edited = final_prompt(built.clone(), Some("mine"), true).unwrap();
        assert!(edited.starts_with("mine") && edited.contains(prompt::MINOR_CONSTRAINTS));
        assert!(final_prompt(built, Some(&"x".repeat(MAX_PROMPT_CHARS + 1)), false).is_err());
    }

    #[test]
    fn requests_are_checked_against_capabilities() {
        let caps = VideoModel::FalSeedance25.capabilities();
        assert!(validate_request(&request(), &caps).is_ok());
        let mut r = request();
        r.resolution = "4k".into();
        assert!(validate_request(&r, &caps).is_err());
        let mut r = request();
        r.aspect = "2:1".into();
        assert!(validate_request(&r, &caps).is_err());
        let mut r = request();
        r.images[0].role = ImageRole::Location;
        assert!(validate_request(&r, &caps).is_err());
        let mut r = request();
        r.images.clear();
        assert!(validate_request(&r, &caps).is_err());
        let mut r = request();
        r.lines[0].take_id = "../x".into();
        assert!(validate_request(&r, &caps).is_err());
    }

    #[test]
    fn job_ids_are_scene_uuids() {
        assert!(valid_job_id(&format!("scene-{}", uuid::Uuid::new_v4())));
        assert!(!valid_job_id("scene-../../x"));
        assert!(!valid_job_id("nai-1234"));
    }

    #[test]
    fn progress_never_claims_completion() {
        assert_eq!(time_progress(Duration::ZERO, 28), 5);
        assert!(time_progress(Duration::from_secs(10_000), 28) < 100);
    }

    #[test]
    fn reference_images_lose_metadata_and_shrink() {
        let img = image::RgbaImage::from_pixel(3000, 1000, image::Rgba([10, 20, 30, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let jpeg = reference_jpeg(&png).unwrap();
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!(back.width(), MAX_REFERENCE_SIDE);
        assert!(back.height() < 1000);
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn metadata_carries_the_recipe_but_no_secrets() {
        let record = JobRecord {
            version: JOB_VERSION,
            prompt_id: "scene-x".into(),
            handle: QueueHandle {
                request_id: "req".into(),
                status_url: "https://queue.fal.run/a/requests/req/status".into(),
                response_url: "https://queue.fal.run/a/requests/req".into(),
                cancel_url: "https://queue.fal.run/a/requests/req/cancel".into(),
            },
            stage: JobStage::Downloaded,
            created_unix: 0,
            provider_seed: Some(42),
            draft_id: Some("d1".into()),
            recipe: SceneRecipe {
                builder_version: 1,
                model: VideoModel::FalSeedance25,
                resolution: "720p".into(),
                draft: true,
                seconds: 28,
                aspect: "16:9".into(),
                seed: None,
                prompt: "Format: ...".into(),
                voice_id: "v1".into(),
                character_name: "Aoi".into(),
                minor: false,
                lines: vec![],
                images: vec![],
                estimate_usd: 6.17,
                upgraded_from: None,
            },
            voice_track: None,
        };
        let track = format!("scene-{}", uuid::Uuid::nil());
        let json: serde_json::Value =
            serde_json::from_str(&metadata_json(&record, 864, 496, Some(&track))).unwrap();
        assert_eq!(json["sui_image_params"]["seed"], "42");
        assert_eq!(json["mooshie_scene"]["draft_id"], "d1");
        let meta = parse_clip_meta(json.clone()).expect("scene metadata reads back");
        assert_eq!(meta.recipe, record.recipe);
        assert_eq!(meta.voice_track.as_deref(), Some(track.as_str()));
        assert_eq!(meta.request_id.as_deref(), Some("req"));
        assert!(!meta.exact_voice);
        let parsed = crate::metadata::parse_swarmui_json(&json.to_string()).unwrap();
        assert_eq!(parsed.get("size").map(String::as_str), Some("864x496"));
        let text = json.to_string();
        assert!(!text.contains("status_url") && !text.contains("queue.fal.run"));
    }

    fn clip(draft: bool, created: Option<i64>) -> ClipMeta {
        let mut recipe: SceneRecipe = serde_json::from_value(serde_json::json!({
            "builder_version": 2, "model": "fal_seedance25", "resolution": "720p",
            "draft": draft, "seconds": 10, "aspect": "16:9", "seed": null,
            "prompt": "p", "voice_id": "v", "character_name": "Aoi", "minor": false,
            "lines": [], "images": [], "estimate_usd": 2.2
        }))
        .unwrap();
        recipe.draft = draft;
        ClipMeta {
            root: serde_json::Value::Null,
            recipe,
            draft_id: Some("d1".into()),
            request_id: Some("req".into()),
            created_unix: created,
            voice_track: None,
            exact_voice: false,
        }
    }

    #[test]
    fn a_draft_can_be_completed_for_seven_days_only() {
        let day = 24 * 60 * 60;
        let fresh = clip_info_from(&clip(true, Some(0)), false, day);
        let upgrade = fresh.upgrade.expect("a fresh draft can be completed");
        assert_eq!(upgrade.resolution, "1080p");
        assert!((upgrade.usd - 11.64).abs() < 0.01);
        assert_eq!(fresh.draft_expires_unix, Some(7 * day));
        assert!(clip_info_from(&clip(true, Some(0)), false, 7 * day - 60)
            .upgrade
            .is_none());
        assert!(clip_info_from(&clip(false, Some(0)), false, day)
            .upgrade
            .is_none());
        assert!(clip_info_from(&clip(true, None), false, day)
            .upgrade
            .is_none());
    }

    #[test]
    fn exact_voice_needs_a_kept_track_and_is_offered_once() {
        assert!(clip_info_from(&clip(true, Some(0)), true, 0).exact_voice);
        assert!(!clip_info_from(&clip(true, Some(0)), false, 0).exact_voice);
        let mut done = clip(true, Some(0));
        done.exact_voice = true;
        assert!(!clip_info_from(&done, true, 0).exact_voice);
    }

    #[test]
    fn clip_names_are_never_paths() {
        for name in ["", "../a.mp4", "a/b.mp4", "a\\b.mp4", "a.png"] {
            assert!(clip_path(None, name).is_err(), "{name}");
        }
    }

    #[test]
    fn exact_voice_copies_the_picture() {
        let args = exact_voice_args(
            Path::new("v.mp4"),
            Path::new("t.wav"),
            Path::new("o.mp4"),
            "{}",
        );
        let joined = args.join(" ");
        assert!(joined.contains("-map 0:v:0 -map 1:a:0 -c:v copy"));
        assert!(joined.ends_with("o.mp4"));
    }

    /// Runs the real FFmpeg steps: `cargo test -- --ignored ffmpeg` with
    /// `ffmpeg` on PATH (or FFMPEG set to its path).
    #[tokio::test]
    #[ignore = "needs ffmpeg"]
    async fn ffmpeg_steps_build_a_track_and_a_tagged_clip() {
        let ffmpeg = PathBuf::from(std::env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".into()));
        let dir =
            std::env::temp_dir().join(format!("mooshie-scene-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: Vec<String>| {
            let ffmpeg = ffmpeg.clone();
            async move { audio::run_ffmpeg(&ffmpeg, &args, false).await.unwrap() }
        };
        let s = |v: &str| v.to_string();

        // Two stereo 48 kHz "takes" of 2.0 s and 1.5 s.
        for (name, secs) in [("a.wav", "2.0"), ("b.wav", "1.5")] {
            run(vec![
                s("-hide_banner"),
                s("-y"),
                s("-f"),
                s("lavfi"),
                s("-i"),
                format!("sine=frequency=440:sample_rate=48000:duration={secs}"),
                s("-ac"),
                s("2"),
                dir.join(name).to_string_lossy().into_owned(),
            ])
            .await;
        }
        let takes = vec![(dir.join("a.wav"), 1.5), (dir.join("b.wav"), 0.5)];
        let track = dir.join("voice.wav");
        run(audio::track_args(&takes, 1.0, &track)).await;
        let total = audio::probe_duration(&ffmpeg, &track).await.unwrap();
        assert!((total - 6.5).abs() < 0.05, "{total}");

        // A short clip standing in for the provider's mp4.
        let raw = dir.join("raw.mp4");
        run(vec![
            s("-hide_banner"),
            s("-y"),
            s("-f"),
            s("lavfi"),
            s("-i"),
            s("testsrc=size=320x240:rate=24:duration=1"),
            s("-c:v"),
            s("mpeg4"),
            raw.to_string_lossy().into_owned(),
        ])
        .await;
        let tagged = dir.join("scene.mp4");
        let comment = r#"{"sui_image_params":{"prompt":"Format: test","width":320,"height":240}}"#;
        run(tag_args(&raw, &tagged, comment)).await;
        let meta = crate::metadata::read_file_metadata(&tagged).expect("metadata");
        assert_eq!(
            meta.get("positive_prompt").map(String::as_str),
            Some("Format: test")
        );

        let banner = audio::run_ffmpeg(
            &ffmpeg,
            &[
                s("-hide_banner"),
                s("-i"),
                tagged.to_string_lossy().into_owned(),
            ],
            true,
        )
        .await
        .unwrap();
        let info = audio::parse_video_info(&banner).unwrap();
        assert_eq!((info.width, info.height, info.fps), (320, 240, 24.0));

        let frame = dir.join("poster.png");
        run(poster_args(&tagged, &frame)).await;
        let poster = dir.join("poster.webp");
        image::open(&frame)
            .unwrap()
            .save_with_format(&poster, image::ImageFormat::WebP)
            .unwrap();
        assert!(poster.metadata().unwrap().len() > 0);

        // The exact-voice mux keeps the picture, takes the track's sound and
        // carries the scene metadata the page reads back.
        let comment = serde_json::json!({
            "sui_image_params": {"prompt": "Format: test"},
            "mooshie_scene": {"audio": "exact_voice", "recipe": {}},
        })
        .to_string();
        let muxed = dir.join("exact.mp4");
        run(exact_voice_args(&tagged, &track, &muxed, &comment)).await;
        let root = crate::metadata::read_video_comment_json(&muxed).expect("comment");
        assert_eq!(root["mooshie_scene"]["audio"], "exact_voice");
        let banner = audio::run_ffmpeg(
            &ffmpeg,
            &[
                s("-hide_banner"),
                s("-i"),
                muxed.to_string_lossy().into_owned(),
            ],
            true,
        )
        .await
        .unwrap();
        assert!(banner.contains("Audio: aac"), "{banner}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
