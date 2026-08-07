/**
 * Export math for the video export popover.
 *
 * This mirrors `src-tauri/src/commands/video_export.rs` on purpose: Rust is the
 * canonical implementation and the only place with tests, but the popover needs
 * these values synchronously as the user drags a slider, and an IPC round-trip
 * per frame of interaction is not viable. Each function names its Rust
 * counterpart. Change one, change the other.
 */

/** Rust: `MIN_OFFERED_FPS` */
export const MIN_OFFERED_FPS = 6;
/** Rust: `AUTO_SEAM_THRESHOLD` */
export const AUTO_SEAM_THRESHOLD = 2.0;
/** Rust: `DEFAULT_CROSSFADE_FRAMES` */
export const DEFAULT_CROSSFADE_FRAMES = 4;

export type ExportFormat = "avif" | "webp" | "gif";
export type LoopMode = "auto" | "none" | "trim" | "crossfade" | "pingpong";
export type SizeTarget = "discord" | "nitro" | "none";

export interface ExportPreset {
  /** Locale key suffix under `video.export.` */
  id: string;
  /**
   * Target frame rate, or `"source"` to mean "whatever this clip runs at".
   * Resolved through `snapFps` against the offered list.
   */
  fpsTarget: number | "source";
  /** Requested width; clamped down to the source width by `outputDimensions`. */
  width: number;
  /** AV1 quality 0-100 (AVIF), libwebp quality 0-100 (WebP), or palette colour count 2-256 (GIF). */
  quality: number;
}

/**
 * AVIF is the recommended default: roughly a quarter of the WebP byte count at comparable
 * quality, and on ordinary video-like content it also encodes faster. Both advantages invert
 * on incompressible noise, so they are content-dependent, not unconditional.
 * The trade-off is reach - Discord, Slack, Teams and Signal
 * will not animate an AVIF inline, they post it as a file attachment. The popover states that
 * per format rather than hiding it, so WebP stays a first-class choice for sharing.
 *
 * `quality` here is an AV1 quality 0-100, not the libwebp scale. The numbers are lower than the
 * WebP presets on purpose - AVIF at 63 is visually comparable to WebP at 80.
 */
export const AVIF_PRESETS: ExportPreset[] = [
  { id: "preset_smooth", fpsTarget: "source", width: 480, quality: 55 },
  { id: "preset_balanced", fpsTarget: 24, width: 640, quality: 63 },
  { id: "preset_max", fpsTarget: "source", width: 832, quality: 75 },
];

/** Spec Section 2, "Presets, split by format". WebP is the default tab. */
export const WEBP_PRESETS: ExportPreset[] = [
  { id: "preset_smooth", fpsTarget: "source", width: 480, quality: 75 },
  { id: "preset_balanced", fpsTarget: 24, width: 640, quality: 80 },
  { id: "preset_max", fpsTarget: "source", width: 832, quality: 90 },
];

export const GIF_PRESETS: ExportPreset[] = [
  { id: "preset_discord", fpsTarget: 12, width: 480, quality: 128 },
  { id: "preset_balanced", fpsTarget: 16, width: 640, quality: 192 },
  { id: "preset_max", fpsTarget: 24, width: 832, quality: 256 },
];

/** Presets for a format, in the order the popover shows them. */
export function presetsFor(format: ExportFormat): ExportPreset[] {
  if (format === "avif") return AVIF_PRESETS;
  if (format === "webp") return WEBP_PRESETS;
  return GIF_PRESETS;
}

/**
 * Valid range for the overloaded `quality` field.
 *
 * `quality` means three different things by format: an AV1 quality for AVIF, a libwebp quality
 * for WebP, and a palette colour count for GIF. The advanced slider reads its bounds from here
 * instead of hardcoding them.
 */
export function qualityRange(format: ExportFormat): { min: number; max: number } {
  return format === "gif" ? { min: 2, max: 256 } : { min: 0, max: 100 };
}

/**
 * Whether the container honours an explicit repeat count.
 *
 * AVIF accepts a `loop` argument at encode time but neither Pillow nor typical players read it
 * back - animated AVIF loops continuously regardless. The popover disables the repeat-count
 * control for AVIF and shows `video.export.loop_count_unsupported`.
 */
