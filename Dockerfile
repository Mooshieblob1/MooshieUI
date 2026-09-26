# =============================================================================
# MooshieUI Server — Multi-stage Docker Build
# =============================================================================
# Builds a headless MooshieUI server with ComfyUI + PyTorch pre-installed.
#
#   docker build -t mooshieui .
#   docker run --gpus all -p 3200:3200 -v mooshie-data:/data mooshieui
#
# Build args:
#   COMFYUI_VERSION  — Optional ComfyUI ref override (default: managed source pin)
#   TORCH_VERSION    — PyTorch version string (default: 2.7.1)
# =============================================================================

# ---------------------------------------------------------------------------
# Stage 1: Build the Svelte frontend
# ---------------------------------------------------------------------------
FROM node:20-slim AS frontend

WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts
COPY index.html svelte.config.js tsconfig.json vite.config.ts ./
COPY src/ src/
RUN npm run build

# ---------------------------------------------------------------------------
# Stage 2: Build the Rust server binary
# ---------------------------------------------------------------------------
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
# Copy Cargo manifests first for dependency caching
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/build.rs ./src-tauri/
# Create stub files so cargo can resolve the workspace
RUN mkdir -p src-tauri/src && \
    echo 'fn main() {}' > src-tauri/src/main.rs && \
    echo '' > src-tauri/src/lib.rs && \
    echo '#[tokio::main] async fn main() {}' > src-tauri/src/server_main.rs
# Copy comfyui-nodes (needed by include_str! in nodes.rs)
COPY comfyui-nodes/ comfyui-nodes/
# Pre-build dependencies
RUN cd src-tauri && \
    cargo build --release --no-default-features --features server --bin mooshieui-server 2>/dev/null || true

# Now copy the real source and build
COPY src-tauri/ src-tauri/
# The prompt-assistant grounding corpus is include_str!'d from the frontend
# asset tree (src/lib/assets/anima-tags.json), which lives outside src-tauri.
COPY src/lib/assets/anima-tags.json src/lib/assets/anima-tags.json
RUN touch src-tauri/src/lib.rs src-tauri/src/server_main.rs src-tauri/src/main.rs && \
    cd src-tauri && \
    cargo build --release --no-default-features --features server --bin mooshieui-server

# ---------------------------------------------------------------------------
# Stage 2b: CUDA-enabled llama.cpp server (prompt assistant)
# ---------------------------------------------------------------------------
# The llama.cpp GitHub release assets only ship a CPU build for Linux, so the
# app would otherwise run enhance/compose on CPU even though the host has GPUs.
# A 7B model on CPU takes >100s and trips Cloudflare's 524 timeout. Pull the
# official CUDA server image and copy its binary + ggml/llama shared libs; the
# app is pointed at them via MOOSHIEUI_LLAMA_BIN_DIR so `-ngl` offloads to the
# GPU and a generation finishes in seconds. Pinned by digest for reproducibility:
# this is `server-cuda` as of 2026-09-26 (build b11176, llama.cpp f805c57, CUDA
# 12.8.1), also tagged `server-cuda-b11176`. Bump tag and digest together.
FROM ghcr.io/ggml-org/llama.cpp:server-cuda-b11176@sha256:1f4b9cf58982dd4d7cc497aea31b1a456ca9a3a1f94f527d317d3fdee0d60ab6 AS llama

# ---------------------------------------------------------------------------
# Stage 3: Runtime with CUDA + Python + ComfyUI
# ---------------------------------------------------------------------------
FROM nvidia/cuda:12.6.3-runtime-ubuntu24.04

ARG COMFYUI_VERSION
ARG TORCH_VERSION=2.11.0

ENV DEBIAN_FRONTEND=noninteractive \
    MOOSHIEUI_DATA_DIR=/data \
    COMFYUI_PATH=/opt/comfyui \
    NVIDIA_VISIBLE_DEVICES=all \
    NVIDIA_DRIVER_CAPABILITIES=compute,utility \
    MOOSHIEUI_LLAMA_BIN_DIR=/app/llama \
    LD_LIBRARY_PATH=/app/llama:${LD_LIBRARY_PATH}

