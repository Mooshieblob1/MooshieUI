/**
 * Sample characters every user starts with, so the Characters tab shows what
 * a card looks like before they save one of their own.
 *
 * Each sample is added once per user: its id goes into the `seen` list that
 * is saved and synced with the cards, so deleting a sample keeps it deleted
 * on every device. A sample gets one card in every tag-prompted architecture,
 * with a fixed id per architecture, so two devices that add it on their own
 * still end up with one card each when their copies merge.
 *
 * Pure and store-free (the pictures are passed in) so it can be tested on its
 * own; `savedCharacters` calls it on load and when synced prefs arrive.
 */

/** The fields of a saved card a sample fills in (see `SavedCharacter`). */
export interface SampleCard {
  id: string;
  name: string;
  prompt: string;
  negative: string;
  architecture: string;
  thumbnail: string | null;
  reference: string | null;
  parentId: null;
  memory: { id: string; name: string; content: string; updatedAt: number }[];
  createdAt: number;
  updatedAt: number;
}

/** A card's picture: the thumbnail shown, and the larger copy for references. */
export interface SamplePictures {
  thumbnail: string | null;
  reference: string | null;
}

interface SampleDefinition {
  key: string;
  pictureKey?: string;
  name: string;
  /** Written for NovelAI V4+ (tag style) or prose-prompt models (natural style). */
  prompt: string;
  negative: string;
  architectures: readonly string[];
  memory: { name: string; content: string }[];
}

/** NovelAI first, then the local architectures prompted with Danbooru tags. */
export const SAMPLE_ARCHITECTURES = ["novelai", "illustrious", "sdxl", "pony", "anima", "sd15"] as const;

/** Local architectures prompted with natural-language prose. */
export const SAMPLE_NATURAL_ARCHITECTURES = [
  "flux1d",
  "flux1s",
  "flux1krea",
  "flux",
  "flux1kontext",
  "flux2d",
  "flux3klein",
  "flux3kleinbase",
  "flux2klein9b",
  "flux2klein9bbase",
  "flux2klein4b",
  "flux2klein4bbase",
  "krea2",
  "qwen",
  "qwenimage21",
  "qwenimage21turbo",
  "mossimage",
  "mossimagebase",
  "mingimage",
  "mingimagelayer",
  "minimaxh3image",
  "minimaxh3imagepdmd",
  "zib",
  "zit",
  "chroma",
  "sd3",
  "sd3turbo",
  "kolors",
  "pixart",
  "pixartalpha",
  "cascade",
  "ideogram4",
  "auraflow",
  "hunyuandit",
  "mugen",
  "nanosaur",
  "wan",
  "wan22a14b",
  "wan22ti2v",
] as const;

export const YUE: SampleDefinition = {
  key: "yue",
  name: "Yue",
  prompt:
    "girl, yue, long white hair, straight hair, blunt bangs, blue eyes, pointed ears, fangs, chibi, small, tiny, oversized white hoodie, barefoot",
  negative:
    "twintails, high twintails, low twintails, ponytail, pigtails, braid, twin braids, hair bun, wavy hair, curly hair, drilled hair, hair bobbles, hair ribbon, hair bow, skinny, thin waist, flat stomach, athletic, muscular, abs, large breasts, huge breasts, crew neck, short sleeves, eye highlight, catchlight, detailed eyes, shaded eyes, fake animal ears, tail, braid",
  architectures: SAMPLE_ARCHITECTURES,
  memory: [{ name: "personality.md", content: "- Usual expression: smiling, :D, open mouth smile" }],
};

export const YUE_NATURAL: SampleDefinition = {
  key: "yue-natural",
  pictureKey: "yue",
  name: "Yue",
  prompt:
    "A tiny chibi anime girl with long straight white hair and blunt bangs, simple blue eyes without highlights, pointed ears and small fangs, wearing an oversized white hoodie with bare feet.",
  negative:
    "twintails, ponytail, braids, wavy or curly hair, tall or athletic proportions, large chest, detailed shaded eyes with catchlights, fake animal ears, tail",
  architectures: SAMPLE_NATURAL_ARCHITECTURES,
  memory: [{ name: "personality.md", content: "- Usual expression: smiling, :D, open mouth smile" }],
};

export const SAMPLES: readonly SampleDefinition[] = [YUE, YUE_NATURAL];

/**
 * The prompt as `architecture` reads it. Local tag models learnt Danbooru's
 * `1girl`; NovelAI's character prompts take `girl`; natural-language prompts
 * stay untouched.
 */
export function samplePrompt(prompt: string, architecture: string): string {
  if (architecture === "novelai") return prompt;
  return prompt.replace(/^girl(?=,|$)/, "1girl");
}

/** The fixed id of a sample's card in `architecture`. */
export function sampleCardId(key: string, architecture: string): string {
  return `sample-${key}-${architecture}`;
}

/**
 * `characters` with every sample not in `seen` added, and the new `seen`
 * list. An architecture that already holds the sample's card, or a card of
 * the same name, gets no second one. `changed` is false when nothing was
 * added and `seen` stays as it was.
 */
export function withSamples<T extends { id: string; name: string; architecture: string }>(
  characters: readonly T[],
  seen: readonly string[],
  pictures: Record<string, SamplePictures>,
  now: number,
  extraNaturalArchitectures: readonly string[] = [],
): { characters: (T | SampleCard)[]; seen: string[]; changed: boolean } {
  const next: (T | SampleCard)[] = [...characters];
  const nextSeen = [...seen];
  let changed = false;
  for (const sample of SAMPLES) {
    if (seen.includes(sample.key)) continue;
    nextSeen.push(sample.key);
    changed = true;
    const picture =
      pictures[sample.key] ??
      (sample.pictureKey ? pictures[sample.pictureKey] : undefined) ??
      { thumbnail: null, reference: null };
    const architectures =
      sample.key === YUE_NATURAL.key && extraNaturalArchitectures.length > 0
        ? [...new Set([...sample.architectures, ...extraNaturalArchitectures])]
        : sample.architectures;
    for (const architecture of architectures) {
      const id = sampleCardId(sample.key, architecture);
      const name = sample.name.toLowerCase();
      if (next.some((c) => c.id === id || (c.architecture === architecture && c.name.trim().toLowerCase() === name))) {
        continue;
      }
      next.push({
        id,
        name: sample.name,
        prompt: samplePrompt(sample.prompt, architecture),
        negative: sample.negative,
        architecture,
        thumbnail: picture.thumbnail,
        reference: picture.reference,
        parentId: null,
        memory: sample.memory.map((file) => ({
          id: `${id}-${file.name}`,
          name: file.name,
          content: file.content,
          updatedAt: now,
        })),
        createdAt: now,
        updatedAt: now,
      });
    }
  }
  return { characters: next, seen: nextSeen, changed };
}

/** The saved `seen` list, or empty when it is missing or malformed. */
export function sanitizeSeen(raw: unknown): string[] {
  return Array.isArray(raw) ? raw.filter((k): k is string => typeof k === "string") : [];
}
