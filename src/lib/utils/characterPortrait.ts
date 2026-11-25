/**
 * Portraits for saved characters: a front-on, neck-up picture of the
 * character rendered with the selected model, used as the card's picture
 * (and, on NovelAI, as the Precise Reference that Use attaches).
 *
 * The prompt is built here and shown in the portrait dialog, where it can be
 * edited. Chunks, Artist Styles and artist tags the user picks go on top,
 * and the UC those chunks and styles carry goes on the negative prompt. The
 * generation goes through the normal queue with the user's model, sampler and
 * LoRAs, but nothing from the prompt area: no prompt boxes, character boxes,
 * active chunks or styles, references, regions or ControlNet. On NovelAI
 * models with character prompts, the character (look, expression and the
 * card's UC) goes in one character box and the main prompt keeps the count,
 * framing and style, as Use lays a card out.
 *
 * Lives in utils rather than a store because it reaches into `generation`,
 * `styles` and `promptPresets`, and `generation` imports those two. App.svelte
 * hands the finished image back through `applyCharacterPortrait`.
 */
import { createNovelAiCharacter, generation } from "../stores/generation.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { promptPresets } from "../stores/promptPresets.svelte.js";
import {
  architecturePromptStyle,
  savedCharacters,
  type SavedCharacter,
} from "../stores/savedCharacters.svelte.js";
import { fragmentForStyles, negativeForStyles, styles } from "../stores/styles.svelte.js";
import type { NovelAiDirectorReference, OutputImage } from "../types/index.js";
import { userScopedKey } from "./ipc.js";
import { currentImageToCharacterThumbnail } from "./characterThumbnail.js";
import {
  clampCharacterReferenceFidelity,
  clampCharacterReferenceStrength,
  normalizeCharacterReferenceDescription,
} from "./characterPreciseRefLogic.js";
import { classifyGenerationError } from "./generationErrors.js";
import { submitGeneration } from "./generationSubmit.js";
import { NOVELAI_MAX_DIRECTOR_REFERENCES } from "./novelaiModels.js";
import { neckUpProse, neckUpTags } from "./portraitFilter.js";
import { BLANK_FACE_GUARD, splitExpressionTags } from "./portraitExpression.js";
import { splitCountTags } from "./portraitCharacterBox.js";

/** Framing preset when generating a character Precise Reference image. */
export type CharacterRefFraming = "full_body" | "upper_body" | "portrait" | "sheet";

export const FULL_BODY_REF_FRAMING_TAGS =
  "full body, standing, straight-on, facing viewer, looking at viewer, simple background";
export const UPPER_BODY_REF_FRAMING_TAGS =
  "upper body, straight-on, facing viewer, looking at viewer, simple background";
export const UPPER_BODY_REF_AVOID_TAGS =
  `full body, from side, from behind, profile, ${BLANK_FACE_GUARD.tags}`;
export const SHEET_REF_FRAMING_TAGS =
  "character sheet, multiple views, full body, front view, side view, simple background, white background";

/** Framing tags for tag-prompted models: one person, head and shoulders, facing the viewer. */
export const PORTRAIT_FRAMING_TAGS =
  "portrait, close-up, straight-on, facing viewer, looking at viewer, simple background";
/** Framings and angles a neck-up, front-on portrait must not drift into, and the blank stare models default to. */
export const PORTRAIT_AVOID_TAGS = `upper body, cowboy shot, full body, from side, from behind, profile, multiple views, ${BLANK_FACE_GUARD.tags}`;

/** Trim a prompt and the stray commas or full stop it may end with. */
function tidy(text: string): string {
  return text.trim().replace(/^[,\s]+|[,.\s]+$/g, "");
}

/** Join prompt parts with commas, skipping empty ones. */
function joinParts(parts: string[]): string {
  return parts.map(tidy).filter(Boolean).join(", ");
}

/** What a portrait starts from: the main prompt and UC, and on NovelAI V4+ the character box. */
export interface PortraitPrompts {
  positive: string;
  negative: string;
  /**
   * The character's own prompt and UC, for a NovelAI character box. Null when
   * the whole portrait goes in the main prompt.
   */
  character: { positive: string; negative: string } | null;
  dropped: string[];
  expression: string;
}

/**
 * Does the selected model take character prompts? Then a portrait puts the
 * character in a character box, the way Use does.
 */
export function portraitUsesCharacterBox(): boolean {
  return generation.isNovelAi && !!generation.novelAiModel?.v4Prompt;
}

