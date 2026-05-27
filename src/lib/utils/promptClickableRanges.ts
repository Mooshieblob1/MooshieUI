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

function isEscaped(raw: string, index: number): boolean {
  let slashCount = 0;
  for (let i = index - 1; i >= 0 && raw[i] === "\\"; i--) {
    slashCount += 1;
  }
  return slashCount % 2 === 1;
}

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
  let angleDepth = 0;

  for (let i = start + 1; i < end - 1; i++) {
    if (isEscaped(raw, i)) continue;

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
    if (ch === "<") {
      angleDepth += 1;
      continue;
    }
    if (ch === ">" && angleDepth > 0) {
      angleDepth -= 1;
      continue;
    }

    if (
      ch === ":" &&
      parenDepth === 0 &&
      bracketDepth === 0 &&
      braceDepth === 0 &&
      angleDepth === 0
    ) {
      const left = raw.slice(start + 1, i).trim();
      const right = raw.slice(i + 1, end - 1).trim();
      if (left.length > 0 && WEIGHT_SUFFIX_RE.test(right)) {
        return true;
      }
    }
  }

  return false;
}

function getWeightedRanges(raw: string): Range[] {
  const stack: OpenToken[] = [];
  const candidates: Range[] = [];
  let angleDepth = 0;

  const openForClose: Record<string, OpenToken["char"]> = {
    ")": "(",
    "}": "{",
    "]": "[",
  };

  const isWrappedWeight = (start: number, end: number) =>
    raw.slice(start + 1, end - 1).trim().length > 0;

  for (let i = 0; i < raw.length; i++) {
    if (isEscaped(raw, i)) continue;

    const ch = raw[i];
    if (ch === "<") {
      angleDepth += 1;
      continue;
    }
    if (ch === ">" && angleDepth > 0) {
      angleDepth -= 1;
      continue;
    }
    if (angleDepth > 0) {
      continue;
    }

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
      const isWeighted =
        open === "(" ? isWeightedExpression(raw, start, end) : isWrappedWeight(start, end);
      if (isWeighted) {
        candidates.push({ start, end });
      }
    }
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
  if (trimmed.includes("<") || trimmed.includes(">")) return false;
  if (trimmed.toLowerCase().startsWith("@preset:")) return false;
  return /[A-Za-z0-9_@]/.test(trimmed);
}

function pushGapSegments(segments: PromptClickableSegment[], raw: string, start: number, end: number) {
  if (start >= end) return;

  let tokenStart = start;
  let parenDepth = 0;
  let bracketDepth = 0;
  let braceDepth = 0;
  let angleDepth = 0;

  const pushToken = (rangeStart: number, rangeEnd: number) => {
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
  };

  for (let i = start; i < end; i++) {
    if (!isEscaped(raw, i)) {
      const ch = raw[i];
      if (ch === "(") parenDepth += 1;
      else if (ch === ")" && parenDepth > 0) parenDepth -= 1;
      else if (ch === "[") bracketDepth += 1;
      else if (ch === "]" && bracketDepth > 0) bracketDepth -= 1;
      else if (ch === "{") braceDepth += 1;
      else if (ch === "}" && braceDepth > 0) braceDepth -= 1;
      else if (ch === "<") angleDepth += 1;
      else if (ch === ">" && angleDepth > 0) angleDepth -= 1;
      else if (
        ch === "," &&
        parenDepth === 0 &&
        bracketDepth === 0 &&
        braceDepth === 0 &&
        angleDepth === 0
      ) {
        pushToken(tokenStart, i);
        pushSegment(segments, i, i + 1, "text", false);
        tokenStart = i + 1;
      }
    }
  }

  pushToken(tokenStart, end);
}

export function getPromptClickableSegments(raw: string): PromptClickableSegment[] {
  if (!raw) return [];

  const segments: PromptClickableSegment[] = [];
  const weightedRanges = getWeightedRanges(raw);
  let cursor = 0;

  for (const range of weightedRanges) {
    pushGapSegments(segments, raw, cursor, range.start);
    pushSegment(segments, range.start, range.end, "weighted", true);
    cursor = range.end;
  }

  pushGapSegments(segments, raw, cursor, raw.length);
  return segments;
}
