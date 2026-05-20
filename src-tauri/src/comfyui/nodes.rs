//! Auto-deploy bundled MooshieUI custom nodes into ComfyUI's custom_nodes directory.
//! The Python source is embedded at compile time and written to disk before ComfyUI starts.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Clone, Copy)]
struct RequiredCustomNodePackage {
    name: &'static str,
    git_url: &'static str,
    verify_nodes: &'static [&'static str],
}

const REQUIRED_CONTROLNET_PACKAGES: &[RequiredCustomNodePackage] = &[
    RequiredCustomNodePackage {
        name: "comfyui_controlnet_aux",
        git_url: "https://github.com/Fannovel16/comfyui_controlnet_aux.git",
        verify_nodes: &[
            "CannyEdgePreprocessor",
            "DepthAnythingV2Preprocessor",
            "OpenposePreprocessor",
            "LineArtPreprocessor",
            "ScribblePreprocessor",
            "HEDPreprocessor",
            "FakeScribblePreprocessor",
        ],
    },
    RequiredCustomNodePackage {
        name: "ComfyUI-Anima-LLLite",
        git_url: "https://github.com/kohya-ss/ComfyUI-Anima-LLLite.git",
        verify_nodes: &["AnimaLLLiteApply"],
    },
];

/// Substring present in [`format_missing_mooshie_nodes_error`] output.
pub const MISSING_MOOSHIE_NODES_MARKER: &str = "has not loaded required MooshieUI custom nodes";

/// Substring present in [`verify_required_controlnet_nodes`] error output.
pub const MISSING_CONTROLNET_NODES_MARKER: &str = "Required ControlNet custom nodes failed to load";

const REQUIRED_MOOSHIE_NODE_CLASSES: &[&str] = &[
    "MooshieSaveImage",
    "MooshieFaceDetailer",
    "MooshieSoftGuidance",
    "MooshieSmartGuidance",
    "NanoSaurLoader",
    "ApplyTiledDiffusion",
];

const MOOSHIE_NODES_INIT: &str = include_str!("mooshie_nodes.py");
const TILED_DIFFUSION_PY: &str = include_str!("../../../comfyui-nodes/nodes_tiled_diffusion.py");
const GUIDANCE_PY: &str = include_str!("../../../comfyui-nodes/nodes_guidance.py");
/// Combined flat file: nodes.py content + NODE_CLASS_MAPPINGS.
/// Deployed as a single top-level file to avoid the circular import that occurs when a
/// package named `nodes.py` tries to `import nodes` (ComfyUI's own nodes.py) while
/// ComfyUI's nodes.py is still being initialized.
const SDXL_FLUX2VAE_PY: &str =
    include_str!("../../../comfyui-nodes/nodes_sdxl_flux2vae_combined.py");

// ── Nanosaur custom node package (NanoSaurLoader) ────────────────────────────
const NANOSAUR_INIT_PY: &str = include_str!("../../../comfyui-nodes/nanosaur_support/__init__.py");
const NANOSAUR_NODES_PY: &str = include_str!("../../../comfyui-nodes/nanosaur_support/nodes.py");
const NANOSAUR_MODEL_PY: &str = include_str!("../../../comfyui-nodes/nanosaur_support/model.py");
const NANOSAUR_TEXT_ENCODER_PY: &str =
    include_str!("../../../comfyui-nodes/nanosaur_support/text_encoder.py");
const NANOSAUR_VAE_PY: &str = include_str!("../../../comfyui-nodes/nanosaur_support/vae.py");

