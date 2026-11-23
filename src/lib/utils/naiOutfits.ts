/**
 * Outfit isolation for NovelAI multi-character prompts.
 *
 * The commonest V5 multi-character failure is clothes jumping between boxes:
 * the girl in the hoodie ends up in her friend's skirt. Two things invite it.
 * An outfit restated in the base prompt belongs to nobody in particular, so the
 * model hands it to whichever character it likes. And two boxes describing
 * similar bodies have nothing pushing their clothes apart. This file finds both
 * and builds the cheap fix for the second: each character's undesired content
 * lists the other characters' distinctive garments ("opposite-outfit UC").
 *
 * Deterministic and tag-level. It never reads the natural language body as tags,
 * because the base prose is allowed one placing sentence per character ("the girl
 * in the white hoodie stands on the left"), and naming a garment there is the
 * whole point of that sentence.
 *
 * Leaf util. It must not import any store.
 */

/**
 * Garment and accessory nouns, matched against a tag's last word. Last word
 * only, so action tags that merely mention clothing ("skirt lift", "hand in
 * pocket") are not mistaken for something being worn.
 */
const GARMENT_NOUNS = new Set([
  "shirt", "t-shirt", "blouse", "top", "camisole", "sweater", "pullover", "hoodie",
  "cardigan", "jacket", "coat", "overcoat", "blazer", "vest", "waistcoat", "parka",
  "windbreaker", "cape", "capelet", "cloak", "poncho", "robe", "kimono", "yukata",
  "hakama", "haori", "dress", "sundress", "gown", "skirt", "miniskirt", "pants",
  "trousers", "jeans", "shorts", "leggings", "tights", "pantyhose", "stockings",
  "thighhighs", "kneehighs", "socks", "legwear", "uniform", "serafuku", "gakuran",
  "suit", "tuxedo", "necktie", "tie", "bowtie", "neckerchief", "ribbon", "scarf",
  "muffler", "shawl", "gloves", "glove", "mittens", "boots", "shoes", "sneakers",
  "sandals", "heels", "loafers", "slippers", "hat", "cap", "beret", "beanie",
  "helmet", "hood", "headband", "hairband", "hairclip", "headdress", "headwear",
  "apron", "bikini", "swimsuit", "leotard", "bodysuit", "armor", "armour", "belt",
  "suspenders", "collar", "choker", "necklace", "pendant", "earrings", "bracelet",
  "glasses", "eyewear", "sunglasses", "goggles", "mask", "veil", "tiara", "crown",
  "bra", "panties", "underwear", "lingerie", "corset", "garter", "garters",
  "pajamas", "overalls", "jumpsuit", "cheongsam", "hanfu", "dirndl", "sleeves",
  "wristband", "armband", "sash", "obi", "bandana", "earmuffs", "headphones",
  "backpack", "bag", "satchel", "outfit", "clothes", "costume", "attire", "maid",
  "nun", "habit", "jersey", "tracksuit", "raincoat", "bloomers",
  "buruma", "fundoshi", "sarashi", "loincloth", "pauldrons", "gauntlets",
]);

/** Tags whose last word is a garment noun without anything being worn. */
const NOT_CLOTHING = new Set(["girl on top", "boy on top", "on top", "no bra", "no panties", "topless", "bottomless"]);

/** Hairstyle words that mark a hair tag even without the word "hair". */
const HAIRSTYLE_WORDS = new Set([
  "ponytail", "twintails", "braid", "braids", "bun", "buns", "bob", "ahoge", "bangs",
  "sidelocks", "pigtails", "drills", "afro", "mohawk", "bald", "dreadlocks",
  "hime cut", "bob cut", "buzz cut", "crew cut", "undercut",
]);

/** Hair length tags nearly every character carries, too common to flag as a shared trait. */
const COMMON_HAIR = new Set([
  "long hair", "short hair", "medium hair", "very long hair", "absurdly long hair",
  "very short hair",
]);

