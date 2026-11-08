/**
 * What a prompt-assistant request is about, judged from the user's own words.
 *
 * Two questions, asked of the positive text only: does it involve a minor, and
 * does it ask for sexual or suggestive content. Never the negative prompt,
 * where "nsfw" or "nude" is the thing being kept out of the image.
 *
 * - A clear minor word with a clearly explicit word (nudity, sex acts,
 *   genitals, porn) is refused here, before anything is sent. That line is
 *   fixed and nothing the user writes moves it. It is kept that narrow on
 *   purpose: everything short of it goes out and the model handles it.
 * - A minor with only suggestive or body words (cleavage on the mother in a
 *   family scene, "sexy", "lingerie", "don't make it sexual"), or an
 *   ambiguous minor word ("baby") with explicit ones, is sent as
 *   `suggestive_minor`: no explicit vocabulary, and the non-sexual minors
 *   rule always on, so the model drops whatever does not belong.
 * - A minor alone is an ordinary safe-for-work request (a family at the beach,
 *   a kid with a kite). It goes out with the minors rule (which the user may
 *   turn off) and without the explicit vocabulary the V5 specification carries
 *   for adult scenes.
 * - Sexual content alone keeps the explicit vocabulary, as before.
 *
 * Leaf util with no imports, so `scripts/tests/contentSafety.test.ts` can load
 * it directly.
 */

export type ContentRating = "sfw" | "minor" | "suggestive_minor" | "explicit" | "blocked";

/** Thrown, as the message, when a request pairs a clear minor with explicit content. */
export const BLOCKED_MINOR_CONTENT = "blocked_minor_content";

/** Words or tags that put a minor in the scene. */
const MINOR_TERMS = [
  "child", "children", "childs", "kid", "kids", "kiddo", "toddler", "toddlers",
  "infant", "infants", "baby", "babies", "newborn", "minor", "minors",
  "underage", "under age", "preteen", "preteens", "pre teen", "tween", "tweens",
  "teen", "teens", "teenage", "teenaged", "teenager", "teenagers",
  "prepubescent", "pubescent", "loli", "lolis", "lolicon", "shota", "shotas",
  "shotacon", "young girl", "young girls", "young boy", "young boys",
  "little girl", "little girls", "little boy", "little boys", "schoolgirl",
  "schoolgirls", "schoolboy", "schoolboys", "schoolchild", "schoolchildren",
  "schoolkid", "schoolkids", "kindergarten", "kindergartener", "kindergartner",
  "preschool", "preschooler", "elementary schooler", "elementary school student",
  "grade schooler", "middle schooler", "middle school student", "junior high student",
  "high schooler", "aged down", "age regression", "age regressed", "male child",
  "female child",
];

/**
 * Minor words with an everyday adult sense ("hey baby"). They still make a
 * request a minor one, but never refuse it.
 */
const WEAK_MINOR_TERMS = new Set(["baby", "babies"]);

/**
 * Minor terms in scripts that do not put spaces between words, matched as
 * substrings. 少女 and 少年 are left out: they are as much genre words
 * (magical girl, shounen) as ages.
 */
const MINOR_TERMS_CJK = [
  "幼女", "幼児", "児童", "子供", "子ども", "こども", "小学生", "中学生", "未成年",
  "ロリ", "ショタ", "儿童", "幼童", "小孩", "萝莉", "蘿莉", "正太", "아동",
  "어린이", "미성년", "로리", "쇼타",
];

/** Phrases that contain a minor term but mean something else. */
const MINOR_FALSE_FRIENDS = [
  /\bbaby[\s_-]*(?:blue|pink|doll|dolls|powder|breath|carrot|carrots|grand|bump)\b/giu,
  /\bbabydoll\b/giu,
  /\bkid[\s_-]*gloves?\b/giu,
  /\bminor[\s_-]*(?:changes?|tweaks?|edits?|details?|adjustments?|fix(?:es)?|issues?|problems?|injur(?:y|ies)|cuts?|bruises?|scratch(?:es)?|keys?|chords?|scales?|characters?|roles?|npcs?|deit(?:y|ies)|gods?|league|arcana|planets?|differences?|flaws?|errors?|mistakes?|things?|parts?|spoilers?|variations?|improvements?|updates?)\b/giu,
  /ロリータ/gu,
];

/**
 * Clearly explicit words and tags: nudity, sex acts, genitals, porn. Only
 * these, beside a clear minor word, refuse a request. Cancelled by a negation
 * in front.
 */