/**
 * The prompt and negative prompt a portrait starts from, written the way the
 * character's architecture is prompted: Danbooru tags, or a sentence. What a
 * neck-up portrait cannot show (legwear, shoes, poses, what they hold) is
 * left out of the card's prompt and listed in `dropped`. Expression tags the
 * card's prompt has come back as `expression`, for the dialog's own box.
 *
 * With `characterBox` (NovelAI V4+), the card's look and UC go in
 * `character` and the main prompt keeps the count, framing and portrait UC.
 */
export function portraitPrompts(
  character: Pick<SavedCharacter, "prompt" | "negative" | "architecture">,
  characterBox = false,
): PortraitPrompts {
  const avoid = tidy(character.negative);
  if (architecturePromptStyle(character.architecture) === "tags") {
    const look = neckUpTags(tidy(character.prompt));
    const face = splitExpressionTags(look.kept);
    if (characterBox) {
      const cast = splitCountTags(face.rest);
      return {
        positive: joinParts([cast.counts, "solo", PORTRAIT_FRAMING_TAGS]),
        negative: PORTRAIT_AVOID_TAGS,
        character: { positive: cast.look, negative: avoid },
        dropped: look.dropped,
        expression: face.expression,
      };
    }
    return {
      positive: joinParts(["solo", face.rest, PORTRAIT_FRAMING_TAGS]),
      negative: joinParts([avoid, PORTRAIT_AVOID_TAGS]),
      character: null,
      dropped: look.dropped,
      expression: face.expression,
    };
  }
  const look = neckUpProse(tidy(character.prompt));
  const framing =
    "seen straight from the front and framed from the neck up, looking directly at the viewer against a plain background.";
  if (characterBox) {
    return {
      positive: `A head-and-shoulders portrait of one person, ${framing}`,
      negative: BLANK_FACE_GUARD.natural,
      character: { positive: tidy(look.kept), negative: avoid },
      dropped: look.dropped,
      expression: "",
    };
  }
  return {
    positive: `A head-and-shoulders portrait of ${tidy(look.kept) || "the character"}, ${framing}`,
    negative: joinParts([avoid, BLANK_FACE_GUARD.natural]),
    character: null,
    dropped: look.dropped,
    expression: "",
  };
}

/** How a portrait prompt for this card is written. */
export function portraitStyle(character: Pick<SavedCharacter, "architecture">): "tags" | "natural" {
  return architecturePromptStyle(character.architecture) === "tags" ? "tags" : "natural";
}

/**
 * A square canvas with about as many pixels as the current dimensions, in
 * steps of 64, kept between `min` and `max`.
 */
export function portraitSize(
  width: number,
  height: number,
  limits: { max?: number; min?: number } = {},
): { width: number; height: number } {
  const area = Math.max(1, width) * Math.max(1, height);
  let side = Math.round(Math.sqrt(area) / 64) * 64;
  if (limits.max) side = Math.min(side, limits.max);
  side = Math.max(side, limits.min ?? 512);
  return { width: side, height: side };
}

/**
 * Reference-generation prompts for `framing`. `portrait` reuses the neck-up
 * portrait filter; `full_body`, `upper_body` and `sheet` preserve the full
 * character prompt so outfit and footwear tags remain intact.
 */
export function referencePrompts(
  character: Pick<SavedCharacter, "prompt" | "negative" | "architecture">,
  framing: CharacterRefFraming,
  characterBox = false,
): PortraitPrompts {
  if (framing === "portrait") return portraitPrompts(character, characterBox);

  const avoid = tidy(character.negative);
  const tagSpecs: Record<
    Exclude<CharacterRefFraming, "portrait">,
    { tags: string; avoid: string; prose: string }
  > = {
    full_body: {
      tags: FULL_BODY_REF_FRAMING_TAGS,
      avoid: BLANK_FACE_GUARD.tags,
      prose: "seen straight from the front in a full-body standing pose against a plain background.",
    },
    upper_body: {
      tags: UPPER_BODY_REF_FRAMING_TAGS,
      avoid: UPPER_BODY_REF_AVOID_TAGS,
      prose:
        "seen straight from the front and framed from the waist up, looking directly at the viewer against a plain background.",
    },
    sheet: {
      tags: SHEET_REF_FRAMING_TAGS,
      avoid: BLANK_FACE_GUARD.tags,
      prose: "in a multi-view full-body character reference sheet against a plain white background.",
    },
  };
  const spec = tagSpecs[framing];

  if (architecturePromptStyle(character.architecture) === "tags") {
    const face = splitExpressionTags(tidy(character.prompt));
    if (characterBox) {
      const cast = splitCountTags(face.rest);
      return {
        positive: joinParts([cast.counts, "solo", spec.tags]),
        negative: spec.avoid,
        character: { positive: cast.look, negative: avoid },
        dropped: [],
        expression: face.expression,
      };
    }
    return {
      positive: joinParts(["solo", face.rest, spec.tags]),
      negative: joinParts([avoid, spec.avoid]),
      character: null,
      dropped: [],
      expression: face.expression,
    };
  }

  const body = tidy(character.prompt);
  if (characterBox) {
    return {
      positive: `One person, ${spec.prose}`,
      negative: BLANK_FACE_GUARD.natural,
      character: { positive: body, negative: avoid },
      dropped: [],
      expression: "",
    };
  }
  return {
    positive: `${body || "The character"}, ${spec.prose}`,
    negative: joinParts([avoid, BLANK_FACE_GUARD.natural]),
    character: null,
    dropped: [],
    expression: "",
  };
}

