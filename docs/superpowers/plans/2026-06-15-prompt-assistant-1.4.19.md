# Prompt Assistant 1.4.19 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Prompt Assistant Enhance/Compose work correctly and GPU-accelerated on the hosted `mooshieui.gpu.garden` multi-GPU deployment, unify desktop/server code paths, length-shape the compose prompt, and add a mod/admin per-generation GPU picker, shipped as release 1.4.19.

**Architecture:** A new ungated `prompt_assistant/run.rs` holds the single shared Enhance/Compose core that both the Tauri command wrappers and the webserver headless dispatcher call. The offload gate and llama-server launch pin to the largest-VRAM GPU via `CUDA_DEVICE_ORDER=PCI_BUS_ID` + `CUDA_VISIBLE_DEVICES`. A `preferred_gpu_index` field is plumbed end-to-end (Svelte store → `toParams` → `GenerationParams` → `submit_prompt`), enforced for moderators/admins only, and surfaced through a role-gated `<GpuPicker>` dropdown.

**Tech Stack:** Rust (Tauri v2 + axum dual-mode), Svelte 5 runes, llama.cpp server, ComfyUI GPU worker pool, NVML/`nvidia-smi` VRAM queries.

---

## Project gates (this repo has NO test framework)

There is no vitest/jest and no `#[test]` modules to drive TDD. Every task therefore validates against the two build gates the repo uses as its pre-commit gate:

- **Rust (desktop, default):** `cargo check --manifest-path src-tauri/Cargo.toml` → expect `Finished`.
- **Rust (server path):** `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server` → expect `Finished`. Run this for any task that touches `webserver.rs`, `state.rs`, or the ungated `prompt_assistant` module, because the headless prompt-assistant path is `#[cfg(any(feature = "desktop", feature = "server"))]`.
- **Frontend:** `npm run build` → expect output ending in `✓ built in`.

All git commands on Windows MUST be prefixed with `git -c core.hooksPath=/dev/null` (the bash pre-commit hook hangs in PowerShell). Never add `Co-Authored-By` trailers. Each task ends with a path-scoped commit so the work is bisectable.

**Branch setup before Task 1** (run once; `main` is protected so implementation lands on its own branch):

```bash
git -c core.hooksPath=/dev/null checkout main
git -c core.hooksPath=/dev/null pull origin main
git -c core.hooksPath=/dev/null checkout -b feat/prompt-assistant-1.4.19
```

---

### Task 1: A1 — installed-aware natural-language fallback model

When `prompt_assistant_model_id` is unset, the current server fallback picks the first *installed* model (DanTagGen, a tag upsampler) and emits booru-tag garbage. We need a fallback that prefers an installed natural-language model. We cannot reuse `catalog::recommend_model_id` because it ranks the whole catalog and can return a model that is not installed (which then hard-errors). Add an installed-filtered sibling method on `PromptAssistant`.

**Files:**
- Modify: `src-tauri/src/prompt_assistant/mod.rs` (add method near `installed_models`, around line 72)

- [ ] **Step 1: Add `recommend_installed_model` to `PromptAssistant`**

In `src-tauri/src/prompt_assistant/mod.rs`, immediately after the existing `installed_models` method (the one ending around line 72), add:

```rust
    /// Pick the best *installed* model to use when no `prompt_assistant_model_id`
    /// is configured. Prefers a natural-language model that fits VRAM; otherwise any
    /// installed non-tag-upsampler; otherwise any installed model. Returns `None`
    /// only when nothing is installed.
    pub fn recommend_installed_model(&self, total_vram_mb: u64, system_ram_mb: u64) -> Option<String> {
        let available = if total_vram_mb >= 2000 {
            total_vram_mb
        } else {
            (system_ram_mb as f64 * 0.6) as u64
        };

        let installed: Vec<_> = catalog::catalog()
            .into_iter()
            .filter(|e| self.is_model_installed(&e.id))
            .collect();

        // 1. Best natural-language model that fits available memory.
        {
            let nl_pick = installed
                .iter()
                .filter(|e| e.purpose == "natural_language")
                .filter(|e| catalog::best_variant_for(e, available).is_some())
                .max_by_key(|e| {
                    catalog::best_variant_for(e, available)
                        .map(|v| v.vram_mb)
                        .unwrap_or(0)
                });
            if let Some(e) = nl_pick {
                return Some(e.id.clone());
            }
        }

        // 2. Any installed model that is not a tag upsampler.
        if let Some(e) = installed.iter().find(|e| e.purpose != "tag_upsampler") {
            return Some(e.id.clone());
        }

        // 3. Last resort: anything installed.
        installed.into_iter().next().map(|e| e.id)
    }
```

> Note: the `nl_pick` borrow is scoped inside a block so it drops before the step-3 `installed.into_iter()`. `best_variant_for(e, available)` takes `&LlmCatalogEntry`; `e` here is `&&LlmCatalogEntry` from `.iter()`, which deref-coerces at the call site exactly as the existing `recommend_model_id` does.

- [ ] **Step 2: Verify it compiles (desktop)**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished` with no errors.

- [ ] **Step 3: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/prompt_assistant/mod.rs
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): installed-aware NL fallback model"
```

---

### Task 2: A2 — pin llama-server to the largest-VRAM GPU and gate offload on its free VRAM

The offload gate currently reads the global *max* free VRAM across all GPUs (`detect_free_vram_mb`) and never tells llama-server which device to use. On the hosted box CUDA's default `FASTEST_FIRST` ordering lands llama-server on the near-full 4070 and the load aborts (surfaces as a 524). Fix: pick the largest-*total*-VRAM GPU, pass it to llama-server via `CUDA_DEVICE_ORDER=PCI_BUS_ID` + `CUDA_VISIBLE_DEVICES=<idx>`, and gate `n_gpu_layers` on that same GPU's free VRAM.

**Files:**
- Modify: `src-tauri/src/comfyui/gpu_manager.rs` (add two helpers after `detect_free_vram_mb`, ~line 559)
- Modify: `src-tauri/src/prompt_assistant/server.rs` (`ensure_running` signature + env block, ~line 204-246)
- Modify: `src-tauri/src/prompt_assistant/mod.rs` (`ensure_running` offload gate, ~line 162-185)

