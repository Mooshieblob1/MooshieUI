#!/bin/sh
set -e

# ── Admin password guard ──
# Validate admin env vars and refuse to start with a placeholder password (the
# server would not create the admin account, leaving a deployment nobody can
# administer).
if [ -n "$MOOSHIEUI_ADMIN_USER" ] || [ -n "$MOOSHIEUI_ADMIN_PASS" ]; then
    if [ -z "$MOOSHIEUI_ADMIN_USER" ] || [ -z "$MOOSHIEUI_ADMIN_PASS" ]; then
        echo "ERROR: Both MOOSHIEUI_ADMIN_USER and MOOSHIEUI_ADMIN_PASS must be set." >&2
        exit 1
    fi
    if [ ${#MOOSHIEUI_ADMIN_PASS} -lt 4 ]; then
        echo "ERROR: MOOSHIEUI_ADMIN_PASS is too short (minimum 4 characters)." >&2
        exit 1
    fi
    case "$(printf '%s' "$MOOSHIEUI_ADMIN_PASS" | tr '[:upper:]' '[:lower:]')" in
        changeme|replace_me)
            echo "" >&2
            echo "========================================================" >&2
            echo "  ERROR: MOOSHIEUI_ADMIN_PASS is a placeholder." >&2
            echo "  The server will not create an admin account with it." >&2
            echo "  Set a strong password before exposing this server." >&2
            echo "========================================================" >&2
            echo "" >&2
            exit 1
            ;;
    esac
fi

# ── Volume ownership ──
# The image runs as the unprivileged user mooshie (uid 10001, gid 10001). Volumes
# created by earlier images, which ran as root, are root-owned: say how to fix
# that instead of failing later on the first write.
for dir in /data /data/models; do
    if [ -e "$dir" ] && [ ! -w "$dir" ]; then
        echo "ERROR: $dir is not writable by uid $(id -u) (gid $(id -g))." >&2
        echo "  Hand the volume to the image's user once, e.g.:" >&2
        echo "    docker run --rm --user 0 -v <volume-or-host-path>:$dir --entrypoint chown <image> -R 10001:10001 $dir" >&2
        echo "  or, for a host directory: sudo chown -R 10001:10001 <host-path>" >&2
        echo "  On Kubernetes, set the pod securityContext fsGroup: 10001 (see k8s/deployment.yaml)." >&2
        exit 1
    fi
done

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