/**
 * The portrait canvas for the selected model. NovelAI always gets its normal
 * square, 1024 x 1024, the largest size Opus renders for free. Anima stays at
 * or over 1024, below which it falls apart.
 */
export function currentPortraitSize(): { width: number; height: number } {
  if (generation.isNovelAi) return { width: 1024, height: 1024 };
  return portraitSize(generation.width, generation.height, {
    max: 2048,
    min: generation.isAnima ? 1024 : 512,
  });
}

function scaledAspectSize(ratioWidth: number, ratioHeight: number): { width: number; height: number } {
  const min = generation.isAnima ? 1024 : 512;
  const max = 2048;
  const area = Math.max(1, generation.width) * Math.max(1, generation.height);
  const ratio = ratioWidth / ratioHeight;
  let width = Math.round(Math.sqrt(area * ratio) / 64) * 64;
  let height = Math.round((width / ratio) / 64) * 64;
  if (width < min || height < min) {
    const bump = Math.max(min / Math.max(64, width), min / Math.max(64, height));
    width = Math.round((width * bump) / 64) * 64;
    height = Math.round((height * bump) / 64) * 64;
  }
  return {
    width: Math.max(min, Math.min(max, width)),
    height: Math.max(min, Math.min(max, height)),
  };
}

/** Canvas dimensions for generating a reference image under `framing`. */
export function currentReferenceSize(framing: CharacterRefFraming): { width: number; height: number } {
  if (framing === "portrait" || framing === "upper_body") return currentPortraitSize();
  if (generation.isNovelAi) {
    return framing === "sheet" ? { width: 1216, height: 832 } : { width: 832, height: 1216 };
  }
  return framing === "sheet" ? scaledAspectSize(1216, 832) : scaledAspectSize(832, 1216);
}

/** Comma-separated artist names as prompt tags in the current mode's form. */
export function portraitArtistTags(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((t) => t.trim())
    .filter(Boolean)
    .map((t) => (/^artist:/i.test(t) ? t : generation.formatArtistTag(t)));
}

/**
 * A picked chunk as the portrait uses it: one line of a multi-line chunk,
 * picked at random as an inline chunk would be, with what a neck-up picture
 * cannot show left out. Lines the filter empties are never picked.
 */
export function portraitChunkText(
  content: string,
  style: "tags" | "natural",
  framing: CharacterRefFraming = "portrait",
): string {
  const choices = content
    .split(/\r?\n/)
    .map((line) => line.trim().replace(/^,+|,+$/g, "").trim())
    .filter(Boolean)
    .map((line) =>
      framing === "portrait"
        ? (style === "tags" ? neckUpTags(line) : neckUpProse(line)).kept
        : line,
    )
    .filter(Boolean);
  if (choices.length === 0) return "";
  return choices[Math.floor(Math.random() * choices.length)];
}

/** What the user picked to go on top of the portrait prompt. */
export interface PortraitExtras {
  /** Comma-separated artist names, as typed. */
  artists: string;
  chunkIds: string[];
  styleIds: string[];
}

const EXTRAS_KEY = "mooshieui.characterPortrait.v1";

/** The extras picked last time, so a set of portraits can share one look. */
export function loadPortraitExtras(): PortraitExtras {
  const empty: PortraitExtras = { artists: "", chunkIds: [], styleIds: [] };
  try {
    const raw = JSON.parse(localStorage.getItem(userScopedKey(EXTRAS_KEY)) ?? "null");
    if (!raw || typeof raw !== "object") return empty;
    const ids = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
    return {
      artists: typeof raw.artists === "string" ? raw.artists : "",
      // Chunks and styles deleted since are dropped rather than kept as ghosts.
      chunkIds: ids(raw.chunkIds).filter((id) => promptPresets.getById(id)),
      styleIds: ids(raw.styleIds).filter((id) => styles.styles.some((s) => s.id === id)),
    };
  } catch {
    return empty;
  }
}

