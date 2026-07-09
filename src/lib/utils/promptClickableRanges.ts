import {
  getPromptInertRanges,
  isInsidePromptInertRange,
  type PromptTextRange,
} from "./promptInertRanges.js";
import { hasUnescapedSyntaxAngles, isBackslashEscaped } from "./promptSyntaxEscape.js";

export type PromptClickableSegmentKind = "text" | "tag" | "weighted";

export interface PromptClickableSegment {
  start: number;
  end: number;
  kind: PromptClickableSegmentKind;
  clickable: boolean;
}

interface Range {
  start: number;
  end: number;
}

interface OpenToken {
  index: number;
  char: "(" | "{" | "[";
}

const WEIGHT_SUFFIX_RE = /^\d*\.?\d+$/;

function pushSegment(
  segments: PromptClickableSegment[],
  start: number,
  end: number,
  kind: PromptClickableSegmentKind,
  clickable: boolean,
) {
  if (start >= end) return;
  segments.push({ start, end, kind, clickable });
}

function isWeightedExpression(raw: string, start: number, end: number): boolean {
  if (end - start < 5 || raw[start] !== "(" || raw[end - 1] !== ")") return false;

  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let lastValidColon = -1;

  for (let i = start + 1; i < end - 1; i++) {
    if (isBackslashEscaped(raw, i)) continue;

    const ch = raw[i];
    if (ch === "(") {
      parenDepth += 1;
      continue;
    }
    if (ch === ")" && parenDepth > 0) {
      parenDepth -= 1;
      continue;
    }
    if (ch === "[") {
      bracketDepth += 1;
      continue;
    }
    if (ch === "]" && bracketDepth > 0) {
      bracketDepth -= 1;
      continue;
    }
    if (ch === "{") {
      braceDepth += 1;
      continue;
    }
    if (ch === "}" && braceDepth > 0) {
      braceDepth -= 1;
      continue;
    }

    if (
      ch === ":" &&
      parenDepth === 0 &&
      bracketDepth === 0 &&
      braceDepth === 0
    ) {
      const right = raw.slice(i + 1, end - 1).trim();
      if (right.length > 0 && WEIGHT_SUFFIX_RE.test(right)) {
        lastValidColon = i;
      }
    }
  }

  if (lastValidColon === -1) return false;
  const left = raw.slice(start + 1, lastValidColon).trim();
  return left.length > 0;
}

// True if the group content between raw[start] '(' and raw[end] ')' contains an
// unescaped paren, i.e. it is not a flat innermost group. InvokeAI trailing and
// emphasis translation only applies to flat groups ([^()]+), so we mirror that.
function groupInnerHasParen(raw: string, start: number, end: number): boolean {
  for (let k = start + 1; k < end; k++) {
    if (isBackslashEscaped(raw, k)) continue;
    if (raw[k] === "(" || raw[k] === ")") return true;
  }
  return false;
}

// Length of an InvokeAI trailing weight/emphasis run immediately after a group
// close at index `j`, or 0 if none. Emphasis marks (+/-) match directly;
// a numeric weight must be followed by a delimiter or end-of-string so a
// number-starting tag like "(beautiful)1girl" is not read as a weight.
function trailingWeightLen(raw: string, j: number): number {
  const rest = raw.slice(j);
  const marks = rest.match(/^(?:\++|-+)/);
  if (marks) return marks[0].length;
  const num = rest.match(/^\d+\.?\d*(?=[\s,(){}[\]]|$)/);
  if (num) return num[0].length;
  return 0;
}

// NAI numeric weight: N::text::, where text has no colon. Mirrors the
// translateNaiWeightSyntax regex.
const NAI_NUMERIC_RE = /(\d+\.?\d*)::[^:]+::/g;

function getWeightedRanges(raw: string, inertRanges: readonly PromptTextRange[]): Range[] {
  const stack: OpenToken[] = [];
  const candidates: Range[] = [];

  const openForClose: Record<string, OpenToken["char"]> = {
    ")": "(",
    "}": "{",
    "]": "[",
  };

  const isWrappedWeight = (start: number, end: number) =>
    raw.slice(start + 1, end - 1).trim().length > 0;

  for (let i = 0; i < raw.length; i++) {
    if (isInsidePromptInertRange(i, inertRanges)) continue;
    if (isBackslashEscaped(raw, i)) continue;

    const ch = raw[i];
    if (ch === "(" || ch === "{" || ch === "[") {
      stack.push({ index: i, char: ch });
      continue;
    }

    if (ch === ")" || ch === "}" || ch === "]") {
      const open = openForClose[ch];
      const top = stack[stack.length - 1];
      if (!top || top.char !== open) continue;
      stack.pop();

      const start = top.index;
      const end = i + 1;

      // InvokeAI trailing weight / emphasis on a flat paren group: (tag)0.5,
      // (tag)--. The range spans the group plus its trailing run.
      if (
        open === "(" &&
        isWrappedWeight(start, end) &&
        !groupInnerHasParen(raw, start, end - 1)
      ) {
        const trailing = trailingWeightLen(raw, end);
        if (trailing > 0) {
          candidates.push({ start, end: end + trailing });
          continue;
        }
      }

      const isWeighted =
        open === "(" ? isWeightedExpression(raw, start, end) : isWrappedWeight(start, end);
      if (isWeighted) {
        candidates.push({ start, end });
      }
    }
  }

  // NAI numeric weight: N::text::. These carry no brackets, so scan separately.
  NAI_NUMERIC_RE.lastIndex = 0;
  let naiMatch: RegExpExecArray | null;
  while ((naiMatch = NAI_NUMERIC_RE.exec(raw)) !== null) {
    const start = naiMatch.index;
    if (isInsidePromptInertRange(start, inertRanges)) continue;
    if (isBackslashEscaped(raw, start)) continue;
    candidates.push({ start, end: start + naiMatch[0].length });
  }

  candidates.sort((a, b) => (a.start - b.start) || (b.end - a.end));

  const result: Range[] = [];
  for (const candidate of candidates) {
    const last = result[result.length - 1];
    if (!last || candidate.start >= last.end) {
      result.push(candidate);
      continue;
    }
    if (candidate.start <= last.start && candidate.end >= last.end) {
      result[result.length - 1] = candidate;
    }
  }

  return result;
}

