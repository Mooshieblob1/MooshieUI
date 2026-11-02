/**
 * CivitAI serves mature content on civitai.red; civitai.com only shows SFW
 * pages, so a mature model opened there lands on a blocked page instead of
 * redirecting. Model page links pick the host from the model's rating.
 */
const SFW_ORIGIN = "https://civitai.com";
const MATURE_ORIGIN = "https://civitai.red";

/** CivitAI `nsfwLevel` is a bitmask: 1 PG, 2 PG-13, 4 R, 8 X, 16 XXX. */
const MATURE_LEVEL_BITS = 4 | 8 | 16;

export function isMatureCivitaiModel(model: {
  nsfw?: boolean | null;
  nsfwLevel?: number | null;
}): boolean {
  if (model.nsfw === true) return true;
  const level = model.nsfwLevel;
  return typeof level === "number" && (level & MATURE_LEVEL_BITS) !== 0;
}

/** Public page for a CivitAI model, on civitai.red when the model is mature. */
export function civitaiModelPageUrl(modelId: number, mature: boolean | null | undefined): string {
  return `${mature ? MATURE_ORIGIN : SFW_ORIGIN}/models/${modelId}`;
}

/** True for civitai.com, civitai.red and their subdomains (not look-alikes). */
export function isCivitaiPageHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return ["civitai.com", "civitai.red"].some((root) => host === root || host.endsWith(`.${root}`));
}
