# Prompt Assistant — Local LLM Prompt Enhance & Compose — Design

**Date:** 2026-06-13
**Status:** Approved

## Goal

Let users install a small, local LLM (their choice from a curated catalog, auto-recommended
by hardware) that **enhances** their existing prompt or **composes** a brand-new prompt from a
plain-language description. The assistant must produce output that **strictly adheres** to the
active model family's prompting convention:

- **Tag-only families** (illustrious, pony, and other danbooru-trained checkpoints): comma-
  separated danbooru tags only.
- **Anima family:** natural language + Gelbooru tags + known artists prefixed with `@`.

Everything is seamless: one **✨ Enhance** button enhances the current prompt in place; one
**✍ Compose** button opens a modal to describe what you want. On first use a setup modal appears
with a hardware-aware **pre-selected** model recommendation that the user can override then or
later. Supports **GGUF** (CPU/any GPU) and **NVFP4** (Blackwell-class GPUs only) with automatic
hardware detection.

## Non-Goals (v1)

- Arbitrary/custom model URLs. Catalog is curated for v1; custom entries are a later iteration.
- A chat UI or multi-turn conversation. Single request → single result.
- Cloud/remote LLM providers. Local-only, mirroring the privacy posture of the interrogator.
- Fine-tuning or training. We ship/download prebuilt weights only.

## Decisions (locked during brainstorming)

| Question | Decision |
|----------|----------|
| Scope | Full design, phased build |
| Adherence mechanism | Grounding + post-filter repair (not a hard GBNF grammar) |
| Catalog | Curated only for v1; custom entries later |
| VRAM strategy | On-demand load + idle unload |
| Runtime | `llama-server` subprocess (option A) |
| Model selection | **Auto-select best fit from GPU at launch; user overrides in modal/Settings** |

## Architecture Overview

```
Frontend (Svelte)                Rust (Tauri / axum)                 Subprocess
─────────────────                ───────────────────                 ──────────
✨ Enhance / ✍ Compose  ──ipc──►  promptassistant::commands  ──http──►  llama-server
  promptAssistant.svelte.ts        - lifecycle (spawn/health/unload)     (OpenAI-compatible
  api.ts typed wrappers            - on-demand load + idle watchdog       /v1/chat/completions
  setup / compose modals           - hardware detect (GB10/NVFP4)         /health)
                                    - grounding + post-filter repair
                                    - catalog + download_with_progress
```

The frontend **never** talks to llama-server directly — Rust owns the subprocess and proxies all
requests. Consequences: no CSP change needed (no new remote/img/fetch origin), and **browser/web
mode works unchanged** because it already routes IPC through the embedded axum server.

This mirrors the two existing precedents in the codebase:

- **ComfyUI** subprocess + HTTP lifecycle management (process spawn, health poll, child-survives
  config) in `src-tauri/src/comfyui/`.
- **Interrogator** model-download-and-run pattern in `src-tauri/src/interrogator.rs` and
  `src-tauri/src/commands/interrogator.rs` (download-with-progress, ensure-downloaded, blocking
  inference, `interrogator:*` events).

## Backend

### Runtime: `llama-server`

We run llama.cpp's `llama-server`, which exposes an OpenAI-compatible `/v1/chat/completions`
endpoint plus a `/health` endpoint. Rust:

1. On first use, ensures the correct **prebuilt `llama-server` binary** for the platform + the
   best available acceleration backend is downloaded (CUDA / Vulkan / Metal / HIP / SYCL / CPU),
   using the same `download_with_progress` flow as the interrogator's ONNX Runtime download. The
   binary is **not** bundled (it is large and accel-specific); it is fetched on demand like the
   ORT shared library.
2. Spawns `llama-server` with no console window (same no-window subprocess flags as
   `setup.rs`/ComfyUI), bound to `127.0.0.1` on an ephemeral port, with `--n-gpu-layers` computed
   from VRAM headroom (see VRAM management).
3. Polls `/health` until ready, then proxies chat-completion requests.
4. Unloads on idle (see VRAM management).

NVFP4 weights require a recent CUDA-enabled `llama-server` build; the catalog gates NVFP4 model
variants behind the Blackwell-capability flag so a non-capable host never downloads or selects them.

### Hardware detection (`detect_llm_hardware`)

Extends the existing `detect_gpus()` in `src-tauri/src/comfyui/gpu_manager.rs` (which already
queries `nvidia-smi` for `index,name,memory.total`). New command returns, per GPU and overall:

```rust
struct LlmHardware {
    gpus: Vec<LlmGpu>,          // name, vram_mb, vendor, nvfp4_capable
    total_vram_mb: u64,
    system_ram_mb: u64,
    nvfp4_capable: bool,        // any GPU is Blackwell-class
    recommended_model_id: String,
}
```