export function supportsLoopCount(format: ExportFormat): boolean {
  return format !== "avif";
}

/** Rust: `divisors` */
function divisors(n: number): number[] {
  if (n <= 0) return [];
  const out: number[] = [];
  for (let d = n; d >= 1; d--) if (n % d === 0) out.push(d);
  return out;
}

/** Rust: `offered_fps` */
export function offeredFps(sourceFps: number): number[] {
  const src = Math.round(sourceFps);
  if (src <= 0) return [MIN_OFFERED_FPS];
  const offered = divisors(src).filter((d) => d >= MIN_OFFERED_FPS);
  return offered.length > 0 ? offered : [src];
}

/** Rust: `snap_fps` */
export function snapFps(target: number, sourceFps: number): number {
  const offered = offeredFps(sourceFps);
  return offered.find((v) => v <= target) ?? offered[offered.length - 1];
}

/** Resolve a preset's fps target (which may be `"source"`) to a real rate. */
export function presetFps(preset: ExportPreset, sourceFps: number): number {
  const target = preset.fpsTarget === "source" ? Math.round(sourceFps) : preset.fpsTarget;
  return snapFps(target, sourceFps);
}

/**
 * Rust: `output_dimensions`
 *
 * Width is snapped DOWN to even first; height is derived from that snapped width
 * and rounded to the NEAREST even number (not down), so the aspect ratio is
 * preserved as faithfully as possible within the 2px grid.
 *
 * Never upscales past the source - except that the encoder floor of 2px can
 * exceed a source narrower than 2 pixels (degenerate input). The Rust side
 * shares this edge case; it is documented here explicitly so the two files do
 * not drift.
 */
export function outputDimensions(
  srcW: number,
  srcH: number,
  requestedW: number
): { width: number; height: number } {
  if (srcW <= 0 || srcH <= 0) return { width: 0, height: 0 };
  // Snap width down to even first; never upscale past the source.
  const w = Math.max(2, Math.min(requestedW, srcW)) & ~1;
  const hExact = (w * srcH) / srcW;
  // Round height to the nearest even number (not down) so the aspect ratio is
  // preserved as faithfully as possible within the 2px grid.
  const h = Math.max(2, Math.round(hExact / 2) * 2);
  return { width: w, height: h };
}

/** Rust: `crossfade_available` */
export function crossfadeAvailable(f: number, n: number): boolean {
  return n > 0 && f > 3 * n;
}

/** Rust: `output_frame_count` */
export function outputFrameCount(mode: LoopMode, f: number, n: number): number {
  switch (mode) {
    case "trim":
      return Math.max(1, f - 1);
    case "crossfade":
      return crossfadeAvailable(f, n) ? f - n : f;
    case "pingpong":
      return f < 3 ? f : 2 * f - 2;
    default:
      return f;
  }
}

/** Rust: `resolve_auto` */
export function resolveAuto(seamDelta: number): "trim" | "none" {
  return seamDelta < AUTO_SEAM_THRESHOLD ? "trim" : "none";
}

/** Rust: `over_size_limit` */
export function overSizeLimit(bytes: number, target: SizeTarget): boolean {
  if (target === "discord") return bytes > 10 * 1024 * 1024;
  if (target === "nitro") return bytes > 500 * 1024 * 1024;
  return false;
}

/**
 * Order-of-magnitude size estimate for picking a preset. Not a promise - the
 * real byte count replaces it once the export finishes.
 *
 * `frames` must be the post-loop-mode count: ping-pong nearly doubles it, and
 * the estimate has to say so before the click.
 *
 * The AVIF coefficient (0.015) is 0.06 / 4, measured at roughly a quarter of
 * the WebP byte count on a 48-frame 480x270 sample.
 */
export function estimateBytes(
  format: ExportFormat,
  frames: number,
  width: number,
  height: number
): number {
  const k = format === "avif" ? 0.015 : format === "webp" ? 0.06 : 0.35;
  return Math.round(frames * width * height * k);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