/// Ensure all bundled MooshieUI custom nodes exist in ComfyUI's custom_nodes directory.
/// Always overwrites to keep in sync with the app version.
pub fn ensure_mooshie_nodes(comfyui_path: &str) -> Result<(), String> {
    let custom_nodes = Path::new(comfyui_path).join("custom_nodes");

    // ── mooshie-nodes package (face detailer, etc.) ──────────────────────────
    let mooshie_dir = custom_nodes.join("mooshie-nodes");
    std::fs::create_dir_all(&mooshie_dir).map_err(|e| {
        format!(
            "Failed to create mooshie-nodes directory at '{}': {}",
            mooshie_dir.display(),
            e
        )
    })?;

    let init_path = mooshie_dir.join("__init__.py");
    std::fs::write(&init_path, MOOSHIE_NODES_INIT).map_err(|e| {
        format!(
            "Failed to write mooshie-nodes/__init__.py at '{}': {}",
            init_path.display(),
            e
        )
    })?;

    // ── Tiled Diffusion node (required for upscale mode) ─────────────────────
    // Deployed as a top-level file so ComfyUI's comfy_entrypoint discovery works.
    let tiled_path = custom_nodes.join("nodes_tiled_diffusion.py");
    std::fs::write(&tiled_path, TILED_DIFFUSION_PY).map_err(|e| {
        format!(
            "Failed to write nodes_tiled_diffusion.py at '{}': {}",
            tiled_path.display(),
            e
        )
    })?;

    // ── Guidance nodes (Soft Guidance + Smart Guidance) ──────────────────────
    let guidance_path = custom_nodes.join("nodes_guidance.py");
    std::fs::write(&guidance_path, GUIDANCE_PY).map_err(|e| {
        format!(
            "Failed to write nodes_guidance.py at '{}': {}",
            guidance_path.display(),
            e
        )
    })?;

    // ── SDXL Flux2VAE ComfyUI Node (required for Mugen/Flux2VAE SDXL models) ─
    // Deployed as a single flat .py file (not a package) so that `import nodes` inside
    // the file resolves unambiguously to ComfyUI's root nodes.py, avoiding a circular
    // import that occurs when using a package with its own nodes.py submodule.
    // Any stale package directory from a previous deployment is removed first.
    let flux2vae_stale_dir = custom_nodes.join("sdxl-flux2vae-comfyui-node");
    if flux2vae_stale_dir.exists() {
        std::fs::remove_dir_all(&flux2vae_stale_dir).map_err(|e| {
            format!(
                "Failed to remove stale sdxl-flux2vae-comfyui-node directory: {}",
                e
            )
        })?;
    }

    let flux2vae_path = custom_nodes.join("nodes_sdxl_flux2vae.py");
    std::fs::write(&flux2vae_path, SDXL_FLUX2VAE_PY).map_err(|e| {
        format!(
            "Failed to write nodes_sdxl_flux2vae.py at '{}': {}",
            flux2vae_path.display(),
            e
        )
    })?;

    // ── Nanosaur custom node package (NanoSaurLoader) ────────────────────────
    let nanosaur_dir = custom_nodes.join("nanosaur_support");
    std::fs::create_dir_all(&nanosaur_dir).map_err(|e| {
        format!(
            "Failed to create nanosaur_support directory at '{}': {}",
            nanosaur_dir.display(),
            e
        )
    })?;

    for (name, content) in [
        ("__init__.py", NANOSAUR_INIT_PY),
        ("nodes.py", NANOSAUR_NODES_PY),
        ("model.py", NANOSAUR_MODEL_PY),
        ("text_encoder.py", NANOSAUR_TEXT_ENCODER_PY),
        ("vae.py", NANOSAUR_VAE_PY),
    ] {
        let path = nanosaur_dir.join(name);
        std::fs::write(&path, content).map_err(|e| {
            format!(
                "Failed to write nanosaur_support/{} at '{}': {}",
                name,
                path.display(),
                e
            )
        })?;
    }

    log::info!(
        "Deployed mooshie custom nodes to {}",
        custom_nodes.display()
    );
    Ok(())
}

