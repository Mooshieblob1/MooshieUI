import { createArtistGalleryClient } from "./client.js";
import { cdnFetch } from "../utils/cdnFetch.js";
import type {
  ArtistEntry,
  ArtistGalleryClient,
  ArtistManifest,
  ArtistSearchHit,
} from "./types.js";

/**
 * Svelte 5 rune-based wrapper around the artist gallery client.
 *
 * Usage:
 *   const store = createArtistGalleryStore(manifestUrl);
 *   await store.init();
 *   store.setQuery("dairi");
 *   $derived.by(() => store.results);
 */
export type ArtistSortField = "postCount" | "name" | "uniqueness";
export type ArtistSortDir = "asc" | "desc";
export type ArtistPageSize = 25 | 50 | 100;

export class ArtistGalleryStore {
  readonly client: ArtistGalleryClient;

  manifest = $state<ArtistManifest | null>(null);
  manifestError = $state<string | null>(null);
  manifestLoading = $state(false);

  query = $state("");
  results = $state<ArtistSearchHit[]>([]);
  searchLoading = $state(false);

  activeArtist = $state<ArtistEntry | null>(null);
  activeLoading = $state(false);

  // --- Persistent UI state (survives page unmount) ---------------------------
  /** Flat search index loaded on first gallery page visit. */
  allEntries = $state<ArtistSearchHit[]>([]);
  allEntriesLoading = $state(false);
  allEntriesError = $state<string | null>(null);
  /** Per-entry jitter multipliers for uniqueness sort. */
  uniquenessJitter = $state<Float32Array>(new Float32Array(0));

  sortField = $state<ArtistSortField>("postCount");
  sortDir = $state<ArtistSortDir>("desc");
  pageSize = $state<ArtistPageSize>(50);
  currentPage = $state(1);
  queryInput = $state("");

  showOnlyFavourites = $state(false);
  favouriteCategoryFilter = $state<"all" | "__uncat" | string>("all");

  /** Active lightbox entry, if any. */
  lightboxEntry = $state<ArtistEntry | null>(null);
  lightboxIndex = $state(-1);
  lightboxZoomed = $state(false);

  /** Last known scroll position of the gallery scroll container. */
  scrollTop = $state(0);

  /** Guard so rapid typing doesn't race older search results into state. */
  private searchSeq = 0;

  constructor(manifestUrl: string) {
    this.client = createArtistGalleryClient({ manifestUrl, fetchImpl: cdnFetch });
  }

  async init(): Promise<void> {
    if (this.manifest || this.manifestLoading) return;
    this.manifestLoading = true;
    this.manifestError = null;
    try {
      this.manifest = await this.client.loadManifest();
      // Kick off the search index fetch early; it drives typeahead and getArtist fallback.
      this.client.loadSearchIndex().catch((err) => {
        console.error("artist-gallery: search index load failed", err);
      });
    } catch (err) {
      this.manifestError = err instanceof Error ? err.message : String(err);
    } finally {
      this.manifestLoading = false;
    }
  }

  async setQuery(text: string): Promise<void> {
    this.query = text;
    const seq = ++this.searchSeq;
    if (!text.trim()) {
      this.results = [];
      this.searchLoading = false;
      return;
    }
    this.searchLoading = true;
    try {
      const hits = await this.client.search(text, { limit: 50 });
      if (seq === this.searchSeq) {
        this.results = hits;
      }
    } catch (err) {
      console.error("artist-gallery: search failed", err);
      if (seq === this.searchSeq) this.results = [];
    } finally {
      if (seq === this.searchSeq) this.searchLoading = false;
    }
  }

  async openArtist(slugOrTag: string): Promise<void> {
    this.activeLoading = true;
    try {
      this.activeArtist = await this.client.getArtist(slugOrTag);
    } catch (err) {
      console.error("artist-gallery: openArtist failed", err);
      this.activeArtist = null;
    } finally {
      this.activeLoading = false;
    }
  }

  closeArtist(): void {
    this.activeArtist = null;
  }
}

/**
 * Convenience: singleton cached by manifestUrl. Handy for inline popovers that
 * need to share the same caches as the gallery page without plumbing an
 * instance through the component tree.
 */
const stores = new Map<string, ArtistGalleryStore>();
export function createArtistGalleryStore(manifestUrl: string): ArtistGalleryStore {
  const existing = stores.get(manifestUrl);
  if (existing) return existing;
  const created = new ArtistGalleryStore(manifestUrl);
  stores.set(manifestUrl, created);
  return created;
}
