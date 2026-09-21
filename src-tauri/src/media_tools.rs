//! App-owned music prerequisites. Fixed, checksum-pinned packages; no shell installers,
//! elevation, system PATH changes, or dependency on ComfyUI's Python environment.
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};
use tokio::io::AsyncWriteExt;

const DOWNLOAD_LIMIT: u64 = 256 * 1024 * 1024;
const EXTRACT_LIMIT: u64 = 512 * 1024 * 1024;
const TOOLS: [&str; 4] = ["yt-dlp", "ffmpeg", "deno", "node"];

#[derive(Clone, Debug, Deserialize)]
struct Package {
    tool: String,
    version: String,
    url: String,
    sha256: String,
    kind: String,
    member: String,
    binary: String,
}

fn packages(platform: &str) -> Result<Vec<Package>, String> {
    let mut manifest: HashMap<String, Vec<Package>> =
        serde_json::from_str(include_str!("media_tools_manifest.json"))
            .map_err(|_| "Invalid media tool manifest")?;
    manifest.remove(platform).ok_or_else(|| "Automatic music-tool setup is not available for this platform. Configure working yt-dlp, FFmpeg, Deno and Node.js executables on the host.".into())
}

#[derive(Clone, Serialize)]
struct Progress {
    status: &'static str,
    tool: String,
    completed: usize,
    total: usize,
    percent: Option<u64>,
    error: Option<String>,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            status: "checking",
            tool: String::new(),
            completed: 0,
            total: TOOLS.len(),
            percent: None,
            error: None,
        }
    }
}

#[derive(Default)]
pub struct MediaTools {
    progress: RwLock<Progress>,
    selected: RwLock<HashMap<String, PathBuf>>,
    running: AtomicBool,
    closing: AtomicBool,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    proxy_client: Mutex<Option<(String, reqwest::Client)>>,
}
impl MediaTools {
    fn progress(
        &self,
        status: &'static str,
        tool: &str,
        completed: usize,
        percent: Option<u64>,
        error: Option<String>,
    ) {
        if let Ok(mut progress) = self.progress.write() {
            *progress = Progress {
                status,
                tool: tool.into(),
                completed,
                total: TOOLS.len(),
                percent,
                error,
            };
        }
    }
    pub(crate) fn status(&self) -> Value {
        let progress = self.progress.read().map(|p| p.clone()).unwrap_or_default();
        let mut value =
            serde_json::to_value(&progress).unwrap_or_else(|_| json!({"status":"error"}));
        let available =
            progress.status == "ready" && TOOLS.iter().all(|tool| self.path(tool).is_some());
        value["available"] = json!(available);
        if progress.status == "ready" && !available {
            value["status"] = json!("error");
            value["error"] = json!("A music tool is missing. Retry setup to repair it.");
        }
        value["downloader"] = json!(self.path("yt-dlp").is_some());
        value["converter"] = json!(self.path("ffmpeg").is_some());
        value
    }
    pub(crate) fn path(&self, tool: &str) -> Option<PathBuf> {
        self.selected
            .read()
            .ok()?
            .get(tool)
            .filter(|p| p.is_file())
            .cloned()
    }
    fn stopped(&self) -> Result<(), String> {
        if self.closing.load(Ordering::Relaxed) {
            Err("Music-tool setup stopped because the app is closing.".into())
        } else {
            Ok(())
        }
    }
}

