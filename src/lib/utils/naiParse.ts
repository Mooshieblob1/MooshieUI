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
import { fieldTags, isProseLine, tagKind } from "./naiOutfits.js";
import { layoutProseProblems, tidyNaiLayout } from "./naiLayout.js";

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
  /**
   * The `CHAR n UC:` line of each box, aligned with `characters`: that box's
   * own undesired content, or null when the rewrite wrote none for it.
   */
  characterUcs: (string | null)[];
  /** A trailing `NOTE:` line, shown in the modal banner. Empty when absent. */
  note: string;
  /**
   * The `CANON:` field: one line per way a saved canon character's card
   * differs from canon, so the modal can ask which side should win. Empty
   * when absent.
   */
  canon: string[];
  /**
   * The `MEMORY:` field: facts about saved characters to save to their memory
   * files, one `name | file.md | fact` line each. Empty when absent.
   */
  memory: string[];
}

/**
 * Write prompt fields back out in the labelled shape a rewrite answers in.
 *
 * The session records this instead of the model's raw answer, so that a later
 * turn revises the prompt the user actually applied: the raw text still holds
 * whatever the cleanup after parsing took out, and its MEMORY, CANON and NOTE
 * lines are one-off remarks that should not be replayed as history. Empty
 * boxes are left out, which is how the format says a character is absent.
 */
export function serializeNaiFields(fields: {
  base: string;
  uc: string;
  characters: readonly string[];
  characterUcs?: readonly (string | null | undefined)[];
}): string {
  const base = fields.base.trim();
  if (!base) return "";
  const parts = [`BASE:\n${base}`, `UC:\n${fields.uc.trim()}`.trimEnd()];
  let n = 0;
  fields.characters.forEach((box, i) => {
    const text = box.trim();
    if (!text) return;
    n += 1;
    parts.push(`CHAR ${n}:\n${text}`);
    const uc = fields.characterUcs?.[i]?.trim();
    if (uc) parts.push(`CHAR ${n} UC:\n${uc}`);
  });
  return parts.join("\n\n");
}

/** Em dash and en dash. NovelAI treats both as prompt poison. */
const LONG_DASHES = /[–—]/;

