/**
 * Persistent image cache for artist gallery preview images.
 *
 * Uses the Cache API (available in both Tauri's webview and standard browsers)
 * to store artist images permanently until explicitly cleared.  This means:
 *
 *  - Desktop (Tauri): images survive app restarts, never re-downloaded.
 *  - Web hosting: each browser caches images locally after the first view,
 *    so subsequent page loads serve zero R2 Class-B operations for those images.
 *
 * The CDN layer (Cloudflare in front of R2) handles cross-visitor caching once
 * objects are served with `Cache-Control: public, max-age=31536000, immutable`
 * (see scripts/r2_upload_anima.py – the upload script sets those headers).
 *
 * Usage:
 *   <img use:cachedSrc={entry.imageUrl} alt={entry.tag} />
 */

/** Cache storage bucket name.  Bump the version suffix to bust stale caches. */
const CACHE_NAME = "anima-artist-images-v1";

/** In-memory map from CDN URL → blob-URL (avoids re-creating blob URLs for
 *  images already fetched this session but before the component unmounts). */
const _blobUrlMap = new Map<string, string>();

/** Promise map prevents concurrent fetches for the same URL. */
const _inflight = new Map<string, Promise<string>>();

/** Whether the Cache API is available in this environment. */
function hasCacheApi(): boolean {
  return typeof caches !== "undefined";
}

/**
 * Resolve a CDN image URL to a local blob URL, fetching and caching on first
 * access.  Returns the original URL as a fallback if caching fails.
 */
export async function fetchCachedArtistImage(url: string): Promise<string> {
  if (!url) return url;

  // Return existing in-memory blob URL from this session.
  const existing = _blobUrlMap.get(url);
  if (existing) return existing;

  // Deduplicate concurrent requests for the same URL.
  const inFlight = _inflight.get(url);
  if (inFlight) return inFlight;

  const promise = _doFetch(url).catch(() => url); // never reject — fall back to direct URL
  _inflight.set(url, promise);
  promise.finally(() => _inflight.delete(url));
  return promise;
}

async function _doFetch(url: string): Promise<string> {
  if (hasCacheApi()) {
    try {
      const cache = await caches.open(CACHE_NAME);
      const cached = await cache.match(url);
      if (cached) {
        const blob = await cached.blob();
        const blobUrl = URL.createObjectURL(blob);
        _blobUrlMap.set(url, blobUrl);
        return blobUrl;
      }
    } catch {
      // Cache read failed — fall through to network fetch.
    }
  }

  const response = await fetch(url, { mode: "cors", credentials: "omit" });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);

  if (hasCacheApi()) {
    try {
      const cache = await caches.open(CACHE_NAME);
      await cache.put(url, response.clone());
    } catch {
      // Cache write failed — non-fatal.
    }
  }

  const blob = await response.blob();
  const blobUrl = URL.createObjectURL(blob);
  _blobUrlMap.set(url, blobUrl);
  return blobUrl;
}

/**
 * Delete all cached artist images.  Also revokes in-memory blob URLs so any
 * currently-mounted `<img>` elements will gracefully fall back to the CDN
 * URL on their next render cycle.
 */
export async function clearArtistImageCache(): Promise<void> {
  // Revoke all in-memory blob URLs.
  for (const blobUrl of _blobUrlMap.values()) {
    try { URL.revokeObjectURL(blobUrl); } catch { /* ignore */ }
  }
  _blobUrlMap.clear();

  if (hasCacheApi()) {
    await caches.delete(CACHE_NAME);
  }
}

/**
 * Return the approximate number of images currently stored in the cache, or
 * -1 if the Cache API is unavailable.
 */
export async function getArtistImageCacheCount(): Promise<number> {
  if (!hasCacheApi()) return -1;
  try {
    const cache = await caches.open(CACHE_NAME);
    const keys = await cache.keys();
    return keys.length;
  } catch {
    return -1;
  }
}

// ---------------------------------------------------------------------------
// Svelte action
// ---------------------------------------------------------------------------

/**
 * Svelte `use:` action that replaces an `<img>` element's `src` with a
 * locally-cached blob URL, falling back to the original CDN URL on error.
 *
 * @example
 *   <img use:cachedSrc={entry.imageUrl} alt={entry.tag} />
 */
export function cachedSrc(node: HTMLImageElement, url: string) {
  let active = true;

  async function apply(src: string) {
    if (!src) return;
    const resolved = await fetchCachedArtistImage(src);
    if (active) node.src = resolved;
  }

  void apply(url);

  return {
    update(newUrl: string) {
      void apply(newUrl);
    },
    destroy() {
      active = false;
      // Blob URLs are intentionally kept in _blobUrlMap for reuse by other
      // instances.  They are released on clearArtistImageCache().
    },
  };
}
