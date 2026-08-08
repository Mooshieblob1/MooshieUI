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

/**
 * Pixel-budget slider bounds. A continuous 0.1 MP step rather than a handful of
 * presets: the snap-to-32 rule means most neighbouring steps land on genuinely
 * different resolutions, and the VRAM headroom a given card has is not a value
 * three presets can hit. The ceiling is a practical one, not a model limit -
 * beyond 2 MP the activation budget outgrows every consumer card.
 */
export const H3_MIN_MEGAPIXELS = 0.1;
export const H3_MAX_MEGAPIXELS = 2.0;
export const H3_MEGAPIXEL_STEP = 0.1;

/** Nearest tenth, so 0.1-step arithmetic does not leak 0.30000000000000004. */
function roundToStep(megapixels: number): number {
  return Math.round(megapixels * 10) / 10;
}

/** Snaps a stored or typed budget onto the slider's range and step. */
export function clampH3Megapixels(megapixels: number): number {
  if (!Number.isFinite(megapixels)) return H3_MIN_MEGAPIXELS;
  return roundToStep(Math.min(H3_MAX_MEGAPIXELS, Math.max(H3_MIN_MEGAPIXELS, megapixels)));
}

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
 * Timeline (H3 Director) budgets, mirroring the constants at the top of
 * `comfyui-nodes/minimax_director/minimax_plan.py`. The node validates these
 * server-side and aborts the run; enforcing them client-side turns a failed
 * generation into a disabled button.
 */
export const H3_MAX_REF_VIDEOS = 3;
export const H3_MAX_REF_AUDIOS = 3;
/** `substitute_char_tags` only swaps `@character1`-`@character3`. */
export const H3_MAX_TIMELINE_CHARACTERS = 3;
/** Each reference clip must be 2-15 s, and all of them together at most 15 s. */
export const H3_REF_VIDEO_MIN_SECONDS = 2;
export const H3_REF_VIDEO_MAX_SECONDS = 15;
export const H3_REF_VIDEO_TOTAL_SECONDS = 15;

/**
 * Turbo LoRA step bounds, mirroring `H3_TURBO_MIN_STEPS`/`H3_TURBO_MAX_STEPS`
 * in `templates/video.rs`. Below 4 the distilled model has no schedule left to
 * work with; above 8 it stops improving and starts over-sharpening. The
 * undistilled base model uses `H3_DEFAULT_STEPS` instead.
 */
export const H3_TURBO_MIN_STEPS = 4;
export const H3_TURBO_MAX_STEPS = 8;
export const H3_TURBO_DEFAULT_STEPS = 6;
export const H3_DEFAULT_STEPS = 20;

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
 * `W:H` -> numerator/denominator. Accepts any positive pair, not just the five
 * presets, so an uploaded frame's own pixel dimensions ("1920:1080") describe a
 * ratio as well as a label does. Anything unparseable falls back to 16:9.
 * Mirrors `parse_aspect_ratio` in `templates/video.rs`.
 */
