import type {
  ArtistEntry,
  ArtistGalleryClient,
  ArtistManifest,
  ArtistSearchHit,
  ArtistShard,
  SearchOptions,
} from "./types.js";

const MANIFEST_CACHE_MS = 60_000;

interface ClientOptions {
  manifestUrl: string;
  /** Optional fetch override; useful for tests or when running in Node. */
  fetchImpl?: typeof fetch;
}

export function createArtistGalleryClient(opts: ClientOptions): ArtistGalleryClient {
  const manifestUrl = opts.manifestUrl;
  const fetchFn = opts.fetchImpl ?? globalThis.fetch.bind(globalThis);

  let manifestPromise: Promise<ArtistManifest> | null = null;
  let manifestAt = 0;
  const shardPromises = new Map<string, Promise<ArtistShard>>();
  let searchPromise: Promise<ArtistSearchHit[]> | null = null;
  /** slug → shard bucket (populated from search index once loaded). */
  const slugToBucket = new Map<string, string>();
  /** raw tag (lowercased) → slug. */
  const tagToSlug = new Map<string, string>();

  function baseDir(): string {
    // manifest.json lives in the same directory as shards/ and search.json
    const slash = manifestUrl.lastIndexOf("/");
    return slash === -1 ? "" : manifestUrl.slice(0, slash + 1);
  }

  async function fetchJson<T>(url: string): Promise<T> {
    const res = await fetchFn(url, { credentials: "omit" });
    if (!res.ok) {
      throw new Error(`artist-gallery: ${url} returned ${res.status}`);
    }
    const text = await res.text();
    try {
      return JSON.parse(text) as T;
    } catch (e) {
      const preview = text.slice(0, 80).replace(/\s+/g, " ");
      throw new Error(
        `artist-gallery: ${url} returned non-JSON (${res.headers.get("content-type") ?? "no content-type"}): "${preview}…"`,
      );
    }
  }

  async function loadManifest(): Promise<ArtistManifest> {
    const now = Date.now();
    if (manifestPromise && now - manifestAt < MANIFEST_CACHE_MS) {
      return manifestPromise;
    }
    manifestAt = now;
    manifestPromise = fetchJson<ArtistManifest>(manifestUrl).catch((err) => {
      // Let the next call retry on failure.
      manifestPromise = null;
      manifestAt = 0;
      throw err;
    });
    return manifestPromise;
  }

  async function loadShard(bucket: string): Promise<ArtistShard> {
    const existing = shardPromises.get(bucket);
    if (existing) return existing;
    const manifest = await loadManifest();
    const meta = manifest.shards.find((s) => s.bucket === bucket);
    if (!meta) {
      throw new Error(`artist-gallery: no shard "${bucket}" in manifest`);
    }
    const p = fetchJson<ArtistShard>(baseDir() + meta.path).catch((err) => {
      shardPromises.delete(bucket);
      throw err;
    });
    shardPromises.set(bucket, p);
    return p;
  }

  function bucketForSlug(slug: string): string {
    if (!slug) return "_";
    const ch = slug[0].toLowerCase();
    return /[a-z0-9]/.test(ch) ? ch : "_";
  }

  function normalizeTag(tag: string): string {
    // Strip one leading @ for lookup (anima-tags uses "@name"; callers may pass either form).
    return tag.replace(/^@+/, "").toLowerCase();
  }

  async function loadSearchIndex(): Promise<ArtistSearchHit[]> {
    if (searchPromise) return searchPromise;
    const manifest = await loadManifest();
    searchPromise = fetchJson<ArtistSearchHit[]>(
      baseDir() + manifest.searchIndex.path,
    ).then((hits) => {
      for (const h of hits) {
        slugToBucket.set(h.slug, h.shard);
        tagToSlug.set(h.tag.toLowerCase(), h.slug);
        tagToSlug.set(normalizeTag(h.tag), h.slug);
      }
      return hits;
    }).catch((err) => {
      searchPromise = null;
      throw err;
    });
    return searchPromise;
  }

  async function getArtist(slugOrTag: string): Promise<ArtistEntry | null> {
    if (!slugOrTag) return null;
    const trimmed = slugOrTag.trim();
    let slug = trimmed;
    let bucket = bucketForSlug(slug);

    // If caller handed a raw tag ("@dairi" / "dairi"), try direct bucket first;
    // fall back to the search index for indirect resolution.
    const shard = await loadShard(bucket).catch(() => null);
    if (shard?.entries[slug]) return shard.entries[slug];

    // Resolve through the search index.
    await loadSearchIndex();
    const key = normalizeTag(trimmed);
    const resolvedSlug = tagToSlug.get(key) ?? tagToSlug.get(trimmed.toLowerCase());
    if (!resolvedSlug) return null;
    slug = resolvedSlug;
    bucket = slugToBucket.get(slug) ?? bucketForSlug(slug);
    const shard2 = await loadShard(bucket);
    return shard2.entries[slug] ?? null;
  }

  function normalizeQuery(text: string): string {
    return text.toLowerCase().trim().replace(/\s+/g, "_").replace(/\\/g, "").replace(/^@+/, "");
  }

  async function search(query: string, opts: SearchOptions = {}): Promise<ArtistSearchHit[]> {
    const q = normalizeQuery(query);
    if (!q) return [];
    const hits = await loadSearchIndex();
    const limit = Math.max(1, Math.min(100, opts.limit ?? 25));
    const requireImage = opts.requireImage ?? true;

    const prefix: ArtistSearchHit[] = [];
    const contains: ArtistSearchHit[] = [];
    for (const h of hits) {
      if (requireImage && !h.hasImage) continue;
      const slugLower = h.slug.toLowerCase();
      if (slugLower.startsWith(q)) {
        prefix.push(h);
        if (prefix.length >= limit) break;
      } else if (slugLower.includes(q) || h.tag.toLowerCase().includes(q)) {
        if (contains.length < limit) contains.push(h);
      }
    }
    // hits is already sorted by postCount desc, so slice preserves ranking.
    const combined = [...prefix, ...contains];
    return combined.slice(0, limit);
  }

  function invalidate(): void {
    manifestPromise = null;
    manifestAt = 0;
    shardPromises.clear();
    searchPromise = null;
    slugToBucket.clear();
    tagToSlug.clear();
  }

  return {
    manifestUrl,
    loadManifest,
    loadShard,
    getArtist,
    loadSearchIndex,
    search,
    invalidate,
  };
}
