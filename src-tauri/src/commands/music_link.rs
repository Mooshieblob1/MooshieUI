//! Temporary song-link imports. No credentials, cookies, user-selected paths or shells.
//! Working files are deleted before returning audio; the client keeps only a session Blob.
use crate::{error::AppError, state::AppState};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, sync::Mutex};

const AUDIO_LIMIT: u64 = 64 * 1024 * 1024;
const LEASE: Duration = Duration::from_secs(30);
const TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Service {
    Youtube,
    Dailymotion,
    Spotify,
    Deezer,
    Tidal,
}

struct SongLink {
    service: Service,
    url: String,
    id: String,
}

fn error(message: &str) -> AppError {
    AppError::Other(message.into())
}

fn parse_link(raw: &str) -> Result<SongLink, AppError> {
    let invalid = || {
        error("Paste a single YouTube, YouTube Music, Spotify, Deezer, Dailymotion or Tidal song link (not a playlist or album).")
    };
    if raw.len() > 2048 {
        return Err(invalid());
    }
    let url = url::Url::parse(raw.trim()).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(invalid());
    }
    let parts: Vec<_> = url
        .path_segments()
        .ok_or_else(invalid)?
        .filter(|s| !s.is_empty())
        .collect();
    let host = url.host_str().unwrap_or("");
    let (service, id) = match host {
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            let id = if parts.as_slice() == ["watch"] {
                url.query_pairs()
                    .find(|(key, _)| key == "v")
                    .map(|(_, value)| value.into_owned())
            } else if parts.len() == 2 && matches!(parts[0], "shorts" | "embed") {
                Some(parts[1].into())
            } else {
                None
            };
            (Service::Youtube, id.ok_or_else(invalid)?)
        }
        "youtu.be" | "www.youtu.be" if parts.len() == 1 => (Service::Youtube, parts[0].into()),
        "dailymotion.com" | "www.dailymotion.com" if parts.len() == 2 && parts[0] == "video" => (
            Service::Dailymotion,
            parts[1].split('_').next().unwrap_or("").into(),
        ),
        "dai.ly" if parts.len() == 1 => (Service::Dailymotion, parts[0].into()),
        "open.spotify.com" => {
            let parts = if parts.first().is_some_and(|s| s.starts_with("intl-")) {
                &parts[1..]
            } else {
                &parts[..]
            };
            if parts.len() != 2 || parts[0] != "track" {
                return Err(invalid());
            }
            (Service::Spotify, parts[1].into())
        }
        "deezer.com" | "www.deezer.com" => {
            let parts = if parts.len() == 3 && parts[0].len() == 2 {
                &parts[1..]
            } else {
                &parts[..]
            };
            if parts.len() != 2 || parts[0] != "track" {
                return Err(invalid());
            }
            (Service::Deezer, parts[1].into())
        }
        "tidal.com" | "www.tidal.com" | "listen.tidal.com" => {
            let parts = if parts.first() == Some(&"browse") {
                &parts[1..]
            } else {
                &parts[..]
            };
            if parts.len() != 2 || parts[0] != "track" {
                return Err(invalid());
            }
            (Service::Tidal, parts[1].into())
        }
        _ => return Err(invalid()),
    };
    let valid = match service {
        Service::Youtube => {
            id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
        Service::Spotify => id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()),
        Service::Dailymotion => {
            !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric())
        }
        _ => !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()),
    };
    if !valid {
        return Err(invalid());
    }
    let url = match service {
        Service::Youtube => format!("https://www.youtube.com/watch?v={id}"),
        Service::Dailymotion => format!("https://www.dailymotion.com/video/{id}"),
        Service::Spotify => format!("https://open.spotify.com/track/{id}"),
        Service::Deezer => format!("https://www.deezer.com/track/{id}"),
        Service::Tidal => format!("https://tidal.com/browse/track/{id}"),
    };
    Ok(SongLink { service, url, id })
}

fn share_url(raw: &str) -> Option<url::Url> {
    let url = url::Url::parse(raw.trim()).ok()?;
    if raw.len() > 2048
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    matches!(
        url.host_str()?,
        "spotify.link" | "link.deezer.com" | "deezer.page.link"
    )
    .then_some(url)
}

