use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::process::Stdio;
use tauri::{AppHandle, Emitter, Manager};

use crate::comfyui_version::{comfyui_version_info, ComfyUiVersionInfo, COMFYUI_REF};
use crate::config;
use crate::error::AppError;
use crate::state::AppState;

/// Directory name GitHub's source archive expands to for [`COMFYUI_REF`].
/// GitHub strips a leading `v` from the tag, e.g. `v0.26.0` -> `ComfyUI-0.26.0`.
fn comfyui_zip_dirname() -> String {
    format!("ComfyUI-{}", COMFYUI_REF.trim_start_matches('v'))
}

#[derive(Clone, serde::Serialize)]
struct SetupProgress {
    step: String,
    message: String,
    percent: u32,
}

#[derive(Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub filename: String,
    pub downloaded: u64,
    pub total: u64,
    pub done: bool,
}

/// Escapes a path for interpolation into a single-quoted PowerShell string
/// literal by doubling embedded single quotes (PowerShell's own escape
/// convention). Needed because Windows usernames/folders can contain an
/// apostrophe (e.g. `C:\Users\Cole's Computer\...`), which would otherwise
/// terminate the quoted string early and corrupt the command.
#[cfg(target_os = "windows")]
fn ps_quote(path: &Path) -> String {
    path.display().to_string().replace('\'', "''")
}

fn emit(app: &AppHandle, step: &str, msg: &str, pct: u32) {
    app.emit(
        "setup:progress",
        SetupProgress {
            step: step.into(),
            message: msg.into(),
            percent: pct,
        },
    )
    .ok();
}

#[derive(Clone, serde::Serialize)]
struct SetupLogPayload {
    text: String,
    is_update: bool,
}

fn emit_log(app: &AppHandle, line: &str) {
    app.emit(
        "setup:log",
        SetupLogPayload {
            text: line.to_string(),
            is_update: false,
        },
    )
    .ok();
}

fn emit_log_update(app: &AppHandle, line: &str) {
    app.emit(
        "setup:log",
        SetupLogPayload {
            text: line.to_string(),
            is_update: true,
        },
    )
    .ok();
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    // Use the same 3-tier resolution as config::app_data_dir():
    // env var > bootstrap pointer > platform default
    if let Some(dir) = config::app_data_dir() {
        return Ok(dir);
    }
    app.path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))
}

fn uv_bin(base: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        base.join("bin").join("uv.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        base.join("bin").join("uv")
    }
}

fn venv_python(base: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        base.join("venv").join("Scripts").join("python.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        base.join("venv").join("bin").join("python")
    }
}

fn uv_download_url() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-unknown-linux-gnu.tar.gz"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "https://github.com/astral-sh/uv/releases/latest/download/uv-aarch64-unknown-linux-gnu.tar.gz"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-apple-darwin.tar.gz"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "https://github.com/astral-sh/uv/releases/latest/download/uv-aarch64-apple-darwin.tar.gz"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-pc-windows-msvc.zip"
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

struct SetupNetworkOpts {
    network_proxy: Option<String>,
    pip_index_url: Option<String>,
}

impl SetupNetworkOpts {
    fn from_options(network_proxy: Option<String>, pip_index_url: Option<String>) -> Self {
        Self {
            network_proxy: network_proxy
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            pip_index_url: pip_index_url
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        }
    }
}

/// Apply CREATE_NO_WINDOW flag on Windows to prevent console popups.
#[cfg(target_os = "windows")]
fn hide_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[allow(unused_imports)]
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x08000000) // CREATE_NO_WINDOW
}

// On Linux, spawned setup tooling (`uv`, the venv's `python`, `nvidia-smi`, ...)
// must not inherit the AppImage's bundled `LD_LIBRARY_PATH`/`LD_PRELOAD` — see
// `crate::comfyui::process::strip_appimage_env` for why that breaks symbol
// resolution against the system's native libraries.
#[cfg(target_os = "linux")]
fn hide_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    crate::comfyui::process::strip_appimage_env(cmd);
    cmd
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn hide_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    cmd // no-op on macOS
}

/// Run a command with hidden window, capturing stdout/stderr and streaming
/// each line to the frontend via `setup:log`.
async fn read_stream_logged<R: tokio::io::AsyncRead + Unpin>(app: AppHandle, stream: R) {
    use tokio::io::AsyncReadExt;
    let mut reader = stream;
    let mut buf = [0u8; 1024];
    let mut line_buf = Vec::new();
    let mut last_was_cr = false;

    loop {
        match reader.read(&mut buf).await {
            Ok(0) => {
                if !line_buf.is_empty() {
                    let s = String::from_utf8_lossy(&line_buf);
                    let trimmed = s.trim_end_matches(&['\r', '\n'][..]);
                    if !trimmed.is_empty() {
                        if last_was_cr {
                            emit_log_update(&app, trimmed);
                        } else {
                            emit_log(&app, trimmed);
                        }
                    }
                }
                break;
            }
            Ok(n) => {
                for i in 0..n {
                    let b = buf[i];
                    if b == b'\n' {
                        if !(last_was_cr && line_buf.is_empty()) {
                            let s = String::from_utf8_lossy(&line_buf);
                            let trimmed = s.trim_end_matches(&['\r', '\n'][..]);
                            if last_was_cr {
                                emit_log_update(&app, trimmed);
                            } else {
                                emit_log(&app, trimmed);
                            }
                        }
                        line_buf.clear();
                        last_was_cr = false;
                    } else if b == b'\r' {
                        let s = String::from_utf8_lossy(&line_buf);
                        let trimmed = s.trim_end_matches(&['\r', '\n'][..]);
                        if !trimmed.is_empty() {
                            if last_was_cr {
                                emit_log_update(&app, trimmed);
                            } else {
                                emit_log(&app, trimmed);
                            }
                        }
                        line_buf.clear();
                        last_was_cr = true;
                    } else {
                        line_buf.push(b);
                    }
                }
            }
            Err(e) => {
                log::warn!("Error reading stream: {}", e);
                break;
            }
        }
    }
}

/// Run a command with hidden window, capturing stdout/stderr and streaming
/// each line to the frontend via `setup:log`.
async fn run_logged(
    app: &AppHandle,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    hide_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", program, e))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let app_out = app.clone();
    let app_err = app.clone();

    let out_task = tokio::spawn(async move {
        if let Some(out) = stdout {
            read_stream_logged(app_out, out).await;
        }
    });

    let err_task = tokio::spawn(async move {
        if let Some(err) = stderr {
            read_stream_logged(app_err, err).await;
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("{} wait failed: {}", program, e))?;

    out_task.await.ok();
    err_task.await.ok();

    if !status.success() {
        return Err(format!("{} exited with status {}", program, status));
    }
    Ok(())
}

async fn run_command_logged(
    app: &AppHandle,
    mut cmd: tokio::process::Command,
) -> Result<(), String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn command: {}", e))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let app_out = app.clone();
    let app_err = app.clone();

    let out_task = tokio::spawn(async move {
        if let Some(out) = stdout {
            read_stream_logged(app_out, out).await;
        }
    });

    let err_task = tokio::spawn(async move {
        if let Some(err) = stderr {
            read_stream_logged(app_err, err).await;
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Command wait failed: {}", e))?;

    out_task.await.ok();
    err_task.await.ok();

    if !status.success() {
        return Err(format!("Command exited with status {}", status));
    }
    Ok(())
}

/// Download a file with streaming progress events.
async fn download_file(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    // Stream into a sibling `.part` file and only rename into place once the
    // transfer completes, so an interrupted download never leaves a truncated
    // file that later code mistakes for a finished one. If a `.part` already
    // exists, try to resume from where it left off via a Range request.
    let part = dest.with_extension(match dest.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{}.part", ext),
        None => "part".to_string(),
    });
    let resume_from = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let mut req = client.get(url);
    if resume_from > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={}-", resume_from));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    use reqwest::StatusCode;
    let status = resp.status();
    // 416 means the server considers the existing `.part` already complete.
    if status == StatusCode::RANGE_NOT_SATISFIABLE && resume_from > 0 {
        std::fs::rename(&part, dest).map_err(|e| format!("Failed to finalize download: {}", e))?;
        app.emit(
            "download:progress",
            DownloadProgress {
                filename: label.to_string(),
                downloaded: resume_from,
                total: resume_from,
                done: true,
            },
        )
        .ok();
        return Ok(());
    }
    if !status.is_success() {
        return Err(format!("Download returned status {}", status));
    }

    use std::io::Write;
    // The server honors the Range request (206) → append; a plain 200 ignores
    // it (or there was nothing to resume) → start the `.part` over from scratch.
    let resuming = status == StatusCode::PARTIAL_CONTENT && resume_from > 0;
    let mut downloaded: u64 = if resuming { resume_from } else { 0 };
    let total = resp.content_length().unwrap_or(0) + downloaded;
    let mut file = if resuming {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&part)
            .map_err(|e| format!("Failed to open partial file: {}", e))?
    } else {
        std::fs::File::create(&part).map_err(|e| format!("Failed to create file: {}", e))?
    };

    // Emit initial progress
    app.emit(
        "download:progress",
        DownloadProgress {
            filename: label.to_string(),
            downloaded,
            total,
            done: false,
        },
    )
    .ok();

    // Stream chunks
    let mut last_emit: u64 = downloaded;
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Download read error: {}", e))?
    {
        file.write_all(&chunk)
            .map_err(|e| format!("Write error: {}", e))?;
        downloaded += chunk.len() as u64;

        // Throttle progress events to ~every 256KB
        if downloaded - last_emit > 256 * 1024 || downloaded == total {
            last_emit = downloaded;
            app.emit(
                "download:progress",
                DownloadProgress {
                    filename: label.to_string(),
                    downloaded,
                    total,
                    done: false,
                },
            )
            .ok();
        }
    }

    // Flush and atomically move the completed file into place.
    file.flush().map_err(|e| format!("Flush error: {}", e))?;
    drop(file);
    std::fs::rename(&part, dest).map_err(|e| format!("Failed to finalize download: {}", e))?;

    app.emit(
        "download:progress",
        DownloadProgress {
            filename: label.to_string(),
            downloaded,
            total,
            done: true,
        },
    )
    .ok();

    Ok(())
}

// ─── Step helpers ───────────────────────────────────────────────────────────

async fn step_download_uv(
    app: &AppHandle,
    base: &Path,
    client: &reqwest::Client,
) -> Result<(), String> {
    let uv = uv_bin(base);
    if uv.exists() {
        return Ok(());
    }
    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    let url = uv_download_url();

    #[cfg(not(target_os = "windows"))]
    {
        let archive = base.join("_uv.tar.gz");
        download_file(app, client, url, &archive, "uv").await?;

        run_logged(
            app,
            "tar",
            &[
                "xzf",
                archive.to_str().unwrap(),
                "--strip-components=1",
                "-C",
                bin_dir.to_str().unwrap(),
            ],
            &[],
        )
        .await
        .map_err(|_| "Failed to extract uv archive".to_string())?;

        // Ensure executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&uv, std::fs::Permissions::from_mode(0o755)).ok();
        }
        std::fs::remove_file(&archive).ok();
    }

    #[cfg(target_os = "windows")]
    {
        let archive = base.join("_uv.zip");
        let temp_dir = base.join("_uv_extract");
        download_file(app, client, url, &archive, "uv").await?;

        let ps_cmd = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force; \
             Get-ChildItem -Path '{}' -Filter 'uv.exe' -Recurse | Select-Object -First 1 | Move-Item -Destination '{}\\uv.exe' -Force; \
             Get-ChildItem -Path '{}' -Filter 'uvx.exe' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1 | Move-Item -Destination '{}\\uvx.exe' -Force",
            ps_quote(&archive),
            ps_quote(&temp_dir),
            ps_quote(&temp_dir),
            ps_quote(&bin_dir),
            ps_quote(&temp_dir),
            ps_quote(&bin_dir),
        );
        run_logged(app, "powershell", &["-NoProfile", "-Command", &ps_cmd], &[])
            .await
            .map_err(|_| "Failed to extract uv archive".to_string())?;

        std::fs::remove_dir_all(&temp_dir).ok();
        std::fs::remove_file(&archive).ok();
    }

    // Verify uv was actually extracted
    if !uv.exists() {
        return Err(format!(
            "uv binary not found at {} after extraction. The download may have failed or the archive structure changed.",
            uv.display()
        ));
    }

    Ok(())
}

