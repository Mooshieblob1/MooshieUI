#!/bin/sh
set -e

# ── Admin password guard ──
# Validate admin env vars and warn about the default password.
if [ -n "$MOOSHIEUI_ADMIN_USER" ] || [ -n "$MOOSHIEUI_ADMIN_PASS" ]; then
    if [ -z "$MOOSHIEUI_ADMIN_USER" ] || [ -z "$MOOSHIEUI_ADMIN_PASS" ]; then
        echo "ERROR: Both MOOSHIEUI_ADMIN_USER and MOOSHIEUI_ADMIN_PASS must be set." >&2
        exit 1
    fi
    if [ ${#MOOSHIEUI_ADMIN_PASS} -lt 4 ]; then
        echo "ERROR: MOOSHIEUI_ADMIN_PASS is too short (minimum 4 characters)." >&2
        exit 1
    fi
    if [ "$MOOSHIEUI_ADMIN_PASS" = "changeme" ]; then
        echo "" >&2
        echo "========================================================" >&2
        echo "  WARNING: Using default admin password 'changeme'." >&2
        echo "  Change MOOSHIEUI_ADMIN_PASS before exposing this" >&2
        echo "  server to the network!" >&2
        echo "========================================================" >&2
        echo "" >&2
    fi
fi

# Ensure the persistent models directory exists.
mkdir -p /data/models

# Replace ComfyUI's models/ with a symlink to the persistent volume.
# This ensures downloaded models survive container recreation.
# The symlink may have been broken by a volume overlay.
if [ ! -L "${COMFYUI_PATH}/models" ] || [ "$(readlink "${COMFYUI_PATH}/models")" != "/data/models" ]; then
    rm -rf "${COMFYUI_PATH}/models"
    ln -s /data/models "${COMFYUI_PATH}/models"
fi

# Copy default ComfyUI model subdirectory structure if the volume is fresh.
# This is needed because ComfyUI expects certain subdirectories to exist.
for subdir in checkpoints clip clip_vision configs controlnet diffusers diffusion_models embeddings gligen hypernetworks loras photomaker style_models unet upscale_models vae vae_approx ultralytics; do
    mkdir -p "/data/models/${subdir}"
done

exec "$@"
