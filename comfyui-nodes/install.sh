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

# 1. Copy the tiled diffusion node
echo "  → Copying nodes_tiled_diffusion.py to comfy_extras/"
cp "$SCRIPT_DIR/nodes_tiled_diffusion.py" "$COMFYUI_PATH/comfy_extras/nodes_tiled_diffusion.py"

# 2. Copy blueprint
echo "  → Copying blueprint to blueprints/"
mkdir -p "$COMFYUI_PATH/blueprints"
cp "$SCRIPT_DIR/Image Tiled Upscale (img2img).json" "$COMFYUI_PATH/blueprints/"

# 3. Register the node in nodes.py if not already registered
if grep -q "nodes_tiled_diffusion.py" "$COMFYUI_PATH/nodes.py"; then
    echo "  → nodes_tiled_diffusion.py already registered in nodes.py"
else
    echo "  → Registering nodes_tiled_diffusion.py in nodes.py"
    sed -i 's/"nodes_upscale_model.py",/"nodes_upscale_model.py",\n        "nodes_tiled_diffusion.py",/' "$COMFYUI_PATH/nodes.py"
fi

echo ""
echo "✅ Installation complete!"
echo ""
echo "Restart ComfyUI to load the new nodes."