/// Called by both desktop and headless startup. Repeated calls cannot overlap.
pub fn start(state: Arc<AppState>) {
    let Ok(mut slot) = state.media_tools.task.lock() else {
        return;
    };
    if state.media_tools.closing.load(Ordering::Relaxed)
        || state.media_tools.status()["available"] == true
        || state
            .media_tools
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return;
    }
    state.media_tools.progress("checking", "", 0, None, None);
    let shared = state.clone();
    let task = tokio::spawn(async move {
        for attempt in 0..3 {
            let result = prepare(&shared).await;
            if shared.media_tools.closing.load(Ordering::Relaxed) {
                break;
            }
            match result {
                Ok(()) => {
                    shared
                        .media_tools
                        .progress("ready", "", TOOLS.len(), Some(100), None);
                    break;
                }
                Err(error) => {
                    let previous = shared
                        .media_tools
                        .progress
                        .read()
                        .map(|p| p.clone())
                        .unwrap_or_default();
                    shared.media_tools.progress(
                        if attempt < 2 { "retrying" } else { "error" },
                        &previous.tool,
                        previous.completed,
                        None,
                        Some(error),
                    );
                    if attempt == 2 {
                        break;
                    }
                    // Recover from transient/offline startup without a modal or restart.
                    let delay = if attempt == 0 { 15 } else { 60 };
                    for _ in 0..delay {
                        if shared.media_tools.closing.load(Ordering::Relaxed) {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    if shared.media_tools.closing.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }
        shared.media_tools.running.store(false, Ordering::SeqCst);
    });
    *slot = Some(task);
}

pub async fn shutdown(state: &AppState) {
    state.media_tools.closing.store(true, Ordering::Relaxed);
    let task = state
        .media_tools
        .task
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some(task) = task {
        let _ = task.await;
    }
}

fn candidates(tool: &str) -> Vec<PathBuf> {
    let variable = match tool {
        "yt-dlp" => "MOOSHIE_YT_DLP",
        "ffmpeg" => "MOOSHIE_FFMPEG",
        "deno" => "MOOSHIE_DENO",
        _ => "MOOSHIE_NODE",
    };
    let mut paths: Vec<_> = std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .into_iter()
        .collect();
    let name = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.into()
    };
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(
            std::env::split_paths(&path)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(&name)),
        );
    }
    #[cfg(target_os = "macos")]
    paths.extend([
        PathBuf::from("/opt/homebrew/bin").join(&name),
        PathBuf::from("/usr/local/bin").join(&name),
    ]);
    paths.retain(|p| p.is_file());
    paths.dedup();
    paths
}

fn version(text: &str) -> Option<(u32, u32, u32)> {
    let re = regex::Regex::new(r"(\d+)\.(\d+)(?:\.(\d+))?").ok()?;
    let found = re.captures(text.lines().next()?)?;
    Some((
        found[1].parse().ok()?,
        found[2].parse().ok()?,
        found.get(3).map_or(Some(0), |s| s.as_str().parse().ok())?,
    ))
}

fn compatible(tool: &str, text: &str) -> bool {
    let Some(version) = version(text) else {
        return false;
    };
    match tool {
        "yt-dlp" => version >= (2026, 8, 19),
        "deno" => text.starts_with("deno ") && version >= (2, 3, 0),
        "node" => text.starts_with('v') && version >= (22, 0, 0),
        "ffmpeg" => text.starts_with("ffmpeg version ") && version >= (6, 0, 0),
        _ => false,
    }
}

async fn output(path: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = tokio::process::Command::new(path);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let result = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| "Tool verification timed out")?
        .map_err(|_| "Tool could not start")?;
    if !result.status.success() || result.stdout.len() > 256 * 1024 {
        return Err("Tool verification failed".into());
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

async fn verify(path: &Path, tool: &str) -> Result<(), String> {
    let result = output(
        path,
        &[if tool == "ffmpeg" {
            "-version"
        } else {
            "--version"
        }],
    )
    .await?;
    if !compatible(tool, &result) {
        return Err(format!("{tool} is not a supported version"));
    }
    if tool == "ffmpeg"
        && !output(path, &["-hide_banner", "-encoders"])
            .await?
            .contains("libmp3lame")
    {
        return Err("FFmpeg has no MP3 encoder".into());
    }
    Ok(())
}

async fn prepare(state: &AppState) -> Result<(), String> {
    let base = crate::config::app_data_dir()
        .ok_or("App data folder is unavailable")?
        .join("bin/media");
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let packages = packages(&platform).unwrap_or_default();
    let network_proxy = state.config.read().await.network_proxy.clone();
    for (index, tool) in TOOLS.iter().enumerate() {
        state.media_tools.stopped()?;
        state
            .media_tools
            .progress("checking", tool, index, None, None);
        let package = packages.iter().find(|p| p.tool == *tool);
        let managed = package.map(|p| destination(&base, p));
        let mut chosen = None;
        // An existing managed copy is verified offline; no repeated downloads.
        if let (Some(package), Some(dir)) = (package, managed) {
            if verify_installed(&dir, package).await.is_ok() {
                chosen = Some(dir.join(&package.binary));
            }
        }
        if chosen.is_none() {
            for candidate in candidates(tool) {
                state.media_tools.stopped()?;
                // yt-dlp's PyPI/system install can lack EJS even when --version works.
                // Prefer our self-contained distribution unless explicitly configured.
                if *tool == "yt-dlp"
                    && std::env::var_os("MOOSHIE_YT_DLP")
                        .map(PathBuf::from)
                        .as_ref()
                        != Some(&candidate)
                {
                    continue;
                }
                if verify(&candidate, tool).await.is_ok() {
                    chosen = Some(candidate);
                    break;
                }
            }
        }
        let path = if let Some(path) = chosen {
            path
        } else {
            let package = package.ok_or_else(|| format!("No automatic {tool} package is available for {platform}. Configure this tool on the host."))?;
            let client = download_client(state, network_proxy.as_deref())?;
            install(&base, package, &client, &state.media_tools, index).await?
        };
        state
            .media_tools
            .selected
            .write()
            .map_err(|_| "Tool state is unavailable")?
            .insert((*tool).into(), path);
    }
    Ok(())
}

fn download_client(state: &AppState, proxy: Option<&str>) -> Result<reqwest::Client, String> {
    let Some(proxy) = proxy.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(state.http_client.clone());
    };
    let mut cached = state
        .media_tools
        .proxy_client
        .lock()
        .map_err(|_| "Download client is unavailable")?;
    if cached.as_ref().is_none_or(|(url, _)| url != proxy) {
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(proxy).map_err(|_| "Check the network proxy in Settings")?)
            .build()
            .map_err(|_| "Could not configure the download proxy")?;
        *cached = Some((proxy.into(), client));
    }
    cached
        .as_ref()
        .map(|(_, client)| client.clone())
        .ok_or_else(|| "Download client is unavailable".into())
}

