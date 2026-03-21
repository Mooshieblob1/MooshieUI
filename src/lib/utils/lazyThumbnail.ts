import type { OutputImage } from "../types/index.js";

export interface LazyThumbnailOpts {
  image: OutputImage;
  size?: number;
}

/**
 * Svelte action that lazily applies an image src when the element scrolls into view.
 * Uses thumbnailUrl (protocol-served WebP) for persisted images, or url for session images.
 * Usage: <img use:lazyThumbnail={{ image, size: 480 }} />
 */
export function lazyThumbnail(node: HTMLImageElement, opts: LazyThumbnailOpts) {
  let current = opts;

  function getSrc(): string | undefined {
    const img = current.image;
    if (img.url) return img.url;
    if (img.thumbnailUrl) {
      const size = current.size ?? 384;
      return `${img.thumbnailUrl}?size=${size}`;
    }
    return undefined;
  }

  function applySrc() {
    const src = getSrc();
    if (src && node.src !== src) {
      node.src = src;
    }
  }

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
      applySrc();
    },
    destroy() {
      observer.disconnect();
    },
  };
}