/// Ensure all ControlNet custom-node packages used by MooshieUI presets are
/// present before ComfyUI boots. Requirements are installed once per
/// requirements.txt content hash, then reinstalled only if the file changes.
pub async fn ensure_required_controlnet_nodes(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
) -> Result<(), String> {
    let custom_nodes = Path::new(comfyui_path).join("custom_nodes");
    std::fs::create_dir_all(&custom_nodes).map_err(|e| {
        format!(
            "Failed to create ComfyUI custom_nodes directory at '{}': {}",
            custom_nodes.display(),
            e
        )
    })?;

    for package in REQUIRED_CONTROLNET_PACKAGES {
        ensure_custom_node_package(&custom_nodes, venv_path, network_proxy, *package).await?;
    }

    log::info!("Ensured required ControlNet custom node packages");
    Ok(())
}

/// Verify that ComfyUI actually loaded every custom node class required by the
/// built-in ControlNet presets. Directory presence alone is not enough because
/// custom-node import failures leave the class missing from /object_info.
pub async fn verify_required_controlnet_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<(), String> {
    let mut missing = Vec::new();

    for attempt in 0..5 {
        missing = missing_required_controlnet_nodes(http_client, base_url).await?;
        if missing.is_empty() {
            log::info!("Verified required ControlNet custom node classes");
            return Ok(());
        }

        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Err(format!(
        "{}: {}. Check the ComfyUI log for custom-node import errors.",
        MISSING_CONTROLNET_NODES_MARKER,
        missing.join(", ")
    ))
}

/// Verify that ComfyUI loaded the MooshieUI custom node classes required by
/// every generated workflow. If ComfyUI was already running when nodes were
/// deployed to disk, the files exist but /object_info will still be missing
/// these classes until the server is restarted.
pub async fn verify_required_mooshie_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<(), String> {
    let mut missing = Vec::new();

    for attempt in 0..5 {
        missing = missing_mooshie_nodes(http_client, base_url).await?;
        if missing.is_empty() {
            log::info!("Verified required MooshieUI custom node classes");
            return Ok(());
        }

        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Err(format_missing_mooshie_nodes_error(&missing))
}

/// Build the user-facing missing-nodes error, including a ComfyUI log excerpt when available.
pub fn format_missing_mooshie_nodes_error(missing: &[String]) -> String {
    let mut msg = format!(
        "This ComfyUI server {}: {}. If MooshieUI just installed or updated the nodes, fully stop ComfyUI/python.exe, then start MooshieUI again so the custom nodes load. If this is a remote or external ComfyUI server, install the MooshieUI custom nodes there and restart that server.",
        MISSING_MOOSHIE_NODES_MARKER,
        missing.join(", ")
    );
    if let Some(log) = super::process::read_comfyui_log_tail(25) {
        msg.push_str("\n\nComfyUI log (last lines):\n");
        msg.push_str(&log);
    }
    msg
}

pub fn is_missing_mooshie_nodes_error(message: &str) -> bool {
    message.contains(MISSING_MOOSHIE_NODES_MARKER)
}

/// Parse missing node class names from a formatted missing-nodes error.
pub fn parse_missing_nodes_from_error(message: &str) -> Vec<String> {
    let Some(start) = message.find(MISSING_MOOSHIE_NODES_MARKER) else {
        return Vec::new();
    };
    let rest = &message[start + MISSING_MOOSHIE_NODES_MARKER.len()..];
    let Some(colon) = rest.find(':') else {
        return Vec::new();
    };
    let after = rest[colon + 1..].trim_start();
    let end = after.find('.').unwrap_or(after.len());
    after[..end]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// JSON payload for `comfyui:server_error` / startup failures.
pub fn server_error_payload(error: &str, port: u16) -> serde_json::Value {
    let missing_nodes = parse_missing_nodes_from_error(error);
    let kind = if !missing_nodes.is_empty() {
        "missing_mooshie_nodes"
    } else if error.contains(MISSING_CONTROLNET_NODES_MARKER) {
        "missing_controlnet_nodes"
    } else if error.contains("exited with") || error.contains("process exited") {
        "crashed"
    } else {
        "generic"
    };
    let log_excerpt = super::process::read_comfyui_log_tail(25);
    serde_json::json!({
        "error": error,
        "kind": kind,
        "missing_nodes": missing_nodes,
        "log_excerpt": log_excerpt,
        "port": port,
    })
}

async fn ensure_custom_node_package(
    custom_nodes: &Path,
    venv_path: &str,
    network_proxy: Option<&str>,
    package: RequiredCustomNodePackage,
) -> Result<(), String> {
    let target_dir = custom_nodes.join(package.name);

    if target_dir.exists() && !target_dir.is_dir() {
        return Err(format!(
            "Cannot install required custom node '{}': '{}' exists but is not a directory",
            package.name,
            target_dir.display()
        ));
    }

    if !target_dir.exists() {
        log::info!(
            "Installing required custom node '{}' from {}",
            package.name,
            package.git_url
        );
        clone_custom_node(package.git_url, &target_dir, network_proxy).await?;
    }

    let requirements = target_dir.join("requirements.txt");
    if requirements.exists() {
        install_requirements_if_needed(
            &requirements,
            &target_dir,
            venv_path,
            network_proxy,
            package.name,
        )
        .await?;
    }

    Ok(())
}

pub(crate) fn apply_network_proxy(cmd: &mut tokio::process::Command, network_proxy: Option<&str>) {
    if let Some(proxy) = network_proxy.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.env("HTTP_PROXY", proxy)
            .env("HTTPS_PROXY", proxy)
            .env("ALL_PROXY", proxy);
    }
}

async fn clone_custom_node(
    git_url: &str,
    target_dir: &Path,
    network_proxy: Option<&str>,
) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["clone", "--depth=1", git_url]).arg(target_dir);
    apply_network_proxy(&mut cmd, network_proxy);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("git clone failed to start for {}: {}", git_url, e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git clone failed for {}: {}",
            git_url,
            command_output_excerpt(&output)
        ))
    }
}

