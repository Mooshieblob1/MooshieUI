import type { OutputImage } from "../types/index.js";
import { loadOutputImageForGenerationInput } from "./galleryActions.js";

export interface NormalizedInputImage {
  bytes: number[];
  previewBlob: Blob;
  previewUrl: string;
  width: number;
  height: number;
  filename: string;
}

export interface PreparedEditImage {
  uploadBytes: number[];
  uploadFilename: string;
  normalized: NormalizedInputImage | null;
}

const MAX_INPUT_PIXELS = 1024 * 1024;

export async function normalizeGenerationInputBytes(
  imageBytes: number[],
  fallbackFilename: string,
): Promise<NormalizedInputImage> {
  const sourceBlob = new Blob([new Uint8Array(imageBytes)], { type: "image/png" });
  const sourceUrl = URL.createObjectURL(sourceBlob);

  const dims = await new Promise<{ width: number; height: number }>((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve({ width: img.naturalWidth, height: img.naturalHeight });
    img.onerror = () => reject(new Error("Failed to read image dimensions"));
    img.src = sourceUrl;
  });

  const sourcePixels = dims.width * dims.height;
  if (sourcePixels <= MAX_INPUT_PIXELS) {
    return {
      bytes: imageBytes,
      previewBlob: sourceBlob,
      previewUrl: sourceUrl,
      width: dims.width,
      height: dims.height,
      filename: fallbackFilename,
    };
  }

  const scale = Math.sqrt(MAX_INPUT_PIXELS / sourcePixels);
  const targetWidth = Math.max(8, Math.round(dims.width * scale));
  const targetHeight = Math.max(8, Math.round(dims.height * scale));

  const resizedBlob = await new Promise<Blob>((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      const out = document.createElement("canvas");
      out.width = targetWidth;
      out.height = targetHeight;
      const ctx = out.getContext("2d");
      if (!ctx) {
        reject(new Error("Failed to create resize context"));
        return;
      }
      ctx.imageSmoothingEnabled = true;
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(img, 0, 0, targetWidth, targetHeight);
      out.toBlob((blob) => {
        if (!blob) {
          reject(new Error("Failed to encode resized image"));
          return;
        }
        resolve(blob);
      }, "image/png");
    };
    img.onerror = () => reject(new Error("Failed to decode source image"));
    img.src = sourceUrl;
  });

  URL.revokeObjectURL(sourceUrl);
  const resizedBuffer = await resizedBlob.arrayBuffer();
  const resizedBytes = Array.from(new Uint8Array(resizedBuffer));

  return {
    bytes: resizedBytes,
    previewBlob: resizedBlob,
    previewUrl: URL.createObjectURL(resizedBlob),
    width: targetWidth,
    height: targetHeight,
    filename: fallbackFilename,
  };
}

export async function prepareOutputImageForEditMode(
  image: OutputImage,
  mode: "img2img" | "inpainting",
): Promise<PreparedEditImage> {
  const source = await loadOutputImageForGenerationInput(
    image,
    mode === "inpainting" ? "inpaint_input.png" : "img2img_input.png",
  );

  if (mode !== "inpainting") {
    return {
      uploadBytes: source.bytes,
      uploadFilename: source.filename,
      normalized: null,
    };
  }

  const normalized = await normalizeGenerationInputBytes(source.bytes, source.filename);
  return {
    uploadBytes: normalized.bytes,
    uploadFilename: normalized.filename,
    normalized,
  };
}
