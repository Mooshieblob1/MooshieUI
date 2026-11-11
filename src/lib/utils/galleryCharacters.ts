/**
 * Find saved characters in gallery images by comparing tags: the tags a
 * character card inserts into a prompt against the prompt an image was made
 * with. No model runs, so a whole gallery is checked in milliseconds.
 *
 * A card matches an image when
 *  - the image has a subject of the card's kind at all (a `1girl` card is
 *    skipped outright for an image with no girl tag), and
 *  - the image names the character, or carries enough of the card's identity
 *    tags (hair, eyes, ears, ...). Outfit and pose tags only nudge the score:
 *    the same character wears many outfits.
 */

/** The parts of a saved character card the matcher reads. */
export interface DetectCard {
  id: string;
  name: string;
  prompt: string;
  /** The card this one is a variant of, or null. */
  parentId: string | null;
}

export interface CharacterHit {
  /** The top-level character: the card itself, or its parent for a variant. */
  characterId: string;
  /** The card that matched best: the character or one of its variants. */
  cardId: string;
  /** 0..1; 1 when the image names the character. */
  score: number;
}

/** Score a card must reach to count as in the image. */
export const CHARACTER_MATCH_THRESHOLD = 0.7;

type Subject = "girl" | "boy" | "other";

const SUBJECT_TAG = /^(?:\d\+?|multiple|two|three|four|five|six)? ?(girl|boy|other)s?$/;

const PROSE_SUBJECT = / \d?(girls?|wom[ae]n|lady|female|boys?|m[ae]n|guy|male)(?= )/g;

/** Tags that say nothing about who is in the picture. */
const NOISE_TAG = new RegExp(
  "^(?:" +
    [
      "solo(?: focus)?",
      "masterpiece",
      "(?:best|high|highest|amazing|great|good|normal|low|worst|bad) quality",
      "(?:very )?(?:aesthetic|awa|displeasing)",
      "(?:absurd|high|low)res",
      "newest|recent|mid|early|old|oldest",
      "(?:safe|sensitive|general|questionable|explicit|nsfw|sfw)",
      "rating ?: ?\\w+",
      "year \\d{4}",
      "score \\d(?: up)?",
      "no text|location|official art|anime (?:screencap|coloring)|game cg",
      "looking at viewer|simple background|white background",
    ].join("|") +
    ")$",
);

/** Tags that describe the character's body rather than what they wear or do. */
const IDENTITY_TAG = new RegExp(
  [
    "\\bhair\\b",
    "\\b(?:ahoge|bangs|twintails|ponytail|braids?|sidelocks|hair bun|double bun|bob cut|hime cut|pixie cut|drill hair|hair intakes|antenna hair|forehead)\\b",
    "\\beyes?\\b",
    "\\b(?:heterochromia|pupils|eyelashes|eyebrows|sclera|tsurime|tareme)\\b",
    "\\bskin\\b",
    "\\b(?:tan|tanlines|freckles|mole|scar|tattoo|facial mark|body markings?|whisker markings)\\b",
    "\\b(?:ears?|tail|horns?|wings?|halo|fangs?|antennae|tusks)\\b",
    "\\b(?:elf|furry|kemonomimi|monster girl|android|robot|cyborg)\\b",
    "\\b(?:beard|mustache|stubble|facial hair)\\b",
  ].join("|"),
);

/** Lower-case a tag and strip prompt syntax: weights, brackets, underscores, escapes. */
export function normalizeTag(raw: string): string {
  let tag = raw.toLowerCase();
  // <lora:name:1> and other angle-bracket syntax is not a tag.
  tag = tag.replace(/<[^>]*>/g, " ");
  // NovelAI numeric emphasis: `1.2::tag::`.
  tag = tag.replace(/-?\d*\.?\d+::/g, " ").replace(/::/g, " ");
  // Escaped parentheses belong to the tag ("name \(series\)"); the rest are weights.
  tag = tag.replace(/\\\(/g, "\u0001").replace(/\\\)/g, "\u0002");
  tag = tag.replace(/[(){}\[\]]/g, " ").replace(/:\s*-?\d*\.?\d+\s*$/, " ");
  tag = tag.replace(/\u0001/g, "(").replace(/\u0002/g, ")");
  return tag.replace(/_/g, " ").replace(/\s+/g, " ").trim();
}

