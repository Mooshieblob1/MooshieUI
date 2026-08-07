import type { VideoAspectRatio } from "../types/index.js";

/**
 * TypeScript mirror of the H3 geometry helpers in
 * `src-tauri/src/templates/video.rs`. The backend remains the authority — these
 * exist purely so the settings panel can show the frame count and resolution the
 * user is about to get without a round trip. Any change to the Rust formulas
 * must be mirrored here or the preview silently lies.
 */

/** H3's frame-count widget accepts 17n+5 only; 24 fps, capped at 3592 frames. */
export const H3_FPS = 24;

/** ComfyUI's MiniMaxH3ImageToVideo `length` widget maximum, snapped down to 17n+5. */
const H3_MAX_FRAMES = 3592;

/** Duration slider bounds, matching the Rust validation arm in `templates/mod.rs`. */
export const H3_MIN_DURATION_SECONDS = 1;
export const H3_MAX_DURATION_SECONDS = 15;

/** Megapixel budgets offered in the panel. */
export const H3_MEGAPIXEL_OPTIONS = [0.4, 0.6, 1.0] as const;

export const H3_ASPECT_RATIOS: readonly VideoAspectRatio[] = [
  "16:9",
  "9:16",
  "1:1",
  "4:3",
  "3:4",
] as const;

/** Reference-to-video accepts at most 9 images (the node's autogrow limit). */
export const H3_MAX_REF_IMAGES = 9;

/**
 * Lowercase filename substrings identifying MiniMax H3 weights, mirroring
 * `H3_DIFFUSION_MARKERS` in `templates/video.rs`. Used to filter the diffusion
 * dropdown and to warn before the backend rejects the submission.
 */
export const H3_DIFFUSION_MARKERS = ["minimax", "h3"] as const;

/**
 * Frame count for a requested duration, snapped up to the next 17n+5 value.
 * 5 s -> 124 frames, matching `compute_h3_frame_length`.
 */
export function computeH3FrameLength(seconds: number): number {
  const base = Math.max(5, Math.round(seconds * H3_FPS));
  const snapped = base + (((5 - (base % 17)) % 17) + 17) % 17;
  return Math.min(snapped, H3_MAX_FRAMES);
}

/**
 * Width/height for a megapixel budget and aspect ratio, snapped to multiples of
 * 32 (the H3 width/height widget step). Mirrors `compute_h3_dimensions`;
 * unknown ratios fall back to 16:9.
 */
export function computeH3Dimensions(
  aspectRatio: string,
  megapixels: number,
): { width: number; height: number } {
  let rw = 16;
  let rh = 9;
  if (aspectRatio === "9:16") {
    rw = 9;
    rh = 16;
  } else if (aspectRatio === "1:1") {
    rw = 1;
    rh = 1;
  } else if (aspectRatio === "4:3") {
    rw = 4;
    rh = 3;
  } else if (aspectRatio === "3:4") {
    rw = 3;
    rh = 4;
  }
  const pixels = Math.max(0.05, megapixels) * 1_000_000;
  const height = Math.sqrt((pixels * rh) / rw);
  const width = (height * rw) / rh;
  const snap = (v: number) => Math.max(2, Math.round(v / 32)) * 32;
  return { width: snap(width), height: snap(height) };
}

/**
 * Resident VRAM of the H3 DiT, guessed from the quantisation marker in its
 * filename. The published stacks are NVFP4 (~12.5 GB), int8_convrot (~21 GB),
 * fp8_scaled (~22 GB) and bf16 (~32 GB); an unrecognised name assumes
 * int8_convrot, the widest-compatibility default.
 */
export function estimateH3ModelGb(filename: string | null | undefined): number {
  const name = (filename ?? "").toLowerCase();
  if (name.includes("nvfp4")) return 12.5;
  if (name.includes("fp8")) return 22;
  if (name.includes("bf16") || name.includes("fp16")) return 32;
  return 21;
}

/**
 * Rough VRAM ceiling for a given pixel/frame budget. Measured envelope: a
 * 124-frame 1344x768 generation fits in 24 GB while 362 frames at the same size
 * OOMs, so budget scales with pixels x frames on top of the resident model.
 * Returns gigabytes, deliberately conservative — this only drives a warning.
 */
export function estimateH3VramGb(
  width: number,
  height: number,
  frames: number,
  modelGb: number,
): number {
  const megapixelFrames = ((width * height) / 1_000_000) * frames;
  // ~11.9 GB resident for the pruned NVFP4 DiT at 128 MP-frames leaves roughly
  // 0.045 GB per MP-frame of activation/latent headroom.
  return modelGb + megapixelFrames * 0.045;
}
