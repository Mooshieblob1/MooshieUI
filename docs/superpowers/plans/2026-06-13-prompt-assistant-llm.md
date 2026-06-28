# Prompt Assistant — Local LLM Prompt Enhance & Compose — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a curated, hardware-auto-selected local LLM that **enhances** the current positive prompt in place (✨ Enhance) or **composes** a new one from a description (✍ Compose), with output strictly adhering to the active model family's convention (danbooru tags for tag-only families; natural language + Gelbooru tags + `@artist` for Anima).

**Architecture:** A `llama-server` subprocess owned entirely by Rust (frontend never talks to it). Rust handles hardware detection (incl. Blackwell/GB10 → NVFP4), a static curated catalog with auto-fit selection, on-demand model download, grounding over a bundled danbooru corpus, and post-filter repair, exposed via 8 Tauri commands + 2 event streams. The frontend adds a decoupled store, two buttons, two modals, and a Settings section. Browser/web mode works unchanged because every command also routes through the existing axum dispatch.

**Tech Stack:** Rust (Tauri v2, tokio, reqwest, serde, thiserror), Svelte 5 runes, Tailwind, llama.cpp `llama-server` (OpenAI-compatible HTTP).

---

## Validation Model (read first)

**This repo has NO test framework** (no vitest/jest, no pre-existing `#[test]` modules). Per `CLAUDE.md`, the standing validation gates are:

- Rust: `cargo fmt --manifest-path src-tauri/Cargo.toml` → `cargo check --manifest-path src-tauri/Cargo.toml` → `cargo clippy --manifest-path src-tauri/Cargo.toml`
- Frontend: `npm run build`

We DO add focused `#[test]` modules for the **pure** Rust logic (Blackwell classification, catalog auto-fit, tag/artist repair) because `cargo test` runs them with zero new infrastructure and these functions are exactly where correctness is subtle. Run them with `cargo test --manifest-path src-tauri/Cargo.toml <name>`. We do NOT invent tests for subprocess/IPC/UI code — those are gated by `cargo check`/`npm run build` + the manual smoke checks noted per task.

**Git on Windows:** prefix every git command with `git -c core.hooksPath=/dev/null` (the bash pre-commit hook hangs in PowerShell). **Never** add `Co-Authored-By` trailers. **No em dashes** in any GitHub/external text written as the user.

**Serialization convention (verified):** Rust command structs serialize field names **as-is (snake_case)**; TS interfaces mirror snake_case (e.g. `vram_mb`, `recommended_model_id`). Do NOT add `#[serde(rename_all="camelCase")]` to response structs.

---

## File Structure

**New Rust (`src-tauri/src/`):**
- `prompt_assistant/mod.rs` — `PromptAssistant` state struct; paths; model install/delete; status; idle watchdog; re-exports.
- `prompt_assistant/hardware.rs` — `LlmHardware`/`LlmGpu`, Blackwell/NVFP4 classification, `detect_llm_hardware()`.
- `prompt_assistant/catalog.rs` — `LlmCatalogEntry`/`LlmVariant`, static `catalog()`, `recommend_model_id()` auto-fit.
- `prompt_assistant/grounding.rs` — bundled danbooru corpus (`include_str!` of `anima-tags.json`), retrieval, post-filter repair (tag-only + Anima `@artist`), system-prompt builders.
- `prompt_assistant/server.rs` — `llama-server` binary resolve/download/extract, spawn (no window), `/health` poll, `/v1/chat/completions` call, idle unload.
- `commands/prompt_assistant.rs` — the 8 `#[tauri::command]`s.

**Modified Rust:**
- `error.rs` — add `AppError::LlmError(String)`.
- `config.rs` — add `prompt_assistant_model_id`, `prompt_assistant_idle_timeout_secs`, `prompt_assistant_setup_done` (+ Default + normalize).
- `state.rs` — add `pub prompt_assistant: Arc<PromptAssistant>` (gated, like `interrogator`).
- `commands/mod.rs` — declare `pub mod prompt_assistant;` (desktop-gated).
- `lib.rs` — `pub mod prompt_assistant;` + register 8 commands in `generate_handler![]`.
- `webserver.rs` — add the 8 commands to the browser-mode dispatch match.
- `Cargo.toml` (`src-tauri/`) — add `sysinfo` (system RAM) if not present; `zip`/`tar`/`flate2` already present (used by interrogator).

**New Frontend (`src/`):**
- `lib/stores/promptAssistant.svelte.ts` — decoupled store (no other-store imports).
- `lib/components/generation/PromptAssistantSetupModal.svelte` — hardware banner, model cards, download.
- `lib/components/generation/PromptComposeModal.svelte` — describe → generate → replace/append.

**Modified Frontend:**
- `lib/types/index.ts` — `LlmGpu`, `LlmHardware`, `LlmVariant`, `LlmCatalogEntry`, `LlmStatus`, `PromptAssistantOpts`.
- `lib/utils/api.ts` — 8 typed wrappers.
- `lib/components/generation/PromptInputs.svelte` — ✨ Enhance / ✍ Compose buttons + inline enhance + undo.
- `lib/components/settings/SettingsPage.svelte` — Prompt Assistant section (after interrogator).
- `App.svelte` — launch-time `promptAssistant.init()`.
- `lib/locales/en.ts` + **every** other `lib/locales/*.ts` — new i18n keys.

---

## PHASE 1 — Backend runtime

### Task 1: Scaffolding — error variant, config fields, module wiring

**Files:**
- Modify: `src-tauri/src/error.rs`
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/lib.rs:1-19` (module decls)
- Modify: `src-tauri/src/commands/mod.rs`
- Create: `src-tauri/src/prompt_assistant/mod.rs` (stub for now)

- [ ] **Step 1: Add the error variant**

In `src-tauri/src/error.rs`, add before `Other`:

```rust
    #[error("Prompt assistant error: {0}")]
    LlmError(String),
```

- [ ] **Step 2: Add config fields**

In `src-tauri/src/config.rs`, inside `struct AppConfig` (after the interrogator threshold fields, ~line 94):

```rust
    /// Prompt assistant: selected/installed catalog model id (None = not chosen yet).
    pub prompt_assistant_model_id: Option<String>,
    /// Prompt assistant: idle seconds before the llama-server subprocess is unloaded.
    #[serde(default = "default_llm_idle_timeout")]
    pub prompt_assistant_idle_timeout_secs: u64,
    /// Prompt assistant: true once the user has completed first-run setup.
    pub prompt_assistant_setup_done: bool,
```

Add the default helper near `default_true` (~line 20):

```rust
fn default_llm_idle_timeout() -> u64 {
    120
}
```

In `impl Default for AppConfig` (after `interrogator_character_threshold: 0.85,`):

```rust
            prompt_assistant_model_id: None,
            prompt_assistant_idle_timeout_secs: 120,
            prompt_assistant_setup_done: false,
```

- [ ] **Step 3: Declare the Rust module**

In `src-tauri/src/lib.rs`, after the `notifications` module decl (keep alphabetical-ish ordering, ~line 13), add:

```rust
#[cfg(any(feature = "desktop", feature = "server"))]
pub mod prompt_assistant;
```

In `src-tauri/src/commands/mod.rs`, add:

```rust
#[cfg(feature = "desktop")]
pub mod prompt_assistant;
```

- [ ] **Step 4: Create the module stub**

Create `src-tauri/src/prompt_assistant/mod.rs`:

```rust
pub mod catalog;
pub mod grounding;
pub mod hardware;
pub mod server;
```

(The `PromptAssistant` struct is added in Task 6; this stub lets Tasks 2-5 compile independently.)

- [ ] **Step 5: Gate check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles (modules `catalog`/`grounding`/`hardware`/`server` are missing → it will fail until Task 2-5 create them). **Acceptable interim state** — proceed to Task 2. If you want a green checkpoint now, temporarily comment the four `pub mod` lines, run check, then restore. Do not commit a non-compiling tree.

---

### Task 2: Hardware detection + Blackwell/NVFP4 classification

**Files:**
- Create: `src-tauri/src/prompt_assistant/hardware.rs`
- Cargo: ensure `sysinfo` dependency exists (for system RAM).

- [ ] **Step 1: Add `sysinfo` if missing**

Check `src-tauri/Cargo.toml` for `sysinfo`. If absent, add under `[dependencies]`:

```toml
sysinfo = "0.33"
```

Run: `cargo check --manifest-path src-tauri/Cargo.toml` to fetch it.

- [ ] **Step 2: Write `hardware.rs` with the classifier + unit tests**

Create `src-tauri/src/prompt_assistant/hardware.rs`:

```rust
use serde::Serialize;

use crate::prompt_assistant::catalog;

#[derive(Debug, Clone, Serialize)]
pub struct LlmGpu {
    pub name: String,
    pub vram_mb: u64,
    pub vendor: String,
    pub nvfp4_capable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmHardware {
    pub gpus: Vec<LlmGpu>,
    pub total_vram_mb: u64,
    pub system_ram_mb: u64,
    pub nvfp4_capable: bool,
    pub recommended_model_id: String,
}

/// Substrings (lowercased) that mark an NVIDIA GPU as Blackwell-class / NVFP4-capable.
/// Name-based detection with room to grow; the compute-capability fallback in
/// `is_blackwell_name` covers SKUs whose names are not yet listed.
const BLACKWELL_MARKERS: &[&str] = &[
    "rtx 50",     // GeForce RTX 5060/5070/5080/5090 (+Ti/Super/Laptop)
    "rtx pro 6000", // RTX PRO 6000 Blackwell workstation
    "b200",       // datacenter B200
    "gb200",      // Grace-Blackwell GB200
    "gb10",       // Grace-Blackwell GB10
    "dgx spark",  // DGX Spark (GB10)
    "blackwell",  // explicit branding
];

/// True when a GPU name indicates a Blackwell-class NVIDIA part.
pub fn is_blackwell_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    BLACKWELL_MARKERS.iter().any(|m| n.contains(m))
}

/// Coarse vendor classification from the GPU name.
pub fn vendor_of(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.contains("nvidia")
        || n.contains("geforce")
        || n.contains("rtx")
        || n.contains("gtx")
        || n.contains("quadro")
        || n.contains("tesla")
    {
        "nvidia"
    } else if n.contains("radeon") || n.contains("amd") || n.contains("instinct") {
        "amd"
    } else if n.contains("intel") || n.contains("arc") {
        "intel"
    } else if n.contains("apple") {
        "apple"
    } else {
        "unknown"
    }
}

/// Read total system RAM in MB.
pub fn system_ram_mb() -> u64 {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.total_memory() / 1024 / 1024 // bytes → MB
}

