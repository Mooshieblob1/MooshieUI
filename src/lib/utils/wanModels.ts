/**
 * Wan 2.x as an image model. Mirrors `WanVariant` in
 * src-tauri/src/architectures/wan.rs: keep the filename rules in sync. Families,
 * encoder, VAE and sampling defaults come from that file's registry spec.
 */

export type WanVariant = "wan21" | "ti2v5b" | "a14bHigh" | "a14bLow" | "notTextToImage";

/** `GenerationParams.architecture_files` slot for the Wan 2.2 A14B low-noise expert. */
export const WAN_LOW_NOISE_SLOT = "low_noise_model";

function nameTokens(filename: string): string[] {
  const lower = filename.toLowerCase();
  const base = lower.split(/[\\/]/).pop() ?? lower;
  return base.split(/[^a-z0-9]+/).filter(Boolean);
}

function namesExpert(tokens: string[], level: "high" | "low"): boolean {
  return tokens.some((token) => token === level || token.includes(`${level}noise`));
}

const NOT_TEXT_TO_IMAGE = ["i2v", "flf2v", "s2v", "vace", "animate", "camera", "fun", "chrono", "magref"];

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
