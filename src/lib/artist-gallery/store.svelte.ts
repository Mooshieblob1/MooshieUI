import { createArtistGalleryClient } from "./client.js";
import { cdnFetch, proxiedCdnUrl } from "../utils/cdnFetch.js";
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
  /**
   * Show artists the model knows that have no CDN preview yet (issue #527).
   * On by default: hiding them is what made them unfindable in the first place.
   */
  includeNoPreview = $state(true);

  /** Active lightbox entry, if any. */
  lightboxEntry = $state<ArtistEntry | null>(null);
  lightboxIndex = $state(-1);
  lightboxZoomed = $state(false);

  /** Last known scroll position of the gallery scroll container. */
  scrollTop = $state(0);

  // --- Image variant toggle (index v2+; inert on single-variant v1 data) -----
  /** Globally selected variant (1-based). Persisted across sessions. */
  globalVariant = $state(1);
  /**
   * Per-card variant overrides. An entry is stored ONLY while it differs from
   * `globalVariant` — a card flipped back to the global value drops its
   * override so it re-syncs on the next global toggle.
   */
  variantOverrides = $state<Record<string, number>>({});

  /** Guard so rapid typing doesn't race older search results into state. */
  private searchSeq = 0;

  private static readonly GLOBAL_VARIANT_KEY = "artist-gallery-global-variant";
  private static readonly VARIANT_OVERRIDES_KEY = "artist-gallery-variant-overrides";
  private static readonly INCLUDE_NO_PREVIEW_KEY = "artist-gallery-include-no-preview";

  constructor(manifestUrl: string) {
    this.client = createArtistGalleryClient({ manifestUrl, fetchImpl: cdnFetch });
    this.loadVariants();
    this.loadIncludeNoPreview();
  }

  private loadVariants(): void {
    try {
      const g = localStorage.getItem(ArtistGalleryStore.GLOBAL_VARIANT_KEY);
      if (g !== null) {
        const n = parseInt(g, 10);
        if (Number.isFinite(n) && n >= 1) this.globalVariant = n;
      }
      const o = localStorage.getItem(ArtistGalleryStore.VARIANT_OVERRIDES_KEY);
      if (o) {
        const parsed = JSON.parse(o);
        if (parsed && typeof parsed === "object") {
          const clean: Record<string, number> = {};
          for (const [slug, v] of Object.entries(parsed)) {
            if (typeof v === "number" && Number.isFinite(v) && v >= 1) clean[slug] = v;
          }
          this.variantOverrides = clean;
        }
      }
    } catch {
      /* localStorage unavailable; defaults are fine */
    }
  }

  private persistVariants(): void {
    try {
      localStorage.setItem(ArtistGalleryStore.GLOBAL_VARIANT_KEY, String(this.globalVariant));
      localStorage.setItem(
        ArtistGalleryStore.VARIANT_OVERRIDES_KEY,
        JSON.stringify(this.variantOverrides),
      );
    } catch {
      /* ignore */
    }
  }

  private loadIncludeNoPreview(): void {
    try {
      const raw = localStorage.getItem(ArtistGalleryStore.INCLUDE_NO_PREVIEW_KEY);
      if (raw !== null) this.includeNoPreview = raw === "true";
    } catch {
      /* localStorage unavailable; the default (on) is fine */
    }
  }

  setIncludeNoPreview(value: boolean): void {
    this.includeNoPreview = value;
    try {
      localStorage.setItem(ArtistGalleryStore.INCLUDE_NO_PREVIEW_KEY, String(value));
    } catch {
      /* ignore */
    }
  }

  /** Variant a card should display (1-based): its override if any, else global. */
  resolveVariant(slug: string): number {
    return this.variantOverrides[slug] ?? this.globalVariant;
  }

  /**
   * Set a single card's variant. The override is stored ONLY while it differs
   * from the global value; setting it equal to the global removes the override
   * so the card re-syncs to the global on the next toggle.
   */
  setVariant(slug: string, value: number): void {
    if (value === this.globalVariant) {
      if (slug in this.variantOverrides) {
        const next = { ...this.variantOverrides };
        delete next[slug];
        this.variantOverrides = next;
      }
    } else if (this.variantOverrides[slug] !== value) {
      this.variantOverrides = { ...this.variantOverrides, [slug]: value };
    }
    this.persistVariants();
  }

  /**
   * Set the global variant. Every override equal to the new global is absorbed
   * (deleted) so those cards flip with the global; overrides that still differ
   * are kept so those cards stay put. Symmetric in both directions.
   */
  setGlobalVariant(value: number): void {
    this.globalVariant = value;
    const next: Record<string, number> = {};
    for (const [slug, v] of Object.entries(this.variantOverrides)) {
      if (v !== value) next[slug] = v;
    }
    this.variantOverrides = next;
    this.persistVariants();
  }

  async init(): Promise<void> {
    if (this.manifest || this.manifestLoading) return;
    this.manifestLoading = true;
    this.manifestError = null;
    try {
      const manifest = await this.client.loadManifest();
      // `imageBaseUrl` stays absolute (cdn.mooshieblob.com). The browser-mode
      // proxy rewrite (`/internal-api/_cdn/...`) AND the LAN auth-token
      // injection happen at the point of use via `proxiedCdnUrl()` / `cachedSrc`.
      // Rewriting the base here would drop the token — a query param can't ride
      // on a base URL that later has `/images/...` concatenated onto it — so
      // every `<img src>` would be rejected with a 401 by the LAN auth gate.
      this.manifest = manifest;
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
      const hits = await this.client.search(text, {
        limit: 50,
        requireImage: !this.includeNoPreview,
      });
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
      const artist = await this.client.getArtist(slugOrTag);
      this.activeArtist = artist ? { ...artist, imageUrl: proxiedCdnUrl(artist.imageUrl) } : null;
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
