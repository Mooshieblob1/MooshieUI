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
