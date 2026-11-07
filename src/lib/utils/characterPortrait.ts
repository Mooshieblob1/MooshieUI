/**
 * Portraits for saved characters: a front-on, neck-up picture of the
 * character rendered with the selected model, used as the card's picture
 * (and, on NovelAI, as the Precise Reference that Use attaches).
 *
 * The prompt is built here and shown in the portrait dialog, where it can be
 * edited. Chunks, Artist Styles and artist tags the user picks go on top. The
 * generation goes through the normal queue with the user's model, sampler and
 * LoRAs, but nothing from the prompt area: no prompt boxes, character boxes,
 * active chunks or styles, references, regions or ControlNet.
 *
 * Lives in utils rather than a store because it reaches into `generation`,
 * `styles` and `promptPresets`, and `generation` imports those two. App.svelte
 * hands the finished image back through `applyCharacterPortrait`.
 */
import { generation } from "../stores/generation.svelte.js";
import { gallery } from "../stores/gallery.svelte.js";
import { locale } from "../stores/locale.svelte.js";
import { inlineChunkToken, promptPresets } from "../stores/promptPresets.svelte.js";
import {
  architecturePromptStyle,
  savedCharacters,
  type SavedCharacter,
} from "../stores/savedCharacters.svelte.js";
import { fragmentForStyles, styles } from "../stores/styles.svelte.js";
import type { OutputImage } from "../types/index.js";
import { userScopedKey } from "./ipc.js";
import { currentImageToCharacterThumbnail } from "./characterThumbnail.js";
import { classifyGenerationError } from "./generationErrors.js";
import { submitGeneration } from "./generationSubmit.js";
import { neckUpProse, neckUpTags } from "./portraitFilter.js";
import { splitExpressionTags } from "./portraitExpression.js";

/** Framing tags for tag-prompted models: one person, head and shoulders, facing the viewer. */
export const PORTRAIT_FRAMING_TAGS =
  "portrait, close-up, straight-on, facing viewer, looking at viewer, simple background";
/** Framings and angles a neck-up, front-on portrait must not drift into. */
export const PORTRAIT_AVOID_TAGS =
  "upper body, cowboy shot, full body, from side, from behind, profile, multiple views";

/** Trim a prompt and the stray commas or full stop it may end with. */
function tidy(text: string): string {
  return text.trim().replace(/^[,\s]+|[,.\s]+$/g, "");
}

/** Join prompt parts with commas, skipping empty ones. */
function joinParts(parts: string[]): string {
  return parts.map(tidy).filter(Boolean).join(", ");
}

/**
 * The prompt and negative prompt a portrait starts from, written the way the
 * character's architecture is prompted: Danbooru tags, or a sentence. What a
 * neck-up portrait cannot show (legwear, shoes, poses, what they hold) is
 * left out of the card's prompt and listed in `dropped`. Expression tags the
 * card's prompt has come back as `expression`, for the dialog's own box.
 */
export function portraitPrompts(
  character: Pick<SavedCharacter, "prompt" | "negative" | "architecture">,
): { positive: string; negative: string; dropped: string[]; expression: string } {
  const avoid = tidy(character.negative);
  if (architecturePromptStyle(character.architecture) === "tags") {
    const look = neckUpTags(tidy(character.prompt));
    const face = splitExpressionTags(look.kept);
    return {
      positive: joinParts(["solo", face.rest, PORTRAIT_FRAMING_TAGS]),
      negative: joinParts([avoid, PORTRAIT_AVOID_TAGS]),
      dropped: look.dropped,
      expression: face.expression,
    };
  }
  const look = neckUpProse(tidy(character.prompt));
  return {
    positive:
      `A head-and-shoulders portrait of ${tidy(look.kept) || "the character"}, seen straight from the front ` +
      "and framed from the neck up, looking directly at the viewer against a plain background.",
    negative: avoid,
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

/** Comma-separated artist names as prompt tags in the current mode's form. */
export function portraitArtistTags(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((t) => t.trim())
    .filter(Boolean)
    .map((t) => (/^artist:/i.test(t) ? t : generation.formatArtistTag(t)));
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

/**
 * Queue a portrait of `character`. Resolves once the job is submitted; the
 * picture is set on the card when it lands (see `applyCharacterPortrait`).
 * Throws with a message fit for a toast when it cannot be submitted.
 */
export async function generateCharacterPortrait(
  character: SavedCharacter,
  prompts: { positive: string; negative: string },
  extras: PortraitExtras,
): Promise<void> {
  if (generation.mode === "video") throw new Error(locale.t("characters.portrait.no_video"));
  if (!generation.checkpoint) throw new Error(locale.t("generation.error_no_checkpoint"));

  const chunkTokens = extras.chunkIds
    .map((id) => promptPresets.getById(id))
    .filter((p): p is NonNullable<typeof p> => p !== null)
    .map((p) => inlineChunkToken(p.name));
  const positive = joinParts([prompts.positive, ...portraitArtistTags(extras.artists), ...chunkTokens]);
  if (!positive) throw new Error(locale.t("characters.portrait.empty_prompt"));
  const chosenStyles = styles.styles.filter((s) => extras.styleIds.includes(s.id));
  const styleFragment = fragmentForStyles(chosenStyles, generation.isNovelAi);

  const params = generation.toParams({
    prompts: { positive, negative: prompts.negative.trim() },
    skipActiveStyles: true,
    extraPositive: styleFragment || undefined,
    includeConditioningRegions: false,
    outsidePausedRun: true,
    // A fresh face each time, so Generate again gives another take.
    seed: "-1",
    overrides: { mode: "txt2img", input_image: null, mask_image: null },
  });
  const size = currentPortraitSize();
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
    params.novelai = {
      ...params.novelai,
      characters: [],
      use_coords: false,
      vibes: [],
      director_references: [],
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
  savedCharacters.claimPortrait(promptId, character.id);
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
