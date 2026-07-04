# Human-Readable Errors (Sub-project A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the errors users actually hit into human-readable guidance (what happened, why, how to fix) and give a one-click path to file a prefilled GitHub issue.

**Architecture:** A frontend-only, additive layer. A catalog matches raw error text to a stable id; a resolver returns a localized `FriendlyError`; an `ErrorCard` renders it; a `ReportSink` turns it into a prefilled GitHub issue (with a drop-in proxy path for Sub-project B later). No Rust `AppError` serialization change.

**Tech Stack:** Svelte 5 runes, TypeScript, Tailwind, existing `ipcInvoke`/`api.ts` wrappers, existing i18n (`locale.t`), existing `exportLogsContent()`.

## Global Constraints

- Svelte 5 runes only; no `svelte/store`. Stores are class singletons with `$state` in `*.svelte.ts` files. Reassign arrays with spread, not `.push()`.
- All backend calls go through `ipcInvoke`/`ipcListen` (never raw `invoke`/`listen`). Typed wrappers live in `src/lib/utils/api.ts`.
- UI: Tailwind only, no `<style>` blocks in `.svelte`. Use `onclick`, not `on:click`. Dark neutral palette, accents via `--theme-accent-*`.
- i18n: user-facing strings only via `locale.t('key')`. Every key and `{placeholder}` added to `src/lib/locales/en.ts` must exist in all other locale files.
- No `AppError` serialization change; the catalog matches on error message text.
- No credential ships in the app. Phase A only opens a prefilled issue URL; it never creates issues directly.
- No em dashes or non-ASCII flourishes in any text posted in the user's voice (issue titles/bodies, PR/commit text). Plain ASCII.
- Validation gate is `npm run build` (frontend typecheck + bundle). There is no test framework; do not add one. Pure-function correctness is checked against the input/output tables in each task and visually via the dev error gallery.
- Repo git commands on Windows must be prefixed with `git -c core.hooksPath=/dev/null` (the bash pre-commit hook hangs in PowerShell).

---

### Task 1: Error types and platform info helpers

**Files:**
- Create: `src/lib/errors/types.ts`
- Create: `src/lib/utils/platformInfo.ts`
- Create: `src/lib/utils/openExternal.ts`

**Interfaces:**
- Produces: `FriendlyError`, `ReportPayload`, `ReportSink` (types); `appVersion(): string`, `appMode(): "desktop" | "browser"`, `platformInfo(): { os: string; arch: string }`; `openExternalUrl(url: string): Promise<void>`.

- [ ] **Step 1: Create the shared types**

`src/lib/errors/types.ts`:

```ts
/** A raw backend/frontend error resolved into user-facing guidance. */
export interface FriendlyError {
  /** Catalog id, or "unknown" for the generic fallback. */
  code: string;
  title: string;
  what: string;
  why: string;
  /** Ordered steps the user can try. */
  fixes: string[];
  /** Whether a "Report this error" action is offered. */
  reportable: boolean;
  /** Original error text, always preserved for the details block and reports. */
  raw: string;
}

/** Structured payload sent to a ReportSink. */
export interface ReportPayload {
  errorCode: string;
  rawMessage: string;
  appVersion: string;
  os: string;
  arch: string;
  mode: "desktop" | "browser";
  timestamp: string;
  userNote?: string;
  /** Tail of exportLogsContent(); used by the proxy sink, omitted for URL sink. */
  logsTail?: string;
}

/** Destination for a report. Phase A: prefilled URL. Phase B: NUC proxy. */
export interface ReportSink {
  submit(payload: ReportPayload): Promise<{ issueUrl?: string }>;
}
```

- [ ] **Step 2: Create platform info helpers**

`src/lib/utils/platformInfo.ts`:

```ts
import { isTauri } from "./ipc.js";

declare const __APP_VERSION__: string;

export function appVersion(): string {
  return typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : "dev";
}

export function appMode(): "desktop" | "browser" {
  return isTauri ? "desktop" : "browser";
}

/** Best-effort OS/arch from the browser environment. No new Tauri plugin needed. */
export function platformInfo(): { os: string; arch: string } {
  const nav = globalThis.navigator;
  const os = nav?.platform || nav?.userAgent || "unknown";
  const uaArch = (nav as unknown as { userAgentData?: { platform?: string } })?.userAgentData?.platform;
  const arch = uaArch || (/(x86_64|x64|amd64|arm64|aarch64)/i.exec(nav?.userAgent ?? "")?.[0]) || "unknown";
  return { os, arch };
}
```

- [ ] **Step 3: Create the shared external-url opener**

`src/lib/utils/openExternal.ts`:

```ts
import { isTauri } from "./ipc.js";

/** Open a URL in the OS browser (Tauri) or a new tab (browser mode). */
export async function openExternalUrl(url: string): Promise<void> {
  if (isTauri) {
    const { open } = await import("@tauri-apps/plugin-shell");
    await open(url);
  } else {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}
```

- [ ] **Step 4: Verify build**

Run: `npm run build`
Expected: build succeeds, no TypeScript errors referencing the new files.

