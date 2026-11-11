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
  name: string;
  /** Written for NovelAI V4+, where a character prompt counts heads as `girl`. */
  prompt: string;
  negative: string;
  memory: { name: string; content: string }[];
}

/** NovelAI first, then the local architectures prompted with Danbooru tags. */
export const SAMPLE_ARCHITECTURES = ["novelai", "illustrious", "sdxl", "pony", "anima", "sd15"] as const;

export const YUE: SampleDefinition = {
  key: "yue",
  name: "Yue",
  prompt:
    "girl, yue, long white hair, straight hair, blunt bangs, blue eyes, pointed ears, fangs, chibi, small, tiny, oversized white hoodie, barefoot",
  negative:
    "twintails, high twintails, low twintails, ponytail, pigtails, braid, twin braids, hair bun, wavy hair, curly hair, drilled hair, hair bobbles, hair ribbon, hair bow, skinny, thin waist, flat stomach, athletic, muscular, abs, large breasts, huge breasts, crew neck, short sleeves, eye highlight, catchlight, detailed eyes, shaded eyes, fake animal ears, tail, braid",
  memory: [{ name: "personality.md", content: "- Usual expression: smiling, :D, open mouth smile" }],
};

export const SAMPLES: readonly SampleDefinition[] = [YUE];

/**
 * The prompt as `architecture` reads it. Local tag models learnt Danbooru's
 * `1girl`; NovelAI's character prompts take `girl`.
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
): { characters: (T | SampleCard)[]; seen: string[]; changed: boolean } {
  const next: (T | SampleCard)[] = [...characters];
  const nextSeen = [...seen];
  let changed = false;
  for (const sample of SAMPLES) {
    if (seen.includes(sample.key)) continue;
    nextSeen.push(sample.key);
    changed = true;
    const picture = pictures[sample.key] ?? { thumbnail: null, reference: null };
    for (const architecture of SAMPLE_ARCHITECTURES) {
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
