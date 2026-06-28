import { SYNTAX_ANGLE_LOOKBEHIND } from "./promptSyntaxEscape.ts";
import type { DetailSegment } from "../types/index.js";

/**
 * SwarmUI-style <segment:...> auto-refinement tags.
 *
 * Opening tag: <segment:<target>[,<creativity>[,<threshold>]]>
 *   - target: free text (CLIPSeg detection) or "yolo-<model filename>" with an
 *     optional trailing "-<n>" match index (e.g. "yolo-face_yolov8n.pt-1").
 *   - creativity: re-sample denoise, default 0.6, valid (0, 1].
 *   - threshold: detection threshold, default 0.1 (CLIPSeg) / 0.25 (YOLO), valid (0, 1).
 *
 * The refinement prompt is either everything after the tag until the next
 * <segment: tag or end of prompt (SwarmUI trailing form), or the text up to a
 * closing </segment> (MooshieUI closed form).
 *
 * Note: a trailing-form segment's prompt swallows everything (including other
 * tag syntax) until the next <segment: tag or end of prompt, so trailing-form
 * segments are best placed at the end of the prompt.
 */
export const PROMPT_SEGMENT_OPEN_REGEX = new RegExp(
  `${SYNTAX_ANGLE_LOOKBEHIND}<segment:([^>]+)>`,
  "gi",
);

export const DEFAULT_SEGMENT_CREATIVITY = 0.6;
export const DEFAULT_CLIPSEG_THRESHOLD = 0.1;
export const DEFAULT_YOLO_THRESHOLD = 0.25;

export interface ParsedSegmentDetailPrompt {
  baseText: string;
  segments: DetailSegment[];
  /** Raw-prompt character range of each valid segment (open tag through prompt/closer), parallel to `segments`. */
  ranges: Array<{ start: number; end: number }>;
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
  // Accept the full SwarmUI-compatible closed range [0, 1]; reject only
  // non-numeric / out-of-range values. Clamp to a safe interior so boundary
  // values (0, 1) imported from SwarmUI produce a usable mask instead of being
  // left as broken literal text in the prompt.
  if (!Number.isFinite(creativity) || creativity < 0 || creativity > 1) return null;
  if (!Number.isFinite(threshold) || threshold < 0 || threshold > 1) return null;
  const safeCreativity = Math.min(1, Math.max(0.05, creativity));
  const safeThreshold = Math.min(0.99, Math.max(0.01, threshold));
  return { target, creativity: safeCreativity, threshold: safeThreshold };
}

/**
 * Extract <segment:...> tags from a prompt. Tag text and refinement prompts are
 * removed from baseText; invalid tags are left as literal text (parser convention
 * shared with scheduling/region tags).
 */
export function parseSegmentDetailPrompt(raw: string): ParsedSegmentDetailPrompt {
  if (!raw || !raw.toLowerCase().includes("<segment:")) {
    return { baseText: raw ?? "", segments: [], ranges: [] };
  }

  const opens: Array<{ start: number; end: number; spec: string }> = [];
  PROMPT_SEGMENT_OPEN_REGEX.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = PROMPT_SEGMENT_OPEN_REGEX.exec(raw)) !== null) {
    opens.push({ start: match.index, end: match.index + match[0].length, spec: match[1] });
  }

  const segments: DetailSegment[] = [];
  const ranges: Array<{ start: number; end: number }> = [];
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
    const closeMatch = /<\/segment>/i.exec(between);

    if (closeMatch) {
      // Closed form: prompt up to </segment>; text after the closer returns to base.
      segments.push({ ...spec, prompt: between.slice(0, closeMatch.index).trim() });
      cursor = open.end + closeMatch.index + closeMatch[0].length;
    } else {
      // Trailing form: prompt runs to the next segment tag or end of prompt.
      segments.push({ ...spec, prompt: between.trim() });
      cursor = regionEnd;
    }
    ranges.push({ start: open.start, end: cursor });
  }

  baseText += raw.slice(cursor);
  baseText = baseText
    .replace(/,\s*,/g, ",")
    .replace(/^\s*,\s*/, "")
    .replace(/\s*,\s*$/, "")
    .trim();

  return { baseText, segments, ranges };
}

/**
 * Serialize segments back to canonical closed-form tags for metadata embedding.
 * Closed form survives reimport regardless of position; explicit numbers make
 * the round-trip exact.
 */
export function serializeSegmentTag(s: DetailSegment): string {
  return `<segment:${s.target},${s.creativity},${s.threshold}>${s.prompt}</segment>`;
}

export function serializeSegmentTags(
  segments: ReadonlyArray<DetailSegment>,
): string {
  return segments.map(serializeSegmentTag).join(", ");
}

/** Rewrite the nth valid segment tag in `raw` with canonical closed-form syntax. */
export function replaceSegmentInPrompt(
  raw: string,
  index: number,
  segment: DetailSegment,
): string {
  const { ranges } = parseSegmentDetailPrompt(raw);
  const range = ranges[index];
  if (!range) return raw;
  return raw.slice(0, range.start) + serializeSegmentTag(segment) + raw.slice(range.end);
}

/** Remove the nth valid segment tag (and its refinement prompt) from `raw`. */
export function removeSegmentFromPrompt(raw: string, index: number): string {
  const { ranges } = parseSegmentDetailPrompt(raw);
  const range = ranges[index];
  if (!range) return raw;
  return (raw.slice(0, range.start) + raw.slice(range.end))
    .replace(/,\s*,/g, ",")
    .replace(/\s*,\s*$/, "")
    .trimEnd();
}

/** Append a closed-form segment tag (defaults apply for creativity/threshold). */
export function appendSegmentToPrompt(
  raw: string,
  target: string,
  prompt: string,
): string {
  const tag = `<segment:${target}>${prompt}</segment>`;
  const trimmed = raw.trimEnd();
  if (!trimmed) return tag;
  return trimmed.endsWith(",") ? `${trimmed} ${tag}` : `${trimmed}, ${tag}`;
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