/** Expression tags that end in "eyes" without being an eye colour. */
const EYE_EXPRESSIONS = new Set([
  "closed eyes", "half-closed eyes", "wide eyes", "narrowed eyes", "one eye closed",
  "empty eyes", "rolling eyes", "covering eyes", "glowing eyes", "crazy eyes",
  "teary eyes", "sparkling eyes", "tired eyes", "sleepy eyes", "shaded eyes",
]);

/** Body descriptors that are not hair or eyes. */
const BODY_WORDS = /\b(?:breasts?|skin|tan|tanned|petite|tall|short|muscular|curvy|slim|chubby|plump|thick thighs|wide hips|flat chest|ears|tail|horns|wings|freckles|mole)\b/;

/** Words that place a character in the frame. */
export const POSITION_WORDS =
  /\b(?:left|right|center|centre|middle|foreground|background|front|behind|far side|near side)\b/i;

export type TagKind = "clothing" | "hair" | "body" | "other";

/** One tag as written, plus the lowercased bare form used to compare it. */
export interface NaiTag {
  /** Exactly as it appears in the field, emphasis and all. */
  raw: string;
  /** Lowercase, emphasis and weight markers removed, underscores as spaces. */
  norm: string;
}

/**
 * Strip emphasis and weight markers from one tag, for comparison only.
 * `{white hoodie}`, `[white hoodie]` and `1.2::white hoodie::` all become
 * `white hoodie`.
 */
export function bareTag(tag: string): string {
  return tag
    .replace(/-?\d+(?:\.\d+)?::/g, "")
    .replace(/::/g, "")
    .replace(/[{}[\]]/g, "")
    .replace(/_/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase();
}

/** The tag with any trailing danbooru qualifier dropped: `ribbon (hair)` stays a ribbon. */
function lastWord(norm: string): string {
  const words = norm.replace(/\s*\([^)]*\)\s*$/, "").split(" ");
  return words[words.length - 1] ?? "";
}

/** What kind of trait a bare tag describes. */
export function tagKind(norm: string): TagKind {
  if (!norm || norm.includes("#")) return "other";
  const last = lastWord(norm);
  if (!NOT_CLOTHING.has(norm) && (GARMENT_NOUNS.has(last) || /wear$/.test(last))) return "clothing";
  if (/\bhair\b/.test(norm) || HAIRSTYLE_WORDS.has(norm) || HAIRSTYLE_WORDS.has(last)) return "hair";
  if (/\beyes?\b/.test(norm) && !EYE_EXPRESSIONS.has(norm)) return "body";
  if (BODY_WORDS.test(norm)) return "body";
  return "other";
}

