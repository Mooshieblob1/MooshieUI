/**
 * Turning a picked file into the base64 PNG NovelAI's reference fields take.
 *
 * NovelAI reference images go straight into the request body as bare base64,
 * with no data-URL prefix and no ComfyUI upload step, so this never touches the
 * backend. Large sources are downscaled first: a reference only needs enough
 * pixels for NovelAI to read it, and the encoded string is carried in every
 * request the vibe stays attached to.
 *
 * This is a leaf util. It must not import a store.
 */

/** Longest side a reference image is scaled down to before encoding. */
export const NOVELAI_REFERENCE_MAX_DIMENSION = 1024;

async function decode(file: Blob): Promise<HTMLImageElement> {
  const url = URL.createObjectURL(file);
  try {
    return await new Promise<HTMLImageElement>((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = () => reject(new Error("Failed to decode reference image"));
      img.src = url;
    });
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Encode an image file as bare base64 PNG, downscaled to fit `maxDimension`.
 *
 * Returns null when the file is not an image or cannot be decoded, which the
 * caller shows as a dropped upload rather than an error.
 */
export async function fileToNovelAiBase64(
  file: Blob,
  maxDimension = NOVELAI_REFERENCE_MAX_DIMENSION,
): Promise<string | null> {
  if (file.type && !file.type.startsWith("image/")) return null;
  try {
    const image = await decode(file);
    const longest = Math.max(image.width, image.height);
    if (!longest) return null;
    const scale = longest > maxDimension ? maxDimension / longest : 1;
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(image.width * scale));
    canvas.height = Math.max(1, Math.round(image.height * scale));
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
    // NovelAI wants the payload without the `data:image/png;base64,` prefix.
    return canvas.toDataURL("image/png").split(",")[1] ?? null;
  } catch {
    return null;
  }
}

/** Wrap a stored bare base64 PNG back into something an `<img>` can show. */
export function novelAiBase64ToSrc(base64: string): string {
  return `data:image/png;base64,${base64}`;
}
