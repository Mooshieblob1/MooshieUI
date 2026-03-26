#!/usr/bin/env bash
# MooshieUI - ComfyUI Node Installer
# Installs the custom nodes required by MooshieUI into your ComfyUI installation.
#
# Usage: ./install.sh /path/to/ComfyUI

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ -z "$1" ]; then
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

echo "Installing MooshieUI nodes into: $COMFYUI_PATH"

# 1. Copy the tiled diffusion node into custom_nodes/
# ComfyUI auto-discovers all .py files in custom_nodes/ and supports
# the comfy_entrypoint extension API used by this node.
echo "  → Copying nodes_tiled_diffusion.py to custom_nodes/"
cp "$SCRIPT_DIR/nodes_tiled_diffusion.py" "$COMFYUI_PATH/custom_nodes/nodes_tiled_diffusion.py"

# 2. Copy blueprint
echo "  → Copying blueprint to blueprints/"
mkdir -p "$COMFYUI_PATH/blueprints"
cp "$SCRIPT_DIR/Image Tiled Upscale (img2img).json" "$COMFYUI_PATH/blueprints/"

echo ""
echo "✅ Installation complete!"
echo ""
echo "Restart ComfyUI to load the new nodes."