- [ ] **Step 5: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/errors/types.ts src/lib/utils/platformInfo.ts src/lib/utils/openExternal.ts
git -c core.hooksPath=/dev/null commit -m "feat(errors): add error types and platform helpers"
```

---

### Task 2: Error catalog and resolver

**Files:**
- Create: `src/lib/errors/errorCatalog.ts`
- Create: `src/lib/errors/resolveError.ts`

**Interfaces:**
- Consumes: `FriendlyError` (Task 1); `locale` singleton from `src/lib/stores/locale.svelte.ts` (method `t(key, params?)`).
- Produces: `CATALOG: CatalogEntry[]`, `type CatalogEntry = { id: string; match: (raw: string) => boolean }`; `resolveError(raw: unknown): FriendlyError`; `catalogIds(): string[]`.

- [ ] **Step 1: Create the catalog**

`src/lib/errors/errorCatalog.ts`. Each entry's user-facing text lives in i18n under `errors.<id>.*` (added in Task 3). Order specific-to-general; first match wins.

```ts
export interface CatalogEntry {
  id: string;
  match: (raw: string) => boolean;
}

const has = (needle: string) => (raw: string) => raw.toLowerCase().includes(needle);
const re = (pattern: RegExp) => (raw: string) => pattern.test(raw);

/** Ordered: specific matchers first, broad ones last. */
export const CATALOG: CatalogEntry[] = [
  // Connectivity
  { id: "comfyui_not_running", match: re(/comfyui.*(not running|not started|unavailable)|failed to connect to comfyui/i) },
  { id: "connection_failed", match: re(/connection failed|connection refused|failed to connect|could not connect/i) },
  { id: "websocket_dropped", match: re(/websocket error|websocket.*(closed|dropped|disconnect)/i) },
  { id: "api_error_5xx", match: re(/api error \((5\d\d)\)|http error.*(5\d\d)/i) },

  // Downloads / models
  { id: "download_404", match: re(/\b404\b|not found/i) },
  { id: "disk_full", match: re(/no space left|not enough space|disk full|insufficient disk/i) },
  { id: "checksum_mismatch", match: re(/checksum|sha256.*mismatch|hash mismatch/i) },
  { id: "civitai_auth", match: re(/civitai.*(401|403|unauthor|api key|token)/i) },
  { id: "hf_page_url", match: re(/huggingface.*(page url|\/blob\/)|not a direct file/i) },
  { id: "model_not_found", match: re(/model.*(not found|missing)|no such model|checkpoint.*not found/i) },
  { id: "download_network", match: re(/http error|network error|timed out|timeout|reqwest/i) },

  // Setup / runtime
  { id: "comfyui_launch_failed", match: re(/failed to start comfyui|process.*spawn|failed to spawn/i) },
  { id: "python_env_broken", match: re(/python.*(not found|missing)|venv|virtualenv|no module named/i) },
  { id: "attention_backend_install", match: re(/attention backend|flash.?attn|sage.?attn|xformers/i) },
  { id: "out_of_memory", match: re(/out of memory|cuda.*memory|oom|allocat.*fail/i) },
  { id: "unsupported_gpu", match: re(/unsupported gpu|no cuda|no gpu|device.*not supported/i) },

  // Generation
  { id: "missing_node", match: re(/missing node|node type.*not found|unknown node|no node named/i) },
  { id: "invalid_workflow", match: re(/invalid workflow|malformed workflow|workflow.*invalid/i) },
  { id: "generation_interrupted", match: re(/interrupted|cancell?ed|aborted/i) },

  // IO / misc
  { id: "io_permission", match: re(/permission denied|access is denied|io error/i) },
  { id: "serialization", match: has("serialization error") },
];

export function catalogIds(): string[] {
  return CATALOG.map((e) => e.id);
}
```

- [ ] **Step 2: Create the resolver**

`src/lib/errors/resolveError.ts`:

```ts
import { locale } from "../stores/locale.svelte.js";
import type { FriendlyError } from "./types.js";
import { CATALOG } from "./errorCatalog.js";

/** Coerce any thrown value to a display string. */
function toRawString(raw: unknown): string {
  if (raw == null) return "";
  if (typeof raw === "string") return raw;
  if (raw instanceof Error) return raw.message;
  if (typeof raw === "object" && "message" in raw && typeof (raw as { message: unknown }).message === "string") {
    return (raw as { message: string }).message;
  }
  return String(raw);
}

/** Read errors.<id>.fixes as an array; tolerate a single string. */
function fixesFor(id: string): string[] {
  const joined = locale.t(`errors.${id}.fixes`);
  // Fixes are authored as a single string with " || " separators to keep i18n flat.
  if (!joined || joined === `errors.${id}.fixes`) return [];
  return joined.split(" || ").map((s) => s.trim()).filter(Boolean);
}

function buildFriendly(id: string, raw: string, reportable: boolean): FriendlyError {
  return {
    code: id,
    title: locale.t(`errors.${id}.title`),
    what: locale.t(`errors.${id}.what`),
    why: locale.t(`errors.${id}.why`),
    fixes: fixesFor(id),
    reportable,
    raw,
  };
}

/** Resolve any error into user-facing guidance. Never throws. */
export function resolveError(raw: unknown): FriendlyError {
  const text = toRawString(raw);
  for (const entry of CATALOG) {
    try {
      if (entry.match(text)) return buildFriendly(entry.id, text, true);
    } catch {
      // A broken matcher must never break resolution.
    }
  }
  // Generic fallback. `what` shows the raw text so nothing is hidden.
  return {
    code: "unknown",
    title: locale.t("errors.generic.title"),
    what: text || locale.t("errors.generic.what"),
    why: locale.t("errors.generic.why"),
    fixes: fixesFor("generic"),
    reportable: true,
    raw: text,
  };
}
```

- [ ] **Step 3: Verify matcher behavior against the table**

Confirm each raw input maps to the expected id by reading `CATALOG` order:

| Raw input (substring) | Expected id |
|---|---|
| `Connection failed: os error 111` | `connection_failed` |
| `Failed to start ComfyUI: spawn error` | `comfyui_launch_failed` |
| `API error (503): Service Unavailable` | `api_error_5xx` |
| `HTTP error: 404 Not Found` | `download_404` |
| `No space left on device` | `disk_full` |
| `CUDA out of memory` | `out_of_memory` |
| `Invalid workflow: node missing` | `missing_node` (matches before invalid_workflow) |
| `Serialization error: expected value` | `serialization` |
| `something totally unknown xyz` | `unknown` (generic fallback) |

Note the intended precedence: `missing_node` is listed before `invalid_workflow`, so "node missing" resolves to `missing_node`. If a real message should be `invalid_workflow`, it must not contain the missing-node phrasing.

- [ ] **Step 4: Verify build**

Run: `npm run build`
Expected: succeeds. (i18n keys are added in Task 3; missing keys fall back to the key string at runtime but do not break the build.)

- [ ] **Step 5: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/errors/errorCatalog.ts src/lib/errors/resolveError.ts
git -c core.hooksPath=/dev/null commit -m "feat(errors): add catalog matchers and resolver"
```

