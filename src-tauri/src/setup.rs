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
    // Prefer MOOSHIEUI_DATA_DIR env var for custom install locations,
    // fall back to Tauri's platform-specific app data directory.
    if let Ok(custom) = std::env::var("MOOSHIEUI_DATA_DIR") {
        let p = PathBuf::from(custom.trim());
        if !p.as_os_str().is_empty() {
            return Ok(p);
        }
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
        &["venv", venv_dir.to_str().unwrap(), "--python", "3.11", "--allow-existing"],
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
                                    return "intel".to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
        // Windows: check for discrete AMD/Intel GPUs via WMI (rocm-smi won't exist on Windows)
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
                // Only match discrete AMD GPUs (RX series) — not integrated Radeon on Ryzen APUs.
                // Integrated GPUs report as "AMD Radeon Graphics" or "AMD Radeon Vega X Graphics"
                // and don't support ROCm. Discrete GPUs have "RX" in the name (RX 7900, RX 6800, etc.)
                if text.contains("radeon rx") || text.contains("radeon pro w") {
                    return "amd".to_string();
                }
                // Intel Arc discrete GPUs (A770, A750, B580, etc.)
                if text.contains("intel arc") || text.contains("arc a") || text.contains("arc b") {
                    return "intel".to_string();
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

/// Detect the AMD GPU architecture string (e.g. "gfx1100", "gfx1201") by reading
/// sysfs on Linux. Returns None if detection fails or the GPU is not AMD.
#[cfg(target_os = "linux")]
async fn detect_amd_gpu_arch() -> Option<String> {
    // Try rocm-smi first (most reliable if ROCm is installed)
    let mut cmd = tokio::process::Command::new("rocm-smi");
    cmd.args(["--showproductname"]);
    hide_window(&mut cmd);
    if let Ok(output) = cmd.output().await {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
            // Look for gfx architecture identifiers in output
            for word in text.split_whitespace() {
                if word.starts_with("gfx") {
                    return Some(word.to_string());
                }
            }
        }
    }

    // Fallback: read the GPU firmware version from sysfs to determine architecture
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

            // Try to read the device ID to infer architecture
            let device_path = entry.path().join("device/device");
            if let Ok(device_id) = std::fs::read_to_string(&device_path) {
                let device_id = device_id.trim().to_lowercase();
                // RX 9070 XT / RDNA 4 device IDs start with 0x15xx
                // Map known device ID prefixes to GPU architectures
                if device_id.starts_with("0x15") {
                    return Some("gfx1201".to_string());
                }
            }

            // Also try reading the GPU ID from ip_discovery or amdgpu debugfs
            let ip_path = entry.path().join("device/ip_discovery/die/0/GC/0/major");
            if let Ok(major) = std::fs::read_to_string(&ip_path) {
                let major = major.trim();
                if major == "12" {
                    return Some("gfx1200".to_string()); // RDNA 4 family
                }
            }
        }
    }

    None
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

#[cfg(not(target_os = "linux"))]
async fn amd_pytorch_index_url() -> &'static str {
    "https://download.pytorch.org/whl/rocm6.2"
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
            let index_url = amd_pytorch_index_url().await;
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

    // Fast path: setup marker exists
    if dir.join(".setup_complete").exists() {
        return Ok(true);
    }

    // Fallback: if the persisted config points to a valid ComfyUI installation,
    // treat setup as complete. This handles the case where the data directory
    // was moved or the marker file was lost.
    let cfg = config::load_persisted_config();
    let comfy_main = Path::new(&cfg.comfyui_path).join("main.py");
    if comfy_main.exists() {
        // Recreate the marker file so future checks are fast
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(".setup_complete"), "");
        log::info!("Recovered setup state: ComfyUI found at {}", cfg.comfyui_path);
        return Ok(true);
    }

    Ok(false)
}

#[tauri::command]
pub async fn detect_gpu() -> Result<String, String> {
    Ok(detect_gpu_type().await)
}

/// Save a custom install location. Called before `run_setup` so the setup
/// installs into the chosen directory instead of the platform default.
#[tauri::command]
pub async fn set_install_path(path: String) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Install path cannot be empty".to_string());
    }
    config::save_custom_data_dir(trimmed)?;
    Ok(())
}

/// Return the current resolved data directory path so the frontend can show it.
#[tauri::command]
pub async fn get_install_path(app: AppHandle) -> Result<String, String> {
    let dir = data_dir(&app)?;
    Ok(dir.to_string_lossy().to_string())
}

