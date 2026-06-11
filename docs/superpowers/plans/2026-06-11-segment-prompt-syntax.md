# `<segment:...>` Prompt Syntax Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SwarmUI-style `<segment:eyes>` prompt tags that auto-detect a region (CLIPSeg text or YOLO model) in the generated image and re-denoise it with a region-specific prompt — issue #288.

**Architecture:** Frontend parses segment tags out of the positive prompt into structured `detail_segments`; the Rust workflow builder appends one `MooshieSegmentDetailer` custom node per segment (each with its own `CLIPTextEncode`) after upscale/facefix; the Python node does detect → crop → re-denoise → mask-composite in one ComfyUI job.

**Tech Stack:** Svelte 5 / TypeScript frontend, Rust (Tauri) workflow templates, Python ComfyUI custom node (torch, ultralytics, transformers CLIPSeg).

**Spec:** `docs/superpowers/specs/2026-06-11-segment-prompt-syntax-design.md`

**Validation note (per CLAUDE.md):** This repo has **no test framework** — no vitest/jest, no `#[test]` for new code. Each task's verification is `npm run build` (frontend) and/or `cargo check --manifest-path src-tauri/Cargo.toml` (Rust), plus `cargo fmt`/`cargo clippy` before the final commit. The Python node cannot be unit-tested here; it is validated by the manual checklist in Task 8.

**Git note (per CLAUDE.md):** The pre-commit hook is bash and hangs in PowerShell — prefix every git command with `git -c core.hooksPath=/dev/null`. Work happens on the existing branch `feat/segment-prompt-syntax`.

---

### Task 1: TS type + segment tag parser + inert ranges

**Files:**
- Modify: `src/lib/types/index.ts` (next to `PromptSegment`, ~line 50–75)
- Create: `src/lib/utils/promptSegmentDetail.ts`
- Modify: `src/lib/utils/promptInertRanges.ts`

- [ ] **Step 1.1: Add the `DetailSegment` type and params field**

In `src/lib/types/index.ts`, directly after the `PromptSegment` interface, add:

```ts
/** A <segment:...> auto-refinement region parsed from the positive prompt. */
export interface DetailSegment {
  /** Detection target: free text (CLIPSeg) or "yolo-<model filename>[-<match index>]". */
  target: string;
  /** Refinement prompt for the detected region (may be empty). */
  prompt: string;
  /** Denoise strength for the re-sample, (0, 1]. */
  creativity: number;
  /** Detection threshold, (0, 1). */
  threshold: number;
}
```

In the `GenerationParams` interface, after `negative_segments: PromptSegment[];`, add:

```ts
  detail_segments: DetailSegment[];
```

- [ ] **Step 1.2: Create the parser module**

Create `src/lib/utils/promptSegmentDetail.ts` with this exact content:

```ts
import { SYNTAX_ANGLE_LOOKBEHIND } from "./promptSyntaxEscape.ts";
import type { DetailSegment } from "../types/index.js";

/**
 * SwarmUI-style <segment:...> auto-refinement tags.
 *
 * Opening tag: <segment:<target>[,<creativity>[,<threshold>]]>
 *   - target: free text (CLIPSeg detection) or "yolo-<model filename>" with an
 *     optional trailing "-<n>" match index (e.g. "yolo-face_yolov8n.pt-1").
 *   - creativity: re-sample denoise, default 0.6, valid (0, 1].
 *   - threshold: detection threshold, default 0.5 (CLIPSeg) / 0.25 (YOLO), valid (0, 1).
 *
 * The refinement prompt is either everything after the tag until the next
 * <segment: tag or end of prompt (SwarmUI trailing form), or the text up to a
 * closing </segment> (MooshieUI closed form).
 */
export const PROMPT_SEGMENT_OPEN_REGEX = new RegExp(
  `${SYNTAX_ANGLE_LOOKBEHIND}<segment:([^>]+)>`,
  "gi",
);

const SEGMENT_CLOSE = "</segment>";

export const DEFAULT_SEGMENT_CREATIVITY = 0.6;
export const DEFAULT_CLIPSEG_THRESHOLD = 0.5;
export const DEFAULT_YOLO_THRESHOLD = 0.25;

export interface ParsedSegmentDetailPrompt {
  baseText: string;
  segments: DetailSegment[];
}

interface ParsedSpec {
  target: string;
  creativity: number;
  threshold: number;
}

/** Parse the inside of the opening tag. Returns null when invalid (tag stays literal). */
function parseSegmentSpec(spec: string): ParsedSpec | null {
  const parts = spec.split(",").map((p) => p.trim());
  // Pop up to two trailing numeric parts: creativity, then threshold.
  const nums: number[] = [];
  while (parts.length > 1 && nums.length < 2) {
    const last = parts[parts.length - 1];
    if (!/^\d*\.?\d+$/.test(last)) break;
    nums.unshift(parseFloat(last));
    parts.pop();
  }
  const target = parts.join(",").trim();
  if (!target) return null;
  const isYolo = target.toLowerCase().startsWith("yolo-");
  const creativity = nums.length >= 1 ? nums[0] : DEFAULT_SEGMENT_CREATIVITY;
  const threshold =
    nums.length >= 2
      ? nums[1]
      : isYolo
        ? DEFAULT_YOLO_THRESHOLD
        : DEFAULT_CLIPSEG_THRESHOLD;
  if (!(creativity > 0 && creativity <= 1)) return null;
  if (!(threshold > 0 && threshold < 1)) return null;
  return { target, creativity, threshold };
}

/**
 * Extract <segment:...> tags from a prompt. Tag text and refinement prompts are
 * removed from baseText; invalid tags are left as literal text (parser convention
 * shared with scheduling/region tags).
 */
export function parseSegmentDetailPrompt(raw: string): ParsedSegmentDetailPrompt {
  if (!raw || !raw.toLowerCase().includes("<segment:")) {
    return { baseText: raw ?? "", segments: [] };
  }

  const opens: Array<{ start: number; end: number; spec: string }> = [];
  PROMPT_SEGMENT_OPEN_REGEX.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = PROMPT_SEGMENT_OPEN_REGEX.exec(raw)) !== null) {
    opens.push({ start: match.index, end: match.index + match[0].length, spec: match[1] });
  }

  const segments: DetailSegment[] = [];
  let baseText = "";
  let cursor = 0;

  for (let i = 0; i < opens.length; i++) {
    const open = opens[i];
    baseText += raw.slice(cursor, open.start);

    const spec = parseSegmentSpec(open.spec);
    if (!spec) {
      // Invalid tag stays literal; the text after it stays in the base prompt.
      baseText += raw.slice(open.start, open.end);
      cursor = open.end;
      continue;
    }

    const regionEnd = i + 1 < opens.length ? opens[i + 1].start : raw.length;
    const between = raw.slice(open.end, regionEnd);
    const closeIdx = between.toLowerCase().indexOf(SEGMENT_CLOSE);

    if (closeIdx >= 0) {
      // Closed form: prompt up to </segment>; text after the closer returns to base.
      segments.push({ ...spec, prompt: between.slice(0, closeIdx).trim() });
      cursor = open.end + closeIdx + SEGMENT_CLOSE.length;
    } else {
      // Trailing form: prompt runs to the next segment tag or end of prompt.
      segments.push({ ...spec, prompt: between.trim() });
      cursor = regionEnd;
    }
  }

  baseText += raw.slice(cursor);
  baseText = baseText
    .replace(/,\s*,/g, ",")
    .replace(/^\s*,\s*/, "")
    .replace(/\s*,\s*$/, "")
    .trim();

  return { baseText, segments };
}

/**
 * For a "yolo-..." target, return the detector model filename (match-index
 * suffix stripped). Returns null for CLIPSeg (non-yolo) targets.
 */
export function yoloTargetFilename(target: string): string | null {
  if (!target.toLowerCase().startsWith("yolo-")) return null;
  let name = target.slice("yolo-".length).trim();
  const indexed = name.match(/^(.+\.(?:pt|onnx))-\d+$/i);
  if (indexed) name = indexed[1];
  return name || null;
}

/** Cheap check used to skip parsing on every keystroke. */
export function hasSegmentDetailTags(raw: string): boolean {
  if (!raw || !raw.toLowerCase().includes("<segment:")) return false;
  PROMPT_SEGMENT_OPEN_REGEX.lastIndex = 0;
  return PROMPT_SEGMENT_OPEN_REGEX.test(raw);
}
```

- [ ] **Step 1.3: Register segment tags as inert ranges**

In `src/lib/utils/promptInertRanges.ts`, after `PROMPT_REGION_TAG_REGEX` (line 27–30), add:

```ts
/** <segment:...> opening tags and </segment> closers. Only the tags are inert —
 * the refinement prompt text between them stays interactive (autocomplete,
 * clickable tags). */
export const PROMPT_SEGMENT_TAG_REGEX = new RegExp(
  `${SYNTAX_ANGLE_LOOKBEHIND}<segment:[^>]+>|<\\/segment>`,
  "gi",
);
```

In `getPromptInertRanges`, after the `<region:` block (line 74–76), add:

```ts
  const lower = raw.toLowerCase();
  if (lower.includes("<segment:") || lower.includes("</segment>")) {
    ranges.push(...collectRegexRanges(raw, PROMPT_SEGMENT_TAG_REGEX));
  }
```

- [ ] **Step 1.4: Verify build**

