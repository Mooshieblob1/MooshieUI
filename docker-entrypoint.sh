#!/bin/sh
set -e

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