async fn step_install_python(
    app: &AppHandle,
    base: &Path,
    python_version: &str,
) -> Result<(), String> {
    let uv = uv_bin(base);
    let python_dir = base.join("python");
    std::fs::create_dir_all(&python_dir).map_err(|e| e.to_string())?;

    // Detect and purge partial installs from a previous interrupted run.
    // uv extracts the CPython standalone into `python/cpython-3.11.x-<triple>/`
    // and then stamps `Lib/EXTERNALLY-MANAGED` at the end.  If the previous run
    // was killed between extraction and stamping — or if AV quarantined files
    // mid-extract — the top-level dir can exist without a proper `Lib/`,
    // causing the next run to fail with "ERROR_PATH_NOT_FOUND" on that stamp.
    // A partial install is detected by the absence of either `python.exe`
    // (Windows) / `bin/python3` (Unix) or `Lib/` / `lib/`.
    if let Ok(entries) = std::fs::read_dir(&python_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("cpython-") {
                continue;
            }
            #[cfg(target_os = "windows")]
            let complete = path.join("python.exe").exists() && path.join("Lib").is_dir();
            #[cfg(not(target_os = "windows"))]
            let complete = path.join("bin").join("python3").exists() && path.join("lib").is_dir();
            if !complete {
                emit_log(
                    app,
                    &format!("Removing incomplete Python install at {}", path.display()),
                );
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }

    let python_dir_str = python_dir.to_string_lossy().to_string();
    let args = ["python", "install", python_version];
    let env = [("UV_PYTHON_INSTALL_DIR", python_dir_str.as_str())];

    // Try once; on failure, fall back to `--reinstall` which forces uv to
    // purge and re-extract the CPython archive (handles the rarer case where
    // our heuristic above didn't match but the install dir is still broken).
    if run_logged(app, uv.to_str().unwrap(), &args, &env)
        .await
        .is_ok()
    {
        return Ok(());
    }
    emit_log(app, "Python install failed; retrying with --reinstall...");
    run_logged(
        app,
        uv.to_str().unwrap(),
        &["python", "install", "--reinstall", python_version],
        &env,
    )
    .await
    .map_err(|_| format!("Failed to install Python {}", python_version))
}

async fn step_download_comfyui(
    app: &AppHandle,
    base: &Path,
    client: &reqwest::Client,
) -> Result<(), String> {
    let comfyui_dir = base.join("comfyui");
    if comfyui_dir.join("main.py").exists() {
        return Ok(());
    }

    // Try git clone first (most systems have git). Pin to the tested-good tag so
    // installs are reproducible instead of tracking a moving `master`.
    let git_result = run_logged(
        app,
        "git",
        &[
            "clone",
            "--depth=1",
            "--branch",
            COMFYUI_REF,
            "https://github.com/comfyanonymous/ComfyUI.git",
            comfyui_dir.to_str().unwrap(),
        ],
        &[],
    )
    .await;

    if git_result.is_ok() {
        return Ok(());
    }

    // Fallback: download the pinned tag's source zip
    emit_log(app, "Git clone failed, falling back to zip download...");
    let zip_url = format!(
        "https://github.com/comfyanonymous/ComfyUI/archive/refs/tags/{}.zip",
        COMFYUI_REF
    );
    let zip_path = base.join("_comfyui.zip");
    download_file(app, client, &zip_url, &zip_path, "ComfyUI").await?;

    #[cfg(not(target_os = "windows"))]
    {
        run_logged(
            app,
            "unzip",
            &[
                "-q",
                zip_path.to_str().unwrap(),
                "-d",
                base.to_str().unwrap(),
            ],
            &[],
        )
        .await
        .map_err(|_| "Failed to extract ComfyUI".to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        let ps = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            ps_quote(&zip_path),
            ps_quote(base)
        );
        run_logged(app, "powershell", &["-Command", &ps], &[])
            .await
            .map_err(|_| "Failed to extract ComfyUI".to_string())?;
    }

    // A previous interrupted attempt may have left a partial `comfyui` dir.
    // Renaming onto an existing directory fails on Windows (and on a non-empty
    // dir on Unix), so clear any stale target before moving the freshly
    // extracted tree into place.
    if comfyui_dir.exists() {
        std::fs::remove_dir_all(&comfyui_dir)
            .map_err(|e| format!("Failed to clear stale ComfyUI dir: {}", e))?;
    }
    std::fs::rename(base.join(comfyui_zip_dirname()), &comfyui_dir)
        .map_err(|e| format!("Failed to rename ComfyUI dir: {}", e))?;
    std::fs::remove_file(&zip_path).ok();
    Ok(())
}

async fn step_create_venv(
    app: &AppHandle,
    base: &Path,
    python_version: &str,
) -> Result<(), String> {
    let uv = uv_bin(base);
    let venv_dir = base.join("venv");
    let python_dir = base.join("python");

    let python_dir_str = python_dir.to_string_lossy().to_string();
    run_logged(
        app,
        uv.to_str().unwrap(),
        &[
            "venv",
            venv_dir.to_str().unwrap(),
            "--python",
            python_version,
            "--allow-existing",
        ],
        &[("UV_PYTHON_INSTALL_DIR", &python_dir_str)],
    )
    .await
    .map_err(|_| {
        // The most common cause on Windows is a Python process still holding
        // `venv\Scripts\python.exe` open, which uv reports as "Access is denied.
        // (os error 5)". Setup stops its own ComfyUI first, so a survivor here is
        // either a keep_alive process or one started outside MooshieUI.
        "Failed to create virtual environment. If the log above says \"Access is denied\", \
         a Python process is still using the environment — close any running ComfyUI \
         (including one left running by the keep-alive setting) and run setup again."
            .to_string()
    })
}

async fn detect_gpu_type() -> String {
    #[cfg(target_os = "macos")]
    {
        log::info!("GPU detection: macOS detected, using MPS");
        return "mps".to_string();
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Try nvidia-smi from PATH first
        let nvidia_result = {
            let mut cmd = tokio::process::Command::new("nvidia-smi");
            hide_window(&mut cmd);
            cmd.output().await
        };
        match &nvidia_result {
            Ok(output) if output.status.success() => {
                log::info!("GPU detection: NVIDIA GPU found via nvidia-smi");
                return "nvidia".to_string();
            }
            Ok(output) => {
                log::info!("GPU detection: nvidia-smi exited with {}", output.status);
            }
            Err(e) => {
                log::info!("GPU detection: nvidia-smi not found in PATH: {}", e);
            }
        }

        // Windows: nvidia-smi may not be in PATH but exists at a known location.
        // Also try WMI as a secondary check for NVIDIA GPUs.
        #[cfg(target_os = "windows")]
        {
            // Try common nvidia-smi locations
            let nvidia_paths = [
                r"C:\Windows\System32\nvidia-smi.exe",
                r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe",
            ];
            for path in &nvidia_paths {
                if std::path::Path::new(path).exists() {
                    let mut cmd = tokio::process::Command::new(path);
                    hide_window(&mut cmd);
                    if let Ok(output) = cmd.output().await {
                        if output.status.success() {
                            log::info!("GPU detection: NVIDIA GPU found via {}", path);
                            return "nvidia".to_string();
                        }
                    }
                }
            }

            // WMI fallback: check video controller names for NVIDIA GPUs
            let mut cmd = tokio::process::Command::new("powershell");
            cmd.args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
            ]);
            hide_window(&mut cmd);
            if let Ok(output) = cmd.output().await {
                let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
                log::info!("GPU detection: WMI video controllers: {}", text.trim());
                if text.contains("nvidia")
                    || text.contains("geforce")
                    || text.contains("rtx")
                    || text.contains("gtx")
                    || text.contains("quadro")
                {
                    log::info!("GPU detection: NVIDIA GPU found via WMI");
                    return "nvidia".to_string();
                }
                // Only match discrete AMD GPUs (RX series) — not integrated Radeon on Ryzen APUs.
                // Integrated GPUs report as "AMD Radeon Graphics" or "AMD Radeon Vega X Graphics"
                // and don't support ROCm. Discrete GPUs have "RX" in the name (RX 7900, RX 6800, etc.)
                // "radeon ai pro" catches the R9700, whose product name doesn't contain "rx" or
                // "pro w" (it's ROCm-on-Windows-eligible -- see amd_windows_rocm_model_supported).
                if text.contains("radeon rx")
                    || text.contains("radeon pro w")
                    || text.contains("radeon ai pro")
                {
                    log::info!("GPU detection: AMD discrete GPU found via WMI");
                    return "amd".to_string();
                }
                // Intel Arc discrete GPUs (A770, A750, B580, etc.)
                if text.contains("intel arc") || text.contains("arc a") || text.contains("arc b") {
                    log::info!("GPU detection: Intel Arc GPU found via WMI");
                    return "intel".to_string();
                }
            }
        }

        let rocm_result = {
            let mut cmd = tokio::process::Command::new("rocm-smi");
            hide_window(&mut cmd);
            cmd.output().await
        };
        if let Ok(output) = rocm_result {
            if output.status.success() {
                log::info!("GPU detection: AMD GPU found via rocm-smi");
                return "amd".to_string();
            }
        }

        #[cfg(target_os = "linux")]
        if Path::new("/opt/rocm").exists() {
            log::info!("GPU detection: AMD GPU found via /opt/rocm");
            return "amd".to_string();
        }
        // Linux: detect discrete AMD GPUs via sysfs even when ROCm tooling isn't
        // installed. Arch-based distros (e.g. Garuda) ship amdgpu + Mesa but not ROCm,
        // so rocm-smi / /opt/rocm are absent and the GPU would otherwise be misdetected
        // as CPU. detect_amd_gpu_arch() reads /sys/class/drm and is multi-GPU aware: it
        // identifies a discrete RDNA 2/3/4 (or newer) card by GC version / device ID,
        // gated on dedicated VRAM size, and ignores integrated Radeon graphics, so this
        // won't false-positive on a Ryzen APU's iGPU.
        #[cfg(target_os = "linux")]
        if let Some(arch) = detect_amd_gpu_arch().await {
            log::info!("GPU detection: AMD GPU found via sysfs (arch {})", arch);
            return "amd".to_string();
        }
        // Linux: check for Intel Arc discrete GPU via sysfs
        #[cfg(target_os = "linux")]
        {
            if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
                for entry in entries.flatten() {
                    let vendor_path = entry.path().join("device/vendor");
                    if let Ok(vendor) = std::fs::read_to_string(&vendor_path) {
                        // Intel PCI vendor ID is 0x8086
                        if vendor.trim() == "0x8086" {
                            // Check if it's a VGA-class device (0x0300). Every Intel CPU
                            // since ~2011 has an integrated GPU that also reports vendor
                            // 0x8086 / class 0x0300, so this alone can't distinguish a
                            // discrete Arc card from a CPU's iGPU.
                            let class_path = entry.path().join("device/class");
                            let is_vga = std::fs::read_to_string(&class_path)
                                .is_ok_and(|c| c.trim().starts_with("0x0300"));
                            if !is_vga {
                                continue;
                            }
                            // Gate on dedicated VRAM, like the AMD discrete check above.
                            // Only cards with local memory (Arc's i915 DG2 path or the
                            // newer xe driver) expose these sysfs files at all -- an
                            // integrated GPU has neither, so this excludes it rather
                            // than reading a real "0".
                            let device_dir = entry.path().join("device");
                            if device_vram_bytes(&device_dir).unwrap_or(0) > 0 {
                                log::info!("GPU detection: Intel Arc GPU found via sysfs");
                                return "intel".to_string();
                            }
                        }
                    }
                }
            }
        }
        log::warn!("GPU detection: No discrete GPU detected, falling back to CPU");
        "cpu".to_string()
    }
}

