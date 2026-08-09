/**
 * The exact recipe every CDN artist preview was rendered with.
 *
 * Two consumers:
 *   - the "Preview Generation Parameters" modal, which displays it verbatim
 *   - the local "generate this preview yourself" action on placeholder cards
 *     for artists the CDN index has no image for (issue #527)
 *
 * Changing a value here silently makes locally generated previews look
 * different from the shipped set. Keep it in sync with the dataset pipeline.
 */
export const ARTIST_PREVIEW_RECIPE = {
  unet: "anima-base-v1.0.safetensors",
  textEncoder: "qwen_3_06b_base.safetensors",
  clipType: "wan",
  vae: "qwen_image_vae.safetensors",
  /** Matches `generation.modelFamily` for Anima; the backend picks the workflow from it. */
  architecture: "anima",
  sampler: "er_sde",
  scheduler: "sgm_uniform",
  steps: 25,
  cfg: 4.0,
  denoise: 1.0,
  /** Fixed across the whole set. Decimal string: 63-bit seeds exceed JS's safe-integer range. */
  seed: "7243057331061028000",
  width: 896,
  height: 1152,
  negativePrompt:
    "worst quality, low quality, score_1, score_2, score_3, blurry, jpeg artifacts, sepia, sensitive, nsfw, explicit",
  /** One positive prompt per variant. `{artist_tag}` is the substitution marker. */
  positivePrompts: [
    "{artist_tag}, year 2025, newest, masterpiece, best quality, score_9, score_8, highres, safe, 1girl, hatsune miku, straight-on, cowboy shot, school, serafuku, fence, long sleeves, outdoors, hamburger, eating, blue sky, plant",
    "{artist_tag}, year 2025, newest, masterpiece, best quality, score_9, score_8, highres, safe, 1girl, solo, umbrella, standing, holding umbrella, mouse girl, mouse ears, mouse tail, raincoat, yellow raincoat, rubber boots, yellow footwear, street, rain, raining, cowboy shot, black hair, long hair, blunt bangs, blunt ends, blue eyes, straight-on",
  ],
  /**
   * How the CDN copy is encoded for delivery. Display-only: a locally
   * generated preview is saved at full `width` x `height` in the user's
   * configured output format.
   */
  delivery: { format: "AVIF", width: 720, height: 926, quality: "80 (4:2:0)" },
} as const;

export type ArtistPreviewVariant = 1 | 2;

export interface ArtistPreviewStatus {
  state: "idle" | "running" | "ready" | "unavailable";
  /** Displayable object URL. Set only when `state === "ready"`. */
  src?: string;
  /** Recipe model filenames that are not installed. Set only when `state === "unavailable"`. */
  missing?: string[];
}

/**
 * Substitute the artist tag into a variant's positive prompt.
 *
 * Uses the RAW underscored tag with any leading "@" stripped, NOT
 * `artistInsert.normalizeArtistTag()`, which turns underscores into spaces.
 * The CDN previews were rendered with underscores; spaces tokenise
 * differently and would produce a different image.
 */
export function artistPreviewPrompt(tag: string, variant: ArtistPreviewVariant): string {
  const raw = tag.replace(/^@+/, "");
  return ARTIST_PREVIEW_RECIPE.positivePrompts[variant - 1].replace("{artist_tag}", raw);
}

function basename(filename: string): string {
  const normalized = filename.replace(/\\/g, "/");
  const slash = normalized.lastIndexOf("/");
  return slash >= 0 ? normalized.slice(slash + 1) : normalized;
}

/**
 * Recipe model files that are not installed, matched on basename because the
 * model lists carry a folder prefix (`diffusion_models/...`, `unet/...`).
 * An empty array means the recipe can run.
 */
export function missingRecipeModels(available: {
  diffusionModels: string[];
  textEncoders: string[];
  vaes: string[];
}): string[] {
  const has = (list: string[], want: string) => list.some((f) => basename(f) === want);
  const missing: string[] = [];
  if (!has(available.diffusionModels, ARTIST_PREVIEW_RECIPE.unet)) {
    missing.push(ARTIST_PREVIEW_RECIPE.unet);
  }
  if (!has(available.textEncoders, ARTIST_PREVIEW_RECIPE.textEncoder)) {
    missing.push(ARTIST_PREVIEW_RECIPE.textEncoder);
  }
  if (!has(available.vaes, ARTIST_PREVIEW_RECIPE.vae)) {
    missing.push(ARTIST_PREVIEW_RECIPE.vae);
  }
  return missing;
}
