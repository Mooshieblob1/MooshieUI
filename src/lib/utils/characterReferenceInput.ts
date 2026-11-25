/**
 * Shared image-source helpers for loading character Precise Reference images
 * from files, clipboard paste, gallery selections, current preview, or
 * desktop Tauri file drops.
 */
import { readClipboardImageSafe } from "./api.js";
import { loadOutputImageForGenerationInput } from "./galleryActions.js";
import { fileToNovelAiBase64 } from "./novelaiImage.js";
import {
  blobToCharacterThumbnail,
  characterReferenceBase64,
  type CharacterImages,
} from "./characterThumbnail.js";
import type { OutputImage } from "../types/index.js";

/** Extract image blobs from a clipboard paste event or fall back to OS clipboard read. */
export async function readImageBlobsFromClipboard(
  clipboardItems?: DataTransferItemList,
): Promise<Blob[]> {
  if (clipboardItems) {
    const out: Blob[] = [];
    for (const item of Array.from(clipboardItems)) {
      if (item.type.startsWith("image/")) {
        const file = item.getAsFile();
        if (file) out.push(file);
      }
    }
    if (out.length > 0) return out;
  }
  try {
    const bytes = await readClipboardImageSafe();
    if (!bytes || bytes.length === 0) return [];
    return [new Blob([new Uint8Array(bytes)], { type: "image/png" })];
  } catch {
    return [];
  }
}

/** Decode gallery `OutputImage` selections into PNG blobs for reference ingestion. */
export async function outputImagesToBlobs(images: readonly OutputImage[]): Promise<Blob[]> {
  const blobs: Blob[] = [];
  for (const image of images) {
    try {
      const { bytes } = await loadOutputImageForGenerationInput(image, "reference.png");
      blobs.push(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
    } catch (err) {
      console.error("Failed to load gallery image for character reference:", err);
    }
  }
  return blobs;
}

/** Read local filesystem paths from a Tauri file-drop event into `File` blobs. */
export async function tauriPathsToFiles(paths: readonly string[]): Promise<File[]> {
  if (paths.length === 0) return [];
  const { readFile } = await import("@tauri-apps/plugin-fs");
  const files: File[] = [];
  for (const p of paths) {
    const bytes = await readFile(p);
    const name = p.split(/[\\/]/).pop() || "reference.png";
    files.push(new File([bytes], name));
  }
  return files;
}

/** Convert image blobs into stored character thumbnail+reference WebP data URLs. */
export async function blobsToCharacterImages(blobs: readonly Blob[]): Promise<{
  items: CharacterImages[];
  failed: number;
}> {
  const items: CharacterImages[] = [];
  let failed = 0;
  for (const blob of blobs) {
    const res = await blobToCharacterThumbnail(blob);
    if (res) items.push(res);
    else failed += 1;
  }
  return { items, failed };
}

/** Convert image blobs into NovelAI bare base64 PNG strings. */
export async function blobsToNovelAiBase64s(blobs: readonly Blob[]): Promise<{
  images: string[];
  failed: number;
}> {
  const images: string[] = [];
  let failed = 0;
  for (const blob of blobs) {
    const base64 = await fileToNovelAiBase64(blob);
    if (base64) images.push(base64);
    else failed += 1;
  }
  return { images, failed };
}

/** Encode a stored character thumbnail/reference pair into NovelAI base64 PNG. */
export { characterReferenceBase64 };

/** Svelte action that forwards Tauri custom `tauri-file-drop` paths to `onPaths`. */
export function tauriDropFiles(
  el: HTMLElement,
  onPaths: (paths: string[]) => void,
) {
  let cb = onPaths;
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ path?: string; paths?: string[] }>).detail;
    const paths = detail?.paths?.length ? detail.paths : detail?.path ? [detail.path] : [];
    if (paths.length > 0) cb(paths);
  };
  el.addEventListener("tauri-file-drop", handler);
  return {
    update(next: (paths: string[]) => void) {
      cb = next;
    },
    destroy() {
      el.removeEventListener("tauri-file-drop", handler);
    },
  };
}