**Blackwell / NVFP4 classification.** `nvfp4_capable` is true when a detected GPU is Blackwell-
class. The detection set (matched case-insensitively against the GPU name, with room to grow):

- GeForce **RTX 50-series** (RTX 5060 / 5070 / 5080 / 5090, incl. Ti/Super/Laptop variants)
- **RTX PRO 6000 Blackwell** (and other RTX PRO Blackwell workstation cards)
- Data-center **B200 / GB200**
- **GB10 (Grace-Blackwell) / DGX Spark**  ← explicitly included per requirement

GB10 is Grace-Blackwell (`sm_12x`) and is treated on the same NVFP4 path as the RTX 50-series and
GB200. Classification is name-based with a fallback to CUDA compute-capability when available
(compute capability ≥ 10.0 ⇒ Blackwell), so future Blackwell SKUs are caught even if the name
list lags.

### Auto model selection (launch) + override

On app launch (or first hardware query), Rust computes `recommended_model_id` from detected
hardware using the catalog's per-model requirements:

- Pick the **largest catalog model that fits comfortably** in available VRAM (GPU path) or system
  RAM (CPU path), preferring an NVFP4 variant when `nvfp4_capable`, else the appropriate GGUF
  quant.
- This recommendation is **pre-selected** in the setup modal. It does **not** silently download
  multi-GB weights — download still requires one explicit confirmation (privacy/bandwidth), after
  which the requested Enhance/Compose action runs automatically.
- The user can switch to any other catalog model in the setup modal at first use, or later via
  Settings → Prompt Assistant. The chosen model id is persisted in config (guarded `!== undefined`).

### Curated catalog (v1)

Catalog is a static, versioned list in Rust (mirrored to a TS type for the cards). Each entry:

```rust
struct LlmCatalogEntry {
    id: String,
    name: String,
    purpose: ModelPurpose,        // TagUpsampler | NaturalLanguage
    families: Vec<ModelFamily>,   // which checkpoint families it serves well
    variants: Vec<LlmVariant>,    // { format: Gguf{quant} | Nvfp4, size_mb, vram_mb, repo, file }
    pros: String,
    cons: String,
    best_for: String,
}
```

Representative tiers (exact HF repos/filenames pinned during implementation):

| Tier | Example | Purpose | Footprint | Notes |
|------|---------|---------|-----------|-------|
| Tiny | DanTagGen / TIPO (KohakuBlueleaf) | Tag upsampling for **tag-only** families | ~0.3–0.7 GB GGUF | Runs on CPU; purpose-built for danbooru tags |
| Small | 1–3B instruct (GGUF Q4/Q5) | Natural language + tags (Anima) and compose | ~1–2 GB | Good default for laptops / 6–8 GB VRAM |
| Medium | 7–8B instruct (GGUF Q4/Q5, or NVFP4) | Higher-quality compose/enhance | ~4–6 GB | Default for ≥12 GB VRAM; NVFP4 on Blackwell |

NVFP4 variants appear only when `nvfp4_capable`. Cards the host can't comfortably run are **dimmed
with a reason** ("needs ~6 GB VRAM"), not hidden.

### Grounding + post-filter repair (strict adherence)

Adherence is enforced in two stages, not by a hard grammar (chosen for flexibility while keeping
output valid):

1. **Grounding (retrieval).** Lexical/keyword retrieval over shipped corpora seeds the prompt with
   a short, relevant candidate set:
   - Tag corpus (the autocomplete/danbooru tag list already shipped).
   - Artist corpus — reuse `src/lib/assets/anima-tags.json` (entries `{n, c}`, `c===1` ⇒ artist)
     for the Anima `@artist` set. The same normalization helpers from `InterrogateModal.svelte`
     (`normalizeArtistTagForLookup`, `formatArtistTagForAnima`) define the canonical forms.
   The retrieved candidates + a family-specific system prompt steer the model toward real tags.

2. **Post-filter repair.** The raw model output is parsed and repaired in Rust before it ever
   reaches the prompt box:
   - **Tag-only families:** split to comma tags, drop prose/sentences, validate each tag against
     the tag corpus (fuzzy-snap near-misses, drop unrecoverable hallucinations), dedupe, normalize
     underscores/spaces and parenthesis escaping.
   - **Anima:** allow natural-language clause(s) + Gelbooru tags; force any recognized artist into
     `@artist` form via the anima-tags lookup (paren-escaped), matching `formatArtistTagForAnima`.
   - Strip weights the user didn't ask for, collapse whitespace, cap length.

