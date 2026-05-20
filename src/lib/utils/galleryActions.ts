/**
 * Shared gallery image actions used by both desktop App.svelte handlers and
 * the mobile action-sheet UI. These helpers wrap the gallery / generation
 * stores so callers don't need to duplicate the bytes-load + upload dance.
 */
import type { OutputImage } from "../types/index.js";
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
    if (sourceIsJxl) throw new Error("JXL display copy is unavailable");
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
    const isJxl = image.gallery_filename.toLowerCase().endsWith(".jxl");
    const bytes = isJxl
      ? await loadGalleryImagePng(image.gallery_filename)
      : await loadGalleryImage(image.gallery_filename);
    return { bytes, filename: isJxl ? uploadFilename : image.filename || fallbackFilename };
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

  if (image.url) {
    try {
      return { bytes: await imageUrlToPngBytes(image.url), filename: uploadFilename };
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

/** Send image to txt2img/img2img tab as the input image. Caller switches tab. */
export async function sendImageToImg2Img(image: OutputImage): Promise<void> {
  const name = await uploadOutputImageForGenerationInput(image, "img2img_input.png");
  generation.inputImage = name;
  generation.mode = "img2img";
  gallery.closeLightbox();
}

/** Re-use image as upscale input. */
export async function sendImageToUpscale(image: OutputImage): Promise<void> {
  const name = await uploadOutputImageForGenerationInput(image, "refine_input.png");
  generation.inputImage = name;
  generation.mode = "img2img";
  generation.upscaleEnabled = true;
  gallery.closeLightbox();
}
