/**
 * Parse, normalize and validate the labelled-field response from the V5 rewrite.
 *
 * The rewrite is asked for `BASE:` / `UC:` / `CHAR n:` blocks and nothing else.
 * Local models ignore that some of the time, so this file is written to salvage
 * a usable answer from a sloppy one rather than to reject it: fences get
 * stripped, a preamble before the first label gets dropped, and an unclosed
 * weight span gets closed. Only what cannot be salvaged becomes a validator
 * problem, and a problem buys exactly one retry.
 *
 * Leaf util. It must not import any store.
 */

import { NAI_PRESET_UC_JUNK, NAI_QUALITY_FILLER } from "./naiPrompt.js";

/** What one full rewrite turn (attempt plus optional retry) produced. */
export interface NaiRewriteResult {
  parsed: NaiParsedResponse;
  /** Validator problems that survived the retry. Advisory: the modal still opens. */
  problems: string[];
}

export interface NaiParsedResponse {
  base: string;
  uc: string;
  /** One entry per `CHAR n:` block, in the order the model returned them. */
  characters: string[];
  /** A trailing `NOTE:` line, shown in the modal banner. Empty when absent. */
  note: string;
}

/** Em dash and en dash. NovelAI treats both as prompt poison. */
const LONG_DASHES = /[–—]/;