The system prompt and repair rules are selected from `generation.modelFamily` (the active family
getter set: `isAnima`, `isIllustrious`, `isPony`, `isNanosaur`, etc.), passed from the frontend on
each request.

### VRAM management: on-demand load + idle unload

- **On-demand load:** the server is spawned on first Enhance/Compose, not at app start.
- **Idle unload watchdog:** an idle timer (default ~120s, configurable; mirrors the existing
  browser-server idle heartbeat watchdog) terminates `llama-server` to free VRAM. Next request
  re-spawns transparently (UI shows a "loading model" stage like the interrogator).
- **Generation guard:** the LLM will not load/run during an active ComfyUI generation to avoid
  VRAM contention; if asked, it waits or surfaces a clear toast.
- **Manual control:** Settings exposes "Unload now" and the idle-timeout slider.

### Commands & events

`#[tauri::command]`, each returning `Result<T, AppError>`, registered in `lib.rs`
`generate_handler![]`, using shared `state.http_client` and the re-exported `State`:

- `detect_llm_hardware() -> LlmHardware`
- `list_llm_catalog() -> Vec<LlmCatalogEntry>`
- `download_llm_model(id, variant)` — emits `llm:download_progress`
- `delete_llm_model(id)`
- `enhance_prompt({ prompt, family, opts }) -> String`
- `compose_prompt({ description, family, opts }) -> String`
- `unload_llm()`
- `llm_status() -> { installed_model, server_state }`

Events mirror the interrogator: `llm:download_progress` (`{ downloaded, total, filename, done }`)
and `llm:stage` (`"loading_model"`, `"generating"`).

## Frontend

### Button placement

The new buttons live in the existing thin action row under the positive-prompt header in
`src/lib/components/generation/PromptInputs.svelte` (currently the `mb-1 flex justify-end` row that
holds the **Regional** button, lines ~140–158). That row becomes a split toolbar:

```
 Positive Prompt                    [quality applied] [✦ style ×1.0] …
 ┌──────────────────────────────────────────────────────────────┐
 │ ✨ Enhance   ✍ Compose                              [ Regional ]│
 └──────────────────────────────────────────────────────────────┘
 ┌──────────────────────────────────────────────────────────────┐
 │ (positive prompt textarea)                                     │
 └──────────────────────────────────────────────────────────────┘
```

- **✨ Enhance** — operates on the current positive prompt in place. Disabled (with tooltip) when
  the prompt is empty.
- **✍ Compose** — opens the compose modal.
- Styling matches the Regional button (`rounded-lg border px-2 py-0.5 text-[10px]`). Both show a
  spinner and disable while loading/generating.
- Buttons are **positive-prompt only** — deliberately not added to the shared `PromptTextarea`
  weight-toolbar (which the negative prompt also uses).
- **First-ever click of either** opens the setup modal (with the recommended model pre-selected),
  then runs the requested action once a model is installed.

### Setup modal (`PromptAssistantSetupModal.svelte`)

Opens on first Enhance/Compose, or from Settings. Single modal (not a stepper):

1. **Hardware banner** from `detect_llm_hardware`, e.g. *"RTX 5090 — 32 GB VRAM, Blackwell (NVFP4
   supported)"*, *"GB10 DGX Spark — NVFP4 supported"*, or *"CPU only — small models recommended."*
2. **Recommended model pre-selected** and badged ("Recommended for your hardware").
3. **Curated model cards**: name, on-disk size, RAM/VRAM need, "best for", pros/cons. Unfit cards
   dimmed with reason. NVFP4 only offered when `nvfp4_capable`.
4. **Format toggle** per card where applicable: `GGUF Q4_K_M | Q5_K_M | Q8 | NVFP4 (Blackwell)`.
5. **Download** → progress bar (reusing the `llm:download_progress` pattern). On completion the
   modal closes and the originally-requested action runs automatically.

### Enhance flow (inline, no modal)

High-frequency action, kept fast:

- Click ✨ Enhance → spinner → Rust ensures server up (on-demand) → ground + generate + repair →
  result applied to the positive prompt.
- **Apply semantics reuse `InterrogateModal` logic**: Anima → natural language + Gelbooru tags +
  `@artist` formatting via `anima-tags.json`; tag-only families → comma tags.
- A transient **"↩ Undo"** affordance appears ~10s after applying (prompt history already exists,
  so revert is cheap).
- Errors (server down, OOM, busy) surface via `gallery.showToast`.

### Compose flow (`PromptComposeModal.svelte`)

