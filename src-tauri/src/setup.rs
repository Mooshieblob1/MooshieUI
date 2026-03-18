use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use std::process::Stdio;

use crate::config;
use crate::state::AppState;

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

fn emit_log(app: &AppHandle, line: &str) {
    app.emit("setup:log", line).ok();
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
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
async fn run_logged(
    app: &AppHandle,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log(&app_out, &line);
            }
        }
    });

    let err_task = tokio::spawn(async move {
        if let Some(err) = stderr {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log(&app_err, &line);
            }
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
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Download returned status {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file =
        std::fs::File::create(dest).map_err(|e| format!("Failed to create file: {}", e))?;

    // Emit initial progress
    app.emit(
        "download:progress",
        DownloadProgress {
            filename: label.to_string(),
            downloaded: 0,
            total,
            done: false,
        },
    )
    .ok();

    // Stream chunks
    let mut last_emit: u64 = 0;
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Download read error: {}", e))?
    {
        use std::io::Write;
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

    let python_dir_str = python_dir.to_string_lossy().to_string();
    run_logged(
        app,
        uv.to_str().unwrap(),
        &["python", "install", "3.11"],
        &[("UV_PYTHON_INSTALL_DIR", &python_dir_str)],
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

    // Try git clone first (most systems have git)
    let git_result = run_logged(
        app,
        "git",
        &[
            "clone",
            "--depth=1",
            "https://github.com/comfyanonymous/ComfyUI.git",
            comfyui_dir.to_str().unwrap(),
        ],
        &[],
    )
    .await;

    if git_result.is_ok() {
        return Ok(());
    }

    // Fallback: download zip
    emit_log(app, "Git clone failed, falling back to zip download...");
    let zip_url = "https://github.com/comfyanonymous/ComfyUI/archive/refs/heads/master.zip";
    let zip_path = base.join("_comfyui.zip");
    download_file(app, client, zip_url, &zip_path, "ComfyUI").await?;

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

    std::fs::rename(base.join("ComfyUI-master"), &comfyui_dir)
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
        &["venv", venv_dir.to_str().unwrap(), "--python", "3.11"],
        &[("UV_PYTHON_INSTALL_DIR", &python_dir_str)],
    )
    .await
    .map_err(|_| "Failed to create virtual environment".to_string())
}

async fn detect_gpu_type() -> String {
    #[cfg(target_os = "macos")]
    {
        return "mps".to_string();
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Use hidden-window commands for detection
        let nvidia_result = {
            let mut cmd = tokio::process::Command::new("nvidia-smi");
            hide_window(&mut cmd);
            cmd.output().await
        };
        if let Ok(output) = nvidia_result {
            if output.status.success() {
                return "nvidia".to_string();
            }
        }

        let rocm_result = {
            let mut cmd = tokio::process::Command::new("rocm-smi");
            hide_window(&mut cmd);
            cmd.output().await
        };
        if let Ok(output) = rocm_result {
            if output.status.success() {
                return "amd".to_string();
            }
        }

        #[cfg(target_os = "linux")]
        if Path::new("/opt/rocm").exists() {
            return "amd".to_string();
        }
        // Windows: check for AMD GPU via WMI (rocm-smi won't exist on Windows)
        #[cfg(target_os = "windows")]
        {
            let mut cmd = tokio::process::Command::new("powershell");
            cmd.args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
            ]);
            hide_window(&mut cmd);
            if let Ok(output) = cmd.output().await {
                let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
                if text.contains("radeon") || text.contains("amd") {
                    return "amd".to_string();
                }
            }
        }
        "cpu".to_string()
    }
}

async fn uv_pip(app: &AppHandle, base: &Path, args: &[&str]) -> Result<(), String> {
    let uv = uv_bin(base);
    let python = venv_python(base);
    let python_dir = base.join("python");

    let python_str = python.to_string_lossy().to_string();
    let python_dir_str = python_dir.to_string_lossy().to_string();

    let mut cmd_args: Vec<&str> = vec!["pip", "install", "--python", &python_str];
    cmd_args.extend_from_slice(args);

    run_logged(
        app,
        uv.to_str().unwrap(),
        &cmd_args,
        &[("UV_PYTHON_INSTALL_DIR", &python_dir_str)],
    )
    .await
    .map_err(|_| format!("pip install failed for: {}", args.join(" ")))
}

async fn step_install_pytorch(app: &AppHandle, base: &Path, gpu: &str) -> Result<(), String> {
    match gpu {
        "nvidia" => {
            uv_pip(
                app,
                base,
                &[
                    "torch",
                    "torchvision",
                    "torchaudio",
                    "--index-url",
                    "https://download.pytorch.org/whl/cu128",
                ],
            )
            .await
        }
        "amd" => {
            uv_pip(
                app,
                base,
                &[
                    "torch",
                    "torchvision",
                    "torchaudio",
                    "--index-url",
                    "https://download.pytorch.org/whl/rocm6.2",
                ],
            )
            .await
        }
        "mps" => uv_pip(app, base, &["torch", "torchvision", "torchaudio"]).await,
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
                ],
            )
            .await
        }
    }
}

