# Build environment for Anthropic's OSS Scanner (https://red.anthropic.com/oss-scanner).
# The scanner builds this with the repository as the build context and network
# access, then audits the finished image fully offline, so everything the build
# and the tests need is fetched here.
#
# Covers the Svelte frontend, both Rust builds of src-tauri (desktop and the
# headless server), the Rust test suite, and report-proxy. The runtime pieces
# that deploy/Dockerfile adds (CUDA, PyTorch, ComfyUI, llama.cpp) are left out:
# they are third-party and not what the scanner should audit.
#
# Try it locally:
#   docker build -f .oss-scanner/Dockerfile -t mooshieui-oss-scanner .
#   docker run --rm -it --network none mooshieui-oss-scanner
FROM node:20-bookworm-slim AS node

FROM rust:1-bookworm

ENV DEBIAN_FRONTEND=noninteractive

# Same Linux packages as the desktop job in .github/workflows/rust-checks.yml.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates git pkg-config python3 \
      libssl-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
      libappindicator3-dev librsvg2-dev libgtk-3-dev libsoup-3.0-dev \
    && rm -rf /var/lib/apt/lists/*

COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && rustup component add rustfmt clippy

# scanner contract: the checkout lives inside the image, at /src
COPY . /src
WORKDIR /src

# Frontend. --ignore-scripts matches deploy/Dockerfile; the build output in
# dist/ is also what tauri::generate_context! needs at Rust compile time.
RUN npm ci --ignore-scripts && npm run build

# Fetch every crate up front so later cargo commands work offline.
RUN cargo fetch --locked --manifest-path src-tauri/Cargo.toml \
    && cargo fetch --locked --manifest-path report-proxy/Cargo.toml

# Desktop build (default features) and the headless server build. Both are
# debug builds, which keep debug info and overflow checks.
RUN cargo build --locked --manifest-path src-tauri/Cargo.toml --all-targets \
    && cargo build --locked --manifest-path src-tauri/Cargo.toml \
         --no-default-features --features server --bin mooshieui-server \
    && cargo build --locked --manifest-path report-proxy/Cargo.toml --all-targets

# Run the tests once so the image is known good, without failing the build on
# a test failure. Re-run offline with:
#   cargo test --offline --manifest-path src-tauri/Cargo.toml
RUN (cargo test --locked --manifest-path src-tauri/Cargo.toml \
      || echo "WARNING: src-tauri tests reported failures") \
    && (cargo test --locked --manifest-path report-proxy/Cargo.toml \
      || echo "WARNING: report-proxy tests reported failures")

ENV CARGO_NET_OFFLINE=true
CMD ["bash"]
