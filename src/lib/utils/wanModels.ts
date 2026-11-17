/**
 * Wan 2.x as an image model. Mirrors `WanVariant` in
 * src-tauri/src/templates/wan.rs: keep the filename rules in sync.
 */

export type WanVariant = "wan21" | "ti2v5b" | "a14bHigh" | "a14bLow" | "notTextToImage";

/** `GenerationParams.architecture_files` slot for the Wan 2.2 A14B low-noise expert. */
export const WAN_LOW_NOISE_SLOT = "low_noise_model";

/** umT5-XXL, Wan's text encoder. Mirrors `TEXT_ENCODER_MARKERS` in wan.rs. */
export function isWanEncoder(filename: string | null | undefined): boolean {
  return !!filename && filename.toLowerCase().includes("umt5");
}

const WAN21_VAE = /wan[_]?2[._]?1[_]vae/;
const WAN22_VAE = /wan[_]?2[._]?2[_]vae/;

/** The 16-channel Wan 2.1 VAE, or the Qwen Image VAE fine-tuned from it. */
export function isWan21Vae(filename: string): boolean {
  const lower = filename.toLowerCase();
  return WAN21_VAE.test(lower) || lower.includes("qwen");
}

/** The 48-channel Wan 2.2 VAE, used only by TI2V 5B. */
export function isWan22Vae(filename: string): boolean {
  return WAN22_VAE.test(filename.toLowerCase());
}

function nameTokens(filename: string): string[] {
  const lower = filename.toLowerCase();
  const base = lower.split(/[\\/]/).pop() ?? lower;
  return base.split(/[^a-z0-9]+/).filter(Boolean);
}

function namesExpert(tokens: string[], level: "high" | "low"): boolean {
  return tokens.some((token) => token === level || token.includes(`${level}noise`));
}

const NOT_TEXT_TO_IMAGE = ["i2v", "flf2v", "s2v", "vace", "animate", "camera", "fun", "chrono"];

export function wanVariant(filename: string | null | undefined): WanVariant {
  const tokens = nameTokens(filename ?? "");
  if (tokens.some((token) => NOT_TEXT_TO_IMAGE.includes(token) || token.startsWith("chronoedit"))) {
    return "notTextToImage";
  }
  if (tokens.includes("ti2v") || tokens.includes("5b")) return "ti2v5b";
  if (namesExpert(tokens, "high")) return "a14bHigh";
  if (namesExpert(tokens, "low")) return "a14bLow";
  return "wan21";
}

function stripDir(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/**
 * The low-noise expert that finishes a Wan 2.2 A14B high-noise model: the
 * same filename with `high` swapped for `low`, else the only installed
 * low-noise model. Null when nothing fits.
 */
export function findWanLowNoisePartner(
  highNoiseModel: string | null | undefined,
  installed: readonly string[],
): string | null {
  if (!highNoiseModel || wanVariant(highNoiseModel) !== "a14bHigh") return null;
  const candidates = installed.filter((name) => wanVariant(name) === "a14bLow");
  if (candidates.length === 0) return null;

  const swapped = stripDir(highNoiseModel).toLowerCase().replace(/high/g, "low");
  const exact = candidates.find((name) => stripDir(name).toLowerCase() === swapped);
  if (exact) return exact;
  return candidates.length === 1 ? candidates[0] : null;
}

/** Sampler defaults per variant, from Comfy-Org's Wan templates. */
export function wanSamplingPreset(variant: WanVariant): {
  steps: number;
  cfg: number;
  samplerName: string;
  scheduler: string;
} {
  if (variant === "a14bHigh" || variant === "a14bLow") {
    return { steps: 20, cfg: 3.5, samplerName: "euler", scheduler: "simple" };
  }
  if (variant === "ti2v5b") {
    return { steps: 20, cfg: 5, samplerName: "uni_pc", scheduler: "simple" };
  }
  return { steps: 30, cfg: 6, samplerName: "uni_pc", scheduler: "simple" };
}