/// Build the full hardware report and pick a recommended catalog model.
pub fn detect() -> LlmHardware {
    let raw = crate::comfyui::gpu_manager::detect_gpus(); // Vec<(index, name, vram_mb)>
    let gpus: Vec<LlmGpu> = raw
        .into_iter()
        .map(|(_idx, name, vram_mb)| {
            let nvfp4 = is_blackwell_name(&name);
            let vendor = vendor_of(&name).to_string();
            LlmGpu {
                name,
                vram_mb,
                vendor,
                nvfp4_capable: nvfp4,
            }
        })
        .collect();

    let total_vram_mb = gpus.iter().map(|g| g.vram_mb).max().unwrap_or(0);
    let nvfp4_capable = gpus.iter().any(|g| g.nvfp4_capable);
    let system_ram_mb = system_ram_mb();

    let recommended_model_id =
        catalog::recommend_model_id(total_vram_mb, system_ram_mb, nvfp4_capable);

    LlmHardware {
        gpus,
        total_vram_mb,
        system_ram_mb,
        nvfp4_capable,
        recommended_model_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blackwell_names_classify_true() {
        for name in [
            "NVIDIA GeForce RTX 5090",
            "NVIDIA GeForce RTX 5070 Ti",
            "RTX PRO 6000 Blackwell",
            "NVIDIA GB200",
            "GB10",
            "NVIDIA DGX Spark",
        ] {
            assert!(is_blackwell_name(name), "{name} should be Blackwell");
        }
    }

    #[test]
    fn non_blackwell_names_classify_false() {
        for name in [
            "NVIDIA GeForce RTX 4090",
            "NVIDIA GeForce RTX 3060",
            "AMD Radeon RX 7900 XTX",
            "Apple M3 Max",
        ] {
            assert!(!is_blackwell_name(name), "{name} should not be Blackwell");
        }
    }

    #[test]
    fn vendor_classification() {
        assert_eq!(vendor_of("NVIDIA GeForce RTX 5090"), "nvidia");
        assert_eq!(vendor_of("AMD Radeon RX 7900"), "amd");
        assert_eq!(vendor_of("Intel Arc A770"), "intel");
    }
}
```

- [ ] **Step 3: Run the unit tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml prompt_assistant::hardware`
Expected: 3 tests PASS. (Will fail to compile until Task 3 defines `catalog::recommend_model_id`; if so, do Task 3 then return here.)

---

### Task 3: Curated catalog + auto-fit recommendation

**Files:**
- Create: `src-tauri/src/prompt_assistant/catalog.rs`

- [ ] **Step 1: Write the catalog, variant types, and auto-fit with unit tests**

Create `src-tauri/src/prompt_assistant/catalog.rs`. The catalog is the **single source of truth** for model metadata; HF repos/files are pinned here.

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LlmVariant {
    /// "gguf" or "nvfp4".
    pub format: String,
    /// Quant label for GGUF (e.g. "Q4_K_M"); None for nvfp4.
    pub quant: Option<String>,
    /// On-disk size estimate (MB).
    pub size_mb: u64,
    /// VRAM needed to run fully offloaded (MB). Used for fit/dimming.
    pub vram_mb: u64,
    /// HuggingFace repo id.
    pub repo: String,
    /// File name within the repo.
    pub file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmCatalogEntry {
    pub id: String,
    pub name: String,
    /// "tag_upsampler" or "natural_language".
    pub purpose: String,
    /// Model families this entry serves well (matches generation.modelFamily values),
    /// or ["*"] for any family.
    pub families: Vec<String>,
    pub variants: Vec<LlmVariant>,
    pub pros: String,
    pub cons: String,
    pub best_for: String,
}

fn gguf(quant: &str, size_mb: u64, vram_mb: u64, repo: &str, file: &str) -> LlmVariant {
    LlmVariant {
        format: "gguf".into(),
        quant: Some(quant.into()),
        size_mb,
        vram_mb,
        repo: repo.into(),
        file: file.into(),
    }
}

fn nvfp4(size_mb: u64, vram_mb: u64, repo: &str, file: &str) -> LlmVariant {
    LlmVariant {
        format: "nvfp4".into(),
        quant: None,
        size_mb,
        vram_mb,
        repo: repo.into(),
        file: file.into(),
    }
}

/// The curated v1 catalog. Repos/files are PINNED here — update this list to
/// change available models. Sizes/vram are conservative estimates for fit logic.
pub fn catalog() -> Vec<LlmCatalogEntry> {
    vec![
        // Tiny — purpose-built danbooru tag upsampler. CPU-friendly.
        LlmCatalogEntry {
            id: "dantaggen-l".into(),
            name: "DanTagGen-delta (Large)".into(),
            purpose: "tag_upsampler".into(),
            families: vec![
                "illustrious".into(),
                "pony".into(),
                "nanosaur".into(),
                "anima".into(),
            ],
            variants: vec![gguf(
                "Q8",
                420,
                700,
                "KBlueLeaf/DanTagGen-delta",
                "ggml-model-Q8_0.gguf",
            )],
            pros: "Tiny, fast, purpose-built for danbooru tags; runs on CPU.".into(),
            cons: "Tags only — cannot write natural-language prose.".into(),
            best_for: "Expanding a few tags into a fuller tag prompt.".into(),
        },
        // Small — general instruct, natural language + tags. Laptop default.
        LlmCatalogEntry {
            id: "qwen25-3b-instruct".into(),
            name: "Qwen2.5 3B Instruct".into(),
            purpose: "natural_language".into(),
            families: vec!["*".into()],
            variants: vec![
                gguf(
                    "Q4_K_M",
                    1900,
                    3200,
                    "Qwen/Qwen2.5-3B-Instruct-GGUF",
                    "qwen2.5-3b-instruct-q4_k_m.gguf",
                ),
                gguf(
                    "Q5_K_M",
                    2300,
                    3700,
                    "Qwen/Qwen2.5-3B-Instruct-GGUF",
                    "qwen2.5-3b-instruct-q5_k_m.gguf",
                ),
            ],
            pros: "Good quality for its size; natural language + tags; 6-8 GB VRAM friendly.".into(),
            cons: "Less nuanced than 7B+ models.".into(),
            best_for: "Laptops / 6-8 GB GPUs; Anima natural-language prompts.".into(),
        },
        // Medium — higher quality; NVFP4 variant gated to Blackwell.
        LlmCatalogEntry {
            id: "qwen25-7b-instruct".into(),
            name: "Qwen2.5 7B Instruct".into(),
            purpose: "natural_language".into(),
            families: vec!["*".into()],
            variants: vec![
                gguf(
                    "Q4_K_M",
                    4700,
                    6500,
                    "Qwen/Qwen2.5-7B-Instruct-GGUF",
                    "qwen2.5-7b-instruct-q4_k_m.gguf",
                ),
                gguf(
                    "Q5_K_M",
                    5400,
                    7300,
                    "Qwen/Qwen2.5-7B-Instruct-GGUF",
                    "qwen2.5-7b-instruct-q5_k_m.gguf",
                ),
                nvfp4(
                    4300,
                    6200,
                    "nvidia/Qwen2.5-7B-Instruct-NVFP4",
                    "model.nvfp4.gguf",
                ),
            ],
            pros: "High-quality compose/enhance; NVFP4 on Blackwell is fast and compact.".into(),
            cons: "Needs ~6-7 GB VRAM; slow on CPU.".into(),
            best_for: ">=12 GB GPUs; best compose quality.".into(),
        },
    ]
}

/// Look up a catalog entry by id.
pub fn entry(id: &str) -> Option<LlmCatalogEntry> {
    catalog().into_iter().find(|e| e.id == id)
}

/// Pick the smallest-footprint variant a host can run, preferring NVFP4 when capable.
/// Returns None if no variant fits.
pub fn best_variant_for<'a>(
    entry: &'a LlmCatalogEntry,
    available_vram_mb: u64,
    nvfp4_capable: bool,
) -> Option<&'a LlmVariant> {
    // Prefer NVFP4 when the host supports it and the variant fits.
    if nvfp4_capable {
        if let Some(v) = entry
            .variants
            .iter()
            .find(|v| v.format == "nvfp4" && v.vram_mb <= available_vram_mb)
        {
            return Some(v);
        }
    }
    // Otherwise the largest GGUF quant that still fits (best quality that fits).
    entry
        .variants
        .iter()
        .filter(|v| v.format == "gguf" && v.vram_mb <= available_vram_mb)
        .max_by_key(|v| v.vram_mb)
}

/// Recommend the best catalog model id for the detected hardware:
/// the largest natural-language model that fits comfortably, else the tiny
/// tag upsampler (always runnable on CPU/RAM).
///
/// "available" is GPU VRAM when a GPU is present, otherwise a fraction of system RAM.
pub fn recommend_model_id(total_vram_mb: u64, system_ram_mb: u64, nvfp4_capable: bool) -> String {
    // CPU path: use ~60% of system RAM as a safe working budget.
    let available = if total_vram_mb >= 2000 {
        total_vram_mb
    } else {
        (system_ram_mb as f64 * 0.6) as u64
    };

    let cat = catalog();
    // Prefer natural-language models, largest that fits.
    let nl_pick = cat
        .iter()
        .filter(|e| e.purpose == "natural_language")
        .filter(|e| best_variant_for(e, available, nvfp4_capable).is_some())
        .max_by_key(|e| {
            best_variant_for(e, available, nvfp4_capable)
                .map(|v| v.vram_mb)
                .unwrap_or(0)
        });
    if let Some(e) = nl_pick {
        return e.id.clone();
    }
    // Fallback: the tiny tag upsampler (id known-present in the catalog).
    "dantaggen-l".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_vram_blackwell_picks_7b_nvfp4() {
        let id = recommend_model_id(32000, 65536, true);
        assert_eq!(id, "qwen25-7b-instruct");
        let e = entry(&id).unwrap();
        let v = best_variant_for(&e, 32000, true).unwrap();
        assert_eq!(v.format, "nvfp4");
    }

    #[test]
    fn midrange_gpu_picks_largest_fitting_gguf() {
        // 8 GB GPU, not Blackwell → 7B Q5 needs 7.3 GB → fits; should pick 7B.
        let id = recommend_model_id(8000, 32768, false);
        assert_eq!(id, "qwen25-7b-instruct");
    }

    #[test]
    fn small_gpu_picks_3b() {
        // 4 GB GPU → 7B does not fit, 3B Q4 (3.2 GB) fits.
        let id = recommend_model_id(4000, 16384, false);
        assert_eq!(id, "qwen25-3b-instruct");
    }

    #[test]
    fn cpu_only_low_ram_falls_back_to_tiny() {
        // No GPU, 4 GB RAM → 60% = 2.4 GB, 3B Q4 needs 3.2 GB → no NL fits → tiny.
        let id = recommend_model_id(0, 4096, false);
        assert_eq!(id, "dantaggen-l");
    }
}
```

- [ ] **Step 2: Run the unit tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml prompt_assistant::catalog`
Expected: 4 tests PASS.

---

### Task 4: Grounding corpus + post-filter repair

**Files:**
- Create: `src-tauri/src/prompt_assistant/grounding.rs`

The corpus is `src/lib/assets/anima-tags.json` (6.4 MB, entries `{n, c, p, a}`; `c==0` general, `c==1` artist, `c==3` copyright, `c==4` character, `c==5` meta). We bake it in with `include_str!`, parse once into a `OnceLock`, and use it for both retrieval (seed the system prompt) and repair (validate/snap tags, force `@artist`).

- [ ] **Step 1: Write `grounding.rs` with repair + unit tests**

Create `src-tauri/src/prompt_assistant/grounding.rs`:

```rust
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

/// Raw corpus entry from anima-tags.json.
#[derive(Debug, Deserialize)]
struct RawTag {
    n: String,
    // i8, not u8: the corpus contains a few `"c": -1` (unknown) entries; u8 would
    // make serde_json fail the whole parse and yield an empty corpus via unwrap_or_default.
    c: i8,
    #[serde(default)]
    a: Vec<String>,
}

pub struct Corpus {
    /// Canonical general/character/copyright tag names (underscored form).
    pub tags: HashSet<String>,
    /// Canonical artist names (underscored form, category 1).
    pub artists: HashSet<String>,
    /// alias (underscored) → canonical name, for snapping near-misses.
    pub alias_to_canonical: HashMap<String, String>,
}

static CORPUS: OnceLock<Corpus> = OnceLock::new();

// Baked into the binary so it works identically in desktop, browser, and server modes.
const ANIMA_TAGS_JSON: &str = include_str!("../../../src/lib/assets/anima-tags.json");

pub fn corpus() -> &'static Corpus {
    CORPUS.get_or_init(|| {
        let raw: Vec<RawTag> = serde_json::from_str(ANIMA_TAGS_JSON).unwrap_or_default();
        let mut tags = HashSet::new();
        let mut artists = HashSet::new();
        let mut alias_to_canonical = HashMap::new();
        for t in raw {
            let canon = normalize(&t.n);
            match t.c {
                1 => {
                    artists.insert(canon.clone());
                }
                // general, copyright, character — all valid danbooru tags
                0 | 3 | 4 => {
                    tags.insert(canon.clone());
                }
                _ => {}
            }
            for alias in t.a {
                alias_to_canonical.insert(normalize(&alias), canon.clone());
            }
        }
        Corpus {
            tags,
            artists,
            alias_to_canonical,
        }
    })
}

/// Lowercase, trim, collapse whitespace to single underscores (danbooru canonical form).
pub fn normalize(s: &str) -> String {
    s.trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

/// Convert a canonical underscored tag to display form (spaces, escaped parens
/// left intact for prompt usage).
fn to_display(tag: &str) -> String {
    tag.replace('_', " ")
}

/// Resolve a single raw token to a canonical tag if recognized (exact or alias).
fn resolve_tag(token: &str) -> Option<String> {
    let n = normalize(token);
    let c = corpus();
    if c.tags.contains(&n) {
        Some(n)
    } else {
        c.alias_to_canonical
            .get(&n)
            .filter(|canon| c.tags.contains(*canon))
            .cloned()
    }
}

/// Resolve a token to a canonical artist if recognized.
fn resolve_artist(token: &str) -> Option<String> {
    let n = normalize(token.trim_start_matches('@'));
    let c = corpus();
    if c.artists.contains(&n) {
        Some(n)
    } else {
        c.alias_to_canonical
            .get(&n)
            .filter(|canon| c.artists.contains(*canon))
            .cloned()
    }
}

/// Retrieve up to `limit` candidate tags that share a token with the input,
/// to seed the system prompt (lexical grounding).
pub fn retrieve_candidates(input: &str, limit: usize) -> Vec<String> {
    let c = corpus();
    let input_tokens: HashSet<String> = input
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect();
    let mut out: Vec<String> = Vec::new();
    for tag in &c.tags {
        if out.len() >= limit {
            break;
        }
        if tag
            .split('_')
            .any(|part| part.len() > 2 && input_tokens.contains(part))
        {
            out.push(to_display(tag));
        }
    }
    out
}

/// Whether a family uses tag-only prompting (vs Anima natural language).
pub fn is_tag_only_family(family: &str) -> bool {
    !matches!(family, "anima")
}

/// Build the family-specific system prompt, seeded with grounding candidates.
pub fn system_prompt(family: &str, mode: GenMode, candidates: &[String]) -> String {
    let cand = if candidates.is_empty() {
        String::new()
    } else {
        format!(
            "\nRelevant known tags you may draw from: {}.",
            candidates.join(", ")
        )
    };
    if is_tag_only_family(family) {
        let verb = match mode {
            GenMode::Enhance => "Expand and enrich the user's danbooru tag list",
            GenMode::Compose => "Convert the user's description into a danbooru tag list",
        };
        format!(
            "You are a danbooru tag prompt writer for an anime image generator. \
{verb}. Output ONLY a comma-separated list of lowercase danbooru tags. \
No sentences, no explanations, no quotes, no numbering. Keep existing tags. \
Prefer concrete, well-known tags.{cand}"
        )
    } else {
        // Anima: natural language + Gelbooru tags + @artist
        let verb = match mode {
            GenMode::Enhance => "Enhance the user's prompt",
            GenMode::Compose => "Write a prompt from the user's description",
        };
        format!(
            "You are a prompt writer for the Anima anime image model. {verb}. \
Use a short natural-language description followed by relevant Gelbooru-style tags. \
Reference known artists only as @artist_name. No explanations or quotes.{cand}"
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenMode {
    Enhance,
    Compose,
}

/// Post-filter repair of raw model output. Validates/repairs against the corpus
/// and enforces family conventions. Returns a cleaned prompt string (possibly
/// empty if nothing survived — caller keeps the original prompt in that case).
pub fn repair(raw: &str, family: &str) -> String {
    if is_tag_only_family(family) {
        repair_tag_only(raw)
    } else {
        repair_anima(raw)
    }
}

/// Tag-only: split on commas, drop prose, validate/snap each tag, dedupe.
fn repair_tag_only(raw: &str) -> String {
    let mut seen = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for chunk in raw.split(',') {
        let token = chunk.trim().trim_matches(|c| c == '.' || c == '"' || c == '\'');
        if token.is_empty() {
            continue;
        }
        // Drop obvious prose: a chunk with >4 words is a sentence, not a tag.
        if token.split_whitespace().count() > 4 {
            continue;
        }
        if let Some(canon) = resolve_tag(token) {
            let display = to_display(&canon);
            if seen.insert(display.clone()) {
                out.push(display);
            }
        }
        // Unrecognized tokens are dropped (hallucination guard).
    }
    out.join(", ")
}

/// Anima: keep natural-language clauses + Gelbooru tags; force recognized
/// artists into @artist form.
fn repair_anima(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for chunk in raw.split(',') {
        let token = chunk.trim();
        if token.is_empty() {
            continue;
        }
        // Already an @artist reference — validate it.
        if let Some(rest) = token.strip_prefix('@') {
            if let Some(canon) = resolve_artist(rest) {
                let formatted = format!("@{}", to_display(&canon).replace(' ', "_"));
                if seen.insert(formatted.clone()) {
                    out.push(formatted);
                }
            }
            continue;
        }
        // A bare recognized artist → promote to @artist.
        if let Some(canon) = resolve_artist(token) {
            let formatted = format!("@{}", to_display(&canon).replace(' ', "_"));
            if seen.insert(formatted.clone()) {
                out.push(formatted);
            }
            continue;
        }
        // Otherwise keep the clause/tag as-is (natural language allowed).
        let cleaned = token.trim_matches('"').trim().to_string();
        if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
            out.push(cleaned);
        }
    }
    out.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_loads_known_tags() {
        let c = corpus();
        assert!(c.tags.contains("1girl"), "expected 1girl in corpus");
        assert!(!c.tags.is_empty());
    }

    #[test]
    fn tag_only_drops_prose_and_unknowns() {
        // "1girl" valid; the sentence is prose (>4 words) → dropped;
        // "zzzznotarealtag" unknown → dropped.
        let out = repair_tag_only("1girl, this is clearly a long sentence, zzzznotarealtag, solo");
        assert_eq!(out, "1girl, solo");
    }

    #[test]
    fn tag_only_snaps_alias() {
        // "1_girl" is an alias of "1girl".
        let out = repair_tag_only("1_girl");
        assert_eq!(out, "1girl");
    }

    #[test]
    fn tag_only_dedupes() {
        let out = repair_tag_only("solo, solo, 1girl");
        assert_eq!(out, "solo, 1girl");
    }

    #[test]
    fn anima_keeps_prose_clauses() {
        let out = repair_anima("a serene forest at dawn, 1girl, soft lighting");
        assert!(out.contains("a serene forest at dawn"));
        assert!(out.contains("1girl"));
    }
}
```

**Note:** the alias-snap test (`1_girl`) and dedupe assume specific corpus contents seen in the file head. If a test asserts on a tag/alias not present, adjust the assertion to a tag you confirm via `Grep` in `anima-tags.json` — do NOT weaken the repair logic to pass.

- [ ] **Step 2: Run the unit tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml prompt_assistant::grounding`
Expected: 5 tests PASS. First run is slower (parses 6.4 MB once).

---

### Task 5: `llama-server` runtime — resolve/download/spawn/health/generate/idle

**Files:**
- Create: `src-tauri/src/prompt_assistant/server.rs`

This is the highest-risk task (external binary distribution). Backends: **Vulkan** (Win/Linux, covers NVIDIA/AMD/Intel without a CUDA toolkit) and **Metal** (macOS); **CPU** fallback. NVFP4 needs a CUDA build — for v1 the CUDA backend constants are present and used when an NVFP4 model is requested on Windows. The pinned release tag is a single constant — update it to roll the binary forward.

- [ ] **Step 1: Write `server.rs`**

Create `src-tauri/src/prompt_assistant/server.rs`:

```rust
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use crate::error::AppError;

/// Pinned llama.cpp release. Update this constant to roll the binary forward.
const LLAMA_RELEASE: &str = "b4585";
const LLAMA_BASE_URL: &str = "https://github.com/ggml-org/llama.cpp/releases/download";

/// Acceleration backend for the downloaded binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Vulkan,
    Metal,
    Cuda,
    Cpu,
}

/// Pick the default backend for this platform (GPU-accelerated where possible).
pub fn default_backend(needs_cuda: bool) -> Backend {
    if cfg!(target_os = "macos") {
        Backend::Metal
    } else if needs_cuda {
        Backend::Cuda
    } else if cfg!(any(target_os = "windows", target_os = "linux")) {
        Backend::Vulkan
    } else {
        Backend::Cpu
    }
}

/// Archive asset name(s) for a backend on this platform. The second entry is an
/// optional companion archive (e.g. Windows CUDA runtime).
fn assets_for(backend: Backend) -> (String, Option<String>) {
    let t = LLAMA_RELEASE;
    #[cfg(target_os = "windows")]
    {
        match backend {
            Backend::Vulkan => (format!("llama-{t}-bin-win-vulkan-x64.zip"), None),
            Backend::Cuda => (
                format!("llama-{t}-bin-win-cuda-cu12.4-x64.zip"),
                Some("cudart-llama-bin-win-cu12.4-x64.zip".to_string()),
            ),
            _ => (format!("llama-{t}-bin-win-cpu-x64.zip"), None),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = backend;
        (format!("llama-{t}-bin-ubuntu-x64.zip"), None)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = backend;
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        (format!("llama-{t}-bin-macos-{arch}.zip"), None)
    }
}

#[cfg(target_os = "windows")]
const SERVER_BIN: &str = "llama-server.exe";
#[cfg(not(target_os = "windows"))]
const SERVER_BIN: &str = "llama-server";

/// Manages a single llama-server child process and its idle lifetime.
pub struct LlamaServer {
    bin_dir: PathBuf,
    child: tokio::sync::Mutex<Option<Child>>,
    port: std::sync::atomic::AtomicU16,
    active_model: std::sync::Mutex<Option<String>>,
    last_used: std::sync::Mutex<Instant>,
    watchdog_started: std::sync::atomic::AtomicBool,
}

impl LlamaServer {
    pub fn new(bin_dir: PathBuf) -> Self {
        Self {
            bin_dir,
            child: tokio::sync::Mutex::new(None),
            port: std::sync::atomic::AtomicU16::new(0),
            active_model: std::sync::Mutex::new(None),
            last_used: std::sync::Mutex::new(Instant::now()),
            watchdog_started: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn server_path(&self) -> PathBuf {
        self.bin_dir.join(SERVER_BIN)
    }

    pub fn is_binary_present(&self) -> bool {
        self.server_path().exists()
    }

    pub fn is_running(&self) -> bool {
        self.port.load(std::sync::atomic::Ordering::Relaxed) != 0
    }

    pub fn active_model(&self) -> Option<String> {
        self.active_model.lock().unwrap().clone()
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    /// Download + extract the llama-server binary for the given backend if absent.
    pub async fn ensure_binary(
        &self,
        client: &reqwest::Client,
        backend: Backend,
        progress: &dyn Fn(&str, u64, u64, bool),
    ) -> Result<(), AppError> {
        if self.is_binary_present() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.bin_dir)?;
        let (primary, companion) = assets_for(backend);
        for asset in std::iter::once(primary).chain(companion) {
            let url = format!("{LLAMA_BASE_URL}/{LLAMA_RELEASE}/{asset}");
            let archive = self.bin_dir.join(&asset);
            download_with_progress(client, &url, &archive, &asset, progress).await?;
            extract_all_into(&archive, &self.bin_dir)?;
            std::fs::remove_file(&archive).ok();
        }
        if !self.is_binary_present() {
            return Err(AppError::LlmError(format!(
                "llama-server not found after extracting {}",
                self.bin_dir.display()
            )));
        }
        Ok(())
    }

    /// Ensure the server is running with `model_path` loaded. Spawns + health-polls
    /// on first use or after an idle unload / model switch.
    pub async fn ensure_running(
        &self,
        model_path: &Path,
        model_id: &str,
        n_gpu_layers: i32,
        idle_timeout_secs: u64,
    ) -> Result<u16, AppError> {
        // Already running with the right model?
        if self.is_running() && self.active_model().as_deref() == Some(model_id) {
            self.touch();
            return Ok(self.port.load(std::sync::atomic::Ordering::Relaxed));
        }
        // Switching models: stop the old server first.
        if self.is_running() {
            self.unload().await;
        }

        let port = pick_free_port()?;
        let mut cmd = Command::new(self.server_path());
        cmd.arg("-m")
            .arg(model_path)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("-ngl")
            .arg(n_gpu_layers.to_string())
            .arg("--no-webui")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            // tokio::process::Command exposes creation_flags inherently on Windows;
            // the std::os::windows CommandExt trait is NOT needed (importing it warns as unused).
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let child = cmd
            .spawn()
            .map_err(|e| AppError::LlmError(format!("Failed to spawn llama-server: {e}")))?;

        *self.child.lock().await = Some(child);
        self.port.store(port, std::sync::atomic::Ordering::Relaxed);
        *self.active_model.lock().unwrap() = Some(model_id.to_string());

        // Health poll (up to ~60s).
        let health = format!("http://127.0.0.1:{port}/health");
        let client = reqwest::Client::new();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if Instant::now() > deadline {
                self.unload().await;
                return Err(AppError::LlmError("llama-server health timeout".into()));
            }
            if let Ok(resp) = client.get(&health).send().await {
                if resp.status().is_success() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        self.touch();
        self.spawn_idle_watchdog(idle_timeout_secs);
        Ok(port)
    }

    /// POST a single chat completion and return the assistant message content.
    pub async fn chat(
        &self,
        client: &reqwest::Client,
        port: u16,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        self.touch();
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
        let body = json!({
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "temperature": 0.7,
            "max_tokens": max_tokens,
            "stream": false
        });
        let resp = client
            .post(&url)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| AppError::LlmError(format!("llama-server request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::LlmError(format!(
                "llama-server returned {}",
                resp.status()
            )));
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::LlmError(format!("Bad llama-server response: {e}")))?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        self.touch();
        Ok(content)
    }

    /// Terminate the server and clear running state.
    pub async fn unload(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        self.port.store(0, std::sync::atomic::Ordering::Relaxed);
        *self.active_model.lock().unwrap() = None;
    }

    /// Spawn (once) a background task that unloads the server after idle timeout.
    fn spawn_idle_watchdog(self: &std::sync::Arc<Self>, _idle_secs: u64) {
        // Note: takes &Arc<Self>; see PromptAssistant which wraps LlamaServer in Arc.
    }
}

/// Idle watchdog implemented as a free function so it can hold an Arc clone.
pub fn start_idle_watchdog(server: std::sync::Arc<LlamaServer>, idle_secs: u64) {
    if server
        .watchdog_started
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return; // already running
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            if !server.is_running() {
                continue;
            }
            let idle = server.last_used.lock().unwrap().elapsed().as_secs();
            if idle >= idle_secs {
                log::info!("[prompt-assistant] idle {idle}s, unloading llama-server");
                server.unload().await;
            }
        }
    });
}

