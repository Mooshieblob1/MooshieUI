/**
 * Shared gallery image actions used by both desktop App.svelte handlers and
 * the mobile action-sheet UI. These helpers wrap the gallery / generation
 * stores so callers don't need to duplicate the bytes-load + upload dance.
 */
import type { OutputImage } from "../types/index.js";
import { locale } from "../stores/locale.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { generation } from "../stores/generation.svelte.js";
import { authHeaders, isTauri } from "./ipc.js";
import {
  loadGalleryImage,
  loadGalleryImagePng,
  getOutputImage,
  readTempImage,
  uploadImageBytes,
} from "./api.js";

function blobToBytes(blob: Blob): Promise<number[]> {
  return blob.arrayBuffer().then((buffer) => Array.from(new Uint8Array(buffer)));
}

function pngFilename(filename: string | undefined, fallback: string): string {
  const name = (filename && filename.trim()) || fallback;
  const pngName = name.replace(/\.(jpe?g|webp|jxl)$/i, ".png");
  return /\.[A-Za-z0-9]+$/.test(pngName) ? pngName : `${pngName}.png`;
}

function blobUrlToPngBytes(blobUrl: string): Promise<number[]> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const ctx = canvas.getContext("2d");
      if (!ctx) {
        reject(new Error("Canvas 2D context unavailable"));
        return;
      }
      ctx.drawImage(img, 0, 0);
      canvas.toBlob((blob) => {
        if (!blob) {
          reject(new Error("Canvas toBlob failed"));
          return;
        }
        blobToBytes(blob).then(resolve, reject);
      }, "image/png");
    };
    img.onerror = () => reject(new Error("Failed to load image URL"));
    img.src = blobUrl;
  });
}

async function blobToPngBytes(blob: Blob): Promise<number[]> {
  if (blob.type === "image/png") return blobToBytes(blob);
  const url = URL.createObjectURL(blob);
  try {
    return await blobUrlToPngBytes(url);
  } finally {
    URL.revokeObjectURL(url);
  }
}

export async function imageUrlToPngBytes(url: string): Promise<number[]> {
  if (url.startsWith("blob:")) return blobUrlToPngBytes(url);

  const response = await fetch(url, { headers: authHeaders() });
  if (!response.ok) throw new Error(`Failed to fetch image: ${response.status}`);
  return blobToPngBytes(await response.blob());
}

async function tempImageToPngBytes(tempFilename: string, outputFilename: string): Promise<number[]> {
  const lowerTempFilename = tempFilename.toLowerCase();
  const sourceIsJxl = lowerTempFilename.endsWith(".jxl");
  const outputIsJxl = outputFilename.toLowerCase().endsWith(".jxl");
  if (isTauri) {
    if (sourceIsJxl) throw new Error(locale.t("gallery.error.jxl_copy_unavailable"));
    const bytes = await readTempImage(tempFilename);
    const type = lowerTempFilename.endsWith(".webp")
      ? "image/webp"
      : lowerTempFilename.endsWith(".jpg") || lowerTempFilename.endsWith(".jpeg")
        ? "image/jpeg"
        : "image/png";
    return blobToPngBytes(new Blob([new Uint8Array(bytes)], { type }));
  }

  const suffix = sourceIsJxl || outputIsJxl ? "?format=webp" : "";
  const response = await fetch(`/internal-api/_temp_image/${encodeURIComponent(tempFilename)}${suffix}`, {
    headers: authHeaders(),
  });
  if (!response.ok) throw new Error(`Temp image fetch failed: ${response.status}`);
  return blobToPngBytes(await response.blob());
}

export async function loadOutputImageForGenerationInput(
  image: OutputImage,
  fallbackFilename = "input.png",
): Promise<{ bytes: number[]; filename: string }> {
  const uploadFilename = pngFilename(image.filename, fallbackFilename);

  if (image.gallery_filename) {
    // JXL and WebP are transcoded to PNG server-side; PNG is the one format every
    // stage of the ComfyUI input pipeline is guaranteed to accept.
    const lower = image.gallery_filename.toLowerCase();
    const needsTranscode = lower.endsWith(".jxl") || lower.endsWith(".webp");
    const bytes = needsTranscode
      ? await loadGalleryImagePng(image.gallery_filename)
      : await loadGalleryImage(image.gallery_filename);
    return {
      bytes,
      filename: needsTranscode ? uploadFilename : image.filename || fallbackFilename,
    };
  }

  if (image.sessionBlob && image.sessionBlob.type !== "image/jxl") {
    return { bytes: await blobToPngBytes(image.sessionBlob), filename: uploadFilename };
  }

  if (image.displayTempFilename) {
    try {
      return {
        bytes: await tempImageToPngBytes(image.displayTempFilename, image.filename),
        filename: uploadFilename,
      };
    } catch (tempError) {
      if (!image.url) throw tempError;
      console.warn("Display temp image fetch failed; falling back to preview URL:", tempError);
    }
  }

  if (image.tempFilename && !image.tempFilename.toLowerCase().endsWith(".jxl")) {
    try {
      return {
        bytes: await tempImageToPngBytes(image.tempFilename, image.filename),
        filename: uploadFilename,
      };
    } catch (tempError) {
      if (!image.url) throw tempError;
      console.warn("Temp image fetch failed; falling back to preview URL:", tempError);
    }
  }

  const fallbackUrl = image.url || (gallery.selectedImage === image && gallery.lightboxUrl ? gallery.lightboxUrl : undefined);
  if (fallbackUrl) {
    try {
      return { bytes: await imageUrlToPngBytes(fallbackUrl), filename: uploadFilename };
    } catch (urlError) {
      if (!image.tempFilename) throw urlError;
      console.warn("Preview URL load failed; falling back to temp image:", urlError);
    }
  }

  if (image.tempFilename) {
    return {
      bytes: await tempImageToPngBytes(image.tempFilename, image.filename),
      filename: uploadFilename,
    };
  }

  const bytes = await getOutputImage(image.filename, image.subfolder);
  return { bytes, filename: image.filename || fallbackFilename };
}