const FENCE_LINE = /^\s*```/;
const LABEL_BASE = /^\s*(?:\*\*)?base(?:\s*prompt)?(?:\*\*)?\s*:\s*/i;
const LABEL_UC =
  /^\s*(?:\*\*)?(?:uc|undesired(?:\s*content)?|negative(?:\s*prompt)?)(?:\*\*)?\s*:\s*/i;
const LABEL_CHAR = /^\s*(?:\*\*)?char(?:acter)?\s*\d*(?:\*\*)?\s*:\s*/i;
const LABEL_NOTE = /^\s*(?:\*\*)?note(?:\*\*)?\s*:\s*/i;

type Field = "base" | "uc" | "char" | "note" | null;

/**
 * Pull the labelled fields out of a raw completion.
 *
 * Text before the first recognised label is normally discarded, which is what
 * makes a chatty "Sure, here you go" preamble harmless. It is kept in one case:
 * no BASE label anywhere and nothing else captured as base. A reasoning model
 * routinely emits the scene body first and only labels the character boxes
 * after it, so dropping that preamble drops the whole rewrite. Text with no
 * label anywhere is treated as a bare base prompt, because that is what a model
 * that ignored the output contract almost always produced.
 */
export function parseNaiResponse(raw: string): NaiParsedResponse {
  const lines = (raw ?? "").replace(/\r\n/g, "\n").split("\n");
  const base: string[] = [];
  const uc: string[] = [];
  const chars: string[][] = [];
  const note: string[] = [];

  let field: Field = null;
  let sawLabel = false;
  let sawBaseLabel = false;
  const preamble: string[] = [];

  for (const line of lines) {
    if (FENCE_LINE.test(line)) continue;

    if (LABEL_BASE.test(line)) {
      field = "base";
      sawLabel = true;
      sawBaseLabel = true;
      const rest = line.replace(LABEL_BASE, "");
      if (rest.trim()) base.push(rest);
      continue;
    }
    if (LABEL_UC.test(line)) {
      field = "uc";
      sawLabel = true;
      const rest = line.replace(LABEL_UC, "");
      if (rest.trim()) uc.push(rest);
      continue;
    }
    if (LABEL_CHAR.test(line)) {
      field = "char";
      sawLabel = true;
      chars.push([]);
      const rest = line.replace(LABEL_CHAR, "");
      if (rest.trim()) chars[chars.length - 1].push(rest);
      continue;
    }
    if (LABEL_NOTE.test(line)) {
      field = "note";
      sawLabel = true;
      const rest = line.replace(LABEL_NOTE, "");
      if (rest.trim()) note.push(rest);
      continue;
    }

    if (!sawLabel) {
      preamble.push(line);
      continue;
    }
    switch (field) {
      case "base":
        base.push(line);
        break;
      case "uc":
        uc.push(line);
        break;
      case "char":
        if (chars.length > 0) chars[chars.length - 1].push(line);
        break;
      case "note":
        note.push(line);
        break;
      default:
        break;
    }
  }

  if (!sawLabel) {
    const bare = (raw ?? "").replace(/^\s*```[^\n]*\n?|```\s*$/g, "").trim();
    return { base: bare, uc: "", characters: [], note: "" };
  }

  const joinedBase = joinField(base);
  return {
    // The preamble is the base prompt whenever the model never labelled one:
    // it wrote the scene body first and started labelling at CHAR 1.
    base: joinedBase || (sawBaseLabel ? "" : joinField(stripLeadIn(preamble))),
    uc: joinField(uc),
    characters: chars.map((c) => joinField(c)).filter((c) => c !== ""),
    note: joinField(note),
  };
}

/**
 * Drop the leading junk from a recovered preamble.
 *
 * A model whose label was swallowed upstream leaves the punctuation behind: a
 * bare `:` line where `BASE:` should have been. That is noise in a prompt, and
 * removing it here is safer than loosening `LABEL_BASE` to match a lone colon.
 */
function stripLeadIn(lines: string[]): string[] {
  let i = 0;
  while (i < lines.length && /^[\s:*#>-]*$/.test(lines[i])) i++;
  return lines.slice(i);
}

/** Collapse a captured block, keeping deliberate paragraph breaks in the body. */
function joinField(lines: string[]): string {
  return lines
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/**
 * Close a weight span the model left hanging.
 *
 * `1.5::rain` with no closing `::` applies that weight to everything after it,
 * which is a silent quality cliff rather than an error. An odd count is always
 * a missing close, never a spurious open, because the opener carries the number.
 */
export function normalizeWeightSpans(text: string): string {
  const trimmed = (text ?? "").trim();
  if (!trimmed) return "";
  const count = (trimmed.match(/::/g) ?? []).length;
  return count % 2 === 0 ? trimmed : `${trimmed}::`;
}

/** True when a span token would make novelai.net treat `::` as part of the name. */
function endsInDigit(token: string): boolean {
  return /\d$/.test(token.trim());
}

/**
 * Put digit-suffix names first inside numeric weight spans.
 *
 * NovelAI's website parser reads trailing digits on the last token of a
 * `n::...::` span as the start of an unclosed weight, so `1.2::rain, as109::`
 * breaks when pasted into novelai.net. Generation through the API is fine;
 * users still copy prompts out. If every token ends in a digit, a trailing
 * comma is the remaining fix: `1.2::as109, pigeon666,::`.
 */
export function normalizeNumericSpans(text: string): string {
  if (!text) return text;
  return text.replace(/(-?\d+(?:\.\d+)?)::((?:(?!::)[\s\S])*?)::/g, (_m, weight: string, inner: string) => {
    const tokens = inner
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t !== "");
    // An empty span is not this function's problem; leave it as the model wrote it.
    if (tokens.length === 0) return _m;
    const head = tokens.filter(endsInDigit);
    const rest = tokens.filter((t) => !endsInDigit(t));
    const ordered = [...head, ...rest];
    let body = ordered.join(", ");
    if (ordered.length > 0 && endsInDigit(ordered[ordered.length - 1])) body += ",";
    return `${weight}::${body}::`;
  });
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Split a field at its Text: block. The newline(s) that precede the label stay
 * with the tail, so a caller can trim the head and concatenate the two without
 * gluing the last prompt line onto `Text:`.
 */
function splitTextBlock(text: string): { head: string; tail: string } {
  const match = text.match(/(^|\n)(\s*Text\s*:)/i);
  if (!match || match.index === undefined) return { head: text, tail: "" };
  return { head: text.slice(0, match.index), tail: text.slice(match.index) };
}

/**
 * Comma- or span-bounded tag match. The `m` flag lets `^`/`$` stand in for a
 * newline, so the pattern never has to write one into a string constructor.
 */
function listedTagPattern(tag: string, flags: string): RegExp {
  return new RegExp("(^|,|::)\\s*" + escapeRegExp(tag) + "\\s*(?=,|$|::)", flags);
}

/**
 * Strip listed tags from one line. Returns the line untouched when nothing
 * matched, `null` when the strip emptied it, and otherwise the line with only
 * the delimiters the removal orphaned repaired. Anything the model wrote
 * correctly, including a deliberate trailing comma, is left alone.
 */
function stripTagsFromLine(line: string, tags: readonly string[]): string | null {
  let out = line;
  for (const tag of tags) {
    const re = listedTagPattern(tag, "gi");
    let prev = "";
    while (prev !== out) {
      prev = out;
      out = out.replace(re, "$1");
    }
  }
  if (out === line) return line;
  const hadTrailingComma = /,\s*$/.test(line);
  out = out
    // The removed item was first inside a span: `1.2::, rain::`.
    .replace(/(-?\d+(?:\.\d+)?)::\s*,\s*/g, "$1::")
    // The removed item was last inside a span: `1.2::rain,::`.
    .replace(/,\s*::/g, "::")
    // The span held nothing but filler.
    .replace(/-?\d+(?:\.\d+)?::::/g, "")
    // The removed item sat between two others.
    .replace(/,(?:\s*,)+/g, ",")
    // The removed item was first on the line.
    .replace(/^\s*,\s*/, "")
    .trim();
  if (!hadTrailingComma) out = out.replace(/,\s*$/, "");
  return out === "" ? null : out;
}

/**
 * Drop listed tags as comma (or span) delimited items, leaving the Text: block
 * untouched. Used for quality filler and preset UC junk the model still emits.
 *
 * Works line by line so that a line the strip never touched comes back
 * byte-for-byte, and a line it emptied disappears along with its newline.
 */
export function stripListedTags(text: string, tags: readonly string[]): string {
  if (!text || tags.length === 0) return text;
  const { head, tail } = splitTextBlock(text);
  const lines = head
    .split("\n")
    .map((line) => stripTagsFromLine(line, tags))
    .filter((line): line is string => line !== null);
  const out = lines
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
  return out === "" ? tail.trimStart() : `${out}${tail}`;
}

function fieldContainsListedTag(text: string, tags: readonly string[]): string[] {
  const { head } = splitTextBlock(text);
  return tags.filter((tag) => listedTagPattern(tag, "im").test(head));
}

/** Em dash and en dash break NovelAI gens. A comma is the prescribed substitute. */
export function replaceLongDashes(text: string): string {
  if (!text) return text;
  const { head, tail } = splitTextBlock(text);
  const cleaned = head
    .replace(/[ \t]*[–—][ \t]*/g, ", ")
    .replace(/,(?:[ \t]*,)+/g, ",")
    .replace(/[ \t]+$/gm, "")
    .trim();
  return `${cleaned}${tail}`;
}

/**
 * Salvage the two ways a model numbers a character box.
 *
 * Counts belong in BASE; a leading `1girl` or `Character 1:` is a formatting
 * miss, not a different character, and is cheaper to repair than to retry.
 */
export function normalizeCharIdentity(box: string): string {
  let text = (box ?? "").trim();
  if (!text) return "";
  text = text.replace(/^(?:character|char)\s*\d+\s*[:,.\-–—]?\s*/i, "");
  text = text.replace(
    /^(\d+)\s*(girls?|boys?|others?)\b/i,
    (_m, _n: string, noun: string) => {
      const lower = noun.toLowerCase();
      if (lower.startsWith("girl")) return "girl";
      if (lower.startsWith("boy")) return "boy";
      return "other";
    },
  );
  return text.trim();
}

/**
 * One field through every repair, in dependency order: dashes become commas,
 * a hanging span is closed, filler comes out, and only then are digit-suffix
 * names reordered, so the trailing comma that step may add is never mistaken
 * for a strip artefact and removed again.
 */
function normalizeField(text: string, extraTags: readonly string[] = []): string {
  return normalizeNumericSpans(
    stripListedTags(normalizeWeightSpans(replaceLongDashes(text)), [
      ...NAI_QUALITY_FILLER,
      ...extraTags,
    ]),
  );
}

/** Run the normalizer over every field of a parsed response. */
export function normalizeNaiResponse(parsed: NaiParsedResponse): NaiParsedResponse {
  return {
    base: normalizeField(parsed.base),
    uc: normalizeField(parsed.uc, NAI_PRESET_UC_JUNK),
    characters: parsed.characters.map((box) => normalizeField(normalizeCharIdentity(box))),
    note: parsed.note.trim(),
  };
}

/** A count tag that belongs in the base prompt and nowhere else. */
const COUNT_TAG = /(?:^|[,\s])(\d+\s*(?:girls?|boys?|others?))(?:$|[,\s])/i;
/** The model numbering a box instead of typing it. */
const NUMBERED_BOX = /^\s*(?:character|char)\s*\d/i;

/**
 * Everything worth spending a retry on.
 *
 * Anything the normalizer already fixed is deliberately absent here: a validator
 * problem means the response is wrong in a way no local edit can repair.
 */
export function validateNaiResponse(parsed: NaiParsedResponse): string[] {
  const problems: string[] = [];
  const all = [parsed.base, parsed.uc, ...parsed.characters];

  if (!parsed.base.trim()) {
    problems.push("The BASE field was empty. Return a BASE: field with the full base prompt.");
  }

  if (all.some((t) => LONG_DASHES.test(t))) {
    problems.push(
      "A field contained an em dash or an en dash. Remove every one of them and use commas or periods.",
    );
  }

  if (/```/.test(parsed.base) || /```/.test(parsed.uc)) {
    problems.push("The answer contained a markdown fence. Return the labelled fields as plain text.");
  }

  for (const [i, box] of parsed.characters.entries()) {
    if (COUNT_TAG.test(box)) {
      problems.push(
        `CHAR ${i + 1} contained a count tag such as 1girl. Counts belong in BASE only. Start the box with girl, boy or other.`,
      );
    }
    if (NUMBERED_BOX.test(box)) {
      problems.push(
        `CHAR ${i + 1} started with a numbered label. Start the box with girl, boy or other instead.`,
      );
    }
  }

  // Unconditional: the model is never the one who writes the quality stack.
  // With NovelAI's toggle on this would double it, and with the toggle off the
  // user has asked for no stack at all. Checked on every field because models
  // also dump filler into UC and character boxes.
  const fillerFields: Array<[string, string]> = [
    ["BASE", parsed.base],
    ["UC", parsed.uc],
    ...parsed.characters.map((box, i): [string, string] => [`CHAR ${i + 1}`, box]),
  ];
  for (const [label, text] of fillerFields) {
    const filler = fieldContainsListedTag(text, NAI_QUALITY_FILLER);
    if (filler.length > 0) {
      problems.push(
        `${label} contained quality filler (${filler.join(", ")}). NovelAI's quality toggle owns that stack, so remove it.`,
      );
    }
  }

  const ucJunk = fieldContainsListedTag(parsed.uc, NAI_PRESET_UC_JUNK);
  if (ucJunk.length > 0) {
    problems.push(
      `UC restated the preset (${ucJunk.join(", ")}). Keep custom motif undesirables only.`,
    );
  }

  // A blank line inside a Text: block starts a second string rather than ending
  // the block, so "is there anything after it" is not a question the shape can
  // answer. What it can answer is whether a later segment is prompt rather than
  // lettering: weight spans, emphasis markers and count tags never appear in
  // words that are meant to be rendered into the image.
  const textIndex = parsed.base.search(/(?:^|\n)\s*Text\s*:/i);
  if (textIndex !== -1) {
    const trailing = parsed.base
      .slice(textIndex)
      .split(/\n\s*\n/)
      .slice(1);
    if (trailing.some((segment) => /::|[{}[\]]|\b\d+(?:girls?|boys?|others?)\b/i.test(segment))) {
      problems.push(
        "Prompt content followed the Text: block in BASE. Everything after Text: is rendered as lettering, so that block must come last.",
      );
    }
  }

  if (!balanced(parsed.base, "{", "}") || !balanced(parsed.base, "[", "]")) {
    problems.push("BASE had unbalanced { } or [ ] emphasis brackets. Close every one you open.");
  }

  return problems;
}

function balanced(text: string, open: string, close: string): boolean {
  let depth = 0;
  for (const ch of text) {
    if (ch === open) depth++;
    else if (ch === close) depth--;
    if (depth < 0) return false;
  }
  return depth === 0;
}

/** The corrective turn appended to the user message for the single retry. */
export function naiRetryInstruction(problems: string[]): string {
  return `Your previous answer had these problems:

${problems.map((p) => `- ${p}`).join("\n")}

Rewrite the prompt again, fixing every one of them. Answer with the labelled fields only.`;
}