- [ ] **Step 1: Add VRAM-per-GPU helpers to `gpu_manager.rs`**

In `src-tauri/src/comfyui/gpu_manager.rs`, immediately after the existing `detect_free_vram_mb` function (around line 559), add:

```rust
/// Query each GPU's `(index, total_mb, free_mb)` via nvidia-smi.
pub fn detect_gpu_vram() -> Vec<(u32, u64, u64)> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(',').map(|s| s.trim());
            let idx = parts.next()?.parse::<u32>().ok()?;
            let total = parts.next()?.parse::<u64>().ok()?;
            let free = parts.next()?.parse::<u64>().ok()?;
            Some((idx, total, free))
        })
        .collect()
}

/// The GPU with the most *total* VRAM, as `(index, total_mb, free_mb)`.
/// Used to pin the prompt-assistant llama-server to the largest device so it
/// does not land on a near-full smaller GPU under CUDA's default ordering.
pub fn max_total_vram_gpu() -> Option<(u32, u64, u64)> {
    detect_gpu_vram()
        .into_iter()
        .max_by_key(|(_, total, _)| *total)
}
```

- [ ] **Step 2: Add a `pin_gpu_index` parameter to `LlamaServer::ensure_running`**

In `src-tauri/src/prompt_assistant/server.rs`, change the `ensure_running` signature (around line 204) to add a trailing parameter:

```rust
    pub async fn ensure_running(
        &self,
        client: &reqwest::Client,
        model_path: &Path,
        model_id: &str,
        n_gpu_layers: i32,
        pin_gpu_index: Option<u32>,
    ) -> Result<u16, AppError> {
```

Then, immediately after the line `let mut cmd = Command::new(self.server_path());` (around line 228) and **before** the first `cmd.arg("-m")...` call, insert:

```rust
        // Pin llama-server to a specific GPU. CUDA's default ordering is
        // FASTEST_FIRST, not PCI order, so we force PCI ordering and expose only
        // the chosen device. Without this, the server can land on a near-full GPU
        // and abort the model load.
        if let Some(idx) = pin_gpu_index {
            cmd.env("CUDA_DEVICE_ORDER", "PCI_BUS_ID");
            cmd.env("CUDA_VISIBLE_DEVICES", idx.to_string());
        }
```

- [ ] **Step 3: Gate offload on the pinned GPU and pass it through (mod.rs)**

In `src-tauri/src/prompt_assistant/mod.rs`, inside `ensure_running`, replace the existing free-VRAM detection block (the `let free_vram_mb = spawn_blocking(detect_free_vram_mb)...` lines, ~162-166) with:

```rust
        // Pin to the largest-VRAM GPU and gate offload on *that* GPU's free VRAM,
        // not the global max, so the layer count matches where the model loads.
        let pin = spawn_blocking(crate::comfyui::gpu_manager::max_total_vram_gpu)
            .await
            .ok()
            .flatten();
        let pin_gpu_index = pin.map(|(idx, _, _)| idx);
        let free_vram_mb = match pin {
            Some((_, _, free)) => Some(free),
            None => spawn_blocking(crate::comfyui::gpu_manager::detect_free_vram_mb)
                .await
                .ok()
                .flatten(),
        };
```

> Keep the existing `n_gpu_layers` match block that follows unchanged.

Then update the call to `self.server.ensure_running(...)` (around line 182) to pass the new argument last:

```rust
        self.server
            .ensure_running(client, &model_path, model_id, n_gpu_layers, pin_gpu_index)
            .await
```

> If `mod.rs` does not already import `spawn_blocking`, the existing code in this method already uses it (it was used for the old `detect_free_vram_mb` call), so the import is present. The fully-qualified `crate::comfyui::gpu_manager::...` paths avoid needing a new `use`.

- [ ] **Step 4: Verify it compiles (desktop)**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished` with no errors.

- [ ] **Step 5: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/comfyui/gpu_manager.rs src-tauri/src/prompt_assistant/server.rs src-tauri/src/prompt_assistant/mod.rs
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): pin llama-server to largest GPU + per-GPU offload gate"
```

---

### Task 3: B1 — unify the desktop and server prompt-assistant code paths

Today the Tauri command (`commands/prompt_assistant.rs::run_generation`) and the webserver headless path (`webserver.rs::run_prompt_assistant_headless`) are two divergent copies: the server copy has no `reconcile_enhance`, hardcodes `192` max tokens, ignores `opts`, and uses the wrong fallback. Extract one shared async core into a new **ungated** module file so both desktop and server builds compile it, and reduce both callers to thin wrappers. `commands/*` is `#[cfg(feature = "desktop")]`, so the shared core must NOT live there — it goes in `prompt_assistant/run.rs`.

**Files:**
- Create: `src-tauri/src/prompt_assistant/run.rs`
- Modify: `src-tauri/src/prompt_assistant/mod.rs` (add `pub mod run;`, ~line 4)
- Modify: `src-tauri/src/commands/prompt_assistant.rs` (rewrite wrappers, remove `run_generation`, move `PromptAssistantOpts`)
- Modify: `src-tauri/src/webserver.rs` (headless wrapper ~4400-4467 + dispatch arm ~4172-4187)

- [ ] **Step 1: Create the shared core `prompt_assistant/run.rs`**

Create `src-tauri/src/prompt_assistant/run.rs` with the full file:

```rust
use std::sync::Arc;

use serde::Deserialize;

use crate::error::AppError;
use crate::prompt_assistant::grounding::{self, GenMode};
use crate::prompt_assistant::hardware;
use crate::state::AppState;

/// Per-request options forwarded from the UI for Enhance/Compose.
#[derive(Debug, Default, Deserialize)]
pub struct PromptAssistantOpts {
    pub length: Option<String>,
    #[serde(default)]
    pub include_artists: bool,
}

/// Shared Enhance/Compose core used by both the Tauri command wrappers and the
/// webserver headless dispatcher.
///
/// `stage` reports lifecycle stages (`"loading_model"`, `"generating"`) and
/// `progress` reports model-download progress (`label`, `downloaded`, `total`,
/// `done`). Desktop passes closures that emit Tauri events; the server passes
/// no-op closures.
pub async fn run_prompt_assistant(
    state: &Arc<AppState>,
    input: &str,
    family: &str,
    mode: GenMode,
    opts: &PromptAssistantOpts,
    stage: &(dyn Fn(&str) + Sync),
    progress: &(dyn Fn(&str, u64, u64, bool) + Sync),
) -> Result<String, AppError> {
    let cfg = state.config.read().await;
    let configured_model_id = cfg.prompt_assistant_model_id.clone();
    let idle_timeout_secs = cfg.prompt_assistant_idle_timeout_secs;
    drop(cfg);

    let hw = tokio::task::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| AppError::LlmError(format!("hardware detection failed: {e}")))?;

    let model_id = match configured_model_id {
        Some(id) => id,
        None => state
            .prompt_assistant
            .recommend_installed_model(hw.total_vram_mb, hw.system_ram_mb)
            .ok_or_else(|| AppError::LlmError("prompt_assistant.no_model".into()))?,
    };

    stage("loading_model");
    state
        .prompt_assistant
        .ensure_running(
            &state.http_client,
            &model_id,
            hw.total_vram_mb,
            idle_timeout_secs,
            progress,
        )
        .await?;

    stage("generating");

    let purpose = grounding::model_purpose(&model_id);
    let tag_only = purpose == "tag_upsampler";
    let candidates = grounding::candidate_tags(input, family, mode, opts.include_artists);

    let system = grounding::system_prompt(tag_only, mode, &candidates);
    let max_tokens = match opts.length.as_deref() {
        Some("short") => 96,
        Some("detailed") => 384,
        _ => 192,
    };

    let raw = state
        .prompt_assistant
        .chat(&state.http_client, &system, input, max_tokens)
        .await?;

    let repaired = grounding::repair(&raw, tag_only);
    let result = match mode {
        GenMode::Enhance => grounding::reconcile_enhance(input, &repaired),
        GenMode::Compose => repaired,
    };

    Ok(result)
}
```

> The exact helper names (`grounding::model_purpose`, `grounding::candidate_tags`, `PromptAssistant::chat`, `grounding::repair`, `grounding::reconcile_enhance`, `cfg.prompt_assistant_idle_timeout_secs`, `hw.total_vram_mb`, `hw.system_ram_mb`) are the same identifiers the current `commands/prompt_assistant.rs::run_generation` uses — this step is a faithful move, not a redesign. If any identifier differs in the live `run_generation`, copy the live identifier; do not invent a new one.

- [ ] **Step 2: Register the module**

In `src-tauri/src/prompt_assistant/mod.rs`, add to the module declarations at the top (alongside `pub mod catalog; pub mod grounding; pub mod hardware; pub mod server;`):

```rust
pub mod run;
```

- [ ] **Step 3: Verify the shared core compiles in isolation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished`. (Errors here mean an identifier in Step 1 does not match the live API — fix by matching the live `run_generation` identifiers before continuing.)

- [ ] **Step 4: Rewrite the Tauri wrappers in `commands/prompt_assistant.rs`**

In `src-tauri/src/commands/prompt_assistant.rs`:

1. Remove the local `PromptAssistantOpts` struct (lines ~18-24) and import the moved type instead. At the top of the file, add:

```rust
use crate::prompt_assistant::run::{run_prompt_assistant, PromptAssistantOpts};
```

2. Delete the entire `run_generation` function (~lines 102-178).

3. Add a `run_with_emit` helper that builds the Tauri-event closures and delegates to the shared core. Place it where `run_generation` was:

```rust
async fn run_with_emit(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
    input: &str,
    family: &str,
    mode: GenMode,
    opts: &PromptAssistantOpts,
) -> Result<String, AppError> {
    let stage_app = app.clone();
    let stage = move |s: &str| {
        let _ = stage_app.emit("llm:stage", s.to_string());
    };

    let progress_app = app.clone();
    let progress = move |label: &str, downloaded: u64, total: u64, done: bool| {
        let _ = progress_app.emit(
            "llm:download_progress",
            DownloadProgress {
                label: label.to_string(),
                downloaded,
                total,
                done,
            },
        );
    };

    run_prompt_assistant(
        state.inner(),
        input,
        family,
        mode,
        opts,
        &stage,
        &progress,
    )
    .await
}
```

4. Rewrite the two `#[tauri::command]` wrappers (`enhance_prompt`, `compose_prompt`) to call `run_with_emit`:

```rust
#[tauri::command]
pub async fn enhance_prompt(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    input: String,
    family: String,
    opts: Option<PromptAssistantOpts>,
) -> Result<String, AppError> {
    let opts = opts.unwrap_or_default();
    run_with_emit(&app, &state, &input, &family, GenMode::Enhance, &opts).await
}

#[tauri::command]
pub async fn compose_prompt(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    input: String,
    family: String,
    opts: Option<PromptAssistantOpts>,
) -> Result<String, AppError> {
    let opts = opts.unwrap_or_default();
    run_with_emit(&app, &state, &input, &family, GenMode::Compose, &opts).await
}
```

> Keep the existing `DownloadProgress` struct (~lines 26-32) in this file — `run_with_emit` constructs it. Keep the existing `use` for `GenMode`, `AppHandle`, `Emitter`/`emit`, `State`, `Arc<AppState>`, `AppError`. Remove any now-unused imports the compiler flags.

- [ ] **Step 5: Reduce the webserver headless path to a thin wrapper**

In `src-tauri/src/webserver.rs`, replace the body of `run_prompt_assistant_headless` (~lines 4400-4467) so it takes `opts` and delegates to the shared core with no-op closures:

```rust
async fn run_prompt_assistant_headless(
    state: &Arc<AppState>,
    input: &str,
    family: &str,
    mode: crate::prompt_assistant::grounding::GenMode,
    opts: &crate::prompt_assistant::run::PromptAssistantOpts,
) -> Result<String, String> {
    let stage = |_s: &str| {};
    let progress = |_label: &str, _downloaded: u64, _total: u64, _done: bool| {};

    crate::prompt_assistant::run::run_prompt_assistant(
        state, input, family, mode, opts, &stage, &progress,
    )
    .await
    .map_err(|e| e.to_string())
}
```

> This deletes the old body, which included the wrong `installed_models().next()` fallback AND the `state.free_comfyui_vram_for_llm().await` call. The free-VRAM call is intentionally dropped here (Task 4 deletes the method); the shared core does not free ComfyUI VRAM.

- [ ] **Step 6: Parse `opts` in the dispatch arm and pass it**

In `src-tauri/src/webserver.rs`, in the `"enhance_prompt" | "compose_prompt"` dispatch arm (~lines 4172-4187), add `opts` parsing before the call and pass it:

```rust
            let opts: crate::prompt_assistant::run::PromptAssistantOpts =
                serde_json::from_value(args.get("opts").cloned().unwrap_or(serde_json::Value::Null))
                    .unwrap_or_default();
            let result = run_prompt_assistant_headless(&state, &input, &family, mode, &opts).await?;
```

> Keep the existing `input` / `family` / `mode` parsing in that arm unchanged; only add the `opts` line and thread `&opts` into the call. `args` is the dispatch arm's JSON args value already in scope.

- [ ] **Step 7: Verify both builds compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished`.

Run: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server`
Expected: `Finished`. (This is the build that exercises the headless wrapper and the dispatch arm.)

- [ ] **Step 8: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/prompt_assistant/run.rs src-tauri/src/prompt_assistant/mod.rs src-tauri/src/commands/prompt_assistant.rs src-tauri/src/webserver.rs
git -c core.hooksPath=/dev/null commit -m "refactor(prompt-assistant): unify desktop and server code paths"
```

---

### Task 4: A3 — delete the now-unused `free_comfyui_vram_for_llm`

After Task 3, the only caller of `state.free_comfyui_vram_for_llm()` (the headless path) is gone. The shared core never frees ComfyUI VRAM before loading the LLM (the offload gate + GPU pin from Task 2 make that unnecessary). Remove the dead method.

**Files:**
- Modify: `src-tauri/src/state.rs` (delete method ~lines 585-663)

- [ ] **Step 1: Confirm there are no remaining callers**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server`
Expected: `Finished`, but with a dead-code or unused warning is acceptable. Then grep:

Search `free_comfyui_vram_for_llm` across `src-tauri/src`. Expected: only the definition in `state.rs` remains (the webserver caller was removed in Task 3). If any caller remains, stop and route it through the shared core instead.

- [ ] **Step 2: Delete the method**

In `src-tauri/src/state.rs`, delete the entire `free_comfyui_vram_for_llm` method including its doc comment and `#[cfg(any(feature = "desktop", feature = "server"))]` attribute (the block beginning with the doc comment ~line 585, through the method's closing `}` ~line 663). The next method (`dispatch_webhook_event`, ~line 665) and the `free_llm_vram_for_generation` method (~line 578) both remain.

- [ ] **Step 3: Verify both builds compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished`.

Run: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server`
Expected: `Finished` with no unused-method warning for `free_comfyui_vram_for_llm`.

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/state.rs
git -c core.hooksPath=/dev/null commit -m "refactor(prompt-assistant): remove unused free_comfyui_vram_for_llm"
```

---

### Task 5: B2 — length-shaped compose system prompt

The compose length selector currently only changes `max_tokens`; the natural-language system prompt is identical for short/medium/detailed, so output length barely varies. Introduce a `PromptLength` enum and vary the natural-language Compose instruction (and bump the token caps) by length. Enhance and tag-only paths are unchanged.

**Files:**
- Modify: `src-tauri/src/prompt_assistant/grounding.rs` (add enum, change `system_prompt` signature + NL Compose body, ~155-218)
- Modify: `src-tauri/src/prompt_assistant/run.rs` (compute length, pass it, bump caps)

- [ ] **Step 1: Add the `PromptLength` enum to `grounding.rs`**

In `src-tauri/src/prompt_assistant/grounding.rs`, near the `GenMode` enum (~line 220), add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptLength {
    Short,
    Medium,
    Detailed,
}

impl PromptLength {
    pub fn from_opt(s: Option<&str>) -> Self {
        match s {
            Some("short") => PromptLength::Short,
            Some("detailed") => PromptLength::Detailed,
            _ => PromptLength::Medium,
        }
    }
}
```

- [ ] **Step 2: Thread `length` through `system_prompt` and shape the NL Compose body**

In `src-tauri/src/prompt_assistant/grounding.rs`, change the `system_prompt` signature (~line 155) to accept length:

```rust
pub fn system_prompt(
    tag_only: bool,
    mode: GenMode,
    length: PromptLength,
    candidates: &[String],
) -> String {
```

In the natural-language Compose branch (the branch that currently builds the terse `body = "Write a prompt from the user's description."` ~line 203 and the trailing tag/sentence instruction ~lines 209-210), replace the fixed `body` and trailing instruction with length-varied strings. Use exactly:

```rust
        let body = match length {
            PromptLength::Short => {
                "Write a short, focused prompt from the user's description. Keep it concise."
            }
            PromptLength::Medium => {
                "Write a prompt from the user's description with a balanced level of detail."
            }
            PromptLength::Detailed => {
                "Write a richly detailed prompt from the user's description, covering subject, setting, lighting, and mood."
            }
        };
        let sentence = match length {
            PromptLength::Short => {
                "then finish with one short natural-language sentence describing the scene."
            }
            PromptLength::Medium => {
                "then finish with one detailed, grammatically complete natural-language sentence describing the scene."
            }
            PromptLength::Detailed => {
                "then finish with two or three detailed, grammatically complete natural-language sentences describing the scene, its setting, and its mood."
            }
        };
```

Then build the trailing format instruction so it reads (keeping everything on one output line):

```rust
        format!(
            "{body} Put all the tags first as a single comma-separated section, {sentence} Keep everything on one line.{candidate_block}"
        )
```

> `candidate_block` is whatever the existing code already appends for `candidates` (e.g. a "You may use these tags: ..." suffix). Reuse the existing candidate-formatting variable verbatim; do not change how candidates are rendered. The Enhance branch and the `tag_only` branch remain exactly as they are — they ignore `length`.

- [ ] **Step 3: Update the single caller in `run.rs` and bump the token caps**

In `src-tauri/src/prompt_assistant/run.rs`, replace the `system` + `max_tokens` block from Task 3 Step 1 with length-driven versions:

```rust
    let length = grounding::PromptLength::from_opt(opts.length.as_deref());
    let system = grounding::system_prompt(tag_only, mode, length, &candidates);
    let max_tokens = match length {
        grounding::PromptLength::Short => 128,
        grounding::PromptLength::Medium => 256,
        grounding::PromptLength::Detailed => 512,
    };
```

> This supersedes the `opts.length.as_deref()` `match` that returned `96/384/192` in Task 3 — there is now exactly one length computation, feeding both the prompt and the cap.

- [ ] **Step 4: Verify both builds compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished`.

Run: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server`
Expected: `Finished`.

- [ ] **Step 5: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/prompt_assistant/grounding.rs src-tauri/src/prompt_assistant/run.rs
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): length-shaped compose system prompt"
```

---

### Task 6: C1 — plumb `preferred_gpu_index` through params (backend field + frontend store)

Add the transient per-generation field end-to-end so a chosen GPU index can reach the backend. No behavior change yet (the field is parsed but unused until Task 7).

**Files:**
- Modify: `src-tauri/src/comfyui/types.rs` (`GenerationParams`, before close ~line 249)
- Modify: `src/lib/types/index.ts` (`GenerationParams`, after line 145)
- Modify: `src/lib/stores/generation.svelte.ts` (state field after ~line 278; `toParams` before ~line 1784)

- [ ] **Step 1: Add the Rust field**

In `src-tauri/src/comfyui/types.rs`, inside `GenerationParams`, immediately before the struct's closing `}` (~line 249, after the `style_transfer_blocks` field), add:

```rust
    #[serde(default)]
    pub preferred_gpu_index: Option<u32>,
