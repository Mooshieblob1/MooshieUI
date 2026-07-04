# Report Proxy (Sub-project B) Design

**Status:** Approved design, pre-implementation
**Date:** 2026-07-05
**Depends on:** Sub-project A (human-readable errors + reporting UI), already on `main`.

## Goal

Stand up a small self-hosted service on the NUC (`192.168.4.80`) that receives in-app
error reports and creates GitHub issues on `Mooshieblob1/MooshieUI` server-side, so the
GitHub write credential (a fine-grained PAT) lives only on the NUC and never ships in the
app binary. Optionally enrich each issue with a one-paragraph plain-English summary using
the LLM already running on the NUC.

## Non-goals (YAGNI)

- Embeddings / vector RAG (the `snowywood-embed` server exists but is not used here).
- A dedicated LLM container (we reuse the running `snowywood-llm`).
- Live source-file retrieval from the repo.
- Cloudflare Turnstile / CAPTCHA (left as a documented future hook).
- Multi-repo support.

## Confirmed environment (verified via SSH, 2026-07-04)

- Host: Arch Linux, `192.168.4.80`, user `blob`, key-based SSH. CPU-only, no GPU.
- LLM to reuse: container **`snowywood-llm`**, image `ghcr.io/ggml-org/llama.cpp:server`,
  model **`Qwen_Qwen3-0.6B-Q4_K_M.gguf`**, launched with `-c 3072 -t 3 --parallel 1`.
  On docker network **`blob_default`**, container port **8080** (host-mapped
  `127.0.0.1:8089`). OpenAI-compatible `POST /v1/chat/completions`.
  Defined in `/home/blob/docker-compose.yml` (compose project `blob`).
- The model is a **Qwen3 thinking model** and is **shared with a Discord bot's `/ask`**
  command. It can be busy; it is CPU-slow (~21 tok/s, ~0.3s/token prompt eval); context is
  only 3072 tokens. Therefore the LLM is best-effort enrichment, never load-bearing.
- No `cloudflared` / reverse proxy currently exposes anything for this purpose. lighttpd
  holds `:80`, vaultwarden `:8000`. The `MooshieUI/docker-compose.yml` on the box already
  contains a commented-out `cloudflared` sidecar pattern, so the tunnel approach is familiar.

## The app -> proxy contract (fixed by Sub-project A, do not change)

Request: `POST <report_endpoint>` with headers
`{ "Content-Type": "application/json", "X-Mooshie-App": "1" }` and JSON body `ReportPayload`:

```
errorCode: string          // e.g. "out_of_memory"
rawMessage: string         // original technical error text
appVersion: string
os: string
arch: string
mode: "desktop" | "browser"
timestamp: string          // ISO 8601
userNote?: string          // optional free text from the user
logsTail?: string          // up to ~200 KB tail of diagnostics; present in proxy mode
```

Response the app expects: `200` with JSON `{ "issueUrl": "https://github.com/..." }`.
`src/lib/errors/reportError.ts` `ProxySink.submit()` throws on any non-2xx.

## Architecture

A new Rust + axum service `report-proxy`, packaged as a Docker container that joins the
external `blob_default` network (to reach `snowywood-llm`) and is fronted by a Cloudflare
Tunnel. Single route: `POST /report`. Also `GET /health` for readiness.

### Request flow (issue creation is NEVER blocked by the LLM)

1. **Gate + validate.** Reject if `X-Mooshie-App: 1` header absent. Enforce a body-size cap
   (e.g. 512 KB, covers the ~200 KB logsTail). Parse into the `ReportPayload` shape; reject
   malformed bodies with `400`. Apply a per-IP token-bucket rate limit; `429` when exceeded.
2. **Dedup.** Compute a signature `sig = sha256(errorCode + "\n" + normalize(rawMessage))`,
   truncated. Normalization strips digits, hex, paths, and whitespace runs so
   near-identical errors collapse. Fetch open issues via the GitHub API filtered to the
   `in-app-report` label. If an open issue body contains the marker
   `<!-- mooshie-sig: <sig> -->`, post a short "seen again (+1), appVersion X, os Y" comment
   on that issue and return its URL instead of opening a duplicate.
3. **Template body.** Build a deterministic Markdown body mirroring the app's existing
   `issueBody()` structure: What happened (userNote) / Error (rawMessage) / Environment
   (appVersion, os, arch, mode, errorCode, timestamp) / Diagnostics (fenced logsTail,
   itself capped server-side). Append the hidden `<!-- mooshie-sig: <sig> -->` marker.
4. **Optional LLM enrichment (best-effort, hard timeout ~20s).** Build a small prompt:
   the catalog entry for `errorCode` (what/why/fix, extracted from `en.ts`) + the last
   ~1-1.5k tokens of `logsTail`, prefixed with `/no_think` to disable Qwen3 reasoning.
   Call `http://snowywood-llm:8080/v1/chat/completions` with generous `max_tokens`, read
   `message.content` (NOT `reasoning_content`). On success, prepend a
   `### Summary` paragraph to the body. On timeout / connection error / empty content:
   skip silently, keep the template-only body.
5. **Create issue.** `POST /repos/Mooshieblob1/MooshieUI/issues` with the PAT, title
   `[in-app] <errorCode>: <rawMessage first 80 chars>`, labels `bug,in-app-report`.
   Return `{ issueUrl }`. On GitHub API failure, return `502` (the app then falls back to
   the prefilled-issue sink, see App-side changes).

