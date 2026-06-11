# `<segment:...>` Automatic Regional Refinement — Design

**Issue:** [#288](https://github.com/Mooshieblob1/MooshieUI/issues/288)
**Date:** 2026-06-11
**Status:** Approved

## Goal

SwarmUI-style `<segment:...>` prompt syntax: the user names a region in plain text
(e.g. `<segment:eyes>`), the app detects that region in the generated image and
re-denoises it with a region-specific prompt — like face fix, but for arbitrary
targets, without manual inpainting.

Reference behavior: [SwarmUI Prompt Syntax docs](https://github.com/mcmonkeyprojects/SwarmUI/blob/master/docs/Features/Prompt%20Syntax.md).
SwarmUI's chain per segment: detect mask (CLIPSeg for text, YOLO for `yolo-` prefixed
targets) → threshold → grow/blur → crop to mask bounds → re-sample the crop with the
segment's prompt at `creativity` denoise → composite back. Segments run sequentially.

## Syntax

Parsed from the **positive prompt only**:

```
<segment:<target>[,<creativity>[,<threshold>]]>
```

- `target` — either free text (CLIPSeg detection, e.g. `eyes`) or
  `yolo-<model filename>` (YOLO detection using models in `models/ultralytics/`).
  A trailing `-<n>` on a YOLO target selects only match #n (1-based, sorted by
  confidence), e.g. `yolo-face_yolov8n.pt-1`.
- `creativity` — denoise strength for the re-sample. Default **0.6**. Valid (0, 1].
- `threshold` — detection threshold. Default **0.5** for CLIPSeg, **0.25** for YOLO.
  Valid (0, 1).

The segment's **refinement prompt** is written in one of two ways (both supported,
matching how `promptSchedule.ts` already accepts both SwarmUI `fromto` and MooshieUI
XML styles):

1. **Trailing form (SwarmUI-compatible):** everything after the tag until the next
   `<segment:` tag or end of prompt.
   `1girl, park <segment:face,0.6> freckles, smile <segment:eyes> green eyes`
2. **Closed form (MooshieUI style):** `<segment:eyes>green eyes</segment>`. Text
   after the closing tag returns to the base prompt (or to a preceding trailing-form
   segment if one is open — closed tags are self-contained; the simplest rule is:
   a closed-form block belongs to its own segment and surrounding text is parsed as
   if the block were removed).

An empty refinement prompt is valid — the region is re-denoised with the base prompt
context only (sharpening/detailing effect).

Invalid tags (bad numbers, empty target, creativity/threshold out of range) are left
in the prompt as literal text — same convention as scheduling/region tags.

## Architecture

Single ComfyUI job. One new custom node executes the whole
detect → crop → re-denoise → composite loop per segment (the proven
`MooshieFaceDetailer` pattern), appended by the Rust workflow builder.

```
KSampler → [upscale] → [facefix]
  → MooshieSegmentDetailer(eyes)   ← CLIPTextEncode("…context…, green eyes")
  → MooshieSegmentDetailer(hands)  ← CLIPTextEncode("…context…, detailed hands")
  → MooshieSaveImage
```

### Frontend

1. **Parser** — new `src/lib/utils/promptSegmentDetail.ts`:
   `parseSegmentDetailPrompt(raw) → { baseText, segments: DetailSegment[] }` where

   ```ts
   interface DetailSegment {
     target: string;      // "eyes" or "yolo-face_yolov8n.pt" or "yolo-….pt-1"
     prompt: string;      // refinement prompt (may be "")
     creativity: number;  // 0.6 default
     threshold: number;   // 0.5 clipseg / 0.25 yolo default
   }
   ```

   Tag regex registered in `promptInertRanges.ts` so segment blocks are skipped by
   LoRA-tag/clickable-range logic, and the backdrop overlay highlights them (new
   `TAG_COLORS` entry, distinct hue from scheduling gold).

2. **Store wiring** — generation store strips segment tags via the parser when
   building params; `toParams()` maps to snake_case `detail_segments`. Per-segment
   encode text is produced Rust-side from the shared regional context (see below),
   so the frontend only ships raw segment data. Order of appearance in the prompt
   is preserved.

3. **Detector model availability** — for `yolo-` targets, reuse the existing
   ensure-downloaded flow in `GenerateButton.svelte` (HuggingFace URL → `ultralytics`
   folder) for known detector filenames; unknown filenames are sent as-is and the
   node passes through with a logged warning if missing.

