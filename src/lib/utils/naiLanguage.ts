/**
 * Target-language handling for the NovelAI V5 prompt enhance.
 *
 * NovelAI trained V5 on English and Japanese, and reports the other four
 * languages here as tester-grade: they work often enough to offer, badly enough
 * that the UI has to say so. Nothing in this file guesses at quality - it only
 * decides which language the user typed in and what to tell the model about it.
 *
 * Leaf util. It must not import any store, or the generation store that needs it
 * would form a cycle.
 */

export type NaiLanguage = "en" | "ja" | "zh" | "de" | "es" | "pt";

/** `"auto"` means detect from the prompt; anything else is the user's override. */
export type NaiLanguageChoice = NaiLanguage | "auto";

export interface NaiLanguageInfo {
  code: NaiLanguage;
  /** Endonym, shown in the select. Language names are not translated. */
  label: string;
  /** English name, written into the prompt so the model is not reading a glyph. */
  englishName: string;
  /**
   * Officially supported by NovelAI. The other four are tester languages whose
   * results vary, because they were not a primary focus during training.
   */
  official: boolean;
}

export const NAI_LANGUAGES: readonly NaiLanguageInfo[] = [
  { code: "en", label: "English", englishName: "English", official: true },
  { code: "ja", label: "日本語", englishName: "Japanese", official: true },
  { code: "zh", label: "中文", englishName: "Chinese", official: false },
  { code: "de", label: "Deutsch", englishName: "German", official: false },
  { code: "es", label: "Español", englishName: "Spanish", official: false },
  { code: "pt", label: "Português", englishName: "Portuguese", official: false },
] as const;

export function naiLanguageInfo(code: NaiLanguage): NaiLanguageInfo {
  return NAI_LANGUAGES.find((l) => l.code === code) ?? NAI_LANGUAGES[0];
}

/** Hiragana and katakana, including the halfwidth katakana block. */
const KANA = /[぀-ゟ゠-ヿｦ-ﾝ]/;
/** CJK unified ideographs, shared by Japanese kanji and Chinese hanzi. */
const HAN = /[㐀-䶿一-鿿豈-﫿]/;

/**
 * Latin-script scoring tables.
 *
 * Diacritics are the strong signal and stopwords the weak one, so a single
 * diacritic outweighs a single stopword. Portuguese and Spanish share most of
 * their function words, so only the words that genuinely separate them are
 * listed: `de`, `la`, `que` and friends would score both and decide nothing.
 */
const LATIN_SIGNALS: Record<"de" | "es" | "pt", { chars: RegExp; words: readonly string[] }> = {
  de: {
    chars: /[äöüß]/,
    words: ["der", "die", "das", "und", "mit", "eine", "einen", "einem", "nicht", "auf", "im", "ist", "sich", "sie", "vor", "über"],
  },
  es: {
    chars: /[ñ¿¡]/,
    words: ["el", "los", "las", "una", "unos", "con", "sobre", "pelo", "ojos", "está", "muy", "sus", "desde", "hacia"],
  },
  pt: {
    chars: /[ãõçâê]/,
    words: ["um", "uma", "com", "não", "dos", "das", "olhos", "cabelo", "está", "muito", "seus", "sobre", "pela", "pelo"],
  },
};

const LATIN_WEIGHT_CHAR = 3;
const LATIN_WEIGHT_WORD = 1;

/**
 * Best guess at which language a prompt was typed in.
 *
 * Script settles Japanese and Chinese outright: kana can only be Japanese, and
 * Han without kana is Chinese in every case this feature cares about. Latin
 * script is scored instead, and defaults to English on a tie or a blank, which
 * is both the common case and the safe one.
 *
 * Spanish against Portuguese on a short Latin prompt is genuinely unreliable and
 * is not worth more machinery: the override exists for exactly that, and a wrong
 * guess is cheap because the user sees the result in the modal before anything
 * is applied.
 */
export function detectPromptLanguage(prompt: string): NaiLanguage {
  const text = (prompt ?? "").trim();
  if (!text) return "en";
  if (KANA.test(text)) return "ja";
  if (HAN.test(text)) return "zh";

  const lower = text.toLowerCase();
  const words = lower.split(/[^\p{L}]+/u).filter(Boolean);
  let best: NaiLanguage = "en";
  let bestScore = 0;
  for (const code of ["de", "es", "pt"] as const) {
    const signals = LATIN_SIGNALS[code];
    let score = 0;
    for (const ch of lower) if (signals.chars.test(ch)) score += LATIN_WEIGHT_CHAR;
    for (const w of words) if (signals.words.includes(w)) score += LATIN_WEIGHT_WORD;
    // Strictly greater, so a tie keeps the earlier candidate and an all-zero
    // sweep leaves English standing.
    if (score > bestScore) {
      bestScore = score;
      best = code;
    }
  }
  return bestScore > 0 ? best : "en";
}

/** Resolve the user's select value against the prompt they typed. */
export function resolveNaiLanguage(
  choice: NaiLanguageChoice,
  prompt: string,
): NaiLanguage {
  return choice === "auto" ? detectPromptLanguage(prompt) : choice;
}

/**
 * What the model is told about the target language.
 *
 * Tags and complexity keywords stay in English because they are trained tokens,
 * not prose: translating `high complexity` produces a phrase the model has never
 * seen conditioned on anything. Only the natural language body and any `Text:`
 * string follow the user's language.
 */
export function naiLanguageDirective(code: NaiLanguage): string {
  const info = naiLanguageInfo(code);
  if (code === "en") {
    return "Write the entire prompt in English.";
  }
  return `Write the natural language body of the base prompt in ${info.englishName}, and write any Text: string in ${info.englishName}.

Danbooru-style tags, character box tags, quality tags and the complexity keywords (low complexity, medium complexity, high complexity, ultra complexity) stay in English in every language. They are trained tokens rather than prose, and translating them produces something the model has never seen.`;
}
