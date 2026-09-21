# Native music-tool checks

This small crate compiles `src-tauri/src/media_tools.rs` unchanged against a
minimal host state. It avoids installing the GUI, ComfyUI, or any models.
The native CI matrix tests Linux x64/ARM64 and macOS Intel/Apple Silicon.

On a native host, choose an isolated absolute temporary directory:

```sh
export MOOSHIE_MEDIA_TOOLS_SMOKE_ROOT="$PWD/media-smoke"
export MOOSHIE_MEDIA_TOOLS_TEST_DIR="$MOOSHIE_MEDIA_TOOLS_SMOKE_ROOT/bin/media"
cargo test --manifest-path scripts/media-tools-smoke/Cargo.toml --locked
cargo test --manifest-path scripts/media-tools-smoke/Cargo.toml --locked live_install_and_offline_reuse -- --ignored --nocapture
cargo test --manifest-path scripts/media-tools-smoke/Cargo.toml --locked startup_reuses_tools_offline_and_runs_media_pipeline -- --ignored --nocapture
```

The live checks install all four pinned packages, validate checksums and versions,
then test startup with an unreachable proxy to prove reuse requires no network.
They also check executable permissions, JavaScript execution, MP3 encoding and
decoding, explicit runtime paths, and shutdown cleanup. They leave the verified
tools in the chosen temporary directory for inspection and repeat checks.

These checks do not validate the installed Tauri UI, Finder/Gatekeeper behavior,
or music generation on physical hardware.
