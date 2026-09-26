#!/usr/bin/env bash
# MooshieUI - ComfyUI Node Installer
# Installs the custom nodes MooshieUI requires into an external ComfyUI
# installation. When MooshieUI launches ComfyUI itself it deploys these same
# files on every start (`ensure_mooshie_nodes` in src-tauri/src/comfyui/nodes.rs);
# this script must stay a mirror of that function, which the
# `install_script_tests` Rust test checks.
#
# Run it from a MooshieUI source checkout: the mooshie-nodes package source
# lives in src-tauri/src/comfyui/mooshie_nodes.py, outside this folder.
#
# Usage: ./install.sh /path/to/ComfyUI

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
MOOSHIE_NODES_INIT="$REPO_DIR/src-tauri/src/comfyui/mooshie_nodes.py"
# Must match MOOSHIE_NODES_REQUIREMENTS in src-tauri/src/comfyui/nodes.rs.
MOOSHIE_NODES_REQUIREMENTS='ultralytics==8.4.75'

if [ -z "${1:-}" ]; then
    echo "Usage: $0 /path/to/ComfyUI"
    echo ""
    echo "Example: $0 ~/ComfyUI"
    exit 1
fi

COMFYUI_PATH="$1"

if [ ! -f "$COMFYUI_PATH/nodes.py" ]; then
    echo "Error: '$COMFYUI_PATH' does not appear to be a ComfyUI installation."
    echo "Could not find nodes.py"
    exit 1
fi

# Every source file, checked up front so a partial copy never passes for an install.
SOURCES=(
    "$MOOSHIE_NODES_INIT"
    "$SCRIPT_DIR/h3_drafts.py"
    "$SCRIPT_DIR/h3_upscaler.py"
    "$SCRIPT_DIR/h3_upscaler.LICENSE"
    "$SCRIPT_DIR/h3_preview.py"
    "$SCRIPT_DIR/nodes_tiled_diffusion.py"
    "$SCRIPT_DIR/nodes_guidance.py"
    "$SCRIPT_DIR/nodes_anima_teacache.py"
    "$SCRIPT_DIR/nodes_sdxl_flux2vae_combined.py"
    "$SCRIPT_DIR/nanosaur_support/__init__.py"
    "$SCRIPT_DIR/nanosaur_support/nodes.py"
    "$SCRIPT_DIR/nanosaur_support/model.py"
    "$SCRIPT_DIR/nanosaur_support/text_encoder.py"
    "$SCRIPT_DIR/nanosaur_support/vae.py"
    "$SCRIPT_DIR/minimax_director/__init__.py"
    "$SCRIPT_DIR/minimax_director/minimax_core.py"
    "$SCRIPT_DIR/minimax_director/minimax_plan.py"
    "$SCRIPT_DIR/minimax_director/minimax_media.py"
    "$SCRIPT_DIR/minimax_director/minimax_director.py"
    "$SCRIPT_DIR/minimax_director/minimax_retake.py"
    "$SCRIPT_DIR/minimax_director/LICENSE"
    "$SCRIPT_DIR/Image Tiled Upscale (img2img).json"
)
for src in "${SOURCES[@]}"; do
    if [ ! -f "$src" ]; then
        echo "Error: missing node source file: $src" >&2
        echo "Run this script from a complete MooshieUI source checkout:" >&2
        echo "  git clone https://github.com/Mooshieblob1/MooshieUI" >&2
        echo "  MooshieUI/comfyui-nodes/install.sh $COMFYUI_PATH" >&2
        exit 1
    fi
done

CUSTOM_NODES="$COMFYUI_PATH/custom_nodes"
echo "Installing MooshieUI nodes into: $COMFYUI_PATH"
mkdir -p "$CUSTOM_NODES"

# 1. mooshie-nodes package (save/load, detailers, guidance helpers, H3 drafts
#    and live previews)
echo "  - mooshie-nodes/"
mkdir -p "$CUSTOM_NODES/mooshie-nodes"
cp "$MOOSHIE_NODES_INIT" "$CUSTOM_NODES/mooshie-nodes/__init__.py"
for name in h3_drafts.py h3_upscaler.py h3_upscaler.LICENSE h3_preview.py; do
    cp "$SCRIPT_DIR/$name" "$CUSTOM_NODES/mooshie-nodes/$name"
done
printf '%s\n' "$MOOSHIE_NODES_REQUIREMENTS" > "$CUSTOM_NODES/mooshie-nodes/requirements.txt"

# 2. Single-file nodes. ComfyUI discovers top-level .py files in custom_nodes/.
for name in nodes_tiled_diffusion.py nodes_guidance.py nodes_anima_teacache.py; do
    echo "  - $name"
    cp "$SCRIPT_DIR/$name" "$CUSTOM_NODES/$name"
done
# Deployed flat (not as a package) so its `import nodes` resolves to ComfyUI's
# own nodes.py; an older package-style copy would shadow it, so remove that.
echo "  - nodes_sdxl_flux2vae.py"
rm -rf "$CUSTOM_NODES/sdxl-flux2vae-comfyui-node"
cp "$SCRIPT_DIR/nodes_sdxl_flux2vae_combined.py" "$CUSTOM_NODES/nodes_sdxl_flux2vae.py"

# 3. Packages
for pkg in nanosaur_support minimax_director; do
    echo "  - $pkg/"
    mkdir -p "$CUSTOM_NODES/$pkg"
done
for name in __init__.py nodes.py model.py text_encoder.py vae.py; do
    cp "$SCRIPT_DIR/nanosaur_support/$name" "$CUSTOM_NODES/nanosaur_support/$name"
done
for name in __init__.py minimax_core.py minimax_plan.py minimax_media.py \
    minimax_director.py minimax_retake.py LICENSE; do
    cp "$SCRIPT_DIR/minimax_director/$name" "$CUSTOM_NODES/minimax_director/$name"
done

# 4. Blueprint (optional ComfyUI UI convenience; MooshieUI does not need it)
echo "  - blueprints/Image Tiled Upscale (img2img).json"
mkdir -p "$COMFYUI_PATH/blueprints"
cp "$SCRIPT_DIR/Image Tiled Upscale (img2img).json" "$COMFYUI_PATH/blueprints/"

echo ""
echo "Installation complete."
echo ""
echo "Next steps:"
echo "  1. Install the face detailer's Python dependency with ComfyUI's own Python:"
echo "       python -m pip install -r \"$CUSTOM_NODES/mooshie-nodes/requirements.txt\""
echo "  2. Restart ComfyUI to load the new nodes."
echo ""
echo "Feature packs from other authors (ComfyUI-GGUF, comfyui_controlnet_aux, ...)"
echo "are only installed automatically when MooshieUI manages ComfyUI. Install"
echo "the ones you need (for example with ComfyUI-Manager); MooshieUI names any"
echo "missing pack when a feature needs it."