async fn install_requirements_if_needed(
    requirements: &Path,
    target_dir: &Path,
    venv_path: &str,
    network_proxy: Option<&str>,
    package_name: &str,
) -> Result<(), String> {
    let req_hash = file_sha256(requirements)?;
    let stamp_path = target_dir.join(".mooshieui-requirements.sha256");
    if std::fs::read_to_string(&stamp_path)
        .map(|s| s.trim() == req_hash)
        .unwrap_or(false)
    {
        return Ok(());
    }

    if venv_path.trim().is_empty() {
        return Err(format!(
            "Cannot install requirements for {}: ComfyUI venv_path is empty",
            package_name
        ));
    }

    log::info!(
        "Installing requirements for required custom node {}",
        package_name
    );

    let mut command = if let Some(uv_path) = find_uv_bin(venv_path).await {
        let mut command = tokio::process::Command::new(uv_path);
        command
            .args(["pip", "install", "-r"])
            .arg(requirements)
            .env("VIRTUAL_ENV", venv_path);
        command
    } else {
        let mut command = tokio::process::Command::new(resolve_pip_bin(venv_path));
        command.args(["install", "-r"]).arg(requirements);
        command
    };

    apply_network_proxy(&mut command, network_proxy);
    let output = command
        .output()
        .await
        .map_err(|e| format!("Failed to install requirements for {}: {}", package_name, e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to install requirements for {}: {}",
            package_name,
            command_output_excerpt(&output)
        ));
    }

    std::fs::write(&stamp_path, req_hash).map_err(|e| {
        format!(
            "Failed to write requirements stamp for {} at '{}': {}",
            package_name,
            stamp_path.display(),
            e
        )
    })?;

    Ok(())
}

async fn find_uv_bin(venv_path: &str) -> Option<PathBuf> {
    let base = Path::new(venv_path)
        .parent()
        .unwrap_or(Path::new(venv_path));

    #[cfg(target_os = "windows")]
    let local_uv = base.join("bin").join("uv.exe");
    #[cfg(not(target_os = "windows"))]
    let local_uv = base.join("bin").join("uv");

    if local_uv.exists() {
        return Some(local_uv);
    }

    let global_uv = PathBuf::from("uv");
    let status = tokio::process::Command::new(&global_uv)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    match status {
        Ok(status) if status.success() => Some(global_uv),
        _ => None,
    }
}

