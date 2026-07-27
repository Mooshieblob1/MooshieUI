/**
 * Persistent image cache for artist gallery preview images.
 *
 * Uses the Cache API (available in both Tauri's webview and standard browsers)
 * to store artist images permanently until explicitly cleared, so each browser
 * caches images locally after the first view and subsequent page loads serve
 * zero R2 Class-B operations for those images.
 *
 * The CDN layer (Cloudflare in front of R2) handles cross-visitor caching once
 * objects are served with `Cache-Control: public, max-age=31536000, immutable`
 * (see scripts/r2_upload_anima.py – the upload script sets those headers).
 *
 * Two paths exist because of what the environment can actually do:
 *
 *  - Desktop (Tauri), AVIF-capable webview: the CDN sends no CORS headers, so
 *    `fetch` is blocked and this layer steps aside — `<img src>` loads the CDN
 *    URL directly and the webview's own HTTP cache honours the immutable
 *    headers above.
 *  - Webview without AVIF support (WebKitGTK on Linux): the bytes are pulled
 *    through the CDN proxy, decoded in WebAssembly, re-encoded to WebP/PNG,
 *    and the *transcoded* result is what gets cached — so the decode cost is
 *    paid once per image rather than once per view.
 *
 * Usage:
 *   <img use:cachedSrc={entry.imageUrl} alt={entry.tag} />
 */

import { cdnFetchBytes, isCdnUrl, proxiedCdnUrl } from "../utils/cdnFetch.js";
import { isTauri } from "../utils/ipc.js";

/** Cache storage bucket name.  Bump the version suffix to bust stale caches. */
const CACHE_NAME = "anima-artist-images-v1";

/**
 * 1x1 red AVIF (314 bytes, produced by libavif) used to probe whether the
 * webview can decode AVIF at all.  WebKitGTK on Linux frequently ships without
 * AVIF support, which is what breaks the gallery there — the dataset has no
 * WebP/JPEG fallbacks on the CDN.
 */
const AVIF_PROBE =
  "data:image/avif;base64,AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUIAAADybWV0YQAAAAAAAAAoaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAGxpYmF2aWYAAAAADnBpdG0AAAAAAAEAAAAeaWxvYwAAAABEAAABAAEAAAABAAABGgAAACAAAAAoaWluZgAAAAAAAQAAABppbmZlAgAAAAABAABhdjAxQ29sb3IAAAAAamlwcnAAAABLaXBjbwAAABRpc3BlAAAAAAAAAAEAAAABAAAAEHBpeGkAAAAAAwgICAAAAAxhdjFDgQAMAAAAABNjb2xybmNseAACAAIABoAAAAAXaXBtYQAAAAAAAAABAAEEAQKDBAAAAChtZGF0EgAKCBgABggQEDQgMhIYAAooooQARmzv9mB1+uPnYcA=";

let _nativeAvif: Promise<boolean> | null = null;

/** Whether this webview decodes AVIF natively.  Probed once per session. */
export function supportsNativeAvif(): Promise<boolean> {
  if (!_nativeAvif) {
    _nativeAvif = new Promise<boolean>((resolve) => {
      if (typeof Image === "undefined") {
        resolve(true); // no DOM — nothing to render anyway
        return;
      }
      const img = new Image();
      // A webview that "loads" an undecodable image still reports 0x0.
      img.onload = () => resolve(img.naturalWidth === 1 && img.naturalHeight === 1);
      img.onerror = () => resolve(false);
      img.src = AVIF_PROBE;
    });
  }
  return _nativeAvif;
}

function isAvifUrl(url: string): boolean {
  return /\.avif(?:$|[?#])/i.test(url);
}

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

/** Open the cache bucket, or null if the environment doesn't allow it. */
async function openCache(): Promise<Cache | null> {
  if (!hasCacheApi()) return null;
  try {
    return await caches.open(CACHE_NAME);
  } catch {
    return null;
  }
}

async function _doFetch(url: string): Promise<string> {
  const requestUrl = proxiedCdnUrl(url);
  const needsTranscode = isAvifUrl(url) && !(await supportsNativeAvif());

  // Tauri's webview can't `fetch` the CDN at all — it serves no
  // `Access-Control-Allow-Origin` header for any origin the app runs under.
  // Unless we're taking the transcode path (which pulls bytes through the Rust
  // proxy instead) there is nothing this layer can cache, so bail out and let
  // the caller fall back to a plain `<img src>`, which is exempt from CORS and
  // hits the webview's own HTTP cache.
  if (!needsTranscode && isTauri && isCdnUrl(requestUrl)) {
    throw new Error("CDN fetch is CORS-blocked in the desktop webview");
  }

  const cache = await openCache();

  // Cached entries are always in a directly-renderable format (the original
  // bytes, or the WebP/PNG transcode of an AVIF), so a hit skips everything.
  if (cache) {
    try {
      const cached = await cache.match(requestUrl);
      if (cached) return _remember(url, await cached.blob());
    } catch {
      // Cache read failed — fall through to network fetch.
    }
  }

  const blob = await _load(url, requestUrl, needsTranscode);

  if (cache) {
    try {
      await cache.put(requestUrl, new Response(blob));
    } catch {
      // Cache write failed — non-fatal.
    }
  }

  return _remember(url, blob);
}

/** Load the image bytes, transcoding AVIF where the webview can't decode it. */
async function _load(
  url: string,
  requestUrl: string,
  needsTranscode: boolean,
): Promise<Blob> {
  if (needsTranscode) {
    // `cdnFetchBytes` picks its own transport: the Rust proxy on desktop (where
    // `fetch` is CORS-blocked), the same-origin proxy in browser/LAN mode.
    const bytes = await cdnFetchBytes(url);
    const { decodeAvifToBlob } = await import("./avifDecode.js");
    return decodeAvifToBlob(bytes);
  }

  const response = await fetch(requestUrl, { credentials: "omit" });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.blob();
}

function _remember(url: string, blob: Blob): string {
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
