import type { ArtistPreviewVariant } from "../artist-gallery/previewRecipe.js";

/**
 * Locally generated artist previews.
 *
 * The CDN index ships a preview for ~42k artist tags; ~7.4k tags the model
 * knows have none (issue #527). Those render as placeholder cards with a
 * "generate this preview yourself" action, and the result is recorded here so
 * the card shows the local image on later visits.
 *
 * Only the gallery filename is stored: gallery images live on disk as JXL and
 * are resolved for display through `loadGalleryImageDisplay()`.
 */

interface LocalPreviewSlot {
  p1?: string;
  p2?: string;
}

const STORAGE_KEY = "artist-gallery-local-previews";

function slotKey(variant: ArtistPreviewVariant): "p1" | "p2" {
  return variant === 2 ? "p2" : "p1";
}

function runKey(slug: string, variant: ArtistPreviewVariant): string {
  return `${slug}::p${variant}`;
}

class ArtistLocalPreviewsStore {
  /** slug -> gallery filenames, one per variant. Persisted. */
  previews = $state<Record<string, LocalPreviewSlot>>({});
  /** `slug::pN` -> true while a generation is in flight. Not persisted. */
  running = $state<Record<string, boolean>>({});

  /** prompt_id -> target card. In-memory only: a queued job dies with the app. */
  private pending = new Map<string, { slug: string; variant: ArtistPreviewVariant }>();

  constructor() {
    this.load();
  }

  private load(): void {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      if (!parsed || typeof parsed !== "object") return;
      const clean: Record<string, LocalPreviewSlot> = {};
      for (const [slug, slot] of Object.entries(parsed)) {
        if (!slot || typeof slot !== "object") continue;
        const s = slot as LocalPreviewSlot;
        const next: LocalPreviewSlot = {};
        if (typeof s.p1 === "string" && s.p1) next.p1 = s.p1;
        if (typeof s.p2 === "string" && s.p2) next.p2 = s.p2;
        if (next.p1 || next.p2) clean[slug] = next;
      }
      this.previews = clean;
    } catch {
      /* localStorage unavailable or corrupt; start empty */
    }
  }

  private persist(): void {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.previews));
    } catch {
      /* ignore */
    }
  }

  isRunning(slug: string, variant: ArtistPreviewVariant): boolean {
    return this.running[runKey(slug, variant)] === true;
  }

  /** Mark a submitted generation so its finished images can be routed back. */
  attach(promptId: string, slug: string, variant: ArtistPreviewVariant): void {
    this.pending.set(promptId, { slug, variant });
    this.running = { ...this.running, [runKey(slug, variant)]: true };
  }

  /** Claim the target for a finished prompt. Consumes the mapping. */
  resolve(promptId: string): { slug: string; variant: ArtistPreviewVariant } | null {
    const target = this.pending.get(promptId);
    if (!target) return null;
    this.pending.delete(promptId);
    return target;
  }

  record(slug: string, variant: ArtistPreviewVariant, filename: string): void {
    const slot = { ...(this.previews[slug] ?? {}), [slotKey(variant)]: filename };
    this.previews = { ...this.previews, [slug]: slot };
    this.clearRunning(slug, variant);
    this.persist();
  }

  /** Drop a mapping whose file no longer loads (deleted from the gallery). */
  forget(slug: string, variant: ArtistPreviewVariant): void {
    const existing = this.previews[slug];
    if (!existing) return;
    const slot = { ...existing };
    delete slot[slotKey(variant)];
    const next = { ...this.previews };
    if (slot.p1 || slot.p2) next[slug] = slot;
    else delete next[slug];
    this.previews = next;
    this.clearRunning(slug, variant);
    this.persist();
  }

  /** Generation failed or was cancelled: stop showing the spinner. */
  fail(slug: string, variant: ArtistPreviewVariant): void {
    this.clearRunning(slug, variant);
  }

  /**
   * Cancel all in-flight previews at once (queue cleared, execution error with
   * no prompt_id). Drains `pending` and clears every `running` entry.
   */
  failAll(): void {
    for (const { slug, variant } of this.pending.values()) {
      this.clearRunning(slug, variant);
    }
    this.pending.clear();
  }

  private clearRunning(slug: string, variant: ArtistPreviewVariant): void {
    const key = runKey(slug, variant);
    if (!(key in this.running)) return;
    const next = { ...this.running };
    delete next[key];
    this.running = next;
  }
}

export const artistLocalPreviews = new ArtistLocalPreviewsStore();
