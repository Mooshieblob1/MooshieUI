import { SYNTAX_ANGLE_LOOKBEHIND } from "./promptSyntaxEscape.ts";

/**
 * Shared detection for prompt syntax blocks that should not participate in
 * comma-split tag selection or naive angle-bracket depth tracking.
 */

export interface PromptTextRange {
  start: number;
  end: number;
}

/** MooshieUI + SwarmUI scheduling syntax. */
export const PROMPT_SCHEDULE_REGEX = new RegExp(
  `${SYNTAX_ANGLE_LOOKBEHIND}<(?:(from|to|range):(\\d+(?:\\.\\d+)?)(?::(\\d+(?:\\.\\d+)?))?>([ \\s\\S]*?)<\\/\\1>|fromto\\[(\\d+(?:\\.\\d+)?)\\]:([^>]+)>)`,
  "g",
);

/** Match `@preset:<slug>` directives. Slug = lowercase alnum + underscore. */
export const PROMPT_PRESET_TOKEN_REGEX = /@preset:([a-z0-9_]+)/gi;

export const PROMPT_LORA_TOKEN_REGEX = new RegExp(
  `${SYNTAX_ANGLE_LOOKBEHIND}<lora:\\s*[^>]+>`,
  "gi",
);

export const PROMPT_REGION_TAG_REGEX = new RegExp(
  `${SYNTAX_ANGLE_LOOKBEHIND}<region:(\\d*\\.?\\d+)\\s*,\\s*(\\d*\\.?\\d+)\\s*,\\s*(\\d*\\.?\\d+)\\s*,\\s*(\\d*\\.?\\d+)>([\\s\\S]*?)<\\/region>`,
  "gi",
);

/** <segment:...> opening tags and </segment> closers. Only the tags are inert —
 * the refinement prompt text between them stays interactive (autocomplete,
 * clickable tags). */
export const PROMPT_SEGMENT_TAG_REGEX = new RegExp(
  `${SYNTAX_ANGLE_LOOKBEHIND}<segment:[^>]+>|<\\/segment>`,
  "gi",
);

function mergePromptTextRanges(ranges: PromptTextRange[]): PromptTextRange[] {
  if (ranges.length === 0) return [];
  ranges.sort((a, b) => a.start - b.start || a.end - b.end);
  const merged: PromptTextRange[] = [];
  for (const range of ranges) {
    const last = merged[merged.length - 1];
    if (!last || range.start > last.end) {
      merged.push({ ...range });
      continue;
    }
    last.end = Math.max(last.end, range.end);
  }
  return merged;
}

function collectRegexRanges(raw: string, regex: RegExp): PromptTextRange[] {
  const ranges: PromptTextRange[] = [];
  regex.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(raw)) !== null) {
    ranges.push({ start: match.index, end: match.index + match[0].length });
  }
  return ranges;
}

/**
 * Ranges that should not participate in comma-split tag selection or weight
 * parsing — scheduling blocks, presets, LoRA tags, and regional tags.
 */
export function getPromptInertRanges(raw: string): PromptTextRange[] {
  if (!raw) return [];

  const ranges: PromptTextRange[] = [
    ...collectRegexRanges(raw, PROMPT_SCHEDULE_REGEX),
  ];

  if (raw.includes("@preset:")) {
    ranges.push(...collectRegexRanges(raw, PROMPT_PRESET_TOKEN_REGEX));
  }
  if (raw.includes("<lora:")) {
    ranges.push(...collectRegexRanges(raw, PROMPT_LORA_TOKEN_REGEX));
  }
  if (raw.includes("<region:")) {
    ranges.push(...collectRegexRanges(raw, PROMPT_REGION_TAG_REGEX));
  }
  const lower = raw.toLowerCase();
  if (lower.includes("<segment:") || lower.includes("</segment>")) {
    ranges.push(...collectRegexRanges(raw, PROMPT_SEGMENT_TAG_REGEX));
  }

  return mergePromptTextRanges(ranges);
}

export function isInsidePromptInertRange(
  index: number,
  ranges: readonly PromptTextRange[],
): boolean {
  let lo = 0;
  let hi = ranges.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const range = ranges[mid];
    if (index < range.start) {
      hi = mid - 1;
    } else if (index >= range.end) {
      lo = mid + 1;
    } else {
      return true;
    }
  }
  return false;
}

export function findPromptInertRangeContaining(
  raw: string,
  position: number,
  ranges?: readonly PromptTextRange[],
): PromptTextRange | null {
  const list = ranges ?? getPromptInertRanges(raw);
  for (const range of list) {
    // Half-open `[start, end)`, matching isInsidePromptInertRange. A caret at
    // `end` sits just past the block (the start of the next tag) and a caret at
    // `start` sits at the block's first character, so the old `> start && <= end`
    // both swallowed the tag after the block and dropped the block's leading edge.
    if (position >= range.start && position < range.end) {
      return range;
    }
  }
  return null;
}