Run: `npm run build`
Expected: completes with no TypeScript errors. (`detail_segments` is a required field but `toParams` is updated in Task 3 — if the build fails on the missing field in `generation.svelte.ts`, temporarily note it and confirm Task 3 fixes it; alternatively run Tasks 1–3 before the first build. To keep every commit green, **declare the field optional-free but commit Tasks 1+2+3 only after each one's own build passes** — the field addition is the last edit of Step 1.1; if `npm run build` fails ONLY with "Property 'detail_segments' is missing" in `generation.svelte.ts`/`regionalInpaintChain.ts`, proceed to Task 3 and run the build at the end of Task 3 before committing Task 1+2+3 together as one commit.)

- [ ] **Step 1.5: Commit (or hold until Task 3 if the build requires the store wiring)**

```pwsh
git -c core.hooksPath=/dev/null add src/lib/types/index.ts src/lib/utils/promptSegmentDetail.ts src/lib/utils/promptInertRanges.ts
git -c core.hooksPath=/dev/null commit -m "feat: parse <segment:...> detail tags from prompts"
```

---

### Task 2: Prompt highlighting for segment tags

**Files:**
- Modify: `src/lib/utils/promptSchedule.ts`

- [ ] **Step 2.1: Import the segment regex**

In `src/lib/utils/promptSchedule.ts`, extend the existing import from `./promptInertRanges.js` (lines 2–6) to include `PROMPT_SEGMENT_TAG_REGEX`:

```ts
import {
  PROMPT_PRESET_TOKEN_REGEX,
  PROMPT_REGION_TAG_REGEX,
  PROMPT_SCHEDULE_REGEX,
  PROMPT_SEGMENT_TAG_REGEX,
} from "./promptInertRanges.js";
```

- [ ] **Step 2.2: Add a segment-tag-aware text renderer**

Below `renderPresetSegment` (after ~line 276), add:

```ts
/** Teal pill for <segment:...> / </segment> tags — distinct from scheduling gold. */
const SEGMENT_TAG_COLOR = {
  bg: "rgba(45, 212, 191, 0.12)",
  border: "rgba(45, 212, 191, 0.45)",
  glow: "0 0 10px rgba(45, 212, 191, 0.25), 0 0 4px rgba(45, 212, 191, 0.12)",
};

/**
 * Highlight <segment:...> tags within a plain-text run, delegating the
 * remaining text to renderPresetSegment for @preset highlighting.
 */
function renderSegmentAwareText(
  text: string,
  knownPresetSlugs?: ReadonlySet<string>,
): string {
  if (!text) return "";
  const lower = text.toLowerCase();
  if (!lower.includes("<segment:") && !lower.includes("</segment>")) {
    return renderPresetSegment(text, knownPresetSlugs);
  }
  let html = "";
  let lastIndex = 0;
  PROMPT_SEGMENT_TAG_REGEX.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = PROMPT_SEGMENT_TAG_REGEX.exec(text)) !== null) {
    html += renderPresetSegment(text.slice(lastIndex, match.index), knownPresetSlugs);
    lastIndex = match.index + match[0].length;
    html += `<span style="display:inline;color:transparent;background:${SEGMENT_TAG_COLOR.bg};border:1px solid ${SEGMENT_TAG_COLOR.border};border-radius:4px;box-shadow:${SEGMENT_TAG_COLOR.glow};padding:1px 3px;margin:0 1px;">`;
    html += escapeHtml(match[0]);
    html += `</span>`;
  }
  html += renderPresetSegment(text.slice(lastIndex), knownPresetSlugs);
  return html;
}
```

- [ ] **Step 2.3: Route plain-text runs through the new renderer**

In `renderHighlightedPrompt`, replace both `renderPresetSegment(...)` call sites
(currently `html += renderPresetSegment(raw.slice(lastIndex, matchStart), knownPresetSlugs);` inside the loop and `html += renderPresetSegment(raw.slice(lastIndex), knownPresetSlugs);` after it) with:

```ts
    html += renderSegmentAwareText(raw.slice(lastIndex, matchStart), knownPresetSlugs);
```

and

```ts
  html += renderSegmentAwareText(raw.slice(lastIndex), knownPresetSlugs);
```

- [ ] **Step 2.4: Verify build**

Run: `npm run build`
Expected: success (or only the known `detail_segments` missing-field error resolved by Task 3 — see Step 1.4).

- [ ] **Step 2.5: Commit**

```pwsh
git -c core.hooksPath=/dev/null add src/lib/utils/promptSchedule.ts
git -c core.hooksPath=/dev/null commit -m "feat: highlight <segment> tags in prompt overlay"
```

---

### Task 3: Store wiring (`toParams`) + regional chain gating

**Files:**
- Modify: `src/lib/stores/generation.svelte.ts` (imports ~line 6, `toParams` ~lines 1562–1660)
- Modify: `src/lib/utils/regionalInpaintChain.ts` (~lines 72–134)

- [ ] **Step 3.1: Parse segment tags in `toParams`**

In `src/lib/stores/generation.svelte.ts`, add to the utils imports (next to `parseRegionalPrompt`/`parseScheduledPrompt`):

```ts
import { parseSegmentDetailPrompt } from "../utils/promptSegmentDetail.js";
```

In `toParams`, directly after `positivePrompt = parsedRegions.baseText;` (line 1568), insert:

```ts
    // Parse <segment:...> auto-refinement tags (SwarmUI-style) before schedule
    // parsing. Keep the tagged text around so gallery metadata round-trips.
    const promptWithSegmentTags = positivePrompt;
    const parsedSegmentDetails = parseSegmentDetailPrompt(positivePrompt);
    positivePrompt = parsedSegmentDetails.baseText;
```

- [ ] **Step 3.2: Ship `detail_segments` and keep tags in metadata**

In the `params: GenerationParams = { ... }` literal:

After `negative_segments: ...` (ends line 1653), add:

```ts
      detail_segments: parsedSegmentDetails.segments.map((s) => ({
        target: s.target,
        prompt: translateNaiWeightSyntax(s.prompt),
        creativity: s.creativity,
        threshold: s.threshold,
      })),
```

Change `raw_positive_prompt: translateNaiWeightSyntax(positivePrompt),` (line 1654) to:

```ts
      raw_positive_prompt: translateNaiWeightSyntax(promptWithSegmentTags),
```

(`promptWithSegmentTags` still contains scheduling tags too — it is captured before `parseScheduledPrompt` runs, identical to the previous value of `positivePrompt` at that line, plus the segment tags.)

- [ ] **Step 3.3: Run segments only on the final step of the regional inpaint chain**

In `src/lib/utils/regionalInpaintChain.ts`, mirror the existing `facefixOnFinal` pattern. After line 74 (`const facefixOnFinal = baseParams.facefix_enabled;`) the block becomes:

```ts
  const facefixOnFinal = baseParams.facefix_enabled;
  // Face fix + segment refinement once on the final combined image; skip on
  // base + intermediate inpaints.
  baseParams.facefix_enabled = false;
  const segmentsOnFinal = baseParams.detail_segments;
  baseParams.detail_segments = [];
```

And in the `regionParams` literal (after `facefix_enabled: isFinalOutput && facefixOnFinal,`), add:

```ts
      detail_segments: isFinalOutput ? segmentsOnFinal : [],
```

- [ ] **Step 3.4: Verify build**

Run: `npm run build`
Expected: PASS with zero TypeScript errors (this resolves the required-field error from Tasks 1–2 if it appeared).

- [ ] **Step 3.5: Commit (include Task 1/2 files if their commits were held)**

```pwsh
git -c core.hooksPath=/dev/null add src/lib/stores/generation.svelte.ts src/lib/utils/regionalInpaintChain.ts
git -c core.hooksPath=/dev/null commit -m "feat: send detail_segments in generation params"
```

---

### Task 4: Detector download flow + i18n keys

**Files:**
- Modify: `src/lib/components/generation/GenerateButton.svelte` (~lines 75–209)
- Modify: `src/lib/locales/en.ts`, `de.ts`, `es.ts`, `fr.ts`, `it.ts`, `ja.ts`, `ko.ts`, `pt.ts`, `ru.ts`, `zh.ts`, `zh-tw.ts`

- [ ] **Step 4.1: Extract a shared detector-ensure helper**

In `GenerateButton.svelte`, add the import:

```ts
import { parseSegmentDetailPrompt, yoloTargetFilename } from "$lib/utils/promptSegmentDetail.js";
```

(Match the file's existing import style — if other utils are imported via relative paths like `../../utils/...`, use that form.)

Below `ensureFacefixPythonDependency` (line 83), add:

```ts
  const DETECTOR_META: Record<string, { url: string; sha256?: string }> = {
    "Anzhc Face seg 640 v4 y11n.pt": {
      url: "https://huggingface.co/Anzhc/Anzhcs_YOLOs/resolve/0319daeae9ae40752c2fb3904069cb35cc61d2ec/Anzhc%20Face%20seg%20640%20v4%20y11n.pt",
      sha256: "1e77ad7bd349babd8a4a90478bfc965348642b63a8d95d3b43ee13db42fd0a64",
    },
  };

  /** Download a YOLO detector into models/ultralytics if missing, then ensure the python dep. */
  async function ensureUltralyticsDetector(detector: string, toastKey: string): Promise<void> {
    if (!models.ultralyticsModels.includes(detector)) {
      gallery.showToast(locale.t(toastKey), "info");
      const meta = DETECTOR_META[detector];
      const url = meta?.url ?? `https://huggingface.co/Bingsu/adetailer/resolve/main/${detector}`;
      await downloadModel(url, "ultralytics", detector, undefined, meta?.sha256);
      await models.refresh();
    }
    await ensureFacefixPythonDependency();
  }