```

- [ ] **Step 2: Add the TypeScript field**

In `src/lib/types/index.ts`, inside the `GenerationParams` interface, immediately after the `style_transfer_blocks?: string;` line (~line 145), add:

```typescript
  preferred_gpu_index?: number | null;
```

- [ ] **Step 3: Add the store state field**

In `src/lib/stores/generation.svelte.ts`, immediately after `batchSize = $state(1);` (~line 278), add:

```typescript
  preferredGpuIndex = $state<number | null>(null);
```

> This is a transient selection (resets to `null` on reload). Do NOT add it to `saveSettings`/`loadSettings` — it is intentionally non-persisted.

- [ ] **Step 4: Map it in `toParams`**

In `src/lib/stores/generation.svelte.ts`, inside `toParams`, immediately after the `style_transfer_blocks: this.styleTransferBlocks,` line (~line 1783) and before the closing `};` (~line 1784), add:

```typescript
      preferred_gpu_index: this.preferredGpuIndex ?? null,
```

- [ ] **Step 5: Verify both builds compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished`.

Run: `npm run build`
Expected: output ends with `✓ built in`.

- [ ] **Step 6: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/comfyui/types.rs src/lib/types/index.ts src/lib/stores/generation.svelte.ts
git -c core.hooksPath=/dev/null commit -m "feat(generation): plumb preferred_gpu_index through params"
```

---

### Task 7: C2 + C3 — honor the pinned GPU in the worker pool + enforce the role gate

This is one atomic task: adding a parameter to `submit_prompt` breaks all five call sites and the `HeldPrompt` struct until they are all updated, so they change together. The pin logic routes a generation to the requested worker (reserving it if idle, otherwise queuing on its server queue). The role gate ensures only moderators/admins can pin; regular/anonymous server users always get `None`. Desktop always honors the field (single trusted user).

**Files:**
- Modify: `src-tauri/src/comfyui/gpu_manager.rs` (`submit_prompt` signature + pin block, ~223-266)
- Modify: `src-tauri/src/state.rs` (`HeldPrompt` struct, ~28-42)
- Modify: `src-tauri/src/commands/workflow.rs` (2 call sites, ~75 and ~123)
- Modify: `src-tauri/src/webserver.rs` (drain reactor ~727; generate arm role gate + submit + HeldPrompt ~2596-2662; preprocessor preview ~2750)

- [ ] **Step 1: Add the `preferred_gpu_index` parameter + pin block to `submit_prompt`**

In `src-tauri/src/comfyui/gpu_manager.rs`, change the `submit_prompt` signature (~line 223) to add a trailing parameter:

```rust
    pub async fn submit_prompt(
        &self,
        workflow: serde_json::Value,
        client_id: &str,
        timeout: Duration,
        preferred_gpu_index: Option<u32>,
    ) -> Result<(u32, PromptResponse), AppError> {
```

Then, at the very start of the method body (before `let deadline = ...`), insert the pin block:

```rust
        // Honor an explicit GPU pin: send the job to that worker, reserving it if
        // idle, otherwise queuing on its server-side queue. Falls through to the
        // normal scheduler only if no worker has the requested index.
        if let Some(idx) = preferred_gpu_index {
            if let Some(worker) = self.workers.iter().find(|w| w.gpu_index == idx) {
                if worker.try_reserve() {
                    return self.do_submit(worker, workflow, client_id).await;
                }
                return self
                    .do_submit_to_server_queue(worker, workflow, client_id)
                    .await;
            }
        }
```

> `self.workers.iter().find(...)` yields `Option<&Arc<GpuWorker>>`; `do_submit` and `do_submit_to_server_queue` both take `&Arc<GpuWorker>`, so `worker` passes directly. The existing 3-tier scheduling logic below the inserted block is unchanged.

- [ ] **Step 2: Add the field to `HeldPrompt`**

In `src-tauri/src/state.rs`, inside the `HeldPrompt` struct (~lines 28-42), add a field (place it after `placeholder_id`):

```rust
    pub preferred_gpu_index: Option<u32>,
```

- [ ] **Step 3: Update the two desktop call sites in `workflow.rs`**

In `src-tauri/src/commands/workflow.rs`:

The generate submit (~line 73-76) gains the params field (desktop is a single trusted user, so it always honors the pin):

```rust
    let (gpu_index, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, timeout, params.preferred_gpu_index)
        .await?;
```

The preprocessor preview submit (~line 121-124) passes `None` (previews are not user-pinnable):

```rust
    let (_gpu_index, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, timeout, None)
        .await?;
```

> Match the existing binding patterns in `workflow.rs` (the variable names `gpu_index`/`_gpu_index` and whether the tuple is destructured) — only the trailing argument changes. If the live code binds the tuple differently, keep its binding and just append the `params.preferred_gpu_index` / `None` argument.

- [ ] **Step 4: Update the drain reactor in `webserver.rs`**

In `src-tauri/src/webserver.rs`, the drain reactor submit (~line 723-727) forwards the held value:

```rust
        let submit = drain_state
            .gpu_manager
            .submit_prompt(hp.workflow, &drain_state.client_id, timeout, hp.preferred_gpu_index)
            .await;
```

> Keep the existing surrounding binding/handling; only `hp.workflow` was there before and now `hp.preferred_gpu_index` is appended. `hp` is the `HeldPrompt` being drained.

- [ ] **Step 5: Compute the role-gated pin in the generate arm**

In `src-tauri/src/webserver.rs`, in the generate dispatch arm, immediately after `GenerationParams` is parsed (~line 2543-2545) and **before** `tokio::spawn` (~line 2596), add:

```rust
    // Only moderators and admins may pin a generation to a specific GPU.
    // UserRole has no PartialOrd, so match the elevated roles explicitly.
    let preferred_gpu_index = if matches!(caller_role, UserRole::Admin | UserRole::Moderator) {
        params.preferred_gpu_index
    } else {
        None
    };
```

> `caller_role: UserRole` is the `dispatch_command` parameter already in scope. `preferred_gpu_index` is `Option<u32>` (Copy), so it is captured by the `move` closure without cloning.

- [ ] **Step 6: Use the gated pin at the direct submit and on `HeldPrompt`**

Still in the generate arm of `src-tauri/src/webserver.rs`:

The direct submit (~line 2660-2662) passes the gated value:

```rust
            let result = bg_state
                .gpu_manager
                .submit_prompt(workflow, &bg_state.client_id, timeout, preferred_gpu_index)
                .await;
```

The `HeldPrompt` construction (~line 2607-2613) sets the new field:

```rust
            let held = HeldPrompt {
                workflow,
                username: user.clone(),
                placeholder_id,
                preferred_gpu_index,
                submitted,
                result,
            };
```

> Match the live field names/order of the existing `HeldPrompt` literal; only add `preferred_gpu_index`. If `workflow` is moved into both the submit and the struct in different branches, keep the existing branch structure — `preferred_gpu_index` is `Copy` so it can be used in multiple branches.

- [ ] **Step 7: Update the preprocessor preview submit in `webserver.rs`**

In `src-tauri/src/webserver.rs`, the preprocessor preview submit (~line 2750) passes `None`:

```rust
    let (_gpu_index, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, timeout, None)
        .await?;
```

> Keep the live binding pattern; only append the `None` argument.

- [ ] **Step 8: Verify both builds compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished`. (A failure naming a `submit_prompt` arity mismatch means a call site was missed — there are exactly five: `workflow.rs` x2, `webserver.rs` drain/generate/preprocessor.)

Run: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server`
Expected: `Finished`.

- [ ] **Step 9: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/comfyui/gpu_manager.rs src-tauri/src/state.rs src-tauri/src/commands/workflow.rs src-tauri/src/webserver.rs
git -c core.hooksPath=/dev/null commit -m "feat(generation): honor preferred GPU with mod/admin role gate"
```

---

### Task 8: C4 — role-gated GPU picker UI + i18n

Surface the picker. A tiny session store mirrors the user's role (stores must not import each other, so App.svelte syncs it). The `<GpuPicker>` reads the role + `gpu_workers` config, binds `generation.preferredGpuIndex`, and only renders for admins/moderators with more than one enabled worker. On desktop the role is always `"admin"`, so the same gate shows it there too.

**Files:**
- Create: `src/lib/stores/session.svelte.ts`
- Create: `src/lib/components/generation/GpuPicker.svelte`
- Modify: `src/App.svelte` (import + `$effect` sync, near ~517-579)
- Modify: `src/lib/components/generation/SamplerSettings.svelte` (import after ~line 8; render before ~line 171)
- Modify: `src/lib/locales/en.ts` + all 10 other locale files (3 keys each)

- [ ] **Step 1: Create the session store**

Create `src/lib/stores/session.svelte.ts`:

```typescript
type UserRole = "admin" | "moderator" | "user" | "anonymous";

class SessionStore {
  role = $state<UserRole>("admin");
}

export const session = new SessionStore();
```

- [ ] **Step 2: Sync role from `App.svelte`**

In `src/App.svelte`, add the import alongside the other store imports at the top of the `<script>` block:

```typescript
import { session } from "./lib/stores/session.svelte.js";
```

Then, after the `userRole` state declaration and its sibling auth-state declarations (~after line 528), add a top-level effect that mirrors the role into the session store:

```typescript
  $effect(() => {
    session.role = userRole;
  });
```

> `userRole` is already assigned `"admin"` on desktop (~line 532) and `data.role ?? ...` in server mode (~lines 541/579); the effect re-runs whenever it changes. The session store exists solely so leaf components can read the role without prop threading.

- [ ] **Step 3: Create the `GpuPicker` component**

Create `src/lib/components/generation/GpuPicker.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { generation } from "../../stores/generation.svelte.js";
  import { session } from "../../stores/session.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { getConfig } from "../../utils/api.js";
  import InfoTip from "../ui/InfoTip.svelte";

  type GpuWorker = {
    gpu_index: number;
    enabled: boolean;
    label: string | null;
  };

  let workers = $state<GpuWorker[]>([]);

  const canPin = $derived(session.role === "admin" || session.role === "moderator");

  onMount(async () => {
    try {
      const cfg = await getConfig();
      workers = (cfg.gpu_workers ?? []).filter((w) => w.enabled);
    } catch {
      workers = [];
    }
  });
</script>

{#if canPin && workers.length > 1}
  <div>
    <label class="mb-1 flex items-center gap-1 text-xs text-neutral-400" for="gpu-picker">
      {locale.t("generation.gpu_picker.label")}
      <InfoTip text={locale.t("generation.gpu_picker.tip")} />
    </label>
    <select
      id="gpu-picker"
      class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-200"
      value={generation.preferredGpuIndex ?? ""}
      onchange={(e) => {
        const v = (e.currentTarget as HTMLSelectElement).value;
        generation.preferredGpuIndex = v === "" ? null : Number(v);
      }}
    >
      <option value="">{locale.t("generation.gpu_picker.auto")}</option>
      {#each workers as w (w.gpu_index)}
        <option value={w.gpu_index}>{w.label ?? `GPU ${w.gpu_index}`}</option>
      {/each}
    </select>
  </div>
{/if}
```

> Tailwind only, no `<style>`; `onchange` not `on:change`. `InfoTip` is the same component `SamplerSettings.svelte` already imports — confirm its relative path matches the one used there (`../ui/InfoTip.svelte`) and adjust if `SamplerSettings` imports it from a different location. `getConfig` is exported from `src/lib/utils/api.ts` (~line 684) and returns `AppConfig` whose `gpu_workers` array carries `gpu_index`/`enabled`/`label`.

- [ ] **Step 4: Render the picker in `SamplerSettings.svelte`**

In `src/lib/components/generation/SamplerSettings.svelte`, add the import immediately after the existing import block (~after line 8):

```typescript
  import GpuPicker from "./GpuPicker.svelte";
```

Then, immediately before the `<!-- Sampler + Scheduler -->` comment (~line 171), add:

```svelte
  <GpuPicker />

```

- [ ] **Step 5: Add the three i18n keys to `en.ts`**

In `src/lib/locales/en.ts`, within the `generation.*` section (e.g. right after the `generation.sampler.batch_tip` entry, ~line 565), add:

```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "Auto",
  "generation.gpu_picker.tip":
    "Pin this generation to a specific GPU. 'Auto' lets the server pick the next free worker.",
```

- [ ] **Step 6: Add the same three keys to every other locale**

Add the three keys (same flat key names, no `{placeholder}` tokens, so parity is just key presence) to each of the remaining 10 files. Place them next to the corresponding `generation.sampler.batch_tip` entry in each file. Use these translations:

`src/lib/locales/de.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "Auto",
  "generation.gpu_picker.tip":
    "Diese Generierung an eine bestimmte GPU binden. 'Auto' überlässt dem Server die Wahl des nächsten freien Workers.",
```

`src/lib/locales/es.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "Auto",
  "generation.gpu_picker.tip":
    "Fija esta generación a una GPU concreta. 'Auto' deja que el servidor elija el siguiente worker libre.",
```

`src/lib/locales/fr.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "Auto",
  "generation.gpu_picker.tip":
    "Épingle cette génération à un GPU précis. « Auto » laisse le serveur choisir le prochain worker libre.",
```

`src/lib/locales/it.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "Auto",
  "generation.gpu_picker.tip":
    "Blocca questa generazione su una GPU specifica. 'Auto' lascia che il server scelga il prossimo worker libero.",
```

`src/lib/locales/ja.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "自動",
  "generation.gpu_picker.tip":
    "この生成を特定のGPUに固定します。「自動」ではサーバーが次に空いているワーカーを選びます。",
```

`src/lib/locales/ko.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "자동",
  "generation.gpu_picker.tip":
    "이 생성을 특정 GPU에 고정합니다. '자동'은 서버가 다음 빈 워커를 선택하도록 합니다.",
```

`src/lib/locales/pt.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "Auto",
  "generation.gpu_picker.tip":
    "Fixa esta geração em uma GPU específica. 'Auto' deixa o servidor escolher o próximo worker livre.",
```

`src/lib/locales/ru.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "Авто",
  "generation.gpu_picker.tip":
    "Закрепить эту генерацию за конкретным GPU. «Авто» позволяет серверу выбрать следующий свободный воркер.",
```

`src/lib/locales/zh.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "自动",
  "generation.gpu_picker.tip":
    "将本次生成固定到指定 GPU。“自动”让服务器选择下一个空闲的工作器。",
```

`src/lib/locales/zh-tw.ts`:
```typescript
  "generation.gpu_picker.label": "GPU",
  "generation.gpu_picker.auto": "自動",
  "generation.gpu_picker.tip":
    "將本次生成固定到指定 GPU。「自動」讓伺服器選擇下一個空閒的工作器。",
```

- [ ] **Step 7: Verify the frontend builds (this is the i18n parity gate)**

Run: `npm run build`
Expected: output ends with `✓ built in`. (A missing key in any locale does NOT fail the build — it silently falls back to English — so also eyeball that all 11 files received the three keys before committing.)

- [ ] **Step 8: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/stores/session.svelte.ts src/lib/components/generation/GpuPicker.svelte src/App.svelte src/lib/components/generation/SamplerSettings.svelte src/lib/locales
git -c core.hooksPath=/dev/null commit -m "feat(generation): role-gated GPU picker UI + i18n"
```

---

### Task 9: Version bump 1.4.18 → 1.4.19 + changelog

Bump the three version files (they must match exactly) and prepend release notes. The actual PR/merge/tag/CI is done afterward via the `release` skill — this task only stages the version + changelog content.

**Files:**
- Modify: `package.json` (`"version"`)
- Modify: `src-tauri/Cargo.toml` (`version` under `[package]`, line 3)
- Modify: `src-tauri/tauri.conf.json` (`"version"`, line 4)
- Modify: `RELEASE_NOTES.md` (prepend)
- Modify: `CHANGELOG.md` (prepend, under `# Changelog`)

- [ ] **Step 1: Bump `package.json`**

In `package.json`, change `"version": "1.4.18"` to `"version": "1.4.19"`.

- [ ] **Step 2: Bump `src-tauri/Cargo.toml`**

In `src-tauri/Cargo.toml`, line 3, change `version = "1.4.18"` to `version = "1.4.19"`.

- [ ] **Step 3: Bump `src-tauri/tauri.conf.json`**

In `src-tauri/tauri.conf.json`, line 4, change `"version": "1.4.18"` to `"version": "1.4.19"`.

- [ ] **Step 4: Verify all three match**

Run: `git -c core.hooksPath=/dev/null --no-pager grep -n "1.4.19" -- package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json`
Expected: exactly one match per file.

- [ ] **Step 5: Prepend to `RELEASE_NOTES.md`**

At the very top of `RELEASE_NOTES.md`, prepend:

```markdown
## What's New in v1.4.19

### Prompt Assistant
- Enhance and Compose now pick an installed natural-language model automatically when none is configured, instead of falling back to a tag upsampler.
- The prompt-assistant model now pins to the largest-VRAM GPU and gates GPU offload on that GPU's free memory, fixing failed loads and timeouts on multi-GPU hosts.
- Desktop and server now share one Enhance/Compose code path, so server generations get the same model selection, length handling, and enhance reconciliation as desktop.
- The Compose length selector now shapes the generated prompt (short / medium / detailed), not just the token budget.

### Generation
- Moderators and admins can pin an individual generation to a specific GPU from a new GPU picker in the sampler settings.

---

```

- [ ] **Step 6: Prepend to `CHANGELOG.md`**

In `CHANGELOG.md`, directly under the `# Changelog` heading, insert the same block:

```markdown
## What's New in v1.4.19

### Prompt Assistant
- Enhance and Compose now pick an installed natural-language model automatically when none is configured, instead of falling back to a tag upsampler.
- The prompt-assistant model now pins to the largest-VRAM GPU and gates GPU offload on that GPU's free memory, fixing failed loads and timeouts on multi-GPU hosts.
- Desktop and server now share one Enhance/Compose code path, so server generations get the same model selection, length handling, and enhance reconciliation as desktop.
- The Compose length selector now shapes the generated prompt (short / medium / detailed), not just the token budget.

### Generation
- Moderators and admins can pin an individual generation to a specific GPU from a new GPU picker in the sampler settings.

---

```

- [ ] **Step 7: Final full build validation**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `Finished`.

Run: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features server`
Expected: `Finished`.

Run: `npm run build`
Expected: output ends with `✓ built in`.

- [ ] **Step 8: Commit**

```bash
git -c core.hooksPath=/dev/null add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json RELEASE_NOTES.md CHANGELOG.md
git -c core.hooksPath=/dev/null commit -m "v1.4.19: prompt assistant multi-GPU fixes + GPU picker"
```

- [ ] **Step 9: Release**

Invoke the `release` skill for version `1.4.19` (repo hygiene → pre-commit-check → build validation → release branch/PR → GlassWorm + bot triage → merge → tag → CI). Do not tag before the PR merges; tags are protected (fallback: `gh workflow run release.yml -f tag=v1.4.19`).

---

## Self-review

**1. Spec coverage:**
- A1 (installed-aware NL fallback) → Task 1. Fallback used by shared core in Task 3.
- A2 (enhance GPU pin + per-GPU offload gate) → Task 2.
- A3 (delete `free_comfyui_vram_for_llm`) → Task 4 (after Task 3 removes its sole caller).
- B1 (unify desktop/server paths) → Task 3.
- B2 (length-shaped compose) → Task 5.
- C1 (`preferred_gpu_index` plumbing) → Task 6.
- C2 (role enforcement) + C3 (server-queue on pinned worker) → Task 7.
- C4 (role-gated UI + i18n) → Task 8.
- Version bump + changelog → Task 9.
All spec sections map to a task.

**2. Placeholder scan:** No "TBD"/"add error handling"/"similar to Task N"/"write tests for the above". Every code step contains complete code. Where a step depends on a live identifier, the instruction names the exact identifier and says to match the live one rather than invent (faithful-move guidance, not a placeholder).

**3. Type consistency:**
- `recommend_installed_model(total_vram_mb, system_ram_mb) -> Option<String>` defined Task 1, called Task 3 Step 1 — names match.
- `PromptAssistantOpts` moved to `prompt_assistant/run.rs` (Task 3 Step 1); referenced as `crate::prompt_assistant::run::PromptAssistantOpts` in `webserver.rs` (Task 3 Steps 5-6) and imported in `commands/prompt_assistant.rs` (Task 3 Step 4) — consistent.
- `run_prompt_assistant(state, input, family, mode, opts, stage, progress)` signature identical across Task 3 (definition), Task 3 Steps 4-5 (callers), Task 5 Step 3 (caller body) — consistent.
- `ensure_running(..., n_gpu_layers, pin_gpu_index)` added in Task 2 Step 2, called with `pin_gpu_index` in Task 2 Step 3 — consistent.
- `submit_prompt(workflow, client_id, timeout, preferred_gpu_index)` defined Task 7 Step 1, all five callers updated Task 7 Steps 3-7 — consistent arity.
- `HeldPrompt.preferred_gpu_index: Option<u32>` added Task 7 Step 2, set Task 7 Step 6, read Task 7 Step 4 — consistent.
- Frontend `preferredGpuIndex` (camelCase store) → `preferred_gpu_index` (snake_case param/Rust field) mapping in `toParams` (Task 6) matches the Rust `GenerationParams` field and the `GpuPicker` binding (Task 8) — consistent.
- `PromptLength` enum + `from_opt` defined Task 5 Step 1, used Task 5 Steps 2-3 — consistent.
- i18n keys `generation.gpu_picker.{label,auto,tip}` defined in `en.ts` (Task 8 Step 5), mirrored in all 10 locales (Step 6), consumed in `GpuPicker.svelte` (Step 3) — consistent.

No gaps found.