/** Split a prompt into normalized tags. */
export function promptTags(prompt: string): string[] {
  const tags: string[] = [];
  for (const part of prompt.split(/[,\n|;]|\bBREAK\b/)) {
    const tag = normalizeTag(part);
    if (tag) tags.push(tag);
  }
  return tags;
}

/** One thing a card is compared against: a prompt, or a NovelAI character box with the base prompt. */
export interface PromptUnit {
  tags: Set<string>;
  /** The whole text, punctuation flattened, padded with spaces: for prose prompts. */
  text: string;
  subjects: Set<Subject>;
  /** Every word of `text`: a tag whose words are not all here cannot be in it. */
  words: Set<string>;
  /** Tags already looked up in this unit, shared by every card. */
  seen: Map<string, boolean>;
}

function buildUnit(prompts: string[]): PromptUnit {
  const tags = new Set<string>();
  const subjects = new Set<Subject>();
  for (const prompt of prompts) {
    for (const tag of promptTags(prompt)) {
      tags.add(tag);
      const subject = SUBJECT_TAG.exec(tag)?.[1] as Subject | undefined;
      if (subject) subjects.add(subject);
    }
  }
  const text = ` ${[...tags].join(" ").replace(/[^\p{L}\p{N}]+/gu, " ").trim()} `;
  // Prose prompts have no count tag of their own: "a photo of a woman with ...".
  for (const [, word] of text.matchAll(PROSE_SUBJECT)) {
    subjects.add(/^(?:girl|wom[ae]n|lady|female)/.test(word!) ? "girl" : "boy");
  }
  return { tags, text, subjects, words: new Set(text.split(" ")), seen: new Map() };
}

/**
 * The units an image offers. A NovelAI image with character boxes gives one
 * unit per box (with the base prompt, which holds the head count) so two
 * characters in one picture are each compared on their own tags.
 */
export function imageUnits(metadata: Record<string, string> | null | undefined): PromptUnit[] {
  const base = metadata?.positive_prompt ?? "";
  const boxes: string[] = [];
  if (metadata?.mooshie_novelai_characters) {
    try {
      const parsed = JSON.parse(metadata.mooshie_novelai_characters);
      if (Array.isArray(parsed)) {
        for (const box of parsed) {
          if (typeof box?.prompt === "string" && box.prompt.trim() && box.enabled !== false) boxes.push(box.prompt);
        }
      }
    } catch {
      // An unreadable character list just means the base prompt is all there is.
    }
  }
  if (boxes.length > 0) return boxes.map((box) => buildUnit([base, box]));
  return base.trim() ? [buildUnit([base])] : [];
}

export interface PreparedCard {
  card: DetectCard;
  characterId: string;
  subjects: Subject[];
  /** The character's name as tags an image may carry. */
  names: string[];
  identity: string[];
  other: string[];
}

function wordsOf(text: string): string {
  return text.replace(/[^\p{L}\p{N}]+/gu, " ").trim();
}

/** A card's name minus a variant label: "Miku (swimsuit)" names "miku". */
function nameTags(card: DetectCard, tags: string[]): string[] {
  const names = new Set<string>();
  const full = normalizeTag(card.name);
  const bare = full.replace(/\s*\(.*\)\s*$/, "").trim();
  for (const name of [full, bare]) {
    // One short word ("rin", "red") is too common to trust as a name on its own.
    if (name.length >= 4 || name.includes(" ")) names.add(name);
  }
  // A tag in the card that carries the bare name is a name tag too: "hatsune miku (vocaloid)".
  if (bare.length >= 4) {
    const padded = ` ${wordsOf(bare)} `;
    for (const tag of tags) {
      if (` ${wordsOf(tag)} `.includes(padded)) names.add(tag);
    }
  }
  return [...names];
}

