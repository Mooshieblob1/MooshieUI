# Prompt Assistant 1.4.19 — Design

Date: 2026-06-15
Status: Approved design, pending spec review

## Background

Two production problems on the hosted `mooshieui.gpu.garden` deployment (browser/server mode, 3 GPUs) were fixed live via config/env:

- A **524 timeout** on Enhance/Compose, root-caused to `llama-server` defaulting (CUDA `FASTEST_FIRST`) onto the near-full RTX 4070 instead of the idle Quadro RTX 8000. Worked around with `CUDA_VISIBLE_DEVICES=2`.
- **Garbage tag-dump output**, root-caused to the no-model fallback loading `dantaggen-l` (a tag upsampler, first in catalog order). Worked around by pinning `prompt_assistant_model_id` in the ConfigMap.

A third bug was then found: the **compose length selector does nothing** on the deployment, and compose output is short/unimpressive.

This release replaces the manual workarounds with code fixes, fixes compose, and adds the requested mod/admin per-generation GPU picker.

## Goals

1. A fresh deployment needs no manual `CUDA_VISIBLE_DEVICES` env var or `prompt_assistant_model_id` config to get correct, GPU-accelerated Enhance/Compose.
2. The compose length selector produces visibly different, higher-quality output across `short` / `medium` / `detailed`.
3. Moderators and admins can pin an individual image generation to a specific GPU. (Prompt enhancing is unaffected — it always uses the most-VRAM GPU.)

## Non-goals

- Replacing `llama-server` with Ollama or another runtime.
- Per-user GPU quotas, scheduling fairness, or load balancing beyond the existing first-available worker reservation.
- Exposing the GPU picker to regular (non-mod) users.

---

## A. Prompt-assistant reliability

### A1. No-model fallback prefers a natural-language model

When `prompt_assistant_model_id` is unset, the server path currently falls back to
`installed_models().next()` ([webserver.rs:4424-4432](../../../src-tauri/src/webserver.rs)), which returns
the first catalog entry (`dantaggen-l`). Replace this with
`catalog::recommend_model_id(total_vram_mb, system_ram_mb)`, which already prefers the largest-fitting
natural-language model and only falls back to DanTagGen when nothing else fits. Detect hardware
(`hardware::detect`) for the VRAM/RAM inputs, which the function already does just below the fallback.

### A2. Pin enhance to the most-VRAM GPU in code

Before spawning `llama-server` ([server.rs](../../../src-tauri/src/prompt_assistant/server.rs) `ensure_running`),
detect the NVIDIA GPU with the highest **total** VRAM and pin the child to it:

- Set `CUDA_DEVICE_ORDER=PCI_BUS_ID` so CUDA indices match nvidia-smi/PCI order.
- Set `CUDA_VISIBLE_DEVICES=<pci index of the max-total-VRAM GPU>`.

Detection reuses the nvidia-smi query already in [gpu_manager.rs](../../../src-tauri/src/comfyui/gpu_manager.rs)
(`detect_free_vram_mb` queries per-GPU memory; add/extend a helper returning the index of the GPU with the
greatest total memory). The child process overrides inherited env (same pattern as ComfyUI workers at
`process.rs:1154`), so this is robust regardless of host `CUDA_VISIBLE_DEVICES`. Removes the need for the
deployment env var. If detection fails (no nvidia-smi, no GPUs), spawn without pinning (current behavior).

### A3. Remove the abandoned free-VRAM code

Delete `free_comfyui_vram_for_llm()` from [state.rs](../../../src-tauri/src/state.rs) and its call site in
[webserver.rs](../../../src-tauri/src/webserver.rs). It was staged against a wrong theory (GPU contention);
the real cause was GPU targeting, now fixed by A2.

---

## B. Compose length and quality

### B1. Unify the desktop and server code paths

There are two implementations of enhance/compose:

- Desktop: `commands/prompt_assistant.rs::run_generation` — reads `opts.length`, maps to `max_tokens`
  (`short=96`, `medium=192`, `detailed=384`).
- Server/browser: `webserver.rs::run_prompt_assistant_headless` — a stale fork that never reads `opts` and
  hardcodes `max_tokens=192`. The dispatch handler ([webserver.rs:4173-4187](../../../src-tauri/src/webserver.rs))
  never parses `args["opts"]`.

This divergence is the root cause of the length selector being a no-op on the deployment.