### Rust (`src-tauri`)

1. **Types** — `DetailSegment` struct in `comfyui/types.rs` (serde defaults), new
   `#[serde(default)] pub detail_segments: Vec<DetailSegment>` on `GenerationParams`.
2. **Template** — new `templates/segment_detail.rs`:
   `append_segment_chain(result, params, current_image, seed) → (String, u32)`.
   Called from `finish_workflow` in `templates/mod.rs` **after** facefix, before
   `MooshieSaveImage`. For each segment, in prompt order:
   - `CLIPTextEncode` with `merge_regional_encode_text(build_regional_context_prompt(params), segment.prompt)`
     — identical context rule to regional prompts.
   - `MooshieSegmentDetailer` node: image from previous stage, model/vae/negative
     from `WorkflowResult` sources, the segment's conditioning as positive,
     `denoise = creativity`, `threshold`, `detection = target`, sampler/scheduler/cfg
     from params, `steps = facefix_steps` (reuse), `guide_size = facefix_guide_size`,
     `mask_grow`/`mask_blur` at their defaults (16/8 — not user-tunable in v1),
     seed `seed + 3 + i`.
3. **Validation** — `validate_generation_params`: error when `detail_segments` is
   non-empty and style transfer is enabled (mirror of the facefix rule).

### Python (`comfyui/mooshie_nodes.py`)

New `MooshieSegmentDetailer` node (IMAGE in/out), sharing helpers with
`MooshieFaceDetailer` (feathered/blurred compositing, crop scaling):

- **Inputs:** `image, model, vae, positive, negative, detection (STRING), seed,
  steps, cfg, sampler_name, scheduler, denoise, guide_size, threshold,
  mask_grow (INT, default 16), mask_blur (INT, default 8)`.
- **Detection:**
  - `detection` starts with `yolo-` → strip prefix and optional `-<n>` index;
    load via ultralytics from the `ultralytics` model folder. Use segmentation
    masks when the model provides them, else boxes. Filter by confidence
    `threshold`; if an index was given keep only that match.
  - Otherwise → CLIPSeg: `transformers` (already a ComfyUI core dependency)
    `CLIPSegProcessor` + `CLIPSegForImageSegmentation` from
    `CIDAS/clipseg-rd64-refined`, `cache_dir` = `<models_dir>/clipseg/`
    (auto-download on first use; progress visible in the Rust log ring buffer).
    Mask = `sigmoid(logits)` upscaled to image size, binarized at `threshold`.
- **Refinement:** union mask → grow by `mask_grow` px, gaussian-blur by `mask_blur`
  → bounding box of the mask (padded, clamped, min 8px) → crop → scale to
  `guide_size` (multiple of 8) → VAE encode → sample at `denoise` with the
  segment conditioning → decode → scale back → composite using the **blurred
  detected mask** (not a rectangular feather) so irregular shapes blend cleanly.
- **No detections / model file missing / empty mask after threshold:** log and
  return the input image unchanged (facefix behavior).
- CLIPSeg model and YOLO models are released/garbage-collected after the node runs
  (no persistent VRAM residency beyond what facefix already does).

### i18n / docs

- Tooltip/help strings for the new syntax in `en.ts` and **all** other locale files.
- README / prompt-syntax help UI section documenting the tag, params, and defaults.

## Error handling summary

| Failure | Behavior |
|---|---|
| Invalid tag values | Tag left as literal prompt text (parser convention) |
| Style transfer + segments | Rust validation error before submit |
| YOLO model file missing | Node logs warning, passes image through |
| No region detected | Node logs info, passes image through |
| CLIPSeg download fails | ComfyUI job error surfaces through existing error path |

## Testing

No test framework exists (per CLAUDE.md). Validation:

- `npm run build` + `cargo check` gates.
- Manual: txt2img with `<segment:eyes> green eyes` (CLIPSeg path),
  `<segment:yolo-Anzhc Face seg 640 v4 y11n.pt> detailed face` (YOLO path),
  two segments in one prompt, closed-form syntax, invalid tag left literal,
  segments combined with upscale + facefix, browser mode parity.

## Out of scope (v1 — future work)

- Pipe grouping `<segment:face|hair>`
- Per-segment LoRA application
- YOLO class filters (`:0,apple:`)
- Segment tags in the negative prompt
- Save-segment-mask preview output
- Segment target resolution override (uses `guide_size` instead)