/// Scan common locations for existing AI tool model directories.
/// Returns a list of detected paths with metadata about which tool they belong to.
#[tauri::command]
pub async fn detect_model_directories() -> Result<Vec<DetectedModelDir>, String> {
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
            candidates.push((home.join("Desktop").join(name).join("models"), "A1111/Forge"));
        }

        // SwarmUI
        candidates.push((home.join("SwarmUI").join("Models"), "SwarmUI"));
        candidates.push((home.join("StableSwarmUI").join("Models"), "SwarmUI"));

        // StabilityMatrix
        candidates.push((home.join("StabilityMatrix").join("Models"), "StabilityMatrix"));
        candidates.push((home.join(".stabilitymatrix").join("Models"), "StabilityMatrix"));
        candidates.push((
            home.join("AppData").join("Roaming").join("StabilityMatrix").join("Models"),
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
            for name in &[
                "stable-diffusion-webui",
                "stable-diffusion-webui-forge",
            ] {
                candidates.push((root.join(name).join("models"), "A1111/Forge"));
            }
            candidates.push((root.join("SwarmUI").join("Models"), "SwarmUI"));
            candidates.push((root.join("StabilityMatrix").join("Models"), "StabilityMatrix"));
        }
    }

    // Linux: check /opt and common locations
    #[cfg(target_os = "linux")]
    {
        let opt = PathBuf::from("/opt");
        candidates.push((opt.join("ComfyUI").join("models"), "ComfyUI"));
        candidates.push((opt.join("stable-diffusion-webui").join("models"), "A1111/Forge"));
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
    state: tauri::State<'_, AppState>,
    new_path: String,
) -> Result<(), String> {
    let new_path = new_path.trim().to_string();
    if new_path.is_empty() {
        return Err("New path cannot be empty".to_string());
    }

    let current = data_dir(&app)?;
    let dest = PathBuf::from(&new_path);

    if current == dest {
        return Err("New path is the same as the current location".to_string());
    }

    // Verify current installation exists
    if !current.exists() {
        return Err(format!("Current data directory does not exist: {}", current.display()));
    }

    // Create destination parent if needed
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create destination parent: {}", e))?;
    }

    // Check destination doesn't already have stuff (unless empty)
    if dest.exists() && dest.is_dir() {
        let is_empty = dest.read_dir()
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return Err(format!(
                "Destination already exists and is not empty: {}. Choose an empty folder or a new path.",
                dest.display()
            ));
        }
    }

    emit(&app, "move", "Stopping ComfyUI...", 5);

    // Stop ComfyUI if running
    if let Err(e) = crate::comfyui::process::stop_comfyui_process(&state).await {
        log::warn!("Could not stop ComfyUI before move: {}", e);
    }

    emit(&app, "move", "Copying files to new location... This may take a few minutes.", 15);

    // Copy the entire directory tree
    copy_dir_recursive(&current, &dest)
        .map_err(|e| format!("Failed to copy data: {}", e))?;

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

        // Save config to new location
        let config_json = serde_json::to_string_pretty(&*cfg).map_err(|e| e.to_string())?;
        std::fs::write(dest.join("config.json"), config_json)
            .map_err(|e| format!("Failed to write config to new location: {}", e))?;
    }

    // Update bootstrap pointer
    config::save_custom_data_dir(&new_path)?;

    // Copy .setup_complete marker
    if current.join(".setup_complete").exists() && !dest.join(".setup_complete").exists() {
        let _ = std::fs::write(dest.join(".setup_complete"), "1");
    }

    emit(&app, "move", "Cleaning up old location...", 90);

    // Remove old directory
    if let Err(e) = std::fs::remove_dir_all(&current) {
        log::warn!("Could not remove old data directory {}: {}. You may want to delete it manually.", current.display(), e);
    }

    emit(&app, "done", &format!("Installation moved to {}", dest.display()), 100);
    Ok(())
}

/// Recursively copy a directory and all its contents.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_symlink() {
            // Preserve symlinks
            let target = std::fs::read_link(&src_path)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &dst_path)?;
            #[cfg(windows)]
            {
                if target.is_dir() {
                    std::os::windows::fs::symlink_dir(&target, &dst_path)?;
                } else {
                    std::os::windows::fs::symlink_file(&target, &dst_path)?;
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
    _state: tauri::State<'_, AppState>,
    index_url: Option<String>,
) -> Result<(), String> {
    let base = data_dir(&app)?;
    let gpu = detect_gpu_type().await;

    let url = match index_url {
        Some(ref url) => url.as_str(),
        None => match gpu.as_str() {
            "nvidia" => "https://download.pytorch.org/whl/cu128",
            "amd" => amd_pytorch_index_url().await,
            "intel" => "https://download.pytorch.org/whl/xpu",
            "mps" => "",
            _ => "https://download.pytorch.org/whl/cpu",
        },
    };

    emit(&app, "pytorch", "Reinstalling PyTorch...", 50);

    let mut args = vec![
        "torch",
        "torchvision",
        "torchaudio",
        "--force-reinstall",
    ];
    if !url.is_empty() {
        args.push("--index-url");
        args.push(url);
    }

    uv_pip(&app, &base, &args).await?;
    emit(&app, "done", "PyTorch reinstalled successfully.", 100);
    Ok(())
}

#[tauri::command]
pub async fn run_setup(app: AppHandle, state: tauri::State<'_, AppState>, gpu_type: Option<String>, install_path: Option<String>) -> Result<(), String> {
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
        "amd" => "AMD ROCm",
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
