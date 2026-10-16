/**
 * What a prompt-assistant request is about, judged from the user's own words.
 *
 * Two questions, asked of the positive text only: does it involve a minor, and
 * does it ask for sexual or suggestive content. Never the negative prompt,
 * where "nsfw" or "nude" is the thing being kept out of the image.
 *
 * - Both at once is refused here, before anything is sent. That line is fixed
 *   and nothing the user writes moves it.
 * - A minor alone is an ordinary safe-for-work request (a family at the beach,
 *   a kid with a kite). It goes out with a rule that keeps every such
 *   character non-sexual, and without the explicit vocabulary the V5
 *   specification carries for adult scenes. Sending that vocabulary, plus "do
 *   not refuse", beside a child tag is what made hosted providers decline
 *   requests that were completely SFW.
 * - Sexual content alone keeps the explicit vocabulary, as before.
 *
 * Deliberately broad on both lists: a false "minor" only adds a safety rule to
 * an SFW request, and a false "sexual" next to a real minor refuses something
 * that was borderline anyway. Narrow lists are where the hard line would leak.
 *
 * Leaf util with no imports, so `scripts/tests/contentSafety.test.ts` can load
 * it directly.
 */

export type ContentRating = "sfw" | "minor" | "explicit" | "blocked";

/** Thrown, as the message, when a request pairs a minor with sexual content. */
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
  /ロリータ/gu,
];

/** Sexual or suggestive words and tags, cancelled by a negation in front. */
const SEXUAL_TERMS = [
  "nsfw", "nude", "nudes", "nudity", "naked", "topless", "bottomless", "nipple",
  "nipples", "areola", "areolae", "areolas", "pussy", "vagina", "vaginal", "vulva",
  "labia", "clitoris", "penis", "penises", "erection", "testicles", "scrotum", "anus", "anal", "sex", "sexual", "sexually",
  "sexualized", "sexualised", "sexy", "seductive", "seduce", "seducing",
  "seduction", "erotic", "erotica", "eroticism", "lewd", "hentai", "porn", "porno",
  "pornography", "pornographic", "ecchi", "smut", "smutty", "explicit", "xxx",
  "x rated", "r18", "r 18", "18+", "cum", "cumshot", "creampie", "semen",
  "fellatio", "blowjob", "handjob", "footjob", "titjob", "paizuri", "cunnilingus",
  "masturbation", "masturbating", "masturbate", "fingering", "orgasm", "ahegao",
  "bukkake", "intercourse", "cowgirl position", "doggystyle", "doggy style",
  "missionary position", "rape", "raped", "molest", "molested", "molestation",
  "grope", "groped", "groping", "fondle", "fondling", "lingerie", "panties",
  "pantsu", "pantyshot", "panty shot", "upskirt", "downblouse", "cameltoe",
  "underwear only", "micro bikini", "bdsm", "bondage", "sex toy", "vibrator",
  "dildo", "condom", "aroused", "arousal", "horny", "spread legs", "spread pussy",
  "breast", "breasts", "boobs", "tits", "cleavage", "busty", "voluptuous", "thicc",
  "ass", "crotch", "groin", "undress", "undressing", "undressed", "unclothed",
  "striptease", "stripping", "suggestive", "provocative", "fanservice",
  "fan service", "rating:explicit", "rating:questionable", "rating:sensitive",
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
]);
/** Skipped when looking back for a negation: "not a child", "no visible nudity". */
const FILLER = new Set(["a", "an", "any", "the", "visible", "of"]);

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
const SEXUAL_RE = termPattern(SEXUAL_TERMS);
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
  return text.normalize("NFKC").toLowerCase().replace(/-\d*(?:\.\d+)?::[\s\S]*?::/g, " ");
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

/** Whether any match of `re` in `text` is not negated. */
function anyAffirmed(re: RegExp, text: string): boolean {
  for (const m of text.matchAll(re)) {
    if (!negated(text, m.index ?? 0)) return true;
  }
  return false;
}

