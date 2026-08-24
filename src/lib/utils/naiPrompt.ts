/**
 * The NovelAI Diffusion V5 prompt specification, assembled per request.
 *
 * V5 does not want what the danbooru enhance produces. It wants a natural
 * language scene body, structured character boxes, its own emphasis syntax and a
 * short list of formatting rules that break generations when violated. Feeding
 * it tag soup produces worse images than the raw prompt the user typed, which is
 * why this path exists beside the existing enhance rather than inside it.
 *
 * Curated and Full are mutually exclusive. Exactly one variant block is built
 * per request, and when Full is selected the model never sees a word of Curated
 * guidance. V4.5 and V4 are out of scope entirely and keep the existing
 * danbooru-tag enhance unchanged.
 *
 * Leaf util. It must not import any store, or the generation store that needs it
 * would form a cycle.
 */

import { naiLanguageDirective } from "./naiLanguage.js";
import type { NaiLanguage } from "./naiLanguage.js";

export type NaiVariant = "curated" | "full";

export interface NaiPromptContext {
  variant: NaiVariant;
  /**
   * NovelAI's own quality stack is prepended server side when this is on, so the
   * model must not write one. With it off the model supplies the stack instead,
   * and the validator rule inverts to match.
   */
  qualityToggle: boolean;
  /** `uc_preset` as sent, so the model knows which negatives are already covered. */
  ucPreset: number;
  /** Character boxes that already exist, so the rewrite revises rather than replaces. */
  characterCount: number;
  language: NaiLanguage;
}

/**
 * Soft text budget per variant, in tokens.
 *
 * Advisory, not enforced: the modal shows a bar and turns it amber, and nothing
 * blocks. NovelAI's own hard cap is 1471 for the whole prompt, well above both.
 */
export const NAI_VARIANT_BUDGET: Record<NaiVariant, number> = {
  curated: 374,
  full: 750,
};

/**
 * Token ceiling for the rewrite turn.
 *
 * Comfortably above the Full budget plus the field labels and the character
 * boxes, because a rewrite cut off mid-span leaves an unbalanced `::` that costs
 * a retry.
 */
export const NAI_MAX_TOKENS = 1600;

/** The quality stack NovelAI prepends itself, named so the model can avoid it. */
export const NAI_QUALITY_FILLER = [
  "masterpiece",
  "best quality",
  "very aesthetic",
  "absurdres",
  "amazing quality",
  "high quality",
] as const;

const SHARED_RULES = `You are rewriting a user's idea into a NovelAI Diffusion V5 prompt.

EMPHASIS SYNTAX
- {tag} multiplies that tag's weight by 1.05. Nesting stacks, so {{tag}} is 1.05 squared.
- [tag] divides that tag's weight by 1.05. Nesting stacks the same way.
- 1.5::rain, night:: applies an explicit weight of 1.5 to everything inside the span.
- 0.5::coat:: weakens it. -1::hat:: suppresses it.
- :: closes a span. Every span you open must be closed, or the weight swallows the rest of the prompt.
- Group artist names by the weight you want them at rather than weighting each one separately.
- Inside a numeric span, put any artist name ending in a digit first: write 1.2::as109, rain:: and not 1.2::rain, as109::.

STRUCTURE
- Counts such as 1girl, 2girls, 1boy and 1other belong in the base prompt only. Never put a count inside a character box.
- Character boxes start with girl, boy or other, and are never numbered. Write "girl, silver hair, red coat". Do not write "1girl" and do not write "Character 1".
- A Text: block, when present, goes last in the base prompt. Anything written after it is swallowed.
- Omit, do not negate. If you do not want something, leave it out rather than adding it to undesired content.

FORBIDDEN CHARACTERS
- Never use an em dash or an en dash anywhere in any field. Use a comma or a period instead.

V5 CUSTOM TAGS
These are trained V5 tokens and are available to you: depthness, attractive male, low complexity, medium complexity, high complexity, ultra complexity, transparent background, has alpha, alpha transparency, meta:novel era, meta:golden era, visual novel art, bg, cg, chibi, sprite.`;

