/**
 * NovelAI PNG metadata reader, for browser mode.
 *
 * Mirrors `src-tauri/src/novelai/metadata.rs`. Desktop reads NovelAI images
 * through that Rust reader; browser mode parses the file client-side and never
 * reaches it, so the same parse lives here. The two are kept in step by hand,
 * and the Rust module carries the fuller notes on what NovelAI writes.
 *
 * NovelAI spreads its metadata across chunks of its own (`Title`,
 * `Description`, `Software`, `Source`, `Comment`) instead of the single
 * `parameters` chunk A1111 and this app write, with `Comment` holding the real
 * payload as JSON.
 */

/**
 * Translate NovelAI weight syntax back into ComfyUI syntax.
 *
 * `1.1::tag::` becomes `(tag:1.10)`, `{tag}` becomes `(tag:1.05)`, `[tag]`
 * becomes `(tag:0.95)`, innermost first so nesting compounds the way NovelAI
 * applies it. An escaped bracket is a literal character and is left alone.
 *
 * Deliberately duplicated rather than shared: the generation store has this
 * translation for prompts a user types in, and the Rust reader has a third copy
 * for the desktop path. Each layer owns its own, because a leaf util reaching
 * into a store would invert the dependency direction.
 */
function naiWeightsToComfy(prompt: string): string {
  let out = prompt.replace(
    /(\d+\.?\d*)::([^:]+)::/g,
    (_match, weight: string, text: string) => `(${text.trim()}:${parseFloat(weight).toFixed(2)})`,
  );

  let previous: string;
  do {
    previous = out;
    out = out.replace(/\{([^{}]+)\}/g, (_match, inner: string) => `(${inner}:1.05)`);
  } while (out !== previous);

  do {
    previous = out;
    out = out.replace(/(?<!\\)\[([^[\]]+)\]/g, (_match, inner: string) => `(${inner}:0.95)`);
  } while (out !== previous);

  return out;
}

/** Does this set of PNG text chunks come from NovelAI? */
export function isNovelAiChunks(chunks: Record<string, string>): boolean {
  const saysNovelAi = (key: string) => (chunks[key] ?? "").toLowerCase().includes("novelai");
  return saysNovelAi("Software") || saysNovelAi("Source");
}

/** Render a JSON scalar as the plain string the flat metadata map holds. */
function scalar(value: unknown): string | undefined {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return undefined;
}

/** Pull `caption.base_caption` out of a V4 structured prompt block. */
function baseCaption(block: unknown): string | undefined {
  const caption = (block as any)?.caption?.base_caption;
  return typeof caption === "string" ? caption : undefined;
}

/**
 * Map NovelAI's `Source` string onto the API model id the app selects with.
 *
 * `Source` carries a version and a weights checksum, not a model id, and says
 * nothing about Full versus Curated, so every version resolves to its Full id.
 * The longest version marker has to be tested first, or "V4.5" matches "V4".
 */
function modelIdFromSource(source: string): string | undefined {
  const upper = source.toUpperCase();
  const versions: [string, string][] = [
    ["V4.5", "nai-diffusion-4-5-full"],
    ["V5", "nai-diffusion-5-full"],
    ["V4", "nai-diffusion-4-full"],
  ];
  for (const [marker, model] of versions) {
    if (upper.includes(marker)) return model;
  }
  return undefined;
}

/** Rebuild the app's character list from the two parallel V4 prompt blocks. */
function parseCharacters(comment: any): string | undefined {
  const positives = comment?.v4_prompt?.caption?.char_captions;
  if (!Array.isArray(positives) || positives.length === 0) return undefined;
  const negatives = comment?.v4_negative_prompt?.caption?.char_captions;
  const negativeList: any[] = Array.isArray(negatives) ? negatives : [];

  const characters = positives.map((entry: any, index: number) => {
    const center = entry?.centers?.[0];
    const negative = negativeList[index]?.char_caption;
    return {
      prompt: naiWeightsToComfy(typeof entry?.char_caption === "string" ? entry.char_caption : ""),
      negative_prompt: naiWeightsToComfy(typeof negative === "string" ? negative : ""),
      center: {
        x: typeof center?.x === "number" ? center.x : 0.5,
        y: typeof center?.y === "number" ? center.y : 0.5,
      },
      enabled: true,
    };
  });
  return JSON.stringify(characters);
}