---

### Task 3: English i18n strings for every catalog entry

**Files:**
- Modify: `src/lib/locales/en.ts`

**Interfaces:**
- Produces: `errors.<id>.title|what|why|fixes` for all 21 catalog ids plus `errors.generic.*`, and reporting UI keys under `errors.report.*` and `errors.card.*`.

- [ ] **Step 1: Locate the insertion point**

Open `src/lib/locales/en.ts`. It is a single flat object literal mapping dotted string keys to values (for example `"common.close": "Close"`). Add the `errors.*` keys below anywhere inside that object (a block near the end, before the closing brace, is fine). Do NOT author nested objects; every key is a flat `"errors.<id>.<field>": "..."` string. Match the file's quote style (double quotes) and keep a trailing comma after the last added line.

- [ ] **Step 2: Add the strings**

Add these flat keys. `fixes` uses ` || ` as the step separator, consumed by `fixesFor()` in Task 2. Plain ASCII only, no em dashes.

```ts
  "errors.generic.title": "Something went wrong",
  "errors.generic.what": "The app hit an error it did not have specific guidance for.",
  "errors.generic.why": "This can happen from an unexpected state, a transient glitch, or a bug.",
  "errors.generic.fixes": "Try the action again. || Restart the app, and ComfyUI if it is running. || If it keeps happening, report it with the Report this error button below.",
  "errors.connection_failed.title": "Could not reach the server",
  "errors.connection_failed.what": "The app could not open a connection to the backend.",
  "errors.connection_failed.why": "The server or ComfyUI may be stopped, still starting up, or blocked by a firewall.",
  "errors.connection_failed.fixes": "Wait a few seconds and retry. || Check that ComfyUI is running in Settings. || Confirm no firewall or VPN is blocking localhost.",
  "errors.comfyui_not_running.title": "ComfyUI is not running",
  "errors.comfyui_not_running.what": "The app needs ComfyUI running but could not find it.",
  "errors.comfyui_not_running.why": "ComfyUI has not been started, crashed, or is still launching.",
  "errors.comfyui_not_running.fixes": "Start ComfyUI from Settings. || Wait for startup to finish, then retry. || Check the logs for a ComfyUI crash.",
  "errors.websocket_dropped.title": "Live connection dropped",
  "errors.websocket_dropped.what": "The realtime connection to ComfyUI closed unexpectedly.",
  "errors.websocket_dropped.why": "ComfyUI may have restarted, or the network connection was interrupted.",
  "errors.websocket_dropped.fixes": "Retry the action to reconnect. || Confirm ComfyUI is still running. || Restart ComfyUI if the problem persists.",
  "errors.api_error_5xx.title": "The server returned an error",
  "errors.api_error_5xx.what": "The backend responded with a server-side error (5xx).",
  "errors.api_error_5xx.why": "ComfyUI or the backend hit an internal error while handling the request.",
  "errors.api_error_5xx.fixes": "Retry in a moment. || Check the logs for the underlying error. || Restart ComfyUI if it keeps failing.",
  "errors.download_404.title": "File not found (404)",
  "errors.download_404.what": "The download URL returned Not Found.",
  "errors.download_404.why": "The link is wrong, the file was moved or removed, or it needs a login the app does not have.",
  "errors.download_404.fixes": "Double-check the URL opens in a browser. || For Hugging Face, use a /resolve/ link, not a /blob/ page. || For gated files, add your access token in Settings.",
  "errors.download_network.title": "Download failed",
  "errors.download_network.what": "The file could not be downloaded.",
  "errors.download_network.why": "The connection dropped, timed out, or the host was unreachable.",
  "errors.download_network.fixes": "Check your internet connection and retry. || Try again later if the host is busy. || Confirm no VPN or firewall is blocking the download.",
  "errors.disk_full.title": "Not enough disk space",
  "errors.disk_full.what": "There was not enough free space to finish writing the file.",
  "errors.disk_full.why": "The drive holding your models or output folder is full.",
  "errors.disk_full.fixes": "Free up disk space and retry. || Point the models or gallery folder to a larger drive in Settings. || Delete unused models from Model Hub.",
  "errors.checksum_mismatch.title": "Downloaded file was corrupted",
  "errors.checksum_mismatch.what": "The downloaded file did not match its expected checksum.",
  "errors.checksum_mismatch.why": "The download was interrupted or altered in transit.",
  "errors.checksum_mismatch.fixes": "Delete the partial file and download again. || Retry on a more stable connection. || If it keeps failing, report the URL.",
  "errors.civitai_auth.title": "CivitAI login required",
  "errors.civitai_auth.what": "CivitAI rejected the download as unauthorized.",
  "errors.civitai_auth.why": "The file needs a CivitAI account or API key that is missing or invalid.",
  "errors.civitai_auth.fixes": "Add or update your CivitAI API key in Settings. || Confirm the key has access to this file. || Sign in on civitai.com to check the file is still available.",
  "errors.hf_page_url.title": "That is a Hugging Face page, not a file",
  "errors.hf_page_url.what": "The URL points to a model page instead of a downloadable file.",
  "errors.hf_page_url.why": "Hugging Face /blob/ links open a web page; downloads need a /resolve/ link.",
  "errors.hf_page_url.fixes": "Use the /resolve/main/... form of the URL. || The app auto-fixes /blob/ links in the Model Hub field. || Copy the direct download link from the file's page.",
  "errors.model_not_found.title": "Model not found",
  "errors.model_not_found.what": "The requested model or checkpoint could not be located.",
  "errors.model_not_found.why": "It is not installed, was moved or renamed, or the folder is not indexed.",
  "errors.model_not_found.fixes": "Install the model from Model Hub. || Refresh the model list in Settings. || Confirm the file is in the correct models folder.",
  "errors.comfyui_launch_failed.title": "ComfyUI failed to start",
  "errors.comfyui_launch_failed.what": "The app tried to launch ComfyUI but the process did not start.",
  "errors.comfyui_launch_failed.why": "The install may be incomplete, a dependency is missing, or a port is in use.",
  "errors.comfyui_launch_failed.fixes": "Re-run setup from Settings. || Check the logs for the launch error. || Make sure no other ComfyUI is already using the port.",
  "errors.python_env_broken.title": "Python environment problem",
  "errors.python_env_broken.what": "The bundled Python environment could not be used.",
  "errors.python_env_broken.why": "The environment is missing, incomplete, or a package failed to import.",
  "errors.python_env_broken.fixes": "Re-run setup to rebuild the environment. || Check the logs for the failing import. || Report it if setup keeps failing.",
  "errors.attention_backend_install.title": "Attention backend install failed",
  "errors.attention_backend_install.what": "Installing the acceleration backend did not complete.",
  "errors.attention_backend_install.why": "The backend may not support your GPU, or the download or build failed.",
  "errors.attention_backend_install.fixes": "Try a different attention backend in Settings. || Confirm your GPU is supported. || Check the logs and retry the install.",
  "errors.out_of_memory.title": "Ran out of memory",
  "errors.out_of_memory.what": "The GPU or system ran out of memory during generation.",
  "errors.out_of_memory.why": "The resolution, batch size, or model is too large for available memory.",
  "errors.out_of_memory.fixes": "Lower the resolution or batch size and retry. || Close other GPU-heavy apps. || Use a smaller or more quantized model.",
  "errors.unsupported_gpu.title": "GPU not supported",
  "errors.unsupported_gpu.what": "The app could not use your GPU for this operation.",
  "errors.unsupported_gpu.why": "No compatible GPU or driver was detected, or the feature needs CUDA.",
  "errors.unsupported_gpu.fixes": "Update your GPU drivers. || Confirm your GPU meets the requirements. || Report your GPU model if you think it should work.",
  "errors.missing_node.title": "A required ComfyUI node is missing",
  "errors.missing_node.what": "The workflow needs a custom node that is not installed.",
  "errors.missing_node.why": "A custom node pack referenced by the workflow is not present.",
  "errors.missing_node.fixes": "Re-run setup to reinstall bundled nodes. || Check the logs for the node name. || Report it if the node should ship with the app.",
  "errors.invalid_workflow.title": "The workflow was invalid",
  "errors.invalid_workflow.what": "The generation workflow could not be built or was malformed.",
  "errors.invalid_workflow.why": "A setting combination produced a workflow ComfyUI could not accept.",
  "errors.invalid_workflow.fixes": "Reset the affected settings and retry. || Try a default preset to confirm the setup works. || Report it with the settings you used.",
  "errors.generation_interrupted.title": "Generation was stopped",
  "errors.generation_interrupted.what": "The generation did not finish.",
  "errors.generation_interrupted.why": "It was cancelled, or ComfyUI stopped partway through.",
  "errors.generation_interrupted.fixes": "Start the generation again. || Confirm ComfyUI is still running. || Check the logs if you did not cancel it.",
  "errors.io_permission.title": "File access was denied",
  "errors.io_permission.what": "The app could not read or write a needed file.",
  "errors.io_permission.why": "The folder is protected, read-only, or owned by another process.",
  "errors.io_permission.fixes": "Choose a folder your user account can write to. || Close any app locking the file. || Run from a location that is not write-protected.",
  "errors.serialization.title": "Could not read the data",
  "errors.serialization.what": "The app received data it could not parse.",
  "errors.serialization.why": "A response was malformed or a file was corrupted.",
  "errors.serialization.fixes": "Retry the action. || Restart the app. || Report it if it keeps happening.",
  "errors.card.details": "Technical details",
  "errors.card.copy_diagnostics": "Copy diagnostics",
  "errors.card.copied": "Copied",
  "errors.card.what_label": "What happened",
  "errors.card.why_label": "Why",
  "errors.card.fix_label": "How to fix it",
  "errors.card.report": "Report this error",
  "errors.report.title": "Report this error",
  "errors.report.intro": "This opens a prefilled GitHub issue. Your diagnostics log is copied to your clipboard so you can paste it into the issue.",
  "errors.report.note_label": "Anything else that helps (optional)",
  "errors.report.note_placeholder": "What were you doing when this happened?",
  "errors.report.submit": "Open issue",
  "errors.report.opening": "Opening...",
  "errors.report.copied_hint": "Diagnostics copied to clipboard. Paste them into the issue body.",
```

