/** Curated upscale models offered in Upscale settings (auto-downloaded on
 *  first selection). Shared with the preview Refine button, which defaults to
 *  OmniSR 2x when the user hasn't picked a model upscaler. */

export interface RecommendedUpscaleModel {
  label: string;
  filename: string;
  url: string;
  description: string;
}

const HF_BASE = "https://huggingface.co/AshtakaOOf/safetensored-upscalers/resolve/main";

/** Default model upscaler used by the preview Refine button. */
export const DEFAULT_REFINE_UPSCALER = "OmniSR_X2_DIV2K.safetensors";
export const DEFAULT_REFINE_UPSCALER_SCALE = 2;

/** Extract scale factor from upscaler model names (e.g., "OmniSR_X4_DIV2K" → 4, "2x_Modern..." → 2) */
export function extractScaleFromModel(filename: string): number | null {
  const match =
    filename.match(/_X(\d+)[_.]/i) ||
    filename.match(/[_-](\d+)x[_.]/i) ||
    filename.match(/^(\d+)x[_A-Z]/i);
  return match ? parseInt(match[1], 10) : null;
}

export const recommendedUpscaleModels: RecommendedUpscaleModel[] = [
  // SPAN — fast, sharp, excellent general-purpose upscaler
  {
    label: "SPAN 2x — Spanimation",
    filename: "2x_ModernSpanimationV1.safetensors",
    url: `${HF_BASE}/span/2x_ModernSpanimationV1.safetensors`,
    description: "Fast 2x with clean lines and vivid colours. Great for anime and illustration.",
  },
  {
    label: "SPAN 4x — NomosUni",
    filename: "4xNomosUni_span_multijpg.safetensors",
    url: `${HF_BASE}/span/4xNomosUni_span_multijpg.safetensors`,
    description: "Fast 4x all-rounder. Handles photos, art, and JPEG artifacts well.",
  },
  // OmniSR — lightweight, reliable, good balance of speed and quality
  {
    label: "OmniSR 2x",
    filename: "OmniSR_X2_DIV2K.safetensors",
    url: `${HF_BASE}/omnisr/OmniSR_X2_DIV2K.safetensors`,
    description: "Tiny model (~1.6 MB). Quick and artifact-free 2x upscale.",
  },
  {
    label: "OmniSR 3x",
    filename: "OmniSR_X3_DIV2K.safetensors",
    url: `${HF_BASE}/omnisr/OmniSR_X3_DIV2K.safetensors`,
    description: "Tiny model (~1.7 MB). Balanced 3x upscale when 2x isn't enough and 4x is too much.",
  },
  {
    label: "OmniSR 4x",
    filename: "OmniSR_X4_DIV2K.safetensors",
    url: `${HF_BASE}/omnisr/OmniSR_X4_DIV2K.safetensors`,
    description: "Tiny model (~1.7 MB). Quick 4x upscale with solid detail for its size.",
  },
  // DAT — slow but highest quality, best for final output
  {
    label: "DAT 4x — IllustrationJaNai",
    filename: "4x_IllustrationJaNai_V1_DAT2_190k.safetensors",
    url: `${HF_BASE}/dat/4x_IllustrationJaNai_V1_DAT2_190k.safetensors`,
    description: "Slow but excellent quality. Best for illustrations and anime final prints (~140 MB).",
  },
];