/**
 * Parse NovelAI PNG text chunks into the app's flat metadata map.
 *
 * Returns null for anything that is not a NovelAI image, so this can sit as a
 * fallback after every other reader has declined.
 */
export function parseNovelAiChunks(chunks: Record<string, string>): Record<string, string> | null {
  if (!isNovelAiChunks(chunks)) return null;

  let comment: any = null;
  try {
    comment = chunks.Comment ? JSON.parse(chunks.Comment) : null;
  } catch {
    // A `Comment` we cannot read still leaves the plain chunks worth reporting.
    comment = null;
  }
  if (typeof comment !== "object" || comment === null) comment = null;

  const params: Record<string, string> = { mooshie_backend: "novelai" };

  // The V4 structured block is authoritative where present: `prompt` is a
  // flattened rendering of it that folds every character prompt into one line.
  const positive =
    baseCaption(comment?.v4_prompt) ?? scalar(comment?.prompt) ?? chunks.Description;
  if (positive !== undefined) params.positive_prompt = naiWeightsToComfy(positive);

  const negative = baseCaption(comment?.v4_negative_prompt) ?? scalar(comment?.uc);
  if (negative !== undefined) params.negative_prompt = naiWeightsToComfy(negative);

  if (comment) {
    // NovelAI's noise schedule occupies the same slot in the panel as a local
    // scheduler does: one dropdown, one value recorded per image.
    const direct: [string, string][] = [
      ["steps", "steps"],
      ["scale", "cfg"],
      ["seed", "seed"],
      ["sampler", "sampler"],
      ["noise_schedule", "scheduler"],
      ["cfg_rescale", "mooshie_novelai_cfg_rescale"],
      ["uncond_scale", "mooshie_novelai_uncond_scale"],
      ["dynamic_thresholding", "mooshie_novelai_dynamic_thresholding"],
      // Which endpoint made the image. An `Img2ImgRequest` was seeded by a
      // source image nothing in the metadata carries, so the settings here
      // cannot reproduce it and the import dialog says so.
      ["request_type", "mooshie_novelai_request_type"],
    ];
    for (const [naiKey, internal] of direct) {
      const value = scalar(comment[naiKey]);
      if (value !== undefined) params[internal] = value;
    }

    const width = scalar(comment.width);
    const height = scalar(comment.height);
    if (width !== undefined && height !== undefined) params.size = `${width}x${height}`;

    // Variety+ is not stored as a boolean: NovelAI records the sigma above
    // which it starts skipping CFG, and the feature is off when that is null.
    if (comment.skip_cfg_above_sigma !== null && comment.skip_cfg_above_sigma !== undefined) {
      params.mooshie_novelai_variety_plus = "true";
    }

    if (typeof comment.v4_prompt?.use_coords === "boolean") {
      params.mooshie_novelai_use_coords = String(comment.v4_prompt.use_coords);
    }

    const characters = parseCharacters(comment);
    if (characters !== undefined) params.mooshie_novelai_characters = characters;
  }

  // The captured prompt and UC already have the quality tags and the preset
  // text folded in, so re-enabling either toggle would append a second copy.
  params.mooshie_novelai_quality_toggle = "false";
  params.mooshie_novelai_uc_preset = "0";

  if (chunks.Source) {
    params.mooshie_novelai_source = chunks.Source;
    const model = modelIdFromSource(chunks.Source);
    // An unrecognised version leaves `model` unset, so the panel keeps the
    // checkpoint already selected rather than being pointed at nothing.
    if (model) params.model = model;
  }
  if (chunks["Generation time"]) {
    params.mooshie_novelai_generation_time = chunks["Generation time"];
  }

  return params;
}

/**
 * Parse a NovelAI stealth-alpha payload.
 *
 * NovelAI hides the same chunk set in the alpha LSBs as well as writing it in
 * the clear, so an image that lost its text chunks to a re-encode that kept the
 * pixels intact can still be read. The payload is a flat JSON object keyed by
 * the chunk names, which is why it hands straight off to the chunk parser.
 */
export function parseNovelAiStealthJson(text: string): Record<string, string> | null {
  let root: unknown;
  try {
    root = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof root !== "object" || root === null || Array.isArray(root)) return null;

  const chunks: Record<string, string> = {};
  for (const [key, value] of Object.entries(root)) {
    const asString = scalar(value);
    if (asString !== undefined) chunks[key] = asString;
  }
  return parseNovelAiChunks(chunks);
}