export function savePortraitExtras(extras: PortraitExtras): void {
  try {
    localStorage.setItem(userScopedKey(EXTRAS_KEY), JSON.stringify(extras));
  } catch {
    // Remembering the picks is a convenience; the portrait still runs.
  }
}

export interface CharacterImageGenerationOptions {
  target?: "portrait" | "reference";
  framing?: CharacterRefFraming;
  directorReferences?: NovelAiDirectorReference[];
  referenceDescription?: string;
  enablePr?: boolean;
}

/**
 * Queue a portrait or Precise Reference generation for `character`. Resolves
 * once submitted; when it finishes, App.svelte routes the image back to the card.
 * Throws with a message fit for a toast when it cannot be submitted.
 */
export async function generateCharacterPortrait(
  character: SavedCharacter,
  prompts: { positive: string; negative: string; character?: { positive: string; negative: string } | null },
  extras: PortraitExtras,
  options?: CharacterImageGenerationOptions,
): Promise<void> {
  if (generation.mode === "video") throw new Error(locale.t("characters.portrait.no_video"));
  if (!generation.checkpoint) throw new Error(locale.t("generation.error_no_checkpoint"));

  const target = options?.target ?? "portrait";
  const framing: CharacterRefFraming = options?.framing ?? (target === "reference" ? "full_body" : "portrait");
  const style = portraitStyle(character);
  // The character box, while the selected model still takes one. When the
  // model was switched since the dialog opened, or the box was emptied, the
  // character goes back into the main prompt and its UC into the main UC.
  const box = prompts.character?.positive.trim() && portraitUsesCharacterBox() ? prompts.character : null;
  const boxNegative = !!box && !!generation.novelAiModel?.characterNegatives;
  const mainPositive = box ? prompts.positive : joinParts([prompts.positive, prompts.character?.positive ?? ""]);
  const mainNegative = boxNegative
    ? prompts.negative
    : joinParts([prompts.character?.negative ?? "", prompts.negative]);
  const chunks = extras.chunkIds
    .map((id) => promptPresets.getById(id))
    .filter((p): p is NonNullable<typeof p> => p !== null);
  const chunkTexts = chunks.map((p) => portraitChunkText(p.content, style, framing));
  const positive = joinParts([mainPositive, ...portraitArtistTags(extras.artists), ...chunkTexts]);
  if (!positive) throw new Error(locale.t("characters.portrait.empty_prompt"));
  const chosenStyles = styles.styles.filter((s) => extras.styleIds.includes(s.id));
  const styleFragment = fragmentForStyles(chosenStyles, generation.isNovelAi);
  const uc = joinParts([
    negativeForStyles(chosenStyles),
    promptPresets.negativeFor(chunks.map((p) => p.id)),
  ]);

  const params = generation.toParams({
    prompts: { positive, negative: mainNegative.trim() },
    skipActiveStyles: true,
    extraPositive: styleFragment || undefined,
    extraNegative: uc || undefined,
    includeConditioningRegions: false,
    outsidePausedRun: true,
    // A fresh take each time.
    seed: "-1",
    overrides: { mode: "txt2img", input_image: null, mask_image: null },
  });
  const size = target === "reference" ? currentReferenceSize(framing) : currentPortraitSize();
  params.width = size.width;
  params.height = size.height;
  // One picture, nothing from the scene the prompt area is set up for.
  params.batch_size = 1;
  params.upscale_enabled = false;
  params.save_pre_upscale_image = false;
  params.controlnet = null;
  params.positive_regions = [];
  params.style_transfer_enabled = false;
  params.style_ref_enabled = false;
  params.edit_reference_images = [];
  if (params.novelai) {
    const model = generation.novelAiModel;
    const supportsPr = generation.supportsNovelAiPreciseReference;
    const supportsOutfits = model?.outfitReferences ?? false;
    const strengthMax = model?.preciseReferenceStrengthMax ?? 1;
    const guideRefs = supportsPr
      ? (options?.directorReferences ?? [])
          .filter((r) => r.enabled !== false && r.image.length > 0)
          .slice(0, NOVELAI_MAX_DIRECTOR_REFERENCES)
          .map((r) => ({
            image: r.image,
            description: normalizeCharacterReferenceDescription(r.description, {
              supportsOutfits,
              supportsSameOutfit: supportsOutfits,
            }),
            strength: clampCharacterReferenceStrength(r.strength, strengthMax),
            fidelity: clampCharacterReferenceFidelity(r.fidelity),
            enabled: true,
          }))
      : [];
    params.novelai = {
      ...params.novelai,
      // The character in its own box, with its own UC; nothing from the
      // character boxes the prompt area is set up with.
      characters: box
        ? [{ ...createNovelAiCharacter(), prompt: box.positive.trim(), negative_prompt: boxNegative ? box.negative.trim() : "" }]
        : [],
      use_coords: false,
      vibes: [],
      director_references: guideRefs,
      transparent_background: false,
      local_post_process: false,
      face_detail: { ...params.novelai.face_detail, enabled: false },
    };
  }

  let promptId: string;
  try {
    promptId = await submitGeneration(params);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    const classified = classifyGenerationError(message);
    throw new Error(
      classified.messageKey === "generation.toast.failed"
        ? message
        : locale.t(classified.messageKey, classified.params),
    );
  }
  if (target === "reference") {
    savedCharacters.claimReference(promptId, character.id, {
      description: options?.referenceDescription,
      enablePr: options?.enablePr,
    });
  } else {
    savedCharacters.claimPortrait(promptId, character.id);
  }
}

