import { downloadModel } from "./api.js";
import { recommendedUpscaleModels } from "./upscalers.js";
import { models } from "../stores/models.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { locale } from "../stores/locale.svelte.js";

/**
 * Make sure an upscaler model is installed before a generation names it.
 *
 * The picker is persisted, so a pick can outlive its file (deleted, or a
 * different models folder after a reinstall). ComfyUI then rejects the whole
 * graph, and in NovelAI mode that rejection is swallowed by the local pass,
 * which delivers the image un-upscaled. A recommended model is downloaded
 * here the same way picking it in the Refiner panel would; any other missing
 * model stops the generation with a toast saying which one.
 *
 * Returns true when the model is installed (or the inventory has not loaded
 * yet, so nothing can be told). On false, a toast has already said why.
 */
export async function ensureUpscalerInstalled(model: string): Promise<boolean> {
  // Before the first inventory load every list is empty; leave validation to
  // ComfyUI rather than download a model that may well be there.
  if (!("upscale_models" in models.serverModels)) return true;
  if (models.upscaleModels.includes(model)) return true;

  const rec = recommendedUpscaleModels.find((r) => r.filename === model);
  if (!rec) {
    gallery.showToast(locale.t("generation.upscale.model_missing", { model }), "error");
    return false;
  }
  gallery.showToast(locale.t("generation.upscale.model_downloading", { model: rec.label }), "info");
  try {
    await downloadModel(rec.url, "upscale_models", rec.filename);
    await models.refresh();
    return true;
  } catch (e) {
    gallery.showToast(
      locale.t("generation.upscale.model_download_failed", { model: rec.label, error: String(e) }),
      "error",
    );
    return false;
  }
}
