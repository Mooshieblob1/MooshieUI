//! Auto-deploy bundled MooshieUI custom nodes into ComfyUI's custom_nodes directory.
//! The Python source is embedded at compile time and written to disk before ComfyUI starts.

use std::path::Path;

const MOOSHIE_NODES_INIT: &str = include_str!("mooshie_nodes.py");
const TILED_DIFFUSION_PY: &str = include_str!("../../../comfyui-nodes/nodes_tiled_diffusion.py");
const GUIDANCE_PY: &str = include_str!("../../../comfyui-nodes/nodes_guidance.py");
/// Combined flat file: nodes.py content + NODE_CLASS_MAPPINGS.
/// Deployed as a single top-level file to avoid the circular import that occurs when a
/// package named `nodes.py` tries to `import nodes` (ComfyUI's own nodes.py) while
/// ComfyUI's nodes.py is still being initialized.
const SDXL_FLUX2VAE_PY: &str =
    include_str!("../../../comfyui-nodes/nodes_sdxl_flux2vae_combined.py");

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

    log::info!(
        "Deployed mooshie custom nodes to {}",
        custom_nodes.display()
    );
    Ok(())
}
