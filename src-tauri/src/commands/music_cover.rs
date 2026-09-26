//! SheetSage2 runs in a host-configured environment, never in ComfyUI's Python.
//! Clients upload bytes and poll private jobs; they cannot select executables or paths.
use crate::{error::AppError, state::AppState, templates::music::validate_cover_score};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncReadExt,
    sync::{oneshot, Mutex},
};

const AUDIO_LIMIT: usize = 64 * 1024 * 1024;
const SCORE_LIMIT: u64 = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30 * 60);

struct Job {
    owner: Option<String>,
    result: Value,
    cancel: Option<oneshot::Sender<()>>,
    running: bool,
    updated: Instant,
}

#[derive(Default)]
pub struct CoverJobs(Mutex<HashMap<String, Job>>);

fn runtime() -> Result<(PathBuf, PathBuf, String), AppError> {
    let python = std::env::var_os("MOOSHIE_SHEETSAGE2_PYTHON").map(PathBuf::from);
    let model = std::env::var_os("MOOSHIE_SHEETSAGE2_MODEL_DIR").map(PathBuf::from);
    match (python, model) {
        (Some(python), Some(model)) if python.is_absolute() && python.is_file()
            && model.is_absolute() && model.join("config.json").is_file() => {
            // CPU is isolated from ComfyUI's GPU queue. Hosts may opt into a dedicated GPU.
            let device = std::env::var("MOOSHIE_SHEETSAGE2_DEVICE").unwrap_or_else(|_| "cpu".into());
            if device != "cpu" && device != "cuda" && !(device.starts_with("cuda:") && device[5..].parse::<u32>().is_ok()) {
                return Err(AppError::Other("MOOSHIE_SHEETSAGE2_DEVICE must be cpu, cuda, or cuda:N.".into()));
            }
            Ok((python, model, device))
        }
        _ => Err(AppError::Other("Set MOOSHIE_SHEETSAGE2_PYTHON to the separate environment's Python executable and MOOSHIE_SHEETSAGE2_MODEL_DIR to the downloaded SheetSage2 folder on the MooshieUI host, then restart MooshieUI. You can also import a score.abc transcribed externally.".into())),
    }
}

pub(crate) async fn capabilities(state: &AppState, owner: Option<&str>) -> Value {
    let mut caps = match runtime() {
        Ok((_, _, device)) => json!({"configured": true, "device": device}),
        Err(error) => json!({"configured": false, "error": error.to_string()}),
    };
    let jobs = state.cover_jobs.0.lock().await;
    if let Some((id, _)) = jobs
        .iter()
        .filter(|(_, job)| {
            job.owner.as_deref() == owner && job.updated.elapsed() < Duration::from_secs(3600)
        })
        .max_by_key(|(_, job)| job.updated)
    {
        caps["latest_job"] = json!(id);
    }
    caps
}

pub(crate) fn decode_audio(encoded: &str, filename: &str) -> Result<(Vec<u8>, String), AppError> {
    let extension = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "wav" | "mp3" | "flac" | "m4a" | "ogg" | "opus" | "aiff" | "aif"
    ) {
        return Err(AppError::Other(
            "Choose a WAV, MP3, FLAC, M4A, OGG, Opus or AIFF recording.".into(),
        ));
    }
    if encoded.len() > AUDIO_LIMIT.div_ceil(3) * 4 {
        return Err(AppError::Other("Cover source audio exceeds 64 MiB.".into()));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| AppError::Other("Invalid encoded source recording.".into()))?;
    if bytes.is_empty() || bytes.len() > AUDIO_LIMIT {
        return Err(AppError::Other(
            "Choose a non-empty recording up to 64 MiB.".into(),
        ));
    }
    Ok((bytes, extension))
}

