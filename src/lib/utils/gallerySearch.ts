/**
 * Gallery search: a small query language over the metadata the gallery has
 * already read from each image.
 *
 *   cat girl             every word must appear (prompt, model, LoRAs, file name)
 *   "red scarf"          an exact phrase
 *   -hat                 must not appear
 *   neg:blurry           negative prompt
 *   model:illustrious    checkpoint      lora:detail   LoRAs
 *   sampler:euler        scheduler:karras             file:0042
 *   seed:1234            steps:>=30      cfg:<6        (numbers take = > >= < <=)
 *
 * Any other `word:value` is plain text, so booru tags such as
 * `rating:explicit` or `artist:foo` search the prompt as typed.
 */

type TextField = "text" | "prompt" | "neg" | "model" | "lora" | "sampler" | "scheduler" | "file";
type NumberField = "seed" | "steps" | "cfg";
type Comparator = "=" | ">" | ">=" | "<" | "<=";

export type GallerySearchTerm =
  | { kind: "text"; field: TextField; value: string; negate: boolean }
  | { kind: "number"; field: NumberField; op: Comparator; value: number; negate: boolean };

/** What a term is matched against: the image's file name and its embedded metadata. */
export interface GallerySearchSubject {
  filename: string;
  metadata?: Record<string, string> | null;
}

const FIELD_ALIASES: Record<string, TextField | NumberField> = {
  prompt: "prompt",
  neg: "neg",
  negative: "neg",
  model: "model",
  checkpoint: "model",
  lora: "lora",
  loras: "lora",
  sampler: "sampler",
  scheduler: "scheduler",
  file: "file",
  filename: "file",
  seed: "seed",
  steps: "steps",
  cfg: "cfg",
};

const NUMBER_FIELDS = new Set<string>(["seed", "steps", "cfg"]);

/** Split on whitespace, keeping "quoted phrases" (and `field:"quoted value"`) together. */
function tokenize(query: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quoted = false;
  for (const ch of query) {
    if (ch === '"') {
      quoted = !quoted;
      current += ch;
    } else if (!quoted && /\s/.test(ch)) {
      if (current) tokens.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  if (current) tokens.push(current);
  return tokens;
}

function unquote(value: string): string {
  return value.replace(/"/g, "").trim().toLowerCase();
}

function parseNumberTerm(field: NumberField, raw: string, negate: boolean): GallerySearchTerm | null {
  const match = /^(>=|<=|>|<|=)?\s*(-?\d+(?:\.\d+)?)$/.exec(raw.trim());
  if (!match) return null;
  return { kind: "number", field, op: (match[1] as Comparator | undefined) ?? "=", value: Number(match[2]), negate };
}

export function parseGallerySearch(query: string): GallerySearchTerm[] {
  const terms: GallerySearchTerm[] = [];
  for (let token of tokenize(query)) {
    let negate = false;
    if (token.length > 1 && token.startsWith("-")) {
      negate = true;
      token = token.slice(1);
    }
    const colon = token.indexOf(":");
    const field = colon > 0 && !token.slice(0, colon).includes('"')
      ? FIELD_ALIASES[token.slice(0, colon).toLowerCase()]
      : undefined;
    if (field) {
      const raw = token.slice(colon + 1);
      if (NUMBER_FIELDS.has(field)) {
        const term = parseNumberTerm(field as NumberField, unquote(raw), negate);
        if (term) terms.push(term);
        continue;
      }
      const value = unquote(raw);
      if (value) terms.push({ kind: "text", field: field as TextField, value, negate });
      continue;
    }
    const value = unquote(token);
    if (value) terms.push({ kind: "text", field: "text", value, negate });
  }
  return terms;
}

function meta(subject: GallerySearchSubject, key: string): string {
  return subject.metadata?.[key]?.toLowerCase() ?? "";
}

function textFor(subject: GallerySearchSubject, field: TextField): string {
  switch (field) {
    case "prompt":
      return meta(subject, "positive_prompt");
    case "neg":
      return meta(subject, "negative_prompt");
    case "model":
      return meta(subject, "model");
    case "lora":
      return meta(subject, "loras");
    case "sampler":
      return meta(subject, "sampler");
    case "scheduler":
      return meta(subject, "scheduler");
    case "file":
      return subject.filename.toLowerCase();
    case "text":
      return [
        meta(subject, "positive_prompt"),
        meta(subject, "model"),
        meta(subject, "loras"),
        subject.filename.toLowerCase(),
      ].join("\n");
  }
}

function compare(actual: number, op: Comparator, expected: number): boolean {
  switch (op) {
    case "=":
      return actual === expected;
    case ">":
      return actual > expected;
    case ">=":
      return actual >= expected;
    case "<":
      return actual < expected;
    case "<=":
      return actual <= expected;
  }
}

function termMatches(term: GallerySearchTerm, subject: GallerySearchSubject): boolean {
  if (term.kind === "text") return textFor(subject, term.field).includes(term.value);
  const raw = subject.metadata?.[term.field];
  const actual = raw === undefined ? NaN : Number(raw);
  return Number.isFinite(actual) && compare(actual, term.op, term.value);
}

/** True when `subject` satisfies every term. An empty query matches everything. */
export function matchesGallerySearch(terms: GallerySearchTerm[], subject: GallerySearchSubject): boolean {
  return terms.every((term) => termMatches(term, subject) !== term.negate);
}