const FENCE_LINE = /^\s*```/;
const LABEL_BASE = /^\s*(?:\*\*)?base(?:\s*prompt)?(?:\*\*)?\s*:\s*/i;
const LABEL_UC =
  /^\s*(?:\*\*)?(?:uc|undesired(?:\s*content)?|negative(?:\s*prompt)?)(?:\*\*)?\s*:\s*/i;
const LABEL_CHAR = /^\s*(?:\*\*)?char(?:acter)?\s*\d*(?:\*\*)?\s*:\s*/i;
/** A box's own undesired content: `CHAR 2 UC:`. Checked before `LABEL_CHAR`. */
const LABEL_CHAR_UC =
  /^\s*(?:\*\*)?char(?:acter)?\s*(\d*)\s*(?:uc|undesired(?:\s*content)?|negative(?:\s*prompt)?)(?:\*\*)?\s*:\s*/i;
const LABEL_NOTE = /^\s*(?:\*\*)?note(?:\*\*)?\s*:\s*/i;
const LABEL_CANON = /^\s*(?:\*\*)?canon(?:\s*conflicts?)?(?:\*\*)?\s*:\s*/i;
const LABEL_MEMORY = /^\s*(?:\*\*)?memory(?:\*\*)?\s*:\s*/i;

type Field = "base" | "uc" | "char" | "charUc" | "note" | "canon" | "memory" | null;

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
  // Keyed by the box it belongs to, so a UC line written after the wrong box
  // or numbered out of order still lands on its own character.
  const charUcs = new Map<number, string[]>();
  let ucTarget = -1;
  const note: string[] = [];
  const canon: string[] = [];
  const memory: string[] = [];

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
    const charUc = line.match(LABEL_CHAR_UC);
    if (charUc) {
      field = "charUc";
      sawLabel = true;
      const n = Number.parseInt(charUc[1], 10);
      ucTarget = Number.isFinite(n) && n > 0 ? n - 1 : chars.length - 1;
      if (!charUcs.has(ucTarget)) charUcs.set(ucTarget, []);
      const rest = line.replace(LABEL_CHAR_UC, "");
      if (rest.trim()) charUcs.get(ucTarget)!.push(rest);
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

    if (LABEL_CANON.test(line)) {
      field = "canon";
      sawLabel = true;
      const rest = line.replace(LABEL_CANON, "");
      if (rest.trim()) canon.push(rest);
      continue;
    }

    if (LABEL_MEMORY.test(line)) {
      field = "memory";
      sawLabel = true;
      const rest = line.replace(LABEL_MEMORY, "");
      if (rest.trim()) memory.push(rest);
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
      case "charUc":
        charUcs.get(ucTarget)?.push(line);
        break;
      case "note":
        note.push(line);
        break;
      case "canon":
        canon.push(line);
        break;
      case "memory":
        memory.push(line);
        break;
      default:
        break;
    }
  }

  if (!sawLabel) {
    const bare = (raw ?? "").replace(/^\s*```[^\n]*\n?|```\s*$/g, "").trim();
    return { base: bare, uc: "", characters: [], characterUcs: [], note: "", canon: [], memory: [] };
  }

  const joinedBase = joinField(base);
  const boxes = chars
    .map((c, i) => {
      const uc = charUcs.get(i);
      return { prompt: joinField(c), uc: uc ? blankToEmpty(joinField(uc)) : null };
    })
    .filter((c) => !isBlankBox(c.prompt));
  return {
    // The preamble is the base prompt whenever the model never labelled one:
    // it wrote the scene body first and started labelling at CHAR 1.
    base: joinedBase || (sawBaseLabel ? "" : joinField(stripLeadIn(preamble))),
    uc: joinField(uc),
    characters: boxes.map((c) => c.prompt),
    characterUcs: boxes.map((c) => c.uc),
    note: joinField(note),
    canon: canonLines(canon),
    memory: canonLines(memory),
  };
}

/** One difference per line, bullets and blanks dropped. */
function canonLines(lines: string[]): string[] {
  return lines.map((l) => l.replace(/^\s*(?:[-*•]|\d+[.)])\s*/, "").trim()).filter(Boolean);
}

/**
 * Placeholders a model returns instead of leaving a character box out.
 *
 * The edit turn writes `(empty)` under a box that has no text, and a model
 * asked to return every field will sometimes echo that marker back rather than
 * omit the block. Taken literally it becomes a character box whose prompt is
 * the word "(empty)", which NovelAI would dutifully render. Treated as the
 * blank it means, the block drops out and the box is offered for removal
 * instead, which is what a shorter cast is supposed to do.
 *
 * Deliberately not on this list: "unchanged". A model writing that is asking to
 * keep the box, and reading it as a blank would delete the very thing it meant
 * to protect.
 */
const BLANK_BOX = /^[\s.\-*_]*(?:\(?\s*(?:empty|none|n\/a|blank|removed|deleted)\s*\)?)?[\s.\-*_]*$/i;

function isBlankBox(box: string): boolean {
  return BLANK_BOX.test(box);
}

/** A box UC the model marked as empty is an empty UC, not the word "none". */
function blankToEmpty(uc: string): string {
  return isBlankBox(uc) ? "" : uc;
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

/**
 * Run the normalizer over every field of a parsed response, then the layout
 * cleanup across fields (`tidyNaiLayout`): object-relative tags out of the
 * boxes, assertions such as `opaque` out of every field, and no UC that
 * negates an object the scene contains.
 */
export function normalizeNaiResponse(parsed: NaiParsedResponse): NaiParsedResponse {
  const layout = tidyNaiLayout({
    base: normalizeField(parsed.base),
    uc: normalizeField(parsed.uc, NAI_PRESET_UC_JUNK),
    boxes: parsed.characters.map((box) => normalizeField(normalizeCharIdentity(box))),
    ucs: parsed.characterUcs.map((uc) => (uc === null ? null : normalizeField(uc, NAI_PRESET_UC_JUNK))),
  });
  return {
    base: layout.base,
    uc: layout.uc,
    characters: layout.boxes,
    characterUcs: layout.ucs,
    note: parsed.note.trim(),
    canon: parsed.canon,
    memory: parsed.memory,
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

  problems.push(...outfitIsolationProblems(parsed));

  const negations = layoutProseProblems(parsed.base);
  if (negations.length > 0) {
    problems.push(
      `BASE prose says what the layout is not (${negations.map((n) => `"${n}"`).join(", ")}). Naming a failure such as transparency or sitting on the table invites it. Delete that wording and state only the layout you want, in one short present-tense sentence.`,
    );
  }

  if (!balanced(parsed.base, "{", "}") || !balanced(parsed.base, "[", "]")) {
    problems.push("BASE had unbalanced { } or [ ] emphasis brackets. Close every one you open.");
  }

  return problems;
}

/**
 * Interactions the base prose can describe that V5 binds with `source#` /
 * `target#` / `mutual#` tags. Kept to clear, physical ones: a validator problem
 * costs a retry, so a vague verb is not worth one.
 */