function parseAspectRatio(aspectRatio: string): [number, number] {
  const parts = aspectRatio.split(":");
  if (parts.length !== 2) return [16, 9];
  const rw = Number(parts[0]);
  const rh = Number(parts[1]);
  if (!Number.isFinite(rw) || !Number.isFinite(rh) || rw <= 0 || rh <= 0) return [16, 9];
  return [rw, rh];
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
  const [rw, rh] = parseAspectRatio(aspectRatio);
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
 * Fraction of card VRAM above which pinning the whole DiT resident is harmful.
 * ComfyUI's `--highvram` (VRAM Mode "high") force-loads the model instead of
 * staging it dynamically, so a DiT this close to the card's capacity leaves no
 * headroom for activations and the pass spills to host memory over PCIe.
 * Measured on a 12 GB RTX 5070 with the 12.5 GB NVFP4 DiT: 176 s/it under
 * `--highvram` against 4.2 s/it with dynamic loading, and no out-of-memory
 * error to explain the difference.
 */
const H3_HIGHVRAM_HEADROOM_FRACTION = 0.9;

/**
 * Whether VRAM Mode "high" would starve an H3 pass of activation headroom.
 * `false` when VRAM is unknown — a warning nobody can act on is just noise.
 */
export function isH3HighVramHarmful(
  modelGb: number,
  detectedVramGb: number | null,
): boolean {
  if (detectedVramGb === null || detectedVramGb <= 0) return false;
  return modelGb > detectedVramGb * H3_HIGHVRAM_HEADROOM_FRACTION;
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
  options?: { nvfp4Emulated?: boolean },
): number {
  const megapixelFrames = ((width * height) / 1_000_000) * frames;
  // ~11.9 GB resident for the pruned NVFP4 DiT at 128 MP-frames leaves roughly
  // 0.045 GB per MP-frame of activation/latent headroom.
  const perMegapixelFrame = 0.045 * (options?.nvfp4Emulated ? H3_NVFP4_EMULATION_PENALTY : 1);
  return modelGb + megapixelFrames * perMegapixelFrame;
}

/** Blackwell. Below this the NVFP4 kernels fall back to a slow emulation path. */
export const H3_BLACKWELL_COMPUTE_CAPABILITY = 12.0;

/** Whether the selected DiT is one of the NVFP4 quants. */
export function isH3Nvfp4Model(filename: string | null | undefined): boolean {
  return (filename ?? "").toLowerCase().includes("nvfp4");
}

/**
 * True when an NVFP4 DiT is about to run on a pre-Blackwell card. The FP4
 * tensor-core path does not exist there, so the weights are widened on the fly
 * during the pass - the file on disk is still small, but the transient cost per
 * frame is not. `false` when the compute capability probe failed: guessing a
 * penalty from no information would only misplace the warning.
 */
export function isH3Nvfp4Emulated(
  filename: string | null | undefined,
  computeCapability: number | null,
): boolean {
  if (computeCapability === null) return false;
  return isH3Nvfp4Model(filename) && computeCapability < H3_BLACKWELL_COMPUTE_CAPABILITY;
}

/** Extra activation headroom the widening path costs, per MP-frame. */
const H3_NVFP4_EMULATION_PENALTY = 1.35;

/**
 * VRAM the desktop, the driver and this app's own surfaces hold that a
 * generation never gets. Modelled as a fraction with a ceiling rather than a
 * flat number: the compositor costs about the same on a 24 GB card as on an
 * 8 GB one, so a flat reserve would be a rounding error on the big card and a
 * third of the useful memory on the small one.
 */
const H3_VRAM_SYSTEM_RESERVE_FRACTION = 0.08;
const H3_VRAM_SYSTEM_RESERVE_MAX_GB = 1.5;

/** Card VRAM minus the desktop reserve; `null` when the probe found no GPU. */
export function h3UsableVramGb(detectedVramGb: number | null): number | null {
  if (detectedVramGb === null || detectedVramGb <= 0) return null;
  const reserve = Math.min(
    H3_VRAM_SYSTEM_RESERVE_MAX_GB,
    detectedVramGb * H3_VRAM_SYSTEM_RESERVE_FRACTION,
  );
  return Math.max(0, detectedVramGb - reserve);
}

/**
 * `"tight"` once the estimate is within this fraction of usable VRAM. A pass
 * that only just fits does not fail - it starts swapping weights over PCIe
 * between steps, which is slow enough to look like a hang.
 */
const H3_VRAM_TIGHT_FRACTION = 0.85;

export type H3VramVerdict = "unknown" | "fits" | "tight" | "over";

/** Where a required budget lands against the usable VRAM of the card. */
export function assessH3Vram(requiredGb: number, usableGb: number | null): H3VramVerdict {
  if (usableGb === null) return "unknown";
  if (requiredGb > usableGb) return "over";
  if (requiredGb > usableGb * H3_VRAM_TIGHT_FRACTION) return "tight";
  return "fits";
}

/**
 * Largest 0.1-step pixel budget that still lands on `"fits"` for this card,
 * duration and model, or `null` when nothing in range does (a DiT too big for
 * the card cannot be rescued by shrinking frames). Drives the "try N MP"
 * suggestion, so it walks the whole range rather than only downwards - a user
 * on a 32 GB card should be told they have room to spare, not only when to cut.
 */
export function suggestH3Megapixels(
  aspectRatio: string,
  frames: number,
  modelGb: number,
  usableGb: number | null,
  options?: { nvfp4Emulated?: boolean },
): number | null {
  if (usableGb === null) return null;
  const steps = Math.round(H3_MAX_MEGAPIXELS / H3_MEGAPIXEL_STEP);
  for (let step = steps; step >= 1; step--) {
    const megapixels = roundToStep(step * H3_MEGAPIXEL_STEP);
    const { width, height } = computeH3Dimensions(aspectRatio, megapixels);
    const required = estimateH3VramGb(width, height, frames, modelGb, options);
    if (assessH3Vram(required, usableGb) === "fits") return megapixels;
  }
  return null;
}
