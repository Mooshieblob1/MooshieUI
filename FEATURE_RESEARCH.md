# MooshieUI — Novel Feature Research Shortlist

**Constraints**: Research only. Mixed audience. 12 GB+ VRAM baseline. Local/offline-first. Desktop (Tauri) advantages welcome.

**Exclusion list** (previously rejected concept families): aesthetic scoring filters, denoise-step live preview strips, generic analytics dashboards, CLIP semantic image search, character identity vaults, IP-Adapter moodboards, auto-negative prompt generators, semantic AI brush canvas, prompt divergence trees.

---

## Ecosystem Audit Summary

Features surveyed across A1111, ComfyUI, SwarmUI, Fooocus, InvokeAI:

| Capability | A1111 | ComfyUI | SwarmUI | Fooocus | InvokeAI | MooshieUI |
|---|---|---|---|---|---|---|
| X/Y/Z grid (batch param sweep) | ✅ | via nodes | ✅ (Grid Gen) | — | — | — |
| Prompt editing `[from:to:step]` | ✅ | via nodes | ✅ (syntax) | — | — | — |
| Regional prompting | ext | nodes | ✅ (text syntax) | — | — | — |
| SAM2 segmenter in editor | — | nodes | ✅ | — | — | — |
| Prompt array processing | — | — | — | ✅ `[[a,b,c]]` | — | — |
| GPT-2 prompt enhancement | — | — | — | ✅ | — | — |
| Unified large canvas | — | — | editor | — | ✅ | — |
| Layout customization | — | — | ✅ | — | — | — |
| User workspace profiles | — | — | ✅ | — | ✅ | — |
| Dynamic VRAM allocator | — | ✅ (aimdo) | — | — | — | — |
| Model starring/favorites | — | — | ✅ | — | — | — |
| CivitAI bulk metadata scan | — | — | ✅ | — | — | — |
| ONNX tagger (interrogator) | — | nodes | ext | ✅ (Describe) | — | ✅ |
| ControlNet presets | ext | nodes | auto-detect | — | — | ✅ |
| Per-model param memory | — | — | — | — | — | — |
| Generation macros/actions | — | — | — | — | — | — |
| Desktop system tray gen | — | — | — | — | — | — |
| CLIP tokenization debugger | ext (separate) | — | — | — | — | — |

"—" = not present. "ext" = third-party extension exists.

---

## Shortlist (10 Features)

### Phase A — Quick Wins (low infra, high signal)

#### 1. Per-Model Parameter Memory

**Problem**: Every time you switch checkpoints, you must re-dial CFG, sampler, scheduler, resolution, and step count. Different models have wildly different sweet spots (e.g., Euler a / CFG 7 / 20 steps for one model, DPM++ 2M Karras / CFG 3.5 / 28 steps for another).

**Why nothing else does this**: All existing UIs maintain a single global parameter state. A1111, ComfyUI, SwarmUI, Fooocus — none save per-model parameter profiles. Users rely on notes, memory, or style presets that don't capture the full parameter surface.

**Minimum implementation path**:
- Extend `GenerationStore` persistence: on model switch, auto-save current param snapshot keyed to the outgoing model hash/name. On model load, if a saved profile exists, restore it.
- Storage: `@tauri-apps/plugin-store` JSON, keyed by `checkpoint_name → {cfg, steps, sampler, scheduler, width, height, loras[]}`.
- UI: subtle indicator showing "Restored settings for [model]" toast. Optional per-model "pin" to prevent auto-restore.

**Dependencies**: None beyond existing store infrastructure.
**12 GB feasibility**: Zero VRAM impact — pure UI/persistence.
**Risk**: LOW. Main edge case: LoRA references that don't exist for the new model.
**Novelty score**: ★★★★★ — No tool does this.

---

#### 2. CLIP Tokenization Live Debugger

**Problem**: Users don't understand why their 200-word prompt produces bad results. They don't know about CLIP's 75-token chunk boundaries, that commas consume tokens, that words get split into sub-tokens, or that content beyond chunk 1 has diminishing influence.

**Why nothing else does this**: A1111 has a separate "Tokenizer" extension (not inline). No tool shows tokenization in real-time as you type, integrated into the prompt editor itself.

**Minimum implementation path**:
- Add a Rust command `tokenize_prompt` that calls ComfyUI's `/tokenize` endpoint (or bundle a local CLIP tokenizer — Rust crate `tokenizers` by HuggingFace is 0-VRAM).
- Frontend: below the prompt textarea, render a colored token strip. Each token is a pill/chip. Color-code by chunk (chunk 1 = bright, chunk 2 = dimmer, etc.). Show `N/75` counter per chunk.
- Update on debounced keystrokes (200ms delay).