/**
 * Set a finished portrait as its card's picture, with Undo putting the old
 * one back. Does nothing when the card was deleted meanwhile.
 */
export async function applyCharacterPortrait(id: string, image: OutputImage): Promise<void> {
  const card = savedCharacters.characters.find((c) => c.id === id);
  if (!card) return;
  const images = await currentImageToCharacterThumbnail(image.url ?? null, image).catch(() => null);
  const current = savedCharacters.characters.find((c) => c.id === id);
  if (!current) return;
  if (!images) {
    gallery.showToast(locale.t("characters.thumbnail.failed"), "error");
    return;
  }
  const previous = { thumbnail: current.thumbnail, reference: current.reference };
  savedCharacters.setThumbnail(id, images.thumbnail, images.reference);
  gallery.showToast(locale.t("characters.portrait.done", { name: current.name }), "success", {
    actionLabel: locale.t("characters.undo"),
    onAction: () => savedCharacters.setThumbnail(id, previous.thumbnail, previous.reference),
    durationMs: 8000,
  });
}

/**
 * Save a finished reference generation as a Precise Reference image on `id`,
 * setting the card picture too when the card had none, with Undo.
 */
export async function applyCharacterGeneratedReference(
  id: string,
  image: OutputImage,
  opts?: { description?: string; enablePr?: boolean },
): Promise<void> {
  const card = savedCharacters.characters.find((c) => c.id === id);
  if (!card) return;
  const images = await currentImageToCharacterThumbnail(image.url ?? null, image).catch(() => null);
  const current = savedCharacters.characters.find((c) => c.id === id);
  if (!current) return;
  if (!images) {
    gallery.showToast(locale.t("characters.thumbnail.failed"), "error");
    return;
  }
  const hadThumbnail = Boolean(current.thumbnail);
  const previousThumb = { thumbnail: current.thumbnail, reference: current.reference };
  const wasPrEnabled = Boolean(current.usePreciseReference);
  const res = savedCharacters.addPreciseReferences(
    id,
    [
      {
        thumbnail: images.thumbnail,
        reference: images.reference,
        description: opts?.description,
      },
    ],
    { enablePr: opts?.enablePr },
  );
  if (res.added.length === 0) {
    if (res.hitCap) {
      gallery.showToast(
        locale.t("characters.references.toast_full", { max: String(NOVELAI_MAX_DIRECTOR_REFERENCES) }),
        "info",
      );
    } else if (res.duplicates > 0) {
      gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
    }
    return;
  }
  if (!hadThumbnail) {
    savedCharacters.setThumbnail(id, images.thumbnail, images.reference);
  }
  const addedId = res.added[0].id;
  gallery.showToast(locale.t("characters.reference_gen.done", { name: current.name }), "success", {
    actionLabel: locale.t("characters.undo"),
    onAction: () => {
      savedCharacters.removePreciseReference(id, addedId);
      if (opts?.enablePr && !wasPrEnabled) {
        savedCharacters.setUsePreciseReference(id, false);
      }
      if (!hadThumbnail) {
        savedCharacters.setThumbnail(id, previousThumb.thumbnail, previousThumb.reference);
      }
    },
    durationMs: 8000,
  });
}
