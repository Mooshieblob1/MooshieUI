//! Auto-deploy bundled MooshieUI custom nodes into ComfyUI's custom_nodes directory.
//! The Python source is embedded at compile time and written to disk before ComfyUI starts.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::process::tokio_command_no_window;
use super::types::GenerationParams;

#[derive(Clone, Copy)]
struct RequiredCustomNodePackage {
    name: &'static str,
    git_url: &'static str,
    verify_nodes: &'static [&'static str],
    /// Requirements file to pip-install from, relative to the package root.
    /// Almost every pack ships `requirements.txt`, but not all
    /// (ComfyUI-Frame-Interpolation splits its deps into with/without-cupy
    /// variants), so the name is per-package rather than hardcoded.
    requirements_file: &'static str,
}

const STYLE_TRANSFER_PACKAGES: &[RequiredCustomNodePackage] = &[
    RequiredCustomNodePackage {
        name: "ComfyUi-Untwisting-RoPE",
        git_url: "https://github.com/BigStationW/ComfyUi-Untwisting-RoPE.git",
        verify_nodes: &["RFInversion", "UntwistingRoPE"],
        requirements_file: "requirements.txt",
    },
    RequiredCustomNodePackage {
        name: "ComfyUi-Scale-Image-to-Total-Pixels-Advanced",
        git_url: "https://github.com/BigStationW/ComfyUi-Scale-Image-to-Total-Pixels-Advanced.git",
        verify_nodes: &["ImageScaleToTotalPixelsX"],
        requirements_file: "requirements.txt",
    },
];

// GGUF-quantized diffusion models / text encoders cannot be loaded by core
// ComfyUI loaders; the workflow builder emits UnetLoaderGGUF / CLIPLoaderGGUF
// for .gguf files, which this package provides.
const GGUF_PACKAGES: &[RequiredCustomNodePackage] = &[RequiredCustomNodePackage {
    name: "ComfyUI-GGUF",
    git_url: "https://github.com/city96/ComfyUI-GGUF.git",
    verify_nodes: &["UnetLoaderGGUF", "CLIPLoaderGGUF"],
    requirements_file: "requirements.txt",
}];

// Anima ControlNet-LLLite is deliberately absent: ComfyUI ships
// `ModelPatchLoader` + `AnimaLLLiteApply` in core (comfy_extras/nodes_model_patch.py)
// since v0.29.0, and `load_custom_node` ignores custom classes that shadow core
// names, so installing kohya-ss/ComfyUI-Anima-LLLite could never take effect and
// only made the app emit the unreachable third-party input shape (#522).
const REQUIRED_CONTROLNET_PACKAGES: &[RequiredCustomNodePackage] = &[RequiredCustomNodePackage {
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
    requirements_file: "requirements.txt",
}];

// RIFE 2x frame interpolation for video generation. Installed lazily from the
// video settings panel rather than at startup: the pack is only useful in video
// mode, and its checkpoint is a separate ~20 MB download.
const RIFE_PACKAGE_DIR: &str = "ComfyUI-Frame-Interpolation";

const RIFE_PACKAGES: &[RequiredCustomNodePackage] = &[RequiredCustomNodePackage {
    name: RIFE_PACKAGE_DIR,
    git_url: "https://github.com/Fannovel16/ComfyUI-Frame-Interpolation.git",
    verify_nodes: &["RIFE VFI"],
    // The pack ships no requirements.txt. `requirements-with-cupy.txt` pulls a
    // multi-hundred-MB CUDA-version-specific cupy wheel, which only the
    // sepconv/GMFSS backends need; RIFE's arch imports torch only, and the ops
    // backend is imported lazily inside each model's `vfi()` method.
    requirements_file: "requirements-no-cupy.txt",
}];

/// Checkpoint the `RIFE VFI` node is driven with. 2x interpolation of 24 fps
/// H3 output to 48 fps.
pub const RIFE_CKPT_FILENAME: &str = "rife49.pth";

/// Where to fetch the checkpoint from, tried in order. Downloading it up front
/// keeps the install inside the app's progress UI instead of stalling
/// mid-generation with no feedback.
///
/// These mirror the pack's own `BASE_MODEL_DOWNLOAD_URLS` plus its
/// `CKPT_FALLBACK_URLS` table. The pack's first-choice base
/// (styler00dollar/VSGAN-tensorrt-docker) now returns 404 for every checkpoint,
/// which is why that table exists at all, so a single hardcoded URL is not
/// survivable here either. Every mirror below serves a byte-identical
/// 21,345,274-byte file.
pub const RIFE_CKPT_URLS: &[&str] = &[
    "https://github.com/Fannovel16/ComfyUI-Frame-Interpolation/releases/download/models/rife49.pth",
    "https://huggingface.co/marduk191/rife/resolve/main/rife49.pth",
    "https://huggingface.co/MachineDelusions/RIFE/resolve/main/rife49.pth",
    "https://huggingface.co/Isi99999/Frame_Interpolation_Models/resolve/main/rife49.pth",
    "https://huggingface.co/hfmaster/models-moved/resolve/main/rife/rife49.pth",
];

/// Smallest plausible size for the checkpoint. A mirror that has been replaced
/// by an HTML error page or an LFS pointer still answers 200, so size is the
/// cheap guard against renaming junk into place and failing cryptically at
/// generation time instead.
const RIFE_CKPT_MIN_BYTES: u64 = 16 * 1024 * 1024;