**Dependencies**: ComfyUI tokenize endpoint or HuggingFace `tokenizers` crate.
**12 GB feasibility**: Zero VRAM — tokenizer is CPU-only and instant.
**Risk**: LOW. Main risk: ComfyUI may not expose a tokenize endpoint for all model types (Flux uses T5, not CLIP). Fall back to local tokenizer.
**Novelty score**: ★★★★☆ — Technical extension exists for A1111 but never inline/real-time in any UI.

---

#### 3. Prompt Diff in History

**Problem**: MooshieUI stores 100 prompt history entries per mode, but they're displayed as a flat list. When iterating on a prompt, you can't see what you actually changed between version N and N+1.

**Why nothing else does this**: No tool shows prompt diffs. History is always flat text recall.

**Minimum implementation path**:
- When displaying history, compute a simple word-level diff between adjacent entries.
- Render additions in green highlight, deletions in red strikethrough, directly in the history dropdown or panel.
- No backend changes needed — pure frontend string diff.

**Dependencies**: None. A simple diff algorithm (Myers or patience diff) in ~50 lines of TS.
**12 GB feasibility**: Pure UI.
**Risk**: VERY LOW.
**Novelty score**: ★★★★☆ — Trivial to build, zero prior art, immediately useful.

---

#### 4. Inline Prompt Variant Expansion

**Problem**: You want to try "a {red|blue|green} flower" and generate all 3 variants. Fooocus has `[[red, green, blue]]` array processing and A1111 has `|` prompt matrix, but both use hidden syntax that users must discover and memorize.

**Why nothing else does this well**: The syntax exists elsewhere but is never surfaced as a visual UI element. No tool lets you click on a word and say "add alternatives" as a dropdown/chip picker that visually shows the expansion.

**Minimum implementation path**:
- In the prompt editor, detect `{a|b|c}` syntax. Render these segments as interactive chips/pills inline.
- Click a chip to edit alternatives. Add a "+" button to extend. Show "×3 variants" badge.
- On Generate, expand to N prompts and queue them as a batch.
- Backend: modify `generate` command to accept `Vec<GenerationParams>` (or loop on the frontend).

**Dependencies**: Minor workflow command change to handle batch.
**12 GB feasibility**: Sequential generation, same VRAM as single gen.
**Risk**: LOW-MEDIUM. Prompt parsing complexity.
**Novelty score**: ★★★☆☆ — Existing syntax in other tools, but visual UI is new.

---

### Phase B — Medium Lift (clear differentiation)

#### 5. Parameter A/B Comparison Mode

**Problem**: "Is CFG 5 or CFG 7 better for this prompt?" Currently you generate one, change the value, generate again, scroll back and forth in the gallery trying to compare. Repeat for sampler, scheduler, steps, etc.

**Why nothing else does this**: X/Y/Z grids (A1111, SwarmUI) generate a full matrix batch and dump results into a grid image. That's a batch tool, not an interactive comparison tool. No tool offers a two-pane "generate both, swipe to compare, pick winner" flow.