fn destination(base: &Path, package: &Package) -> PathBuf {
    base.join(format!(
        "{}-{}-{}-{}",
        package.tool,
        package.version,
        std::env::consts::OS,
        std::env::consts::ARCH
    ))
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    package_sha256: String,
    executable_sha256: String,
}
fn file_hash(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|_| "Cannot read the installed tool")?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Cannot read the installed tool")?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hex::encode(hash.finalize()))
}
async fn verify_installed(dir: &Path, package: &Package) -> Result<(), String> {
    let dir = dir.to_owned();
    let spec = package.clone();
    let executable = tokio::task::spawn_blocking(move || {
        let receipt: Receipt = serde_json::from_slice(
            &std::fs::read(dir.join("receipt.json")).map_err(|_| "Tool is not installed")?,
        )
        .map_err(|_| "Incomplete tool installation")?;
        let path = dir.join(&spec.binary);
        if receipt.package_sha256 != spec.sha256 || file_hash(&path)? != receipt.executable_sha256 {
            return Err("Installed tool checksum mismatch".to_string());
        }
        Ok(path)
    })
    .await
    .map_err(|_| "Tool verification stopped")??;
    verify(&executable, &package.tool).await
}

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn download(
    package: &Package,
    path: &Path,
    client: &reqwest::Client,
    state: &MediaTools,
    index: usize,
) -> Result<(), String> {
    let mut request = Box::pin(
        client
            .get(&package.url)
            .header("User-Agent", "Mozilla/5.0")
            .timeout(Duration::from_secs(300))
            .send(),
    );
    let mut response = loop {
        tokio::select! {
            result = &mut request => break result.map_err(|_| format!("Could not download {}. Check the connection or proxy and retry.", package.tool))?,
            _ = tokio::time::sleep(Duration::from_millis(200)) => state.stopped()?,
        }
    }.error_for_status().map_err(|_| format!("The {} download server is unavailable. Please retry.", package.tool))?;
    let total = response.content_length();
    if total.is_some_and(|n| n > DOWNLOAD_LIMIT) {
        return Err("Tool download exceeds the size limit".into());
    }
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|_| "Cannot create a tool download; check free space and folder permissions")?;
    let mut hash = Sha256::new();
    let mut downloaded = 0u64;
    loop {
        state.stopped()?;
        let chunk = tokio::select! {
            result = response.chunk() => result.map_err(|_| "Tool download was interrupted; please retry")?,
            _ = tokio::time::sleep(Duration::from_millis(200)) => { state.stopped()?; continue; }
        };
        let Some(chunk) = chunk else {
            break;
        };
        downloaded += chunk.len() as u64;
        if downloaded > DOWNLOAD_LIMIT {
            return Err("Tool download exceeds the size limit".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|_| "Cannot save the tool download; check free disk space")?;
        state.progress(
            "downloading",
            &package.tool,
            index,
            total
                .filter(|n| *n > 0)
                .map(|n| (100 * downloaded / n).min(100)),
            None,
        );
    }
    file.flush()
        .await
        .map_err(|_| "Cannot finish the tool download")?;
    if hex::encode(hash.finalize()) != package.sha256 {
        return Err(format!(
            "{} download failed checksum verification. Please retry.",
            package.tool
        ));
    }
    Ok(())
}