/// AMD's ROCm-on-Windows PyTorch preview (7.2.1) supports only an explicit
/// hardware allowlist -- unlike Linux ROCm, it isn't "any RDNA3/4 card".
/// See: https://rocm.docs.amd.com/projects/radeon-ryzen/en/docs-7.2.1/docs/compatibility/compatibilityryz/native_windows/windows_compatibility.html
/// RX 7700 XT is explicitly NOT supported even though plain RX 7700 is, so
/// the XT variant is checked first to keep the shorter "rx 7700" substring
/// from false-matching it.
fn amd_windows_rocm_model_supported(name_lower: &str) -> bool {
    if name_lower.contains("rx 7700 xt") {
        return false;
    }
    const SUPPORTED_MARKERS: [&str; 6] = [
        "rx 9070",             // covers "RX 9070" and "RX 9070 XT"
        "rx 9060 xt",          // RX 9060 XT
        "radeon ai pro r9700", // Radeon AI PRO R9700
        "rx 7900 xtx",         // RX 7900 XTX
        "pro w7900",           // covers "PRO W7900" and "PRO W7900 Dual Slot"
        "rx 7700",             // RX 7700 (not XT, excluded above)
    ];
    SUPPORTED_MARKERS.iter().any(|m| name_lower.contains(m))
}

/// Windows-only: query WMI for the GPU name and check it against AMD's ROCm
/// preview allowlist. Only meaningful when `detect_gpu_type()` already
/// returned "amd" -- this narrows that down to models the Windows preview
/// actually supports (see `amd_windows_rocm_model_supported`).
#[cfg(target_os = "windows")]
async fn detect_amd_windows_rocm_supported() -> bool {
    let mut cmd = tokio::process::Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
    ]);
    hide_window(&mut cmd);
    let Ok(output) = cmd.output().await else {
        return false;
    };
    let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let eligible = amd_windows_rocm_model_supported(&text);
    if eligible {
        log::info!("GPU detection: AMD GPU is eligible for the ROCm-on-Windows PyTorch preview");
    } else {
        log::info!(
            "GPU detection: AMD GPU is not on the ROCm-on-Windows preview allowlist \u{2014} CPU fallback"
        );
    }
    eligible
}

#[cfg(not(target_os = "windows"))]
async fn detect_amd_windows_rocm_supported() -> bool {
    false
}

// AMD's ROCm-on-Windows PyTorch preview ships as direct wheel/tarball
// downloads from repo.radeon.com rather than a pip index, and the SDK and
// PyTorch wheels are versioned in lockstep. These URLs are pinned to ROCm
// 7.2.1 / torch 2.9.1 / torchvision 0.24.1 and WILL go stale as AMD ships
// new releases -- check
// https://rocm.docs.amd.com/projects/radeon-ryzen/en/latest/docs/install/installryz/windows/install-pytorch.html
// for the current version before bumping.
#[cfg(target_os = "windows")]
const AMD_ROCM_WINDOWS_SDK_WHEELS: [&str; 4] = [
    "https://repo.radeon.com/rocm/windows/rocm-rel-7.2.1/rocm_sdk_core-7.2.1-py3-none-win_amd64.whl",
    "https://repo.radeon.com/rocm/windows/rocm-rel-7.2.1/rocm_sdk_devel-7.2.1-py3-none-win_amd64.whl",
    "https://repo.radeon.com/rocm/windows/rocm-rel-7.2.1/rocm_sdk_libraries_custom-7.2.1-py3-none-win_amd64.whl",
    "https://repo.radeon.com/rocm/windows/rocm-rel-7.2.1/rocm-7.2.1.tar.gz",
];
#[cfg(target_os = "windows")]
const AMD_ROCM_WINDOWS_TORCH_WHEELS: [&str; 3] = [
    "https://repo.radeon.com/rocm/windows/rocm-rel-7.2.1/torch-2.9.1%2Brocm7.2.1-cp312-cp312-win_amd64.whl",
    "https://repo.radeon.com/rocm/windows/rocm-rel-7.2.1/torchaudio-2.9.1%2Brocm7.2.1-cp312-cp312-win_amd64.whl",
    "https://repo.radeon.com/rocm/windows/rocm-rel-7.2.1/torchvision-0.24.1%2Brocm7.2.1-cp312-cp312-win_amd64.whl",
];

/// Install AMD's ROCm-on-Windows PyTorch preview: SDK components first, then
/// the torch/torchvision/torchaudio wheels, per AMD's documented two-stage
/// install. Requires a Python 3.12 venv (the wheels are cp312-only) and a
/// model on `amd_windows_rocm_model_supported`'s allowlist -- callers must
/// check both before calling this.
#[cfg(target_os = "windows")]
async fn install_amd_windows_rocm_pytorch(
    app: &AppHandle,
    base: &Path,
    net: &SetupNetworkOpts,
) -> Result<(), String> {
    emit_log(
        app,
        "Installing AMD ROCm 7.2.1 SDK components (Windows preview)...",
    );
    let mut sdk_args: Vec<&str> = AMD_ROCM_WINDOWS_SDK_WHEELS.to_vec();
    sdk_args.push("--reinstall");
    uv_pip(app, base, &sdk_args, net, false).await?;

    emit_log(app, "Installing PyTorch (ROCm 7.2.1) wheels...");
    let mut torch_args: Vec<&str> = AMD_ROCM_WINDOWS_TORCH_WHEELS.to_vec();
    torch_args.push("--reinstall");
    uv_pip(app, base, &torch_args, net, false).await
}

#[cfg(not(target_os = "windows"))]
async fn install_amd_windows_rocm_pytorch(
    _app: &AppHandle,
    _base: &Path,
    _net: &SetupNetworkOpts,
) -> Result<(), String> {
    Err("AMD ROCm Windows preview install invoked on a non-Windows platform".to_string())
}

/// Read the Python major.minor a venv was created with, from its
/// `pyvenv.cfg` (`version = 3.12.7` or `version_info = 3.12.7`). `None` if
/// the venv or file is missing or unparseable.
fn venv_python_version(venv_dir: &Path) -> Option<String> {
    let cfg = std::fs::read_to_string(venv_dir.join("pyvenv.cfg")).ok()?;
    for line in cfg.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "version" || key.trim() == "version_info" {
            let mut parts = value.trim().splitn(3, '.');
            if let (Some(major), Some(minor)) = (parts.next(), parts.next()) {
                return Some(format!("{}.{}", major, minor));
            }
        }
    }
    None
}

async fn uv_pip(
    app: &AppHandle,
    base: &Path,
    args: &[&str],
    net: &SetupNetworkOpts,
    apply_pip_mirror: bool,
) -> Result<(), String> {
    let uv = uv_bin(base);
    let python = venv_python(base);
    let python_dir = base.join("python");

    let python_str = python.to_string_lossy().to_string();
    let python_dir_str = python_dir.to_string_lossy().to_string();

    let mut cmd = tokio::process::Command::new(uv);
    cmd.arg("pip")
        .arg("install")
        .arg("--python")
        .arg(&python_str)
        .args(args)
        .env("UV_PYTHON_INSTALL_DIR", &python_dir_str);

    if apply_pip_mirror {
        crate::comfyui::nodes::apply_pip_install_options(
            &mut cmd,
            true,
            net.network_proxy.as_deref(),
            net.pip_index_url.as_deref(),
        );
    } else {
        crate::comfyui::nodes::apply_network_proxy(&mut cmd, net.network_proxy.as_deref());
    }

    run_command_logged(app, cmd)
        .await
        .map_err(|_| format!("pip install failed for: {}", args.join(" ")))
}

/// Map an amdgpu Graphics Core (GC) IP major version (read from
/// `/sys/.../device/ip_discovery/die/0/GC/0/major`) to a representative gfx
/// architecture string. Covers the discrete AMD GPU families MooshieUI installs
/// ROCm PyTorch for — RDNA 4/3/2 and GCN5/CDNA. Returns None for unknown majors.
#[cfg(target_os = "linux")]
fn gfx_arch_from_gc_major(major: &str) -> Option<&'static str> {
    match major.trim() {
        "12" => Some("gfx1200"), // RDNA 4  (RX 9000 series)
        "11" => Some("gfx1100"), // RDNA 3  (RX 7000 series)
        "10" => Some("gfx1030"), // RDNA 2  (RX 6000 series)
        "9" => Some("gfx900"),   // GCN 5 / Vega, CDNA
        _ => None,
    }
}

/// Detect the AMD GPU architecture string (e.g. "gfx1100", "gfx1201") by reading
/// sysfs on Linux. When multiple AMD GPUs are present (e.g. integrated + discrete),
/// prefers the highest / most capable architecture. Returns None if detection fails
/// or no AMD GPU is found. Integrated Ryzen APU iGPUs are excluded via a dedicated
/// VRAM gate so they aren't misclassified as ROCm-capable discrete cards.
#[cfg(target_os = "linux")]
async fn detect_amd_gpu_arch() -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();

    // Try rocm-smi first (most reliable if ROCm is installed).
    // Collect ALL gfx versions — multi-GPU systems list several.
    let mut cmd = tokio::process::Command::new("rocm-smi");
    cmd.args(["--showproductname"]);
    hide_window(&mut cmd);
    if let Ok(output) = cmd.output().await {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
            for word in text.split_whitespace() {
                if word.starts_with("gfx") && !candidates.contains(&word.to_string()) {
                    candidates.push(word.to_string());
                }
            }
        }
    }

    // Fallback: read the GPU firmware version from sysfs to determine architecture.
    // Iterate ALL drm cards and collect every architecture we find.
    if candidates.is_empty() {
        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let vendor_path = entry.path().join("device/vendor");

                // Verify this is an AMD GPU (vendor 0x1002)
                if let Ok(vendor) = std::fs::read_to_string(&vendor_path) {
                    if !vendor.trim().contains("0x1002") {
                        continue;
                    }
                } else {
                    continue;
                }

                // Skip integrated APU iGPUs. A Ryzen APU's integrated Radeon
                // shares vendor 0x1002 and reports a GC major (10/11) just like a
                // discrete RDNA card, but it has almost no dedicated VRAM and is
                // not ROCm-supported. Only cards with substantial dedicated VRAM
                // are treated as discrete ROCm candidates: discrete GPUs ship with
                // GBs of VRAM (>= 4 GB even on entry models) while APU carveouts
                // default to a few hundred MB.
                let vram_mb =
                    std::fs::read_to_string(entry.path().join("device/mem_info_vram_total"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .map(|bytes| bytes / (1024 * 1024))
                        .unwrap_or(0);
                if vram_mb < 3072 {
                    continue;
                }

                // Try ip_discovery first — it gives the GC major version directly,
                // which maps cleanly to a gfx architecture across RDNA generations.
                let ip_path = entry.path().join("device/ip_discovery/die/0/GC/0/major");
                if let Ok(major) = std::fs::read_to_string(&ip_path) {
                    if let Some(arch) = gfx_arch_from_gc_major(&major) {
                        let arch = arch.to_string();
                        if !candidates.contains(&arch) {
                            candidates.push(arch);
                        }
                        continue;
                    }
                }

                // Fallback: infer architecture from the device ID when
                // ip_discovery is unavailable (older kernels).
                let device_path = entry.path().join("device/device");
                if let Ok(device_id) = std::fs::read_to_string(&device_path) {
                    let device_id = device_id.trim().to_lowercase();
                    // RDNA 4 (RX 9070 series) device IDs start with 0x75xx
                    if device_id.starts_with("0x75") {
                        let arch = "gfx1201".to_string();
                        if !candidates.contains(&arch) {
                            candidates.push(arch);
                        }
                    }
                }
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    // Prefer the most capable architecture: gfx120X (RDNA 4) takes priority
    if let Some(rdna4) = candidates.iter().find(|a| a.starts_with("gfx120")) {
        return Some(rdna4.clone());
    }

    // Otherwise return the first candidate
    Some(candidates.remove(0))
}

/// Check if the AMD GPU requires nightly ROCm builds (gfx120X = RDNA 4).
/// Returns the appropriate PyTorch index URL for AMD GPUs.
#[cfg(target_os = "linux")]
async fn amd_pytorch_index_url() -> &'static str {
    if let Some(arch) = detect_amd_gpu_arch().await {
        if arch.starts_with("gfx120") {
            log::info!(
                "Detected AMD {} (RDNA 4) — using ROCm nightly index for gfx120X",
                arch
            );
            return "https://rocm.nightlies.amd.com/v2/gfx120X-all/";
        }
        log::info!("Detected AMD {} — using stable ROCm 6.2 index", arch);
    }
    "https://download.pytorch.org/whl/rocm6.2"
}

#[cfg(target_os = "windows")]
async fn amd_pytorch_index_url() -> &'static str {
    // ROCm wheels are Linux-only. On Windows there are no AMD GPU-accelerated
    // PyTorch wheels, so we fall back to the CPU index.
    log::warn!(
        "AMD ROCm is not available on Windows — PyTorch will be installed without GPU acceleration"
    );
    "https://download.pytorch.org/whl/cpu"
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn amd_pytorch_index_url() -> &'static str {
    // ROCm wheels are Linux-only.
    log::warn!("AMD ROCm is not available on this platform — PyTorch will be installed without GPU acceleration");
    "https://download.pytorch.org/whl/cpu"
}

