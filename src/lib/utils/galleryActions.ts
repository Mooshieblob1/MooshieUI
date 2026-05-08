/**
 * Shared gallery image actions used by both desktop App.svelte handlers and
 * the mobile action-sheet UI. These helpers wrap the gallery / generation
 * stores so callers don't need to duplicate the bytes-load + upload dance.
 */
import type { OutputImage } from "../types/index.js";
import { gallery } from "../stores/gallery.svelte.js";
import { generation } from "../stores/generation.svelte.js";
import {
  loadGalleryImage,
  getOutputImage,
  uploadImageBytes,
} from "./api.js";

async function loadAndUpload(image: OutputImage): Promise<string> {
  const bytes = image.gallery_filename
    ? await loadGalleryImage(image.gallery_filename)
    : await getOutputImage(image.filename, image.subfolder);
  const response = await uploadImageBytes(bytes, image.filename);
  return response.name;
}

/** Send image to txt2img/img2img tab as the input image. Caller switches tab. */
export async function sendImageToImg2Img(image: OutputImage): Promise<void> {
  const name = await loadAndUpload(image);
  generation.inputImage = name;
  generation.mode = "img2img";
  gallery.closeLightbox();
}

/** Re-use image as upscale input. */
export async function sendImageToUpscale(image: OutputImage): Promise<void> {
  const name = await loadAndUpload(image);
  generation.inputImage = name;
  generation.mode = "img2img";
  generation.upscaleEnabled = true;
  gallery.closeLightbox();
}
