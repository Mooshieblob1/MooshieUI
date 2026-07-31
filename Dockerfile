# =============================================================================
# MooshieUI Server — Multi-stage Docker Build
# =============================================================================
# Builds a headless MooshieUI server with ComfyUI + PyTorch pre-installed.
#
#   docker build -t mooshieui .
#   docker run --gpus all -p 3200:3200 -v mooshie-data:/data mooshieui
#
# Build args:
#   COMFYUI_VERSION  — ComfyUI git tag/branch (default: master)
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
# GPU and a generation finishes in seconds. Pin a digest for reproducibility.
FROM ghcr.io/ggml-org/llama.cpp:server-cuda AS llama

# ---------------------------------------------------------------------------
# Stage 3: Runtime with CUDA + Python + ComfyUI
# ---------------------------------------------------------------------------
FROM nvidia/cuda:12.6.3-runtime-ubuntu24.04

ARG COMFYUI_VERSION=master
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

# Install uv (fast Python package manager)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh && \
    mv /root/.local/bin/uv /usr/local/bin/uv

# Clone ComfyUI
RUN git clone --depth=1 --branch ${COMFYUI_VERSION} \
    https://github.com/comfyanonymous/ComfyUI.git ${COMFYUI_PATH}

# Create venv and install PyTorch + ComfyUI requirements
RUN uv venv ${COMFYUI_PATH}/.venv --python python3.12 && \
    . ${COMFYUI_PATH}/.venv/bin/activate && \
    uv pip install torch==${TORCH_VERSION} torchvision torchaudio \
        --index-url https://download.pytorch.org/whl/cu126 && \
    uv pip install -r ${COMFYUI_PATH}/requirements.txt && \
    uv pip install ultralytics==8.4.34 && \
    uv pip install --force-reinstall --no-deps opencv-python-headless

# Install ControlNet custom-node packages required by MooshieUI presets before
# ComfyUI ever boots. The server also verifies these node classes after every
# restart so broken imports fail early instead of at generation time.
RUN mkdir -p ${COMFYUI_PATH}/custom_nodes && \
    git clone --depth=1 https://github.com/Fannovel16/comfyui_controlnet_aux.git \
        ${COMFYUI_PATH}/custom_nodes/comfyui_controlnet_aux && \
    git clone --depth=1 https://github.com/BigStationW/ComfyUi-Untwisting-RoPE.git \
        ${COMFYUI_PATH}/custom_nodes/ComfyUi-Untwisting-RoPE && \
    git clone --depth=1 https://github.com/BigStationW/ComfyUi-Scale-Image-to-Total-Pixels-Advanced.git \
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
# deploy step — e.g. if ComfyUI is already running)
COPY comfyui-nodes/nodes_tiled_diffusion.py ${COMFYUI_PATH}/custom_nodes/
COPY comfyui-nodes/nodes_guidance.py ${COMFYUI_PATH}/custom_nodes/
COPY comfyui-nodes/nodes_sdxl_flux2vae.py ${COMFYUI_PATH}/custom_nodes/
COPY comfyui-nodes/nodes_sdxl_flux2vae_combined.py ${COMFYUI_PATH}/custom_nodes/
COPY comfyui-nodes/nanosaur_support/ ${COMFYUI_PATH}/custom_nodes/nanosaur_support/

# Copy server binary, frontend, and entrypoint
COPY --from=builder /build/src-tauri/target/release/mooshieui-server /app/mooshieui-server
COPY --from=frontend /build/dist /app/dist

# CUDA llama-server + its ggml/llama shared libs for the prompt assistant. The
# official server-cuda image lays the binary and *.so out flat under /app, so
# copying that directory gives both. MOOSHIEUI_LLAMA_BIN_DIR (set above) points
# the app here, and LD_LIBRARY_PATH lets the binary find its libs. NOTE: this
# build links libcuda.so.1 (the driver stub), so the container must be run with
# GPU access (--gpus all); on a CPU-only host enhance/compose will fail to load.
COPY --from=llama /app/ /app/llama/
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh

# Create data directory and default config.
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