/// "8.9" → (8, 9). `None` for anything that is not a numeric major.minor pair.
fn parse_major_minor(s: &str) -> Option<(u32, u32)> {
    let (major, minor) = s.trim().split_once('.')?;
    Some((major.trim().parse().ok()?, minor.trim().parse().ok()?))
}

/// Pick the correct PyTorch CUDA wheel index for NVIDIA GPUs.
///
/// cu130 is the default for every GPU CUDA 13 still supports, and not only for
/// Blackwell. `comfy-kitchen` — which provides the int4/int8 convrot kernels —
/// enables its optimized CUDA and Triton backends only on a torch built against
/// CUDA 13.0 or newer. On an older build it logs "You need pytorch with cu130 or
/// higher to use optimized CUDA operations", marks both backends disabled, and
/// leaves only eager, where a quantized checkpoint generates *slower* than its
/// unquantized base.
///
/// CUDA 13 dropped Maxwell, Pascal and Volta, so compute capability below 7.5
/// stays on cu128.
async fn nvidia_pytorch_index_url() -> &'static str {
    const CU130: &str = "https://download.pytorch.org/whl/cu130";
    const CU128: &str = "https://download.pytorch.org/whl/cu128";

    let output = {
        let mut cmd = tokio::process::Command::new("nvidia-smi");
        cmd.args(["--query-gpu=compute_cap", "--format=csv,noheader,nounits"]);
        hide_window(&mut cmd);
        cmd.output().await
    };

    if let Ok(o) = output {
        if o.status.success() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // One venv serves every worker, so the oldest card decides.
            let oldest = stdout.lines().filter_map(parse_major_minor).min();
            if let Some(cap) = oldest {
                if cap < (7, 5) {
                    log::info!(
                        "Compute capability {}.{} predates CUDA 13 — using cu128 PyTorch index",
                        cap.0,
                        cap.1
                    );
                    return CU128;
                }
                log::info!(
                    "Compute capability {}.{} — using cu130 PyTorch index",
                    cap.0,
                    cap.1
                );
                return CU130;
            }
        }
    }
    // Without a compute capability to check, cu128 is the safer default: it
    // supports strictly more hardware than cu130.
    log::warn!("Could not read compute capability from nvidia-smi — using cu128 PyTorch index");
    CU128
}

async fn step_install_pytorch(
    app: &AppHandle,
    base: &Path,
    gpu: &str,
    amd_windows_rocm: bool,
    net: &SetupNetworkOpts,
) -> Result<(), String> {
    // Spawn a heartbeat so the user sees activity during the long, silent download.
    // uv produces no line output while downloading multi-GB CUDA wheels.
    let app_hb = app.clone();
    let heartbeat = tokio::spawn(async move {
        let mut elapsed = 0u64;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            elapsed += 30;
            let mins = elapsed / 60;
            let secs = elapsed % 60;
            let msg = if elapsed < 60 {
                format!(
                    "[{}s] Still working \u{2014} downloading PyTorch packages...",
                    secs
                )
            } else {
                format!(
                    "[{}m {}s] Still working \u{2014} large GPU packages are downloading...",
                    mins, secs
                )
            };
            emit_log(&app_hb, &msg);
        }
    });

    let result = match gpu {
        "nvidia" => {
            let index_url = nvidia_pytorch_index_url().await;
            emit_log(app, &format!("Using PyTorch index: {}", index_url));
            uv_pip(
                app,
                base,
                &[
                    "torch",
                    "torchvision",
                    "torchaudio",
                    "--index-url",
                    index_url,
                ],
                net,
                false,
            )
            .await
        }
        "amd" if amd_windows_rocm => {
            emit_log(app, "Using AMD's ROCm-on-Windows PyTorch preview (7.2.1)");
            install_amd_windows_rocm_pytorch(app, base, net).await
        }
        "amd" => {
            let index_url = amd_pytorch_index_url().await;
            emit_log(app, &format!("Using PyTorch index: {}", index_url));
            // Linux always gets real ROCm wheels from amd_pytorch_index_url() above,
            // so this CPU-only warning only applies where that function's cfg-gated
            // Windows/other-platform variants fall back to the CPU index — i.e. here,
            // on Windows, only when the GPU isn't on the ROCm preview's allowlist
            // (amd_windows_rocm is false, see amd_windows_rocm_model_supported).
            #[cfg(not(target_os = "linux"))]
            emit_log(
                app,
                "⚠ WARNING: this AMD GPU doesn't support GPU acceleration here. \
                 PyTorch will be installed in CPU-only mode. AMD ROCm works on Linux \
                 for most discrete cards, or on Windows via AMD's ROCm preview for a \
                 narrow set of RDNA3/4 models.",
            );
            uv_pip(
                app,
                base,
                &[
                    "torch",
                    "torchvision",
                    "torchaudio",
                    "--index-url",
                    index_url,
                    "--extra-index-url",
                    "https://pypi.org/simple/",
                ],
                net,
                false,
            )
            .await
        }
        "intel" => {
            uv_pip(
                app,
                base,
                &[
                    "torch",
                    "torchvision",
                    "torchaudio",
                    "--index-url",
                    "https://download.pytorch.org/whl/xpu",
                    "--extra-index-url",
                    "https://pypi.org/simple/",
                ],
                net,
                false,
            )
            .await
        }
        "mps" => {
            uv_pip(
                app,
                base,
                &["torch", "torchvision", "torchaudio"],
                net,
                false,
            )
            .await
        }
        _ => {
            uv_pip(
                app,
                base,
                &[
                    "torch",
                    "torchvision",
                    "torchaudio",
                    "--index-url",
                    "https://download.pytorch.org/whl/cpu",
                    "--extra-index-url",
                    "https://pypi.org/simple/",
                ],
                net,
                false,
            )
            .await
        }
    };

    heartbeat.abort();
    result
}

async fn step_install_deps(
    app: &AppHandle,
    base: &Path,
    net: &SetupNetworkOpts,
) -> Result<(), String> {
    let requirements = base.join("comfyui").join("requirements.txt");
    let uv = uv_bin(base);
    let python = venv_python(base);
    let python_dir = base.join("python");

    let python_str = python.to_string_lossy().to_string();
    let python_dir_str = python_dir.to_string_lossy().to_string();
    let req_str = requirements.to_string_lossy().to_string();

    let mut cmd = tokio::process::Command::new(uv);
    cmd.arg("pip")
        .arg("install")
        .arg("--python")
        .arg(&python_str)
        .arg("-r")
        .arg(&req_str)
        .env("UV_PYTHON_INSTALL_DIR", &python_dir_str);
    crate::comfyui::nodes::apply_pip_install_options(
        &mut cmd,
        true,
        net.network_proxy.as_deref(),
        net.pip_index_url.as_deref(),
    );

    run_command_logged(app, cmd)
        .await
        .map_err(|_| "Failed to install ComfyUI dependencies".to_string())
}

/// True when the venv's torch is a CUDA build (`torch.version.cuda` is set).
/// Used to decide whether `triton-windows` is worth installing — no GPU vendor is
/// persisted in config, so the venv itself is the source of truth on update runs.
async fn venv_torch_is_cuda(base: &Path) -> bool {
    let mut cmd = tokio::process::Command::new(venv_python(base));
    cmd.arg("-c")
        .arg("import torch,sys; sys.exit(0 if torch.version.cuda else 1)");
    hide_window(&mut cmd);
    matches!(cmd.output().await, Ok(out) if out.status.success())
}

/// The CUDA version the venv's torch was built against, e.g. `(13, 0)`.
/// `None` when torch is missing, broken, or a CPU build.
async fn venv_torch_cuda_version(base: &Path) -> Option<(u32, u32)> {
    let mut cmd = tokio::process::Command::new(venv_python(base));
    cmd.arg("-c")
        .arg("import torch; print(torch.version.cuda or '')");
    hide_window(&mut cmd);
    let out = cmd.output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    parse_major_minor(&String::from_utf8_lossy(&out.stdout))
}

/// Ensure `triton-windows` is present on Windows + CUDA installs.
///
/// The convrot quantization kernels in `comfy-kitchen` (pinned by ComfyUI's own
/// requirements.txt) pick a backend in the order hip → cuda → triton → eager.
/// When the prebuilt CUDA kernels can't load, triton is the last fast path; without
/// it comfy-kitchen silently degrades to pure-PyTorch eager and a quantized model
/// generates *slower* than its unquantized base. Linux CUDA torch wheels bundle
/// triton; Windows torch does not, and upstream triton publishes no Windows wheels.
///
/// Non-fatal by design: eager still produces correct images, so a failure here is a
/// speed warning, never a blocked install.
async fn ensure_quant_kernel_backend(app: &AppHandle, base: &Path, net: &SetupNetworkOpts) {
    if !cfg!(target_os = "windows") {
        return;
    }
    if !venv_torch_is_cuda(base).await {
        return;
    }
    emit_log(app, "Ensuring Triton (fast quantization kernels)...");
    if let Err(e) = uv_pip(app, base, &["triton-windows"], net, true).await {
        emit_log(
            app,
            &format!(
                "Warning: could not install triton-windows ({}). Quantized (convrot) models will still work but run slower.",
                e
            ),
        );
    }
}

