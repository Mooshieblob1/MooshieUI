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
    const withAt = "@" + tag.replace(/^@/, "");
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
    const existing = generation.positivePrompt.trim();
    let newPrompt: string;
    if (mode === "replace") {
      const stripped = existing
        .split(",")
        .map((s) => s.trim())
        .filter((s) => !s.startsWith("@"))
        .join(", ");
      newPrompt = stripped ? `${withAt}, ${stripped}` : withAt;
    } else {
      newPrompt = existing ? `${withAt}, ${existing}` : withAt;
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