```

- [ ] **Step 4.2: Use the helper for face fix and segment targets**

Replace the existing face fix ensure block (lines 185–209, from `// Ensure face fix dependencies are ready when enabled` through the closing `}` after `await ensureFacefixPythonDependency();`) with:

```ts
      // Ensure face fix dependencies are ready when enabled
      if (generation.facefixEnabled) {
        const detector = generation.facefixDetector || "Anzhc Face seg 640 v4 y11n.pt";
        await ensureUltralyticsDetector(detector, "generation.downloading_facefix");
        generation.facefixDetector = detector;
      }

      // Ensure YOLO detectors referenced by <segment:yolo-...> tags are ready.
      // Unknown models that fail to download are skipped with a warning — the
      // segment node passes through unchanged when its model is missing.
      for (const segment of parseSegmentDetailPrompt(generation.positivePrompt).segments) {
        const detector = yoloTargetFilename(segment.target);
        if (!detector) continue;
        try {
          await ensureUltralyticsDetector(detector, "generation.segment.downloading_detector");
        } catch (e) {
          console.warn("[segment] detector unavailable:", detector, e);
          gallery.showToast(
            locale.t("generation.segment.detector_unavailable", { name: detector }),
            "warning",
          );
        }
      }
```

(Note: `generation.facefixDetector = detector;` moved after the ensure call but keeps its original effect; `models.refresh()` now happens inside the helper.)

- [ ] **Step 4.3: Add i18n keys**

In `src/lib/locales/en.ts`, next to the existing `generation.facefix.*` keys (~line 768), add:

```ts
  "generation.segment.downloading_detector": "Downloading segment detector model...",
  "generation.segment.detector_unavailable": "Segment detector '{name}' is not available — that segment will be skipped.",
```

Add the same two keys, translated, to **every** other locale file (`de.ts`, `es.ts`, `fr.ts`, `it.ts`, `ja.ts`, `ko.ts`, `pt.ts`, `ru.ts`, `zh.ts`, `zh-tw.ts`), in the same neighborhood as their `generation.facefix.*` keys. Keep the `{name}` placeholder verbatim in every translation. Suggested translations:

| Locale | downloading_detector | detector_unavailable |
|---|---|---|
| de | `"Segment-Detektormodell wird heruntergeladen..."` | `"Segment-Detektor '{name}' ist nicht verfügbar — dieses Segment wird übersprungen."` |
| es | `"Descargando modelo detector de segmentos..."` | `"El detector de segmentos '{name}' no está disponible — ese segmento se omitirá."` |
| fr | `"Téléchargement du modèle de détection de segment..."` | `"Le détecteur de segment '{name}' n'est pas disponible — ce segment sera ignoré."` |
| it | `"Download del modello rilevatore di segmenti..."` | `"Il rilevatore di segmenti '{name}' non è disponibile — quel segmento verrà saltato."` |
| ja | `"セグメント検出モデルをダウンロード中..."` | `"セグメント検出器「{name}」が利用できないため、そのセグメントはスキップされます。"` |
| ko | `"세그먼트 감지 모델 다운로드 중..."` | `"세그먼트 감지기 '{name}'를 사용할 수 없어 해당 세그먼트를 건너뜁니다."` |
| pt | `"Baixando modelo detector de segmentos..."` | `"O detector de segmentos '{name}' não está disponível — esse segmento será ignorado."` |
| ru | `"Загрузка модели детектора сегментов..."` | `"Детектор сегментов '{name}' недоступен — этот сегмент будет пропущен."` |
| zh | `"正在下载分割检测模型..."` | `"分割检测器 '{name}' 不可用——将跳过该分割。"` |
| zh-tw | `"正在下載分割偵測模型..."` | `"分割偵測器 '{name}' 無法使用——將跳過該分割。"` |

- [ ] **Step 4.4: Verify build**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 4.5: Commit**

```pwsh
git -c core.hooksPath=/dev/null add src/lib/components/generation/GenerateButton.svelte src/lib/locales
git -c core.hooksPath=/dev/null commit -m "feat: ensure segment YOLO detectors before generation"
```

---

### Task 5: Rust types + validation

**Files:**
- Modify: `src-tauri/src/comfyui/types.rs` (struct after `PromptSegment` ~line 68; field after `negative_segments` ~line 90)
- Modify: `src-tauri/src/templates/mod.rs` (`validate_generation_params`, style-transfer block lines 78–116)

- [ ] **Step 5.1: Add the `DetailSegment` struct and params field**

In `src-tauri/src/comfyui/types.rs`, after the `PromptSegment` struct (line 65–69), add:

```rust
/// A `<segment:...>` auto-refinement region parsed from the positive prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailSegment {
    /// Detection target: free text (CLIPSeg) or "yolo-<model filename>[-<match index>]".
    pub target: String,
    /// Refinement prompt for the detected region (may be empty).
    pub prompt: String,
    /// Denoise strength for the re-sample, (0, 1].
    pub creativity: f64,
    /// Detection threshold, (0, 1).
    pub threshold: f64,
}
```

In `GenerationParams`, after `pub negative_segments: Vec<PromptSegment>,` (line 90), add:

```rust
    #[serde(default)]
    pub detail_segments: Vec<DetailSegment>,
```

- [ ] **Step 5.2: Reject segments + style transfer**

In `src-tauri/src/templates/mod.rs`, inside the `if params.style_transfer_enabled {` block, after the `facefix_enabled` check (lines 111–115), add:

```rust
        if !params.detail_segments.is_empty() {
            return Err(
                "Style transfer cannot be used with <segment> refinement in this version — remove segment tags from the prompt.".into(),
            );
        }
```

- [ ] **Step 5.3: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles with no errors or new warnings.

- [ ] **Step 5.4: Commit**

```pwsh
git -c core.hooksPath=/dev/null add src-tauri/src/comfyui/types.rs src-tauri/src/templates/mod.rs
git -c core.hooksPath=/dev/null commit -m "feat: add detail_segments to Rust generation params"
```

---

### Task 6: `MooshieSegmentDetailer` Python node

**Files:**
- Modify: `src-tauri/src/comfyui/mooshie_nodes.py` (new class before `MooshieSaveImage` ~line 225; registration maps lines 409–417; imports lines 6–18)
- Modify: `src-tauri/src/comfyui/nodes.rs` (`REQUIRED_MOOSHIE_NODE_CLASSES` line 59)

- [ ] **Step 6.1: Add `re` import**

At the top of `mooshie_nodes.py`, the import block (lines 6–10) becomes:

```python
import io
import json
import re
import struct
import torch
import numpy as np
```

- [ ] **Step 6.2: Add the node class**

Insert the following class between `MooshieFaceDetailer` and `MooshieSaveImage` (i.e. after line 222, before the `class MooshieSaveImage:` comment block):