export async function uploadOutputImageForGenerationInput(
  image: OutputImage,
  fallbackFilename = "input.png",
): Promise<string> {
  const { bytes, filename } = await loadOutputImageForGenerationInput(image, fallbackFilename);
  const response = await uploadImageBytes(bytes, filename);
  return response.name;
}

export async function uploadImageUrlForGenerationInput(
  url: string,
  fallbackFilename = "input.png",
): Promise<string> {
  const bytes = await imageUrlToPngBytes(url);
  const response = await uploadImageBytes(bytes, pngFilename(fallbackFilename, "input.png"));
  return response.name;
}

/**
 * Upload a gallery/session image as a ComfyUI input, reporting its own "W:H"
 * and an object URL for the slot thumbnail.
 *
 * The aspect string mirrors what a dragged file produces in the video panel, so
 * callers can set `videoAspectRatio = "auto"` and have the canvas match the
 * image the user picked. A decode failure is not fatal: the upload still
 * happens and the aspect comes back null.
 *
 * `previewUrl` belongs to the caller. The video panel hands it to `setPreview`,
 * which owns revoking it; callers that show no preview must revoke it directly.
 */
export async function uploadForVideo(
  image: OutputImage,
  fallback: string,
): Promise<{ name: string; aspect: string | null; previewUrl: string }> {
  const { bytes, filename } = await loadOutputImageForGenerationInput(image, fallback);
  const blob = new Blob([new Uint8Array(bytes)], { type: mimeForFilename(filename) });
  let aspect: string | null = null;
  try {
    const bitmap = await createImageBitmap(blob);
    aspect = `${bitmap.width}:${bitmap.height}`;
    bitmap.close();
  } catch (e) {
    console.warn("Could not read frame dimensions; leaving the aspect ratio alone:", e);
  }
  const response = await uploadImageBytes(bytes, filename);
  return { name: response.name, aspect, previewUrl: URL.createObjectURL(blob) };
}

/** Best-effort image type for a preview blob, so the thumbnail renders. */
function mimeForFilename(filename: string): string {
  const ext = filename.toLowerCase().split(".").pop() ?? "";
  if (ext === "jpg" || ext === "jpeg") return "image/jpeg";
  if (ext === "webp") return "image/webp";
  return "image/png";
}

/** Load `image` as the first frame and switch to video mode. */
export async function sendImageToVideoFrame(image: OutputImage): Promise<void> {
  const { name, aspect, previewUrl } = await uploadForVideo(image, "video_first_frame.png");
  // The panel re-reads the slot from the store, so this caller shows no preview.
  URL.revokeObjectURL(previewUrl);
  generation.videoVariant = "fl2va";
  generation.videoFirstFrame = name;
  generation.videoFirstFrameAspect = aspect;
  if (aspect) generation.videoAspectRatio = "auto";
  generation.mode = "video";
  generation.saveSettings();
}

/** Reference slots still free in `videoRefImages`. */
export function videoReferenceSlotsFree(): number {
  return generation.videoRefImages.filter((slot) => !slot).length;
}

/** Append `image` to the reference list. Returns the 1-based slot it landed in. */
export async function addImageToVideoReference(image: OutputImage): Promise<number> {
  const index = generation.videoRefImages.findIndex((slot) => !slot);
  if (index === -1) throw new Error("video-refs-full");
  const { name, previewUrl } = await uploadForVideo(image, "video_reference.png");
  URL.revokeObjectURL(previewUrl);
  generation.videoVariant = "ref2va";
  generation.videoRefImages = generation.videoRefImages.map((current, i) =>
    i === index ? name : current,
  );
  generation.mode = "video";
  generation.saveSettings();
  return index + 1;
}