- [ ] **Step 3: Verify no missing interpolation and build**

Run: `npm run build`
Expected: succeeds. Confirm the `errors` block is valid TS (no trailing comma errors, matches file's quote style).

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/locales/en.ts
git -c core.hooksPath=/dev/null commit -m "feat(errors): add English strings for error catalog"
```

---

### Task 4: Report payload and sinks

**Files:**
- Create: `src/lib/errors/reportError.ts`
- Modify: `src/lib/types/index.ts:212` (add optional `report_endpoint` to `AppConfig`)

**Interfaces:**
- Consumes: `FriendlyError`, `ReportPayload`, `ReportSink` (Task 1); `appVersion`, `appMode`, `platformInfo` (Task 1); `openExternalUrl` (Task 1); `exportLogsContent` and `getConfig` from `src/lib/utils/api.ts`.
- Produces: `buildReportPayload(error, userNote?): Promise<ReportPayload>`; `reportError(error, userNote?): Promise<{ issueUrl?: string }>`; `GITHUB_REPO = "Mooshieblob1/MooshieUI"`.

- [ ] **Step 1: Add report_endpoint to AppConfig**

In `src/lib/types/index.ts`, inside the `AppConfig` interface (starts at line 212), add near `civitai_api_key`:

```ts
  /** When set, in-app error reports POST here (Sub-project B proxy) instead of opening a prefilled GitHub issue. */
  report_endpoint?: string | null;
```

- [ ] **Step 2: Create reportError.ts**

```ts
import type { FriendlyError, ReportPayload, ReportSink } from "./types.js";
import { appVersion, appMode, platformInfo } from "../utils/platformInfo.js";
import { openExternalUrl } from "../utils/openExternal.js";
import { exportLogsContent, getConfig } from "../utils/api.js";

export const GITHUB_REPO = "Mooshieblob1/MooshieUI";
const MAX_LOG_TAIL = 200_000; // ~200 KB cap for the proxy payload

/** Assemble a structured report from a resolved error. */
export async function buildReportPayload(
  error: FriendlyError,
  userNote?: string,
  includeLogs = false,
): Promise<ReportPayload> {
  const { os, arch } = platformInfo();
  let logsTail: string | undefined;
  if (includeLogs) {
    try {
      const logs = await exportLogsContent();
      logsTail = logs.length > MAX_LOG_TAIL ? logs.slice(-MAX_LOG_TAIL) : logs;
    } catch {
      logsTail = undefined;
    }
  }
  return {
    errorCode: error.code,
    rawMessage: error.raw,
    appVersion: appVersion(),
    os,
    arch,
    mode: appMode(),
    timestamp: new Date().toISOString(),
    userNote: userNote?.trim() || undefined,
    logsTail,
  };
}

/** Build a Markdown issue body. Plain ASCII, no em dashes. */
function issueBody(p: ReportPayload): string {
  const lines = [
    "### What happened",
    p.userNote || "(no description provided)",
    "",
    "### Error",
    "```",
    p.rawMessage || "(empty)",
    "```",
    "",
    "### Environment",
    `- App version: ${p.appVersion}`,
    `- OS: ${p.os}`,
    `- Arch: ${p.arch}`,
    `- Mode: ${p.mode}`,
    `- Error code: ${p.errorCode}`,
    `- When: ${p.timestamp}`,
    "",
    "### Diagnostics",
    "Paste your diagnostics log below (it was copied to your clipboard):",
    "",
    "```",
    "",
    "```",
  ];
  return lines.join("\n");
}

/** Phase A: open a prefilled GitHub New Issue page and copy diagnostics to clipboard. */
class PrefilledIssueSink implements ReportSink {
  async submit(payload: ReportPayload): Promise<{ issueUrl?: string }> {
    // Copy full diagnostics to clipboard since URL length is capped near 8 KB.
    try {
      const logs = await exportLogsContent();
      await globalThis.navigator?.clipboard?.writeText(logs);
    } catch {
      // Clipboard may be unavailable; the issue still opens.
    }
    const title = `[in-app] ${payload.errorCode}: ${payload.rawMessage.slice(0, 80)}`;
    const params = new URLSearchParams({
      title,
      body: issueBody(payload),
      labels: "bug,in-app-report",
    });
    const url = `https://github.com/${GITHUB_REPO}/issues/new?${params.toString()}`;
    await openExternalUrl(url);
    return { issueUrl: url };
  }
}

/** Phase B: POST to the NUC proxy, which creates the issue and returns its URL. */
class ProxySink implements ReportSink {
  constructor(private endpoint: string) {}
  async submit(payload: ReportPayload): Promise<{ issueUrl?: string }> {
    const resp = await fetch(this.endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Mooshie-App": "1" },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) throw new Error(`Report endpoint returned ${resp.status}`);
    const data = await resp.json().catch(() => ({}));
    return { issueUrl: data.issueUrl };
  }
}

/** Choose the active sink from config. Proxy when report_endpoint is set. */
async function activeSink(): Promise<{ sink: ReportSink; usesLogs: boolean }> {
  try {
    const cfg = await getConfig();
    const endpoint = cfg.report_endpoint;
    if (endpoint) return { sink: new ProxySink(endpoint), usesLogs: true };
  } catch {
    // Fall through to the prefilled sink.
  }
  return { sink: new PrefilledIssueSink(), usesLogs: false };
}

/** Report a resolved error via the active sink. */
export async function reportError(
  error: FriendlyError,
  userNote?: string,
): Promise<{ issueUrl?: string }> {
  const { sink, usesLogs } = await activeSink();
  const payload = await buildReportPayload(error, userNote, usesLogs);
  return sink.submit(payload);
}
```

- [ ] **Step 3: Verify build**

Run: `npm run build`
Expected: succeeds. Confirm `exportLogsContent` and `getConfig` import paths resolve (both are exported from `src/lib/utils/api.ts`).

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/errors/reportError.ts src/lib/types/index.ts
git -c core.hooksPath=/dev/null commit -m "feat(errors): add report payload builder and sinks"
```

---

### Task 5: ErrorCard component and report modal

**Files:**
- Create: `src/lib/components/errors/ErrorCard.svelte`
- Create: `src/lib/components/errors/ReportErrorModal.svelte`

**Interfaces:**
- Consumes: `FriendlyError` (Task 1); `reportError` (Task 4); `locale` singleton; `exportLogsContent` (api.ts).
- Produces: `ErrorCard` (props: `error: FriendlyError`, `compact?: boolean`); `ReportErrorModal` (props: `error: FriendlyError`, `onclose: () => void`).

- [ ] **Step 1: Create ErrorCard.svelte**

```svelte
<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { exportLogsContent } from "../../utils/api.js";
  import type { FriendlyError } from "../../errors/types.js";
  import ReportErrorModal from "./ReportErrorModal.svelte";

  let { error, compact = false }: { error: FriendlyError; compact?: boolean } = $props();

  let detailsOpen = $state(false);
  let copied = $state(false);
  let showReport = $state(false);

  async function copyDiagnostics() {
    try {
      const logs = await exportLogsContent();
      await navigator.clipboard.writeText(logs || error.raw);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      // Clipboard unavailable; ignore.
    }
  }
</script>

<div class="rounded-lg border border-neutral-700 bg-neutral-900 p-4 text-sm text-neutral-200">
  <h3 class="text-base font-semibold text-neutral-100">{error.title}</h3>
  <p class="mt-1 text-neutral-300">{error.what}</p>

  {#if !compact}
    <p class="mt-2 text-neutral-400"><span class="text-neutral-500">{locale.t("errors.card.why_label")}:</span> {error.why}</p>

    {#if error.fixes.length}
      <p class="mt-3 text-neutral-300">{locale.t("errors.card.fix_label")}</p>
      <ol class="mt-1 list-decimal space-y-1 pl-5 text-neutral-300">
        {#each error.fixes as fix}
          <li>{fix}</li>
        {/each}
      </ol>
    {/if}
  {/if}

  <div class="mt-3 flex flex-wrap items-center gap-2">
    {#if error.reportable}
      <button
        onclick={() => (showReport = true)}
        class="rounded-lg bg-indigo-600 px-3 py-1.5 text-white transition-colors hover:bg-indigo-500"
      >
        {locale.t("errors.card.report")}
      </button>
    {/if}
    <button
      onclick={() => (detailsOpen = !detailsOpen)}
      class="rounded-lg bg-neutral-800 px-3 py-1.5 text-neutral-300 transition-colors hover:bg-neutral-700"
    >
      {locale.t("errors.card.details")}
    </button>
  </div>

  {#if detailsOpen}
    <div class="mt-3 rounded-lg bg-neutral-950 p-3">
      <pre class="max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs text-neutral-400">{error.raw}</pre>
      <button
        onclick={copyDiagnostics}
        class="mt-2 rounded-lg bg-neutral-800 px-3 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-700"
      >
        {copied ? locale.t("errors.card.copied") : locale.t("errors.card.copy_diagnostics")}
      </button>
    </div>
  {/if}
</div>

{#if showReport}
  <ReportErrorModal {error} onclose={() => (showReport = false)} />
{/if}
```

- [ ] **Step 2: Create ReportErrorModal.svelte**

```svelte
<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { reportError } from "../../errors/reportError.js";
  import type { FriendlyError } from "../../errors/types.js";

  let { error, onclose }: { error: FriendlyError; onclose: () => void } = $props();

  let userNote = $state("");
  let submitting = $state(false);
  let copiedHint = $state(false);

  async function submit() {
    submitting = true;
    try {
      await reportError(error, userNote);
      copiedHint = true;
      setTimeout(onclose, 1500);
    } catch {
      submitting = false;
    }
  }
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
  onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
  onkeydown={(e) => { if (e.key === "Escape") onclose(); }}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="w-full max-w-md space-y-4 rounded-xl border border-neutral-700 bg-neutral-900 p-6 shadow-2xl">
    <h3 class="text-base font-semibold text-neutral-100">{locale.t("errors.report.title")}</h3>
    <p class="text-sm text-neutral-400">{locale.t("errors.report.intro")}</p>

    <div>
      <label class="mb-1 block text-xs text-neutral-400">{locale.t("errors.report.note_label")}</label>
      <textarea
        bind:value={userNote}
        placeholder={locale.t("errors.report.note_placeholder")}
        rows="4"
        class="w-full resize-y rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
      ></textarea>
    </div>

    {#if copiedHint}
      <p class="text-xs text-emerald-400">{locale.t("errors.report.copied_hint")}</p>
    {/if}

    <div class="flex justify-end gap-3 pt-1">
      <button onclick={onclose} class="rounded-lg bg-neutral-800 px-4 py-2 text-sm text-neutral-300 hover:bg-neutral-700">
        {locale.t("common.cancel")}
      </button>
      <button
        onclick={submit}
        disabled={submitting}
        class="rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {submitting ? locale.t("errors.report.opening") : locale.t("errors.report.submit")}
      </button>
    </div>
  </div>
</div>
```

- [ ] **Step 3: Verify build**

Run: `npm run build`
Expected: succeeds. Confirm `common.cancel` exists in `en.ts` (it is already used by the existing report modal).

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/components/errors/ErrorCard.svelte src/lib/components/errors/ReportErrorModal.svelte
git -c core.hooksPath=/dev/null commit -m "feat(errors): add ErrorCard and report modal"
```

---

### Task 6: Global error surface and wiring

**Files:**
- Create: `src/lib/stores/errorModal.svelte.ts`
- Create: `src/lib/components/errors/GlobalErrorModal.svelte`
- Modify: `src/App.svelte` (mount GlobalErrorModal)
- Modify: `src/lib/components/modelhub/ModelHubPage.svelte:900` (route install failure through showError)

**Interfaces:**
- Consumes: `resolveError` (Task 2); `ErrorCard` (Task 5); `FriendlyError` (Task 1).
- Produces: `errorModal` singleton with `show(raw: unknown): void`, `close(): void`, `current: FriendlyError | null`; convenience `showError(raw: unknown): void`.

- [ ] **Step 1: Create the errorModal store**

`src/lib/stores/errorModal.svelte.ts`:

```ts
import { resolveError } from "../errors/resolveError.js";
import type { FriendlyError } from "../errors/types.js";

class ErrorModalStore {
  current = $state<FriendlyError | null>(null);

  show(raw: unknown) {
    this.current = resolveError(raw);
  }

  close() {
    this.current = null;
  }
}

export const errorModal = new ErrorModalStore();

/** Resolve and display any error in the global modal. */
export function showError(raw: unknown) {
  errorModal.show(raw);
}
```

- [ ] **Step 2: Create GlobalErrorModal.svelte**

```svelte
<script lang="ts">
  import { errorModal } from "../../stores/errorModal.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import ErrorCard from "./ErrorCard.svelte";
</script>

{#if errorModal.current}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
    onclick={(e) => { if (e.target === e.currentTarget) errorModal.close(); }}
    onkeydown={(e) => { if (e.key === "Escape") errorModal.close(); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="w-full max-w-lg space-y-3">
      <ErrorCard error={errorModal.current} />
      <div class="flex justify-end">
        <button
          onclick={() => errorModal.close()}
          class="rounded-lg bg-neutral-800 px-4 py-2 text-sm text-neutral-300 hover:bg-neutral-700"
        >
          {locale.t("common.close")}
        </button>
      </div>
    </div>
  </div>
{/if}
```

- [ ] **Step 3: Mount GlobalErrorModal in App.svelte**

In `src/App.svelte`, add the import with the other component imports:

```ts
import GlobalErrorModal from "./lib/components/errors/GlobalErrorModal.svelte";
```

Add the component near the end of the root markup, alongside other top-level modals (search for an existing top-level `{#if}` modal such as the lightbox and place it beside that):

```svelte
<GlobalErrorModal />
```

- [ ] **Step 4: Route one real surface through showError**

In `src/lib/components/modelhub/ModelHubPage.svelte`, add to the imports:

```ts
import { showError } from "../../stores/errorModal.svelte.js";
```

In `installFromDirectUrl`, replace the raw catch assignment (around line 900):

```ts
    } catch (e) {
      directStatus = e instanceof Error ? e.message : String(e);
    } finally {
```

with:

```ts
    } catch (e) {
      showError(e);
      directStatus = null;
    } finally {
```

- [ ] **Step 5: Verify build and manual check**

Run: `npm run build`
Expected: succeeds.
Manual: run `npm run tauri dev`, enter a bad direct-download URL in Model Hub (for example a 404 link), click Download, and confirm the global error modal shows friendly text with a working Report this error button.

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/stores/errorModal.svelte.ts src/lib/components/errors/GlobalErrorModal.svelte src/App.svelte src/lib/components/modelhub/ModelHubPage.svelte
git -c core.hooksPath=/dev/null commit -m "feat(errors): add global error modal and wire Model Hub"
```

---

### Task 7: Dev-only error gallery

**Files:**
- Create: `src/lib/components/errors/ErrorGallery.svelte`
- Modify: `src/App.svelte` (mount behind a dev-only toggle)

**Interfaces:**
- Consumes: `CATALOG`, `catalogIds` (Task 2); `resolveError` (Task 2); `ErrorCard` (Task 5).
- Produces: `ErrorGallery` component; opened via a URL hash `#error-gallery` in dev builds only.

- [ ] **Step 1: Create ErrorGallery.svelte**

Renders every catalog id plus the generic fallback through `ErrorCard`, using a representative raw string per id so the resolver path is exercised.

```svelte
<script lang="ts">
  import { catalogIds } from "../../errors/errorCatalog.js";
  import { resolveError } from "../../errors/resolveError.js";
  import ErrorCard from "./ErrorCard.svelte";

  // A representative raw message per id, chosen to match that id's matcher.
  const SAMPLES: Record<string, string> = {
    comfyui_not_running: "Failed to connect to ComfyUI: not running",
    connection_failed: "Connection failed: os error 111",
    websocket_dropped: "WebSocket error: connection closed",
    api_error_5xx: "API error (503): Service Unavailable",
    download_404: "HTTP error: 404 Not Found",
    disk_full: "No space left on device",
    checksum_mismatch: "sha256 mismatch for downloaded file",
    civitai_auth: "CivitAI download failed: 401 unauthorized (api key)",
    hf_page_url: "This looks like a huggingface.co /blob/ page url",
    model_not_found: "Model not found: sd_xl_base.safetensors",
    download_network: "HTTP error: request timed out",
    comfyui_launch_failed: "Failed to start ComfyUI: spawn error",
    python_env_broken: "python: No module named torch",
    attention_backend_install: "flash-attn install failed",
    out_of_memory: "CUDA out of memory",
    unsupported_gpu: "No CUDA device detected",
    missing_node: "Unknown node: ImpactWildcardProcessor",
    invalid_workflow: "Invalid workflow: malformed graph",
    generation_interrupted: "Generation was interrupted",
    io_permission: "IO error: permission denied",
    serialization: "Serialization error: expected value at line 1",
  };

  const ids = [...catalogIds(), "unknown"];
  function sampleFor(id: string): string {
    if (id === "unknown") return "some totally unrecognized failure text";
    return SAMPLES[id] ?? id;
  }
</script>

<div class="fixed inset-0 z-[60] overflow-auto bg-neutral-950 p-6">
  <h2 class="mb-4 text-lg font-semibold text-neutral-100">Error gallery ({ids.length})</h2>
  <div class="grid gap-4 md:grid-cols-2">
    {#each ids as id}
      <div>
        <p class="mb-1 text-xs text-neutral-500">{id}</p>
        <ErrorCard error={resolveError(sampleFor(id))} />
      </div>
    {/each}
  </div>
</div>
```

- [ ] **Step 2: Mount behind a dev-only hash toggle in App.svelte**

In `src/App.svelte` add the import:

```ts
import ErrorGallery from "./lib/components/errors/ErrorGallery.svelte";
```

Add reactive state and mount (dev builds only). `import.meta.env.DEV` is true only in `npm run tauri dev` / vite dev, so the gallery never ships in production:

```svelte
<script lang="ts">
  // ...existing script...
  let showErrorGallery = $state(
    import.meta.env.DEV && globalThis.location?.hash === "#error-gallery",
  );
</script>

<!-- near the other top-level modals -->
{#if showErrorGallery}
  <ErrorGallery />
{/if}
```

- [ ] **Step 3: Verify build and view the gallery**

Run: `npm run build`
Expected: succeeds, and the production bundle does not include the gallery (guarded by `import.meta.env.DEV`).
Manual: run `npm run tauri dev`, navigate to the app URL with `#error-gallery` appended, and confirm every entry renders with populated title/what/why/fix text (no raw `errors.*` keys showing, which would indicate a missing i18n key).

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/components/errors/ErrorGallery.svelte src/App.svelte
git -c core.hooksPath=/dev/null commit -m "feat(errors): add dev-only error gallery"
```

---

### Task 8: Propagate i18n keys to all locales

**Files:**
- Modify: all non-English files in `src/lib/locales/` (`de.ts`, `es.ts`, `fr.ts`, `it.ts`, `ja.ts`, `ko.ts`, `pl.ts`, `pt.ts`, `ru.ts`, `zh.ts`, `zh-tw.ts`, and any others present)
- Reference: `scripts/i18n-*` (existing gap-fill workflow)

**Interfaces:**
- Consumes: the `errors.*` keys added to `en.ts` in Task 3.
- Produces: the same `errors.*` key set present in every locale file (machine-translated values; English acceptable as fallback where a script cannot translate).

- [ ] **Step 1: Inspect the existing i18n tooling**

Run: `ls scripts/ | grep i18n`
Read the scripts referenced in CLAUDE.md (`scripts/i18n-sweep-keys.txt`, `scripts/i18n-gap-*.json`) and any runner script to learn how keys are propagated. Follow that established workflow rather than inventing a new one.

- [ ] **Step 2: Run the gap-fill workflow for the new keys**

Use the existing workflow to detect keys present in `en.ts` but missing elsewhere and fill them (machine translation). If the repo has a script entry, run it; otherwise follow the documented manual sweep in `docs`/CLAUDE.md. The goal: every `errors.*` key from Task 3 exists in every locale file.

- [ ] **Step 3: Verify parity**

Run: `npm run build`
Expected: succeeds. Then spot-check two locale files (for example `de.ts` and `ja.ts`) to confirm they contain an `errors` block with `generic`, `card`, `report`, and the 21 catalog ids.

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/locales/
git -c core.hooksPath=/dev/null commit -m "i18n: propagate error catalog strings to all locales"
```

---

### Task 9: Final validation and self-check

**Files:** none (verification only)

- [ ] **Step 1: Full build gate**

Run: `npm run build`
Expected: succeeds with no new errors.

- [ ] **Step 2: Rust unchanged sanity**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: succeeds (this sub-project touches no Rust; this confirms nothing regressed).

- [ ] **Step 3: Manual smoke of the full flow**

Run `npm run tauri dev`:
1. Open `#error-gallery`, confirm all entries render with real text.
2. Trigger a real Model Hub download error, confirm the global modal appears.
3. Click Report this error, add a note, click Open issue.
4. Confirm a GitHub New Issue page opens with a prefilled title/body and that the clipboard holds the diagnostics log.

- [ ] **Step 4: Confirm no production leakage**

Confirm the built `dist/` bundle excludes the gallery (guarded by `import.meta.env.DEV`) and that no GitHub token or secret exists anywhere in the frontend.

---

## Notes for the implementer

- Incremental adoption: Task 6 wires one real surface (Model Hub) as the proven pattern. Converting additional `catch` sites to `showError(e)` is safe follow-up work, not part of this plan's completion criteria.
- Sub-project B (NUC proxy + Cloudflare Tunnel) is a separate plan. This plan already ships the `ProxySink` and `report_endpoint` config so B activates by setting one config value, with no further frontend changes.
- If `en.ts` uses flat dotted keys rather than nested objects, flatten the Task 3 block to match; the resolver reads `errors.<id>.<field>` either way through `locale.t`.
