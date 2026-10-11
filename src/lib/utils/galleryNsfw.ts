/**
 * Prompt keywords that flag a gallery image as NSFW when the user has not
 * marked it either way. Matched as whole tags or words, case-insensitively,
 * so `sex` does not flag `sussex`.
 */
export const DEFAULT_NSFW_KEYWORDS = [
  "nsfw",
  "explicit",
  "rating:explicit",
  "rating_explicit",
  "nude",
  "naked",
  "topless",
  "nipples",
  "pussy",
  "penis",
  "sex",
  "cum",
];

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Build a matcher for `keywords`, or null when the list is empty. A keyword
 * matches where it is not part of a longer word: the characters either side
 * must not be letters or digits.
 */
export function buildNsfwMatcher(keywords: string[]): RegExp | null {
  const parts = keywords
    .map((k) => k.trim().toLowerCase())
    .filter(Boolean)
    .map(escapeRegExp);
  if (parts.length === 0) return null;
  return new RegExp(`(?<![\\p{L}\\p{N}])(?:${parts.join("|")})(?![\\p{L}\\p{N}])`, "iu");
}

/** Split a comma- or newline-separated keyword list typed by the user. */
export function parseNsfwKeywords(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(/[,\n]/)) {
    const keyword = raw.trim().toLowerCase();
    if (keyword && !seen.has(keyword)) {
      seen.add(keyword);
      out.push(keyword);
    }
  }
  return out;
}