pub(crate) async fn start(
    state: Arc<AppState>,
    audio_base64: &str,
    filename: &str,
    owner: Option<String>,
) -> Result<String, AppError> {
    let (python, model, device) = runtime()?;
    let (bytes, extension) = decode_audio(audio_base64, filename)?;
    let mut jobs = state.cover_jobs.0.lock().await;
    jobs.retain(|_, job| job.running || job.updated.elapsed() < Duration::from_secs(3600));
    if jobs.values().any(|job| job.running) {
        return Err(AppError::Other(
            "SheetSage2 is already transcribing a recording. Wait for it to finish.".into(),
        ));
    }
    // Bounded retention even when many accounts use the host.
    if jobs.len() >= 16 {
        if let Some(oldest) = jobs
            .iter()
            .min_by_key(|(_, job)| job.updated)
            .map(|(id, _)| id.clone())
        {
            jobs.remove(&oldest);
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (cancel, receiver) = oneshot::channel();
    jobs.insert(
        id.clone(),
        Job {
            owner,
            result: json!({"status": "running"}),
            cancel: Some(cancel),
            running: true,
            updated: Instant::now(),
        },
    );
    drop(jobs);
    let job_id = id.clone();
    tokio::spawn(async move {
        let result = transcribe(python, model, device, bytes, extension, receiver).await;
        let mut jobs = state.cover_jobs.0.lock().await;
        if let Some(job) = jobs.get_mut(&job_id) {
            job.result = result
                .unwrap_or_else(|error| json!({"status": "error", "error": error.to_string()}));
            job.cancel = None;
            job.running = false;
            job.updated = Instant::now();
        }
    });
    Ok(id)
}

pub(crate) async fn status(
    state: &AppState,
    id: &str,
    owner: Option<&str>,
    cancel: bool,
) -> Result<Value, AppError> {
    let mut jobs = state.cover_jobs.0.lock().await;
    let job = jobs
        .get_mut(id)
        .filter(|job| {
            job.owner.as_deref() == owner
                && (job.running || job.updated.elapsed() < Duration::from_secs(3600))
        })
        .ok_or_else(|| AppError::Other("Cover transcription job is unavailable.".into()))?;
    if cancel {
        if let Some(sender) = job.cancel.take() {
            let _ = sender.send(());
        }
        // running stays true until process cleanup, so another job cannot overlap it.
    }
    Ok(job.result.clone())
}

struct WorkDir(PathBuf);
impl WorkDir {
    /// A newly generated directory in the shared temp dir that only this user
    /// can enter (0700 on Unix, as `music_link::WorkDir` does): the uploaded
    /// recording is private to whoever sent it.
    async fn create() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("mooshie-cover-{}", uuid::Uuid::new_v4()));
        let mut builder = tokio::fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        builder.create(&path).await?;
        Ok(Self(path))
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Write `bytes` to a new file readable only by this user (0600 on Unix).
/// `create_new` refuses to reuse, or follow, anything already at `path`.
async fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).await?;
    file.write_all(bytes).await?;
    file.flush().await
}

async fn transcribe(
    python: PathBuf,
    model: PathBuf,
    device: String,
    bytes: Vec<u8>,
    extension: String,
    mut cancel: oneshot::Receiver<()>,
) -> Result<Value, AppError> {
    // Only a newly generated private directory is ever written or removed.
    let dir = WorkDir::create().await?;
    let source = dir.0.join(format!("source.{extension}"));
    let output = dir.0.join("result.json");
    write_private(&source, &bytes).await?;
    drop(bytes);
    let mut command = tokio::process::Command::new(python);
    command
        .current_dir(&model)
        .arg("-u")
        .arg("-c")
        .arg(include_str!("music_cover.py"))
        .arg(model)
        .arg(&source)
        .arg(&output)
        .arg(device)
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|e| AppError::Other(format!("Unable to start the SheetSage2 environment: {e}")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Other("Missing transcription error stream.".into()))?;
    let reader = tokio::spawn(read_tail(stderr));
    let outcome = tokio::select! {
        status = child.wait() => Some(status),
        _ = &mut cancel => None,
        _ = tokio::time::sleep(TIMEOUT) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            reader.abort();
            return Err(AppError::Other("SheetSage2 exceeded the 30-minute transcription limit. Try a shorter recording or import an externally transcribed ABC score.".into()));
        }
    };
    let Some(outcome) = outcome else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        reader.abort();
        return Ok(json!({"status": "cancelled"}));
    };
    let errors = reader.await.unwrap_or_default();
    if !outcome?.success() {
        return Err(AppError::Other(format!("SheetSage2 transcription failed. Check its separate environment, model access and FFmpeg shared libraries.\n{errors}")));
    }
    if tokio::fs::metadata(&output).await?.len() > SCORE_LIMIT {
        return Err(AppError::Other(
            "SheetSage2 returned an oversized score.".into(),
        ));
    }
    let result: Value = serde_json::from_slice(&tokio::fs::read(output).await?)?;
    validate_cover_score(result["abc"].as_str().unwrap_or("")).map_err(AppError::Other)?;
    Ok(json!({"status": "completed", "abc": result["abc"], "warnings": result["warnings"]}))
}