### Files (Rust crate `report-proxy/`)

- `src/main.rs` — axum app, router (`POST /report`, `GET /health`), config from env,
  shared `reqwest::Client`, rate-limiter state, graceful shutdown.
- `src/report.rs` — the `/report` handler: validation, orchestration of dedup/template/LLM/create.
- `src/github.rs` — GitHub REST client: list open labeled issues, create issue, comment.
- `src/llm.rs` — `snowywood-llm` client: build `/no_think` prompt, call chat completions,
  timeout + fallback, read `message.content`.
- `src/dedup.rs` — signature computation + rawMessage normalization + marker parsing.
- `src/catalog.rs` — embedded errorCode -> {what, why, fix} map, generated from `en.ts` at
  build time by a small extraction step (see below).
- `Dockerfile` — multi-stage (cargo build -> distroless/debian-slim runtime).

### Configuration (env vars on the NUC only)

- `GITHUB_TOKEN` — fine-grained PAT, `Mooshieblob1/MooshieUI` only, Issues: Read+Write.
- `GITHUB_REPO` — default `Mooshieblob1/MooshieUI`.
- `LLM_BASE_URL` — default `http://snowywood-llm:8080`. Empty disables enrichment.
- `LLM_TIMEOUT_SECS` — default `20`.
- `RATE_LIMIT_PER_MIN` — default e.g. `10` per IP.
- `MAX_BODY_BYTES` — default `524288`.
- `BIND_ADDR` — default `0.0.0.0:8091` (container-internal; only the tunnel reaches it).

### Catalog extraction

A small build-time step (Node script `report-proxy/scripts/extract-catalog.mjs`) reads
`src/lib/locales/en.ts`, pulls every `errors.<code>.{title,what,why,fixes}`, and emits
`report-proxy/src/catalog_data.rs` (or a JSON embedded via `include_str!`). This keeps the
proxy's grounding text in sync with the app's error copy without a live dependency. Run
manually when error copy changes; documented in the runbook. The 22 current codes:
generic, connection_failed, comfyui_not_running, websocket_dropped, api_error_5xx,
download_404, download_network, disk_full, checksum_mismatch, civitai_auth, hf_page_url,
model_not_found, comfyui_launch_failed, python_env_broken, attention_backend_install,
out_of_memory, unsupported_gpu, missing_node, invalid_workflow, generation_interrupted,
io_permission, serialization.

## Deployment

`report-proxy/docker-compose.yml` on the NUC:

- Service `report-proxy`: builds the crate, `restart: unless-stopped`, reads env from a
  `.env` file (holds `GITHUB_TOKEN`), attaches to the **external** `blob_default` network
  (so `snowywood-llm` resolves by name), no host port published.
- Service `cloudflared`: `cloudflare/cloudflared:latest`,
  `command: tunnel --no-autoupdate run --token ${CLOUDFLARE_TUNNEL_TOKEN}`,
  `network_mode: "service:report-proxy"` so it can reach the proxy on `localhost:8091`,
  `restart: unless-stopped`. Token supplied via `.env`.

The tunnel is created in the Cloudflare dashboard (user task) and routed
`report.<domain>` -> `http://localhost:8091`.

## Security

- Fine-grained PAT scoped to the single repo, Issues read/write only, stored on the NUC in
  `.env` (git-ignored) / docker secret. Never in the app binary or repo.
- `X-Mooshie-App` presence gate + per-IP rate limit + body-size cap as basic abuse control.
- TLS terminated by Cloudflare; tunnel is outbound-only, so no inbound router ports.
- CORS: allow the app origins; the desktop app sends from a Tauri/`tauri://` or
  `http://localhost` context and browser mode from the hosted origin. Since requests carry
  the custom `X-Mooshie-App` header, a permissive-but-header-gated policy is acceptable; a
  strict allowlist can be added if abuse appears.
- Turnstile hook: documented, not built.

## App-side changes (in the MooshieUI repo)

1. **Rust config field.** Add `report_endpoint: Option<String>` to `AppConfig`
   (`src-tauri/src/config.rs`), `#[serde(default)]`, default `None`. The TS type
   (`src/lib/types/index.ts:243`) already has `report_endpoint?: string | null`. Empty/None
   keeps the app on `PrefilledIssueSink`; setting it to the tunnel URL activates `ProxySink`.
2. **ProxySink fallback.** Today `ProxySink.submit()` throws on non-2xx and the report is
   lost. Change `reportError()` (`src/lib/errors/reportError.ts`) so that when the active
   sink is `ProxySink` and it throws, it falls back to `PrefilledIssueSink` (open prefilled
   GitHub issue + copy diagnostics), so reporting never hard-fails.

## Testing / validation

No test framework in this repo; validation is `npm run build` + `cargo check` for the
app-side changes. For the proxy crate: `cargo check` / `cargo clippy`, plus a manual
`curl` smoke test against the container (`POST /report` with a sample payload -> expect a
real issue URL; a duplicate payload -> expect a comment on the same issue).

## User tasks (Cloudflare, done in the dashboard)

Documented in `report-proxy/RUNBOOK.md`; summarized to the user in plain English:
create a fine-grained GitHub PAT, create a Cloudflare Tunnel, add a public hostname route
`report.<domain>` -> `http://localhost:8091`, copy the tunnel token into the NUC `.env`.
