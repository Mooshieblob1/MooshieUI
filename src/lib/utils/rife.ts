/** Interpolation factors offered in the UI. Rust clamps to 1-4 regardless. */
export const RIFE_MULTIPLIERS = [2, 3, 4] as const;

/** The node's `scale_factor` combo list. Anything else is rejected by ComfyUI. */
export const RIFE_SCALE_FACTORS = [0.25, 0.5, 1, 2, 4] as const;

/** Above this, warn before submitting: RIFE holds the whole output in RAM. */
export const RIFE_MEMORY_WARN_BYTES = 3 * 1024 ** 3;

/**
 * Frames RIFE produces. It interpolates between pairs, so the last frame has
 * no successor to blend with and only the gaps multiply.
 */
export function interpolatedFrameCount(sourceFrames: number, multiplier: number): number {
  if (sourceFrames <= 1) return sourceFrames;
  return (sourceFrames - 1) * multiplier + 1;
}

/** Playback rate after interpolation. Duration is deliberately unchanged. */
export function interpolatedFps(sourceFps: number, multiplier: number): number {
  return sourceFps * multiplier;
}

/**
 * Rough peak system memory for the pass: the interpolated batch lives on the
 * CPU as one float32 RGB tensor. Deliberately ignores the model and decode
 * buffers, so it under-reports rather than crying wolf.
 */
export function estimatedPeakBytes(frames: number, width: number, height: number): number {
  return frames * width * height * 3 * 4;
}

export function formatGigabytes(bytes: number): string {
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}
