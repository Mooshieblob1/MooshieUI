/** Types for the artist gallery portable module. Mirrors shapes written by scripts/r2_build_indices.py. */

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
  /** Danbooru post count (popularity; ranking signal). */
  postCount: number;
  /** Known aliases for the artist tag. */
  aliases: string[];
  /** Whether the webp was present on disk when the index was built. */
  hasImage: boolean;
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
  generatedAt: string;
}

/** Row in the flat search.json index. */
export interface ArtistSearchHit {
  slug: string;
  tag: string;
  /** Matches ArtistEntry.imageId; combine with manifest.imageBaseUrl to render thumbnails without a shard fetch. */
  imageId: string;
  postCount: number;
  shard: string;
  hasImage: boolean;
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
  loadSearchIndex(): Promise<ArtistSearchHit[]>;
  /** Prefix + contains + alias ranking mirrors src/lib/stores/autocomplete.svelte.ts. */
  search(query: string, opts?: SearchOptions): Promise<ArtistSearchHit[]>;
  /** Clear all in-memory caches (next call will re-fetch). */
  invalidate(): void;
}
