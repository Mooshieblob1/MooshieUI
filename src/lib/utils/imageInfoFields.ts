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
 * `name:model:clip, name:model:clip`; SwarmUI writes JSON arrays; prompt-style
 * `<lora:name:0.8>` and bare names are read too.
 */
export function parseLoraField(value: string): LoraInfo[] {
  const trimmed = value.trim();
  if (trimmed.startsWith("[")) {
    try {
      const parsed = JSON.parse(trimmed);
      if (Array.isArray(parsed)) {
        const out: LoraInfo[] = [];
        for (const item of parsed) {
          if (typeof item === "string") {
            const name = item.trim();
            if (name) out.push({ name, model: null, clip: null });
          } else if (item && typeof item === "object") {
            const rec = item as Record<string, unknown>;
            const name = typeof rec.name === "string" ? rec.name.trim() : "";
            if (!name) continue;
            const rawModel = rec.strength_model ?? rec.model;
            const rawClip = rec.strength_clip ?? rec.clip;
            const modelNum = rawModel !== undefined && rawModel !== null ? Number(rawModel) : NaN;
            const clipNum = rawClip !== undefined && rawClip !== null ? Number(rawClip) : NaN;
            out.push({
              name,
              model: Number.isFinite(modelNum) ? modelNum : null,
              clip: Number.isFinite(clipNum) ? clipNum : null,
            });
          }
        }
        return out;
      }
    } catch {
      // Fall through to comma-separated parsing if not valid JSON.
    }
  }

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

const MODEL_EXT_RE = /\.(safetensors|ckpt|pt|pth|bin|gguf)$/i;

function normalizeModelPath(name: string): string {
  return name.trim().replace(/\\/g, "/").toLowerCase();
}

function stripModelExt(name: string): string {
  return name.replace(MODEL_EXT_RE, "");
}

/**
 * Resolve an imported checkpoint or LoRA name (often extensionless or with
 * forward slashes from SwarmUI metadata) against the installed file list.
 * Falls back to `importedName` when no installed match is found.
 */
export function matchInstalledModel(
  importedName: string,
  installed: readonly string[],
): string {
  const trimmed = importedName.trim();
  if (!trimmed || installed.length === 0) return importedName;
  if (installed.includes(trimmed)) return trimmed;

  const targetNorm = normalizeModelPath(trimmed);
  for (const candidate of installed) {
    if (normalizeModelPath(candidate) === targetNorm) return candidate;
  }

  const targetStem = stripModelExt(targetNorm);
  for (const candidate of installed) {
    if (stripModelExt(normalizeModelPath(candidate)) === targetStem) return candidate;
  }

  const targetBase = targetStem.split("/").pop() ?? targetStem;
  const baseMatches = installed.filter((candidate) => {
    const stem = stripModelExt(normalizeModelPath(candidate));
    const base = stem.split("/").pop() ?? stem;
    return base === targetBase;
  });
  if (baseMatches.length === 1) return baseMatches[0];

  return importedName;
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