const EXPLICIT_TERMS = [
  "nsfw", "nude", "nudes", "nudity", "naked", "topless", "bottomless", "nipple",
  "nipples", "areola", "areolae", "areolas", "pussy", "vagina", "vaginal", "vulva",
  "labia", "clitoris", "penis", "penises", "erection", "testicles", "scrotum", "anus",
  "anal", "sex", "sexually", "erotic", "erotica", "eroticism", "lewd", "hentai",
  "porn", "porno", "pornography", "pornographic", "ecchi", "smut", "smutty", "xxx",
  "x rated", "r18", "r 18", "18+", "cum", "cumshot", "creampie", "semen",
  "fellatio", "blowjob", "handjob", "footjob", "titjob", "paizuri", "cunnilingus",
  "masturbation", "masturbating", "masturbate", "orgasm", "ahegao", "bukkake",
  "intercourse", "cowgirl position", "doggystyle", "doggy style",
  "missionary position", "rape", "raped", "molest", "molested", "molestation",
  "grope", "groped", "groping", "fondle", "fondling", "pantyshot", "panty shot",
  "upskirt", "downblouse", "cameltoe", "spread pussy", "bdsm", "sex toy",
  "vibrator", "dildo", "aroused", "arousal", "horny", "undressed", "unclothed",
  "striptease", "rating:explicit", "rating:questionable",
];

/**
 * Suggestive, body and clothing words: an explicit rating for an adult
 * request, but beside a minor they only force the minors rule on. They are
 * the words that show up innocently (the mother's cleavage in a family scene,
 * "don't make it sexual"), so they never refuse anything.
 */
const SUGGESTIVE_TERMS = [
  "sexual", "sexualized", "sexualised", "sexy", "seductive", "seduce", "seducing",
  "seduction", "explicit", "fingering", "lingerie", "panties", "pantsu",
  "underwear only", "micro bikini", "bondage", "condom", "spread legs", "breast",
  "breasts", "boobs", "tits", "cleavage", "busty", "voluptuous", "thicc", "ass",
  "crotch", "groin", "undress", "undressing", "stripping", "suggestive",
  "provocative", "fanservice", "fan service", "rating:sensitive",
];

/**
 * Exposure tags that start with a negation but are themselves explicit
 * ("no panties" means none are worn), so they count whatever precedes them.
 */
const EXPOSURE_PHRASES = [
  "no panties", "no bra", "no underwear", "no clothes", "no pants", "without clothes",
  "without panties", "without underwear", "clothes lift", "shirt up", "skirt lift",
  "clothes pulled aside", "panties aside", "wardrobe malfunction",
];

/** Phrases that contain a sexual term but mean something else. */
const SEXUAL_FALSE_FRIENDS = [
  /\bbreast[\s_-]*(?:pockets?|plates?|stroke)\b/giu,
  /\bchicken[\s_-]*breasts?\b/giu,
];

const SEXUAL_TERMS_CJK = [
  "全裸", "半裸", "裸体", "裸體", "エロ", "セックス", "性交", "色情", "做爱", "做愛",
  "섹스", "누드", "야한",
];

/** A word in front of a term that turns it into something to avoid. */
const NEGATIONS = new Set([
  "no", "not", "without", "avoid", "avoiding", "never", "non", "zero", "nothing", "nor",
  "remove", "removed", "removing", "delete", "deleting", "drop", "stop", "less", "fewer",
]);
/** Skipped when looking back for a negation: "not a child", "no visible nudity". */
const FILLER = new Set([
  "a", "an", "any", "the", "visible", "of", "all", "every", "more", "it", "make", "too", "so", "very",
  "actually", "really", "even", "longer", "considered",
]);
/** Words that carry a negation on to the next term: "not a child or a teenager". */
const LIST_JOINERS = new Set(["or", "nor", "and", "a", "an"]);

/** Name words too common to tie a request to one saved character. */
const NAME_STOPWORDS = new Set([
  "the", "and", "von", "van", "der", "den", "del", "des", "mrs", "miss", "little",
  "young", "big", "old", "girl", "boy", "lady", "kid", "child",
]);

