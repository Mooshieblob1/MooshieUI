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

#[cfg(not(target_os = "windows"))]
fn hide_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    cmd // no-op on non-Windows
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
            archive.display(),
            temp_dir.display(),
            temp_dir.display(),
            bin_dir.display(),
            temp_dir.display(),
            bin_dir.display(),
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

async fn step_install_python(app: &AppHandle, base: &Path) -> Result<(), String> {
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
    let args = ["python", "install", "3.11"];
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
        &["python", "install", "--reinstall", "3.11"],
        &env,
    )
    .await
    .map_err(|_| "Failed to install Python 3.11".to_string())
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
            zip_path.display(),
            base.display()
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

async fn step_create_venv(app: &AppHandle, base: &Path) -> Result<(), String> {
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
            "3.11",
            "--allow-existing",
        ],
        &[("UV_PYTHON_INSTALL_DIR", &python_dir_str)],
    )
    .await
    .map_err(|_| "Failed to create virtual environment".to_string())
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
                if text.contains("radeon rx") || text.contains("radeon pro w") {
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
                            // Check if it's a discrete GPU (class 0x0300 = VGA controller)
                            let class_path = entry.path().join("device/class");
                            if let Ok(class) = std::fs::read_to_string(&class_path) {
                                if class.trim().starts_with("0x0300") {
                                    log::info!("GPU detection: Intel Arc GPU found via sysfs");
                                    return "intel".to_string();
                                }
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

/// Pick the correct PyTorch CUDA wheel index for NVIDIA GPUs.
/// Blackwell (compute ≥ 12.0) needs cu130+; older GPUs use cu128.
async fn nvidia_pytorch_index_url() -> &'static str {
    let output = tokio::process::Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader,nounits"])
        .output()
        .await;

    if let Ok(o) = output {
        if o.status.success() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                if let Some((major_str, _)) = line.trim().split_once('.') {
                    if let Ok(major) = major_str.parse::<u32>() {
                        if major >= 12 {
                            log::info!("Blackwell GPU detected — using cu130 PyTorch index");
                            return "https://download.pytorch.org/whl/cu130";
                        }
                    }
                }
            }
        }
    }
    "https://download.pytorch.org/whl/cu128"
}

async fn step_install_pytorch(
    app: &AppHandle,
    base: &Path,
    gpu: &str,
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
        "amd" => {
            let index_url = amd_pytorch_index_url().await;
            emit_log(app, &format!("Using PyTorch index: {}", index_url));
            #[cfg(target_os = "windows")]
            emit_log(
                app,
                "⚠ WARNING: AMD GPU acceleration (ROCm) is only available on Linux. \
                 PyTorch will be installed in CPU-only mode on Windows. \
                 For GPU acceleration with AMD on Windows, consider using Linux instead.",
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

/// Install an optional attention backend package into the venv.
async fn step_install_attention_backend(
    app: &AppHandle,
    base: &Path,
    backend: &str,
    net: &SetupNetworkOpts,
) -> Result<(), String> {
    match backend {
        "sage_v1" => {
            emit_log(app, "Installing SageAttention v1 (pure Triton)...");
            uv_pip(app, base, &["sageattention==1.0.6"], net, true).await
        }
        "sage_v2" => {
            emit_log(app, "Installing SageAttention v2 (CUDA kernels)...");
            uv_pip(
                app,
                base,
                &["sageattention>=2.0.0,<3.0.0", "--no-build-isolation"],
                net,
                true,
            )
            .await
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
            .await
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
            .await
        }
        _ => Ok(()),
    }
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

    // AMD: sysfs exposes VRAM in bytes (Linux only)
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            let mut max_vram: u64 = 0;
            for entry in entries.flatten() {
                let path = entry.path().join("device/mem_info_vram_total");
                if path.exists() {
                    if let Ok(val) = std::fs::read_to_string(&path) {
                        if let Ok(bytes) = val.trim().parse::<u64>() {
                            max_vram = max_vram.max(bytes / (1024 * 1024));
                        }
                    }
                }
            }
            if max_vram > 0 {
                return max_vram;
            }
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
        let mut cmd = tokio::process::Command::new(&uv_str);
        cmd.args([
            "venv",
            &venv_dir_str,
            "--python",
            "3.11",
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

    let install_result = uv_pip(&app, &base, &args, &net, false).await;
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

    // 1. Download uv
    emit(&app, "uv", "Downloading uv package manager...", 5);
    step_download_uv(&app, &base, &state.http_client).await?;

    // 2. Install Python
    emit(
        &app,
        "python",
        "Installing Python 3.11 (this may take a minute)...",
        15,
    );
    step_install_python(&app, &base).await?;

    // 3. Download ComfyUI
    emit(&app, "comfyui", "Downloading ComfyUI...", 30);
    step_download_comfyui(&app, &base, &state.http_client).await?;

    // 4. Create venv
    emit(&app, "venv", "Creating virtual environment...", 40);
    step_create_venv(&app, &base).await?;

    // 5. Use user-selected GPU type, or auto-detect if not provided
    let gpu = match gpu_type {
        Some(ref g) if !g.is_empty() => g.clone(),
        _ => detect_gpu_type().await,
    };
    let label = match gpu.as_str() {
        "nvidia" => "NVIDIA CUDA",
        #[cfg(target_os = "linux")]
        "amd" => "AMD ROCm",
        #[cfg(not(target_os = "linux"))]
        "amd" => "AMD (CPU-only — ROCm requires Linux)",
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
    step_install_pytorch(&app, &base, &gpu, &net).await?;

    // 6. Install ComfyUI deps
    emit(&app, "deps", "Installing ComfyUI dependencies...", 75);
    step_install_deps(&app, &base, &net).await?;

    // 7. Custom nodes
    emit(&app, "nodes", "Installing MooshieUI custom nodes...", 85);
    step_install_custom_nodes(&base)?;

    // 7b. Optional attention backend (NVIDIA only)
    let attention = attention_backend
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
        if let Err(e) = step_install_attention_backend(&app, &base, &attention, &net).await {
            // Non-fatal: log warning and fall back to default
            log::warn!(
                "Attention backend install failed, falling back to default: {}",
                e
            );
            emit_log(
                &app,
                &format!("⚠ Attention backend install failed: {}. Using default.", e),
            );
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
