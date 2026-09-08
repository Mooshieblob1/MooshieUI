import { generation } from "../stores/generation.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { fragmentForStyles, styles } from "../stores/styles.svelte.js";
import { classifyGenerationError } from "./generationErrors.js";
import { submitGeneration } from "./generationSubmit.js";

/**
 * Generate a thumbnail for a style that has none.
 *
 * The picture is made from whatever the user currently has in the prompt
 * boxes plus this style's artist tags, so the thumbnail shows the style doing
 * the job the user actually wants it for. Other active styles are skipped:
 * the tile has to represent this style alone.
 *
 * This lives in utils rather than in the styles store because `generation`
 * imports `styles`, so the reverse import would close a cycle. App.svelte
 * picks the claim up again when the image lands.
 */
export async function generateStyleThumbnail(styleId: string): Promise<void> {
  const style = styles.styles.find((s) => s.id === styleId);
  if (!style || style.artists.length === 0) return;

  const params = generation.toParams({
    extraPositive: fragmentForStyles(
      [{ artists: style.artists, overallWeight: style.overallWeight }],
      generation.isNovelAi,
    ),
    skipActiveStyles: true,
    overrides: { mode: "txt2img", input_image: null, mask_image: null },
  });
  // One image: the thumbnail only ever uses the first, and a batch would
  // charge the user for renders nothing looks at.
  params.batch_size = 1;

  try {
    const promptId = await submitGeneration(params);
    styles.pendingThumbnail = { styleId, promptId };
  } catch (err) {
    // Nothing is in flight, so nothing will ever arrive to clear a claim:
    // never set one on this path.
    const message = err instanceof Error ? err.message : String(err);
    const classified = classifyGenerationError(message);
    const detail =
      classified.messageKey === "generation.toast.failed"
        ? message
        : locale.t(classified.messageKey, classified.params);
    gallery.showToast(locale.t("styles.manager.thumb_gen_failed", { error: detail }), "error");
  }
}
