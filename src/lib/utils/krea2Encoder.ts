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