async fn resolve_link(state: &AppState, raw: &str) -> Result<SongLink, AppError> {
    if let Ok(link) = parse_link(raw) {
        return Ok(link);
    }
    let mut url = share_url(raw)
        .ok_or_else(|| error("Paste a single supported song link, not an album or playlist."))?;
    for _ in 0..5 {
        let response = state
            .http_client_no_redirect
            .get(url.clone())
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        if !response.status().is_redirection() {
            break;
        }
        let target = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|value| url.join(value).ok())
            .ok_or_else(|| error("Invalid song share link."))?;
        if let Ok(link) = parse_link(target.as_str()) {
            return Ok(link);
        }
        url = share_url(target.as_str()).ok_or_else(|| error("This share link does not lead to a supported song. Copy its full track URL instead."))?;
    }
    Err(error(
        "Could not resolve this share link. Copy the full track URL from the service instead.",
    ))
}

pub(super) struct Control {
    cancelled: AtomicBool,
    touched: std::sync::Mutex<Instant>,
    started: Instant,
}
impl Control {
    pub(super) fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            touched: std::sync::Mutex::new(Instant::now()),
            started: Instant::now(),
        }
    }
    pub(super) fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
            || self.started.elapsed() > TIMEOUT
            || self.touched.lock().map_or(true, |t| t.elapsed() > LEASE)
    }
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub(super) fn touch(&self) {
        if let Ok(mut touched) = self.touched.lock() {
            *touched = Instant::now();
        }
    }
}

