//! Auto-deploy bundled MooshieUI custom nodes into ComfyUI's custom_nodes directory.
//! The Python source is embedded at compile time and written to disk before ComfyUI starts.

use std::path::Path;

const MOOSHIE_NODES_INIT: &str =
    include_str!("../../../../ComfyUI/custom_nodes/mooshie-nodes/__init__.py");

/// Ensure the mooshie-nodes custom node pack exists in ComfyUI's custom_nodes directory.
/// Always overwrites to keep in sync with the app version.
pub fn ensure_mooshie_nodes(comfyui_path: &str) {
    let target_dir = Path::new(comfyui_path)
        .join("custom_nodes")
        .join("mooshie-nodes");

    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        log::warn!("Failed to create mooshie-nodes directory: {}", e);
        return;
    }

    let init_path = target_dir.join("__init__.py");
    if let Err(e) = std::fs::write(&init_path, MOOSHIE_NODES_INIT) {
        log::warn!("Failed to write mooshie-nodes/__init__.py: {}", e);
    } else {
        log::info!("Deployed mooshie-nodes to {}", target_dir.display());
    }
}
