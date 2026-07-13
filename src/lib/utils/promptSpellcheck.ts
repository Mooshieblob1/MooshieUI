import { getPromptClickableSegments } from "./promptClickableRanges.js";

export interface UnknownTagRange {
  start: number;
  end: number;
  name: string;
}

export interface SpellcheckPiece {
  start: number;
  end: number;
  unknown: boolean;
  name: string | null;
}

/**
 * Damerau-Levenshtein (optimal string alignment) distance with early exit.
 * Returns `max + 1` once the minimum achievable distance is already > max,
 * so callers can cheaply reject far-apart strings.
 */
export function damerauLevenshtein(a: string, b: string, max: number): number {
  const al = a.length;
  const bl = b.length;
  if (Math.abs(al - bl) > max) return max + 1;
  if (al === 0) return bl <= max ? bl : max + 1;
  if (bl === 0) return al <= max ? al : max + 1;

  let prevPrev = new Array<number>(bl + 1).fill(0);
  let prev = new Array<number>(bl + 1);
  let curr = new Array<number>(bl + 1);
  for (let j = 0; j <= bl; j++) prev[j] = j;

  for (let i = 1; i <= al; i++) {
    curr[0] = i;
    let rowMin = curr[0];
    for (let j = 1; j <= bl; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      let val = Math.min(
        prev[j] + 1, // deletion
        curr[j - 1] + 1, // insertion
        prev[j - 1] + cost, // substitution
      );
      if (
        i > 1 &&
        j > 1 &&
        a[i - 1] === b[j - 2] &&
        a[i - 2] === b[j - 1]
      ) {
        val = Math.min(val, prevPrev[j - 2] + 1); // transposition
      }
      curr[j] = val;
      if (val < rowMin) rowMin = val;
    }
    if (rowMin > max) return max + 1;
    const tmp = prevPrev;
    prevPrev = prev;
    prev = curr;
    curr = tmp;
  }
  return prev[bl] <= max ? prev[bl] : max + 1;
}

/**
 * Extract the bare tag name from a clickable segment's raw text. Handles every
 * weighted shape the clickable-range parser produces (A1111 `(name:1.2)`, plain
 * `{name}`/`[name]`, InvokeAI trailing `(name)0.5` / `(name)++`, NAI `1.2::name::`).
 * Multi-tag or nested groups return "" so `isKnownTag("")` treats them as known
 * and they are never false-flagged with a red underline.
 */
function extractName(raw: string, start: number, end: number, weighted: boolean): string {
  const text = raw.slice(start, end);
  if (!weighted) return text;

  // NAI numeric weight: N::name:: — starts with a digit, ends with "::".
  if (/^\d/.test(text) && text.endsWith("::")) {
    const sep = text.indexOf("::");
    return sep === -1 ? "" : text.slice(sep + 2, -2).trim();
  }

  const open = text[0];

  // InvokeAI trailing weight/emphasis on a flat group: (name)0.5, (name)++, (name)--.
  // Opens with "(" but does not end with ")".
  if (open === "(" && text[text.length - 1] !== ")") {
    let depth = 0;
    let closeIdx = -1;
    for (let i = 0; i < text.length; i++) {
      if (text[i] === "(") depth += 1;
      else if (text[i] === ")") {
        depth -= 1;
        if (depth === 0) {
          closeIdx = i;
          break;
        }
      }
    }
    if (closeIdx < 1) return "";
    const inner = text.slice(1, closeIdx);
    return /[,()[\]{}]/.test(inner) ? "" : inner.trim();
  }

  // Remaining forms wrap the name in a matching bracket pair.
  const closeFor: Record<string, string> = { "(": ")", "{": "}", "[": "]" };
  const close = closeFor[open];
  if (!close || text[text.length - 1] !== close) return "";

  const inner = text.slice(1, -1);
  if (open === "(") {
    // A1111/NAI (name:1.2) — greedy capture up to the trailing :number.
    const m = inner.match(/^(.*):\d*\.?\d+$/);
    const name = m ? m[1] : inner;
    return /[,()[\]{}]/.test(name) ? "" : name.trim();
  }

  // {name} or [name].
  return /[,()[\]{}]/.test(inner) ? "" : inner.trim();
}

/**
 * Tag/weighted tokens whose name is not a known tag, excluding the token the
 * caret is currently inside (so in-progress typing is never flagged).
 * Pass caretOffset = -1 to exclude nothing (e.g. when a selection is active).
 */
export function getUnknownTagRanges(
  raw: string,
  isKnown: (name: string) => boolean,
  caretOffset: number,
): UnknownTagRange[] {
  if (!raw) return [];
  const out: UnknownTagRange[] = [];
  for (const seg of getPromptClickableSegments(raw)) {
    if (seg.kind !== "tag" && seg.kind !== "weighted") continue;
    if (caretOffset >= seg.start && caretOffset <= seg.end) continue;
    const name = extractName(raw, seg.start, seg.end, seg.kind === "weighted");
    if (!isKnown(name)) {
      out.push({ start: seg.start, end: seg.end, name });
    }
  }
  return out;
}

/** Cover [0, textLength) with alternating known/unknown pieces for the overlay. */
export function buildSpellcheckPieces(
  textLength: number,
  ranges: UnknownTagRange[],
): SpellcheckPiece[] {
  const pieces: SpellcheckPiece[] = [];
  let cursor = 0;
  for (const r of ranges) {
    if (r.start > cursor) {
      pieces.push({ start: cursor, end: r.start, unknown: false, name: null });
    }
    pieces.push({ start: r.start, end: r.end, unknown: true, name: r.name });
    cursor = r.end;
  }
  if (cursor < textLength) {
    pieces.push({ start: cursor, end: textLength, unknown: false, name: null });
  }
  return pieces;
}
