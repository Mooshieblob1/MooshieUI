/**
 * Card thumbnails for saved characters.
 *
 * Stored inline as a small WebP data URL rather than as a gallery reference,
 * so an uploaded file needs no storage of its own, a thumbnail survives the
 * gallery image being deleted, and it syncs with the rest of the saved
 * character through user prefs. The size cap keeps each one to a few KB.
 */
import type { OutputImage } from "../types/index.js";
import { loadOutputImageForGenerationInput } from "./galleryActions.js";

/** Longest side of a stored thumbnail, in pixels. */
export const CHARACTER_THUMBNAIL_SIZE = 192;

/** Downscale any decodable image to a thumbnail data URL, or null if it cannot be read. */
export async function blobToCharacterThumbnail(blob: Blob): Promise<string | null> {
  if (blob.type && !blob.type.startsWith("image/")) return null;
  try {
    const bitmap = await createImageBitmap(blob);
    const longest = Math.max(bitmap.width, bitmap.height);
    if (!longest) return null;
    const scale = Math.min(1, CHARACTER_THUMBNAIL_SIZE / longest);
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    bitmap.close();
    return canvas.toDataURL("image/webp", 0.82);
  } catch {
    return null;
  }
}

/** Thumbnail from a gallery or session image (JXL on disk, so decoded via PNG). */
export async function outputImageToCharacterThumbnail(image: OutputImage): Promise<string | null> {
  const { bytes } = await loadOutputImageForGenerationInput(image);
  return blobToCharacterThumbnail(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
}

/**
 * Thumbnail from what the preview is showing: the last finished image's URL,
 * falling back to the newest still from this session when that URL cannot be
 * read (a revoked blob, or a format the browser cannot decode).
 */
export async function currentImageToCharacterThumbnail(
  displayUrl: string | null,
  latestSessionImage: OutputImage | null,
): Promise<string | null> {
  if (displayUrl) {
    try {
      const response = await fetch(displayUrl);
      if (response.ok) {
        const thumb = await blobToCharacterThumbnail(await response.blob());
        if (thumb) return thumb;
      }
    } catch {
      // Fall through to the session image.
    }
  }
  return latestSessionImage ? outputImageToCharacterThumbnail(latestSessionImage) : null;
}
