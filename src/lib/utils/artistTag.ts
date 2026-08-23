/**
 * Pure helpers for the artist-tag sigil.
 *
 * The `@` prefix on an artist tag is mode-dependent, so every feature that
 * inserts, copies, matches or removes one has to agree on the same rules.
 * This module holds the string work; the mode decision itself lives on the
 * generation store as `artistTagPrefix` / `formatArtistTag()`, because only
 * the store knows which checkpoint is selected.
 *
 * Nothing here imports a store, so it stays safe to use from leaf utilities
 * and from the store itself.
 */

import type { ArtistTagIndex } from "../artist-gallery/detection.js";

/**
 * Escape any unescaped `(` and `)` in a tag so it round-trips through the
 * prompt scheduler/highlighter without being interpreted as a weight group.
 * Already-escaped parens (preceded by `\`) are left untouched.
 */
export function escapeArtistParens(s: string): string {
  return s.replace(/(\\?)([()])/g, (_, esc, paren) => (esc ? esc + paren : "\\" + paren));
}

/** Drop any leading `@` sigils. Safe on tags that never had one. */
export function stripArtistSigil(tag: string): string {
  return tag.replace(/^@+/, "");
}

/**
 * The comparable body of an artist tag: no sigil, underscores as spaces,
 * unescaped parens escaped, trimmed. This is the form both `@artist \(x\)`
 * and `artist_(x)` collapse to, so the two can be compared or removed
 * regardless of which mode wrote them.
 */
export function artistTagBody(tag: string): string {
  return escapeArtistParens(stripArtistSigil(tag).replace(/_/g, " ").trim());
}

/** Case-insensitive comparison of two artist tags, ignoring sigil and spacing. */
export function artistTagBodiesMatch(a: string, b: string): boolean {
  return artistTagBody(a).toLowerCase() === artistTagBody(b).toLowerCase();
}

/**
 * Split a prompt into individual tags. Tags are separated by commas *or*
 * newlines, so detecting/removing artist tags must honour both, otherwise a
 * tag on its own line escapes duplicate detection and toggle-off.
 */
export function splitPromptTags(prompt: string): string[] {
  return prompt
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** The key form `buildArtistTagIndex()` uses: lowercase, underscored, no sigil. */
export function artistIndexKey(tag: string): string {
  return stripArtistSigil(tag)
    .replace(/\\([()[\]])/g, "$1")
    .toLowerCase()
    .trim()
    .replace(/\s+/g, "_");
}

/**
 * Is this prompt token an artist tag?
 *
 * With a sigil (ComfyUI mode) the `@` is the whole answer, and `@preset:foo`
 * is excluded because that is the inline prompt-chunk directive rather than an
 * artist. With no sigil (NovelAI mode) a bare artist tag is indistinguishable
 * from any other danbooru tag, so the artist index is the only thing that can
 * tell them apart; when the index has not loaded yet this returns false rather
 * than guessing, which degrades to "add" instead of silently eating a tag the
 * user did not mean to replace.
 */
export function isArtistTagToken(
  token: string,
  prefix: string,
  index?: ArtistTagIndex | null,
): boolean {
  if (prefix) {
    return token.startsWith(prefix) && !/^@+preset:/i.test(token);
  }
  if (!index || index.size === 0) return false;
  return index.has(artistIndexKey(token));
}
