import type { OutputImage } from "../types/index.js";

export interface LazyThumbnailOpts {
  image: OutputImage;
  size?: number;
}

const MAX_RETRIES = 3;
const RETRY_DELAY_MS = 1000;

/**
 * Svelte action that lazily applies an image src when the element scrolls into view.
 * Uses thumbnailUrl (protocol-served WebP) for persisted images, or url for session images.
 * Retries on error (e.g. protocol handler not ready during startup).
 * Usage: <img use:lazyThumbnail={{ image, size: 480 }} />
 */
export function lazyThumbnail(node: HTMLImageElement, opts: LazyThumbnailOpts) {
  let current = opts;
  let retryCount = 0;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;

  function getSrc(): string | undefined {
    const img = current.image;
    // Prefer thumbnail protocol (smaller WebP) over full-res blob URL
    if (img.thumbnailUrl) {
      const size = current.size ?? 384;
      return `${img.thumbnailUrl}?size=${size}`;
    }
    if (img.url) return img.url;
    return undefined;
  }

  function applySrc() {
    const src = getSrc();
    if (src && node.src !== src) {
      node.src = src;
    }
  }

  function onError() {
    if (retryCount < MAX_RETRIES) {
      retryCount++;
      retryTimer = setTimeout(() => {
        // Force reload by busting any cache with a retry param
        const src = getSrc();
        if (src) {
          const sep = src.includes("?") ? "&" : "?";
          node.src = `${src}${sep}_retry=${retryCount}`;
        }
      }, RETRY_DELAY_MS * retryCount);
    }
  }

  node.addEventListener("error", onError);

  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          applySrc();
          observer.unobserve(node);
        }
      }
    },
    { rootMargin: "200px" },
  );

  // Session images already have url — apply immediately if visible
  applySrc();
  observer.observe(node);

  return {
    update(newOpts: LazyThumbnailOpts) {
      current = newOpts;
      retryCount = 0;
      applySrc();
    },
    destroy() {
      observer.disconnect();
      node.removeEventListener("error", onError);
      if (retryTimer) clearTimeout(retryTimer);
    },
  };
}
