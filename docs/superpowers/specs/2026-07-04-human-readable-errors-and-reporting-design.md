# Human-Readable Errors and In-App Reporting

Date: 2026-07-04
Status: Approved (design), pending implementation plan

## Problem

Errors surface to the user as raw strings. On the backend every failure becomes an
`AppError` variant that serializes to a bare string (see `src-tauri/src/error.rs`);
on the frontend, `catch` blocks display `e.message` or `String(e)` verbatim. The user
gets text like `API error (404): Not Found` or `IO error: The system cannot find the
path specified` with no explanation of what happened, why, or what to do next, and no
easy way to report it.

Goal: turn the errors users actually hit into human-readable guidance (what happened,
why, how to fix), and give them a low-friction path to file a GitHub issue that repo
bots can then triage.

## Constraints and Reality Checks

- **Errors arrive as strings.** On desktop, Tauri `invoke()` rejects with the serialized
  `AppError`, which today is a plain string. In browser mode, `ipcInvoke` wraps the
  response body in `new Error(text)`. The only reliable discriminator available without
  a breaking change is the message text. Changing `AppError` to serialize as a structured
  object would break every existing `catch` that reads `.message`. Therefore the catalog
  matches on message text and is fully additive (no Rust serialization change).
- **No GitHub token may ship in the app.** A distributed desktop binary cannot hold a
  write-scoped token; it would be extractable and abusable. Automatic issue creation must
  go through a proxy that holds the token, or through user OAuth.
- **Dependabot / Gemini Code Assist / Claude GitHub app cannot ingest a log and file an
  issue.** They act on existing PRs/issues. The app's job is to create a well-formed issue;
  bot triage happens afterward on GitHub.
- **No test framework.** Validation is `npm run build` + `cargo check` plus manual
  triggering. A dev-only gallery renders every catalog entry for eyeballing.
- **i18n rule (CLAUDE.md).** Every new `errors.<id>.*` key must exist in `en.ts` and all
  other locale files. English is authored now; other locales are populated via the existing
  `scripts/i18n-*` gap-fill workflow.

## Decomposition

**Sub-project A (build now): App error-readability layer.** Catalog, resolver, rendering
component, structured report payload, and a prefilled-GitHub-issue sink that needs zero
infrastructure.

**Sub-project B (build after A): NUC report proxy + Cloudflare Tunnel.** A local service on
the NUC (192.168.4.80) creates the issue server-side with a fine-grained PAT and returns the
URL. Exposed via `cloudflared` at `report.<domain>`. Drops in behind A's sink interface by
setting one config value. Optional B.2: a local LLM on the NUC writes the issue title/body.

A ships value immediately and does not block on B.

---

## Sub-project A: Design

### Components

1. **`src/lib/errors/errorCatalog.ts`**
   - Ordered array of entries: `{ id: string, match: (raw: string) => boolean }`.
   - `match` uses substrings/regex against the text the Rust variants and common frontend
     failures produce. First match wins, so order from specific to general.
   - No user-facing text lives here; text is resolved from i18n by `id`.

2. **`src/lib/errors/resolveError.ts`**
   - `resolveError(raw: unknown): FriendlyError`.
   - Coerces any thrown value to a string (`Error` -> `.message`, else `String(raw)`).
   - Walks the catalog; on first match returns a `FriendlyError` built from
     `errors.<id>.*` i18n keys. On no match returns the **generic fallback**.
   - Always preserves the original text in `raw`.

   ```ts
   interface FriendlyError {
     code: string;        // catalog id, or "unknown"
     title: string;       // localized heading
     what: string;        // what happened, plain language
     why: string;         // likely cause
     fixes: string[];     // ordered steps to try
     reportable: boolean; // whether "Report this error" is offered
     raw: string;         // original message, always preserved
   }
   ```

   Generic fallback: `code: "unknown"`, `title` = errors.generic.title
   ("Something went wrong"), `what` = the raw text, `why` = generic, `fixes` = [restart,
   check that ComfyUI is running, export logs], `reportable: true`.

3. **`src/lib/components/errors/ErrorCard.svelte`**
   - Props: `error: FriendlyError`, optional `compact: boolean`, `onReport?: () => void`.
   - Renders title, what, why, and an ordered list of fix steps.
   - A collapsible "Technical details" block shows `raw` and a "Copy diagnostics" button
     (uses existing `exportLogsContent()`).
   - When `reportable`, shows a "Report this error" button wired to `reportError`.
   - Tailwind only, dark neutral palette, `onclick` handlers, no `<style>` block.

4. **`src/lib/errors/reportError.ts`**
   - `buildReportPayload(error: FriendlyError, userNote?: string): ReportPayload`.
   - `reportError(error, userNote?)`: builds the payload and calls the active `ReportSink`.

   ```ts
   interface ReportPayload {
     errorCode: string;
     rawMessage: string;
     appVersion: string;     // __APP_VERSION__
     os: string;             // navigator/platform or Tauri os plugin
     arch: string;
     mode: "desktop" | "browser";
     timestamp: string;      // ISO
     userNote?: string;
     logsTail?: string;      // last N KB of exportLogsContent(), for proxy sink
   }
   ```