const TECHNIQUE = `CAST HANDLING
- One or two characters is comfortable. Three is the usual party size. Four works if the boxes stay thin. Five or more risks attribute bleed and blows the budget.
- When you are over budget, compress in this order: per character emphasis first, then the base natural language body, then quality tag extras, then drop a character.
- Prevent bleed by giving each character a distinct hair colour, a distinct dominant colour and a distinct silhouette.
- Snipe strays with per character undesired content, or with -1::trait:: in the box itself.
- Order character boxes left to right as they appear in the scene.

ITERATION DISCIPLINE
- The prompt you are given may already be an enhanced prompt. Assume it is.
- Change only the motif the user named. Keep every prior choice unless it was explicitly revised.
- Never redesign the whole prompt for a one line fix.
- When a motif is retired, strip it from the base body and from the character box, and add it to undesired content so it does not ghost back in.`;

const VARIANT_BLOCKS: Record<NaiVariant, string> = {
  curated: `VARIANT: V5 CURATED
- Text budget: about 374 tokens for the whole prompt. Stay under it.
- The natural language body is one to three sentences. Curated rewards precision over volume.
- Use high complexity for detailed work. Do not use ultra complexity, because Curated does not hold it.
- For comics, write two or three panels with short dialogue.
- If the idea genuinely does not fit the budget, compress it and say so in one line at the very end, after all the fields, prefixed with NOTE:. Suggest V5 Full if that is the honest answer.`,
  full: `VARIANT: V5 FULL
- Text budget: about 750 tokens for the whole prompt. Stay under it.
- The natural language body can be rich and run to several paragraphs. Full rewards detail.
- Use high complexity for detailed work, and ultra complexity for ornate illustration and comics.
- For comics, write full pages: name the speaker in each panel, and include sound effects.
- If the idea does not fit the budget, compress it in place. Do not suggest another model.`,
};

function qualityDirective(qualityToggle: boolean): string {
  if (qualityToggle) {
    return `QUALITY TAGS
The user has NovelAI's quality toggle on, so NovelAI prepends its own quality stack server side. Do not write any quality filler yourself. Never include masterpiece, best quality, very aesthetic, absurdres, amazing quality or high quality. Writing them doubles the stack and flattens the image.`;
  }
  return `QUALITY TAGS
The user has NovelAI's quality toggle off, so nothing is prepended server side. Begin the base prompt with a short quality stack of your own, for example: masterpiece, best quality, very aesthetic, absurdres.`;
}

function ucPresetName(ucPreset: number): string {
  switch (ucPreset) {
    case 0:
      return "Heavy";
    case 1:
      return "Light";
    case 2:
      return "Human Focus";
    case 3:
      return "None";
    default:
      return "default";
  }
}

function ucPresetDirective(ucPreset: number): string {
  if (ucPreset === 3) {
    return `UNDESIRED CONTENT
The user has the undesired content preset set to None, so nothing is applied for them. Write a short undesired content list covering the obvious anatomy and artefact failures for this scene, plus any motif the user wants excluded.`;
  }
  return `UNDESIRED CONTENT
The user is on the ${ucPresetName(ucPreset)} undesired content preset, so the usual quality and anatomy negatives are already applied server side. Put only custom motifs there, meaning things specific to this image that the preset would not know about. If there are none, leave the UC field blank.`;
}

const OUTPUT_CONTRACT = `OUTPUT FORMAT
Answer with labelled fields and nothing else. No preamble, no closing remark, no markdown fences, no explanation.

BASE:
<the base prompt: counts, the natural language body, tags, and a Text: sub-block last if the image needs written words>

UC:
<custom motif undesired content only, or leave this blank>

CHAR 1:
girl, <that character's box>

CHAR 2:
boy, <that character's box>

Write one CHAR block per character that needs its own box, numbered in order. Write no CHAR blocks at all if the image has no distinct characters to separate. BASE is required. Everything else is optional.`;

/** The system turn for one rewrite. */
export function naiRewriteSystemPrompt(ctx: NaiPromptContext): string {
  return [
    SHARED_RULES,
    VARIANT_BLOCKS[ctx.variant],
    qualityDirective(ctx.qualityToggle),
    ucPresetDirective(ctx.ucPreset),
    TECHNIQUE,
    naiLanguageDirective(ctx.language),
    OUTPUT_CONTRACT,
  ].join("\n\n");
}

/** The user turn: the raw prompt plus what the model needs to know about the scene. */
export function naiUserPrompt(prompt: string, ctx: NaiPromptContext): string {
  const boxes =
    ctx.characterCount > 0
      ? `\n\nThe user already has ${ctx.characterCount} character box${
          ctx.characterCount === 1 ? "" : "es"
        } set up. Return at least that many CHAR blocks, revising each one rather than replacing it wholesale. Add more only if the scene needs them.`
      : "";
  return `Rewrite this into a V5 prompt:

${prompt.trim()}${boxes}`;
}
