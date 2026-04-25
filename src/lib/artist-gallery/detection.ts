/**
 * Artist tag detection utilities.
 *
 * Scans a generation prompt (typically `image.metadata.positive_prompt`) and
 * returns any artist tags that match the cached search index.  Detects both
 * `@artist` and plain `artist_name` tokens, with the former given priority.
 */

import type { ArtistSearchHit } from "./types.js";

/** Lookup map keyed by normalized tag (lowercase, no leading @). */
export type ArtistTagIndex = Map<string, ArtistSearchHit>;

/**
 * Build a lookup index from a flat array of search hits.  Both the slug and
 * the raw tag (lowercased, `@` stripped) are indexed so either form matches.
 */
export function buildArtistTagIndex(hits: ArtistSearchHit[]): ArtistTagIndex {
  const map: ArtistTagIndex = new Map();
  for (const hit of hits) {
    map.set(hit.slug.toLowerCase(), hit);
    const rawTag = hit.tag.replace(/^@+/, "").toLowerCase();
    if (rawTag && !map.has(rawTag)) map.set(rawTag, hit);
    // Also index with backslash-escaped parens so prompts written with SD
    // attention-escape syntax (e.g. `mitsu_\(mitsu_art\)`) match directly.
    const escapedTag = rawTag.replace(/\(/g, "\\(").replace(/\)/g, "\\)");
    if (escapedTag !== rawTag && !map.has(escapedTag)) map.set(escapedTag, hit);
  }
  return map;
}

/**
 * Split a prompt into normalized tag tokens.  Handles weighted syntax like
 * `(tag:1.2)`, `[tag]`, `<lora:x:1>`, and whitespace/underscore variants.
 * Returns tokens lowercased with leading `@` removed; the original prefix
 * is preserved alongside via the second tuple element for priority detection.
 */
function tokenize(prompt: string): Array<{ token: string; hadAtPrefix: boolean }> {
  const out: Array<{ token: string; hadAtPrefix: boolean }> = [];
  for (const chunk of prompt.split(/[,\n]/)) {
    let t = chunk.trim();
    if (!t) continue;
    // Skip LoRA / embedding syntax
    if (t.startsWith("<") && t.endsWith(">")) continue;
    // Unescape backslash-escaped parens/brackets that SD/ComfyUI uses to
    // prevent attention-weight parsing: \( → (, \) → ), \[ → [, \] → ]
    t = t.replace(/\\([()\[\]])/g, "$1");
    // Strip surrounding weight parens/brackets (repeated)
    while (/^[\(\[]/.test(t) && /[\)\]]$/.test(t)) {
      t = t.slice(1, -1).trim();
      if (!t) break;
    }
    if (!t) continue;
    // Strip trailing weight like ":1.2"
    t = t.replace(/:[-\d.]+\s*$/g, "").trim();
    if (!t) continue;
    const hadAt = t.startsWith("@");
    if (hadAt) t = t.replace(/^@+/, "").trim();
    if (!t) continue;
    // Skip MooshieUI directives like `@preset:foo` — those are inline preset
    // expansions, not artist references. Only relevant when the token had an
    // `@` prefix; a bare `preset:foo` would never reach here as a tag anyway.
    if (hadAt && /^preset:/i.test(t)) continue;
    // Normalise: spaces → underscores, lowercase
    const normalized = t.toLowerCase().replace(/\s+/g, "_");
    if (normalized) out.push({ token: normalized, hadAtPrefix: hadAt });
  }
  return out;
}

/**
 * Detect artist tags referenced in a prompt string.  Returns a deduplicated
 * list of search hits in first-seen order, with `@`-prefixed matches given
 * priority (i.e. they appear first and outrank plain collisions).
 */
export function detectArtistsInPrompt(
  prompt: string | null | undefined,
  index: ArtistTagIndex,
): ArtistSearchHit[] {
  if (!prompt || index.size === 0) return [];
  const atMatches = new Map<string, ArtistSearchHit>();
  const plainMatches = new Map<string, ArtistSearchHit>();
  for (const { token, hadAtPrefix } of tokenize(prompt)) {
    // Primary lookup: tag form with parens, e.g. "mitsu_(mitsu_art)"
    let hit = index.get(token);
    // Secondary lookup: slug form with parens stripped, e.g. "mitsu_mitsu_art".
    // This covers cases where the stored tag uses a different paren convention
    // than the prompt or when the artist index only has the slug form indexed.
    if (!hit) {
      const slugForm = token
        .replace(/[()]/g, "")        // strip all parens
        .replace(/_+/g, "_")         // collapse multiple underscores
        .replace(/^_|_$/g, "");      // trim leading/trailing underscores
      if (slugForm && slugForm !== token) hit = index.get(slugForm);
    }
    if (!hit) continue;
    const bucket = hadAtPrefix ? atMatches : plainMatches;
    if (!bucket.has(hit.slug)) bucket.set(hit.slug, hit);
  }
  // `@` hits first; remove any plain hits that duplicate an @ hit
  const result: ArtistSearchHit[] = [];
  for (const hit of atMatches.values()) result.push(hit);
  for (const hit of plainMatches.values()) {
    if (!atMatches.has(hit.slug)) result.push(hit);
  }
  return result;
}

/** Format an artist slug as a board name.  Keeps the `@` convention. */
export function artistBoardName(slug: string): string {
  return `@${slug}`;
}
