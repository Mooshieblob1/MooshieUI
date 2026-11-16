/**
 * Which image metadata fields the lightbox's Image Info panel shows, and in
 * what order.
 *
 * Metadata reaches the panel as a plain object built from a Rust HashMap, so
 * its key order changes from image to image. The panel therefore sorts the
 * keys itself: the user's own order first, then `DEFAULT_FIELD_ORDER`, then
 * anything else alphabetically. Hidden fields are left out.
 */

export const DEFAULT_FIELD_ORDER = [
  "positive_prompt",
  "negative_prompt",
  "model",
  "loras",
  "vae",
  "mode",
  "size",
  "seed",
  "steps",
  "cfg",
  "sampler",
  "scheduler",
  "denoise",
  "upscale_model",
  "upscale_scale",
  "upscale_denoise",
  "source",
  "date",
  "generation_time",
] as const;

/** Fields shown as a full-width text block rather than a label/value row. */
export const BLOCK_FIELDS = new Set(["positive_prompt", "negative_prompt"]);

export interface ImageInfoPrefs {
  /** Keys in the user's order; keys not listed fall back to the default order. */
  order: string[];
  hidden: string[];
}

export const EMPTY_PREFS: ImageInfoPrefs = { order: [], hidden: [] };

export function sanitizePrefs(raw: unknown): ImageInfoPrefs {
  const obj = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const strings = (v: unknown) =>
    Array.isArray(v) ? [...new Set(v.filter((s): s is string => typeof s === "string" && s !== ""))] : [];
  return { order: strings(obj.order), hidden: strings(obj.hidden) };
}

/** All of `keys`, in display order (hidden ones included). */
export function orderFields(keys: Iterable<string>, prefs: ImageInfoPrefs): string[] {
  const rank = new Map<string, number>();
  for (const key of prefs.order) if (!rank.has(key)) rank.set(key, rank.size);
  for (const key of DEFAULT_FIELD_ORDER) if (!rank.has(key)) rank.set(key, rank.size);
  return [...new Set(keys)].sort((a, b) => {
    const ra = rank.get(a);
    const rb = rank.get(b);
    if (ra !== undefined && rb !== undefined) return ra - rb;
    if (ra !== undefined) return -1;
    if (rb !== undefined) return 1;
    return a < b ? -1 : a > b ? 1 : 0;
  });
}

/** The keys of `metadata` to show, in order, skipping hidden and empty ones. */
export function visibleFields(metadata: Record<string, string>, prefs: ImageInfoPrefs): string[] {
  const hidden = new Set(prefs.hidden);
  return orderFields(Object.keys(metadata), prefs).filter(
    (key) => !hidden.has(key) && (metadata[key] ?? "").trim() !== "",
  );
}

/**
 * Move `key` one step up (-1) or down (+1) within `list` (the order the
 * settings list shows), and return the new prefs with that whole list as the
 * user's order.
 */
export function moveField(list: string[], key: string, delta: -1 | 1, prefs: ImageInfoPrefs): ImageInfoPrefs {
  const from = list.indexOf(key);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= list.length) return prefs;
  const next = [...list];
  [next[from], next[to]] = [next[to], next[from]];
  // Keep the order of keys this image does not have, after the ones it does.
  const rest = prefs.order.filter((k) => !next.includes(k));
  return { ...prefs, order: [...next, ...rest] };
}

export function toggleField(key: string, prefs: ImageInfoPrefs): ImageInfoPrefs {
  const hidden = prefs.hidden.includes(key)
    ? prefs.hidden.filter((k) => k !== key)
    : [...prefs.hidden, key];
  return { ...prefs, hidden };
}

export interface LoraInfo {
  name: string;
  /** Model strength, or null when the entry had none. */
  model: number | null;
  /** CLIP strength when it was written separately. */
  clip: number | null;
}

const NUM = String.raw`[-+]?(?:\d+\.?\d*|\.\d+)`;
const ENTRY = new RegExp(`^(.*?)(?::(${NUM}))?(?::(${NUM}))?$`);

/**
 * Split a `loras` metadata value into one entry per LoRA. MooshieUI writes
 * `name:model:clip, name:model:clip`; prompt-style `<lora:name:0.8>` and bare
 * names are read too.
 */
export function parseLoraField(value: string): LoraInfo[] {
  const out: LoraInfo[] = [];
  for (const raw of value.split(/,(?![^<]*>)/)) {
    let entry = raw.trim();
    if (!entry) continue;
    const tag = entry.match(/^<lora:(.*)>$/i);
    if (tag) entry = tag[1].trim();
    const m = entry.match(ENTRY);
    const name = (m?.[1] ?? entry).trim();
    if (!name) continue;
    const model = m?.[2] !== undefined ? Number(m[2]) : null;
    const clip = m?.[3] !== undefined ? Number(m[3]) : null;
    out.push({ name, model, clip });
  }
  return out;
}

/** File name without folders or the model extension, for display. */
export function loraDisplayName(name: string): string {
  const base = name.split(/[\\/]/).pop() ?? name;
  return base.replace(/\.(safetensors|ckpt|pt|pth|bin)$/i, "");
}

/** `0.80`, or `0.80 / 0.60` when CLIP differs from model strength. */
export function formatLoraWeight(lora: LoraInfo): string | null {
  if (lora.model === null) return null;
  const model = lora.model.toFixed(2);
  if (lora.clip === null || lora.clip === lora.model) return model;
  return `${model} / ${lora.clip.toFixed(2)}`;
}
