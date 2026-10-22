//! Optional Cloudflare quick tunnel, so cloud-hosted MCP clients (Claude's
//! custom connectors, ChatGPT) can reach the local MCP server.
//!
//! Those clients connect from their own servers, never from the owner's PC,
//! so `127.0.0.1` is unreachable for them. A quick tunnel gives the server a
//! public `https://<words>.trycloudflare.com` address with no Cloudflare
//! account. The address changes every time the tunnel starts.
//!
//! `cloudflared` is a fixed release, downloaded on first use and checked
//! against a pinned SHA-256 of the executable before every start.
//! `--http-host-header` makes every tunnelled request arrive with a local
//! `Host`, so the server's DNS-rebinding check needs no exception for it.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

const VERSION: &str = "2026.10.0";
const DOWNLOAD_LIMIT: u64 = 128 * 1024 * 1024;
/// Waits this long between restarts after `cloudflared` exits.
const RESTART_DELAY: Duration = Duration::from_secs(15);
/// Gives up on an address after this long without one.
const START_TIMEOUT: Duration = Duration::from_secs(90);

struct Package {
    /// Release asset name.
    asset: &'static str,
    /// SHA-256 of the downloaded asset.
    asset_sha256: &'static str,
    /// SHA-256 of the `cloudflared` executable (the asset itself, or the
    /// binary inside the macOS archive).
    exe_sha256: &'static str,
}

fn package(platform: &str) -> Option<Package> {
    let (asset, asset_sha256, exe_sha256) = match platform {
        "windows-x86_64" => (
            "cloudflared-windows-amd64.exe",
            "86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c",
            "86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c",
        ),
        "linux-x86_64" => (
            "cloudflared-linux-amd64",
            "d33ff2d14475178d2012c2c56beba87389ac5ded27649519f198a7d3134a99db",
            "d33ff2d14475178d2012c2c56beba87389ac5ded27649519f198a7d3134a99db",
        ),
        "linux-aarch64" => (
            "cloudflared-linux-arm64",
            "e6422b9d4f72d3194bc5a38676f13667c06666523217b842a877d72a80b5ac08",
            "e6422b9d4f72d3194bc5a38676f13667c06666523217b842a877d72a80b5ac08",
        ),
        "macos-aarch64" => (
            "cloudflared-darwin-arm64.tgz",
            "a2f79ff7b9420aa537d74af239f376da170bbabeb529aec416002adac6a72e70",
            "72edfd3eea463aef4d5cb89e2e209cecb048cc756c2b01915de2e0ad7cb39830",
        ),
        "macos-x86_64" => (
            "cloudflared-darwin-amd64.tgz",
            "903845b81828c8cb3c5d13d816a2de71c06a3da5785469df8eb0e1b736d92f9f",
            "0560c9ab7281ac3f746055323623ed23bc0405b6dab9400474020cba33a978da",
        ),
        _ => return None,
    };
    Some(Package {
        asset,
        asset_sha256,
        exe_sha256,
    })
}

fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelState {
    Off,
    Downloading,
    Starting,
    Running,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatus {
    pub state: TunnelState,
    /// `https://<words>.trycloudflare.com` while running.
    pub url: Option<String>,
    pub error: Option<String>,
}

static STATUS: Mutex<TunnelStatus> = Mutex::new(TunnelStatus {
    state: TunnelState::Off,
    url: None,
    error: None,
});

/// The supervisor task and the port it tunnels to.
type Supervisor = Option<(u16, tokio::task::JoinHandle<()>)>;

static TASK: tokio::sync::Mutex<Supervisor> = tokio::sync::Mutex::const_new(None);

pub fn status() -> TunnelStatus {
    STATUS.lock().unwrap().clone()
}

fn set(state: TunnelState, url: Option<String>, error: Option<String>) {
    *STATUS.lock().unwrap() = TunnelStatus { state, url, error };
}

/// Tunnel to `port`, or stop with `None`. Restarts when the port changes.
pub async fn sync(port: Option<u16>, http_client: reqwest::Client) {
    let mut task = TASK.lock().await;
    if task.as_ref().map(|(p, _)| *p) == port {
        return;
    }
    if let Some((_, handle)) = task.take() {
        // Aborting drops the child, and `kill_on_drop` ends it.
        handle.abort();
        let _ = handle.await;
        log::info!("MCP tunnel stopped");
    }
    let Some(port) = port else {
        set(TunnelState::Off, None, None);
        return;
    };
    set(TunnelState::Starting, None, None);
    let handle = tokio::spawn(supervise(port, http_client));
    *task = Some((port, handle));
}

/// Stop the tunnel on app exit, so no `cloudflared` outlives MooshieUI.
pub async fn shutdown() {
    if let Some((_, handle)) = TASK.lock().await.take() {
        handle.abort();
        let _ = handle.await;
    }
}

async fn supervise(port: u16, http_client: reqwest::Client) {
    loop {
        let exe = match ensure_installed(&http_client).await {
            Ok(exe) => exe,
            Err(e) => {
                log::warn!("MCP tunnel unavailable: {e}");
                set(TunnelState::Error, None, Some(e));
                return;
            }
        };
        let error = match run(&exe, port).await {
            Ok(()) => "The tunnel stopped".to_string(),
            Err(e) => e,
        };
        log::warn!(
            "MCP tunnel: {error}; restarting in {}s",
            RESTART_DELAY.as_secs()
        );
        set(TunnelState::Error, None, Some(error));
        tokio::time::sleep(RESTART_DELAY).await;
        set(TunnelState::Starting, None, None);
    }
}

/// The address `cloudflared` prints once the quick tunnel exists.
pub fn quick_tunnel_url(line: &str) -> Option<String> {
    static URL: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(https://[a-z0-9]+(?:-[a-z0-9]+)*\.trycloudflare\.com)(?:[\s|]|$)")
            .unwrap()
    });
    URL.captures(line)
        .map(|c| c[1].to_string())
        .filter(|u| !u.starts_with("https://api.") && !u.starts_with("https://login."))
}