```python
class MooshieSegmentDetailer:
    """Detect a region by text (CLIPSeg) or YOLO model, re-denoise it with its
    own conditioning, and composite back using the (grown + blurred) detected
    mask — SwarmUI-style <segment:...> refinement."""

    CLIPSEG_REPO = "CIDAS/clipseg-rd64-refined"

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "image": ("IMAGE",),
                "model": ("MODEL",),
                "vae": ("VAE",),
                "positive": ("CONDITIONING",),
                "negative": ("CONDITIONING",),
                "detection": ("STRING", {"default": ""}),
                "seed": ("INT", {"default": 0, "min": 0, "max": 0xFFFFFFFFFFFFFFFF}),
                "steps": ("INT", {"default": 20, "min": 1, "max": 100}),
                "cfg": ("FLOAT", {"default": 7.0, "min": 0.0, "max": 100.0, "step": 0.1}),
                "sampler_name": (comfy.samplers.KSampler.SAMPLERS,),
                "scheduler": (comfy.samplers.KSampler.SCHEDULERS,),
                "denoise": ("FLOAT", {"default": 0.6, "min": 0.0, "max": 1.0, "step": 0.05}),
                "guide_size": ("INT", {"default": 512, "min": 64, "max": 2048, "step": 64}),
                "threshold": ("FLOAT", {"default": 0.5, "min": 0.0, "max": 1.0, "step": 0.05}),
                "mask_grow": ("INT", {"default": 16, "min": 0, "max": 256}),
                "mask_blur": ("INT", {"default": 8, "min": 0, "max": 64}),
            }
        }

    RETURN_TYPES = ("IMAGE",)
    FUNCTION = "process"
    CATEGORY = "mooshie"

    def process(
        self,
        image,
        model,
        vae,
        positive,
        negative,
        detection,
        seed,
        steps,
        cfg,
        sampler_name,
        scheduler,
        denoise,
        guide_size,
        threshold,
        mask_grow,
        mask_blur,
    ):
        detection = (detection or "").strip()
        if not detection:
            return (image,)

        B, H, W, C = image.shape
        result = image.clone()

        for b in range(B):
            frame = image[b].cpu().numpy()
            if np.isnan(frame).any():
                frame = np.nan_to_num(frame, nan=0.0)
            img_np = (frame * 255).astype(np.uint8)

            if detection.lower().startswith("yolo-"):
                mask = self._yolo_mask(detection[len("yolo-"):], img_np, H, W, threshold)
            else:
                mask = self._clipseg_mask(detection, img_np, H, W, threshold)

            if mask is None or mask.sum().item() < 16:
                print(f"[MooshieSegmentDetailer] No region found for '{detection}' (batch {b})")
                continue

            mask = mask.to(image.device)
            if mask_grow > 0:
                mask = torch.nn.functional.max_pool2d(
                    mask[None, None],
                    kernel_size=mask_grow * 2 + 1,
                    stride=1,
                    padding=mask_grow,
                )[0, 0]
            blurred = self._blur_mask(mask, mask_blur)

            ys, xs = torch.nonzero(blurred > 0.01, as_tuple=True)
            pad = 32
            cy1 = max(0, int(ys.min().item()) - pad)
            cy2 = min(H, int(ys.max().item()) + 1 + pad)
            cx1 = max(0, int(xs.min().item()) - pad)
            cx2 = min(W, int(xs.max().item()) + 1 + pad)
            crop_h, crop_w = cy2 - cy1, cx2 - cx1
            if crop_h < 8 or crop_w < 8:
                continue

            crop = result[b : b + 1, cy1:cy2, cx1:cx2, :].clone()

            scale = guide_size / max(crop_h, crop_w)
            new_h = max(8, round(crop_h * scale / 8) * 8)
            new_w = max(8, round(crop_w * scale / 8) * 8)
            resized = torch.nn.functional.interpolate(
                crop.permute(0, 3, 1, 2),
                size=(new_h, new_w),
                mode="bilinear",
                align_corners=False,
            ).permute(0, 2, 3, 1)

            latent = vae.encode(resized[:, :, :, :3])
            latent = comfy.sample.fix_empty_latent_channels(model, latent)

            noise = comfy.sample.prepare_noise(latent, seed + b)
            callback = latent_preview.prepare_callback(model, steps)
            samples = comfy.sample.sample(
                model,
                noise,
                steps,
                cfg,
                sampler_name,
                scheduler,
                positive,
                negative,
                latent,
                denoise=denoise,
                force_full_denoise=True,
                callback=callback,
                disable_pbar=False,
                seed=seed + b,
            )

            decoded = vae.decode(samples)
            if decoded.ndim == 5:
                decoded = decoded.reshape(
                    -1, decoded.shape[-3], decoded.shape[-2], decoded.shape[-1]
                )

            back = torch.nn.functional.interpolate(
                decoded.permute(0, 3, 1, 2),
                size=(crop_h, crop_w),
                mode="bilinear",
                align_corners=False,
            ).permute(0, 2, 3, 1)

            # Composite with the blurred detected mask so irregular shapes
            # (eyes, hands) blend cleanly — not a rectangular feather.
            blend = blurred[cy1:cy2, cx1:cx2].unsqueeze(0).unsqueeze(-1)
            original_crop = result[b : b + 1, cy1:cy2, cx1:cx2, :]
            result[b : b + 1, cy1:cy2, cx1:cx2, :] = (
                back * blend + original_crop * (1 - blend)
            ).clamp(0, 1)

        return (result,)

    @staticmethod
    def _parse_yolo_name(name):
        """'model.pt-2' -> ('model.pt', 2); 'model.pt' -> ('model.pt', None)."""
        m = re.match(r"^(.+\.(?:pt|onnx))-(\d+)$", name, re.IGNORECASE)
        if m:
            return m.group(1), int(m.group(2))
        return name, None

    def _yolo_mask(self, name, img_np, H, W, threshold):
        """Union mask [H, W] float 0/1 from YOLO detections, or None."""
        from ultralytics import YOLO

        model_name, match_index = self._parse_yolo_name(name.strip())
        model_path = folder_paths.get_full_path("ultralytics", model_name)
        if model_path is None:
            print(f"[MooshieSegmentDetailer] YOLO model not found: {model_name}")
            return None

        yolo = YOLO(model_path)
        detections = yolo(img_np, verbose=False)
        if not detections or len(detections[0].boxes) == 0:
            return None

        boxes = detections[0].boxes
        seg_masks = detections[0].masks.data if detections[0].masks is not None else None

        # Confidence-sorted indices above threshold; -N selects the Nth best match.
        order = sorted(range(len(boxes)), key=lambda i: boxes.conf[i].item(), reverse=True)
        order = [i for i in order if boxes.conf[i].item() >= threshold]
        if not order:
            return None
        if match_index is not None:
            if match_index < 1 or match_index > len(order):
                return None
            order = [order[match_index - 1]]

        mask = torch.zeros((H, W), dtype=torch.float32)
        for i in order:
            if seg_masks is not None:
                m = torch.nn.functional.interpolate(
                    seg_masks[i][None, None].float().cpu(),
                    size=(H, W),
                    mode="bilinear",
                    align_corners=False,
                )[0, 0]
                mask = torch.maximum(mask, (m > 0.5).float())
            else:
                x1, y1, x2, y2 = boxes.xyxy[i].cpu().int().tolist()
                mask[max(0, y1) : min(H, y2), max(0, x1) : min(W, x2)] = 1.0
        return mask

    def _clipseg_mask(self, text, img_np, H, W, threshold):
        """Binary mask [H, W] from CLIPSeg text detection, or None on failure.

        Weights cache under models/clipseg/ (auto-downloaded on first use) and
        are released after each run — no persistent VRAM/RAM residency.
        """
        from transformers import CLIPSegProcessor, CLIPSegForImageSegmentation
        from PIL import Image as PILImage

        cache_dir = os.path.join(folder_paths.models_dir, "clipseg")
        os.makedirs(cache_dir, exist_ok=True)

        processor = CLIPSegProcessor.from_pretrained(self.CLIPSEG_REPO, cache_dir=cache_dir)
        seg_model = CLIPSegForImageSegmentation.from_pretrained(
            self.CLIPSEG_REPO, cache_dir=cache_dir
        )
        try:
            pil = PILImage.fromarray(img_np)
            inputs = processor(text=[text], images=[pil], return_tensors="pt")
            with torch.no_grad():
                logits = seg_model(**inputs).logits
            heat = torch.sigmoid(logits.float())
            if heat.ndim == 3:
                heat = heat[0]
            mask = torch.nn.functional.interpolate(
                heat[None, None], size=(H, W), mode="bilinear", align_corners=False
            )[0, 0]
            return (mask >= threshold).float()
        finally:
            del seg_model, processor

    @staticmethod
    def _blur_mask(mask, radius):
        """Approximate gaussian blur with 3 box blurs (replicate-padded avg_pool)."""
        if radius <= 0:
            return mask.clamp(0, 1)
        k = radius * 2 + 1
        m = mask[None, None]
        for _ in range(3):
            m = torch.nn.functional.avg_pool2d(
                torch.nn.functional.pad(m, (radius, radius, radius, radius), mode="replicate"),
                kernel_size=k,
                stride=1,
            )
        return m[0, 0].clamp(0, 1)
```