async fn read_tail(mut stream: impl tokio::io::AsyncRead + Unpin) -> String {
    let mut tail = Vec::new();
    let mut buffer = [0; 4096];
    while let Ok(count) = stream.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        tail.extend_from_slice(&buffer[..count]);
        if tail.len() > 16_384 {
            tail.drain(..tail.len() - 16_384);
        }
    }
    String::from_utf8_lossy(&tail).into_owned()
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_cover_capabilities(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Value, AppError> {
    Ok(capabilities(&state, None).await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn transcribe_music_cover(
    state: tauri::State<'_, Arc<AppState>>,
    audio_base64: String,
    filename: String,
) -> Result<String, AppError> {
    start(state.inner().clone(), &audio_base64, &filename, None).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_cover_transcription(
    state: tauri::State<'_, Arc<AppState>>,
    job_id: String,
    cancel: bool,
) -> Result<Value, AppError> {
    status(&state, &job_id, None, cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_uses_only_the_extension_and_rejects_empty_or_invalid_data() {
        assert_eq!(
            decode_audio(&STANDARD.encode(b"RIFFaudio"), "../../private/track.WAV")
                .unwrap()
                .1,
            "wav"
        );
        assert!(decode_audio("", "song.wav").is_err());
        assert!(decode_audio("not base64", "song.wav").is_err());
        assert!(decode_audio(&STANDARD.encode(b"data"), "script.py").is_err());
    }
    #[tokio::test]
    async fn jobs_are_private_and_cancel_waits_for_cleanup() {
        let state = AppState::new(crate::config::AppConfig::default());
        let (sender, receiver) = oneshot::channel();
        state.cover_jobs.0.lock().await.insert(
            "job".into(),
            Job {
                owner: Some("alice".into()),
                result: json!({"status": "running"}),
                cancel: Some(sender),
                running: true,
                updated: Instant::now(),
            },
        );
        assert!(status(&state, "job", Some("bob"), true).await.is_err());
        assert!(status(&state, "job", None, true).await.is_err());
        assert_eq!(
            status(&state, "job", Some("alice"), true).await.unwrap()["status"],
            "running"
        );
        assert!(receiver.await.is_ok());
        assert!(state.cover_jobs.0.lock().await["job"].running);
        assert_eq!(
            capabilities(&state, Some("alice")).await["latest_job"],
            "job"
        );
        assert!(capabilities(&state, Some("bob"))
            .await
            .get("latest_job")
            .is_none());
    }

    #[tokio::test]
    async fn uploaded_audio_lands_in_a_private_dir_and_file() {
        let dir = WorkDir::create().await.unwrap();
        let source = dir.0.join("source.wav");
        write_private(&source, b"RIFFaudio").await.unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), b"RIFFaudio");
        // Never reuses or follows something already at the path.
        assert!(write_private(&source, b"again").await.is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode();
            assert_eq!(mode(&dir.0) & 0o777, 0o700);
            assert_eq!(mode(&source) & 0o777, 0o600);
        }
        let path = dir.0.clone();
        drop(dir);
        assert!(!path.exists());
    }
}
