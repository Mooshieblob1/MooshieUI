import { generation } from "../stores/generation.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { canvas } from "../stores/canvas.svelte.js";
import { progress } from "../stores/progress.svelte.js";
import { loadOutputImageForGenerationInput } from "./galleryActions.js";
import { normalizeGenerationInputBytes, MAX_INPUT_PIXELS_INPAINT } from "./editImagePreparation.js";
import { uploadImageBytes } from "./api.js";

/**
 * Painting corrections into a paused generation.
 *
 * The paused preview goes to the inpaint canvas like "Send to inpaint" does,
 * with the store armed so the next Generate continues the paused run with
 * the painted image instead of starting a normal inpaint. Lives in utils
 * because it orchestrates several stores; a store importing it would cycle.
 */

/** The gallery image produced by the stage the run paused at, if still around. */
export function findPausedStageImage() {
  const last = generation.pausedStages[generation.pausedStages.length - 1];
  if (!last) return null;
  return (
    gallery.sessionImages.find((image) => image.prompt_id === last.promptId) ??
    gallery.images.find((image) => image.prompt_id === last.promptId) ??
    null
  );
}

/**
 * Load the paused preview into the inpaint canvas and arm the edit. Returns
 * false when the preview is no longer available.
 */
export async function beginPausedEdit(): Promise<boolean> {
  const image = findPausedStageImage();
  if (!image) return false;

  const source = await loadOutputImageForGenerationInput(image, `paused_${Date.now()}.png`);
  // Size is locked for the paused run, so the cap only matters for images the
  // run could not have produced anyway; the graph rescales to the run's size.
  const normalized = await normalizeGenerationInputBytes(source.bytes, source.filename, MAX_INPUT_PIXELS_INPAINT);
  const upload = await uploadImageBytes(normalized.bytes, normalized.filename);

  generation.mode = "inpainting";
  canvas.clearMask();
  canvas.isCanvasMode = true;
  canvas.clearStaging();
  canvas.setInpaintDrawMode("mask");
  progress.setLastOutputForMode("inpainting", null);
  canvas.setInpaintOriginalSource({
    previewUrl: normalized.previewUrl,
    width: normalized.width,
    height: normalized.height,
    uploadedInputName: upload.name,
  });
  if (canvas.layers.length === 0 || canvas.canvasWidth !== normalized.width || canvas.canvasHeight !== normalized.height) {
    canvas.initCanvas(normalized.width, normalized.height);
  }
  generation.pausedEditArmed = true;
  return true;
}

function hasVisiblePixels(layer: HTMLCanvasElement): boolean {
  const ctx = layer.getContext("2d");
  if (!ctx) return false;
  const data = ctx.getImageData(0, 0, layer.width, layer.height).data;
  for (let i = 3; i < data.length; i += 4) {
    if (data[i] > 0) return true;
  }
  return false;
}

/**
 * The image the resume blends in: the paused preview with the canvas's
 * paint layers composited on top. With nothing painted the preview itself is
 * used, which re-rolls the masked region from the paused state.
 */
export async function uploadPausedEditImage(raster: HTMLCanvasElement | null): Promise<string | null> {
  const base = generation.inputImage;
  if (!base) return null;
  const referenceUrl = canvas.referenceImageUrl;
  if (!raster || !referenceUrl || !hasVisiblePixels(raster)) return base;

  const reference = await new Promise<HTMLImageElement>((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("paused preview failed to load"));
    img.src = referenceUrl;
  });

  const composite = document.createElement("canvas");
  composite.width = reference.naturalWidth || raster.width;
  composite.height = reference.naturalHeight || raster.height;
  const ctx = composite.getContext("2d");
  if (!ctx) return base;
  ctx.drawImage(reference, 0, 0, composite.width, composite.height);
  ctx.drawImage(raster, 0, 0, composite.width, composite.height);

  const result = await canvas.exportLayerAsImage(composite, "paused_edit.png");
  return result.name;
}