/** True when a line reads as prose rather than a comma separated tag list. */
export function isProseLine(line: string): boolean {
  const text = line.trim();
  if (!text) return false;
  if (/[.!?]["')]?\s*$/.test(text) || /[.!?]\s+[A-Z]/.test(text)) return true;
  const parts = text.split(",").map((p) => p.trim()).filter(Boolean);
  const words = text.split(/\s+/).length;
  return parts.length > 0 && words / parts.length > 5;
}

/**
 * The comma separated tags of a field, in order.
 *
 * Stops at a `Text:` block (lettering, not prompt) and skips prose lines. Tags
 * inside a negative span (`-1::hat::`) are already negatives and are left out,
 * since they say what a character is not wearing.
 */
export function fieldTags(text: string): NaiTag[] {
  if (!text) return [];
  const textAt = text.search(/(?:^|\n)\s*Text\s*:/i);
  const head = textAt === -1 ? text : text.slice(0, textAt);
  const tags: NaiTag[] = [];
  for (const line of head.split("\n")) {
    if (isProseLine(line)) continue;
    const positive = line.replace(/-\d+(?:\.\d+)?::(?:(?!::)[\s\S])*?::/g, "");
    for (const part of positive.split(",")) {
      const norm = bareTag(part);
      if (norm) tags.push({ raw: part.trim(), norm });
    }
  }
  return tags;
}

/** The bare clothing tags of a field, deduplicated, in order. */
export function outfitTags(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const tag of fieldTags(text)) {
    if (tagKind(tag.norm) !== "clothing" || seen.has(tag.norm)) continue;
    seen.add(tag.norm);
    out.push(tag.norm);
  }
  return out;
}

function words(norm: string): Set<string> {
  return new Set(norm.split(" ").filter(Boolean));
}

function isSubset(a: Set<string>, b: Set<string>): boolean {
  for (const w of a) if (!b.has(w)) return false;
  return true;
}

/**
 * Whether `tag` from another character would fight this character's own outfit
 * if it went into their undesired content.
 *
 * True for the same garment, and for a plainer form of one they wear: `hoodie`
 * cannot go into the UC of a girl in a `black hoodie`. A more specific form can,
 * so `white hoodie` may go into hers, which is exactly what keeps the two
 * hoodies different colours.
 */
function clashesWithOwn(tag: string, own: string[]): boolean {
  const tw = words(tag);
  return own.some((o) => o === tag || isSubset(tw, words(o)));
}

/** Most opposite-outfit tags one box's undesired content gets, so a big cast does not flood it. */
export const OPPOSITE_UC_LIMIT = 10;

/**
 * The other characters' distinctive garments for each box, minus anything that
 * box already wears or already avoids.
 *
 * Empty for every box when there is only one character, or when everyone wears
 * the same thing.
 */
export function oppositeOutfitTags(boxes: string[], ucs: string[]): string[][] {
  const outfits = boxes.map(outfitTags);
  return boxes.map((_, i) => {
    if (boxes.length < 2) return [];
    const own = outfits[i];
    const avoided = fieldTags(ucs[i] ?? "").map((t) => t.norm);
    // Already avoided, as written or in a plainer form: `school uniform` in the
    // UC covers `navy school uniform`.
    const covered = (tag: string) => avoided.some((a) => isSubset(words(a), words(tag)));
    const picked: string[] = [];
    for (const [j, other] of outfits.entries()) {
      if (j === i) continue;
      for (const tag of other) {
        if (picked.length >= OPPOSITE_UC_LIMIT) break;
        if (covered(tag) || picked.includes(tag) || clashesWithOwn(tag, own)) continue;
        picked.push(tag);
      }
    }
    return picked;
  });
}

/** Append tags to a comma separated field, keeping what is there untouched. */
export function appendTags(field: string, tags: string[]): string {
  if (tags.length === 0) return field;
  const current = (field ?? "").trim().replace(/,\s*$/, "");
  return current ? `${current}, ${tags.join(", ")}` : tags.join(", ");
}

/**
 * Each box's undesired content with the opposite-outfit tags added.
 *
 * `ucs[i]` is what the box's UC will be before this runs: the rewrite's own
 * `CHAR n UC`, or the box's current one. Returns the same array shape.
 */
export function withOppositeOutfitUc(boxes: string[], ucs: string[]): string[] {
  const extra = oppositeOutfitTags(boxes, ucs);
  return boxes.map((_, i) => appendTags(ucs[i] ?? "", extra[i]));
}

/** The base prompt, each box's prompt and each box's own undesired content. */
export interface OutfitFields {
  base: string;
  boxes: string[];
  ucs: string[];
}

/** A clothing or hair tag written in more than one place. */
export interface OutfitOverlap {
  /** Bare form, as `fieldTags` gives it. */
  tag: string;
  kind: "clothing" | "hair";
  /** Whether the base prompt's tag line has it. */
  inBase: boolean;
  /** Indexes of the boxes that have it. */
  boxes: number[];
}

/**
 * Clothing and hair tags that sit in the base tag line and a box, and
 * clothing written in two boxes.
 */
export function findOutfitOverlaps(base: string, boxes: string[]): OutfitOverlap[] {
  if (boxes.length < 2) return [];
  const baseTags = new Set(fieldTags(base).map((t) => t.norm));
  const boxTags = boxes.map((b) => new Set(fieldTags(b).map((t) => t.norm)));
  const seen = new Set<string>();
  const out: OutfitOverlap[] = [];
  for (const set of boxTags) {
    for (const tag of set) {
      if (seen.has(tag)) continue;
      seen.add(tag);
      const kind = tagKind(tag);
      if (kind !== "clothing" && kind !== "hair") continue;
      const holders = boxTags.flatMap((s, i) => (s.has(tag) ? [i] : []));
      const inBase = baseTags.has(tag);
      // Between boxes only clothing counts: alike hair is a similar body (twins,
      // sisters), which the opposite-outfit UC handles, not a leak to undo.
      const sharedByBoxes = holders.length > 1 && kind === "clothing";
      if (inBase || sharedByBoxes) out.push({ tag, kind, inBase, boxes: holders });
    }
  }
  return out;
}

const WEIGHT_OPEN = /^-?\d+(?:\.\d+)?::/;

/**
 * Remove one tag (by its bare form) from a field's tag lines, keeping any
 * emphasis or weight span it opened or closed for its neighbours.
 *
 * Prose lines and a `Text:` block are left alone.
 */
export function removeTag(text: string, norm: string): string {
  if (!text) return text;
  const textAt = text.search(/(?:^|\n)\s*Text\s*:/i);
  const head = textAt === -1 ? text : text.slice(0, textAt);
  const tail = textAt === -1 ? "" : text.slice(textAt);
  const lines = head.split("\n").flatMap((line) => {
    if (isProseLine(line)) return [line];
    const parts = line.split(",");
    if (!parts.some((p) => bareTag(p) === norm)) return [line];
    const trailingComma = /,\s*$/.test(line);
    const kept: string[] = [];
    let carry = "";
    // Inside a negative span (`-1::hat::`) the tag is already a negative and stays.
    let negative = false;
    for (const part of parts) {
      const trimmed = part.trim();
      const negOpener = trimmed.match(/^-\d+(?:\.\d+)?::/)?.[0] ?? "";
      const inNegative = negative || negOpener !== "";
      if (inNegative) negative = !trimmed.slice(negOpener.length).endsWith("::");
      if (inNegative || bareTag(part) !== norm) {
        if (trimmed || carry) kept.push(carry + trimmed);
        carry = "";
        continue;
      }
      // Markers the removed tag carried for a span wider than itself go to its
      // neighbours: an opener to the next tag, a closer to the previous one.
      let core = trimmed;
      const weight = core.match(WEIGHT_OPEN)?.[0] ?? "";
      core = core.slice(weight.length);
      const closesWeight = core.endsWith("::");
      if (closesWeight) core = core.slice(0, -2);
      const opens = core.match(/^[{[]*/)?.[0] ?? "";
      const closeBraces = core.match(/[}\]]*$/)?.[0] ?? "";
      const net = opens.length - closeBraces.length;
      const open = (weight && !closesWeight ? weight : "") + (net > 0 ? opens.slice(0, net) : "");
      const close = (net < 0 ? closeBraces.slice(0, -net) : "") + (closesWeight && !weight ? "::" : "");
      if (close && kept.length > 0) kept[kept.length - 1] += close;
      carry += open;
    }
    if (carry && kept.length > 0) kept[kept.length - 1] += carry;
    const joined = kept.filter((p) => p !== "").join(", ");
    if (!joined) return [];
    return [trailingComma ? `${joined},` : joined];
  });
  const out = lines.join("\n").replace(/\n{3,}/g, "\n\n").trim();
  return tail ? `${out}${out ? "\n\n" : ""}${tail.replace(/^\n+/, "")}` : out;
}

/** Bare tags of one kind a field holds. */
function tagsOfKind(text: string, kind: TagKind): string[] {
  return fieldTags(text)
    .map((t) => t.norm)
    .filter((t) => tagKind(t) === kind);
}

/** Add `tag` to a box's UC unless the box wears it or the UC already covers it. */
function avoidIn(uc: string, box: string, tag: string): string {
  const kind = tagKind(tag);
  const own = kind === "other" ? [] : tagsOfKind(box, kind);
  if (clashesWithOwn(tag, own)) return uc;
  const tw = words(tag);
  if (fieldTags(uc).some((t) => isSubset(words(t.norm), tw))) return uc;
  return appendTags(uc, [tag]);
}

/**
 * Resolve one overlap: the tag stays in box `keep` (or in whichever boxes have
 * it, when `keep` is null), leaves the base tag line and every other box, and
 * goes into the undesired content of every box that does not wear it.
 */
export function keepTagIn(fields: OutfitFields, tag: string, keep: number | null): OutfitFields {
  const boxes = fields.boxes.map((box, i) => (keep === null || i === keep ? box : removeTag(box, tag)));
  const ucs = boxes.map((box, i) => {
    const uc = fields.ucs[i] ?? "";
    if (fieldTags(box).some((t) => t.norm === tag)) return uc;
    return avoidIn(uc, box, tag);
  });
  return { base: removeTag(fields.base, tag), boxes, ucs };
}

/** Placement words for a box from its position on NovelAI's 0..1 grid. */
export function positionFromX(x: number): string {
  if (x < 0.4) return "on the left";
  if (x > 0.6) return "on the right";
  return "in the center";
}

/**
 * Everything that can be fixed without a rewrite:
 * - clothing and hair tags the base tag line shares with a box leave the base;
 * - a box with no placement words gets `positions[i]`, when given;
 * - every box gets the opposite-outfit UC.
 *
 * Garments two boxes share are left in place: matching clothes can be on
 * purpose, so those stay flagged for the user to settle.
 */
export function isolateOutfits(fields: OutfitFields, positions: (string | null)[] = []): OutfitFields {
  if (fields.boxes.length < 2) return fields;
  const boxTags = new Set(fields.boxes.flatMap((b) => fieldTags(b).map((t) => t.norm)));
  let base = fields.base;
  for (const tag of new Set(fieldTags(base).map((t) => t.norm))) {
    const kind = tagKind(tag);
    if ((kind === "clothing" || kind === "hair") && boxTags.has(tag)) base = removeTag(base, tag);
  }
  const boxes = fields.boxes.map((box, i) => {
    const place = positions[i];
    if (!place || POSITION_WORDS.test(box)) return box;
    return appendTags(box, [place]);
  });
  return { base, boxes, ucs: withOppositeOutfitUc(boxes, fields.ucs) };
}

/**
 * Pairs of boxes with alike bodies (the same hair colour, or two or more shared
 * hair, eye or body tags) whose outfits differ. That is the pair most likely to
 * trade clothes, and the one opposite-outfit UC exists for.
 */
export function similarBodyPairs(boxes: string[]): [number, number][] {
  const traits = boxes.map((b) =>
    fieldTags(b)
      .map((t) => t.norm)
      .filter((t) => (tagKind(t) === "hair" || tagKind(t) === "body") && !COMMON_HAIR.has(t)),
  );
  const outfits = boxes.map(outfitTags);
  const pairs: [number, number][] = [];
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const shared = traits[i].filter((t) => traits[j].includes(t));
      const sameHairColour = shared.some((t) => /\bhair\b/.test(t));
      const alike = sameHairColour || shared.length >= 2;
      const differ = outfits[i].join("|") !== outfits[j].join("|") && outfits[i].length + outfits[j].length > 0;
      if (alike && differ) pairs.push([i, j]);
    }
  }
  return pairs;
}

/** True when every box's UC already holds the other boxes' distinctive garments. */
export function hasOppositeOutfitUc(boxes: string[], ucs: string[]): boolean {
  return oppositeOutfitTags(boxes, ucs).every((tags) => tags.length === 0);
}
