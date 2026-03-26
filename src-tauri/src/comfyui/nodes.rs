//! Auto-deploy bundled MooshieUI custom nodes into ComfyUI's custom_nodes directory.
//! The Python source is embedded at compile time and written to disk before ComfyUI starts.

use std::path::Path;

const MOOSHIE_NODES_INIT: &str = include_str!("mooshie_nodes.py");
const TILED_DIFFUSION_PY: &str = include_str!("../../../comfyui-nodes/nodes_tiled_diffusion.py");

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

    log::info!(
        "Deployed mooshie custom nodes to {}",
        custom_nodes.display()
    );
    Ok(())
}