/// Verify the quantization kernel package imports in the venv.
///
/// `comfy-kitchen` is pinned by ComfyUI's requirements.txt and provides the
/// int4/int8 convrot kernels. If it is missing or broken, quantized checkpoints fail
/// at generate time with an opaque `convrot_w4a4` error; surfacing it here points at
/// the real cause. Non-fatal: unquantized models don't need it.
async fn verify_quant_stack(app: &AppHandle, base: &Path) {
    let mut cmd = tokio::process::Command::new(venv_python(base));
    cmd.arg("-c").arg("import comfy_kitchen");
    hide_window(&mut cmd);
    match cmd.output().await {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            emit_log(
                app,
                &format!(
                    "Warning: comfy_kitchen could not be imported ({}). Quantized (convrot) models will not load.",
                    stderr.trim()
                ),
            );
        }
        Err(e) => emit_log(
            app,
            &format!("Warning: could not check the quantization kernels: {}", e),
        ),
    }

    // comfy-kitchen enables its optimized CUDA and Triton backends only on a torch
    // built against CUDA 13.0 or newer, and drops to eager below that — where a
    // quantized checkpoint is slower than its unquantized base. New installs get
    // cu130 (see `nvidia_pytorch_index_url`), but an existing venv keeps whatever
    // build it was created with until PyTorch is reinstalled. Only advise re-running
    // setup when setup would actually pick a newer build for this GPU; on hardware
    // CUDA 13 dropped, cu128 is the best available and the advice would be a loop.
    if let Some((major, minor)) = venv_torch_cuda_version(base).await {
        if major < 13 && nvidia_pytorch_index_url().await.ends_with("cu130") {
            emit_log(
                app,
                &format!(
                    "Warning: this environment's PyTorch is built against CUDA {}.{}. Quantized \
                     (convrot) models will still generate correctly, but comfy-kitchen needs CUDA \
                     13.0 or newer for its fast kernels and falls back to slow eager ones below \
                     that. Re-run setup to rebuild the environment on the CUDA 13 build.",
                    major, minor
                ),
            );
        }
    }

    // Installing triton-windows is not enough on its own: its JIT needs a
    // C compiler at runtime, so a package that pip-installed cleanly can still
    // fail to import. Check the import, not just the install.
    if cfg!(target_os = "windows") && venv_torch_is_cuda(base).await {
        let mut cmd = tokio::process::Command::new(venv_python(base));
        cmd.arg("-c").arg("import triton");
        hide_window(&mut cmd);
        if !matches!(cmd.output().await, Ok(out) if out.status.success()) {
            emit_log(
                app,
                "Warning: Triton could not be imported. Quantized (convrot) models will still \
                 generate correctly, but fall back to slow eager kernels and can be slower than \
                 the unquantized model.",
            );
        }
    }
}

fn step_install_custom_nodes(base: &Path) -> Result<(), String> {
    let comfyui = base.join("comfyui");
    // Install into custom_nodes/ — ComfyUI auto-discovers all .py files here
    // and supports the comfy_entrypoint extension API used by our node.
    let custom_nodes = comfyui.join("custom_nodes");
    let blueprints = comfyui.join("blueprints");
    std::fs::create_dir_all(&custom_nodes).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&blueprints).map_err(|e| e.to_string())?;

    // Embedded at compile time from comfyui-nodes/ directory
    let node_py = include_str!("../../comfyui-nodes/nodes_tiled_diffusion.py");
    let blueprint = include_str!("../../comfyui-nodes/Image Tiled Upscale (img2img).json");

    std::fs::write(custom_nodes.join("nodes_tiled_diffusion.py"), node_py)
        .map_err(|e| format!("Failed to write node: {}", e))?;
    std::fs::write(
        blueprints.join("Image Tiled Upscale (img2img).json"),
        blueprint,
    )
    .map_err(|e| format!("Failed to write blueprint: {}", e))?;

    Ok(())
}

/// Verify that a freshly-installed attention backend can actually be imported.
/// A pip "success" that can't `import` (missing triton, CUDA mismatch) must be
/// treated as a failure so the caller can fall back to the default backend.
async fn verify_attention_import(base: &Path, import_name: &str) -> Result<(), String> {
    let python = venv_python(base);
    let mut cmd = tokio::process::Command::new(python);
    cmd.arg("-c").arg(format!("import {}", import_name));
    hide_window(&mut cmd);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run python: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "could not import {} after install: {}",
            import_name,
            stderr.trim()
        ))
    }
}

/// Install an optional attention backend package into the venv, then verify it
/// imports. On Windows the SageAttention backends also install `triton-windows`
/// (Windows torch doesn't bundle triton and official triton has no Windows wheels).
async fn step_install_attention_backend(
    app: &AppHandle,
    base: &Path,
    backend: &str,
    net: &SetupNetworkOpts,
    http: &reqwest::Client,
) -> Result<(), String> {
    let is_windows = cfg!(target_os = "windows");
    match backend {
        "sage_v1" => {
            emit_log(app, "Installing SageAttention v1...");
            let mut pkgs = vec!["sageattention==1.0.6"];
            if is_windows {
                pkgs.push("triton-windows");
            }
            uv_pip(app, base, &pkgs, net, true).await?;
            verify_attention_import(base, "sageattention").await
        }
        "sage_v2" => {
            // SageAttention 2.x is not on PyPI: Windows installs a prebuilt
            // wheel matched to the venv's torch/CUDA/Python build, Linux
            // compiles the pinned upstream source.
            if is_windows {
                emit_log(app, "Detecting the venv's PyTorch build...");
                let build = crate::attention::probe_torch_build(&venv_python(base))
                    .await
                    .map_err(|e| format!("cannot install sage_v2: {}", e))?;
                emit_log(
                    app,
                    &format!(
                        "Looking up a prebuilt SageAttention v2 wheel for {}...",
                        build.describe()
                    ),
                );
                let wheel = crate::attention::resolve_sage2_windows_wheel(http, &build)
                    .await
                    .map_err(|e| format!("cannot install sage_v2: {}", e))?;
                emit_log(app, &format!("Installing {}...", wheel.file_name));
                uv_pip(
                    app,
                    base,
                    &[wheel.url.as_str(), "triton-windows"],
                    net,
                    true,
                )
                .await?;
            } else {
                emit_log(
                    app,
                    "Building SageAttention v2 from source (CUDA kernels)...",
                );
                uv_pip(
                    app,
                    base,
                    &[
                        crate::attention::SAGE2_LINUX_GIT_SPEC,
                        "--no-build-isolation",
                    ],
                    net,
                    true,
                )
                .await?;
            }
            verify_attention_import(base, "sageattention").await
        }
        "flash_v1" => {
            emit_log(app, "Installing FlashAttention v1...");
            uv_pip(
                app,
                base,
                &["flash-attn<2.0", "--no-build-isolation"],
                net,
                true,
            )
            .await?;
            verify_attention_import(base, "flash_attn").await
        }
        "flash_v2" => {
            emit_log(app, "Installing FlashAttention v2...");
            uv_pip(
                app,
                base,
                &["flash-attn", "--no-build-isolation"],
                net,
                true,
            )
            .await?;
            verify_attention_import(base, "flash_attn").await
        }
        _ => Ok(()),
    }
}

/// Max VRAM in MB reported under a `/sys/class/drm`-shaped root, across all
/// `card*/device` entries. Checks `mem_info_vram_total` (i915, and AMD's
/// `amdgpu`) first, falling back to `tile0/vram0/total_bytes` (Intel's newer
/// `xe` driver, default for Battlemage and increasingly used for Alchemist).
/// No vendor filtering -- both paths only exist for dedicated VRAM in
/// practice, matching the pre-existing (also unfiltered) behavior here.
/// Dedicated VRAM in bytes for a single `.../cardN/device` sysfs dir, or
/// `None` if neither path exists. Both `mem_info_vram_total` (i915, amdgpu)
/// and `tile0/vram0/total_bytes` (Intel's `xe` driver) are only ever present
/// for cards with local memory -- an integrated GPU has neither file, which
/// is what lets callers use presence (not just value) to tell discrete and
/// integrated GPUs apart.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn device_vram_bytes(device_dir: &std::path::Path) -> Option<u64> {
    std::fs::read_to_string(device_dir.join("mem_info_vram_total"))
        .ok()
        .or_else(|| std::fs::read_to_string(device_dir.join("tile0/vram0/total_bytes")).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
}

// Only called from the #[cfg(target_os = "linux")] block; tests exercise it
// cross-platform, so the function is ungated but unused on non-Linux.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_vram_from_drm_root(root: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let mut max_vram: u64 = 0;
    for entry in entries.flatten() {
        let device_dir = entry.path().join("device");
        if let Some(bytes) = device_vram_bytes(&device_dir) {
            max_vram = max_vram.max(bytes / (1024 * 1024));
        }
    }
    max_vram
}

/// Detect total GPU VRAM in megabytes. Returns 0 if detection fails.
async fn detect_vram_mb() -> u64 {
    // NVIDIA: nvidia-smi reports MiB
    let nvidia_result = {
        let mut cmd = tokio::process::Command::new("nvidia-smi");
        cmd.args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"]);
        hide_window(&mut cmd);
        cmd.output().await
    };
    if let Ok(output) = nvidia_result {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(max) = text
                .lines()
                .filter_map(|l| l.trim().parse::<u64>().ok())
                .max()
            {
                return max;
            }
        }
    }

    // AMD (mem_info_vram_total) and Intel Arc (i915: mem_info_vram_total,
    // xe: tile0/vram0/total_bytes) both expose VRAM via sysfs (Linux only).
    #[cfg(target_os = "linux")]
    {
        let max_vram = parse_vram_from_drm_root(std::path::Path::new("/sys/class/drm"));
        if max_vram > 0 {
            return max_vram;
        }
    }

    // Windows: query GPU VRAM via WMI (covers AMD, Intel, etc.)
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty AdapterRAM",
        ]);
        hide_window(&mut cmd);
        if let Ok(output) = cmd.output().await {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(max) = text
                    .lines()
                    .filter_map(|l| l.trim().parse::<u64>().ok())
                    .max()
                {
                    let mb = max / (1024 * 1024);
                    if mb > 0 {
                        return mb;
                    }
                }
            }
        }
    }

    // macOS: use system_profiler for GPU VRAM
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = tokio::process::Command::new("system_profiler")
            .args(["SPDisplaysDataType"])
            .output()
            .await
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("VRAM") || trimmed.contains("Memory:") {
                        for word in trimmed.split_whitespace() {
                            if let Ok(val) = word.parse::<u64>() {
                                if trimmed.contains("GB") {
                                    return val * 1024;
                                } else if trimmed.contains("MB") {
                                    return val;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    0
}

/// Choose the best VRAM mode based on detected VRAM.
fn recommended_vram_mode(vram_mb: u64) -> &'static str {
    if vram_mb >= 24000 {
        "high" // 24 GB+ — --highvram keeps everything resident; only safe with headroom
    } else if vram_mb >= 4000 {
        "normal" // 4-24 GB — load fully for sampling, offload between gens
    } else if vram_mb > 0 {
        "low" // < 4 GB
    } else {
        "normal" // unknown — safe default
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn check_setup(app: AppHandle) -> Result<bool, AppError> {
    let dir = data_dir(&app)?;

    // Fast path: setup marker exists
    if dir.join(".setup_complete").exists() {
        return Ok(true);
    }

    // Fallback: if the persisted config points to a valid ComfyUI installation,
    // treat setup as complete. This handles the case where the data directory
    // was moved or the marker file was lost.
    let cfg = config::load_persisted_config();
    if cfg.setup_complete
        && matches!(cfg.server_mode, config::ServerMode::Remote)
        && !cfg.server_url.trim().is_empty()
    {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(".setup_complete"), "");
        log::info!(
            "Recovered setup state for remote ComfyUI at {}",
            cfg.server_url
        );
        return Ok(true);
    }
    let comfy_main = Path::new(&cfg.comfyui_path).join("main.py");
    if comfy_main.exists() {
        // Recreate the marker file so future checks are fast
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(".setup_complete"), "");
        log::info!(
            "Recovered setup state: ComfyUI found at {}",
            cfg.comfyui_path
        );
        return Ok(true);
    }

    Ok(false)
}

#[tauri::command]
pub async fn detect_gpu() -> Result<String, AppError> {
    Ok(detect_gpu_type().await)
}

/// Save a custom install location. Called before `run_setup` so the setup
/// installs into the chosen directory instead of the platform default.
#[tauri::command]
pub async fn set_install_path(path: String) -> Result<(), AppError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Install path cannot be empty".into());
    }
    config::save_custom_data_dir(trimmed)?;
    Ok(())
}