// MiniMax-H3 Turbo LoRA nodes, installed lazily from the video settings panel
// for the same reason as RIFE. The adapter file itself is not downloaded here:
// it lives in `models/loras/` and goes through the regular model downloader, so
// it gets the same per-file progress UI as the rest of the H3 stack.
const H3_TURBO_PACKAGE_DIR: &str = "ComfyUI-MiniMax-H3-Turbo";

const H3_TURBO_PACKAGES: &[RequiredCustomNodePackage] = &[RequiredCustomNodePackage {
    name: H3_TURBO_PACKAGE_DIR,
    git_url: "https://github.com/Larryvrh/ComfyUI-MiniMax-H3-Turbo.git",
    verify_nodes: &["MiniMaxH3TurboLoRA", "MiniMaxH3TurboSampler"],
    // The pack ships no requirements file at all — it imports torch and comfy's
    // own modules only, so the clone is the entire install.
    // `ensure_custom_node_package` skips the pip stage when the file is absent.
    requirements_file: "requirements.txt",
}];

/// Turbo adapter filename, as served by `larryvrh/MiniMax-H3-Turbo-Lora`.
///
/// The repo publishes several checkpoints; v4/step600 EMA is the maintainer's
/// recommended one and the only one the app offers. Consumed by
/// `templates::video` as the default `lora_name`.
pub const H3_TURBO_LORA_FILENAME: &str = "minimax_h3_turbo_v4_step600_ema.safetensors";

/// Where the frontend downloads [`H3_TURBO_LORA_FILENAME`] from.
pub const H3_TURBO_LORA_URL: &str =
    "https://huggingface.co/larryvrh/MiniMax-H3-Turbo-Lora/resolve/main/minimax_h3_turbo_v4_step600_ema.safetensors";

// MiniMax-H3 TeaCache, installed lazily from the video settings panel for the
// same reason as Turbo: opt-in, video-only, and no reason to clone it at every
// startup for users who never touch the toggle.
const H3_TEACACHE_PACKAGE_DIR: &str = "ComfyUI-MiniMaxH3-TeaCache";

const H3_TEACACHE_PACKAGES: &[RequiredCustomNodePackage] = &[RequiredCustomNodePackage {
    name: H3_TEACACHE_PACKAGE_DIR,
    git_url: "https://github.com/Icyoung/ComfyUI-MiniMaxH3-TeaCache.git",
    verify_nodes: &["MiniMaxH3TeaCache"],
    // The pack's pyproject.toml declares no dependencies beyond ComfyUI itself
    // (pure torch, already provided by the host), and it ships no
    // requirements.txt at all — the clone is the entire install.
    // `ensure_custom_node_package` skips the pip stage when the file is absent.
    requirements_file: "requirements.txt",
}];

/// Substring present in [`format_missing_mooshie_nodes_error`] output.
pub const MISSING_MOOSHIE_NODES_MARKER: &str = "has not loaded required MooshieUI custom nodes";

/// Substring present in [`verify_required_controlnet_nodes`] error output.
pub const MISSING_CONTROLNET_NODES_MARKER: &str = "Required ControlNet custom nodes failed to load";

/// Substring present in [`verify_required_style_transfer_nodes`] error output.
pub const MISSING_STYLE_TRANSFER_NODES_MARKER: &str =
    "Required style transfer custom nodes failed to load";

/// Substring present in [`verify_required_gguf_nodes`] error output.
pub const MISSING_GGUF_NODES_MARKER: &str = "Required GGUF custom nodes failed to load";

/// Substring present in [`verify_required_rife_nodes`] error output.
pub const MISSING_RIFE_NODES_MARKER: &str = "Required RIFE custom nodes failed to load";

/// Substring present in [`verify_required_h3_director_nodes`] error output.
pub const MISSING_H3_DIRECTOR_NODES_MARKER: &str =
    "Required MiniMax H3 Director custom nodes failed to load";

/// Substring present in [`verify_required_h3_native_nodes`] error output.
pub const MISSING_H3_NATIVE_NODES_MARKER: &str = "Required MiniMax H3 video node failed to load";

const REQUIRED_MOOSHIE_NODE_CLASSES: &[&str] = &[
    "MooshieSaveImage",
    "MooshieSaveVideo",
    "MooshieLoadVideoPath",
    "MooshieFaceDetailer",
    "MooshieSegmentDetailer",
    "MooshieSoftGuidance",
    "MooshieSmartGuidance",
    "MooshieCheckpointLoaderPath",
    "MooshieDiffusionLoaderPath",
    "NanoSaurLoader",
    "ApplyTiledDiffusion",
    "MooshieAnimaTeaCache",
];

const MOOSHIE_NODES_INIT: &str = include_str!("mooshie_nodes.py");
// Python deps the mooshie-nodes package imports but ComfyUI does not ship.
// `ultralytics` (YOLOv8) backs the face-detailer node. torch/torchvision are
// already provided by ComfyUI's own requirements, so they are not repeated.
// Pinned per the project's supply-chain policy (docs/BOT_REVIEW_TRIAGE.md); this
// is the version verified against the bundled ComfyUI torch.
const MOOSHIE_NODES_REQUIREMENTS: &str = "ultralytics==8.4.75\n";
const TILED_DIFFUSION_PY: &str = include_str!("../../../comfyui-nodes/nodes_tiled_diffusion.py");
const GUIDANCE_PY: &str = include_str!("../../../comfyui-nodes/nodes_guidance.py");
const ANIMA_TEACACHE_PY: &str = include_str!("../../../comfyui-nodes/nodes_anima_teacache.py");
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

