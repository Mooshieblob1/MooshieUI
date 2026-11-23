# Threat model

## What this project does

MooshieUI is a front-end for ComfyUI (local AI image, video and music
generation). One Rust crate (`src-tauri/`) builds two ways:

- **Desktop app** (default `desktop` feature, Tauri). The Svelte frontend talks
  to Rust over Tauri IPC. Single user, their own machine.
- **Embedded web server** (`src-tauri/src/webserver.rs`, axum). Serves the same
  UI to a browser, with IPC turned into REST POST + SSE. It runs inside the
  desktop app (browser mode) and as the headless `mooshieui-server` binary
  (`--no-default-features --features server`), which is what the Docker image
  in `deploy/` ships.

Inside the desktop app the server binds `127.0.0.1` unless the user turns on
LAN mode. The headless `mooshieui-server` always runs in LAN mode (see
`server_main.rs`). In LAN mode it binds all interfaces and serves other people:
accounts, roles (Admin, Moderator, User, Anonymous; see `UserRole` in
`webserver.rs`), sessions and passwords in `src-tauri/src/auth.rs`, per-user
galleries and exports. The Docker image is often exposed through a reverse
proxy or Cloudflare Tunnel, so treat LAN mode as internet-facing.

`report-proxy/` is a small separate axum service that turns in-app error
reports into GitHub issues. It is public and unauthenticated by design, and
holds a GitHub token.

## Where untrusted input enters (highest priority first)

1. **Any HTTP request to the web server from a non-localhost client in LAN
   mode.** Assume the attacker can reach the port with no account, or with a
   low-privilege `User` account. Authentication, role checks, per-user
   ownership of gallery items, exports and generations, path handling on
   anything that touches the filesystem, and request parsing all matter.
2. **Browsers on the same machine.** A malicious website visited by the
   desktop user must not be able to drive the localhost server (CSRF, DNS
   rebinding, CORS). See `host_header_is_local`, `request_is_same_origin` and
   `local_request_is_trusted` in `webserver.rs`.
3. **report-proxy.** Anonymous internet input; a GitHub token behind it.
   Abuse that spends the token's write budget, injects content into issues
   beyond the report text, or leaks the token is in scope.
4. **Files the user did not create**: images, video and audio dropped into the
   app or imported from a folder (PNG/JPEG/WebP/GIF/MP4 metadata parsing in
   `src-tauri/src/metadata/`, JXL decode), model files and their metadata, zip
   archives, and ComfyUI workflow JSON.
5. **Responses from remote services**: ComfyUI's HTTP and WebSocket API (it may
   run on another machine), CivitAI, Hugging Face, NovelAI, ElevenLabs and
   GitHub. Treat these as untrusted data, never as code.

## Components that matter most / least

- Most: `webserver.rs`, `auth.rs`, `user_secrets.rs`, `user_prefs.rs`,
  `output_owners.rs`, `temp_images.rs`, `http_range.rs`, `linked_folders.rs`,
  `gallery_index.rs`, `metadata/`, `commands/` (each command is reachable over
  REST in browser mode), and `report-proxy/`.
- Less: desktop-only code with no network path in (window management, tray,
  notifications), and the Svelte frontend except where it renders content an
  attacker controls (XSS that reaches an Admin session is high).
- Out of scope: ComfyUI itself, third-party custom nodes, PyTorch and other
  dependencies, model weights, and `deploy/k8s`. Bugs in dependencies only
  count if MooshieUI uses them unsafely.

## Known non-issues (do not report)

- Localhost with LAN mode off is trusted as the machine owner. Anything a
  local process can do through the server it could already do as that user.
  That trust can be turned off with `MOOSHIEUI_TRUST_LOCALHOST=false`.
- An Admin can change settings, paths, install packages and run ComfyUI, so
  Admin reaching code execution is expected. Moderator or User reaching it is
  not.
- ComfyUI executes whatever workflow it is sent; that is ComfyUI's design.
  What matters is whether a low-privilege MooshieUI user can make MooshieUI
  send something they should not be allowed to.
- The `changeme` placeholder in `docker-compose.yml` is never used as a
  password: `server_main.rs` refuses to create the admin account while it is
  set.

## How to exercise it

- Built image: `/src`, with `node_modules`, `dist/` and Rust `target/` already
  built. Everything works offline.
- Rust tests: `cargo test --offline --manifest-path src-tauri/Cargo.toml` and
  `cargo test --offline --manifest-path report-proxy/Cargo.toml`.
- Server: `src-tauri/target/debug/mooshieui-server`. It does not need ComfyUI
  to start and serve the API; set `MOOSHIEUI_DATA_DIR` to an empty directory.
- A proof of concept as a Rust unit test, or a curl sequence against the
  server binary, is the most useful form.

## How we rate severity

- **Critical**: unauthenticated remote code execution, or authentication
  bypass to Admin, over the LAN/server port; token theft from report-proxy.
- **High**: a low-privilege LAN user reaching code execution, Admin actions,
  arbitrary file read or write, or other users' private data (galleries,
  prompts, stored API keys); a malicious website driving the localhost server;
  stored XSS that runs in another user's session.
- **Medium**: privilege escalation within the non-admin roles, path traversal
  confined to the data directory, memory-safety or crash bugs reachable from an
  imported file, unauthenticated denial of service against the server.
- **Low**: issues that need Admin or local access, information leaks of
  non-sensitive metadata, crashes needing an unusual setup.

## Reports and patches

Keep reports short: affected file and function, the request or input that
triggers it, the role needed, and the impact. A minimal patch is welcome;
match the surrounding code. Group findings that share one root cause.