5. **`ReportSink` interface + implementations**
   - `interface ReportSink { submit(p: ReportPayload): Promise<{ issueUrl?: string }> }`.
   - `PrefilledIssueSink` (default, phase A): builds
     `https://github.com/Mooshieblob1/MooshieUI/issues/new?title=...&body=...&labels=bug,in-app-report`
     and opens it via `openExternalUrl`. Body contains error code, raw message, version,
     OS/arch, mode, and a reproduction stub. Because issue-URL length caps near 8 KB, the
     **full diagnostics log is copied to the clipboard** on click and the body instructs the
     user to paste it into a fenced code block.
   - `ProxySink` (phase B): `POST` the payload (including `logsTail`) to `report_endpoint`,
     return `{ issueUrl }` from the response.
   - Selection: a new optional `report_endpoint` value in `AppConfig`. Unset -> `PrefilledIssueSink`.
     Set -> `ProxySink`.

### Integration points

- Route the high-traffic surfaces through `resolveError` + `ErrorCard`: the notification/toast
  store and the main error modals. The existing Settings -> About "Report an Issue" modal is
  extended to accept a `FriendlyError` context so a contextual report can be launched from an
  error, while the manual report path still works.
- Primitives are drop-in; other `catch` sites adopt `resolveError` incrementally. No mass
  rewrite of every call site.

### Error catalog entries (initial set)

Connectivity: `connection_failed`, `comfyui_not_running`, `websocket_dropped`, `api_error_5xx`.
Downloads/models: `download_404`, `download_network`, `disk_full`, `checksum_mismatch`,
`model_not_found`, `hf_page_url`, `civitai_auth`.
Setup/runtime: `comfyui_launch_failed`, `python_env_broken`, `attention_backend_install`,
`out_of_memory`, `unsupported_gpu`.
Generation: `invalid_workflow`, `missing_node`, `generation_interrupted`.
IO/misc: `io_permission`, `serialization`, plus the generic fallback.

Each entry gets `errors.<id>.title`, `.what`, `.why`, and an array of fix steps
(`.fix1`, `.fix2`, ... or a single joined string, decided in the plan). Matchers key off the
`AppError` variant prefixes (`Connection failed:`, `API error (`, `Failed to start ComfyUI:`,
`Invalid workflow:`, `IO error:`, `Serialization error:`, `HTTP error:`) and content patterns
(`404`, `No space left`, `out of memory`, `CUDA`).

### Dev-only error gallery

A hidden/dev-only view that lists every catalog entry and renders it through `ErrorCard`, so
rendering, i18n keys, and report wiring can be verified without triggering real failures.
Gated so it never appears in normal use.

### i18n

- Add all `errors.*` keys to `src/lib/locales/en.ts`.
- Propagate to the other locale files via the existing `scripts/i18n-*` gap-fill workflow
  (machine translation), satisfying the all-locales rule.

### Validation

- `npm run build` and `cargo check` must pass.
- Manually trigger representative errors (bad download URL, ComfyUI stopped, etc.).
- Walk the dev gallery to confirm every entry renders with populated text and a working
  report button.

---

## Sub-project B: Design (deferred)

### NUC report service (`POST /report`)

- Validate the payload shape and cap `logsTail` size (for example 256 KB).
- Rate-limit per source IP; require a shared `X-Mooshie-App` header to deter random traffic.
- Deduplicate: search open issues by label + `errorCode` signature; if a match exists,
  add a comment to it instead of opening a duplicate.
- Create the issue via the GitHub REST API using a **fine-grained PAT scoped to issues:write
  on this repo only**, stored solely in the NUC environment.
- Return `{ issueUrl }`.
- Implementation language is open (small axum service reuses Rust patterns already in the repo,
  or a small Node service); decided in B's own plan.

### Cloudflare Tunnel

- Run `cloudflared` on the NUC; map `report.<domain>` -> `http://localhost:PORT`.
- No port forwarding, no exposed home IP; Cloudflare terminates TLS.
- Optional Cloudflare Turnstile token to curb abuse.

### B.2 (optional): LLM summarization

- A local llama.cpp on the NUC turns the raw log + error into a clean issue title and body.
- Purely server-side; the app never downloads a model for this.

### App-side wiring

- Set `report_endpoint` in `AppConfig` to the tunnel URL -> `ProxySink` activates.
- On success the UI shows "Reported! View issue ->" linking `issueUrl`.
- No secret ever ships in the app.

---

## Out of Scope

- Rewriting every internal/developer-facing error string. Only user-facing errors get bespoke
  entries; the rest fall through to the generic fallback.
- Changing `AppError`'s serialization format.
- Embedding any credential in the app binary.
- Hand-translating locale strings in this pass (machine-propagated now, optional human pass later).
