/**
 * Krea 2 text encoder selection. Krea 2 only works with Qwen3-VL-4B
 * (12x2560 = 30720-dim conditioning); any other encoder fails deep inside
 * ComfyUI sampling with a feature-count error. Marker list mirrors
 * KREA2_TEXT_ENCODER_MARKERS in src-tauri/src/commands/api.rs.
 */
export const KREA2_ENCODER_MARKERS = ["qwen3vl-4b", "qwen3vl_4b", "qwen3-vl-4b", "qwen3_vl_4b", "qwen3vl4b"];

/**
 * Abliterated copies of the encoder. Qwen3-VL-4B-Instruct is safety tuned, and
 * Krea 2 conditions on its hidden states, so its refusal behaviour can weaken
 * or drop concepts in the image; these copies have that removed.
 */
const UNCENSORED_MARKERS = ["heretic", "abliterated", "uncensored"];

/**
 * Heretic-abliterated Qwen3-VL-4B in ComfyUI's fp8 layout, the same precision
 * class as the stock fp8 encoder. Pinned to a commit so the file cannot change
 * under the same URL.
 */
export const KREA2_UNCENSORED_ENCODER = {
  filename: "qwen3-vl-4b-heretic_fp8_e4m3fn.safetensors",
  url: "https://huggingface.co/DreamFast/Qwen3-VL-4b-Heretic-ComfyUI/resolve/c5bd34e940564e4b5b64286694d72f47f429c126/qwen3-vl-4b-heretic_fp8_e4m3fn.safetensors",
  category: "text_encoders",
  bytes: 4_831_492_476,
};

/**
 * Capitan01R's Krea 2 TextFusion refusal-reduction LoRA, v2.0 full rank
 * (Civitai model 2775340, version 3359022). It retrains only the TextFusion
 * path (layerwise/refiner blocks, the 12-layer projector, txtmlp) so the DiT
 * suppresses requested concepts less: the uncensored encoder changes what the
 * text says, this changes how Krea 2 listens to it. Every key maps onto
 * Krea 2 through ComfyUI's stock LoraLoader, norm scales included.
 *
 * Civitai needs a login to download it, so this is the byte-identical public
 * Hugging Face copy (SHA256 999c6bc1..., matching Civitai's), pinned to a
 * commit. The rank-64 v2 is only mirrored in a gated repo.
 */
export const KREA2_REFUSAL_LORA = {
  filename: "refusal_reduction_v2_full_rank.safetensors",
  url: "https://huggingface.co/yannikrhl/Krea2_TextFusion_Refusal-Reduction_LoRA/resolve/fd1b55fe70b5513dd8e00c517f1b2e7fc277eb77/refusal_reduction_v2_full_rank.safetensors",
  category: "loras",
  bytes: 2_603_651_944,
};

/** Upstream recommendation: strength 1.0. */
export const KREA2_REFUSAL_LORA_STRENGTH = 1.0;

/** Any version of the refusal-reduction LoRA, under its upstream names. */
export function isKrea2RefusalLora(name: string): boolean {
  return /refusal_reduction|textfusion_refusal/i.test(name);
}

/** The installed copy of the pinned LoRA, preferring the exact filename. */
export function installedKrea2RefusalLora(installed: readonly string[]): string | null {
  const basename = (n: string) => n.split(/[\\/]/).pop();
  return installed.find((n) => basename(n) === KREA2_REFUSAL_LORA.filename) ?? null;
}

/**
 * Whether this install has the uncensored encoder but not the refusal-reduction
 * LoRA: what a user who set up uncensored mode before the LoRA joined it has.
 */
export function needsKrea2RefusalLora(encoders: readonly string[], loras: readonly string[]): boolean {
  return encoders.some(isKrea2UncensoredEncoder) && installedKrea2RefusalLora(loras) === null;
}

/**
 * The refusal-reduction LoRA to add to an outgoing generation, or null.
 * Only for Krea 2 with uncensored mode on and the file installed, and never
 * when the user already has a version of it in their own enabled LoRA list,
 * so it cannot leak into another model family or apply twice.
 */
export function krea2RefusalLoraToApply(
  family: string,
  uncensored: boolean,
  installed: readonly string[],
  enabledLoras: readonly string[],
): string | null {
  if (family !== "krea2" || !uncensored) return null;
  if (enabledLoras.some(isKrea2RefusalLora)) return null;
  return installedKrea2RefusalLora(installed);
}

export function isKrea2Encoder(filename: string | null | undefined): boolean {
  if (!filename) return false;
  const lower = filename.toLowerCase();
  return KREA2_ENCODER_MARKERS.some((marker) => lower.includes(marker));
}

export function isKrea2UncensoredEncoder(filename: string | null | undefined): boolean {
  if (!isKrea2Encoder(filename)) return false;
  const lower = filename!.toLowerCase();
  return UNCENSORED_MARKERS.some((marker) => lower.includes(marker));
}

/**
 * The installed Krea 2 encoder to use: the current one when it matches the
 * uncensored preference, else an installed one that does, else the current
 * (or any) Krea 2 encoder. Null when none is installed or the inventory has
 * not loaded yet (an empty list must not read as "nothing installed").
 */
export function pickKrea2Encoder(
  encoders: readonly string[],
  current: string | null | undefined,
  preferUncensored: boolean,
): string | null {
  const installed = (f: string | null | undefined): f is string => !!f && encoders.includes(f);
  const fits = (f: string) => isKrea2Encoder(f) && isKrea2UncensoredEncoder(f) === preferUncensored;
  if (installed(current) && fits(current)) return current;
  const preferred = encoders.find(fits);
  if (preferred) return preferred;
  if (installed(current) && isKrea2Encoder(current)) return current;
  return encoders.find(isKrea2Encoder) ?? null;
}