const INTERACTION_WORDS =
  /\b(?:hug(?:s|ging|ged)?|embrac(?:e|es|ing)|kiss(?:es|ing|ed)?|hold(?:s|ing)? hands|holding (?:her|his|their) hand|headpat|pat(?:s|ting)? (?:her|his|their|the other'?s?) head|carr(?:y|ies|ying) (?:her|him|them)|piggyback|princess carry|feed(?:s|ing) (?:her|him|them)|high five|point(?:s|ing) at (?:her|him|them)|pok(?:e|es|ing) (?:her|his|their) cheek|lap pillow|slap(?:s|ping)? (?:her|him|them)|punch(?:es|ing)? (?:her|him|them))\b/i;

/**
 * The outfit isolation rules that can be checked without a judgement call.
 *
 * - The BASE tag line restating a garment or hair tag that a CHAR box holds.
 *   The base belongs to nobody in particular, so an outfit there is free for
 *   any character to wear. The prose is not checked: one placing sentence per
 *   character may name a garment to tell them apart.
 * - A clear interaction in the prose with no interaction tags in any box.
 * - A `source#` with no `target#` partner, or the other way round.
 */
function outfitIsolationProblems(parsed: NaiParsedResponse): string[] {
  const problems: string[] = [];
  if (parsed.characters.length < 2) return problems;

  const boxTags = parsed.characters.map((box) => new Set(fieldTags(box).map((t) => t.norm)));
  const restated: string[] = [];
  for (const tag of fieldTags(parsed.base)) {
    const kind = tagKind(tag.norm);
    if (kind !== "clothing" && kind !== "hair") continue;
    if (restated.includes(tag.norm)) continue;
    if (boxTags.some((set) => set.has(tag.norm))) restated.push(tag.norm);
  }
  if (restated.length > 0) {
    problems.push(
      `BASE restated character details that belong in a CHAR block (${restated.join(", ")}). Take them out of the BASE tag line and keep them only in the box of the character they describe.`,
    );
  }

  const prose = parsed.base
    .split(/(?:^|\n)\s*Text\s*:/i)[0]
    .split("\n")
    .filter(isProseLine)
    .join(" ");
  const tagged = parsed.characters.some((box) => /\b(?:source|target|mutual)#/i.test(box));
  const action = prose.match(INTERACTION_WORDS);
  if (action && !tagged) {
    problems.push(
      `BASE describes an interaction (${action[0]}) but no CHAR block has interaction tags. Put source#action in the actor's box and target#action in the recipient's, or mutual#action in both.`,
    );
  }

  const sources = new Set<string>();
  const targets = new Set<string>();
  for (const box of parsed.characters) {
    for (const m of box.matchAll(/\b(source|target)#([^,\n:{}[\]]+)/gi)) {
      (m[1].toLowerCase() === "source" ? sources : targets).add(m[2].trim().toLowerCase());
    }
  }
  const unpaired = [
    ...[...sources].filter((a) => !targets.has(a)).map((a) => `source#${a}`),
    ...[...targets].filter((a) => !sources.has(a)).map((a) => `target#${a}`),
  ];
  if (unpaired.length > 0) {
    problems.push(
      `${unpaired.join(", ")} had no partner tag. Every source#action needs target#action in the other character's box.`,
    );
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

Correct that answer: fix every one of these and change nothing else. Return the whole prompt again, every field, with the labelled fields only.`;
}