fn pick_free_port() -> Result<u16, AppError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| AppError::LlmError(format!("No free port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::LlmError(e.to_string()))?
        .port();
    Ok(port)
}

async fn download_with_progress(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
    progress: &dyn Fn(&str, u64, u64, bool),
) -> Result<(), AppError> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::LlmError(format!("Download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::LlmError(format!(
            "Download returned {}",
            resp.status()
        )));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = tokio::fs::File::create(dest).await?;
    progress(label, 0, total, false);
    let mut resp = resp;
    let mut last_emit = 0u64;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| AppError::LlmError(format!("Download read error: {e}")))?
    {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if downloaded - last_emit > 1024 * 1024 || downloaded == total {
            last_emit = downloaded;
            progress(label, downloaded, total, false);
        }
    }
    file.flush().await?;
    progress(label, downloaded, total, true);
    Ok(())
}

/// Extract every file from a zip archive flatly into `dir` (strip directories),
/// preserving executable bits on unix.
fn extract_all_into(archive_path: &Path, dir: &Path) -> Result<(), AppError> {
    let file = std::fs::File::open(archive_path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::LlmError(format!("Bad zip: {e}")))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::LlmError(format!("Zip entry error: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = match entry.enclosed_name().and_then(|p| p.file_name().map(|f| f.to_owned())) {
            Some(n) => n,
            None => continue,
        };
        let out_path = dir.join(name);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if out_path.file_name().and_then(|n| n.to_str()) == Some(SERVER_BIN) {
                std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    Ok(())
}
```

> Remove the empty `spawn_idle_watchdog` method (it was a placeholder for the `&Arc<Self>` signature note). The real watchdog is the free `start_idle_watchdog(Arc<LlamaServer>, u64)`, called from `PromptAssistant::ensure_running` in Task 6. Delete the method body and its call site in `ensure_running` (replace `self.spawn_idle_watchdog(idle_timeout_secs);` with nothing — Task 6 starts the watchdog at the `PromptAssistant` layer where an `Arc<LlamaServer>` is available).

- [ ] **Step 2: Apply the watchdog fix**

In `ensure_running`, delete the line `self.spawn_idle_watchdog(idle_timeout_secs);` and delete the `fn spawn_idle_watchdog` method. Keep the `idle_timeout_secs` param (Task 6 passes it through and calls `start_idle_watchdog`).

- [ ] **Step 3: Gate check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `prompt_assistant/{hardware,catalog,grounding,server}.rs` all compile. (`mod.rs` still only has `pub mod` lines — fine.) Resolve any `zip`/`tokio::fs` feature gaps by confirming the deps are already enabled (interrogator uses `zip`; tokio `fs`/`process`/`io-util` features are needed — add to the `tokio` features in `Cargo.toml` if `cargo check` complains).

---

### Task 6: `PromptAssistant` state — install/delete/status + ensure_running

**Files:**
- Modify: `src-tauri/src/prompt_assistant/mod.rs`
- Modify: `src-tauri/src/state.rs` (add field + construct)

- [ ] **Step 1: Flesh out `mod.rs`**

Replace `src-tauri/src/prompt_assistant/mod.rs` with:

```rust
pub mod catalog;
pub mod grounding;
pub mod hardware;
pub mod server;

use std::path::PathBuf;
use std::sync::Arc;

use crate::config;
use crate::error::AppError;
use server::{Backend, LlamaServer};

/// Top-level prompt-assistant state held in AppState.
pub struct PromptAssistant {
    /// {app_data}/prompt-assistant
    root: PathBuf,
    pub server: Arc<LlamaServer>,
}

impl Default for PromptAssistant {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptAssistant {
    pub fn new() -> Self {
        let root = config::app_data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("prompt-assistant");
        let server = Arc::new(LlamaServer::new(root.join("bin")));
        Self { root, server }
    }

    fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    /// Local on-disk path for a catalog variant's weight file.
    pub fn model_file_path(&self, model_id: &str, file: &str) -> PathBuf {
        self.models_dir().join(model_id).join(file)
    }

    /// Whether ANY variant of a catalog model id is installed.
    pub fn is_model_installed(&self, model_id: &str) -> bool {
        let entry = match catalog::entry(model_id) {
            Some(e) => e,
            None => return false,
        };
        entry
            .variants
            .iter()
            .any(|v| self.model_file_path(model_id, &v.file).exists())
    }

    /// Catalog model ids that are installed.
    pub fn installed_models(&self) -> Vec<String> {
        catalog::catalog()
            .into_iter()
            .filter(|e| self.is_model_installed(&e.id))
            .map(|e| e.id)
            .collect()
    }

    /// Find the installed variant file path + whether it is NVFP4, for a model id.
    fn installed_variant(&self, model_id: &str) -> Option<(PathBuf, bool)> {
        let entry = catalog::entry(model_id)?;
        for v in &entry.variants {
            let p = self.model_file_path(model_id, &v.file);
            if p.exists() {
                return Some((p, v.format == "nvfp4"));
            }
        }
        None
    }

    /// Download a specific variant (by format key: "gguf:Q4_K_M" / "nvfp4") of a model.
    pub async fn download_model(
        &self,
        client: &reqwest::Client,
        model_id: &str,
        variant_key: &str,
        progress: &dyn Fn(&str, u64, u64, bool),
    ) -> Result<(), AppError> {
        let entry =
            catalog::entry(model_id).ok_or_else(|| AppError::LlmError("Unknown model id".into()))?;
        let variant = entry
            .variants
            .iter()
            .find(|v| variant_matches(v, variant_key))
            .ok_or_else(|| AppError::LlmError("Unknown variant".into()))?;

        let dest = self.model_file_path(model_id, &variant.file);
        if dest.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(dest.parent().unwrap())?;
        // HuggingFace resolve URL (mirrors interrogator's HF_BASE_URL pattern).
        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            variant.repo, variant.file
        );
        download_to(client, &url, &dest, &variant.file, progress).await
    }

    /// Delete all installed files for a model id.
    pub fn delete_model(&self, model_id: &str) -> Result<(), AppError> {
        let dir = self.models_dir().join(model_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Ensure the server is up and the model loaded, starting the idle watchdog.
    /// `total_vram_mb` drives GPU-layer offload; `nvfp4_capable` selects the backend.
    pub async fn ensure_running(
        &self,
        client: &reqwest::Client,
        model_id: &str,
        total_vram_mb: u64,
        nvfp4_capable: bool,
        idle_timeout_secs: u64,
        progress: &dyn Fn(&str, u64, u64, bool),
    ) -> Result<u16, AppError> {
        let (model_path, is_nvfp4) = self
            .installed_variant(model_id)
            .ok_or_else(|| AppError::LlmError("Model not installed".into()))?;

        let backend = server::default_backend(is_nvfp4 && nvfp4_capable);
        self.server.ensure_binary(client, backend, progress).await?;

        // Offload all layers when the model fits comfortably in VRAM, else CPU.
        let variant_vram = catalog::entry(model_id)
            .and_then(|e| e.variants.into_iter().find(|v| {
                self.model_file_path(model_id, &v.file).exists()
            }))
            .map(|v| v.vram_mb)
            .unwrap_or(u64::MAX);
        let n_gpu_layers = if total_vram_mb >= variant_vram { 999 } else { 0 };

        let port = self
            .server
            .ensure_running(&model_path, model_id, n_gpu_layers, idle_timeout_secs)
            .await?;
        server::start_idle_watchdog(self.server.clone(), idle_timeout_secs);
        Ok(port)
    }
}

/// Match a catalog variant against a frontend variant key.
/// Keys: "nvfp4" or "gguf:<QUANT>" (e.g. "gguf:Q4_K_M").
fn variant_matches(v: &catalog::LlmVariant, key: &str) -> bool {
    if v.format == "nvfp4" {
        return key == "nvfp4";
    }
    match key.strip_prefix("gguf:") {
        Some(q) => v.quant.as_deref() == Some(q),
        None => false,
    }
}

async fn download_to(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
    label: &str,
    progress: &dyn Fn(&str, u64, u64, bool),
) -> Result<(), AppError> {
    use tokio::io::AsyncWriteExt;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::LlmError(format!("Download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::LlmError(format!(
            "Download returned {}",
            resp.status()
        )));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut file = tokio::fs::File::create(dest).await?;
    progress(label, 0, total, false);
    let mut resp = resp;
    let mut last_emit = 0u64;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| AppError::LlmError(format!("Download read error: {e}")))?
    {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if downloaded - last_emit > 1024 * 1024 || downloaded == total {
            last_emit = downloaded;
            progress(label, downloaded, total, false);
        }
    }
    file.flush().await?;
    progress(label, downloaded, total, true);
    Ok(())
}
```

Re-export the catalog/hardware types for command use by adding at the end of `mod.rs`:

```rust
pub use catalog::{LlmCatalogEntry, LlmVariant};
pub use hardware::{LlmGpu, LlmHardware};
```

- [ ] **Step 2: Add the AppState field**

In `src-tauri/src/state.rs`:
- Add import near the interrogator one (line ~11):

```rust
#[cfg(any(feature = "desktop", feature = "server"))]
use crate::prompt_assistant::PromptAssistant;
```

- Add the field in `struct AppState` (after `interrogator`, ~line 476):

```rust
    #[cfg(any(feature = "desktop", feature = "server"))]
    pub prompt_assistant: Arc<PromptAssistant>,
```

- Construct it in `AppState::new` (after the interrogator line, ~line 541):

```rust
            #[cfg(any(feature = "desktop", feature = "server"))]
            prompt_assistant: Arc::new(PromptAssistant::new()),
```

- [ ] **Step 3: Gate check**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles clean.

---

### Task 7: Commands + registration

**Files:**
- Create: `src-tauri/src/commands/prompt_assistant.rs`
- Modify: `src-tauri/src/lib.rs:356-447` (generate_handler)

- [ ] **Step 1: Write the commands**

Create `src-tauri/src/commands/prompt_assistant.rs`:

```rust
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::error::AppError;
use crate::prompt_assistant::grounding::{self, GenMode};
use crate::prompt_assistant::{catalog, hardware, LlmCatalogEntry, LlmHardware};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct LlmStatus {
    pub installed_models: Vec<String>,
    pub active_model: Option<String>,
    pub server_running: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct PromptAssistantOpts {
    /// "short" | "medium" | "detailed"
    pub length: Option<String>,
    #[serde(default)]
    pub include_artists: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    filename: String,
    downloaded: u64,
    total: u64,
    done: bool,
}

#[tauri::command]
pub async fn detect_llm_hardware() -> Result<LlmHardware, AppError> {
    Ok(tokio::task::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| AppError::LlmError(format!("hardware detect failed: {e}")))?)
}

#[tauri::command]
pub async fn list_llm_catalog() -> Result<Vec<LlmCatalogEntry>, AppError> {
    Ok(catalog::catalog())
}

#[tauri::command]
pub async fn llm_status(state: State<'_, Arc<AppState>>) -> Result<LlmStatus, AppError> {
    let pa = &state.prompt_assistant;
    Ok(LlmStatus {
        installed_models: pa.installed_models(),
        active_model: pa.server.active_model(),
        server_running: pa.server.is_running(),
    })
}

#[tauri::command]
pub async fn download_llm_model(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
    variant: String,
) -> Result<(), AppError> {
    let pa = state.prompt_assistant.clone();
    let app2 = app.clone();
    let progress = move |filename: &str, downloaded: u64, total: u64, done: bool| {
        app2.emit(
            "llm:download_progress",
            DownloadProgress {
                filename: filename.to_string(),
                downloaded,
                total,
                done,
            },
        )
        .ok();
    };
    pa.download_model(&state.http_client, &id, &variant, &progress)
        .await?;
    // Persist selected model id + mark setup done.
    {
        let mut cfg = state.config.write().await;
        cfg.prompt_assistant_model_id = Some(id.clone());
        cfg.prompt_assistant_setup_done = true;
        let _ = crate::config::save_config(&cfg);
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_llm_model(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), AppError> {
    state.prompt_assistant.delete_model(&id)?;
    Ok(())
}

#[tauri::command]
pub async fn unload_llm(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    state.prompt_assistant.server.unload().await;
    Ok(())
}

/// Shared core for enhance/compose: guard, ensure server, ground, generate, repair.
async fn run_generation(
    app: &AppHandle,
    state: &State<'_, Arc<AppState>>,
    input: &str,
    family: &str,
    mode: GenMode,
    opts: &PromptAssistantOpts,
) -> Result<String, AppError> {
    // Generation guard: do not contend with an active ComfyUI generation.
    if !state.prompt_queue.is_empty() {
        return Err(AppError::LlmError(
            "prompt_assistant.busy_generation".into(),
        ));
    }

    let (model_id, idle_secs) = {
        let cfg = state.config.read().await;
        (
            cfg.prompt_assistant_model_id.clone(),
            cfg.prompt_assistant_idle_timeout_secs,
        )
    };
    let model_id = model_id.ok_or_else(|| AppError::LlmError("prompt_assistant.no_model".into()))?;

    let hw = tokio::task::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| AppError::LlmError(e.to_string()))?;

    app.emit("llm:stage", "loading_model").ok();
    let pa = state.prompt_assistant.clone();
    let app2 = app.clone();
    let progress = move |filename: &str, downloaded: u64, total: u64, done: bool| {
        app2.emit(
            "llm:download_progress",
            DownloadProgress {
                filename: filename.to_string(),
                downloaded,
                total,
                done,
            },
        )
        .ok();
    };
    let port = pa
        .ensure_running(
            &state.http_client,
            &model_id,
            hw.total_vram_mb,
            hw.nvfp4_capable,
            idle_secs,
            &progress,
        )
        .await?;

    app.emit("llm:stage", "generating").ok();
    let candidates = grounding::retrieve_candidates(input, 40);
    let system = grounding::system_prompt(family, mode, &candidates);
    let max_tokens = match opts.length.as_deref() {
        Some("short") => 96,
        Some("detailed") => 384,
        _ => 192,
    };
    let raw = pa
        .server
        .chat(&state.http_client, port, &system, input, max_tokens)
        .await?;
    let cleaned = grounding::repair(&raw, family);
    Ok(cleaned)
}

#[tauri::command]
pub async fn enhance_prompt(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    prompt: String,
    family: String,
    opts: Option<PromptAssistantOpts>,
) -> Result<String, AppError> {
    let opts = opts.unwrap_or_default();
    run_generation(&app, &state, &prompt, &family, GenMode::Enhance, &opts).await
}

#[tauri::command]
pub async fn compose_prompt(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    description: String,
    family: String,
    opts: Option<PromptAssistantOpts>,
) -> Result<String, AppError> {
    let opts = opts.unwrap_or_default();
    run_generation(&app, &state, &description, &family, GenMode::Compose, &opts).await
}
```

- [ ] **Step 2: Register in `lib.rs`**

In `src-tauri/src/lib.rs`, inside `generate_handler![]`, after the interrogator commands (~line 433):

```rust
            commands::prompt_assistant::detect_llm_hardware,
            commands::prompt_assistant::list_llm_catalog,
            commands::prompt_assistant::llm_status,
            commands::prompt_assistant::download_llm_model,
            commands::prompt_assistant::delete_llm_model,
            commands::prompt_assistant::unload_llm,
            commands::prompt_assistant::enhance_prompt,
            commands::prompt_assistant::compose_prompt,
```

- [ ] **Step 3: Gate check**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml`
Expected: compiles, no clippy errors. Fix any warnings clippy flags (unused imports, needless clones).

- [ ] **Step 4: Run all prompt-assistant unit tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml prompt_assistant`
Expected: all hardware/catalog/grounding tests PASS (12 total).

- [ ] **Step 5: Commit**

```bash
git -c core.hooksPath=/dev/null add src-tauri/
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): backend runtime, hardware detect, catalog, grounding, commands"
```

---

### Task 8: Browser-mode dispatch parity

**Files:**
- Modify: `src-tauri/src/webserver.rs` (the command dispatch match, ~line 4099)

Browser/server mode routes IPC through the axum dispatch. The hardware/catalog/status/unload commands and enhance/compose must be reachable there too. (Download progress in browser mode goes over SSE via `state.event_tx`; for v1 we mark download as desktop-first and surface a clear message in browser mode — enhance/compose still work if a model is already installed.)

- [ ] **Step 1: Add dispatch arms**

In `src-tauri/src/webserver.rs`, find the big `match command` block (near the interrogator arm at line ~4099) and add, following the existing arm style (feature-gated, reading args from `args`, returning JSON or string):

```rust
        // --- Prompt assistant ---
        #[cfg(any(feature = "desktop", feature = "server"))]
        "detect_llm_hardware" => {
            let hw = tokio::task::spawn_blocking(crate::prompt_assistant::hardware::detect)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(hw).map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "list_llm_catalog" => {
            serde_json::to_value(crate::prompt_assistant::catalog::catalog())
                .map_err(|e| e.to_string())
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "llm_status" => {
            let pa = &state.prompt_assistant;
            Ok(serde_json::json!({
                "installed_models": pa.installed_models(),
                "active_model": pa.server.active_model(),
                "server_running": pa.server.is_running(),
            }))
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "unload_llm" => {
            state.prompt_assistant.server.unload().await;
            Ok(serde_json::Value::Null)
        }
        #[cfg(any(feature = "desktop", feature = "server"))]
        "enhance_prompt" | "compose_prompt" => {
            let input = if command == "enhance_prompt" {
                args["prompt"].as_str().unwrap_or("").to_string()
            } else {
                args["description"].as_str().unwrap_or("").to_string()
            };
            let family = args["family"].as_str().unwrap_or("unknown").to_string();
            let mode = if command == "enhance_prompt" {
                crate::prompt_assistant::grounding::GenMode::Enhance
            } else {
                crate::prompt_assistant::grounding::GenMode::Compose
            };
            let result = crate::webserver::run_prompt_assistant_headless(
                state, &input, &family, mode,
            )
            .await?;
            Ok(serde_json::Value::String(result))
        }
```

- [ ] **Step 2: Add the headless helper**

Add near the interrogator headless helper (the `run_interrogation`-style fn at line ~4257) a `run_prompt_assistant_headless` that mirrors `run_generation` but emits stages over `state.event_tx` instead of `app.emit`, and downloads silently (progress over SSE optional). Reuse the same guard/ensure/ground/generate/repair sequence. Reference the interrogator headless fn for the `event_tx.send(BroadcastEvent{...})` pattern.

```rust
#[cfg(any(feature = "desktop", feature = "server"))]
pub async fn run_prompt_assistant_headless(
    state: &Arc<AppState>,
    input: &str,
    family: &str,
    mode: crate::prompt_assistant::grounding::GenMode,
) -> Result<String, String> {
    use crate::prompt_assistant::{grounding, hardware};
    if !state.prompt_queue.is_empty() {
        return Err("prompt_assistant.busy_generation".to_string());
    }
    let (model_id, idle_secs) = {
        let cfg = state.config.read().await;
        (
            cfg.prompt_assistant_model_id.clone(),
            cfg.prompt_assistant_idle_timeout_secs,
        )
    };
    let model_id = model_id.ok_or_else(|| "prompt_assistant.no_model".to_string())?;
    let hw = tokio::task::spawn_blocking(hardware::detect)
        .await
        .map_err(|e| e.to_string())?;
    let noop = |_: &str, _: u64, _: u64, _: bool| {};
    let port = state
        .prompt_assistant
        .ensure_running(
            &state.http_client,
            &model_id,
            hw.total_vram_mb,
            hw.nvfp4_capable,
            idle_secs,
            &noop,
        )
        .await
        .map_err(|e| e.to_string())?;
    let candidates = grounding::retrieve_candidates(input, 40);
    let system = grounding::system_prompt(family, mode, &candidates);
    let raw = state
        .prompt_assistant
        .server
        .chat(&state.http_client, port, &system, input, 192)
        .await
        .map_err(|e| e.to_string())?;
    Ok(grounding::repair(&raw, family))
}
```

- [ ] **Step 3: Gate check**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml --features server`
Expected: compiles with the `server` feature too. Also run default-feature `cargo check`. Commit:

```bash
git -c core.hooksPath=/dev/null add src-tauri/src/webserver.rs
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): browser-mode dispatch parity"
```

---

## PHASE 2 — IPC + store + wiring

### Task 9: TypeScript types

**Files:**
- Modify: `src/lib/types/index.ts` (after `InterrogationResult`/`GpuStats`, ~line 314)

- [ ] **Step 1: Add the interfaces (snake_case to mirror Rust)**

```typescript
export interface LlmGpu {
  name: string;
  vram_mb: number;
  vendor: string;
  nvfp4_capable: boolean;
}

export interface LlmHardware {
  gpus: LlmGpu[];
  total_vram_mb: number;
  system_ram_mb: number;
  nvfp4_capable: boolean;
  recommended_model_id: string;
}

export interface LlmVariant {
  format: "gguf" | "nvfp4";
  quant: string | null;
  size_mb: number;
  vram_mb: number;
  repo: string;
  file: string;
}

export interface LlmCatalogEntry {
  id: string;
  name: string;
  purpose: "tag_upsampler" | "natural_language";
  families: string[];
  variants: LlmVariant[];
  pros: string;
  cons: string;
  best_for: string;
}

export interface LlmStatus {
  installed_models: string[];
  active_model: string | null;
  server_running: boolean;
}

export interface PromptAssistantOpts {
  length?: "short" | "medium" | "detailed";
  include_artists?: boolean;
}
```

- [ ] **Step 2: Gate check** — `npm run build` (will be exercised fully in later tasks; for now `npx tsc --noEmit` if available, else proceed).

---

### Task 10: api.ts wrappers

**Files:**
- Modify: `src/lib/utils/api.ts` (after `interrogateClipboard`, ~line 740)

- [ ] **Step 1: Add typed wrappers + types import**

Ensure the import line at the top of `api.ts` includes the new types:

```typescript
import type {
  LlmHardware,
  LlmCatalogEntry,
  LlmStatus,
  PromptAssistantOpts,
} from "../types/index.js";
```

Add the wrappers (all go through `ipcInvoke`):

```typescript
export async function detectLlmHardware(): Promise<LlmHardware> {
  return ipcInvoke("detect_llm_hardware");
}

export async function listLlmCatalog(): Promise<LlmCatalogEntry[]> {
  return ipcInvoke("list_llm_catalog");
}

export async function llmStatus(): Promise<LlmStatus> {
  return ipcInvoke("llm_status");
}

export async function downloadLlmModel(id: string, variant: string): Promise<void> {
  return ipcInvoke("download_llm_model", { id, variant });
}

export async function deleteLlmModel(id: string): Promise<void> {
  return ipcInvoke("delete_llm_model", { id });
}

export async function unloadLlm(): Promise<void> {
  return ipcInvoke("unload_llm");
}

export async function enhancePrompt(
  prompt: string,
  family: string,
  opts?: PromptAssistantOpts,
): Promise<string> {
  return ipcInvoke("enhance_prompt", { prompt, family, opts });
}

export async function composePrompt(
  description: string,
  family: string,
  opts?: PromptAssistantOpts,
): Promise<string> {
  return ipcInvoke("compose_prompt", { description, family, opts });
}
```

- [ ] **Step 2: Gate check** — proceed to Task 11 (build gate runs at end of phase).

---

### Task 11: promptAssistant store

**Files:**
- Create: `src/lib/stores/promptAssistant.svelte.ts`

Per repo rules: class singleton with `$state`, `*.svelte.ts` extension, **no imports of other stores**, reassign arrays with spread, guard persisted reads `!== undefined`. The store exposes data + actions returning result strings; the **caller** (PromptInputs.svelte) applies results to the generation store, keeping this store decoupled.

- [ ] **Step 1: Write the store**

Create `src/lib/stores/promptAssistant.svelte.ts`:

```typescript
import {
  detectLlmHardware,
  listLlmCatalog,
  llmStatus,
  downloadLlmModel,
  deleteLlmModel,
  unloadLlm,
  enhancePrompt,
  composePrompt,
} from "../utils/api.js";
import { ipcListen } from "../utils/ipc.js";
import type {
  LlmHardware,
  LlmCatalogEntry,
  LlmStatus,
  PromptAssistantOpts,
} from "../types/index.js";

interface DownloadProgress {
  filename: string;
  downloaded: number;
  total: number;
  done: boolean;
}

class PromptAssistantStore {
  hardware = $state<LlmHardware | null>(null);
  catalog = $state<LlmCatalogEntry[]>([]);
  status = $state<LlmStatus | null>(null);
  /** Auto-recommended model id from hardware (pre-selected in the modal). */
  recommendedModelId = $state<string | null>(null);
  /** User's current selection in the setup modal. */
  selectedModelId = $state<string | null>(null);

  isGenerating = $state(false);
  isDownloading = $state(false);
  downloadProgress = $state<DownloadProgress | null>(null);
  /** "loading_model" | "generating" | null */
  stage = $state<string | null>(null);

  setupModalOpen = $state(false);
  composeModalOpen = $state(false);

  /** True once at least one model is installed. */
  get hasInstalledModel(): boolean {
    return !!this.status && this.status.installed_models.length > 0;
  }

  /** Launch-time bootstrap: detect hardware, load catalog + status, pre-select. */
  async init(): Promise<void> {
    try {
      const [hw, cat, st] = await Promise.all([
        detectLlmHardware(),
        listLlmCatalog(),
        llmStatus(),
      ]);
      this.hardware = hw;
      this.catalog = [...cat];
      this.status = st;
      this.recommendedModelId = hw.recommended_model_id;
      // Pre-select: installed model > recommended.
      this.selectedModelId =
        st.installed_models[0] ?? hw.recommended_model_id ?? null;
    } catch (e) {
      console.warn("[promptAssistant] init failed", e);
    }
  }

  async refreshStatus(): Promise<void> {
    try {
      this.status = await llmStatus();
    } catch (e) {
      console.warn("[promptAssistant] status refresh failed", e);
    }
  }

  /** Default variant key for a model id given current hardware. */
  defaultVariantKey(modelId: string): string {
    const entry = this.catalog.find((e) => e.id === modelId);
    if (!entry) return "gguf:Q4_K_M";
    const nvfp4Ok = this.hardware?.nvfp4_capable ?? false;
    const vram = this.hardware?.total_vram_mb ?? 0;
    if (nvfp4Ok) {
      const v = entry.variants.find((v) => v.format === "nvfp4");
      if (v) return "nvfp4";
    }
    // Largest GGUF that fits, else smallest GGUF.
    const fitting = entry.variants
      .filter((v) => v.format === "gguf" && v.vram_mb <= vram)
      .sort((a, b) => b.vram_mb - a.vram_mb);
    const chosen = fitting[0] ?? entry.variants.find((v) => v.format === "gguf");
    return chosen?.quant ? `gguf:${chosen.quant}` : "gguf:Q4_K_M";
  }

  async download(modelId: string, variantKey: string): Promise<void> {
    this.isDownloading = true;
    this.downloadProgress = null;
    const unlisten = await ipcListen("llm:download_progress", (event: any) => {
      const p = event.payload as DownloadProgress;
      this.downloadProgress = p.done ? null : p;
    });
    try {
      await downloadLlmModel(modelId, variantKey);
      await this.refreshStatus();
    } finally {
      unlisten();
      this.isDownloading = false;
      this.downloadProgress = null;
    }
  }

  async deleteModel(modelId: string): Promise<void> {
    await deleteLlmModel(modelId);
    await this.refreshStatus();
  }

  async unload(): Promise<void> {
    await unloadLlm();
    await this.refreshStatus();
  }

  private async withStageListener<T>(fn: () => Promise<T>): Promise<T> {
    const unlisten = await ipcListen("llm:stage", (event: any) => {
      this.stage = event.payload as string;
    });
    try {
      return await fn();
    } finally {
      unlisten();
      this.stage = null;
    }
  }

  /** Returns the cleaned/enhanced prompt string. Caller applies it. */
  async enhance(
    prompt: string,
    family: string,
    opts?: PromptAssistantOpts,
  ): Promise<string> {
    this.isGenerating = true;
    try {
      return await this.withStageListener(() =>
        enhancePrompt(prompt, family, opts),
      );
    } finally {
      this.isGenerating = false;
    }
  }

  /** Returns the composed prompt string. Caller applies it. */
  async compose(
    description: string,
    family: string,
    opts?: PromptAssistantOpts,
  ): Promise<string> {
    this.isGenerating = true;
    try {
      return await this.withStageListener(() =>
        composePrompt(description, family, opts),
      );
    } finally {
      this.isGenerating = false;
    }
  }
}

export const promptAssistant = new PromptAssistantStore();
```

- [ ] **Step 2: Gate check** — proceed to Task 12.

---

### Task 12: Launch-time init in App.svelte

**Files:**
- Modify: `App.svelte`

- [ ] **Step 1: Wire init at startup**

In `App.svelte`, import the store and call `init()` in the existing startup `onMount`/init flow (where other stores bootstrap). Find where `generation` / config is loaded at mount and add:

```typescript
import { promptAssistant } from "./lib/stores/promptAssistant.svelte.js";
```

In the mount/startup async block (non-blocking — do not await in a way that delays first paint):

```typescript
  // Prompt assistant: detect hardware + pre-select recommended model at launch.
  promptAssistant.init();
```

Place this after the app has confirmed it is in Tauri/browser mode (so `ipcInvoke` works). It must not throw if the backend is briefly unavailable (the store swallows errors).

- [ ] **Step 2: Build gate**

Run: `npm run build`
Expected: build succeeds. Fix any type errors (most likely an import path or a missing type). Commit:

```bash
git -c core.hooksPath=/dev/null add src/lib/types/index.ts src/lib/utils/api.ts src/lib/stores/promptAssistant.svelte.ts App.svelte
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): TS types, api wrappers, store, launch init"
```

---

## PHASE 3 — Setup modal + Settings

### Task 13: PromptAssistantSetupModal

**Files:**
- Create: `src/lib/components/generation/PromptAssistantSetupModal.svelte`

Single modal (not a stepper). Tailwind only, `onclick` not `on:click`, all user strings via `locale.t(...)`. Reuse the dark palette and modal shell pattern from an existing modal (e.g. `InterrogateModal.svelte`) for the overlay/container classes.

- [ ] **Step 1: Write the component**

Create `src/lib/components/generation/PromptAssistantSetupModal.svelte`:

```svelte
<script lang="ts">
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import type { LlmCatalogEntry, LlmVariant } from "../../types/index.js";

  let { onClose, onInstalled }: { onClose: () => void; onInstalled?: () => void } =
    $props();

  const hw = $derived(promptAssistant.hardware);
  const catalog = $derived(promptAssistant.catalog);

  let selectedId = $state(promptAssistant.selectedModelId ?? "");
  let selectedVariant = $state("");

  $effect(() => {
    if (!selectedId && promptAssistant.selectedModelId) {
      selectedId = promptAssistant.selectedModelId;
    }
    if (selectedId && !selectedVariant) {
      selectedVariant = promptAssistant.defaultVariantKey(selectedId);
    }
  });

  function variantKey(v: LlmVariant): string {
    return v.format === "nvfp4" ? "nvfp4" : `gguf:${v.quant}`;
  }

  function variantLabel(v: LlmVariant): string {
    return v.format === "nvfp4" ? "NVFP4 (Blackwell)" : `GGUF ${v.quant}`;
  }

  function fits(v: LlmVariant): boolean {
    const vram = hw?.total_vram_mb ?? 0;
    // CPU path: allow if system RAM can hold it.
    if (vram < 2000) return (hw?.system_ram_mb ?? 0) * 0.6 >= v.vram_mb;
    return v.vram_mb <= vram;
  }

  function variantAvailable(v: LlmVariant): boolean {
    if (v.format === "nvfp4" && !(hw?.nvfp4_capable ?? false)) return false;
    return true;
  }

  function isInstalled(id: string): boolean {
    return promptAssistant.status?.installed_models.includes(id) ?? false;
  }

  let error = $state<string | null>(null);

  async function download() {
    if (!selectedId || !selectedVariant) return;
    error = null;
    try {
      promptAssistant.selectedModelId = selectedId;
      await promptAssistant.download(selectedId, selectedVariant);
      onInstalled?.();
      onClose();
    } catch (e: any) {
      error = String(e);
    }
  }

  function hardwareLabel(): string {
    if (!hw) return locale.t("prompt_assistant.detecting_hardware");
    if (hw.gpus.length === 0) {
      return locale.t("prompt_assistant.cpu_only");
    }
    const g = hw.gpus[0];
    const vramGb = (g.vram_mb / 1024).toFixed(0);
    const nvfp4 = hw.nvfp4_capable
      ? ` — ${locale.t("prompt_assistant.nvfp4_supported")}`
      : "";
    return `${g.name} — ${vramGb} GB VRAM${nvfp4}`;
  }
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
  onclick={onClose}
  role="presentation"
>
  <div
    class="max-h-[85vh] w-full max-w-2xl overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
    onclick={(e) => e.stopPropagation()}
    role="dialog"
    aria-modal="true"
  >
    <div class="mb-3 flex items-center justify-between">
      <h2 class="text-lg font-semibold text-neutral-100">
        {locale.t("prompt_assistant.setup_title")}
      </h2>
      <button
        class="rounded-lg px-2 py-1 text-neutral-400 hover:bg-neutral-800"
        onclick={onClose}
        aria-label={locale.t("common.close")}
      >
        ✕
      </button>
    </div>

    <!-- Hardware banner -->
    <div class="mb-4 rounded-lg border border-neutral-700 bg-neutral-800/50 px-3 py-2 text-sm text-neutral-300">
      {hardwareLabel()}
    </div>

    {#if error}
      <div class="mb-3 rounded-lg border border-red-700 bg-red-900/30 px-3 py-2 text-sm text-red-300">
        {error}
      </div>
    {/if}

    <!-- Model cards -->
    <div class="space-y-3">
      {#each catalog as entry (entry.id)}
        {@const recommended = entry.id === promptAssistant.recommendedModelId}
        {@const installed = isInstalled(entry.id)}
        <button
          class="w-full rounded-lg border p-3 text-left transition-colors {selectedId ===
          entry.id
            ? 'border-[var(--theme-accent-500)] bg-neutral-800'
            : 'border-neutral-700 bg-neutral-800/30 hover:bg-neutral-800/60'}"
          onclick={() => {
            selectedId = entry.id;
            selectedVariant = promptAssistant.defaultVariantKey(entry.id);
          }}
        >
          <div class="flex items-center justify-between">
            <span class="font-medium text-neutral-100">{entry.name}</span>
            <span class="flex gap-1">
              {#if recommended}
                <span class="rounded bg-[var(--theme-accent-600)] px-1.5 py-0.5 text-[10px] text-black">
                  {locale.t("prompt_assistant.recommended")}
                </span>
              {/if}
              {#if installed}
                <span class="rounded bg-green-700 px-1.5 py-0.5 text-[10px] text-white">
                  {locale.t("prompt_assistant.installed")}
                </span>
              {/if}
            </span>
          </div>
          <p class="mt-1 text-xs text-neutral-400">{entry.best_for}</p>
          <p class="mt-1 text-[11px] text-green-400">{entry.pros}</p>
          <p class="text-[11px] text-amber-400/80">{entry.cons}</p>

          {#if selectedId === entry.id}
            <div class="mt-2 flex flex-wrap gap-1.5">
              {#each entry.variants as v}
                {@const available = variantAvailable(v)}
                {@const ok = fits(v)}
                <button
                  disabled={!available}
                  title={!available
                    ? locale.t("prompt_assistant.needs_blackwell")
                    : !ok
                      ? locale.t("prompt_assistant.needs_vram", {
                          gb: (v.vram_mb / 1024).toFixed(1),
                        })
                      : ""}
                  class="rounded border px-2 py-0.5 text-[10px] {selectedVariant ===
                  variantKey(v)
                    ? 'border-[var(--theme-accent-500)] text-neutral-100'
                    : 'border-neutral-600 text-neutral-400'} {!available || !ok
                    ? 'opacity-40'
                    : 'hover:text-neutral-200'}"
                  onclick={(e) => {
                    e.stopPropagation();
                    if (available) selectedVariant = variantKey(v);
                  }}
                >
                  {variantLabel(v)} · {(v.size_mb / 1024).toFixed(1)} GB
                </button>
              {/each}
            </div>
          {/if}
        </button>
      {/each}
    </div>

    <!-- Download progress / action -->
    <div class="mt-4">
      {#if promptAssistant.isDownloading}
        {@const p = promptAssistant.downloadProgress}
        <div class="text-sm text-neutral-300">
          {locale.t("prompt_assistant.downloading")}
          {#if p && p.total > 0}
            <div class="mt-1 h-2 w-full overflow-hidden rounded bg-neutral-700">
              <div
                class="h-full bg-[var(--theme-accent-500)]"
                style="width: {((p.downloaded / p.total) * 100).toFixed(0)}%"
              ></div>
            </div>
            <span class="text-[11px] text-neutral-500">
              {(p.downloaded / 1024 / 1024).toFixed(0)} /
              {(p.total / 1024 / 1024).toFixed(0)} MB
            </span>
          {/if}
        </div>
      {:else}
        <button
          class="w-full rounded-lg bg-[var(--theme-accent-600)] px-4 py-2 font-medium text-black hover:bg-[var(--theme-accent-500)] disabled:opacity-50"
          disabled={!selectedId || !selectedVariant}
          onclick={download}
        >
          {isInstalled(selectedId)
            ? locale.t("prompt_assistant.use_model")
            : locale.t("prompt_assistant.download_install")}
        </button>
      {/if}
    </div>
  </div>
</div>
```

> If `locale.formatDecimal` is the project convention for numbers (seen in SettingsPage), swap the `.toFixed(...)` calls accordingly. Confirm the locale store import path (`../../stores/locale.svelte.js`) matches the project by checking an existing component's import.

- [ ] **Step 2: Build gate** — `npm run build`. Fix import paths / locale usage to match the codebase. (Keys are added in Task 17; missing keys fall back to English at runtime, so build still passes.)

---

### Task 14: Settings → Prompt Assistant section

**Files:**
- Modify: `src/lib/components/settings/SettingsPage.svelte`

- [ ] **Step 1: Register the collapsible section**

Mirror the interrogator section. Near line ~1029 add `prompt_assistant: false,` to the `collapsed` object. Near line ~1064 add to the section list:

```typescript
    { key: "prompt_assistant", labelKey: "settings.sections.prompt_assistant", keywords: "llm prompt enhance compose model gguf nvfp4 ai assistant" },
```

- [ ] **Step 2: Add the section UI after the interrogator block (~line 3333)**

Use the same collapsible header pattern as interrogator. Import the store at the top of the script block:

```typescript
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import PromptAssistantSetupModal from "../generation/PromptAssistantSetupModal.svelte";
```

Add a local `let showPromptAssistantSetup = $state(false);`. Then the section markup:

```svelte
        <!-- Prompt Assistant -->
        {#if sectionVisible("prompt_assistant")}
          <div class="...same wrapper classes as interrogator...">
            <button
              class="...same header classes..."
              onclick={() => (collapsed.prompt_assistant = !collapsed.prompt_assistant)}
            >
              {locale.t('settings.prompt_assistant.title')}
              <svg ...same chevron, using collapsed.prompt_assistant... ></svg>
            </button>
            {#if !collapsed.prompt_assistant}
              <div class="...same body classes...">
                <p class="text-sm text-neutral-400">
                  {locale.t('settings.prompt_assistant.desc')}
                </p>

                <!-- Installed model(s) -->
                <div class="mt-2 text-sm text-neutral-300">
                  {#if promptAssistant.status && promptAssistant.status.installed_models.length > 0}
                    {locale.t('settings.prompt_assistant.installed_label')}:
                    {promptAssistant.status.installed_models.join(", ")}
                  {:else}
                    {locale.t('settings.prompt_assistant.none_installed')}
                  {/if}
                </div>

                <div class="mt-2 flex flex-wrap gap-2">
                  <button
                    class="rounded-lg border border-neutral-600 px-3 py-1 text-sm text-neutral-200 hover:bg-neutral-800"
                    onclick={() => (showPromptAssistantSetup = true)}
                  >
                    {locale.t('settings.prompt_assistant.manage_models')}
                  </button>
                  {#each promptAssistant.status?.installed_models ?? [] as id}
                    <button
                      class="rounded-lg border border-red-700 px-3 py-1 text-sm text-red-300 hover:bg-red-900/30"
                      onclick={() => promptAssistant.deleteModel(id)}
                    >
                      {locale.t('settings.prompt_assistant.delete')} {id}
                    </button>
                  {/each}
                  <button
                    class="rounded-lg border border-neutral-600 px-3 py-1 text-sm text-neutral-200 hover:bg-neutral-800"
                    onclick={() => promptAssistant.unload()}
                  >
                    {locale.t('settings.prompt_assistant.unload_now')}
                  </button>
                </div>

                <!-- Idle timeout slider -->
                <label class="mt-3 block text-sm text-neutral-300">
                  {locale.t('settings.prompt_assistant.idle_timeout')}
                  <span class="text-neutral-300">{config.prompt_assistant_idle_timeout_secs}s</span>
                </label>
                <input
                  type="range"
                  min="30"
                  max="600"
                  step="30"
                  bind:value={config.prompt_assistant_idle_timeout_secs}
                  class="...same slider classes as interrogator..."
                />
              </div>
            {/if}
          </div>
        {/if}
```

At the bottom of the component (where modals are rendered), add:

```svelte
{#if showPromptAssistantSetup}
  <PromptAssistantSetupModal onClose={() => (showPromptAssistantSetup = false)} />
{/if}
```

> Copy the exact wrapper/header/body/slider class strings from the adjacent interrogator block (lines ~3275-3333) so the look matches. `config` here is the same reactive config object the interrogator settings bind to — the idle-timeout field persists through the existing config-save path (verify by checking how `interrogator_general_threshold` is persisted on change in this file, and replicate it for `prompt_assistant_idle_timeout_secs`).

- [ ] **Step 2: Build gate** — `npm run build`. Commit:

```bash
git -c core.hooksPath=/dev/null add src/lib/components/generation/PromptAssistantSetupModal.svelte src/lib/components/settings/SettingsPage.svelte
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): setup modal + settings section"
```

---

## PHASE 4 — Enhance (inline) + Compose modal + i18n

### Task 15: Enhance/Compose buttons in PromptInputs

**Files:**
- Modify: `src/lib/components/generation/PromptInputs.svelte` (the `mb-1 flex justify-end` Regional row, ~lines 140-158)

The Regional row becomes a split toolbar: enhance actions left, Regional right. ✨ Enhance operates on the positive prompt in place; ✍ Compose opens the modal. First-ever click of either opens the setup modal if no model is installed, then runs the action.

- [ ] **Step 1: Import store + modals + generation store (already imported)**

In the `<script>` of `PromptInputs.svelte`:

```typescript
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import PromptAssistantSetupModal from "./PromptAssistantSetupModal.svelte";
  import PromptComposeModal from "./PromptComposeModal.svelte";
  import { gallery } from "../../stores/gallery.svelte.js"; // for showToast — confirm path
```

Add local state + handlers:

```typescript
  let undoSnapshot = $state<string | null>(null);
  let showUndo = $state(false);
  let undoTimer: ReturnType<typeof setTimeout> | null = null;

  function pendingAction(): "enhance" | "compose" | null {
    return _pendingAction;
  }
  let _pendingAction = $state<"enhance" | "compose" | null>(null);

  async function onEnhanceClick() {
    if (!promptAssistant.hasInstalledModel) {
      _pendingAction = "enhance";
      promptAssistant.setupModalOpen = true;
      return;
    }
    await runEnhance();
  }

  async function runEnhance() {
    const current = generation.positivePrompt?.trim();
    if (!current) return;
    try {
      const result = await promptAssistant.enhance(current, generation.modelFamily);
      if (result && result.trim()) {
        undoSnapshot = generation.positivePrompt;
        generation.positivePrompt = result;
        generation.saveSettings?.();
        triggerUndo();
      } else {
        gallery.showToast?.(locale.t("prompt_assistant.couldnt_enhance"));
      }
    } catch (e: any) {
      gallery.showToast?.(mapLlmError(String(e)));
    }
  }

  function onComposeClick() {
    if (!promptAssistant.hasInstalledModel) {
      _pendingAction = "compose";
      promptAssistant.setupModalOpen = true;
      return;
    }
    promptAssistant.composeModalOpen = true;
  }

  function triggerUndo() {
    showUndo = true;
    if (undoTimer) clearTimeout(undoTimer);
    undoTimer = setTimeout(() => (showUndo = false), 10000);
  }

  function undoEnhance() {
    if (undoSnapshot !== null) {
      generation.positivePrompt = undoSnapshot;
      generation.saveSettings?.();
      undoSnapshot = null;
    }
    showUndo = false;
  }

  function mapLlmError(msg: string): string {
    if (msg.includes("busy_generation")) return locale.t("prompt_assistant.busy_generation");
    if (msg.includes("no_model")) return locale.t("prompt_assistant.no_model");
    return locale.t("prompt_assistant.error_generic");
  }

  function onSetupInstalled() {
    const action = _pendingAction;
    _pendingAction = null;
    if (action === "enhance") runEnhance();
    else if (action === "compose") promptAssistant.composeModalOpen = true;
  }
```

> Confirm `gallery.showToast` exists and its exact signature by checking `InterrogateModal.svelte` (the spec lists `gallery.showToast` as the toast path). If the toast API differs, adapt these calls. Confirm `generation.saveSettings` is the correct persist method name in `generation.svelte.ts`.

- [ ] **Step 2: Update the toolbar row markup**

Replace the `mb-1 flex justify-end` row so it is a split toolbar (enhance left, Regional right). Keep the existing Regional button exactly as-is on the right:

```svelte
<div class="mb-1 flex items-center justify-between">
  <div class="flex items-center gap-1.5">
    <button
      class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
      disabled={promptAssistant.isGenerating || !generation.positivePrompt?.trim()}
      title={locale.t("prompt_assistant.enhance_tooltip")}
      onclick={onEnhanceClick}
    >
      {#if promptAssistant.isGenerating}
        <span class="inline-block animate-spin">⟳</span>
      {:else}
        ✨
      {/if}
      {locale.t("prompt_assistant.enhance")}
    </button>
    <button
      class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
      disabled={promptAssistant.isGenerating}
      title={locale.t("prompt_assistant.compose_tooltip")}
      onclick={onComposeClick}
    >
      ✍ {locale.t("prompt_assistant.compose")}
    </button>
    {#if showUndo}
      <button
        class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-[var(--theme-accent-400)] hover:bg-neutral-800"
        onclick={undoEnhance}
      >
        ↩ {locale.t("prompt_assistant.undo")}
      </button>
    {/if}
  </div>

  <!-- existing Regional button stays here, unchanged -->
  <!-- ...Regional button markup... -->
</div>
```

Render the modals near the end of the component template:

```svelte
{#if promptAssistant.setupModalOpen}
  <PromptAssistantSetupModal
    onClose={() => (promptAssistant.setupModalOpen = false)}
    onInstalled={onSetupInstalled}
  />
{/if}
{#if promptAssistant.composeModalOpen}
  <PromptComposeModal onClose={() => (promptAssistant.composeModalOpen = false)} />
{/if}
```

- [ ] **Step 3: Build gate** — `npm run build`. (PromptComposeModal is created in Task 16; if build fails on the missing import, do Task 16 first, then this build.)

---

### Task 16: Compose modal

**Files:**
- Create: `src/lib/components/generation/PromptComposeModal.svelte`

- [ ] **Step 1: Write the component**

Create `src/lib/components/generation/PromptComposeModal.svelte`:

```svelte
<script lang="ts">
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js"; // confirm path

  let { onClose }: { onClose: () => void } = $props();

  let description = $state("");
  let length = $state<"short" | "medium" | "detailed">("medium");
  let includeArtists = $state(false);
  let result = $state("");
  let error = $state<string | null>(null);

  const isAnima = $derived(generation.modelFamily === "anima");

  async function generate() {
    if (!description.trim()) return;
    error = null;
    result = "";
    try {
      result = await promptAssistant.compose(description, generation.modelFamily, {
        length,
        include_artists: includeArtists,
      });
      if (!result.trim()) {
        error = locale.t("prompt_assistant.couldnt_compose");
      }
    } catch (e: any) {
      const msg = String(e);
      error = msg.includes("busy_generation")
        ? locale.t("prompt_assistant.busy_generation")
        : locale.t("prompt_assistant.error_generic");
    }
  }

  function replace() {
    generation.positivePrompt = result;
    generation.saveSettings?.();
    onClose();
  }

  function append() {
    const cur = generation.positivePrompt?.trim();
    generation.positivePrompt = cur ? `${cur}, ${result}` : result;
    generation.saveSettings?.();
    onClose();
  }
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
  onclick={onClose}
  role="presentation"
>
  <div
    class="max-h-[85vh] w-full max-w-xl overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
    onclick={(e) => e.stopPropagation()}
    role="dialog"
    aria-modal="true"
  >
    <div class="mb-3 flex items-center justify-between">
      <h2 class="text-lg font-semibold text-neutral-100">
        {locale.t("prompt_assistant.compose_title")}
      </h2>
      <button
        class="rounded-lg px-2 py-1 text-neutral-400 hover:bg-neutral-800"
        onclick={onClose}
        aria-label={locale.t("common.close")}
      >
        ✕
      </button>
    </div>

    <textarea
      bind:value={description}
      rows="4"
      placeholder={locale.t("prompt_assistant.describe_placeholder")}
      class="w-full rounded-lg border border-neutral-700 bg-neutral-800 p-2 text-sm text-neutral-100"
    ></textarea>

    <div class="mt-2 flex flex-wrap items-center gap-2 text-xs text-neutral-300">
      <span>{locale.t("prompt_assistant.length")}:</span>
      {#each ["short", "medium", "detailed"] as len}
        <button
          class="rounded border px-2 py-0.5 {length === len
            ? 'border-[var(--theme-accent-500)] text-neutral-100'
            : 'border-neutral-600 text-neutral-400'}"
          onclick={() => (length = len as typeof length)}
        >
          {locale.t(`prompt_assistant.length_${len}`)}
        </button>
      {/each}
      {#if isAnima}
        <label class="ml-2 flex items-center gap-1">
          <input type="checkbox" bind:checked={includeArtists} />
          {locale.t("prompt_assistant.include_artists")}
        </label>
      {/if}
    </div>

    {#if error}
      <div class="mt-2 rounded-lg border border-red-700 bg-red-900/30 px-3 py-2 text-sm text-red-300">
        {error}
      </div>
    {/if}

    <button
      class="mt-3 w-full rounded-lg bg-[var(--theme-accent-600)] px-4 py-2 font-medium text-black hover:bg-[var(--theme-accent-500)] disabled:opacity-50"
      disabled={promptAssistant.isGenerating || !description.trim()}
      onclick={generate}
    >
      {#if promptAssistant.isGenerating}
        <span class="inline-block animate-spin">⟳</span>
        {locale.t("prompt_assistant.generating")}
      {:else}
        {locale.t("prompt_assistant.generate")}
      {/if}
    </button>

    {#if result}
      <div class="mt-3">
        <div class="rounded-lg border border-neutral-700 bg-neutral-800/50 p-2 text-sm text-neutral-200">
          {result}
        </div>
        <div class="mt-2 flex gap-2">
          <button
            class="flex-1 rounded-lg border border-neutral-600 px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-800"
            onclick={replace}
          >
            {locale.t("prompt_assistant.replace")}
          </button>
          <button
            class="flex-1 rounded-lg border border-neutral-600 px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-800"
            onclick={append}
          >
            {locale.t("prompt_assistant.append")}
          </button>
        </div>
      </div>
    {/if}
  </div>
</div>
```

- [ ] **Step 2: Build gate** — `npm run build`.

---

### Task 17: i18n — all locales

**Files:**
- Modify: `src/lib/locales/en.ts` (source of truth)
- Modify: **every** other `src/lib/locales/*.ts`

Missing keys silently fall back to English, but per repo rules every key + `{placeholder}` added to `en.ts` MUST exist in all locale files.

- [ ] **Step 1: Add keys to `en.ts`**

Add (under appropriate nesting — match the file's structure for `settings.sections.*`, `settings.interrogator.*`, and a new top-level `prompt_assistant` group; also ensure `common.close` exists):

```typescript
  // settings.sections additions
  "settings.sections.prompt_assistant": "Prompt Assistant",
  // settings.prompt_assistant.*
  "settings.prompt_assistant.title": "Prompt Assistant",
  "settings.prompt_assistant.desc": "Install a local AI model to enhance or compose prompts.",
  "settings.prompt_assistant.installed_label": "Installed",
  "settings.prompt_assistant.none_installed": "No model installed yet.",
  "settings.prompt_assistant.manage_models": "Manage models",
  "settings.prompt_assistant.delete": "Delete",
  "settings.prompt_assistant.unload_now": "Unload now",
  "settings.prompt_assistant.idle_timeout": "Idle unload after",
  // prompt_assistant.*
  "prompt_assistant.enhance": "Enhance",
  "prompt_assistant.compose": "Compose",
  "prompt_assistant.enhance_tooltip": "Enhance the current prompt",
  "prompt_assistant.compose_tooltip": "Compose a prompt from a description",
  "prompt_assistant.undo": "Undo",
  "prompt_assistant.setup_title": "Set up Prompt Assistant",
  "prompt_assistant.recommended": "Recommended",
  "prompt_assistant.installed": "Installed",
  "prompt_assistant.detecting_hardware": "Detecting hardware…",
  "prompt_assistant.cpu_only": "CPU only — small models recommended",
  "prompt_assistant.nvfp4_supported": "Blackwell (NVFP4 supported)",
  "prompt_assistant.needs_blackwell": "Requires a Blackwell GPU",
  "prompt_assistant.needs_vram": "Needs ~{gb} GB VRAM",
  "prompt_assistant.downloading": "Downloading model…",
  "prompt_assistant.download_install": "Download & install",
  "prompt_assistant.use_model": "Use this model",
  "prompt_assistant.compose_title": "Compose a prompt",
  "prompt_assistant.describe_placeholder": "Describe what you want…",
  "prompt_assistant.length": "Length",
  "prompt_assistant.length_short": "Short",
  "prompt_assistant.length_medium": "Medium",
  "prompt_assistant.length_detailed": "Detailed",
  "prompt_assistant.include_artists": "Include artists",
  "prompt_assistant.generate": "Generate",
  "prompt_assistant.generating": "Generating…",
  "prompt_assistant.replace": "Replace",
  "prompt_assistant.append": "Append",
  "prompt_assistant.couldnt_enhance": "Couldn't enhance, try again.",
  "prompt_assistant.couldnt_compose": "Couldn't compose, try again.",
  "prompt_assistant.busy_generation": "Busy generating — try again after it finishes.",
  "prompt_assistant.no_model": "No prompt assistant model installed.",
  "prompt_assistant.error_generic": "Prompt assistant error, try again.",
```

(Match `en.ts`'s actual key style — if it uses nested objects rather than flat dotted keys, nest accordingly. Inspect the file first.)

- [ ] **Step 2: Propagate to every other locale file**

For each `src/lib/locales/*.ts` other than `en.ts`, add the same keys. Translate where you can; English fallback values are acceptable for languages you cannot translate (the rule is presence of the key, not translation quality). Do this for ALL locale files — enumerate them with a glob first.

Run to enumerate: list `src/lib/locales/*.ts`.

- [ ] **Step 3: Build gate** — `npm run build`. Confirm no missing-key console warnings during a quick `npm run tauri dev` smoke (optional). Commit:

```bash
git -c core.hooksPath=/dev/null add src/lib/components/generation/PromptInputs.svelte src/lib/components/generation/PromptComposeModal.svelte src/lib/locales/
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): enhance/compose buttons, compose modal, i18n"
```

---

## PHASE 5 — Polish & verification

### Task 18: Error states, idle UX, manual smoke, docs

**Files:**
- Modify: `src/lib/components/generation/PromptInputs.svelte` (stage indicator)
- Modify: `CHANGELOG.md` / `RELEASE_NOTES.md` (only if shipping a release — otherwise skip)

- [ ] **Step 1: Surface the loading stage near the buttons**

When `promptAssistant.stage === "loading_model"`, show a tiny inline hint (e.g. a `title`/tooltip or a small text `{locale.t("prompt_assistant.loading_model")}` next to the spinner). Add the `prompt_assistant.loading_model` key to all locales ("Loading model…"). Keep it lightweight — no layout shift.

- [ ] **Step 2: Verify error mapping**

Confirm `mapLlmError` handles: `busy_generation`, `no_model`, health-timeout (generic), OOM (generic). The backend returns i18n-key-like error strings (`prompt_assistant.busy_generation`, `prompt_assistant.no_model`) and free-text for the rest; the generic branch covers the rest.

- [ ] **Step 3: Full gates**

Run, expecting all green:
- `cargo fmt --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml prompt_assistant`
- `npm run build`

- [ ] **Step 4: Manual smoke (desktop)**

With `npm run tauri dev`:
1. Launch → confirm no errors; `promptAssistant.init()` populates hardware (check the setup modal hardware banner is correct, including Blackwell/NVFP4 wording on a 50-series, GB10 wording on DGX Spark if available).
2. Click ✨ Enhance with no model → setup modal opens, recommended model pre-selected. Download the tiny model (`dantaggen-l`) → progress bar advances → modal closes → enhance runs → positive prompt updated → ↩ Undo appears for ~10s → Undo restores.
3. Click ✍ Compose → describe → Generate → Replace/Append work.
4. Settings → Prompt Assistant: installed model shown, Unload now works, idle slider persists, Delete works.
5. Start a ComfyUI generation, then click Enhance → busy toast appears (generation guard).
6. Idle > timeout → server unloads (check logs for "idle … unloading").

- [ ] **Step 5: Final commit**

```bash
git -c core.hooksPath=/dev/null add -A
git -c core.hooksPath=/dev/null commit -m "feat(prompt-assistant): loading-stage UX and error polish"
```

---

## Self-Review (completed against the spec)

**1. Spec coverage**

| Spec section | Implemented in |
|---|---|
| Auto-select at launch + override | catalog::recommend_model_id (T3), store.init pre-select (T11), App.svelte init (T12), modal override (T13) |
| `llama-server` runtime, on-demand load, idle unload, generation guard | server.rs (T5), mod.rs ensure_running (T6), run_generation guard (T7) |
| Hardware detect incl. GB10/NVFP4 + compute-cap intent | hardware.rs BLACKWELL_MARKERS incl. gb10/dgx spark (T2) |
| Curated catalog, dimmed-with-reason | catalog.rs (T3), modal fits()/variantAvailable() dimming (T13) |
| Grounding + post-filter repair (tag-only + Anima @artist) | grounding.rs (T4) |
| Commands + events (`llm:download_progress`, `llm:stage`) | commands/prompt_assistant.rs (T7), webserver parity (T8) |
| Button placement (split toolbar, positive-only) | PromptInputs.svelte (T15) |
| Setup modal / Compose modal / Settings section | T13 / T16 / T14 |
| Store decoupled, api.ts wrappers, i18n all locales | T11 / T10 / T17 |
| Browser/web mode unchanged | webserver dispatch parity (T8) |
| Error handling (download fail, health timeout, OOM, busy, empty output) | T7 errors + T15 mapLlmError + T18 |

**2. Compute-capability fallback (spec mentions it):** v1 uses name-based Blackwell detection only (`detect_gpus()` returns name + VRAM, not compute capability). This is a deliberate, documented narrowing — the marker list covers known Blackwell SKUs incl. GB10/DGX Spark. If a future SKU's name isn't matched, add it to `BLACKWELL_MARKERS`. (Noted as a watch-item; not a blocker.)

**3. Type consistency:** Rust response structs are snake_case; TS interfaces mirror snake_case (`vram_mb`, `recommended_model_id`, `installed_models`). Variant keys use the `"nvfp4"` / `"gguf:<QUANT>"` convention consistently across `variant_matches` (Rust, T6), `defaultVariantKey`/`variantKey` (store + modal, T11/T13). Events named `llm:download_progress` / `llm:stage` consistently in Rust emit + store `ipcListen`.

**4. Known risks carried from the spec:** llama.cpp release asset names (`LLAMA_RELEASE` constant) and HF repo/file pins drift — both isolated to single constants/the catalog for easy updates. NVFP4 CUDA path is wired but the actual NVFP4 GGUF repos in the catalog are placeholders to be confirmed against real HF repos during T3 execution (verify each `repo`/`file` resolves before merging).

---

## Open verifications for the executor (resolve during implementation, do not skip)

- Confirm each catalog `repo`/`file` (T3) resolves on HuggingFace; fix any 404s before merging. The NVFP4 entry especially must point at a real GGUF/NVFP4 file or be dropped for v1.
- Confirm the pinned `LLAMA_RELEASE` tag (T5) has the named assets for win-vulkan / ubuntu / macos. If asset naming differs, update `assets_for`.
- Confirm `gallery.showToast`, `generation.saveSettings`, and the `locale` import path against the real components before relying on them (T13/T15/T16).
- Confirm `tokio` features (`fs`, `process`, `io-util`) and `zip` are enabled in `src-tauri/Cargo.toml`; add if `cargo check` complains (T5).
