/**
 * Shared pending-state store for the "insert artist tag" confirmation modal.
 *
 * App.svelte renders the modal and subscribes to `artistInsert.pending`.
 * Any component (artist gallery page, bottom-panel favourites tab, etc.) can
 * call `artistInsert.request(tag)` to trigger the same UX.
 */
import { generation } from "./generation.svelte.js";

export type ArtistInsertPending = {
  tag: string;
  existingTags: string[];
  duplicate: boolean;
};

/**
 * Escape any unescaped `(` and `)` in a tag so it round-trips through the
 * prompt scheduler/highlighter without being interpreted as a weight group.
 * Already-escaped parens (preceded by `\`) are left untouched.
 */
function escapeParens(s: string): string {
  return s.replace(/(\\?)([()])/g, (_, esc, paren) => (esc ? esc + paren : "\\" + paren));
}

class ArtistInsertStore {
  pending = $state<ArtistInsertPending | null>(null);

  /**
   * Request insertion of an artist tag into the positive prompt.
   *
   * - If there are no existing `@` tags, applies immediately (add).
   * - If the exact tag is already present, opens the "already in prompt" hint.
   * - If other `@` tags exist, opens the replace/add confirmation modal.
   *
   * The `tag` may be provided with or without a leading `@`.
   */
  request(tag: string): void {
    // Strip leading @, convert underscores to spaces (danbooru convention),
    // escape any unescaped parens (so `artist (tag)` becomes `artist \(tag\)`),
    // then re-prefix with @. Already-escaped parens are left as-is so prompts
    // round-trip correctly through the scheduler/highlight parser.
    const cleaned = escapeParens(tag.replace(/^@+/, "").replace(/_/g, " ").trim());
    const withAt = "@" + cleaned;
    const existing = generation.positivePrompt.trim();
    const existingArtistTags = existing
      .split(",")
      .map((s) => s.trim())
      .filter((s) => s.startsWith("@"));
    if (existingArtistTags.some((t) => t.toLowerCase() === withAt.toLowerCase())) {
      this.pending = { tag: withAt, existingTags: existingArtistTags, duplicate: true };
    } else if (existingArtistTags.length > 0) {
      this.pending = { tag: withAt, existingTags: existingArtistTags, duplicate: false };
    } else {
      this.apply(withAt, "add");
    }
  }

  apply(withAt: string, mode: "add" | "replace"): void {
    // Defensive: normalize underscores → spaces and escape unescaped parens
    // in case a caller passes a raw danbooru-style tag rather than going
    // through request().
    const cleaned = "@" + escapeParens(withAt.replace(/^@+/, "").replace(/_/g, " ").trim());
    const existing = generation.positivePrompt.trim();
    let newPrompt: string;
    if (mode === "replace") {
      const stripped = existing
        .split(",")
        .map((s) => s.trim())
        .filter((s) => !s.startsWith("@"))
        .join(", ");
      newPrompt = stripped ? `${cleaned}, ${stripped}` : cleaned;
    } else {
      newPrompt = existing ? `${cleaned}, ${existing}` : cleaned;
    }
    generation.positivePrompt = newPrompt;
    generation.saveSettings();
    this.pending = null;
  }

  dismiss(): void {
    this.pending = null;
  }
}

export const artistInsert = new ArtistInsertStore();
