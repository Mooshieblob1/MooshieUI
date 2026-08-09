/** Types for the artist gallery portable module. Mirrors shapes written by scripts/r2_build_indices.py. */

/** Per-variant image entry added in index v2. */
export interface ArtistImage {
  /** "p1" | "p2" — primary and secondary variants. */
  variantId: string;
  imageId: string;
  imageUrl: string;
  objectKey: string;
  /** Whether this specific variant was present when the index was built. */
  hasImage: boolean;
}

export interface ArtistEntry {
  /** Raw artist tag as it appears in anima-tags.json (e.g. "@dairi"). */
  tag: string;
  /** Filesystem-safe slug; matches the leading portion of imageId. */
  slug: string;
  /** Stable image identifier (slug + short sha1). */
  imageId: string;
  /** Fully-qualified HTTPS URL to the preview image. Empty string if host not configured. */
  imageUrl: string;
  /** R2 object key. Useful for direct-to-bucket fetches if a CDN isn't in front. */
  objectKey: string;
  /** Gelbooru post count, or 50 when the tag is below the reliable threshold. */
  postCount: number;
  /** True when the count should render as <=50 instead of an exact post total. */
  belowThreshold?: boolean;
  /** Compact below-threshold marker used by anima-tags.json; accepted for compatibility. */
  b?: number | boolean;
  /** Known aliases for the artist tag. */
  aliases: string[];
  /** Whether the webp was present on disk when the index was built. */
  hasImage: boolean;
  /** Per-variant images; present in index v2+. Absent in v1 — always fall back to imageUrl. */
  images?: ArtistImage[];
}

export interface ArtistShard {
  bucket: string;
  /** Map of slug → ArtistEntry. */
  entries: Record<string, ArtistEntry>;
}

export interface ArtistManifestShardMeta {
  bucket: string;
  count: number;
  path: string;
}

export interface ArtistManifest {
  version: number;
  releasePrefix: string;
  imageBaseUrl: string;
  shardScheme: string;
  artistCount: number;
  artistsWithImage: number;
  shards: ArtistManifestShardMeta[];
  searchIndex: { path: string; entries: number };
  /** Present only on index releases that ship the no-preview fallback list. */
  noPreviewIndex?: { path: string; entries: number };
  generatedAt: string;
}

/** Row in the flat search.json index. */
export interface ArtistSearchHit {
  slug: string;
  tag: string;
  /** Matches ArtistEntry.imageId; combine with manifest.imageBaseUrl to render thumbnails without a shard fetch. */
  imageId: string;
  postCount: number;
  belowThreshold?: boolean;
  b?: number | boolean;
  shard: string;
  hasImage: boolean;
  /** Number of image variants available; present in index v2+. */
  variantCount?: number;
  /**
   * Per-variant images; present in index v2+. Lets grid cards resolve a
   * non-primary variant's thumbnail without a shard fetch. Absent in v1 —
   * always fall back to `imageId`.
   */
  images?: ArtistImage[];
}

/**
 * A raw row of `no-preview.json`: an artist the model knows that the image
 * set never covered. The client synthesises an `ArtistSearchHit` from this
 * with `hasImage: false`.
 */
export interface NoPreviewEntry {
  tag: string;
  slug: string;
  postCount: number;
  belowThreshold?: boolean;
  aliases?: string[];
}

export interface SearchOptions {
  limit?: number;
  /** Only return entries where `hasImage === true`. Default: true. */
  requireImage?: boolean;
}

export interface ArtistGalleryClient {
  manifestUrl: string;
  loadManifest(): Promise<ArtistManifest>;
  loadShard(bucket: string): Promise<ArtistShard>;
  /** Resolve an artist entry by slug or raw tag ("@dairi"). Returns null if unknown. */
  getArtist(slugOrTag: string): Promise<ArtistEntry | null>;
  /** Resolve only by direct shard lookup. Does not load the large search index fallback. */
  getArtistDirect(slugOrTag: string): Promise<ArtistEntry | null>;
  loadSearchIndex(): Promise<ArtistSearchHit[]>;
  /**
   * Artists with no CDN image, as synthesised hits (`hasImage: false`).
   * Resolves to `[]` when the release predates the no-preview index.
   */
  loadNoPreviewHits(): Promise<ArtistSearchHit[]>;
  /** Prefix + contains + alias ranking mirrors src/lib/stores/autocomplete.svelte.ts. */
  search(query: string, opts?: SearchOptions): Promise<ArtistSearchHit[]>;
  /** Clear all in-memory caches (next call will re-fetch). */
  invalidate(): void;
}