function escape(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** A term as a whole-word pattern, with spaces matching runs of space, _ or -. */
function termPattern(terms: string[], flags = "giu"): RegExp {
  const alts = terms
    .slice()
    .sort((a, b) => b.length - a.length)
    .map((t) => escape(t).replace(/ /g, "[\\s_-]*"));
  return new RegExp(`(?<![\\p{L}\\p{N}])(?:${alts.join("|")})(?![\\p{L}\\p{N}])`, flags);
}

const MINOR_RE = termPattern(MINOR_TERMS);
const EXPLICIT_RE = termPattern(EXPLICIT_TERMS);
const SEXUAL_RE = termPattern([...EXPLICIT_TERMS, ...SUGGESTIVE_TERMS]);
const EXPOSURE_RE = termPattern(EXPOSURE_PHRASES, "iu");

/** "12 years old", "12-year-old", "12yo", "aged 12", "age: 9". */
const AGE_RE =
  /(?<![\p{L}\p{N}])(?:(\d{1,2})[\s_-]*(?:years?|yrs?)[\s_-]*old|(\d{1,2})[\s_-]*y\/?o|aged?[\s_:-]*(\d{1,2}))(?![\p{L}\p{N}])/giu;

/**
 * Text ready for matching: lowercased, with NovelAI's suppression spans
 * (`-1::hat::`) removed, since a negative weight asks for the thing to be
 * kept out.
 */
function normalize(text: string): string {
  return text
    .normalize("NFKC")
    .toLowerCase()
    .replace(/-\d*(?:\.\d+)?::[\s\S]*?::/g, " ")
    .replace(/n['\u2019]t(?![\p{L}\p{N}])/gu, " not");
}

/**
 * Whether the words right before `index` negate what follows. The look back
 * stops at a comma or full stop, so "no hat, child" still has a child in it.
 */
function negated(text: string, index: number): boolean {
  const before = text.slice(Math.max(0, index - 40), index);
  const clause = before.slice(Math.max(...[",", ".", ";", "\n"].map((p) => before.lastIndexOf(p))) + 1);
  const words = clause.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  for (let i = words.length - 1, looked = 0; i >= 0 && looked < 3; i--, looked++) {
    if (NEGATIONS.has(words[i])) return true;
    if (!FILLER.has(words[i])) return false;
  }
  return false;
}

/** The first match of `re` in `text` that is not negated (and passes `keep`), or null. */
function firstAffirmed(re: RegExp, text: string, keep: (word: string) => boolean = () => true): string | null {
  let lastNegatedEnd = -1;
  for (const m of text.matchAll(re)) {
    const start = m.index ?? 0;
    const listed =
      lastNegatedEnd >= 0 &&
      text
        .slice(lastNegatedEnd, start)
        .split(/[^\p{L}\p{N}]+/u)
        .every((w) => !w || LIST_JOINERS.has(w)) &&
      !/[,.;\n]/.test(text.slice(lastNegatedEnd, start));
    if (listed || negated(text, start)) {
      lastNegatedEnd = start + m[0].length;
      continue;
    }
    lastNegatedEnd = -1;
    if (keep(m[0])) return m[0];
  }
  return null;
}

/** The first word or phrase that puts a minor in the scene, or null. */
export function minorTerm(text: string): string | null {
  return findMinor(text, false);
}

/**
 * Like `minorTerm`, but only the words that cannot mean anything else: never
 * "baby". Only these refuse a request.
 */
export function clearMinorTerm(text: string): string | null {
  return findMinor(text, true);
}

function findMinor(text: string, clearOnly: boolean): string | null {
  let t = normalize(text);
  for (const re of MINOR_FALSE_FRIENDS) t = t.replace(re, " ");
  const word = firstAffirmed(MINOR_RE, t, (w) => !clearOnly || !WEAK_MINOR_TERMS.has(w));
  if (word) return word;
  const cjk = MINOR_TERMS_CJK.find((w) => t.includes(w));
  if (cjk) return cjk;
  for (const m of t.matchAll(AGE_RE)) {
    const age = Number(m[1] ?? m[2] ?? m[3]);
    if (age < 18 && !negated(t, m.index ?? 0)) return m[0];
  }
  return null;
}

/** The first word or phrase that asks for sexual or suggestive content, or null. */
export function sexualTerm(text: string): string | null {
  return findSexual(text, SEXUAL_RE);
}

/** The first clearly explicit word or phrase (no merely suggestive ones), or null. */
export function explicitTerm(text: string): string | null {
  return findSexual(text, EXPLICIT_RE);
}

function findSexual(text: string, re: RegExp): string | null {
  let t = normalize(text);
  for (const ff of SEXUAL_FALSE_FRIENDS) t = t.replace(ff, " ");
  const exposure = t.match(EXPOSURE_RE);
  if (exposure) return exposure[0];
  const word = firstAffirmed(re, t);
  if (word) return word;
  return SEXUAL_TERMS_CJK.find((w) => t.includes(w)) ?? null;
}

/** Whether the positive text puts a minor in the scene. */
export function mentionsMinor(text: string): boolean {
  return minorTerm(text) !== null;
}

/** Whether the positive text asks for sexual or suggestive content. */
export function mentionsSexualContent(text: string): boolean {
  return sexualTerm(text) !== null;
}

/** What made a request blocked, for the message that refuses it. */
export interface BlockedReason {
  /** The words that put a minor in it. */
  minor?: string;
  /** The words that made it sexual. */
  sexual?: string;
  /** The saved character drawn as a minor that an explicit request named. */
  character?: string;
}

/** The refusal, with what triggered it riding in the message after `|`. */
export function blockedError(reason: BlockedReason): Error {
  const parts = [BLOCKED_MINOR_CONTENT];
  if (reason.character) parts.push(`character=${reason.character}`);
  if (reason.minor) parts.push(`minor=${reason.minor}`);
  if (reason.sexual) parts.push(`sexual=${reason.sexual}`);
  return new Error(parts.join("|"));
}

/** The reason a `blockedError` message carries, or null for any other message. */
export function parseBlocked(message: string): BlockedReason | null {
  const at = message.indexOf(BLOCKED_MINOR_CONTENT);
  if (at === -1) return null;
  const reason: BlockedReason = {};
  for (const part of message.slice(at).split("|").slice(1)) {
    const eq = part.indexOf("=");
    const key = part.slice(0, eq);
    const value = part.slice(eq + 1).trim();
    if (value && (key === "minor" || key === "sexual" || key === "character")) reason[key] = value;
  }
  return reason;
}

function join(texts: readonly string[]): string {
  return texts.filter((t) => t && t.trim()).join("\n");
}

/** The rating, and for a refusal the words that caused it. */
function rate(
  texts: readonly string[],
  background: readonly string[],
): { rating: ContentRating; reason?: BlockedReason } {
  const said = join(texts);
  const all = join([...texts, ...background]);
  const minor = minorTerm(all);
  if (!sexualTerm(said)) return { rating: minor ? "minor" : "sfw" };
  if (!minor) return { rating: "explicit" };
  const clear = clearMinorTerm(all);
  const explicit = explicitTerm(said);
  if (clear && explicit) return { rating: "blocked", reason: { minor: clear, sexual: explicit } };
  return { rating: "suggestive_minor" };
}

/**
 * Rate a request from every positive text it carries: the instruction, the
 * earlier user turns of its session, and any current prompt it edits. Rated
 * together, so a minor in one turn and explicit content in a later one is
 * still the combination that is refused.
 *
 * `background` is what the request reads about its characters without asking
 * for it in the picture: memory notes, linked characters. It can make a
 * request a minor one ("she is 15" in her notes) but never a sexual one, so a
 * canon backstory that mentions an adult game does not sexualize every image
 * of that character.
 */
export function rateRequest(texts: readonly string[], background: readonly string[] = []): ContentRating {
  return rate(texts, background).rating;
}

/**
 * The positive texts a V5 rewrite carries: the instruction, the earlier user
 * turns of its session, and the current base prompt and character boxes when
 * the user asked for them to be edited. Never the undesired content.
 */
export function naiRequestTexts(
  prompt: string,
  existing: { base: string; characters: readonly string[] } | null,
  sessionUserTurns: readonly string[],
): string[] {
  return [prompt, ...sessionUserTurns, ...(existing ? [existing.base, ...existing.characters] : [])];
}

/**
 * Throw before sending a request that pairs a clear minor with explicit
 * content, naming the words that did. `background` as in `rateRequest`.
 */
export function assertAllowedRequest(texts: readonly string[], background: readonly string[] = []): ContentRating {
  const { rating, reason } = rate(texts, background);
  if (rating === "blocked") throw blockedError(reason ?? {});
  return rating;
}

/**
 * The saved characters a request may send, given its rating.
 *
 * Saved characters ride along in the system turn whether or not the user names
 * them. In an explicit request a saved character drawn as a minor is left out.
 * Naming one (by any word of their name) in a request with clearly explicit
 * words refuses it, since the user's text would not say "child" but the saved
 * card does; with only suggestive words the card is just left out.
 *
 * A name word that an adult card the text also names shares (a surname, as in
 * "Miki Hiiragi" and her daughter "Tsukasa Hiiragi") is explained by that adult
 * and does not name the minor. Said on its own ("the Hiiragi sisters"), it
 * still does.
 */
export function savedCharactersForRequest<T extends { name: string; prompt: string; notes?: string }>(
  characters: readonly T[],
  rating: ContentRating,
  texts: readonly string[],
): T[] {
  if (rating !== "explicit") return characters.slice();
  const said = texts.join("\n");
  const explicit = explicitTerm(said) !== null;
  // Memory notes count too: "she is 15" there makes the card a minor.
  const minorOf = characters.map((c) => clearMinorTerm(`${c.name}\n${c.prompt}\n${c.notes ?? ""}`));
  const words = characters.map((c) => nameWords(c.name));
  const explained = new Set<string>();
  characters.forEach((c, i) => {
    if (minorOf[i]) return;
    const own = words[i].filter((w) => !words.some((other, j) => j !== i && other.includes(w)));
    if (own.some((w) => saysWord(w, said))) for (const w of words[i]) explained.add(w);
  });
  const kept: T[] = [];
  characters.forEach((c, i) => {
    const minor = minorOf[i];
    if (!minor) {
      kept.push(c);
      return;
    }
    if (explicit && words[i].some((w) => !explained.has(w) && saysWord(w, said))) {
      throw blockedError({ character: c.name.trim(), minor });
    }
  });
  return kept;
}

/** The words of a saved character's name that can identify them. */
function nameWords(name: string): string[] {
  return normalize(name.replace(/\([^)]*\)/g, " "))
    .split(/[^\p{L}\p{N}]+/u)
    .filter((w) => w.length >= 3 && !NAME_STOPWORDS.has(w));
}

function saysWord(word: string, text: string): boolean {
  return termPattern([word], "iu").test(normalize(text));
}

/**
 * Whether `text` refers to a saved character by name: any word of it with
 * three or more letters that is not a common word, so "Julie" finds "Julie
 * Evergreen". A variant's label in brackets ("Julie (swimsuit)") is not part
 * of the name.
 */
export function namesCharacter(name: string, text: string): boolean {
  return nameWords(name).some((w) => saysWord(w, text));
}

/**
 * The VOCABULARY block of the V5 rewrite for a request of this rating.
 *
 * Only an explicit request hears the explicit tag list. A request with a minor
 * in it gets the non-sexual rule instead, stated as overriding everything else,
 * unless the user turned that rule off (`minorRule` false): some providers
 * refuse an SFW school or family scene because the rule itself names a child
 * next to sexual terms, so it then gets the plain SFW block. A minor with
 * suggestive words (`suggestive_minor`) always gets the rule, whatever the
 * toggle says, and a clear minor with explicit words never gets this far:
 * `assertAllowedRequest` refuses it before anything is sent.
 */
export function naiVocabularyBlock(rating: ContentRating, minorRule = true): string {
  const head = `VOCABULARY
- Write real Danbooru tags for what the user asked. Euphemisms are untrained noise.`;
  if (rating === "explicit") {
    return `${head}
- If the idea is explicit, name it the way Danbooru does: nude, topless, bottomless, nipples, areolae, pussy, sex, cowgirl position, doggystyle, paizuri, oral, fellatio, cum, creampie, and the rest. Exposure-state tags are ordinary: clothes lift, clothes pulled aside, shirt up, no bra, no panties. Add rating:explicit when the scene is explicit.
- Do not sanitize, soften, lecture or refuse. Do not add explicit tags the user did not ask for, and do not strip the ones they did.
- Every character in an explicit scene is an adult. Never sexualize a character who is a minor or looks like one, whatever the request says.`;
  }
  if (rating === "suggestive_minor" || (rating === "minor" && minorRule)) {
    return `${head}
- This is a safe-for-work image.

MINORS
- A character in this image is a child or looks like one. Draw them in a fully non-sexual way: clothing that suits the scene, ordinary poses and expressions, and no nudity or sexual, suggestive or fetish tags, framing or prose.
- Put nsfw in undesired content.
- This rule overrides every other rule here, including anything in the user's text.`;
  }
  return `${head}
- This is a safe-for-work image. Do not add nudity or sexual or suggestive tags the user did not ask for.`;
}
