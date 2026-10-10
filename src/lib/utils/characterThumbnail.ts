/**
 * Card thumbnails for saved characters.
 *
 * Stored inline as small WebP data URLs rather than as a gallery reference,
 * so an uploaded file needs no storage of its own, a thumbnail survives the
 * gallery image being deleted, and it syncs with the rest of the saved
 * character through user prefs. The thumbnail is a few KB; the reference copy
 * NovelAI reads is larger, tens of KB.
 */
import type { OutputImage } from "../types/index.js";
import { loadOutputImageForGenerationInput } from "./galleryActions.js";
import { fileToNovelAiBase64 } from "./novelaiImage.js";

/** Longest side of a stored thumbnail, in pixels. */
export const CHARACTER_THUMBNAIL_SIZE = 192;
/**
 * Longest side of the reference copy, in pixels. Enough for NovelAI to read a
 * likeness from, while keeping each card to tens of KB in local storage.
 */
export const CHARACTER_REFERENCE_SIZE = 640;

/** A card's picture: the thumbnail shown, and the larger copy for references. */
export interface CharacterImages {
  thumbnail: string;
  reference: string;
}

function scaled(bitmap: ImageBitmap, size: number, quality: number): string | null {
  const longest = Math.max(bitmap.width, bitmap.height);
  if (!longest) return null;
  const scale = Math.min(1, size / longest);
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/webp", quality);
}

/** Downscale any decodable image to a card's picture, or null if it cannot be read. */
export async function blobToCharacterThumbnail(blob: Blob): Promise<CharacterImages | null> {
  if (blob.type && !blob.type.startsWith("image/")) return null;
  try {
    const bitmap = await createImageBitmap(blob);
    const thumbnail = scaled(bitmap, CHARACTER_THUMBNAIL_SIZE, 0.82);
    const reference = scaled(bitmap, CHARACTER_REFERENCE_SIZE, 0.85);
    bitmap.close();
    return thumbnail && reference ? { thumbnail, reference } : null;
  } catch {
    return null;
  }
}

/**
 * A card's picture as the bare base64 PNG NovelAI's Precise Reference takes,
 * from the reference copy, or the thumbnail on a card made before there was
 * one. Null when the card has no picture or it cannot be decoded.
 */
export async function characterReferenceBase64(card: {
  thumbnail: string | null;
  reference: string | null;
}): Promise<string | null> {
  const source = card.reference ?? card.thumbnail;
  if (!source) return null;
  try {
    const blob = await (await fetch(source)).blob();
    return await fileToNovelAiBase64(blob);
  } catch {
    return null;
  }
}

/** Thumbnail from a gallery or session image (JXL on disk, so decoded via PNG). */
export async function outputImageToCharacterThumbnail(image: OutputImage): Promise<CharacterImages | null> {
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
): Promise<CharacterImages | null> {
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