- [ ] **Step 6.3: Register the node**

At the bottom of `mooshie_nodes.py`, the mappings become:

```python
NODE_CLASS_MAPPINGS = {
    "MooshieFaceDetailer": MooshieFaceDetailer,
    "MooshieSegmentDetailer": MooshieSegmentDetailer,
    "MooshieSaveImage": MooshieSaveImage,
}

NODE_DISPLAY_NAME_MAPPINGS = {
    "MooshieFaceDetailer": "Mooshie Face Detailer",
    "MooshieSegmentDetailer": "Mooshie Segment Detailer",
    "MooshieSaveImage": "Mooshie Save Image",
}
```

- [ ] **Step 6.4: Require the node class at startup verification**

In `src-tauri/src/comfyui/nodes.rs`, add `"MooshieSegmentDetailer"` to `REQUIRED_MOOSHIE_NODE_CLASSES` (line 59), after `"MooshieFaceDetailer"`:

```rust
const REQUIRED_MOOSHIE_NODE_CLASSES: &[&str] = &[
    "MooshieSaveImage",
    "MooshieFaceDetailer",
    "MooshieSegmentDetailer",
    "MooshieSoftGuidance",
    "MooshieSmartGuidance",
    "NanoSaurLoader",
```

(keep the rest of the array unchanged).

- [ ] **Step 6.5: Verify**

Run: `python -m py_compile src-tauri/src/comfyui/mooshie_nodes.py` (syntax check only — comfy imports are unavailable locally, `py_compile` does not execute them)
Expected: exit 0, no output.

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles (the `include_str!` picks up the .py change automatically).

- [ ] **Step 6.6: Commit**

```pwsh
git -c core.hooksPath=/dev/null add src-tauri/src/comfyui/mooshie_nodes.py src-tauri/src/comfyui/nodes.rs
git -c core.hooksPath=/dev/null commit -m "feat: add MooshieSegmentDetailer custom node (CLIPSeg + YOLO)"
```