// ── MiniMax H3 Director package (video mode) ─────────────────────────────────
// Vendored from ComfyUI-MiniMaxH3-Director v0.1.5 (GPL-3.0); see
// comfyui-nodes/minimax_director/LICENSE. Node classes are namespaced Mooshie* so
// this copy coexists with the upstream pack if a user also installs it.
//
// Deliberately NOT in REQUIRED_MOOSHIE_NODE_CLASSES: that list gates every
// generation, including image mode, and these nodes need ComfyUI >= 0.30's
// comfy_extras/nodes_minimax_h3. A remote or older ComfyUI would otherwise fail
// image generation over a video-only node. Video mode calls
// verify_required_h3_director_nodes() instead.
const MINIMAX_DIRECTOR_INIT_PY: &str =
    include_str!("../../../comfyui-nodes/minimax_director/__init__.py");
const MINIMAX_DIRECTOR_CORE_PY: &str =
    include_str!("../../../comfyui-nodes/minimax_director/minimax_core.py");
const MINIMAX_DIRECTOR_PLAN_PY: &str =
    include_str!("../../../comfyui-nodes/minimax_director/minimax_plan.py");
const MINIMAX_DIRECTOR_MEDIA_PY: &str =
    include_str!("../../../comfyui-nodes/minimax_director/minimax_media.py");
const MINIMAX_DIRECTOR_NODE_PY: &str =
    include_str!("../../../comfyui-nodes/minimax_director/minimax_director.py");
const MINIMAX_DIRECTOR_RETAKE_PY: &str =
    include_str!("../../../comfyui-nodes/minimax_director/minimax_retake.py");
/// GPL-3.0 requires the licence text travel with the source, so it is deployed
/// alongside the modules rather than left behind in the app bundle.
const MINIMAX_DIRECTOR_LICENSE: &str =
    include_str!("../../../comfyui-nodes/minimax_director/LICENSE");