/// Return the current resolved data directory path so the frontend can show it.
#[tauri::command]
pub async fn get_install_path(app: AppHandle) -> Result<String, AppError> {
    let dir = data_dir(&app)?;
    Ok(dir.to_string_lossy().to_string())
}

/// Scan common locations for existing AI tool model directories.
/// Returns a list of detected paths with metadata about which tool they belong to.
#[tauri::command]
pub async fn detect_model_directories() -> Result<Vec<DetectedModelDir>, AppError> {
    Ok(scan_model_directories())
}

#[derive(Clone, serde::Serialize)]
pub struct DetectedModelDir {
    pub path: String,
    pub tool: String,
    pub has_checkpoints: bool,
    pub has_loras: bool,
    pub has_vae: bool,
}

fn scan_model_directories() -> Vec<DetectedModelDir> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Collect candidate directories based on platform
    let mut candidates: Vec<(PathBuf, &str)> = Vec::new();

    if let Some(home) = dirs::home_dir() {
        // ComfyUI common locations
        for name in &["ComfyUI", "comfyui"] {
            candidates.push((home.join(name).join("models"), "ComfyUI"));
            candidates.push((home.join("Desktop").join(name).join("models"), "ComfyUI"));
            candidates.push((home.join("Documents").join(name).join("models"), "ComfyUI"));
        }

        // A1111 / Forge
        for name in &[
            "stable-diffusion-webui",
            "stable-diffusion-webui-forge",
            "sd-webui-forge",
        ] {
            candidates.push((home.join(name).join("models"), "A1111/Forge"));
            candidates.push((
                home.join("Desktop").join(name).join("models"),
                "A1111/Forge",
            ));
        }

        // SwarmUI
        candidates.push((home.join("SwarmUI").join("Models"), "SwarmUI"));
        candidates.push((home.join("StableSwarmUI").join("Models"), "SwarmUI"));

        // StabilityMatrix
        candidates.push((
            home.join("StabilityMatrix").join("Models"),
            "StabilityMatrix",
        ));
        candidates.push((
            home.join(".stabilitymatrix").join("Models"),
            "StabilityMatrix",
        ));
        candidates.push((
            home.join("AppData")
                .join("Roaming")
                .join("StabilityMatrix")
                .join("Models"),
            "StabilityMatrix",
        ));
    }

    // Windows: check common drive roots
    #[cfg(target_os = "windows")]
    {
        for drive in &["C:", "D:", "E:", "F:", "G:"] {
            let root = PathBuf::from(drive).join("\\");
            for name in &["ComfyUI", "comfyui"] {
                candidates.push((root.join(name).join("models"), "ComfyUI"));
            }
            for name in &["stable-diffusion-webui", "stable-diffusion-webui-forge"] {
                candidates.push((root.join(name).join("models"), "A1111/Forge"));
            }
            candidates.push((root.join("SwarmUI").join("Models"), "SwarmUI"));
            candidates.push((
                root.join("StabilityMatrix").join("Models"),
                "StabilityMatrix",
            ));
        }
    }

    // Linux: check /opt and common locations
    #[cfg(target_os = "linux")]
    {
        let opt = PathBuf::from("/opt");
        candidates.push((opt.join("ComfyUI").join("models"), "ComfyUI"));
        candidates.push((
            opt.join("stable-diffusion-webui").join("models"),
            "A1111/Forge",
        ));
    }

    for (path, tool) in candidates {
        if !path.exists() || !path.is_dir() {
            continue;
        }

        // Canonicalize to avoid duplicates
        let canonical = match path.canonicalize() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => path.to_string_lossy().to_string(),
        };
        if !seen.insert(canonical.clone()) {
            continue;
        }

        // Check what model types exist in this directory
        let has_checkpoints = path.join("checkpoints").is_dir()
            || path.join("Stable-diffusion").is_dir()
            || path.join("Stable-Diffusion").is_dir()
            || path.join("StableDiffusion").is_dir();
        let has_loras = path.join("loras").is_dir()
            || path.join("Lora").is_dir()
            || path.join("LyCORIS").is_dir();
        let has_vae = path.join("vae").is_dir() || path.join("VAE").is_dir();

        // Only include if it has at least one recognizable model directory
        if has_checkpoints || has_loras || has_vae {
            results.push(DetectedModelDir {
                path: path.to_string_lossy().to_string(),
                tool: tool.to_string(),
                has_checkpoints,
                has_loras,
                has_vae,
            });
        }
    }

    results
}

/// Move the entire MooshieUI installation to a new directory.
/// Copies all data, updates the bootstrap pointer, and rewrites config paths.
#[tauri::command]
pub async fn move_installation(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    new_path: String,
) -> Result<(), AppError> {
    let new_path = new_path.trim().to_string();
    if new_path.is_empty() {
        return Err("New path cannot be empty".into());
    }

    let current = data_dir(&app)?;
    let dest = PathBuf::from(&new_path);

    if current == dest {
        return Err("New path is the same as the current location".into());
    }

    // Prevent recursive nesting: ensure neither path is inside the other.
    // Canonicalize existing paths; for the destination (which may not exist yet)
    // canonicalize the nearest existing ancestor.
    let current_canon = current.canonicalize().unwrap_or_else(|_| current.clone());
    let dest_canon = {
        let mut ancestor = dest.clone();
        loop {
            if ancestor.exists() {
                break ancestor
                    .canonicalize()
                    .unwrap_or_else(|_| ancestor.clone())
                    .join(dest.strip_prefix(&ancestor).unwrap_or(Path::new("")));
            }
            if !ancestor.pop() {
                break dest.clone();
            }
        }
    };
    if dest_canon.starts_with(&current_canon) {
        return Err(format!(
            "Cannot move into a subdirectory of the current installation. \
             '{}' is inside '{}'. Choose a location outside the current install folder.",
            dest.display(),
            current.display()
        )
        .into());
    }
    if current_canon.starts_with(&dest_canon) && current_canon != dest_canon {
        return Err(format!(
            "Cannot move to a parent of the current installation. \
             '{}' contains '{}'. Choose a different location.",
            dest.display(),
            current.display()
        )
        .into());
    }

    // Verify current installation exists
    if !current.exists() {
        return Err(format!(
            "Current data directory does not exist: {}",
            current.display()
        )
        .into());
    }

    // Create destination parent if needed
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create destination parent: {}", e))?;
    }

    // Check destination doesn't already have stuff (unless empty)
    if dest.exists() && dest.is_dir() {
        let is_empty = dest
            .read_dir()
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return Err(format!(
                "Destination already exists and is not empty: {}. Choose an empty folder or a new path.",
                dest.display()
            )
            .into());
        }
    }

    emit(&app, "move", "Stopping ComfyUI...", 5);

    // Stop ComfyUI if running
    if let Err(e) = crate::comfyui::process::stop_comfyui_process(&state).await {
        log::warn!("Could not stop ComfyUI before move: {}", e);
    }

    // Determine the current gallery location *before* copying.
    // If gallery_path is already custom (outside the data dir), it stays as-is.
    // If it's the default ({data_dir}/gallery), we keep it in place and set
    // gallery_path to the old absolute path so images are NOT copied (could be 400GB+).
    let old_gallery = {
        let cfg = state.config.read().await;
        if let Some(ref custom) = cfg.gallery_path {
            let p = PathBuf::from(custom.trim());
            if !p.as_os_str().is_empty() {
                Some(p) // Already custom — won't be inside `current`
            } else {
                None
            }
        } else {
            None
        }
    };
    let default_gallery = current.join("gallery");
    let gallery_is_default = old_gallery.is_none();
    let preserved_gallery_path = if gallery_is_default && default_gallery.exists() {
        // Gallery lives inside the data dir — skip copying it and point to the old location
        Some(default_gallery.clone())
    } else {
        old_gallery
    };

    emit(
        &app,
        "move",
        "Copying files to new location... This may take a few minutes.",
        15,
    );

    // Copy the entire directory tree, but skip the gallery if it's the default location
    // (it can be enormous — hundreds of GB of images).
    if gallery_is_default && default_gallery.exists() {
        copy_dir_recursive_skip(&current, &dest, "gallery")
            .map_err(|e| format!("Failed to copy data: {}", e))?;
    } else {
        copy_dir_recursive(&current, &dest).map_err(|e| format!("Failed to copy data: {}", e))?;
    }

    emit(&app, "move", "Updating configuration...", 85);

    // Update config paths to point to new location
    {
        let mut cfg = state.config.write().await;
        // Replace the old base path with the new one in comfyui_path and venv_path
        let current_str = current.to_string_lossy().to_string();
        let dest_str = dest.to_string_lossy().to_string();

        if cfg.comfyui_path.starts_with(&current_str) {
            cfg.comfyui_path = cfg.comfyui_path.replacen(&current_str, &dest_str, 1);
        } else {
            // Default layout
            cfg.comfyui_path = dest.join("comfyui").to_string_lossy().to_string();
        }

        if cfg.venv_path.starts_with(&current_str) {
            cfg.venv_path = cfg.venv_path.replacen(&current_str, &dest_str, 1);
        } else {
            cfg.venv_path = dest.join("venv").to_string_lossy().to_string();
        }

        // Preserve gallery at its current location instead of copying it
        if let Some(ref gp) = preserved_gallery_path {
            cfg.gallery_path = Some(gp.to_string_lossy().to_string());
        }

        // Save config to new location
        let config_json = serde_json::to_string_pretty(&*cfg).map_err(|e| e.to_string())?;
        std::fs::write(dest.join("config.json"), config_json)
            .map_err(|e| format!("Failed to write config to new location: {}", e))?;
    }

    // Update bootstrap pointer
    config::save_custom_data_dir(&new_path)?;

    // Recreate the venv so that pyvenv.cfg and uv trampoline executables
    // point to the new Python location.  Without this, `uv` / `python.exe`
    // inside the venv still reference the old absolute path and will fail
    // with "entity not found" on Windows (os error 2).
    let uv = uv_bin(&dest);
    let venv_dir = dest.join("venv");
    let python_dir = dest.join("python");
    if uv.exists() && venv_dir.exists() {
        emit(&app, "move", "Updating virtual environment paths...", 88);
        let python_dir_str = python_dir.to_string_lossy().to_string();
        let venv_dir_str = venv_dir.to_string_lossy().to_string();
        let uv_str = uv.to_string_lossy().to_string();
        // Read the copied venv's own pyvenv.cfg rather than assuming 3.11 —
        // an AMD ROCm-on-Windows-preview install created it with 3.12, and
        // hardcoding 3.11 here would silently downgrade/corrupt that venv.
        let python_version = venv_python_version(&venv_dir).unwrap_or_else(|| "3.11".to_string());
        let mut cmd = tokio::process::Command::new(&uv_str);
        cmd.args([
            "venv",
            &venv_dir_str,
            "--python",
            &python_version,
            "--allow-existing",
        ])
        .env("UV_PYTHON_INSTALL_DIR", &python_dir_str)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
        hide_window(&mut cmd);
        match cmd.status().await {
            Ok(s) if s.success() => {
                log::info!("Venv re-created at new location successfully");
            }
            Ok(s) => {
                log::warn!("uv venv --allow-existing exited with {}", s);
            }
            Err(e) => {
                log::warn!("Failed to re-create venv after move: {}", e);
            }
        }
    }

    // Copy .setup_complete marker
    if current.join(".setup_complete").exists() && !dest.join(".setup_complete").exists() {
        let _ = std::fs::write(dest.join(".setup_complete"), "1");
    }

    emit(&app, "move", "Cleaning up old location...", 90);

    // Remove old directory
    if let Err(e) = std::fs::remove_dir_all(&current) {
        log::warn!(
            "Could not remove old data directory {}: {}. You may want to delete it manually.",
            current.display(),
            e
        );
    }

    emit(
        &app,
        "done",
        &format!("Installation moved to {}", dest.display()),
        100,
    );
    Ok(())
}