- Textarea: *"Describe what you want…"* (any language).
- Optional toggles: target length (short / medium / detailed); "include artists" (Anima only).
- **Generate** → preview pane → **Replace** / **Append** write into the positive prompt (mirrors
  InterrogateModal's replace/append modes).

### Settings → Prompt Assistant

New section alongside the interrogator threshold settings: installed model(s) + disk usage,
**switch/download other model** (reopens setup modal), **delete model**, idle-unload timeout
slider, manual **Unload now**.

### Store & IPC

- New `src/lib/stores/promptAssistant.svelte.ts` class singleton with `$state` for
  `installedModel`, `recommendedModel`, `hardware`, `isGenerating`, `serverStatus`. Per repo rules
  it **does not import other stores**; `App.svelte` wires the apply-to-`generation` step. Persisted
  fields guarded with `!== undefined`; `saveSettings()` called explicitly after mutations.
- Typed wrappers in `src/lib/utils/api.ts` over `ipcInvoke`/`ipcListen`: `detectLlmHardware`,
  `listLlmCatalog`, `downloadLlmModel`, `deleteLlmModel`, `enhancePrompt`, `composePrompt`,
  `unloadLlm`, `llmStatus`. Never raw Tauri `invoke`/`listen`.
- i18n: all new keys + `{placeholders}` added to `src/lib/locales/en.ts` **and every other locale
  file** (missing keys silently fall back to English).

## Data Flow (Enhance)

```
user clicks ✨ Enhance
  → promptAssistant.enhance(generation.positivePrompt, generation.modelFamily)
  → api.enhancePrompt(...) ──ipc──► enhance_prompt command
      → ensure server (download binary/model if needed; spawn; /health)   [llm:stage]
      → ground: retrieve candidate tags/artists from shipped corpora
      → POST /v1/chat/completions (family system prompt + grounding)
      → post-filter repair (validate/repair against corpora; @artist for Anima)
      → return cleaned prompt string
  → App.svelte applies result to generation.positivePrompt (reusing InterrogateModal format logic)
  → transient Undo affordance; idle watchdog will later unload the server
```

## Error Handling

- **Binary/model download failure:** surfaced in the setup modal / toast with retry; partial files
  cleaned up (interrogator pattern).
- **Server spawn / health timeout:** clear error, offer retry; never hang the button.
- **OOM / insufficient VRAM:** caught from llama-server; suggest a smaller catalog variant.
- **Busy (ComfyUI generating):** generation guard returns a "try again after generation" toast.
- **Empty/garbage model output:** post-filter repair yields empty → keep original prompt, toast
  "couldn't enhance, try again."

## Reuse Map

| Need | Reuse |
|------|-------|
| Download with progress | `interrogator.rs` `download_with_progress` + `*:download_progress` events |
| Subprocess (no window) | `setup.rs` / ComfyUI spawn flags |
| GPU detection | `gpu_manager.rs` `detect_gpus()` |
| Idle-unload watchdog | browser-server idle heartbeat pattern |
| Anima artist corpus + formatting | `anima-tags.json`, `normalizeArtistTagForLookup`, `formatArtistTagForAnima` |
| Apply replace/append to prompt | `InterrogateModal.svelte` |
| Family routing | `generation.svelte.ts` family getters + `modelFamily` |

## Phased Build Plan

1. **Backend runtime** — `llama-server` lifecycle (download binary, spawn, health, on-demand load,
   idle unload, generation guard), `detect_llm_hardware` (incl. GB10/NVFP4 + compute-cap fallback),
   catalog, model download/delete, grounding + post-filter repair, commands + events. Gate:
   `cargo check` + `cargo clippy`.
2. **IPC + store** — `api.ts` wrappers, `promptAssistant.svelte.ts`, `App.svelte` wiring,
   auto-select-at-launch hook. Gate: `npm run build`.
3. **Setup modal + Settings** — model cards, hardware banner, recommended pre-selection, download
   progress, management UI.
4. **Enhance (inline) + Compose modal** — the two buttons, apply/undo, toasts, full i18n across all
   locales.
5. **Polish** — empty/error/OOM/busy states, idle-unload UX, docs.

Each phase ends green on `npm run build` + `cargo check`.

## Risks / Watch-Items

- **Binary distribution size** — accel-specific `llama-server` prebuilts are large; downloaded on
  demand rather than bundled. NVFP4 needs a recent CUDA build.
- **VRAM contention with ComfyUI** — mitigated by idle-unload + the active-generation guard.
- **Grounding quality is the hard part** — the post-filter repair (validate against corpora, repair
  hallucinations, enforce `@artist` for Anima) is what makes "strict adherence" real; budget
  iteration here. This is the primary quality risk.
- **Catalog drift** — pinned HF repos/files can move; keep the catalog versioned and easy to update.