/// Node classes provided by the vendored MiniMax H3 Director package.
const REQUIRED_H3_DIRECTOR_NODE_CLASSES: &[&str] = &["MooshieH3Director", "MooshieH3RetakeStitch"];

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

    // The face-detailer node does `from ultralytics import YOLO`, which is not a
    // stock ComfyUI dependency. Declare it here so ensure_mooshie_node_requirements()
    // pip-installs it into the venv; otherwise the node fails at run time with
    // "No module named 'ultralytics'".
    let mooshie_reqs = mooshie_dir.join("requirements.txt");
    std::fs::write(&mooshie_reqs, MOOSHIE_NODES_REQUIREMENTS).map_err(|e| {
        format!(
            "Failed to write mooshie-nodes/requirements.txt at '{}': {}",
            mooshie_reqs.display(),
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

    // ── Anima TeaCache (step-caching for the Anima/Cosmos-Predict2 DiT) ──────
    let anima_teacache_path = custom_nodes.join("nodes_anima_teacache.py");
    std::fs::write(&anima_teacache_path, ANIMA_TEACACHE_PY).map_err(|e| {
        format!(
            "Failed to write nodes_anima_teacache.py at '{}': {}",
            anima_teacache_path.display(),
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

    // ── MiniMax H3 Director package (video mode timeline) ────────────────────
    // A package rather than a flat file: the modules import each other relatively
    // (`from . import minimax_plan as plan`).
    let director_dir = custom_nodes.join("minimax_director");
    std::fs::create_dir_all(&director_dir).map_err(|e| {
        format!(
            "Failed to create minimax_director directory at '{}': {}",
            director_dir.display(),
            e
        )
    })?;

    for (name, content) in [
        ("__init__.py", MINIMAX_DIRECTOR_INIT_PY),
        ("minimax_core.py", MINIMAX_DIRECTOR_CORE_PY),
        ("minimax_plan.py", MINIMAX_DIRECTOR_PLAN_PY),
        ("minimax_media.py", MINIMAX_DIRECTOR_MEDIA_PY),
        ("minimax_director.py", MINIMAX_DIRECTOR_NODE_PY),
        ("minimax_retake.py", MINIMAX_DIRECTOR_RETAKE_PY),
        ("LICENSE", MINIMAX_DIRECTOR_LICENSE),
    ] {
        let path = director_dir.join(name);
        std::fs::write(&path, content).map_err(|e| {
            format!(
                "Failed to write minimax_director/{} at '{}': {}",
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

/// Install the Python dependencies the bundled mooshie-nodes package imports
/// (currently `ultralytics`, used by the face-detailer node). Hash-stamped via
/// install_requirements_if_needed, so it only runs once per requirements.txt
/// change. Failures are non-fatal and only logged: core generation keeps
/// working, the face-detailer node stays unavailable until the install succeeds.
pub async fn ensure_mooshie_node_requirements(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
) -> Result<(), String> {
    let target_dir = Path::new(comfyui_path)
        .join("custom_nodes")
        .join("mooshie-nodes");
    let requirements = target_dir.join("requirements.txt");
    if !requirements.exists() {
        return Ok(());
    }

    if let Err(e) = install_requirements_if_needed(
        &requirements,
        &target_dir,
        venv_path,
        network_proxy,
        pip_index_url,
        "mooshie-nodes",
    )
    .await
    {
        log::warn!(
            "mooshie-nodes requirements (ultralytics) install failed (optional): {}. \
             The face-detailer node will be unavailable until this succeeds. \
             If pip timed out, set Settings → Connection → PyPI mirror URL.",
            e
        );
    }

    Ok(())
}

/// Ensure all ControlNet custom-node packages used by MooshieUI presets are
/// present before ComfyUI boots. Requirements are installed once per
/// requirements.txt content hash, then reinstalled only if the file changes.
pub async fn ensure_required_controlnet_nodes(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
) -> Result<(), String> {
    let custom_nodes = Path::new(comfyui_path).join("custom_nodes");
    std::fs::create_dir_all(&custom_nodes).map_err(|e| {
        format!(
            "Failed to create ComfyUI custom_nodes directory at '{}': {}",
            custom_nodes.display(),
            e
        )
    })?;

    let mut failures = Vec::new();
    for package in REQUIRED_CONTROLNET_PACKAGES {
        if let Err(e) = ensure_custom_node_package(
            &custom_nodes,
            venv_path,
            network_proxy,
            pip_index_url,
            *package,
        )
        .await
        {
            log::warn!(
                "ControlNet custom node '{}' setup failed (optional): {}",
                package.name,
                e
            );
            failures.push(format!("{}: {}", package.name, e));
        }
    }

    if failures.is_empty() {
        log::info!("Ensured required ControlNet custom node packages");
    } else {
        log::warn!(
            "Some ControlNet custom node packages could not be installed ({}). \
             Core MooshieUI generation still works; built-in ControlNet presets may fail until these install. \
             If pip timed out, set Settings → Connection → PyPI mirror URL (e.g. Tsinghua) in addition to any proxy.",
            failures.join("; ")
        );
    }
    Ok(())
}

/// Ensure Untwisting RoPE and related style-transfer custom nodes are present.
/// Failures are logged as warnings and do not block core startup.
pub async fn ensure_required_style_transfer_nodes(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
) -> Result<(), String> {
    let custom_nodes = Path::new(comfyui_path).join("custom_nodes");
    std::fs::create_dir_all(&custom_nodes).map_err(|e| {
        format!(
            "Failed to create ComfyUI custom_nodes directory at '{}': {}",
            custom_nodes.display(),
            e
        )
    })?;

    let mut failures = Vec::new();
    for package in STYLE_TRANSFER_PACKAGES {
        if let Err(e) = ensure_custom_node_package(
            &custom_nodes,
            venv_path,
            network_proxy,
            pip_index_url,
            *package,
        )
        .await
        {
            log::warn!(
                "Style transfer custom node '{}' setup failed (optional): {}",
                package.name,
                e
            );
            failures.push(format!("{}: {}", package.name, e));
        }
    }

    if failures.is_empty() {
        log::info!("Ensured required style transfer custom node packages");
    } else {
        log::warn!(
            "Some style transfer custom node packages could not be installed ({}). \
             Anima Untwisting RoPE style transfer is unavailable until these install.",
            failures.join("; ")
        );
    }
    Ok(())
}

/// Ensure the ComfyUI-GGUF loader nodes are present so .gguf diffusion models
/// and text encoders can be used. Failures are logged as warnings and do not
/// block core startup.
pub async fn ensure_required_gguf_nodes(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
) -> Result<(), String> {
    let custom_nodes = Path::new(comfyui_path).join("custom_nodes");
    std::fs::create_dir_all(&custom_nodes).map_err(|e| {
        format!(
            "Failed to create ComfyUI custom_nodes directory at '{}': {}",
            custom_nodes.display(),
            e
        )
    })?;

    let mut failures = Vec::new();
    for package in GGUF_PACKAGES {
        if let Err(e) = ensure_custom_node_package(
            &custom_nodes,
            venv_path,
            network_proxy,
            pip_index_url,
            *package,
        )
        .await
        {
            log::warn!(
                "GGUF custom node '{}' setup failed (optional): {}",
                package.name,
                e
            );
            failures.push(format!("{}: {}", package.name, e));
        }
    }

    if failures.is_empty() {
        log::info!("Ensured required GGUF custom node packages");
    } else {
        log::warn!(
            "Some GGUF custom node packages could not be installed ({}). \
             GGUF (.gguf) models are unavailable until these install.",
            failures.join("; ")
        );
    }
    Ok(())
}

/// Verify GGUF loader node classes (UnetLoaderGGUF / CLIPLoaderGGUF).
pub async fn verify_required_gguf_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<(), String> {
    let mut missing = Vec::new();

    for attempt in 0..5 {
        missing = missing_required_gguf_nodes(http_client, base_url).await?;
        if missing.is_empty() {
            log::info!("Verified required GGUF custom node classes");
            return Ok(());
        }

        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Err(format!(
        "{}: {}. Check the ComfyUI log for custom-node import errors.",
        MISSING_GGUF_NODES_MARKER,
        missing.join(", ")
    ))
}

/// Directory the frame-interpolation pack loads RIFE checkpoints from.
///
/// Not ComfyUI's `models/` tree: the pack resolves checkpoints relative to its
/// own source directory via `config.yaml`'s `ckpts_path: "./ckpts"`, and the
/// subdirectory is the model package name (`vfi_models/rife`).
pub fn rife_ckpt_dir(comfyui_path: &str) -> PathBuf {
    Path::new(comfyui_path)
        .join("custom_nodes")
        .join(RIFE_PACKAGE_DIR)
        .join("ckpts")
        .join("rife")
}

/// Whether the RIFE pack and its checkpoint are both present on disk.
///
/// Deliberately a disk check rather than a persisted config flag: a flag drifts
/// the moment a user deletes the pack, moves their ComfyUI install, or points
/// the app at a different one, and the failure mode is a queued generation that
/// dies on a missing node.
pub fn is_rife_installed(comfyui_path: &str) -> bool {
    if comfyui_path.trim().is_empty() {
        return false;
    }

    let package_dir = Path::new(comfyui_path)
        .join("custom_nodes")
        .join(RIFE_PACKAGE_DIR);
    if !package_dir.join("__init__.py").is_file() {
        return false;
    }

    // Written by `install_requirements_if_needed` only after pip succeeds, so
    // its presence distinguishes a finished install from a half-cloned one.
    if !package_dir.join(".mooshieui-requirements.sha256").is_file() {
        return false;
    }

    rife_ckpt_has_checkpoint(comfyui_path)
}

/// Whether a plausible RIFE checkpoint sits on disk. Size-checked rather than
/// merely present so a truncated file left by an older build cannot report the
/// install as ready and then fail at generation time.
fn rife_ckpt_has_checkpoint(comfyui_path: &str) -> bool {
    rife_ckpt_dir(comfyui_path)
        .join(RIFE_CKPT_FILENAME)
        .metadata()
        .map(|m| m.is_file() && m.len() >= RIFE_CKPT_MIN_BYTES)
        .unwrap_or(false)
}

/// Install the frame-interpolation pack that provides `RIFE VFI`.
///
/// Unlike the startup pack helpers this returns `Err` on failure instead of
/// logging a warning: it runs from a user-initiated install action, where
/// reporting success and then failing at generation time would be a lie.
pub async fn ensure_required_rife_nodes(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
) -> Result<(), String> {
    let custom_nodes = Path::new(comfyui_path).join("custom_nodes");
    std::fs::create_dir_all(&custom_nodes).map_err(|e| {
        format!(
            "Failed to create ComfyUI custom_nodes directory at '{}': {}",
            custom_nodes.display(),
            e
        )
    })?;

    for package in RIFE_PACKAGES {
        ensure_custom_node_package(
            &custom_nodes,
            venv_path,
            network_proxy,
            pip_index_url,
            *package,
        )
        .await
        .map_err(|e| format!("{}: {}", package.name, e))?;
    }

    log::info!("Ensured RIFE frame interpolation custom node package");
    Ok(())
}

/// Verify ComfyUI actually loaded the `RIFE VFI` node class.
pub async fn verify_required_rife_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<(), String> {
    let mut missing = Vec::new();

    for attempt in 0..5 {
        missing = missing_packages_nodes(http_client, base_url, RIFE_PACKAGES).await?;
        if missing.is_empty() {
            log::info!("Verified RIFE custom node classes");
            return Ok(());
        }

        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Err(format!(
        "{}: {}. Check the ComfyUI log for custom-node import errors.",
        MISSING_RIFE_NODES_MARKER,
        missing.join(", ")
    ))
}

/// Install the RIFE frame-interpolation pack and its checkpoint.
///
/// `on_progress(step, message, done)` is invoked for each stage so the desktop
/// command (Tauri emit) and the browser-mode handler (SSE broadcast) report
/// progress identically from one implementation.
pub async fn install_rife(
    http_client: &reqwest::Client,
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
    on_progress: &(dyn Fn(&str, &str, bool) + Send + Sync),
) -> Result<(), String> {
    if comfyui_path.trim().is_empty() {
        return Err("ComfyUI path is not configured".to_string());
    }

    on_progress("clone", "Installing frame interpolation nodes...", false);
    ensure_required_rife_nodes(comfyui_path, venv_path, network_proxy, pip_index_url).await?;

    let ckpt_dir = rife_ckpt_dir(comfyui_path);
    let ckpt_path = ckpt_dir.join(RIFE_CKPT_FILENAME);
    if rife_ckpt_has_checkpoint(comfyui_path) {
        on_progress("done", "RIFE frame interpolation is ready", true);
        return Ok(());
    }
    // A short file is a leftover from a failed download, not an install.
    let _ = std::fs::remove_file(&ckpt_path);

    std::fs::create_dir_all(&ckpt_dir).map_err(|e| {
        format!(
            "Failed to create RIFE checkpoint directory at '{}': {}",
            ckpt_dir.display(),
            e
        )
    })?;

    on_progress(
        "download",
        &format!("Downloading {}...", RIFE_CKPT_FILENAME),
        false,
    );
    download_rife_checkpoint(http_client, &ckpt_path, on_progress).await?;

    on_progress("done", "RIFE frame interpolation is ready", true);
    Ok(())
}

/// Download the RIFE checkpoint, falling back through [`RIFE_CKPT_URLS`] until
/// one mirror delivers it. Mirrors go stale independently (the pack's own
/// first choice already has), so a failure only surfaces once every mirror has
/// been tried, and the reported error names each one that failed.
async fn download_rife_checkpoint(
    http_client: &reqwest::Client,
    dest: &Path,
    on_progress: &(dyn Fn(&str, &str, bool) + Send + Sync),
) -> Result<(), String> {
    let mut failures: Vec<String> = Vec::new();

    for (index, url) in RIFE_CKPT_URLS.iter().enumerate() {
        if index > 0 {
            on_progress(
                "download",
                &format!(
                    "Retrying {} from mirror {} of {}...",
                    RIFE_CKPT_FILENAME,
                    index + 1,
                    RIFE_CKPT_URLS.len()
                ),
                false,
            );
        }

        match download_rife_checkpoint_from(http_client, url, dest, on_progress).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                log::warn!("RIFE checkpoint mirror {} failed: {}", url, e);
                failures.push(format!("{}: {}", url, e));
            }
        }
    }

    Err(format!(
        "Failed to download {} from any of {} mirrors.\n{}",
        RIFE_CKPT_FILENAME,
        RIFE_CKPT_URLS.len(),
        failures.join("\n")
    ))
}

/// Stream the RIFE checkpoint from one URL to a `.part` file, then rename it
/// into place so an interrupted download never leaves a truncated checkpoint
/// that [`is_rife_installed`] would report as ready.
async fn download_rife_checkpoint_from(
    http_client: &reqwest::Client,
    url: &str,
    dest: &Path,
    on_progress: &(dyn Fn(&str, &str, bool) + Send + Sync),
) -> Result<(), String> {
    use std::io::Write;

    let mut response = http_client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let partial = dest.with_extension("part");
    let mut file = std::fs::File::create(&partial)
        .map_err(|e| format!("Failed to create '{}': {}", partial.display(), e))?;

    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&partial);
                return Err(format!("transfer failed: {}", e));
            }
        };

        if let Err(e) = file.write_all(&chunk) {
            drop(file);
            let _ = std::fs::remove_file(&partial);
            return Err(format!("Failed to write '{}': {}", partial.display(), e));
        }
        downloaded += chunk.len() as u64;

        if downloaded - last_emit > 1024 * 1024 {
            last_emit = downloaded;
            // No Content-Length means no percentage; fall back to raw megabytes.
            let message = match (downloaded * 100).checked_div(total) {
                Some(percent) => format!("Downloading {} ({}%)", RIFE_CKPT_FILENAME, percent),
                None => format!(
                    "Downloading {} ({} MB)",
                    RIFE_CKPT_FILENAME,
                    downloaded >> 20
                ),
            };
            on_progress("download", &message, false);
        }
    }

    let flushed = file.flush();
    drop(file);
    if let Err(e) = flushed {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("Failed to flush '{}': {}", partial.display(), e));
    }

    if downloaded < RIFE_CKPT_MIN_BYTES {
        let _ = std::fs::remove_file(&partial);
        return Err(format!(
            "served only {} bytes, expected at least {} MB",
            downloaded,
            RIFE_CKPT_MIN_BYTES >> 20
        ));
    }

    std::fs::rename(&partial, dest).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        format!(
            "Failed to move RIFE checkpoint into '{}': {}",
            dest.display(),
            e
        )
    })?;

    Ok(())
}

/// Whether the MiniMax-H3 Turbo node pack is present.
///
/// Unlike [`is_rife_installed`] this deliberately does not look for the
/// `.mooshieui-requirements.sha256` stamp: that file is written by
/// `install_requirements_if_needed`, which never runs for a pack that ships no
/// requirements file, so requiring it would report every successful install as
/// incomplete. The adapter file is not checked here either — it lives in
/// `models/loras/` and the frontend sees it in the shared model list.
pub fn is_h3_turbo_installed(comfyui_path: &str) -> bool {
    if comfyui_path.trim().is_empty() {
        return false;
    }

    Path::new(comfyui_path)
        .join("custom_nodes")
        .join(H3_TURBO_PACKAGE_DIR)
        .join("__init__.py")
        .is_file()
}

/// Clone the MiniMax-H3 Turbo node pack into `custom_nodes/`, on demand.
pub async fn ensure_required_h3_turbo_nodes(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
) -> Result<(), String> {
    let custom_nodes = Path::new(comfyui_path).join("custom_nodes");
    std::fs::create_dir_all(&custom_nodes).map_err(|e| {
        format!(
            "Failed to create ComfyUI custom_nodes directory at '{}': {}",
            custom_nodes.display(),
            e
        )
    })?;

    for package in H3_TURBO_PACKAGES {
        ensure_custom_node_package(
            &custom_nodes,
            venv_path,
            network_proxy,
            pip_index_url,
            *package,
        )
        .await
        .map_err(|e| format!("{}: {}", package.name, e))?;
    }

    log::info!("Ensured MiniMax-H3 Turbo custom node package");
    Ok(())
}

/// Install the MiniMax-H3 Turbo node pack.
///
/// Only the node pack: the ~744 MB adapter goes to `models/loras/` through the
/// regular model downloader so it shares the per-file progress UI with the rest
/// of the H3 stack. `on_progress(step, message, done)` keeps the desktop command
/// and the browser-mode handler reporting identically from one implementation.
pub async fn install_h3_turbo(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
    on_progress: &(dyn Fn(&str, &str, bool) + Send + Sync),
) -> Result<(), String> {
    if comfyui_path.trim().is_empty() {
        return Err("ComfyUI path is not configured".to_string());
    }

    on_progress("clone", "Installing MiniMax-H3 Turbo nodes...", false);
    ensure_required_h3_turbo_nodes(comfyui_path, venv_path, network_proxy, pip_index_url).await?;

    on_progress("done", "MiniMax-H3 Turbo nodes are ready", true);
    Ok(())
}

/// Whether the MiniMax-H3 TeaCache node pack is present.
///
/// Same rationale as [`is_h3_turbo_installed`]: no requirements stamp file to
/// check because the pack installs no requirements, so directory presence is
/// the only signal available up front.
pub fn is_h3_teacache_installed(comfyui_path: &str) -> bool {
    if comfyui_path.trim().is_empty() {
        return false;
    }

    Path::new(comfyui_path)
        .join("custom_nodes")
        .join(H3_TEACACHE_PACKAGE_DIR)
        .join("__init__.py")
        .is_file()
}

/// Clone the MiniMax-H3 TeaCache node pack into `custom_nodes/`, on demand.
pub async fn ensure_required_h3_teacache_nodes(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
) -> Result<(), String> {
    let custom_nodes = Path::new(comfyui_path).join("custom_nodes");
    std::fs::create_dir_all(&custom_nodes).map_err(|e| {
        format!(
            "Failed to create ComfyUI custom_nodes directory at '{}': {}",
            custom_nodes.display(),
            e
        )
    })?;

    for package in H3_TEACACHE_PACKAGES {
        ensure_custom_node_package(
            &custom_nodes,
            venv_path,
            network_proxy,
            pip_index_url,
            *package,
        )
        .await
        .map_err(|e| format!("{}: {}", package.name, e))?;
    }

    log::info!("Ensured MiniMax-H3 TeaCache custom node package");
    Ok(())
}

/// Install the MiniMax-H3 TeaCache node pack, driven by the video settings
/// panel the first time the user enables the toggle. Emits `install:progress`
/// events with the same shape as `install_custom_node`.
pub async fn install_h3_teacache(
    comfyui_path: &str,
    venv_path: &str,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
    on_progress: &(dyn Fn(&str, &str, bool) + Send + Sync),
) -> Result<(), String> {
    if comfyui_path.trim().is_empty() {
        return Err("ComfyUI path is not configured".to_string());
    }

    on_progress("clone", "Installing MiniMax-H3 TeaCache nodes...", false);
    ensure_required_h3_teacache_nodes(comfyui_path, venv_path, network_proxy, pip_index_url)
        .await?;

    on_progress("done", "MiniMax-H3 TeaCache nodes are ready", true);
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

/// Verify style-transfer node classes (Untwisting RoPE + image scaler).
pub async fn verify_required_style_transfer_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<(), String> {
    let mut missing = Vec::new();

    for attempt in 0..5 {
        missing = missing_required_style_transfer_nodes(http_client, base_url).await?;
        if missing.is_empty() {
            log::info!("Verified required style transfer custom node classes");
            return Ok(());
        }

        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Err(format!(
        "{}: {}. Check the ComfyUI log for custom-node import errors.",
        MISSING_STYLE_TRANSFER_NODES_MARKER,
        missing.join(", ")
    ))
}

/// Verify ComfyUI loaded the vendored MiniMax H3 Director node classes.
///
/// Video-mode only. The package needs ComfyUI >= 0.30 (it delegates to
/// `comfy_extras/nodes_minimax_h3`), so an older or external server can be
/// perfectly healthy for image generation while missing these.
pub async fn verify_required_h3_director_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<(), String> {
    let mut missing = Vec::new();

    for attempt in 0..5 {
        missing.clear();
        for node_class in REQUIRED_H3_DIRECTOR_NODE_CLASSES {
            if !object_info_has_node_class(http_client, base_url, node_class).await? {
                missing.push((*node_class).to_string());
            }
        }
        if missing.is_empty() {
            log::info!("Verified MiniMax H3 Director custom node classes");
            return Ok(());
        }

        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Err(format!(
        "{}: {}. MooshieUI deploys these on start; fully stop ComfyUI/python.exe and start MooshieUI again so they load. They require ComfyUI 0.30 or newer.",
        MISSING_H3_DIRECTOR_NODES_MARKER,
        missing.join(", ")
    ))
}

/// Native ComfyUI-core node class the non-timeline video path builds into the
/// workflow for a given `video_variant` (see `templates::video::build`).
fn required_h3_native_node_class(video_variant: &str) -> &'static str {
    if video_variant == "fl2va" {
        "MiniMaxH3ImageToVideo"
    } else {
        "MiniMaxH3ReferenceToVideo"
    }
}

/// Verify ComfyUI exposes the native MiniMax H3 node the non-timeline video
/// path depends on for `video_variant`.
///
/// Unlike [`verify_required_h3_director_nodes`], this class ships inside
/// ComfyUI core itself (`comfy_extras/nodes_minimax_h3`, ComfyUI >= 0.30), not
/// a package MooshieUI deploys — so a missing class here means the fix is
/// updating ComfyUI, not restarting it.
pub async fn verify_required_h3_native_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
    video_variant: &str,
) -> Result<(), String> {
    let node_class = required_h3_native_node_class(video_variant);

    for attempt in 0..5 {
        if object_info_has_node_class(http_client, base_url, node_class).await? {
            log::info!("Verified required MiniMax H3 video node class ({node_class})");
            return Ok(());
        }
        if attempt < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    Err(format!(
        "{}: {}. This node ships inside ComfyUI itself (it is not a MooshieUI custom node), and requires ComfyUI 0.30 or newer. Open Settings and run \"Update ComfyUI\", then try again.",
        MISSING_H3_NATIVE_NODES_MARKER, node_class
    ))
}

/// Verify the MiniMax H3 video node(s) a `generate` call actually needs are
/// loaded, before the workflow is submitted to ComfyUI.
///
/// A no-op outside video mode. Video mode branches into either the vendored
/// Director package (when a timeline drives the graph) or ComfyUI's native H3
/// node for the selected variant — same 0.30-or-newer requirement either way,
/// just two different sources for the class. Both the Tauri `generate`
/// command and the LAN web server `generate` route call this so neither can
/// drift out of sync with what `templates::video::build` actually emits.
pub async fn verify_required_h3_nodes_for_generation(
    http_client: &reqwest::Client,
    base_url: &str,
    params: &GenerationParams,
) -> Result<(), String> {
    if params.mode != "video" {
        return Ok(());
    }

    let timeline_drives = params
        .video_timeline_data
        .as_deref()
        .is_some_and(|data| !data.trim().is_empty());

    if timeline_drives {
        verify_required_h3_director_nodes(http_client, base_url).await
    } else {
        verify_required_h3_native_nodes(http_client, base_url, &params.video_variant).await
    }
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
    } else if error.contains(MISSING_STYLE_TRANSFER_NODES_MARKER) {
        "missing_style_transfer_nodes"
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
    pip_index_url: Option<&str>,
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

    let requirements = target_dir.join(package.requirements_file);
    if requirements.exists() {
        install_requirements_if_needed(
            &requirements,
            &target_dir,
            venv_path,
            network_proxy,
            pip_index_url,
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

/// Apply proxy env vars and an optional PyPI index to a pip or uv pip command.
pub(crate) fn apply_pip_install_options(
    cmd: &mut tokio::process::Command,
    use_uv: bool,
    network_proxy: Option<&str>,
    pip_index_url: Option<&str>,
) {
    apply_network_proxy(cmd, network_proxy);
    if let Some(url) = pip_index_url.map(str::trim).filter(|s| !s.is_empty()) {
        if use_uv {
            cmd.arg("--index-url").arg(url);
        } else {
            cmd.args(["-i", url]);
        }
    }
}

async fn clone_custom_node(
    git_url: &str,
    target_dir: &Path,
    network_proxy: Option<&str>,
) -> Result<(), String> {
    let mut cmd = tokio_command_no_window("git");
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
    pip_index_url: Option<&str>,
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

    let uv_path = find_uv_bin(venv_path).await;
    let use_uv = uv_path.is_some();
    let mut command = if let Some(uv_path) = uv_path {
        let mut command = tokio_command_no_window(uv_path);
        command
            .args(["pip", "install", "-r"])
            .arg(requirements)
            .env("VIRTUAL_ENV", venv_path);
        command
    } else {
        let mut command = tokio_command_no_window(resolve_pip_bin(venv_path));
        command.args(["install", "-r"]).arg(requirements);
        command
    };

    apply_pip_install_options(&mut command, use_uv, network_proxy, pip_index_url);
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
    let status = tokio_command_no_window(&global_uv)
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
    missing_packages_nodes(http_client, base_url, REQUIRED_CONTROLNET_PACKAGES).await
}

async fn missing_required_style_transfer_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<Vec<String>, String> {
    missing_packages_nodes(http_client, base_url, STYLE_TRANSFER_PACKAGES).await
}

async fn missing_required_gguf_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
) -> Result<Vec<String>, String> {
    missing_packages_nodes(http_client, base_url, GGUF_PACKAGES).await
}

async fn missing_packages_nodes(
    http_client: &reqwest::Client,
    base_url: &str,
    packages: &[RequiredCustomNodePackage],
) -> Result<Vec<String>, String> {
    let mut missing = Vec::new();

    for package in packages {
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

/// Whether `/object_info/{node_class}` declares `input_name` in either its
/// required or optional section.
///
/// Returns false on any error rather than propagating one. The only caller uses
/// this to decide whether to set an optional input, and a probe that cannot
/// reach the server should degrade to omitting it, not to a failed generation.
pub async fn node_declares_input(
    http_client: &reqwest::Client,
    base_url: &str,
    node_class: &str,
    input_name: &str,
) -> bool {
    let base_url = base_url.trim_end_matches('/');
    let url = format!("{}/object_info/{}", base_url, node_class);
    let Ok(response) = http_client.get(&url).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    let Ok(value) = response.json::<serde_json::Value>().await else {
        return false;
    };
    let Some(input) = value.get(node_class).and_then(|n| n.get("input")) else {
        return false;
    };
    ["required", "optional"]
        .iter()
        .filter_map(|section| input.get(section))
        .any(|section| section.get(input_name).is_some())
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
    fn native_h3_node_class_matches_video_variant() {
        assert_eq!(
            required_h3_native_node_class("fl2va"),
            "MiniMaxH3ImageToVideo"
        );
        assert_eq!(
            required_h3_native_node_class("ref2va"),
            "MiniMaxH3ReferenceToVideo"
        );
        // Any other/unknown variant falls back to the ref2va node, matching
        // `templates::video::build`'s `else` branch (guarded upstream by
        // `validate_generation_params`, which rejects anything but the two
        // known variants before this is ever reached).
        assert_eq!(
            required_h3_native_node_class("unknown"),
            "MiniMaxH3ReferenceToVideo"
        );
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

        // Localized (German, Spanish) state column test cases
        let line_de = "  TCP    0.0.0.0:8188           0.0.0.0:0              ABHÖREN         4242";
        assert_eq!(parse_netstat_listening_pid(line_de, 8188), Some(4242));

        let line_es = "  TCP    0.0.0.0:8188           0.0.0.0:0              ESCUCHANDO      4242";
        assert_eq!(parse_netstat_listening_pid(line_es, 8188), Some(4242));

        // Established connection (foreign port is non-zero/non-asterisk) - should be ignored
        let line_est =
            "  TCP    127.0.0.1:8188         127.0.0.1:52132        ESTABLISHED     4242";
        assert_eq!(parse_netstat_listening_pid(line_est, 8188), None);
    }

    /// `templates/video.rs` sends a `metadata_json` input to `MooshieSaveVideo`.
    /// ComfyUI rejects a prompt that sets an input the node does not declare, so
    /// the two files have to agree and this is the only place that can check it.
    #[test]
    fn bundled_save_video_node_declares_metadata_json() {
        let start = MOOSHIE_NODES_INIT
            .find("class MooshieSaveVideo:")
            .expect("MooshieSaveVideo is bundled");
        let body = &MOOSHIE_NODES_INIT[start..];
        let end = body[1..]
            .find("\nclass ")
            .map(|i| i + 1)
            .unwrap_or(body.len());
        let class_source = &body[..end];

        assert!(
            class_source.contains("\"metadata_json\": (\"STRING\""),
            "MooshieSaveVideo must declare a metadata_json STRING input"
        );
        assert!(
            class_source.contains("metadata_json=\"\""),
            "save_video must default metadata_json so an older caller still works"
        );
    }
}