pub(super) async fn stopped(control: &Control) {
    while !control.stopped() {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
struct Job {
    owner: Option<String>,
    control: Arc<Control>,
    result: Option<Value>,
}
#[derive(Default)]
pub struct LinkImports {
    jobs: Mutex<HashMap<String, Job>>,
    closing: AtomicBool,
}

pub(crate) async fn start(
    state: Arc<AppState>,
    raw: &str,
    query: &str,
    owner: Option<String>,
) -> Result<String, AppError> {
    // Share redirects are resolved inside the bounded job, not before registration.
    if parse_link(raw).is_err() && share_url(raw).is_none() {
        return Err(error(
            "Paste a single supported song link, not an album or playlist.",
        ));
    }
    let raw = raw.trim().to_owned();
    let query = query.trim().to_owned();
    if query.len() > 300 || query.chars().any(char::is_control) {
        return Err(error("Song and artist must be at most 300 characters."));
    }
    if state.media_tools.status()["available"] != true {
        return Err(error("Song-link tools are being prepared. Wait for setup to finish, or retry setup if it failed."));
    }
    let downloader = state
        .media_tools
        .path("yt-dlp")
        .ok_or_else(|| error("yt-dlp is missing. Reopen the app to repair song-link tools."))?;
    let converter = state
        .media_tools
        .path("ffmpeg")
        .ok_or_else(|| error("FFmpeg is missing. Reopen the app to repair song-link tools."))?;
    let mut jobs = state.link_imports.jobs.lock().await;
    jobs.retain(|_, job| job.result.is_none() || !job.control.stopped());
    if state.link_imports.closing.load(Ordering::Relaxed) {
        return Err(error("The app is closing."));
    }
    if jobs.len() >= 4
        || jobs
            .values()
            .any(|j| j.owner == owner && j.result.is_none())
    {
        return Err(error(
            "A song import is still finishing. Wait a moment and try again.",
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let control = Arc::new(Control::new());
    jobs.insert(
        id.clone(),
        Job {
            owner,
            control: control.clone(),
            result: None,
        },
    );
    drop(jobs);
    let job_id = id.clone();
    tokio::spawn(async move {
        let result = async {
            let link = tokio::select! {
                result = resolve_link(&state, &raw) => result?,
                _ = stopped(&control) => return Err(error("Song import cancelled or expired.")),
            };
            import(&state, link, &query, downloader, converter, &control).await
        }
        .await;
        {
            let mut jobs = state.link_imports.jobs.lock().await;
            if control.stopped() {
                jobs.remove(&job_id);
            } else if let Some(job) = jobs.get_mut(&job_id) {
                job.result = Some(
                    result.unwrap_or_else(|e| json!({"status": "error", "error": e.to_string()})),
                );
            }
        }
        // Unclaimed results must not keep private audio resident on a shared host.
        tokio::time::sleep(LEASE).await;
        state.link_imports.jobs.lock().await.remove(&job_id);
    });
    Ok(id)
}

pub(crate) async fn status(
    state: &AppState,
    id: &str,
    owner: Option<&str>,
    cancel: bool,
) -> Result<Value, AppError> {
    let mut jobs = state.link_imports.jobs.lock().await;
    let job = jobs
        .get_mut(id)
        .filter(|j| j.owner.as_deref() == owner)
        .ok_or_else(|| error("Song import expired or is unavailable."))?;
    if cancel {
        job.control.cancelled.store(true, Ordering::Relaxed);
        if job.result.is_some() {
            jobs.remove(id);
        }
        return Ok(json!({"status": "cancelled"}));
    }
    if let Ok(mut touched) = job.control.touched.lock() {
        *touched = Instant::now();
    }
    if job.result.is_some() {
        return Ok(jobs
            .remove(id)
            .and_then(|j| j.result)
            .unwrap_or(Value::Null));
    }
    Ok(json!({"status": "running"}))
}

pub async fn shutdown(state: &AppState) {
    state.link_imports.closing.store(true, Ordering::Relaxed);
    for job in state.link_imports.jobs.lock().await.values() {
        job.control.cancelled.store(true, Ordering::Relaxed);
    }
    // Commands explicitly kill and reap their child before deleting the directory.
    while state
        .link_imports
        .jobs
        .lock()
        .await
        .values()
        .any(|j| j.result.is_none())
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    state.link_imports.jobs.lock().await.clear();
}

async fn get_metadata(state: &AppState, url: &str) -> Result<String, AppError> {
    // Fixed service endpoints only. Never follow an upstream redirect to another host.
    let mut response = state
        .http_client_no_redirect
        .get(url)
        .header("User-Agent", "Mozilla/5.0 MooshieUI")
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(error("Could not read this song's title and artist. Enter them in the matching field or paste a YouTube link."));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err(error("Song metadata was too large."));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn spotify_query(html: &str) -> Option<String> {
    let re = regex::Regex::new(r#"(?s)<script[^>]*id="__NEXT_DATA__"[^>]*>(.*?)</script>"#).ok()?;
    let data: Value = serde_json::from_str(re.captures(html)?.get(1)?.as_str()).ok()?;
    let entity = &data["props"]["pageProps"]["state"]["data"]["entity"];
    let title = entity["name"].as_str()?;
    let artist = entity["artists"][0]["name"].as_str()?;
    Some(format!("{artist} {title}"))
}

fn tidal_query(html: &str) -> Option<String> {
    // TIDAL's public share page describes a track as "Title by Artist on TIDAL".
    let tags = regex::Regex::new(r#"(?is)<meta\s+[^>]*>"#).ok()?;
    let property = regex::Regex::new(r#"(?i)(?:property|name)\s*=\s*["']og:title["']"#).ok()?;
    let content = regex::Regex::new(r#"(?i)content\s*=\s*(?:"([^"]*)"|'([^']*)')"#).ok()?;
    for tag in tags.find_iter(html) {
        if !property.is_match(tag.as_str()) {
            continue;
        }
        let capture = content.captures(tag.as_str())?;
        let title = capture.get(1).or_else(|| capture.get(2))?.as_str();
        let title = title.strip_suffix(" on TIDAL").unwrap_or(title);
        if !title.contains(" by ") && !title.contains(" - ") {
            return None;
        }
        return Some(
            title
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .replace("&amp;", "&"),
        );
    }
    None
}

async fn matching_query(state: &AppState, link: &SongLink) -> Result<String, AppError> {
    let query = match link.service {
        Service::Spotify => spotify_query(
            &get_metadata(
                state,
                &format!("https://open.spotify.com/embed/track/{}", link.id),
            )
            .await?,
        ),
        Service::Deezer => {
            let value: Value = serde_json::from_str(
                &get_metadata(state, &format!("https://api.deezer.com/track/{}", link.id)).await?,
            )?;
            value["title"]
                .as_str()
                .zip(value["artist"]["name"].as_str())
                .map(|(title, artist)| format!("{artist} {title}"))
        }
        Service::Tidal => tidal_query(&get_metadata(state, &link.url).await?),
        _ => None,
    };
    query.filter(|q| !q.trim().is_empty() && q.len() <= 300 && !q.chars().any(char::is_control)).ok_or_else(|| error("Could not read this song's title and artist. Enter them in the matching field or paste a YouTube link."))
}

pub(super) struct WorkDir(pub(super) PathBuf);
impl WorkDir {
    pub(super) fn new() -> Result<Self, AppError> {
        let path = std::env::temp_dir().join(format!("mooshie-song-{}", uuid::Uuid::new_v4()));
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder.create(&path)?;
        Ok(Self(path))
    }
    fn size(&self) -> std::io::Result<u64> {
        std::fs::read_dir(&self.0)?.try_fold(0u64, |size, entry| {
            Ok(size.saturating_add(entry?.metadata()?.len()))
        })
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn read_bounded(mut stream: impl tokio::io::AsyncRead + Unpin, limit: usize) -> Vec<u8> {
    let mut result = Vec::new();
    let mut buffer = [0; 8192];
    while let Ok(count) = stream.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        let retain = count.min(limit.saturating_sub(result.len()));
        result.extend_from_slice(&buffer[..retain]);
    }
    result
}

pub(super) async fn run(
    executable: &Path,
    args: &[String],
    dir: &WorkDir,
    control: &Control,
    failure: &str,
) -> Result<Vec<u8>, AppError> {
    run_output(executable, args, dir, control, failure)
        .await
        .map(|output| output.0)
}

pub(super) async fn run_output(
    executable: &Path,
    args: &[String],
    dir: &WorkDir,
    control: &Control,
    failure: &str,
) -> Result<(Vec<u8>, Vec<u8>), AppError> {
    if control.stopped() {
        return Err(error("Song import cancelled or timed out."));
    }
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .current_dir(&dir.0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().map_err(|_| error(failure))?;
    // Standalone yt-dlp can launch its Python worker and a JS solver. Capture
    // creation-time identity so cancellation stops only this owned process tree.
    let owned = child
        .id()
        .and_then(|pid| crate::comfyui::ManagedProcess::from_child(pid).ok());
    let stdout = child.stdout.take().ok_or_else(|| error(failure))?;
    let reader = tokio::spawn(read_bounded(stdout, 64 * 1024));
    let stderr = child.stderr.take().ok_or_else(|| error(failure))?;
    let errors = tokio::spawn(read_bounded(stderr, 64 * 1024));
    let mut monitor = tokio::time::interval(Duration::from_millis(200));
    let outcome = loop {
        tokio::select! {
            result = child.wait() => break result,
            _ = monitor.tick() => {
                if control.stopped() || dir.size().unwrap_or(u64::MAX) > AUDIO_LIMIT + 16 * 1024 * 1024 {
                    if let Some(owned) = owned {
                        let _ = tokio::task::spawn_blocking(move || owned.stop()).await;
                    }
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    reader.abort();
                    errors.abort();
                    return Err(error("Song import cancelled, expired, or exceeded the size limit."));
                }
            }
        }
    };
    let output = reader.await.unwrap_or_default();
    let diagnostic = errors.await.unwrap_or_default();
    if !outcome?.success() {
        return Err(error(failure));
    }
    Ok((output, diagnostic))
}

async fn import(
    state: &AppState,
    link: SongLink,
    query: &str,
    downloader: PathBuf,
    converter: PathBuf,
    control: &Control,
) -> Result<Value, AppError> {
    let matched = !matches!(link.service, Service::Youtube | Service::Dailymotion);
    let query = if matched && query.is_empty() {
        tokio::select! {
            result = matching_query(state, &link) => result?,
            _ = stopped(control) => return Err(error("Song import cancelled or expired.")),
        }
    } else {
        query.into()
    };
    let target = if matched {
        format!("ytsearch1:{query} official audio")
    } else {
        link.url.clone()
    };
    let dir = WorkDir::new()?;
    let mut args: Vec<String> = [
        "--ignore-config",
        "--no-plugin-dirs",
        "--no-cache-dir",
        "--no-playlist",
        "--no-progress",
        "--no-warnings",
        "--no-simulate",
        "--no-write-info-json",
        "--no-write-thumbnail",
        "--no-write-subs",
        "--no-write-auto-subs",
        "--no-embed-metadata",
        "--fixup",
        "never",
        "--downloader",
        "native",
        "--hls-prefer-native",
        "--socket-timeout",
        "15",
        "--retries",
        "2",
        "--fragment-retries",
        "2",
        "--max-filesize",
        "64M",
        "--match-filters",
        "!is_live & duration > 0 & duration <= 360",
        "--use-extractors",
        "youtube.*,dailymotion.*",
        "--format",
        "bestaudio/best",
        "--output",
        "source.%(ext)s",
        "--print",
        "after_video:%(.{title,webpage_url,duration})j",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    // Use the verified runtimes immediately, without requiring a PATH change/restart.
    args.extend([
        "--ffmpeg-location".into(),
        converter.to_string_lossy().into_owned(),
    ]);
    args.push("--no-js-runtimes".into());
    for runtime in ["deno", "node"] {
        if let Some(path) = state.media_tools.path(runtime) {
            args.extend([
                "--js-runtimes".into(),
                format!("{runtime}:{}", path.display()),
            ]);
        }
    }
    args.extend(["--".into(), target]);
    let output = run(&downloader, &args, &dir, control, "Could not download this song. It may be unavailable, require sign-in, exceed 360 seconds/64 MiB, or need an updated yt-dlp and JavaScript runtime. Try a public single-song link or upload audio manually.").await?;
    let metadata: Value = serde_json::from_slice(&output)
        .map_err(|_| error("No downloadable song was found within the 360-second limit."))?;
    let actual = parse_link(metadata["webpage_url"].as_str().unwrap_or(""))?;
    if !matches!(actual.service, Service::Youtube | Service::Dailymotion) {
        return Err(error("Unexpected song source."));
    }
    let duration = metadata["duration"]
        .as_f64()
        .filter(|d| d.is_finite() && *d > 0.0 && *d <= 360.0)
        .ok_or_else(|| error("Choose a recording up to 360 seconds."))?;
    let source = std::fs::read_dir(&dir.0)?
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name().to_string_lossy().starts_with("source.")
                && e.path()
                    .extension()
                    .is_some_and(|e| e != "part" && e != "ytdl" && e != "json")
        })
        .ok_or_else(|| error("The downloader returned no audio."))?;
    if !source.file_type()?.is_file() || source.metadata()?.len() > AUDIO_LIMIT {
        return Err(error("Downloaded song exceeds 64 MiB."));
    }
    // Run FFmpeg ourselves so cancellation can reap every media process before cleanup.
    let args = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-protocol_whitelist".into(),
        "file,pipe".into(),
        "-i".into(),
        source.path().to_string_lossy().into_owned(),
        "-map".into(),
        "0:a:0".into(),
        "-vn".into(),
        "-map_metadata".into(),
        "-1".into(),
        "-t".into(),
        "361".into(),
        "-ac".into(),
        "2".into(),
        "-ar".into(),
        "44100".into(),
        "-c:a".into(),
        "libmp3lame".into(),
        "-b:a".into(),
        "192k".into(),
        "-y".into(),
        "cover.mp3".into(),
    ];
    run(
        &converter,
        &args,
        &dir,
        control,
        "Could not convert the song to MP3. Try another recording or upload audio manually.",
    )
    .await?;
    let bytes = tokio::fs::read(dir.0.join("cover.mp3")).await?;
    if bytes.is_empty() || bytes.len() as u64 > AUDIO_LIMIT {
        return Err(error("Converted source is empty or exceeds 64 MiB."));
    }
    // Surface a deletion failure instead of claiming a retained download is temporary.
    tokio::fs::remove_dir_all(&dir.0).await?;
    Ok(
        json!({"status": "completed", "audio_base64": STANDARD.encode(bytes), "title": metadata["title"].as_str().unwrap_or("Cover source").chars().take(200).collect::<String>(), "source_url": actual.url, "requested_url": link.url, "matched": matched, "duration": duration}),
    )
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_music_link_capabilities(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Value, AppError> {
    Ok(state.media_tools.status())
}
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn prepare_music_link_tools(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Value, AppError> {
    crate::media_tools::start(state.inner().clone());
    Ok(state.media_tools.status())
}
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn import_music_link(
    state: tauri::State<'_, Arc<AppState>>,
    url: String,
    query: String,
) -> Result<String, AppError> {
    start(state.inner().clone(), &url, &query, None).await
}
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_music_link_import(
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
    fn links_are_single_tracks_with_fixed_hosts_and_clean_queries() {
        for raw in [
            "https://youtu.be/dQw4w9WgXcQ?t=10",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=private",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        ] {
            assert_eq!(
                parse_link(raw).unwrap().url,
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
            );
        }
        for raw in [
            "https://open.spotify.com/intl-de/track/4uLU6hMCjMI75M1A2tKUQC?si=private",
            "https://www.deezer.com/en/track/3135556",
            "https://listen.tidal.com/track/123",
            "https://tidal.com/browse/track/123",
            "https://dai.ly/x123",
            "https://www.dailymotion.com/video/x123_title",
        ] {
            assert!(parse_link(raw).is_ok(), "{raw}");
        }
        for raw in [
            "file:///song.mp3",
            "https://localhost/track/123",
            "https://youtube.com.evil.test/watch?v=dQw4w9WgXcQ",
            "https://user:pass@youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com:123/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/playlist?list=abc",
            "https://open.spotify.com/album/4uLU6hMCjMI75M1A2tKUQC",
            "https://tidal.com/track/../private",
            "--exec=command",
            "https://deezer.com/track/%2fetc",
        ] {
            assert!(parse_link(raw).is_err(), "{raw}");
        }
    }
    #[test]
    fn public_metadata_requires_artist_and_title() {
        let html = r#"<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"name":"Song","artists":[{"name":"Artist"}]}}}}}}</script>"#;
        assert_eq!(spotify_query(html).as_deref(), Some("Artist Song"));
        assert!(spotify_query("<html>Sign in</html>").is_none());
        assert_eq!(
            tidal_query(r#"<meta property="og:title" content="Artist - Song">"#).as_deref(),
            Some("Artist - Song")
        );
        assert!(share_url("https://spotify.link/abc").is_some());
        assert!(share_url("https://spotify.link.evil.test/abc").is_none());
        assert!(share_url("https://user@link.deezer.com/s/abc").is_none());
        assert_eq!(
            tidal_query(r#"<meta content="Song by Artist on TIDAL" property="og:title">"#)
                .as_deref(),
            Some("Song by Artist")
        );
        assert!(tidal_query(r#"<meta content="TIDAL" property="og:title">"#).is_none());
    }
    #[test]
    fn work_directory_removes_partial_downloads_on_error() {
        let dir = WorkDir::new().unwrap();
        let path = dir.0.clone();
        std::fs::write(path.join("source.part"), b"partial").unwrap();
        drop(dir);
        assert!(!path.exists());
    }

    #[test]
    fn import_subprocess_fixture() {
        // This fixture only blocks when explicitly launched in our private test dir.
        if Path::new("run-fixture").is_file() {
            std::fs::write("source.part", b"incomplete download").unwrap();
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[tokio::test]
    async fn cancellation_reaps_the_downloader_before_removing_its_files() {
        let dir = WorkDir::new().unwrap();
        let path = dir.0.clone();
        std::fs::write(path.join("run-fixture"), b"test").unwrap();
        let control = Arc::new(Control::new());
        let cancel = control.clone();
        let watched = path.clone();
        let monitor = tokio::spawn(async move {
            for _ in 0..100 {
                if watched.join("source.part").exists() {
                    cancel.cancelled.store(true, Ordering::Relaxed);
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            cancel.cancelled.store(true, Ordering::Relaxed);
            false
        });
        let result = run(
            &std::env::current_exe().unwrap(),
            &[
                "--exact".into(),
                "commands::music_link::tests::import_subprocess_fixture".into(),
                "--nocapture".into(),
            ],
            &dir,
            &control,
            "Fixture failed",
        )
        .await;
        assert!(monitor.await.unwrap(), "Fixture never started");
        assert!(result.is_err());
        drop(dir);
        assert!(
            !path.exists(),
            "Cancellation left a working directory behind"
        );
    }
    #[tokio::test]
    async fn imports_are_private_cancelled_and_consumed_once() {
        let state = AppState::new(crate::config::AppConfig::default());
        let control = Arc::new(Control::new());
        state.link_imports.jobs.lock().await.insert(
            "test".into(),
            Job {
                owner: Some("alice".into()),
                control: control.clone(),
                result: None,
            },
        );
        assert!(status(&state, "test", Some("bob"), true).await.is_err());
        assert!(!control.stopped());
        assert_eq!(
            status(&state, "test", Some("alice"), false).await.unwrap()["status"],
            "running"
        );
        state
            .link_imports
            .jobs
            .lock()
            .await
            .get_mut("test")
            .unwrap()
            .result = Some(json!({"status": "completed", "audio_base64": "private"}));
        assert_eq!(
            status(&state, "test", Some("alice"), false).await.unwrap()["audio_base64"],
            "private"
        );
        assert!(status(&state, "test", Some("alice"), false).await.is_err());
        *control.touched.lock().unwrap() = Instant::now() - LEASE - Duration::from_secs(1);
        assert!(control.stopped());
    }
}