/// Maximum directory nesting depth for recursive copy — a safety net
/// to prevent infinite recursion if source/destination overlap detection
/// is somehow bypassed.
const MAX_COPY_DEPTH: u32 = 64;

/// Recursively copy a directory and all its contents.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    copy_dir_recursive_inner(src, dst, 0, None)
}

/// Recursively copy a directory, skipping one top-level subdirectory by name.
fn copy_dir_recursive_skip(src: &Path, dst: &Path, skip_dir_name: &str) -> std::io::Result<()> {
    copy_dir_recursive_inner(src, dst, 0, Some(skip_dir_name))
}

fn copy_dir_recursive_inner(
    src: &Path,
    dst: &Path,
    depth: u32,
    skip_top_level: Option<&str>,
) -> std::io::Result<()> {
    if depth > MAX_COPY_DEPTH {
        return Err(std::io::Error::other(format!(
            "Directory nesting exceeded {} levels — aborting to prevent infinite recursion. \
                 Source '{}' may overlap with destination '{}'.",
            MAX_COPY_DEPTH,
            src.display(),
            dst.display()
        )));
    }
    std::fs::create_dir_all(dst)?;

    // Skip the destination directory if it appears inside the source tree
    let dst_canon = dst.canonicalize().ok();
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // If this source entry IS the destination folder, skip it to avoid recursion
        if file_type.is_dir() {
            if let Some(ref dc) = dst_canon {
                if let Ok(sp) = src_path.canonicalize() {
                    if sp == *dc {
                        continue;
                    }
                }
            }
        }

        // At the top level (depth 0), skip the named directory if requested
        if depth == 0 && file_type.is_dir() {
            if let Some(skip_name) = skip_top_level {
                if entry.file_name().to_string_lossy() == skip_name {
                    log::info!(
                        "Skipping '{}' directory during move (gallery preserved in place)",
                        skip_name
                    );
                    continue;
                }
            }
        }

        if file_type.is_dir() {
            copy_dir_recursive_inner(&src_path, &dst_path, depth + 1, None)?;
        } else if file_type.is_symlink() {
            // Try to preserve symlinks; fall back to copying content on Windows
            // where symlink creation requires admin privileges (error 1314)
            let target = std::fs::read_link(&src_path)?;
            let symlink_created = {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&target, &dst_path).is_ok()
                }
                #[cfg(windows)]
                {
                    if target.is_dir() {
                        std::os::windows::fs::symlink_dir(&target, &dst_path).is_ok()
                    } else {
                        std::os::windows::fs::symlink_file(&target, &dst_path).is_ok()
                    }
                }
            };
            if !symlink_created {
                // Symlink failed — copy the actual content instead
                let real_path = std::fs::canonicalize(&src_path)?;
                if real_path.is_dir() {
                    copy_dir_recursive_inner(&real_path, &dst_path, depth + 1, None)?;
                } else {
                    std::fs::copy(&real_path, &dst_path)?;
                }
            }
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn reinstall_pytorch(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    index_url: Option<String>,
) -> Result<(), AppError> {
    let base = data_dir(&app)?;
    let gpu = detect_gpu_type().await;
    let net = {
        let config = state.config.read().await;
        SetupNetworkOpts::from_options(config.network_proxy.clone(), config.pip_index_url.clone())
    };

    // The ROCm-on-Windows preview ships cp312-only wheels, so it's only usable
    // here when the existing venv was already created with Python 3.12 (i.e.
    // setup detected an eligible AMD GPU originally). A 3.11 venv can't be
    // upgraded in place — re-run setup to get a 3.12 one. Also skip it when
    // the caller passed an explicit index_url, since that signals they want
    // that specific index, not the wheel-based preview install.
    #[cfg(target_os = "windows")]
    let amd_windows_rocm = index_url.is_none()
        && gpu == "amd"
        && venv_python_version(&base.join("venv")).as_deref() == Some("3.12")
        && detect_amd_windows_rocm_supported().await;
    #[cfg(not(target_os = "windows"))]
    let amd_windows_rocm = false;

    // Heartbeat so the user sees activity during the long silent download
    let app_hb = app.clone();
    let heartbeat = tokio::spawn(async move {
        let mut elapsed = 0u64;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            elapsed += 30;
            let mins = elapsed / 60;
            let secs = elapsed % 60;
            let msg = if elapsed < 60 {
                format!(
                    "[{}s] Still working \u{2014} downloading PyTorch packages...",
                    secs
                )
            } else {
                format!(
                    "[{}m {}s] Still working \u{2014} large GPU packages are downloading...",
                    mins, secs
                )
            };
            emit_log(&app_hb, &msg);
        }
    });

    let install_result = if amd_windows_rocm {
        emit(
            &app,
            "pytorch",
            "Reinstalling PyTorch (AMD ROCm 7.2.1 Windows preview)...",
            50,
        );
        install_amd_windows_rocm_pytorch(&app, &base, &net).await
    } else {
        let url = match index_url {
            Some(ref url) => url.as_str(),
            None => match gpu.as_str() {
                "nvidia" => nvidia_pytorch_index_url().await,
                "amd" => amd_pytorch_index_url().await,
                "intel" => "https://download.pytorch.org/whl/xpu",
                "mps" => "",
                _ => "https://download.pytorch.org/whl/cpu",
            },
        };

        emit(&app, "pytorch", "Reinstalling PyTorch...", 50);

        let mut args = vec!["torch", "torchvision", "torchaudio", "--force-reinstall"];
        if !url.is_empty() {
            args.push("--index-url");
            args.push(url);
            args.push("--extra-index-url");
            args.push("https://pypi.org/simple/");
        }
        uv_pip(&app, &base, &args, &net, false).await
    };

    heartbeat.abort();
    install_result?;
    emit(&app, "done", "PyTorch reinstalled successfully.", 100);
    Ok(())
}

#[tauri::command]
pub async fn run_setup(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    gpu_type: Option<String>,
    install_path: Option<String>,
    attention_backend: Option<String>,
    network_proxy: Option<String>,
    pip_index_url: Option<String>,
) -> Result<(), AppError> {
    let net = SetupNetworkOpts::from_options(network_proxy.clone(), pip_index_url.clone());
    // If user chose a custom install path, save it as the bootstrap pointer
    // before anything else so all subsequent path resolution uses it.
    if let Some(ref path) = install_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            config::save_custom_data_dir(trimmed)?;
        }
    }
    let base = data_dir(&app)?;
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    // 0. Stop ComfyUI first. Re-running setup over an existing install rewrites
    //    the venv, and on Windows an executable that is currently running cannot
    //    be replaced — a live ComfyUI holds `venv\Scripts\python.exe` open and
    //    `uv venv` fails with "Access is denied. (os error 5)". Same reason
    //    `update_comfyui` stops the server before touching the working tree.
    crate::comfyui::process::stop_comfyui_process(&state)
        .await
        .ok();

    // 1. Download uv
    emit(&app, "uv", "Downloading uv package manager...", 5);
    step_download_uv(&app, &base, &state.http_client).await?;

    // 1b. Use user-selected GPU type, or auto-detect if not provided. Done before
    // Python/venv creation because an AMD GPU eligible for the ROCm-on-Windows
    // preview needs a Python 3.12 venv (the preview's wheels are cp312-only) —
    // every other case still uses 3.11.
    let gpu = match gpu_type {
        Some(ref g) if !g.is_empty() => g.clone(),
        _ => detect_gpu_type().await,
    };
    #[cfg(target_os = "windows")]
    let amd_windows_rocm = gpu == "amd" && detect_amd_windows_rocm_supported().await;
    #[cfg(not(target_os = "windows"))]
    let amd_windows_rocm = false;
    let python_version = if amd_windows_rocm { "3.12" } else { "3.11" };

    // 2. Install Python
    emit(
        &app,
        "python",
        &format!(
            "Installing Python {} (this may take a minute)...",
            python_version
        ),
        15,
    );
    step_install_python(&app, &base, python_version).await?;

    // 3. Download ComfyUI
    emit(&app, "comfyui", "Downloading ComfyUI...", 30);
    step_download_comfyui(&app, &base, &state.http_client).await?;

    // 4. Create venv
    emit(&app, "venv", "Creating virtual environment...", 40);
    step_create_venv(&app, &base, python_version).await?;

    // 5. Install PyTorch for the detected/selected GPU
    let label = match gpu.as_str() {
        "nvidia" => "NVIDIA CUDA",
        "amd" if amd_windows_rocm => "AMD ROCm (Windows preview)",
        #[cfg(target_os = "linux")]
        "amd" => "AMD ROCm",
        #[cfg(not(target_os = "linux"))]
        "amd" => "AMD (CPU-only — GPU not on the ROCm-on-Windows preview allowlist)",
        "intel" => "Intel XPU",
        "mps" => "Apple Metal",
        _ => "CPU",
    };
    emit(
        &app,
        "pytorch",
        &format!(
            "Installing PyTorch ({})... This may take several minutes.",
            label
        ),
        50,
    );
    step_install_pytorch(&app, &base, &gpu, amd_windows_rocm, &net).await?;

    // 6. Install ComfyUI deps
    emit(&app, "deps", "Installing ComfyUI dependencies...", 75);
    step_install_deps(&app, &base, &net).await?;

    // 6b. Quantization (convrot) kernels: comfy-kitchen ships with the deps above,
    // but needs Triton for its fast path on Windows. Both steps are non-fatal.
    ensure_quant_kernel_backend(&app, &base, &net).await;
    verify_quant_stack(&app, &base).await;

    // 7. Custom nodes
    emit(&app, "nodes", "Installing MooshieUI custom nodes...", 85);
    step_install_custom_nodes(&base)?;

    // 7b. Optional attention backend (NVIDIA only)
    let mut attention = attention_backend
        .as_deref()
        .unwrap_or("default")
        .to_string();
    if attention != "default" {
        emit(
            &app,
            "attention",
            &format!("Installing attention backend ({})...", attention),
            88,
        );
        if let Err(e) =
            step_install_attention_backend(&app, &base, &attention, &net, &state.http_client).await
        {
            // Non-fatal, but reset to "default" so we never persist a backend whose
            // package isn't actually installed (which would crash ComfyUI at launch).
            log::warn!(
                "Attention backend install failed, falling back to default: {}",
                e
            );
            emit_log(
                &app,
                &format!("⚠ Attention backend install failed: {}. Using default.", e),
            );
            attention = "default".to_string();
        }
    }

    // 8. Detect VRAM and persist config
    emit(
        &app,
        "config",
        "Detecting VRAM and saving configuration...",
        95,
    );
    let vram_mb = detect_vram_mb().await;
    let vram_mode = recommended_vram_mode(vram_mb);
    log::info!(
        "Detected {}MB VRAM, setting vram_mode={}",
        vram_mb,
        vram_mode
    );
    {
        let mut cfg = state.config.write().await;
        cfg.comfyui_path = base.join("comfyui").to_string_lossy().to_string();
        cfg.venv_path = base.join("venv").to_string_lossy().to_string();
        cfg.vram_mode = vram_mode.to_string();
        cfg.attention_backend = attention;
        cfg.network_proxy = net.network_proxy.clone();
        cfg.pip_index_url = net.pip_index_url.clone();
        cfg.setup_complete = true;
        config::save_config(&cfg)?;
    }

    std::fs::write(base.join(".setup_complete"), "1").map_err(|e| e.to_string())?;
    emit(&app, "done", "Setup complete! Starting ComfyUI...", 100);
    Ok(())
}

