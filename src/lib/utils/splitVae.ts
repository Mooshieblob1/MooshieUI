/**
 * Whether a split model can keep the VAE the user picked.
 *
 * A split model (UNETLoader + CLIPLoader + VAELoader) gets a recommended VAE
 * from the backend (`recommended_vae_from_available` in commands/api.rs). That
 * recommendation only has to replace the current pick when the pick is empty,
 * gone from disk, or decodes a different latent format. A decoder-only
 * fine-tune of the stock VAE (a sharpened Qwen Image VAE for Anima / Krea 2,
 * say) shares the latent format and must survive model reloads and reconnects.
 */
import type { ModelFamily } from "./modelFamily.js";

/** Anima, Qwen Image and Krea 2 decode with the 16-channel Qwen Image VAE (a Wan 2.1 VAE fine-tune). */
const QWEN_LATENT_FAMILIES: ReadonlySet<ModelFamily> = new Set([
  "anima",
  "qwen",
  "qwen_edit",
  "qwen_edit_plus",
  "wan",
  "krea2",
]);

/** Families that decode with the 32-channel Flux 2 VAE. */
const FLUX2_LATENT_FAMILIES: ReadonlySet<ModelFamily> = new Set([
  "ideogram4",
  "flux2d",
  "flux2klein9b",
  "flux2klein9bbase",
  "flux2klein4b",
  "flux2klein4bbase",
  "flux3klein",
  "flux3kleinbase",
  "mossimage",
  "mossimagebase",
]);

/** Families that decode with the 16-channel Flux 1 VAE. */
const FLUX1_LATENT_FAMILIES: ReadonlySet<ModelFamily> = new Set([
  "zib",
  "zit",
  "flux",
  "flux1d",
  "flux1s",
  "flux1krea",
  "flux1kontext",
  "chroma",
]);

/** Flux 1 VAE (`ae.safetensors` or any non-Flux-2 `flux` name). Mirrors `find_flux1_vae` in api.rs. */
export function isFlux1VaeName(filename: string): boolean {
  const lower = filename.toLowerCase();
  const basename = lower.split(/[\\/]/).pop() ?? lower;
  return basename === "ae.safetensors" || (lower.includes("flux") && !lower.includes("flux2") && !lower.includes("flux.2"));
}

/**
 * True when the VAE file name fits the family's latent format, false when it
 * plainly does not, null when the family has no known VAE rule. Uses the same
 * name markers the backend recommends with.
 */
export function splitVaeFitsFamily(vae: string, family: ModelFamily | null | undefined): boolean | null {
  if (!family) return null;
  const lower = vae.toLowerCase();
  if (QWEN_LATENT_FAMILIES.has(family)) {
    // Wan 2.1's VAE shares the latent format; Wan 2.2's 48-channel VAE does not.
    return lower.includes("qwen") || /wan[-_ ]?2[._]1/.test(lower);
  }
  if (FLUX2_LATENT_FAMILIES.has(family)) return /flux2[-_]vae/.test(lower);
  if (FLUX1_LATENT_FAMILIES.has(family)) return isFlux1VaeName(vae);
  return null;
}

function samePath(a: string, b: string): boolean {
  return a.replace(/\\/g, "/") === b.replace(/\\/g, "/");
}

/**
 * Whether the current VAE pick should stay instead of the recommendation.
 * An empty installed list means the inventory has not loaded yet, which is no
 * evidence the file is gone.
 */
export function shouldKeepSplitVae(
  current: string,
  family: ModelFamily | null | undefined,
  installedVaes: readonly string[],
): boolean {
  const pick = current.trim();
  if (!pick) return false;
  if (installedVaes.length > 0 && !installedVaes.some((vae) => samePath(vae, pick))) return false;
  return splitVaeFitsFamily(pick, family) !== false;
}