async fn step_install_deps(app: &AppHandle, base: &Path) -> Result<(), String> {
    let requirements = base.join("comfyui").join("requirements.txt");
    let uv = uv_bin(base);
    let python = venv_python(base);
    let python_dir = base.join("python");

    let python_str = python.to_string_lossy().to_string();
    let python_dir_str = python_dir.to_string_lossy().to_string();
    let req_str = requirements.to_string_lossy().to_string();

    run_logged(
        app,
        uv.to_str().unwrap(),
        &["pip", "install", "--python", &python_str, "-r", &req_str],
        &[("UV_PYTHON_INSTALL_DIR", &python_dir_str)],
    )
    .await
    .map_err(|_| "Failed to install ComfyUI dependencies".to_string())
}

fn step_install_custom_nodes(base: &Path) -> Result<(), String> {
    let comfyui = base.join("comfyui");
    let extras = comfyui.join("comfy_extras");
    let blueprints = comfyui.join("blueprints");
    std::fs::create_dir_all(&extras).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&blueprints).map_err(|e| e.to_string())?;

    // Embedded at compile time from comfyui-nodes/ directory
    let node_py = include_str!("../../comfyui-nodes/nodes_tiled_diffusion.py");
    let blueprint = include_str!("../../comfyui-nodes/Image Tiled Upscale (img2img).json");

    std::fs::write(extras.join("nodes_tiled_diffusion.py"), node_py)
        .map_err(|e| format!("Failed to write node: {}", e))?;
    std::fs::write(
        blueprints.join("Image Tiled Upscale (img2img).json"),
        blueprint,
    )
    .map_err(|e| format!("Failed to write blueprint: {}", e))?;

    // Register in nodes.py
    let nodes_py = comfyui.join("nodes.py");
    let content =
        std::fs::read_to_string(&nodes_py).map_err(|e| format!("Failed to read nodes.py: {}", e))?;
    if !content.contains("nodes_tiled_diffusion.py") {
        let patched = content.replace(
            "\"nodes_upscale_model.py\",",
            "\"nodes_upscale_model.py\",\n        \"nodes_tiled_diffusion.py\",",
        );
        std::fs::write(&nodes_py, patched)
            .map_err(|e| format!("Failed to patch nodes.py: {}", e))?;
    }
    Ok(())
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
    if vram_mb >= 8000 {
        "high" // 8 GB+ — keep everything in VRAM
    } else if vram_mb >= 4000 {
        "normal" // 4-8 GB — load fully for sampling, offload between gens
    } else if vram_mb > 0 {
        "low" // < 4 GB
    } else {
        "normal" // unknown — safe default
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn check_setup(app: AppHandle) -> Result<bool, String> {
    let dir = data_dir(&app)?;
    Ok(dir.join(".setup_complete").exists())
}

#[tauri::command]
pub async fn detect_gpu() -> Result<String, String> {
    Ok(detect_gpu_type().await)
}

#[tauri::command]
pub async fn run_setup(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
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

    // 5. Detect GPU + install PyTorch
    let gpu = detect_gpu_type().await;
    let label = match gpu.as_str() {
        "nvidia" => "NVIDIA CUDA",
        "amd" => "AMD ROCm",
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
    step_install_pytorch(&app, &base, &gpu).await?;

    // 6. Install ComfyUI deps
    emit(&app, "deps", "Installing ComfyUI dependencies...", 75);
    step_install_deps(&app, &base).await?;

    // 7. Custom nodes
    emit(&app, "nodes", "Installing MooshieUI custom nodes...", 90);
    step_install_custom_nodes(&base)?;

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
        cfg.setup_complete = true;
        config::save_config(&cfg)?;
    }

    std::fs::write(base.join(".setup_complete"), "1").map_err(|e| e.to_string())?;
    emit(&app, "done", "Setup complete! Starting ComfyUI...", 100);
    Ok(())
}