/** Whether the positive text puts a minor in the scene. */
export function mentionsMinor(text: string): boolean {
  let t = normalize(text);
  for (const re of MINOR_FALSE_FRIENDS) t = t.replace(re, " ");
  if (anyAffirmed(MINOR_RE, t)) return true;
  if (MINOR_TERMS_CJK.some((w) => t.includes(w))) return true;
  for (const m of t.matchAll(AGE_RE)) {
    const age = Number(m[1] ?? m[2] ?? m[3]);
    if (age < 18 && !negated(t, m.index ?? 0)) return true;
  }
  return false;
}

/** Whether the positive text asks for sexual or suggestive content. */
export function mentionsSexualContent(text: string): boolean {
  let t = normalize(text);
  for (const re of SEXUAL_FALSE_FRIENDS) t = t.replace(re, " ");
  if (EXPOSURE_RE.test(t)) return true;
  if (anyAffirmed(SEXUAL_RE, t)) return true;
  return SEXUAL_TERMS_CJK.some((w) => t.includes(w));
}

/**
 * Rate a request from every positive text it carries: the instruction, the
 * earlier user turns of its session, and any current prompt it edits. Rated
 * together, so a minor in one turn and sexual content in a later one is still
 * the combination that is refused.
 */
export function rateRequest(texts: readonly string[]): ContentRating {
  const joined = texts.filter((t) => t && t.trim()).join("\n");
  const minor = mentionsMinor(joined);
  const sexual = mentionsSexualContent(joined);
  if (minor && sexual) return "blocked";
  if (sexual) return "explicit";
  return minor ? "minor" : "sfw";
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

/** Throw before sending a request that pairs a minor with sexual content. */
export function assertAllowedRequest(texts: readonly string[]): ContentRating {
  const rating = rateRequest(texts);
  if (rating === "blocked") throw new Error(BLOCKED_MINOR_CONTENT);
  return rating;
}

/**
 * The saved characters a request may send, given its rating.
 *
 * Saved characters ride along in the system turn whether or not the user names
 * them. In an explicit request a saved character drawn as a minor is left out,
 * and naming one (by any word of their name) refuses the request: the user's
 * text would not say "child", but the saved card does.
 */
export function savedCharactersForRequest<T extends { name: string; prompt: string }>(
  characters: readonly T[],
  rating: ContentRating,
  texts: readonly string[],
): T[] {
  if (rating !== "explicit") return characters.slice();
  const said = normalize(texts.join("\n"));
  const kept: T[] = [];
  for (const c of characters) {
    if (!mentionsMinor(`${c.name}\n${c.prompt}`)) {
      kept.push(c);
      continue;
    }
    const words = normalize(c.name)
      .split(/[^\p{L}\p{N}]+/u)
      .filter((w) => w.length >= 3 && !NAME_STOPWORDS.has(w));
    if (words.some((w) => termPattern([w], "iu").test(said))) throw new Error(BLOCKED_MINOR_CONTENT);
  }
  return kept;
}

/**
 * The VOCABULARY block of the V5 rewrite for a request of this rating.
 *
 * Only an explicit request hears the explicit tag list. A request with a minor
 * in it gets the non-sexual rule instead, stated as overriding everything else.
 */
export function naiVocabularyBlock(rating: ContentRating): string {
  const head = `VOCABULARY
- Write real Danbooru tags for what the user asked. Euphemisms are untrained noise.`;
  if (rating === "explicit") {
    return `${head}
- If the idea is explicit, name it the way Danbooru does: nude, topless, bottomless, nipples, areolae, pussy, sex, cowgirl position, doggystyle, paizuri, oral, fellatio, cum, creampie, and the rest. Exposure-state tags are ordinary: clothes lift, clothes pulled aside, shirt up, no bra, no panties. Add rating:explicit when the scene is explicit.
- Do not sanitize, soften, lecture or refuse. Do not add explicit tags the user did not ask for, and do not strip the ones they did.
- Every character in an explicit scene is an adult. Never sexualize a character who is a minor or looks like one, whatever the request says.`;
  }
  if (rating === "minor") {
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