async fn run(exe: &Path, port: u16) -> Result<(), String> {
    let origin = format!("127.0.0.1:{port}");
    let mut command = tokio::process::Command::new(exe);
    command
        .args([
            "tunnel",
            "--no-autoupdate",
            "--url",
            &format!("http://{origin}"),
            "--http-host-header",
            &origin,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let mut child = command
        .spawn()
        .map_err(|e| format!("cloudflared could not start: {e}"))?;
    let stderr = child.stderr.take().ok_or("cloudflared has no output")?;
    let mut lines = tokio::io::BufReader::new(stderr).lines();
    let started = tokio::time::Instant::now();
    let mut url: Option<String> = None;
    let mut last_error = None;
    loop {
        let next = if url.is_none() {
            let left = START_TIMEOUT.saturating_sub(started.elapsed());
            match tokio::time::timeout(left, lines.next_line()).await {
                Ok(line) => line,
                Err(_) => return Err(last_error.unwrap_or_else(|| {
                    "Cloudflare did not hand out a tunnel address. Check the internet connection."
                        .to_string()
                })),
            }
        } else {
            lines.next_line().await
        };
        let Ok(Some(line)) = next else {
            break;
        };
        if url.is_none() {
            if let Some(found) = quick_tunnel_url(&line) {
                log::info!("MCP tunnel running at {found}");
                set(TunnelState::Running, Some(found.clone()), None);
                url = Some(found);
                continue;
            }
        }
        if line.contains(" ERR ") {
            let message = line
                .split_once(" ERR ")
                .map_or(line.as_str(), |(_, m)| m)
                .trim()
                .chars()
                .take(300)
                .collect::<String>();
            log::debug!("cloudflared: {message}");
            last_error = Some(message);
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    match last_error {
        Some(e) if !status.success() => Err(e),
        _ => Ok(()),
    }
}

fn install_dir() -> Option<PathBuf> {
    crate::config::app_data_dir().map(|d| d.join("bin").join(format!("cloudflared-{VERSION}")))
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash)?;
    Ok(hex::encode(hash.finalize()))
}

async fn verified(path: &Path, expected: &str) -> bool {
    let path = path.to_path_buf();
    let expected = expected.to_string();
    tokio::task::spawn_blocking(move || sha256_file(&path).is_ok_and(|h| h == expected))
        .await
        .unwrap_or(false)
}

/// The verified `cloudflared` for this platform, downloading it the first time.
async fn ensure_installed(client: &reqwest::Client) -> Result<PathBuf, String> {
    let package = package(&platform()).ok_or_else(|| {
        format!(
            "The built-in tunnel isn't available on {}. Use your own tunnel and enter its address instead.",
            platform()
        )
    })?;
    let dir = install_dir().ok_or("Cannot find the app data directory")?;
    let exe = dir.join(exe_name());
    if verified(&exe, package.exe_sha256).await {
        return Ok(exe);
    }
    set(TunnelState::Downloading, None, None);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let url = format!(
        "https://github.com/cloudflare/cloudflared/releases/download/{VERSION}/{}",
        package.asset
    );
    let partial = dir.join(format!("{}.part", package.asset));
    let result = download(client, &url, &partial, package.asset_sha256).await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(e);
    }
    let staged = dir.join(format!("{}.new", exe_name()));
    let unpacked = unpack(&partial, &staged, package.asset).await;
    let _ = tokio::fs::remove_file(&partial).await;
    unpacked?;
    if !verified(&staged, package.exe_sha256).await {
        let _ = tokio::fs::remove_file(&staged).await;
        return Err("The downloaded cloudflared failed verification".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .await
            .map_err(|e| e.to_string())?;
    }
    tokio::fs::rename(&staged, &exe)
        .await
        .map_err(|e| format!("Cannot install cloudflared: {e}"))?;
    log::info!("Installed cloudflared {VERSION} to {}", exe.display());
    Ok(exe)
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    sha256: &str,
) -> Result<(), String> {
    let mut response = client
        .get(url)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Could not download cloudflared: {e}"))?;
    if response
        .content_length()
        .is_some_and(|n| n > DOWNLOAD_LIMIT)
    {
        return Err("The cloudflared download is too large".into());
    }
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|e| format!("Cannot save cloudflared: {e}"))?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("The cloudflared download was interrupted: {e}"))?
    {
        size += chunk.len() as u64;
        if size > DOWNLOAD_LIMIT {
            return Err("The cloudflared download is too large".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Cannot save cloudflared: {e}"))?;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    if hex::encode(hash.finalize()) != sha256 {
        return Err("The cloudflared download failed checksum verification".into());
    }
    Ok(())
}

/// Copy the executable out of the verified download.
async fn unpack(download: &Path, exe: &Path, asset: &str) -> Result<(), String> {
    if !asset.ends_with(".tgz") {
        return tokio::fs::copy(download, exe)
            .await
            .map(|_| ())
            .map_err(|e| format!("Cannot install cloudflared: {e}"));
    }
    #[cfg(target_os = "macos")]
    {
        let (download, exe) = (download.to_path_buf(), exe.to_path_buf());
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            use std::io::Read;
            let file = std::fs::File::open(&download).map_err(|e| e.to_string())?;
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
            for entry in archive.entries().map_err(|e| e.to_string())? {
                let mut entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path().map_err(|e| e.to_string())?;
                if path.file_name().is_some_and(|n| n == "cloudflared")
                    && entry.header().entry_type().is_file()
                {
                    let mut out = std::fs::File::create(&exe).map_err(|e| e.to_string())?;
                    std::io::copy(&mut (&mut entry).take(DOWNLOAD_LIMIT), &mut out)
                        .map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
            Err("cloudflared is missing from the download".into())
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (download, exe);
        Err("Unexpected cloudflared archive on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_tunnel_address_in_cloudflared_output() {
        let line = "2026-10-22T01:00:00Z INF |  https://clever-mint-rocket-sky.trycloudflare.com                       |";
        assert_eq!(
            quick_tunnel_url(line).as_deref(),
            Some("https://clever-mint-rocket-sky.trycloudflare.com")
        );
        for line in [
            "INF Requesting new quick Tunnel on trycloudflare.com...",
            "INF Settings: map[ha-connections:1 quick-service:https://api.trycloudflare.com]",
            "ERR failed to request quick Tunnel",
            "https://evil.example/https://x.trycloudflare.com.evil.example",
        ] {
            assert_eq!(quick_tunnel_url(line), None, "{line}");
        }
    }

    #[test]
    fn release_platforms_have_pinned_packages() {
        for platform in [
            "windows-x86_64",
            "linux-x86_64",
            "linux-aarch64",
            "macos-aarch64",
            "macos-x86_64",
        ] {
            let p = package(platform).unwrap();
            for hash in [p.asset_sha256, p.exe_sha256] {
                assert_eq!(hash.len(), 64);
                assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
            }
            assert_eq!(
                p.asset.ends_with(".tgz"),
                p.asset_sha256 != p.exe_sha256,
                "{platform}"
            );
        }
        assert!(package("freebsd-x86_64").is_none());
    }

    #[tokio::test]
    async fn only_the_pinned_executable_verifies() {
        let dir = std::env::temp_dir().join(format!("mcp-tunnel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("cloudflared");
        std::fs::write(&file, b"hello").unwrap();
        let good = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verified(&file, good).await);
        assert!(!verified(&file, &"0".repeat(64)).await);
        assert!(!verified(&dir.join("missing"), good).await);
        let _ = std::fs::remove_dir_all(dir);
    }
}