# System packages
# libgomp1 is required by the prompt-assistant llama.cpp build (OpenMP); the
# CUDA runtime base image does not ship it, so without it llama-server exits
# immediately on load with "error while loading shared libraries: libgomp.so.1"
# and every enhance/compose request 500s. libcurl4 is linked by the official
# CUDA llama-server build (HF model fetch support) and is likewise absent.
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3.12 python3.12-venv python3-pip \
    git curl ca-certificates \
    libxcb1 libglib2.0-0 libgl1 libgomp1 libcurl4 \
    && rm -rf /var/lib/apt/lists/*

# Install uv (fast Python package manager). Same pinned release and SHA-256s
# as UV_VERSION/UV_ASSETS in src-tauri/src/setup.rs, instead of piping the
# latest installer script into sh.
RUN case "$(uname -m)" in \
        x86_64) UV_ASSET=uv-x86_64-unknown-linux-gnu.tar.gz; \
                UV_SHA256=23bf5552d220e0842b65c862097b2ebaeba0064b74eda5e565e77fd25969d8c8 ;; \
        aarch64) UV_ASSET=uv-aarch64-unknown-linux-gnu.tar.gz; \
                 UV_SHA256=0804e9b164c64b6914182d5920c08551958a095986f10a3731056df701126436 ;; \
        *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;; \
    esac && \
    curl -fsSL -o /tmp/uv.tar.gz "https://github.com/astral-sh/uv/releases/download/0.12.19/${UV_ASSET}" && \
    echo "${UV_SHA256}  /tmp/uv.tar.gz" | sha256sum -c - && \
    tar -xzf /tmp/uv.tar.gz --strip-components=1 -C /usr/local/bin && \
    rm /tmp/uv.tar.gz

# Unprivileged runtime user, with fixed ids so volume ownership is predictable
# (k8s/deployment.yaml runs as the same uid and sets fsGroup to the same gid).
# The server keeps writing into ComfyUI at runtime (custom node deploys, pip
# installs into its venv, outputs), so the ComfyUI tree and /data belong to this
# user. The steps that create them run as it, rather than a chown afterwards,
# which would copy the multi-GB venv into another layer.
RUN groupadd --gid 10001 mooshie && \
    useradd --uid 10001 --gid mooshie --create-home --shell /usr/sbin/nologin mooshie && \
    install -d -o mooshie -g mooshie "${COMFYUI_PATH}" /data

# Resolve the same source as desktop; fetch also accepts immutable commit SHAs.
COPY src-tauri/src/comfyui_version.rs /tmp/mooshie-source/src-tauri/src/comfyui_version.rs
COPY src-tauri/runtime/comfyui-source.json /tmp/mooshie-source/src-tauri/runtime/comfyui-source.json
COPY scripts/comfyui-compat/resolve_ref.py /tmp/mooshie-source/scripts/comfyui-compat/resolve_ref.py
# From here on (build steps and runtime) everything runs as mooshie. Numeric so
# Kubernetes' runAsNonRoot can verify it; HOME set explicitly so uv/pip/torch
# caches land in a directory it owns whatever resolves the user.
USER 10001:10001
ENV HOME=/home/mooshie
RUN COMFYUI_SOURCE="${COMFYUI_VERSION:-$(python3 /tmp/mooshie-source/scripts/comfyui-compat/resolve_ref.py)}" && \
    git init "${COMFYUI_PATH}" && \
    git -C "${COMFYUI_PATH}" remote add origin https://github.com/Comfy-Org/ComfyUI.git && \
    git -C "${COMFYUI_PATH}" fetch --depth=1 origin "$COMFYUI_SOURCE" && \
    git -C "${COMFYUI_PATH}" reset --hard FETCH_HEAD

# Create venv and install PyTorch + ComfyUI requirements
RUN uv venv ${COMFYUI_PATH}/.venv --python python3.12 && \
    . ${COMFYUI_PATH}/.venv/bin/activate && \
    uv pip install torch==${TORCH_VERSION} torchvision torchaudio \
        --index-url https://download.pytorch.org/whl/cu128 && \
    uv pip install -r ${COMFYUI_PATH}/requirements.txt && \
    uv pip install ultralytics==8.4.34 && \
    uv pip install --force-reinstall --no-deps opencv-python-headless

# Install ControlNet custom-node packages required by MooshieUI presets before
# ComfyUI ever boots. The server also verifies these node classes after every
# restart so broken imports fail early instead of at generation time.
# Each pack is pinned to the same commit as its `git_rev` in
# src-tauri/src/comfyui/nodes.rs (the `dockerfile_node_pins_match` test keeps
# them in step). A depth-1 clone of the default branch cannot check out an
# arbitrary commit, so clone_pinned fetches exactly the pinned SHA instead.
RUN mkdir -p ${COMFYUI_PATH}/custom_nodes && \
    clone_pinned() { \
        git init -q "$3" && \
        git -C "$3" remote add origin "$1" && \
        git -C "$3" fetch --depth=1 origin "$2" && \
        git -C "$3" checkout -q --detach FETCH_HEAD; \
    } && \
    clone_pinned https://github.com/Fannovel16/comfyui_controlnet_aux.git 59b1fc411ede8623b2997855b8018f0b3b6cf49f \
        ${COMFYUI_PATH}/custom_nodes/comfyui_controlnet_aux && \
    clone_pinned https://github.com/BigStationW/ComfyUi-Untwisting-RoPE.git b62f39cd22c0d72c83af4b1da7d6c95b6f1e26f3 \
        ${COMFYUI_PATH}/custom_nodes/ComfyUi-Untwisting-RoPE && \
    clone_pinned https://github.com/BigStationW/ComfyUi-Scale-Image-to-Total-Pixels-Advanced.git 79e831097bb7a76ade3a28359300e62332086c42 \
        ${COMFYUI_PATH}/custom_nodes/ComfyUi-Scale-Image-to-Total-Pixels-Advanced && \
    . ${COMFYUI_PATH}/.venv/bin/activate && \
    for req in \
        ${COMFYUI_PATH}/custom_nodes/comfyui_controlnet_aux/requirements.txt \
        ${COMFYUI_PATH}/custom_nodes/ComfyUi-Untwisting-RoPE/requirements.txt \
        ${COMFYUI_PATH}/custom_nodes/ComfyUi-Scale-Image-to-Total-Pixels-Advanced/requirements.txt; do \
        if [ -f "$req" ]; then uv pip install -r "$req"; fi; \
    done

# Copy custom nodes (auto-deployed by the binary on startup, but also
# pre-copy them so they're available even if the binary doesn't run the
# deploy step — e.g. if ComfyUI is already running). Owned by the runtime user
# so that deploy can overwrite them.
COPY --chown=mooshie:mooshie comfyui-nodes/nodes_tiled_diffusion.py ${COMFYUI_PATH}/custom_nodes/
COPY --chown=mooshie:mooshie comfyui-nodes/nodes_guidance.py ${COMFYUI_PATH}/custom_nodes/
COPY --chown=mooshie:mooshie comfyui-nodes/nodes_sdxl_flux2vae.py ${COMFYUI_PATH}/custom_nodes/
COPY --chown=mooshie:mooshie comfyui-nodes/nodes_sdxl_flux2vae_combined.py ${COMFYUI_PATH}/custom_nodes/
COPY --chown=mooshie:mooshie comfyui-nodes/nanosaur_support/ ${COMFYUI_PATH}/custom_nodes/nanosaur_support/

# Copy server binary, frontend, and entrypoint (root-owned: read-only to the app)
COPY --from=builder /build/src-tauri/target/release/mooshieui-server /app/mooshieui-server
COPY --from=frontend /build/dist /app/dist

# CUDA llama-server + its ggml/llama shared libs for the prompt assistant. The
# official server-cuda image lays the binary and *.so out flat under /app, so
# copying that directory gives both. MOOSHIEUI_LLAMA_BIN_DIR (set above) points
# the app here, and LD_LIBRARY_PATH lets the binary find its libs. NOTE: this
# build links libcuda.so.1 (the driver stub), so the container must be run with
# GPU access (--gpus all); on a CPU-only host enhance/compose will fail to load.
COPY --from=llama /app/ /app/llama/
COPY --chmod=755 docker-entrypoint.sh /app/docker-entrypoint.sh

# Create data directory and default config (as the runtime user, so a fresh
# named volume starts out owned by it).
# Symlink ComfyUI's models directory to the persistent /data/models volume
# so that downloaded models survive container recreation.
RUN mkdir -p /data/gallery /data/thumbnails /data/models && \
    rm -rf ${COMFYUI_PATH}/models && \
    ln -s /data/models ${COMFYUI_PATH}/models && \
    echo '{"comfyui_path":"/opt/comfyui","venv_path":"/opt/comfyui/.venv","auto_start":true,"setup_complete":true,"browser_mode":true,"ui_server_port":3200,"lan_enabled":true,"server_mode":"autolaunch"}' \
    > /data/config.json

WORKDIR /app
EXPOSE 3200
VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:3200/health || exit 1

ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD ["/app/mooshieui-server"]
