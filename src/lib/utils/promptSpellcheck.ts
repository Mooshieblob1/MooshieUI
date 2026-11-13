import { getPromptClickableSegments } from "./promptClickableRanges.js";
import { isProseLine } from "./naiOutfits.js";

/**
 * Which tag vocabulary a prompt is checked against. `danbooru` is the plain
 * taglist; `novelai` adds NovelAI's own tokens and syntax on top of it.
 * Natural-language models are not checked at all (the caller skips them).
 */
export type SpellcheckDialect = "danbooru" | "novelai";

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

  // NAI numeric weight: N::name:: or -N::name:: — ends with "::". A span over
  // several tags (`1.2::rain, night::`) is a group, so it counts as known.
  if (/^-?\.?\d/.test(text) && text.endsWith("::")) {
    const sep = text.indexOf("::");
    if (sep === -1) return "";
    const inner = text.slice(sep + 2, -2);
    return /[,()[\]{}]/.test(inner) ? "" : inner.trim();
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

  // {name} or [name], peeling nested emphasis such as {{name}} or [[name]].
  let name = inner;
  while (name.length >= 2 && name[0] === open && name[name.length - 1] === close) {
    name = name.slice(1, -1);
  }
  return /[,()[\]{}]/.test(name) ? "" : name.trim();
}

/**
 * Quality, score and rating tokens the common anime checkpoints were trained
 * on. They are not Danbooru tags, so the taglist alone would flag
 * `masterpiece, best quality` in nearly every prompt.
 */
const QUALITY_TOKENS = new Set([
  "masterpiece",
  "best quality",
  "amazing quality",
  "great quality",
  "high quality",
  "good quality",
  "normal quality",
  "average quality",
  "low quality",
  "bad quality",
  "worst quality",
  "very aesthetic",
  "aesthetic",
  "displeasing",
  "very displeasing",
  "very awa",
  "newest",
  "recent",
  "mid",
  "early",
  "old",
  "oldest",
  "safe",
  "general",
  "sensitive",
  "nsfw",
  "questionable",
  "explicit",
  "source anime",
  "source cartoon",
  "source furry",
  "source pony",
  "rating safe",
  "rating questionable",
  "rating explicit",
]);

/**
 * NovelAI's own trained tokens (V4.5 quality toggle, V5 custom tags and
 * dataset prefixes, the renamed staples, and the bare girl / boy / other that
 * opens a character box). See the V5 rules in `naiPrompt.ts`.
 */
const NAI_TOKENS = new Set([
  "location",
  "no text",
  "depthness",
  "attractive male",
  "low complexity",
  "medium complexity",
  "high complexity",
  "ultra complexity",
  "transparent background",
  "has alpha",
  "alpha transparency",
  "meta:novel era",
  "meta:golden era",
  "visual novel art",
  "visual novel bg",
  "visual novel cg",
  "visual novel chibi",
  "visual novel sprite",
  "fur dataset",
  "background dataset",
  "peace sign",
  "double peace",
  "bar eyes",
  "neutral face",
  "character image",
  "girl",
  "boy",
  "other",
]);

function normalizeName(name: string): string {
  return name.toLowerCase().replace(/\\/g, "").replace(/_/g, " ").replace(/\s+/g, " ").trim();
}

/**
 * True when `name` is a tag the active model knows: the taglist, the shared
 * quality vocabulary, and for NovelAI its own tokens and prefixes.
 */
export function isKnownTagName(
  name: string,
  isKnown: (name: string) => boolean,
  dialect: SpellcheckDialect,
): boolean {
  let n = normalizeName(name);
  if (!n) return true;
  if (dialect === "novelai") {
    // `artist:` marks the token as an artist on purpose, and NAI knows far
    // more artists than the bundled list.
    if (n.startsWith("artist:")) return true;
    // Interaction tags: source#hug / target#hug / mutual#hug.
    const interaction = n.match(/^(?:source|target|mutual)#\s*(.*)$/);
    if (interaction) {
      n = interaction[1];
      if (!n) return true;
    }
    if (NAI_TOKENS.has(n)) return true;
    // Explicit zero counts (0girls) and the like.
    if (/^\d+(?:girl|boy|other)s?$/.test(n)) return true;
  }
  if (QUALITY_TOKENS.has(n)) return true;
  if (/^year \d{4}$/.test(n)) return true;
  if (/^score \d(?: up)?$/.test(n)) return true;
  return isKnown(n);
}

/** A tag-list piece that reads as a sentence fragment rather than a tag. */
function looksLikeProse(text: string): boolean {
  const words = text.split(/\s+/).filter(Boolean).length;
  if (words > 5) return true;
  return words >= 4 && /[.!?](?:["')]?\s|["')]?$)/.test(text);
}

/** Start/end offsets of every line in `raw`, newline excluded. */
function lineRanges(raw: string): { start: number; end: number }[] {
  const out: { start: number; end: number }[] = [];
  let start = 0;
  for (let i = 0; i <= raw.length; i++) {
    if (i === raw.length || raw[i] === "\n") {
      out.push({ start, end: i });
      start = i + 1;
    }
  }
  return out;
}

/** Offset where a NovelAI `Text:` lettering block starts, or -1. */
function naiTextBlockStart(raw: string): number {
  const m = /(?:^|[\n,])[ \t]*text[ \t]*:/i.exec(raw);
  return m ? m.index : -1;
}

/**
 * Tag/weighted tokens whose name is not a known tag, excluding the token the
 * caret is currently inside (so in-progress typing is never flagged).
 * Pass caretOffset = -1 to exclude nothing (e.g. when a selection is active).
 *
 * Prose is never checked: a line that reads as sentences is skipped whole, as
 * is a long comma-separated fragment, since both are natural language mixed
 * into a tag prompt (NovelAI V5's hybrid format, Anima captions). In the
 * NovelAI dialect a `Text:` lettering block is skipped too.
 */
export function getUnknownTagRanges(
  raw: string,
  isKnown: (name: string) => boolean,
  caretOffset: number,
  dialect: SpellcheckDialect = "danbooru",
): UnknownTagRange[] {
  if (!raw) return [];
  const proseLines = lineRanges(raw).filter((line) => isProseLine(raw.slice(line.start, line.end)));
  const inProseLine = (offset: number) => proseLines.some((l) => offset >= l.start && offset <= l.end);
  const textBlockAt = dialect === "novelai" ? naiTextBlockStart(raw) : -1;

  const out: UnknownTagRange[] = [];
  for (const seg of getPromptClickableSegments(raw)) {
    if (seg.kind !== "tag" && seg.kind !== "weighted") continue;
    const segText = raw.slice(seg.start, seg.end);
    const multiline = segText.includes("\n");
    // A weighted span across lines is a group, not one tag.
    if (multiline && seg.kind === "weighted") continue;

    // A plain tag segment can run across a line break (the parser only splits
    // on commas), so each line of it is checked as its own piece.
    let pieceStart = seg.start;
    for (const part of segText.split("\n")) {
      const lead = part.length - part.trimStart().length;
      const start = pieceStart + lead;
      const end = pieceStart + part.trimEnd().length;
      pieceStart += part.length + 1;
      if (end <= start) continue;
      if (caretOffset >= start && caretOffset <= end) continue;
      if (textBlockAt !== -1 && end > textBlockAt) continue;
      if (inProseLine(start)) continue;
      const text = raw.slice(start, end);
      if (looksLikeProse(text)) continue;
      const name = multiline ? text : extractName(raw, start, end, seg.kind === "weighted");
      if (!isKnownTagName(name, isKnown, dialect)) {
        out.push({ start, end, name });
      }
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
