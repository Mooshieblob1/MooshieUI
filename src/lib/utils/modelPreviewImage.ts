import { fetchCachedImage, uploadImageBytes } from "./api.js";

function dataUrlToBytes(dataUrl: string): { bytes: number[]; mime: string } {
  const match = dataUrl.match(/^data:([^;]+);base64,(.+)$/);
  if (!match) throw new Error("Invalid data URL");
  const mime = match[1];
  const binary = atob(match[2]);
  const bytes = new Array<number>(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return { bytes, mime };
}

function extensionForMime(mime: string): string {
  if (mime.includes("jpeg") || mime.includes("jpg")) return "jpg";
  if (mime.includes("webp")) return "webp";
  return "png";
}

/** Fetch image bytes from a remote URL (via backend cache) or data URL. */
export async function fetchModelPreviewImageBytes(
  url: string,
  fallbackName = "preview.png",
): Promise<{ bytes: number[]; filename: string }> {
  if (url.startsWith("data:")) {
    const { bytes, mime } = dataUrlToBytes(url);
    const ext = extensionForMime(mime);
    const base = fallbackName.replace(/\.[^.]+$/, "");
    return { bytes, filename: `${base}.${ext}` };
  }

  const dataUrl = await fetchCachedImage(url);
  const { bytes, mime } = dataUrlToBytes(dataUrl);
  const ext = extensionForMime(mime);
  const base = fallbackName.replace(/\.[^.]+$/, "");
  return { bytes, filename: `${base}.${ext}` };
}

/** Upload a model/Civitai preview URL to ComfyUI input and return the temp filename. */
export async function uploadModelPreviewImage(
  url: string,
  fallbackName = "model_preview.png",
): Promise<string> {
  const { bytes, filename } = await fetchModelPreviewImageBytes(url, fallbackName);
  const result = await uploadImageBytes(bytes, filename);
  return result.name;
}

export type ModelPreviewActionDetail =
  | { type: "interrogate"; imageUrl: string }
  | { type: "img2img"; imageUrl: string }
  | { type: "style_reference"; imageUrl: string };

export function dispatchModelPreviewAction(detail: ModelPreviewActionDetail): void {
  window.dispatchEvent(new CustomEvent("mooshie:model-preview-action", { detail }));
}