---

### Task 7: Rust workflow template

**Files:**
- Create: `src-tauri/src/templates/segment_detail.rs`
- Modify: `src-tauri/src/templates/mod.rs` (module list line 1–7; `finish_workflow` lines 355–367)

- [ ] **Step 7.1: Create the template**

Create `src-tauri/src/templates/segment_detail.rs`:

```rust
use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

/// Appends one MooshieSegmentDetailer per `<segment:...>` tag, in prompt order,
/// each with its own CLIPTextEncode (global regional context + segment prompt).
/// Returns the (node_id, output_index) of the final refined IMAGE.
pub fn append_segment_chain(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    current_image: (String, u32),
    seed: i64,
) -> (String, u32) {
    let context = super::build_regional_context_prompt(params);
    let mut image = current_image;

    for (i, segment) in params.detail_segments.iter().enumerate() {
        let encode_text = super::merge_regional_encode_text(&context, &segment.prompt);

        let clip_id = result.next_id.to_string();
        result.workflow.insert(
            clip_id.clone(),
            json!({
                "class_type": "CLIPTextEncode",
                "inputs": {
                    "clip": [result.clip_source.0.clone(), result.clip_source.1],
                    "text": encode_text
                }
            }),
        );
        result.next_id += 1;

        let detailer_id = result.next_id.to_string();
        result.workflow.insert(
            detailer_id.clone(),
            json!({
                "class_type": "MooshieSegmentDetailer",
                "inputs": {
                    "image": [image.0, image.1],
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "vae": [result.vae_source.0.clone(), result.vae_source.1],
                    "positive": [clip_id, 0],
                    "negative": [result.negative_source.0.clone(), result.negative_source.1],
                    "detection": segment.target,
                    // seed+2 is taken by facefix
                    "seed": seed + 3 + i as i64,
                    "steps": params.facefix_steps,
                    "cfg": params.cfg,
                    "sampler_name": params.sampler_name,
                    "scheduler": params.scheduler,
                    "denoise": segment.creativity,
                    "guide_size": params.facefix_guide_size,
                    "threshold": segment.threshold,
                    "mask_grow": 16,
                    "mask_blur": 8
                }
            }),
        );
        result.next_id += 1;

        image = (detailer_id, 0);
    }

    image
}
```

- [ ] **Step 7.2: Wire into `finish_workflow`**

In `src-tauri/src/templates/mod.rs`:

Add to the module list (alphabetical, after `pub mod inpainting;`):

```rust
pub mod segment_detail;
```

In `finish_workflow`, after the facefix block (lines 362–367) and before the `MooshieSaveImage` insertion, add:

```rust
    // Apply <segment:...> auto-refinement after facefix so face fix results
    // feed into segment detection.
    let final_image = if !params.detail_segments.is_empty() {
        segment_detail::append_segment_chain(&mut result, params, final_image, seed)
    } else {
        final_image
    };
```

- [ ] **Step 7.3: Verify compile + lint**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles clean.

Run (in `src-tauri/`): `cargo fmt && cargo clippy`
Expected: no formatting diff (or apply it), no new clippy warnings.

- [ ] **Step 7.4: Commit**

```pwsh
git -c core.hooksPath=/dev/null add src-tauri/src/templates/segment_detail.rs src-tauri/src/templates/mod.rs
git -c core.hooksPath=/dev/null commit -m "feat: append segment refinement chain to workflows"
```

---

### Task 8: Final validation + manual test checklist

- [ ] **Step 8.1: Full gates**

Run: `npm run build`
Expected: PASS.

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

Run in `src-tauri/`: `cargo fmt --check && cargo clippy`
Expected: clean.

- [ ] **Step 8.2: i18n completeness check**

For both new keys, confirm presence in all 11 locale files:

```pwsh
Select-String -Path src/lib/locales/*.ts -Pattern "generation.segment.downloading_detector" | Measure-Object
Select-String -Path src/lib/locales/*.ts -Pattern "generation.segment.detector_unavailable" | Measure-Object
```

Expected: Count = 11 for each.

- [ ] **Step 8.3: Manual smoke test (requires `npm run tauri dev` + a local model)**

1. `1girl, park <segment:eyes> green eyes` → CLIPSeg path; first run logs the model download under `models/clipseg/`; eyes get visibly refined.
2. `portrait <segment:yolo-Anzhc Face seg 640 v4 y11n.pt,0.5> detailed face` → YOLO path; detector auto-downloads if missing.
3. Two segments: `... <segment:face> freckles <segment:hands> detailed hands` → both refine, prompt order.
4. Closed form: `a cat <segment:eyes,0.5>glowing eyes</segment> on a sofa` → "on a sofa" stays in the base prompt (check the workflow JSON log).
5. Invalid tag `<segment:>` or `<segment:eyes,5>` → stays literal in the prompt, no segments in params.
6. Segments + upscale + face fix together → chain order upscale → facefix → segments (check workflow JSON).
7. Segment tags highlight teal in the prompt box; gallery metadata reimport restores the tags.
8. Browser mode (`LAN` serving): same prompt generates identically.
9. Style transfer + segment tag → validation error before submit.

- [ ] **Step 8.4: Finish**

Use the **superpowers:finishing-a-development-branch** skill (or the project's `push` skill conventions) to land `feat/segment-prompt-syntax` on main via squash-merged PR referencing issue #288.