**Minimum implementation path**:
- New UI mode: split output area into two panes (A | B). A parameter diff panel lets you select exactly one parameter to vary between A and B.
- Generate button queues A then B sequentially. Both results appear side-by-side.
- Swipe/toggle overlay to compare. "Pick A" / "Pick B" button saves the winning param and optionally applies it to the main settings.
- Track wins over time → surface as "your preferred CFG range for this model" (ties into #1 Per-Model Memory).

**Dependencies**: UI layout work. No backend changes (two sequential `generate` calls).
**12 GB feasibility**: Two sequential generations — same VRAM as one at a time.
**Risk**: MEDIUM. UI/UX design is the hard part. Must not feel clunky.
**Novelty score**: ★★★★★ — No image gen tool has interactive A/B mode with winner tracking.

---

#### 6. System Tray Quick Generate

**Problem**: To generate an image, you must open MooshieUI, navigate to the generation tab, type a prompt, and hit generate. For frequent users, this is friction. You see something inspiring and want to immediately prompt-to-image.

**Why nothing else does this**: Every existing tool is web-based (A1111, ComfyUI, Fooocus, SwarmUI, InvokeAI). They fundamentally cannot integrate with the OS. MooshieUI is a Tauri desktop app — native OS integration is its unique superpower.

**Minimum implementation path**:
- Tauri: Add `tauri-plugin-system-tray` with a context menu: "Quick Generate from Clipboard", "Open MooshieUI".
- "Quick Generate" reads clipboard text as prompt, uses last-used params, queues generation via existing `generate` command.
- Show a native OS notification with thumbnail when result is ready. Click notification → opens gallery with the new image.
- Optional: register a global hotkey (e.g., Ctrl+Shift+G) to open a minimal floating prompt-entry window.

**Dependencies**: `tauri-plugin-system-tray`, `tauri-plugin-notification`, `tauri-plugin-global-shortcut`. All are official Tauri v2 plugins.
**12 GB feasibility**: Same generation, different entry point.
**Risk**: MEDIUM. Platform quirks (Linux tray support varies), focus management.
**Novelty score**: ★★★★★ — Completely unique to desktop. Zero prior art in any AI image gen tool.

---

#### 7. Interactive Seed Neighborhood Explorer

**Problem**: Finding a good seed is trial and error. You generate batches of 4, scan them, regenerate. No systematic way to "zoom in" on a promising seed and explore what's nearby in seed space.

**Why nothing else does this**: A1111 has a "Variation" subseed feature (one slider), but no tool offers a visual grid that lets you click-to-zoom-into a seed neighborhood. The concept of seed-space browsing doesn't exist.

**Minimum implementation path**:
- Generate an initial 3×3 grid of 9 random seeds at reduced resolution (e.g., 256×256) for fast preview.
- User clicks a thumbnail they like → generate a new 3×3 grid centered on that seed's neighborhood (using subseed blending at various low strengths, or seed ± small offsets).
- Repeat drill-down until satisfied. Final click generates at full resolution with selected seed.
- Backend: batch generation at reduced resolution. Could use TAESD for even faster previews.

**Dependencies**: Batch generation support (or rapid sequential), possibly TAESD for fast decode. ComfyUI supports resolution changes per generation.
**12 GB feasibility**: Small previews at 256px use ~2-3 GB VRAM per gen. Fits comfortably.
**Risk**: MEDIUM-HIGH. Speed is critical — if each drill-down takes 30+ seconds, the UX breaks. Needs fast preview pipeline.
**Novelty score**: ★★★★★ — No tool offers visual seed-space browsing.

---

#### 8. Prompt-to-Prompt Interpolation Slider

**Problem**: You have two good prompts but want something in between. Or you want to see how a scene gradually transforms from one concept to another. Currently there's no way to explore the space between two prompts.

**Why nothing else does this as a UI feature**: `stable-diffusion-videos` (4.7k stars) does latent interpolation as a Python library/CLI. A1111 has `[from:to:step]` prompt editing that blends during diffusion. But no tool offers an interactive slider that pre-computes interpolation frames and lets you scrub between two prompts visually.

**Minimum implementation path**:
- UI: Two prompt inputs + a horizontal slider. User enters prompt A and prompt B.
- Backend: Generate N images (e.g., 5-7) at interpolation weights [0.0, 0.17, 0.33, 0.5, 0.67, 0.83, 1.0] using spherical interpolation of CLIP/T5 conditioning tensors.
- Custom ComfyUI node needed: `ConditioningInterpolate` that slerps between two conditioning outputs.
- Slider scrubs through pre-generated frames. User locks desired position and saves.

**Dependencies**: Custom ComfyUI node for conditioning interpolation. Template modification.
**12 GB feasibility**: N sequential generations at normal resolution. Same VRAM as single gen.
**Risk**: MEDIUM. Custom node development. Quality varies — some interpolation points produce garbage.
**Novelty score**: ★★★★☆ — Concept exists in code libraries, but UI integration is new.

---

### Phase C — Ambitious Bets

#### 9. Generation Macros (Photoshop Actions for AI Gen)

**Problem**: Common workflows are multi-step: txt2img → pick best → upscale → face fix → save to specific board. Each step requires manual intervention. Power users repeat these sequences dozens of times daily.

**Why nothing else does this**: ComfyUI has node graphs which can chain operations, but that's a developer tool with a steep learning curve. No simplified UI offers "record a sequence of generation steps and replay it." This is Photoshop Actions for AI image generation.

**Minimum implementation path**:
- Define a macro as a JSON sequence of steps: `[{action: "txt2img", params: {...}}, {action: "pick_by", criteria: "first"}, {action: "upscale", params: {...}}, {action: "face_fix"}, {action: "save_to_board", board: "Finals"}]`.
- UI: "Record Macro" button that captures your manual actions as you perform them. "Stop Recording" saves the macro. "Play Macro" replays the sequence, auto-selecting intermediates by configurable criteria (first, random, or pause-for-manual-pick).
- Backend: Orchestration layer in Rust that sequences Tauri commands and waits for results.

**Dependencies**: Significant orchestration layer. Needs robust error handling for each step.
**12 GB feasibility**: Same as manual workflow — no extra VRAM.
**Risk**: HIGH. State management between steps is complex. "Pause for pick" requires UI→macro coordination. Likely the hardest feature on this list.
**Novelty score**: ★★★★★ — Zero prior art. ComfyUI node graphs are the closest, but they're developer tools, not user-facing macros.

---

#### 10. Generation Lineage Graph

**Problem**: After an hour of iterating — img2img from this, inpaint that, upscale this result — you can't remember which image came from which. You want to backtrack to "the version before I inpainted the background" but the gallery is a flat grid with no relationship awareness.

**Why nothing else does this**: Every tool stores images flat. PNG metadata records the parameters used, but not the *parent-child lineage* between generations. No tool tracks "image B was img2img'd from image A" or "image C was image A with the face inpainted."

**Minimum implementation path**:
- Extend image metadata to include `parent_id` (the source image used for img2img/inpainting/upscaling).
- Store lineage relationships in the gallery data model.
- UI: A "Lineage" view that shows a tree/graph visualization for a selected image. Click any ancestor to view it. "Fork from here" to branch from any point in history.
- Backend: Modify gallery store to track `{image_id, parent_id, operation_type, timestamp}`.

**Dependencies**: Gallery store refactor. Graph visualization component (e.g., D3.js or a simple tree renderer).
**12 GB feasibility**: Pure data/UI.
**Risk**: HIGH. Design complexity — graph visualization must be intuitive, not overwhelming. Storage overhead for relationship tracking. Migration path for existing galleries.
**Novelty score**: ★★★★★ — No tool anywhere tracks generation lineage as a navigable graph.

---

## Ranking Summary

| # | Feature | Phase | Novelty | Impact | VRAM | Risk |
|---|---|---|---|---|---|---|
| 1 | Per-Model Parameter Memory | A | ★★★★★ | HIGH | 0 | LOW |
| 2 | CLIP Tokenization Debugger | A | ★★★★☆ | MED | 0 | LOW |
| 3 | Prompt Diff in History | A | ★★★★☆ | MED | 0 | V.LOW |
| 4 | Inline Prompt Variant Expansion | A | ★★★☆☆ | MED | 0 | LOW |
| 5 | Parameter A/B Comparison | B | ★★★★★ | HIGH | same | MED |
| 6 | System Tray Quick Generate | B | ★★★★★ | MED | same | MED |
| 7 | Seed Neighborhood Explorer | B | ★★★★★ | HIGH | low | MED-HI |
| 8 | Prompt Interpolation Slider | B | ★★★★☆ | MED-HI | same | MED |
| 9 | Generation Macros | C | ★★★★★ | HIGH | 0 | HIGH |
| 10 | Generation Lineage Graph | C | ★★★★★ | MED-HI | 0 | HIGH |

**Recommended first picks** (highest impact/risk ratio): #1 Per-Model Parameter Memory, #3 Prompt Diff in History, #5 A/B Comparison.

---

## Rejection Appendix

Previously rejected concept families and why they are excluded from this shortlist:

| Concept | Why Rejected |
|---|---|
| Aesthetic scoring / quality filters | "Absolute stupidity" — subjective, unreliable with local models, users can judge themselves |
| Denoise step live preview strip | "Just worse than cancelling early" — adds latency for minimal benefit |
| Analytics dashboard | "Funny but mostly useless" — users don't need charts about their generation habits |
| CLIP-based semantic image search | Limited utility with local embeddings, gallery is visual enough |
| Character identity vaults (persistent faces) | "Not good enough with current local tech" — identity preservation is unreliable |
| IP-Adapter style moodboards | "Just use artist tags, IPAdapter is not good enough" — quality ceiling too low |
| Auto-negative prompt generator | "Doesn't make any sense" — experienced users write their own, beginners use quality tags |
| Semantic AI brush (brush that generates) | Conflates editing and generation in confusing ways |
| Prompt divergence/branching trees | Complex UI gimmick that doesn't improve generation quality |