// ─── In-app ComfyUI updater ──────────────────────────────────────────────────

/// Report the installed ComfyUI version against the pinned target so the
/// Settings UI can offer an update.
#[tauri::command]
pub async fn get_comfyui_version(app: AppHandle) -> Result<ComfyUiVersionInfo, AppError> {
    let base = data_dir(&app)?;
    Ok(comfyui_version_info(&base.join("comfyui")))
}

/// Move an existing ComfyUI working tree onto [`COMFYUI_REF`] using git.
///
/// `git reset --hard <tag>` is run WITHOUT `git clean`, so untracked user data
/// (models, outputs, inputs, custom_nodes) is left untouched while ComfyUI's own
/// tracked source files are moved to the pinned release. A zip-based install
/// with no `.git` is converted to a managed git checkout in place; its existing
/// files are untracked and survive the reset. Returns the method used, for
/// reporting.
async fn update_comfyui_checkout(app: &AppHandle, comfyui_dir: &Path) -> Result<String, String> {
    let dir = comfyui_dir
        .to_str()
        .ok_or_else(|| "Invalid ComfyUI path".to_string())?;
    let url = "https://github.com/comfyanonymous/ComfyUI.git";

    let method = if comfyui_dir.join(".git").exists() {
        "git-fetch"
    } else {
        // Zip-based install (no git history). Initialise a repo in place so we
        // can fetch the pinned tag; existing files are untracked and survive the
        // reset that follows.
        emit_log(
            app,
            "ComfyUI install has no git history; initialising repository...",
        );
        run_logged(app, "git", &["-C", dir, "init"], &[])
            .await
            .map_err(|_| "Failed to initialise git in the ComfyUI directory".to_string())?;
        run_logged(
            app,
            "git",
            &["-C", dir, "remote", "add", "origin", url],
            &[],
        )
        .await
        .ok();
        "git-init"
    };

    // Ensure origin points at the canonical ComfyUI repo (the user may have a
    // fork remote). Non-fatal when it already matches.
    run_logged(
        app,
        "git",
        &["-C", dir, "remote", "set-url", "origin", url],
        &[],
    )
    .await
    .ok();

    // Shallow-fetch just the pinned tag, then move the working tree onto it.
    run_logged(
        app,
        "git",
        &[
            "-C",
            dir,
            "fetch",
            "--depth=1",
            "origin",
            "tag",
            COMFYUI_REF,
        ],
        &[],
    )
    .await
    .map_err(|_| format!("Failed to fetch ComfyUI {}", COMFYUI_REF))?;

    run_logged(
        app,
        "git",
        &["-C", dir, "reset", "--hard", COMFYUI_REF],
        &[],
    )
    .await
    .map_err(|_| format!("Failed to check out ComfyUI {}", COMFYUI_REF))?;

    Ok(method.to_string())
}

#[derive(Clone, serde::Serialize)]
pub struct ComfyUiUpdateResult {
    pub updated: bool,
    /// The tag the install was moved to ([`COMFYUI_REF`]).
    pub target_ref: String,
    /// "git-fetch" for an existing git checkout, "git-init" when a zip install
    /// was converted to a managed checkout.
    pub method: String,
}

/// Update the installed ComfyUI to the pinned [`COMFYUI_REF`]: stop the server,
/// move the working tree onto the tag, reinstall ComfyUI's Python deps, and
/// redeploy the bundled MooshieUI custom nodes. ComfyUI is left stopped; the
/// caller restarts it (via `start_comfyui`) so the full websocket wiring runs.
#[tauri::command]
pub async fn update_comfyui(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<ComfyUiUpdateResult, AppError> {
    let base = data_dir(&app)?;
    let comfyui_dir = base.join("comfyui");
    if !comfyui_dir.join("main.py").exists() {
        return Err("ComfyUI is not installed yet. Run setup first.".into());
    }

    // 1. Stop ComfyUI so the working tree is not locked (Windows) and the new
    //    version is picked up on the next start.
    emit(
        &app,
        "comfyui",
        &format!("Stopping ComfyUI to update to {}...", COMFYUI_REF),
        5,
    );
    crate::comfyui::process::stop_comfyui_process(&state)
        .await
        .ok();

    // 2. Move the working tree onto the pinned tag.
    emit(
        &app,
        "comfyui",
        &format!("Updating ComfyUI to {}...", COMFYUI_REF),
        20,
    );
    let method = update_comfyui_checkout(&app, &comfyui_dir).await?;

    // 3. Reinstall ComfyUI's Python deps (a new release may add/upgrade them).
    let net = {
        let config = state.config.read().await;
        SetupNetworkOpts::from_options(config.network_proxy.clone(), config.pip_index_url.clone())
    };
    emit(&app, "deps", "Updating ComfyUI dependencies...", 55);
    step_install_deps(&app, &base, &net).await?;

    // 3b. Re-ensure the quantization kernel backend: a new ComfyUI release can bump
    // comfy-kitchen, and an attention-backend switch may have happened since setup.
    ensure_quant_kernel_backend(&app, &base, &net).await;
    verify_quant_stack(&app, &base).await;

    // 4. Redeploy bundled MooshieUI custom nodes (content-hash gated internally).
    emit(&app, "nodes", "Redeploying MooshieUI custom nodes...", 85);
    let comfy_str = comfyui_dir.to_string_lossy().to_string();
    crate::comfyui::nodes::ensure_mooshie_nodes(&comfy_str)?;
    step_install_custom_nodes(&base)?;

    emit(
        &app,
        "done",
        &format!("ComfyUI updated to {}.", COMFYUI_REF),
        100,
    );
    Ok(ComfyUiUpdateResult {
        updated: true,
        target_ref: COMFYUI_REF.to_string(),
        method,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        amd_windows_rocm_model_supported, device_vram_bytes, parse_vram_from_drm_root,
        venv_python_version,
    };
    use std::fs;

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mooshieui-test-{}-{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_i915_mem_info_vram_total_path() {
        let root = unique_temp_dir("i915");
        let device_dir = root.join("card0").join("device");
        fs::create_dir_all(&device_dir).unwrap();
        fs::write(device_dir.join("mem_info_vram_total"), "17179869184").unwrap(); // 16 GiB
        assert_eq!(parse_vram_from_drm_root(&root), 16384);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn parses_xe_tile0_vram0_total_bytes_path_when_mem_info_absent() {
        let root = unique_temp_dir("xe");
        let device_dir = root.join("card0").join("device");
        let tile_dir = device_dir.join("tile0").join("vram0");
        fs::create_dir_all(&tile_dir).unwrap();
        fs::write(tile_dir.join("total_bytes"), "17179869184").unwrap(); // 16 GiB
        assert_eq!(parse_vram_from_drm_root(&root), 16384);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn prefers_mem_info_vram_total_when_both_paths_present() {
        let root = unique_temp_dir("both");
        let device_dir = root.join("card0").join("device");
        let tile_dir = device_dir.join("tile0").join("vram0");
        fs::create_dir_all(&tile_dir).unwrap();
        fs::write(device_dir.join("mem_info_vram_total"), "8589934592").unwrap(); // 8 GiB
        fs::write(tile_dir.join("total_bytes"), "17179869184").unwrap(); // 16 GiB
        assert_eq!(parse_vram_from_drm_root(&root), 8192);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn takes_max_across_multiple_cards() {
        let root = unique_temp_dir("multi");
        let card0 = root.join("card0").join("device");
        let card1 = root.join("card1").join("device");
        fs::create_dir_all(&card0).unwrap();
        fs::create_dir_all(&card1).unwrap();
        fs::write(card0.join("mem_info_vram_total"), "8589934592").unwrap(); // 8 GiB
        fs::write(card1.join("mem_info_vram_total"), "17179869184").unwrap(); // 16 GiB
        assert_eq!(parse_vram_from_drm_root(&root), 16384);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn returns_zero_when_no_vram_files_present() {
        let root = unique_temp_dir("empty");
        fs::create_dir_all(root.join("card0").join("device")).unwrap();
        assert_eq!(parse_vram_from_drm_root(&root), 0);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn returns_zero_when_root_does_not_exist() {
        let root = std::env::temp_dir().join("mooshieui-test-nonexistent-drm-root");
        assert_eq!(parse_vram_from_drm_root(&root), 0);
    }

    // device_vram_bytes() is what gates the Linux Intel Arc sysfs detection
    // (setup.rs, detect_gpu_type) against misfiring on integrated GPUs: every
    // Intel CPU exposes a vendor=0x8086/class=0x0300 DRM device just like a
    // discrete Arc card, but only a card with local memory has either of
    // these sysfs files at all.
    #[test]
    fn device_vram_bytes_is_none_for_integrated_gpu_with_no_vram_files() {
        let root = unique_temp_dir("igpu");
        let device_dir = root.join("card0").join("device");
        fs::create_dir_all(&device_dir).unwrap();
        assert_eq!(device_vram_bytes(&device_dir), None);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn device_vram_bytes_is_some_for_discrete_gpu_with_vram_file() {
        let root = unique_temp_dir("dgpu");
        let device_dir = root.join("card0").join("device");
        fs::create_dir_all(&device_dir).unwrap();
        fs::write(device_dir.join("mem_info_vram_total"), "8589934592").unwrap(); // 8 GiB
        assert_eq!(device_vram_bytes(&device_dir), Some(8589934592));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn amd_windows_rocm_model_supported_matches_every_allowlisted_model() {
        let names = [
            "amd radeon rx 9070",
            "amd radeon rx 9070 xt",
            "amd radeon rx 9060 xt",
            "amd radeon ai pro r9700",
            "amd radeon rx 7900 xtx",
            "amd radeon pro w7900",
            "amd radeon pro w7900 dual slot",
            "amd radeon rx 7700",
        ];
        for name in names {
            assert!(
                amd_windows_rocm_model_supported(name),
                "expected {name:?} to be supported"
            );
        }
    }

    #[test]
    fn amd_windows_rocm_model_supported_excludes_rx_7700_xt() {
        assert!(!amd_windows_rocm_model_supported("amd radeon rx 7700 xt"));
    }

    #[test]
    fn amd_windows_rocm_model_supported_excludes_unrelated_models() {
        assert!(!amd_windows_rocm_model_supported("amd radeon rx 6800"));
        assert!(!amd_windows_rocm_model_supported("amd radeon graphics"));
        assert!(!amd_windows_rocm_model_supported("amd radeon rx 5600"));
    }

    #[test]
    fn venv_python_version_parses_version_key() {
        let root = unique_temp_dir("venv-version-key");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("pyvenv.cfg"),
            "home = /usr/bin\nversion = 3.12.7\n",
        )
        .unwrap();
        assert_eq!(venv_python_version(&root), Some("3.12".to_string()));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn venv_python_version_parses_version_info_key() {
        let root = unique_temp_dir("venv-version-info-key");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("pyvenv.cfg"), "version_info = 3.11.9\n").unwrap();
        assert_eq!(venv_python_version(&root), Some("3.11".to_string()));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn venv_python_version_none_when_cfg_missing() {
        let root = unique_temp_dir("venv-missing-cfg");
        fs::create_dir_all(&root).unwrap();
        assert_eq!(venv_python_version(&root), None);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn venv_python_version_none_when_key_missing() {
        let root = unique_temp_dir("venv-missing-key");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();
        assert_eq!(venv_python_version(&root), None);
        fs::remove_dir_all(&root).ok();
    }
}