**Fix:** extract the shared core into one non-Tauri function in the `prompt_assistant` module, taking
`(state, input, family, mode, opts)`. Both the desktop Tauri command and the webserver dispatch call it. The
webserver handler parses `args["opts"]` into `PromptAssistantOpts` (defaulting when absent). The Tauri-only
`AppHandle` argument stays out of the shared core (it was only used for the active-generation guard, which the
headless path already omits by design).

### B2. Length shapes the system prompt, not just the token cap

Compose output is ~30 tokens and stops at EOS well under any cap, so raising `max_tokens` alone changes
nothing. `grounding::system_prompt` gains a length parameter (or a dedicated compose variant) so the Compose
instruction varies:

- `short` — a tight, core tag set plus a brief phrase.
- `medium` — current behavior (tags + one detailed sentence).
- `detailed` — richer tag coverage plus 2-3 complete sentences describing scene, lighting, composition, and
  mood, still tags-first and comma-joined per the Anima format.

Token caps are bumped so `detailed` (e.g. 512) is not the binding constraint. The base Compose wording is
improved generally (it is currently a single terse line). Enhance is unchanged. The tag-only (danbooru-family)
path keeps its existing length handling; length shaping primarily targets the natural-language (Anima) compose
path where prose length is meaningful.

---

## C. Mod/admin per-generation GPU picker

Enhancing always uses the most-VRAM GPU (section A2). This section is only about image **generation**.

### C1. Parameter plumbing

Add `preferred_gpu_index: Option<u32>` to the generation params. Threaded:
frontend selector → generation store `toParams()` (camelCase `preferredGpuIndex` → snake_case
`preferred_gpu_index`) → generate Tauri command / webserver dispatch → `gpu_manager::submit_prompt`.

### C2. Server-side authorization

The webserver only honors `preferred_gpu_index` when `resolve_role(state, headers, remote) >= Moderator`
(reusing the existing role machinery at [webserver.rs:108](../../../src-tauri/src/webserver.rs)). A regular
user's value is **silently ignored** (treated as `None`), not an error. On desktop (local trust), the value is
always honored.

### C3. Worker reservation honoring the pin

`submit_prompt` currently reserves the first available worker via `find_available()` + `try_reserve()`. When a
valid `preferred_gpu_index` is supplied, target the worker whose `gpu_index` matches. **If that worker is busy,
wait for it** (queue on the chosen GPU) rather than falling back to another GPU — this honors the explicit
intent. An out-of-range or disabled index falls back to normal first-available behavior.

### C4. UI

A GPU dropdown in the generation settings, listing enabled workers by their `label` (e.g. "Quadro RTX 8000",
"RTX 3090 Ti"), with a default "Auto" option (`None`). Shown only when the current role is Moderator or Admin
(desktop always shows it). This requires exposing the current user's role to the frontend — add a small field
to an existing status/whoami response (the server already has `resolve_role`; surface it via a lightweight
read). New i18n keys for the label and "Auto" option must be added to `en.ts` and all other locale files.

---

## Touchpoints summary

| Area | Files |
|------|-------|
| A1 fallback | `src-tauri/src/webserver.rs`, `src-tauri/src/prompt_assistant/catalog.rs` |
| A2 enhance GPU pin | `src-tauri/src/prompt_assistant/server.rs`, `src-tauri/src/comfyui/gpu_manager.rs` |
| A3 cleanup | `src-tauri/src/state.rs`, `src-tauri/src/webserver.rs` |
| B unify + length | `src-tauri/src/commands/prompt_assistant.rs`, `src-tauri/src/webserver.rs`, `src-tauri/src/prompt_assistant/grounding.rs`, `src-tauri/src/prompt_assistant/mod.rs` |
| C picker | `src-tauri/src/comfyui/gpu_manager.rs`, generate command + `lib.rs`, `src/lib/stores/generation.svelte.ts`, generation settings UI component, `src/lib/utils/api.ts`, `src/lib/locales/*.ts` |

## Validation

No test framework exists. Gates: `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`,
`cargo fmt` + `cargo clippy`. Manual: on the deployment, confirm (a) enhance loads the recommended NL model on
the Quadro with no env/config, (b) compose `short`/`medium`/`detailed` produce distinctly different output,
(c) a mod-pinned generation runs on the chosen GPU and waits when it is busy, (d) a regular user's pin is
ignored.

## Rollout

Ship as 1.4.19 (version bump in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`;
`RELEASE_NOTES.md` + `CHANGELOG.md`). After release, the deployment's manual `CUDA_VISIBLE_DEVICES=2` env var
and `prompt_assistant_model_id` ConfigMap key become redundant and can be removed (optional; harmless if left).
