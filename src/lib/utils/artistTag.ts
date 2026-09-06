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
import { escapeEmphasisMarks, unescapeEmphasisMarks } from "./promptSyntaxEscape.js";

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
 * unescaped parens escaped, emphasis escapes dropped, trimmed. This is the
 * form `@artist \(x\)`, `artist_(x)` and `neverland\+` all collapse to, so
 * they can be compared or removed regardless of which mode wrote them.
 *
 * Parens are normalised by *adding* the escape and `+`/`-` by *removing* it,
 * because that is the canonical form each one has on the way in: a tag name
 * never carries its own backslash, and `escapeArtistParens()` is what the
 * prompt-facing form already used.
 */
export function artistTagBody(tag: string): string {
  return escapeArtistParens(
    unescapeEmphasisMarks(stripArtistSigil(tag).replace(/_/g, " ").trim()),
  );
}

/**
 * The prompt-facing form of an artist tag: the comparable body with any
 * trailing `+`/`-` run escaped, so an artist whose name ends in one (`k+`,
 * `neverland+`, `grs-`) is not rewritten to `(k:1.10)` by the send-time
 * emphasis translator. `formatArtistTag()` on the generation store adds the
 * mode's sigil on top of this.
 */
export function artistTagPromptBody(tag: string): string {
  return escapeEmphasisMarks(artistTagBody(tag));
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
    .replace(/\\([()[\]+-])/g, "$1")
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

/**
 * Drop the tags `shouldDrop` flags from a prompt while keeping its layout.
 *
 * Lines that hold no dropped tag come back byte for byte, so a NovelAI `Text:`
 * block, blank lines between rendered strings, and prose lines all survive an
 * artist tag toggle. A line that loses tags is rejoined with ", " and keeps a
 * trailing comma if it had one; a line left with nothing is removed.
 */
export function filterPromptTags(prompt: string, shouldDrop: (tag: string) => boolean): string {
  return prompt
    .split("\n")
    .map((line) => {
      const parts = line.split(",");
      if (!parts.some((part) => part.trim() && shouldDrop(part.trim()))) return line;
      const kept = parts.map((part) => part.trim()).filter((part) => part && !shouldDrop(part));
      if (kept.length === 0) return null;
      return kept.join(", ") + (/,\s*$/.test(line) ? "," : "");
    })
    .filter((line): line is string => line !== null)
    .join("\n");
}