fn resolve_pip_bin(venv_path: &str) -> PathBuf {
    let venv_base = Path::new(venv_path);
    #[cfg(target_os = "windows")]
    {
        venv_base.join("Scripts").join("pip.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        venv_base.join("bin").join("pip")
    }
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| {
        format!(
            "Failed to read requirements file '{}': {}",
            path.display(),
            e
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn command_output_excerpt(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let lines: Vec<&str> = combined.lines().collect();
    let start = lines.len().saturating_sub(20);
    let excerpt = lines[start..].join("\n");
    if excerpt.trim().is_empty() {
        format!("process exited with {}", output.status)
    } else {
        excerpt
    }
}

async fn missing_required_controlnet_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<Vec<String>, String> {
    let mut missing = Vec::new();

    for package in REQUIRED_CONTROLNET_PACKAGES {
        for node_class in package.verify_nodes {
            if !object_info_has_node_class(http_client, base_url, node_class).await? {
                missing.push(format!("{} ({})", node_class, package.name));
            }
        }
    }

    Ok(missing)
}

async fn missing_mooshie_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<Vec<String>, String> {
    let mut missing = Vec::new();

    for node_class in REQUIRED_MOOSHIE_NODE_CLASSES {
        if !object_info_has_node_class(http_client, base_url, node_class).await? {
            missing.push((*node_class).to_string());
        }
    }

    Ok(missing)
}

async fn object_info_has_node_class(
    http_client: &reqwest::Client,
    base_url: &str,
    node_class: &str,
) -> Result<bool, String> {
    let base_url = base_url.trim_end_matches('/');
    let url = format!("{}/object_info/{}", base_url, node_class);
    match http_client.get(&url).send().await {
        Ok(response) if response.status().is_success() => {
            let value = response
                .json::<serde_json::Value>()
                .await
                .map_err(|e| format!("Failed to parse object_info for {}: {}", node_class, e))?;
            Ok(value.get(node_class).is_some())
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_missing_mooshie_nodes_from_error() {
        let err = format_missing_mooshie_nodes_error(&[
            "MooshieSaveImage".to_string(),
            "ApplyTiledDiffusion".to_string(),
        ]);
        let parsed = parse_missing_nodes_from_error(&err);
        assert_eq!(parsed.len(), 2);
        assert!(parsed.contains(&"MooshieSaveImage".to_string()));
    }

    #[test]
    fn server_error_payload_mooshie_kind() {
        let err = format_missing_mooshie_nodes_error(&["MooshieSaveImage".to_string()]);
        let payload = server_error_payload(&err, 8188);
        assert_eq!(payload["kind"], "missing_mooshie_nodes");
        assert_eq!(payload["port"], 8188);
        assert!(payload["missing_nodes"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn server_error_payload_controlnet_kind() {
        let err = format!(
            "{}: CannyEdgePreprocessor. Check the ComfyUI log for custom-node import errors.",
            MISSING_CONTROLNET_NODES_MARKER
        );
        let payload = server_error_payload(&err, 8188);
        assert_eq!(payload["kind"], "missing_controlnet_nodes");
    }

    #[test]
    fn server_error_payload_crashed_kind() {
        let err = "ComfyUI process exited with exit code: 1";
        let payload = server_error_payload(err, 8188);
        assert_eq!(payload["kind"], "crashed");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_netstat_listening_pid_exact_port() {
        use crate::comfyui::process::parse_netstat_listening_pid;

        let line = "  TCP    0.0.0.0:8188           0.0.0.0:0              LISTENING       4242";
        assert_eq!(parse_netstat_listening_pid(line, 8188), Some(4242));
        assert_eq!(parse_netstat_listening_pid(line, 18188), None);
    }
}