export function prepareCards(cards: readonly DetectCard[]): PreparedCard[] {
  const ids = new Set(cards.map((card) => card.id));
  return cards.map((card) => {
    const tags = [...new Set(promptTags(card.prompt))];
    const names = nameTags(card, tags);
    const nameSet = new Set(names);
    const subjects: Subject[] = [];
    const identity: string[] = [];
    const other: string[] = [];
    for (const tag of tags) {
      const subject = SUBJECT_TAG.exec(tag)?.[1] as Subject | undefined;
      if (subject) {
        if (!subjects.includes(subject)) subjects.push(subject);
      } else if (nameSet.has(tag) || NOISE_TAG.test(tag)) {
        continue;
      } else if (IDENTITY_TAG.test(tag)) {
        identity.push(tag);
      } else {
        other.push(tag);
      }
    }
    const characterId = card.parentId && ids.has(card.parentId) ? card.parentId : card.id;
    return { card, characterId, subjects, names, identity, other };
  });
}

/** A tag's words, cached: the same few hundred card tags are looked up in every image. */
const tagWords = new Map<string, string[]>();

function has(unit: PromptUnit, tag: string): boolean {
  if (unit.tags.has(tag)) return true;
  const seen = unit.seen.get(tag);
  if (seen !== undefined) return seen;
  let words = tagWords.get(tag);
  if (!words) {
    words = wordsOf(tag).split(" ");
    if (tagWords.size > 5000) tagWords.clear();
    tagWords.set(tag, words);
  }
  // Most tags fail on their first word, which spares scanning the whole prompt.
  const found = words.every((word) => unit.words.has(word)) && unit.text.includes(` ${words.join(" ")} `);
  unit.seen.set(tag, found);
  return found;
}

function fraction(unit: PromptUnit, tags: string[]): number {
  if (tags.length === 0) return 0;
  let found = 0;
  for (const tag of tags) if (has(unit, tag)) found++;
  return found / tags.length;
}

/** How well one card fits one unit, 0..1. */
export function scoreCard(card: PreparedCard, unit: PromptUnit): number {
  // The gate: a 1girl card needs a girl in the picture, whatever the head count.
  if (card.subjects.length > 0 && !card.subjects.some((subject) => unit.subjects.has(subject))) return 0;
  if (card.names.some((name) => has(unit, name))) return 1;
  // Two identity tags at least: "blue hair" alone fits half the gallery.
  if (card.identity.length < 2) return 0;
  const identity = fraction(unit, card.identity);
  if (card.other.length === 0) return identity;
  return identity * 0.85 + fraction(unit, card.other) * 0.15;
}

/** The characters in an image, best match first, one entry per top-level character. */
export function detectCharacters(
  metadata: Record<string, string> | null | undefined,
  cards: readonly PreparedCard[],
  threshold = CHARACTER_MATCH_THRESHOLD,
): CharacterHit[] {
  return detectInUnits(imageUnits(metadata), cards, threshold);
}

/** `detectCharacters` for units built earlier, so an image's prompt is parsed once however often the cards change. */
export function detectInUnits(
  units: readonly PromptUnit[],
  cards: readonly PreparedCard[],
  threshold = CHARACTER_MATCH_THRESHOLD,
): CharacterHit[] {
  if (cards.length === 0 || units.length === 0) return [];
  const best = new Map<string, CharacterHit>();
  for (const card of cards) {
    let score = 0;
    for (const unit of units) score = Math.max(score, scoreCard(card, unit));
    if (score < threshold) continue;
    const current = best.get(card.characterId);
    // On a tie the character's own card wins over a variant.
    if (!current || score > current.score || (score === current.score && card.card.id === card.characterId)) {
      best.set(card.characterId, { characterId: card.characterId, cardId: card.card.id, score });
    }
  }
  return [...best.values()].sort((a, b) => b.score - a.score);
}
