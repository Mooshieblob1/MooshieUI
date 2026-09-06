/**
 * Shared pending-state store for the "insert artist tag" confirmation modal.
 *
 * App.svelte renders the modal and subscribes to `artistInsert.pending`.
 * Any component (artist gallery page, bottom-panel favourites tab, etc.) can
 * call `artistInsert.request(tag)` to trigger the same UX.
 *
 * The `@` sigil is mode-dependent, so both the tag written into the prompt and
 * the test for tags already in it come from `generation.artistTagPrefix` and
 * `isArtistTagToken()` rather than a hardcoded `"@"`. In NovelAI mode there is
 * no sigil, so the artist index is what tells an artist tag apart from any
 * other danbooru tag.
 */
import { generation } from "./generation.svelte.js";
import { gallery } from "./gallery.svelte.js";
import {
  artistTagBodiesMatch,
  filterPromptTags,
  isArtistTagToken,
  splitPromptTags,
} from "../utils/artistTag.js";

export type ArtistInsertPending = {
  tag: string;
  existingTags: string[];
  duplicate: boolean;
};

class ArtistInsertStore {
  pending = $state<ArtistInsertPending | null>(null);

  /**
   * Artist tags already in the positive prompt, in prompt order.
   *
   * Sigil mode reads them straight off the `@`. Sigil-free mode needs the
   * artist index; if it has not loaded, this comes back empty and the caller
   * degrades to a plain add rather than replacing tags it cannot identify.
   */
  private existingArtistTags(): string[] {
    const prefix = generation.artistTagPrefix;
    const index = gallery.artistIndexReady ? gallery.artistTagIndex : null;
    return splitPromptTags(generation.positivePrompt.trim()).filter((t) =>
      isArtistTagToken(t, prefix, index),
    );
  }

  /**
   * Request insertion of an artist tag into the positive prompt.
   *
   * - If there are no existing artist tags, applies immediately (add).
   * - If the same artist is already present, removes it (toggle, like LoRAs).
   * - If other artist tags exist, opens the replace/add confirmation modal.
   *
   * The `tag` may be provided with or without a leading `@`; whichever sigil
   * the current mode wants is applied on the way in.
   */
  request(tag: string): void {
    const formatted = generation.formatArtistTag(tag);
    const existingArtistTags = this.existingArtistTags();
    if (existingArtistTags.some((t) => artistTagBodiesMatch(t, formatted))) {
      this.remove(formatted);
      return;
    } else if (existingArtistTags.length > 0) {
      this.pending = { tag: formatted, existingTags: existingArtistTags, duplicate: false };
    } else {
      this.apply(formatted, "add");
    }
  }

  /**
   * Remove a single artist tag from the positive prompt (case-insensitive).
   *
   * Matching is on the tag body, not the whole token, so a tag inserted in one
   * mode is still removable after switching to the other.
   */
  remove(tag: string): void {
    const existing = generation.positivePrompt.trim();
    if (!existing) return;
    // Layout-preserving: only the lines holding the tag are rewritten, so a
    // NovelAI `Text:` block or prose line survives the toggle untouched.
    generation.positivePrompt = filterPromptTags(existing, (t) => artistTagBodiesMatch(t, tag));
    generation.saveSettings();
    this.pending = null;
  }

  apply(tag: string, mode: "add" | "replace"): void {
    // Defensive: re-run the mode's formatting in case a caller passes a raw
    // danbooru-style tag rather than going through request().
    const cleaned = generation.formatArtistTag(tag);
    const existing = generation.positivePrompt.trim();
    let newPrompt: string;
    if (mode === "replace") {
      const drop = new Set(this.existingArtistTags());
      const stripped = filterPromptTags(existing, (s) => drop.has(s));
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
