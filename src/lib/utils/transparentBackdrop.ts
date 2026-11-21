import type { OutputImage } from "../types/index.js";

/**
 * Subtle dark-mode CSS checkerboard backdrop for transparent RGBA images
 * (Tailwind utilities only; no `<style>` blocks).
 */
export const CHECKERBOARD_CLASS =
  "bg-[#1a1a1f] bg-[conic-gradient(#272730_90deg,#1a1a1f_90deg_180deg,#272730_180deg_270deg,#1a1a1f_270deg)] bg-[length:16px_16px]";

const TRANSPARENT_PHRASE = "rgba, 4-channel, transparent background";

/** True when image metadata indicates a Ming-Image Layer output. */
export function isMingLayerImage(image: OutputImage | null | undefined): boolean {
  if (!image?.metadata) return false;
  return (
    image.metadata.mooshie_model_architecture === "mingimagelayer" ||
    Boolean(image.metadata.mooshie_layer_count)
  );
}

/** True when image metadata indicates a transparent RGBA output. */
export function isTransparentImage(image: OutputImage | null | undefined): boolean {
  if (!image?.metadata) return false;
  const m = image.metadata;
  if (m.mooshie_transparent_background === "true") return true;
  if (m.mooshie_novelai_transparent_background === "true") return true;
  if (isMingLayerImage(image)) return true;
  return (m.positive_prompt ?? "").toLowerCase().includes(TRANSPARENT_PHRASE);
}

/** Numeric layer order for sorting siblings within one Ming-Image Layer set. */
export function layerOrderValue(image: OutputImage): number {
  const fromMeta = Number(image.metadata?.mooshie_layer_index);
  if (Number.isFinite(fromMeta) && fromMeta > 0) return fromMeta;
  const match = image.filename.match(/_(\d+)\.[^.]+$/);
  if (match) return Number(match[1]) + 1;
  return 0;
}

/** Resolve 1-based layer position and total count for a Ming-Image Layer tile. */
export function mingLayerBadgeInfo(
  image: OutputImage,
  siblings?: OutputImage[],
): { index: number; total: number } | null {
  if (!isMingLayerImage(image)) return null;
  const metaIndex = Number(image.metadata?.mooshie_layer_index);
  const metaTotal = Number(image.metadata?.mooshie_layer_count);
  if (
    Number.isFinite(metaIndex) &&
    metaIndex > 0 &&
    Number.isFinite(metaTotal) &&
    metaTotal > 1
  ) {
    return { index: metaIndex, total: metaTotal };
  }
  if (!siblings || siblings.length <= 1) return null;
  const ordered = [...siblings].sort((a, b) => layerOrderValue(a) - layerOrderValue(b));
  const idx = ordered.indexOf(image);
  if (idx < 0) return null;
  return { index: idx + 1, total: ordered.length };
}

/**
 * Sample an already-loaded `HTMLImageElement` onto a tiny offscreen canvas
 * to detect transparent pixels. Safely returns false on CORS/tainted images.
 */
export function detectImageAlpha(img: HTMLImageElement): boolean {
  if (!img.complete || img.naturalWidth <= 0 || img.naturalHeight <= 0) return false;
  try {
    const size = 32;
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return false;
    ctx.drawImage(img, 0, 0, size, size);
    const data = ctx.getImageData(0, 0, size, size).data;
    for (let i = 3; i < data.length; i += 4) {
      if ((data[i] ?? 255) < 252) return true;
    }
    return false;
  } catch {
    return false;
  }
}