fn copy_bounded(reader: impl Read, path: &Path) -> Result<(), String> {
    let mut reader = reader.take(EXTRACT_LIMIT + 1);
    let mut file = std::fs::File::create(path).map_err(|_| "Cannot create the tool executable")?;
    let count = std::io::copy(&mut reader, &mut file).map_err(|_| "Cannot extract the tool")?;
    if count == 0 || count > EXTRACT_LIMIT {
        return Err("Invalid tool executable size".into());
    }
    file.flush().map_err(|_| "Cannot finish writing the tool")?;
    Ok(())
}

fn extract(archive: &Path, destination: &Path, package: &Package) -> Result<(), String> {
    let source = std::fs::File::open(archive).map_err(|_| "Cannot open the verified package")?;
    std::fs::create_dir(destination).map_err(|_| "Cannot create the tool folder")?;
    let executable = destination.join(&package.binary);
    match package.kind.as_str() {
        "raw" => copy_bounded(source, &executable)?,
        "zip" => {
            let mut zip = zip::ZipArchive::new(source).map_err(|_| "Invalid tool ZIP archive")?;
            let entry = zip
                .by_name(&package.member)
                .map_err(|_| "Tool archive has no expected executable")?;
            if !entry.is_file() || entry.is_symlink() {
                return Err("Tool archive executable is not a regular file".into());
            }
            copy_bounded(entry, &executable)?;
            // Preserve a supplied license without extracting arbitrary archive paths.
            for i in 0..zip.len() {
                let entry = zip.by_index(i).map_err(|_| "Invalid tool ZIP entry")?;
                if entry.is_file()
                    && !entry.is_symlink()
                    && entry.size() <= 2 * 1024 * 1024
                    && Path::new(entry.name())
                        .file_name()
                        .is_some_and(|n| n == "LICENSE" || n == "LICENSE.txt")
                {
                    copy_bounded(entry, &destination.join("LICENSE.txt"))?;
                    break;
                }
            }
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        "tar.gz" => {
            let decoder = flate2::read::GzDecoder::new(source).take(EXTRACT_LIMIT + 1);
            let mut archive = tar::Archive::new(decoder);
            let mut found = false;
            for entry in archive.entries().map_err(|_| "Invalid tool tar archive")? {
                let entry = entry.map_err(|_| "Invalid tool tar entry")?;
                let name = entry
                    .path()
                    .map_err(|_| "Invalid tool archive path")?
                    .into_owned();
                if name == Path::new(&package.member) {
                    if !entry.header().entry_type().is_file() {
                        return Err("Tool archive executable is not a regular file".into());
                    }
                    copy_bounded(entry, &executable)?;
                    found = true;
                } else if name.file_name().is_some_and(|n| n == "LICENSE")
                    && entry.header().entry_type().is_file()
                {
                    copy_bounded(entry, &destination.join("LICENSE.txt"))?;
                }
            }
            if !found {
                return Err("Tool archive has no expected executable".into());
            }
        }
        _ => return Err("Unsupported tool archive".into()),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .map_err(|_| "Cannot make the tool executable")?;
    }
    let receipt = Receipt {
        package_sha256: package.sha256.clone(),
        executable_sha256: file_hash(&executable)?,
    };
    std::fs::write(
        destination.join("receipt.json"),
        serde_json::to_vec(&receipt).map_err(|_| "Cannot encode tool receipt")?,
    )
    .map_err(|_| "Cannot save tool receipt")?;
    std::fs::write(
        destination.join("SOURCE.txt"),
        format!(
            "{} {}\n{}\nSHA-256: {}\n",
            package.tool, package.version, package.url, package.sha256
        ),
    )
    .map_err(|_| "Cannot save tool source notice")?;
    Ok(())
}

fn publish(staging: &Path, destination: &Path) -> Result<(), String> {
    let backup = staging.with_extension("previous");
    let previous = destination.exists();
    if previous {
        std::fs::rename(destination, &backup).map_err(|_| {
            "Cannot replace the damaged tool; close other MooshieUI instances and retry"
        })?;
    }
    if std::fs::rename(staging, destination).is_err() {
        if previous {
            let _ = std::fs::rename(&backup, destination);
        }
        return Err("Cannot finish installing the tool".into());
    }
    if previous {
        let _ = std::fs::remove_dir_all(backup);
    }
    Ok(())
}

async fn install(
    base: &Path,
    package: &Package,
    client: &reqwest::Client,
    state: &MediaTools,
    index: usize,
) -> Result<PathBuf, String> {
    state.stopped()?;
    tokio::fs::create_dir_all(base)
        .await
        .map_err(|_| "Cannot create the app's tools folder")?;
    let staging = Staging(base.join(format!(".install-{}", uuid::Uuid::new_v4())));
    tokio::fs::create_dir(&staging.0)
        .await
        .map_err(|_| "Cannot create the tool staging folder")?;
    state.progress("downloading", &package.tool, index, Some(0), None);
    download(package, &staging.0.join("download"), client, state, index).await?;
    state.stopped()?;
    state.progress("verifying", &package.tool, index, None, None);
    let stage = staging.0.clone();
    let spec = package.clone();
    tokio::task::spawn_blocking(move || {
        extract(&stage.join("download"), &stage.join("tool"), &spec)
    })
    .await
    .map_err(|_| "Tool extraction stopped")??;
    state.stopped()?;
    verify(&staging.0.join("tool").join(&package.binary), &package.tool)
        .await
        .map_err(|e| format!("Downloaded {} could not be verified: {e}", package.tool))?;
    state.stopped()?;
    let destination = destination(base, package);
    if let Err(error) = publish(&staging.0.join("tool"), &destination) {
        // Another app instance may have finished the same pinned installation.
        if verify_installed(&destination, package).await.is_err() {
            return Err(error);
        }
    }
    Ok(destination.join(&package.binary))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifests_cover_release_platforms_with_pinned_https_packages() {
        for platform in [
            "windows-x86_64",
            "macos-x86_64",
            "macos-aarch64",
            "linux-x86_64",
            "linux-aarch64",
        ] {
            let packages = packages(platform).unwrap();
            assert_eq!(packages.len(), 4);
            for (tool, package) in TOOLS.iter().zip(packages) {
                assert_eq!(*tool, package.tool);
                assert_eq!(package.sha256.len(), 64);
                assert!(package.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
                let url = url::Url::parse(&package.url).unwrap();
                assert_eq!(url.scheme(), "https");
                assert!(matches!(
                    url.host_str(),
                    Some("github.com" | "nodejs.org" | "ffmpeg.martin-riedl.de")
                ));
                assert!(!package.url.contains("/latest/"));
                assert_eq!(Path::new(&package.binary).components().count(), 1);
            }
        }
    }
    #[test]
    fn outdated_or_wrong_tools_are_not_reported_ready() {
        assert!(compatible("deno", "deno 2.9.7 (stable)"));
        assert!(!compatible("deno", "deno 2.2.1"));
        assert!(compatible("node", "v24.21.0"));
        assert!(!compatible("node", "v20.19.0"));
        assert!(compatible("yt-dlp", "2026.08.19"));
        assert!(!compatible("yt-dlp", "2024.01.01"));
        assert!(compatible("ffmpeg", "ffmpeg version 9.0.1-full_build"));
        assert!(!compatible("ffmpeg", "some other program 9.0.1"));
    }
    #[test]
    fn archive_path_traversal_is_not_extracted() {
        let root =
            Staging(std::env::temp_dir().join(format!("media-test-{}", uuid::Uuid::new_v4())));
        std::fs::create_dir(&root.0).unwrap();
        let archive = root.0.join("archive.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file("../../outside", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"bad").unwrap();
        zip.finish().unwrap();
        let package = packages("windows-x86_64").unwrap().remove(2);
        assert!(extract(&archive, &root.0.join("tool"), &package).is_err());
        assert!(!root.0.join("tool").join(&package.binary).exists());
    }

    #[tokio::test]
    async fn corrupted_installation_is_rejected_before_execution() {
        let root =
            Staging(std::env::temp_dir().join(format!("media-test-{}", uuid::Uuid::new_v4())));
        std::fs::create_dir(&root.0).unwrap();
        let package = packages("windows-x86_64").unwrap().remove(2);
        let archive = root.0.join("archive.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file(&package.member, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"original content").unwrap();
        zip.finish().unwrap();
        let dir = root.0.join("tool");
        extract(&archive, &dir, &package).unwrap();
        std::fs::write(dir.join(&package.binary), b"changed content").unwrap();
        assert_eq!(
            verify_installed(&dir, &package).await.unwrap_err(),
            "Installed tool checksum mismatch"
        );
        assert_eq!(
            std::fs::read(dir.join(&package.binary)).unwrap(),
            b"changed content"
        );
    }

    #[tokio::test]
    async fn bad_download_checksum_cleans_staging_and_preserves_existing_tool() {
        use tokio::io::AsyncReadExt;
        let root =
            Staging(std::env::temp_dir().join(format!("media-test-{}", uuid::Uuid::new_v4())));
        std::fs::create_dir(&root.0).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut package = packages("windows-x86_64").unwrap().remove(0);
        package.url = format!("http://{}/tool", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = connection.read(&mut request).await;
            connection
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad")
                .await
                .unwrap();
        });
        let destination = destination(&root.0, &package);
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join(&package.binary), b"existing tool").unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let error = install(&root.0, &package, &client, &MediaTools::default(), 0)
            .await
            .unwrap_err();
        assert!(error.contains("checksum verification"));
        assert_eq!(
            std::fs::read(destination.join(&package.binary)).unwrap(),
            b"existing tool"
        );
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 1);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn closing_interrupts_a_stalled_download_and_cleans_partial_files() {
        let root =
            Staging(std::env::temp_dir().join(format!("media-test-{}", uuid::Uuid::new_v4())));
        std::fs::create_dir(&root.0).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut package = packages("windows-x86_64").unwrap().remove(0);
        package.url = format!("http://{}/tool", listener.local_addr().unwrap());
        let state = Arc::new(MediaTools::default());
        let closing = state.clone();
        let server = tokio::spawn(async move {
            let (_connection, _) = listener.accept().await.unwrap();
            closing.closing.store(true, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_secs(3)).await;
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let error = tokio::time::timeout(
            Duration::from_secs(2),
            install(&root.0, &package, &client, &state, 0),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.contains("app is closing"));
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
        server.abort();
    }

    #[test]
    fn replacing_an_installation_is_atomic_and_keeps_other_tools() {
        let root =
            Staging(std::env::temp_dir().join(format!("media-test-{}", uuid::Uuid::new_v4())));
        std::fs::create_dir(&root.0).unwrap();
        let old = root.0.join("installed");
        let new = root.0.join("staged");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::write(old.join("tool"), "old").unwrap();
        std::fs::write(new.join("tool"), "new").unwrap();
        std::fs::write(root.0.join("other"), "preserved").unwrap();
        publish(&new, &old).unwrap();
        assert_eq!(std::fs::read_to_string(old.join("tool")).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(root.0.join("other")).unwrap(),
            "preserved"
        );
        assert!(!new.exists());
        assert!(!new.with_extension("previous").exists());
    }
    #[tokio::test]
    #[ignore = "Downloads and runs verified packages; set MOOSHIE_MEDIA_TOOLS_TEST_DIR to an isolated test directory"]
    async fn live_install_and_offline_reuse() {
        let base = PathBuf::from(
            std::env::var_os("MOOSHIE_MEDIA_TOOLS_TEST_DIR")
                .expect("Set an isolated test directory"),
        );
        assert!(base.is_absolute());
        let state = MediaTools::default();
        let client = reqwest::Client::new();
        for (index, package) in packages(&format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
        .unwrap()
        .iter()
        .enumerate()
        {
            let destination = destination(&base, package);
            if verify_installed(&destination, package).await.is_err() {
                install(&base, package, &client, &state, index)
                    .await
                    .unwrap();
            }
            verify_installed(&destination, package).await.unwrap();
            println!(
                "PASS installed and verified offline: {} {}",
                package.tool, package.version
            );
        }
    }
}
