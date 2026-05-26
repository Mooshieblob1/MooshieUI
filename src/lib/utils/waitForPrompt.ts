import { progress } from "../stores/progress.svelte.js";
import { readTempImage, recoverPromptOutputs } from "./api.js";

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Poll until a prompt is no longer in the progress queue. */
export async function waitForPromptCompletion(
  promptId: string,
  timeoutMs = 600_000,
): Promise<void> {
  const start = Date.now();
  while (progress.pendingPrompts.some((p) => p.promptId === promptId)) {
    if (Date.now() - start > timeoutMs) {
      throw new Error(`Generation timed out (${promptId})`);
    }
    await sleep(200);
  }
  await sleep(400);
}

async function tryRecoverPromptOutput(promptId: string): Promise<string | null> {
  try {
    const { images } = await recoverPromptOutputs(promptId);
    const filename = images.at(-1)?.temp_filename?.trim();
    if (!filename) return null;
    progress.registerPromptOutput(promptId, filename);
    return filename;
  } catch {
    return null;
  }
}

/** Wait for a prompt to finish and return its output temp filename. */
export async function waitForPromptOutput(
  promptId: string,
  timeoutMs = 600_000,
): Promise<string> {
  const existing = progress.getPromptOutputTemp(promptId);
  if (existing) return existing;

  const start = Date.now();
  let lastRecoverAttempt = 0;

  while (Date.now() - start < timeoutMs) {
    const registered = progress.getPromptOutputTemp(promptId);
    if (registered) return registered;

    const pending = progress.pendingPrompts.some((p) => p.promptId === promptId);
    const now = Date.now();
    if (!pending || now - lastRecoverAttempt >= 400) {
      lastRecoverAttempt = now;
      const recovered = await tryRecoverPromptOutput(promptId);
      if (recovered) return recovered;
    }

    await sleep(200);
  }

  throw new Error(`No output image found for prompt ${promptId}`);
}

/** Load a temp output as PNG bytes for ComfyUI upload. */
export async function tempOutputToUploadBytes(tempFilename: string): Promise<number[]> {
  const bytes = await readTempImage(tempFilename);
  const lower = tempFilename.toLowerCase();
  const type = lower.endsWith(".webp")
    ? "image/webp"
    : lower.endsWith(".jpg") || lower.endsWith(".jpeg")
      ? "image/jpeg"
      : "image/png";
  const blob = new Blob([new Uint8Array(bytes)], { type });
  if (type === "image/png") {
    return Array.from(new Uint8Array(bytes));
  }
  const canvas = document.createElement("canvas");
  const url = URL.createObjectURL(blob);
  try {
    const img = await new Promise<HTMLImageElement>((resolve, reject) => {
      const el = new Image();
      el.onload = () => resolve(el);
      el.onerror = () => reject(new Error("Failed to decode temp output"));
      el.src = url;
    });
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Canvas 2d unavailable");
    ctx.drawImage(img, 0, 0);
    const pngBlob = await new Promise<Blob | null>((resolve) => {
      canvas.toBlob((b) => resolve(b), "image/png");
    });
    if (!pngBlob) throw new Error("PNG encode failed");
    return Array.from(new Uint8Array(await pngBlob.arrayBuffer()));
  } finally {
    URL.revokeObjectURL(url);
  }
}