function isPlainClickableToken(token: string): boolean {
  const trimmed = token.trim();
  if (!trimmed) return false;
  if (trimmed.toLowerCase().startsWith("@preset:")) return false;
  if (hasUnescapedSyntaxAngles(trimmed)) return false;
  return true;
}

function pushTokenSegments(
  segments: PromptClickableSegment[],
  raw: string,
  rangeStart: number,
  rangeEnd: number,
) {
  if (rangeStart >= rangeEnd) return;

  let trimmedStart = rangeStart;
  while (trimmedStart < rangeEnd && /\s/.test(raw[trimmedStart])) {
    trimmedStart += 1;
  }

  let trimmedEnd = rangeEnd;
  while (trimmedEnd > trimmedStart && /\s/.test(raw[trimmedEnd - 1])) {
    trimmedEnd -= 1;
  }

  pushSegment(segments, rangeStart, trimmedStart, "text", false);
  if (trimmedStart < trimmedEnd) {
    const token = raw.slice(trimmedStart, trimmedEnd);
    if (isPlainClickableToken(token)) {
      pushSegment(segments, trimmedStart, trimmedEnd, "tag", true);
    } else {
      pushSegment(segments, trimmedStart, trimmedEnd, "text", false);
    }
  }
  pushSegment(segments, trimmedEnd, rangeEnd, "text", false);
}

function pushGapSegments(
  segments: PromptClickableSegment[],
  raw: string,
  start: number,
  end: number,
  inertRanges: readonly PromptTextRange[],
) {
  if (start >= end) return;

  let cursor = start;
  for (const inert of inertRanges) {
    if (inert.end <= start || inert.start >= end) continue;

    const inertStart = Math.max(start, inert.start);
    const inertEnd = Math.min(end, inert.end);
    if (inertStart > cursor) {
      pushSplittableGap(segments, raw, cursor, inertStart, inertRanges);
    }
    pushSegment(segments, inertStart, inertEnd, "text", false);
    cursor = inertEnd;
  }

  if (cursor < end) {
    pushSplittableGap(segments, raw, cursor, end, inertRanges);
  }
}

function pushSplittableGap(
  segments: PromptClickableSegment[],
  raw: string,
  start: number,
  end: number,
  inertRanges: readonly PromptTextRange[],
) {
  if (start >= end) return;

  let tokenStart = start;
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;

  for (let i = start; i < end; i++) {
    if (isInsidePromptInertRange(i, inertRanges)) continue;
    if (!isBackslashEscaped(raw, i)) {
      const ch = raw[i];
      if (ch === "(") parenDepth += 1;
      else if (ch === ")" && parenDepth > 0) parenDepth -= 1;
      else if (ch === "[") bracketDepth += 1;
      else if (ch === "]" && bracketDepth > 0) bracketDepth -= 1;
      else if (ch === "{") braceDepth += 1;
      else if (ch === "}" && braceDepth > 0) braceDepth -= 1;
      else if (
        ch === "," &&
        parenDepth === 0 &&
        bracketDepth === 0 &&
        braceDepth === 0
      ) {
        pushTokenSegments(segments, raw, tokenStart, i);
        pushSegment(segments, i, i + 1, "text", false);
        tokenStart = i + 1;
      }
    }
  }

  pushTokenSegments(segments, raw, tokenStart, end);
}

export function getPromptClickableSegments(raw: string): PromptClickableSegment[] {
  if (!raw) return [];

  const inertRanges = getPromptInertRanges(raw);
  const segments: PromptClickableSegment[] = [];
  const weightedRanges = getWeightedRanges(raw, inertRanges);
  let cursor = 0;

  for (const range of weightedRanges) {
    pushGapSegments(segments, raw, cursor, range.start, inertRanges);
    pushSegment(segments, range.start, range.end, "weighted", true);
    cursor = range.end;
  }

  pushGapSegments(segments, raw, cursor, raw.length, inertRanges);
  return segments;
}
