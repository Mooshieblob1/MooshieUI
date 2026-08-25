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

/**
 * The user's NovelAI fields as they stand, sent only when they ask for it.
 *
 * The default rewrite is deliberately blind: the model gets the idea and
 * nothing else, because a prompt it can see is a prompt it echoes back with the
 * user's actual instruction sanded off. Handing it over turns the job into a
 * different one, an edit of a prompt that already exists, which is worth having
 * as long as it is the user who asks for it.
 */
export interface NaiExistingPrompt {
  /** The positive prompt box. */
  base: string;
  /** The undesired content box. */
  uc: string;
  /** One entry per open character box, in slot order. */
  characters: string[];
}

export interface NaiPromptContext {
  variant: NaiVariant;
  /** `uc_preset` as sent, so the model knows which negatives are already covered. */
  ucPreset: number;
  /**
   * How many character boxes the user already has open. Used when `existing` is
   * null, where the boxes themselves are not sent: it asks for that many CHAR
   * blocks back rather than for a revision of contents the model cannot see.
   */
  characterCount: number;
  /**
   * The prompt to revise, or null for the ordinary rewrite from scratch.
   *
   * Null changes what the model is told about its own inputs, not just what it
   * is given: the spec's fidelity rules invert between the two modes.
   */
  existing: NaiExistingPrompt | null;
  language: NaiLanguage;
  /**
   * One entry per attached reference image, in the order they are sent, each
   * holding that image's free-text label or "" when the user did not name it.
   *
   * Labels only: the bytes travel separately, and the prompt layer never needs
   * them. What it needs is a way for the user's instruction ("put the outfit
   * from image 1 on her") to line up with what the model is looking at, which
   * is position, plus a name when there is one.
   */
  references: string[];
}

/**
 * Soft prompt budget per variant, in tokens.
 *
 * NovelAI's launch capacity chart gives each model two numbers, an effective
 * prompt size and an in-image text size: 703 and 374 for Curated, 1471 and 750
 * for Full. It is the first of each pair a rewrite has to fit, and reading the
 * second by mistake halves the room the model thinks it has. Full's budget and
 * NovelAI's hard ceiling are the same 1471, so that bar measures the wall.
 *
 * Advisory, not enforced: the modal shows a bar and turns it amber, and nothing
 * blocks.
 */
export const NAI_VARIANT_BUDGET: Record<NaiVariant, number> = {
  curated: 703,
  full: 1471,
};

/**
 * Token ceiling for the rewrite turn.
 *
 * Comfortably above the Full budget plus the field labels, the motif undesired
 * content and the character boxes, because a rewrite cut off mid-span leaves an
 * unbalanced `::` that costs a retry. A reasoning model also bills its thinking
 * against this ceiling, so the headroom is doing two jobs. Rust clamps the
 * request to 4096 regardless.
 */
export const NAI_MAX_TOKENS = 2400;

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
- Actually use this syntax. Every rewrite should weight two to five load bearing elements, the ones the image fails without, and leave the rest at their natural weight. A prompt with no emphasis anywhere is an unfinished prompt.
- Inside a numeric span, put any artist name ending in a digit first: write 1.2::as109, rain:: and not 1.2::rain, as109::.
- The same markers work inside undesired content, where the sense inverts: {tag} avoids it harder and [tag] avoids it less.

STRUCTURE
- Counts such as 1girl, 2girls, 1boy and 1other belong in the base prompt only. Never put a count inside a character box.
- Character boxes start with girl, boy or other, and are never numbered. Write "girl, silver hair, red coat". Do not write "1girl" and do not write "Character 1".
- A Text: block, when present, goes last in the base prompt. Anything written after it is swallowed.
- Omit by default: something the user never mentioned belongs nowhere, not in undesired content.
- Negate deliberately when a detail is likely to arrive uninvited. A named character's default outfit when the user dressed them otherwise, a lookalike character, garbled lettering when the image has words: those have to be named in undesired content or the model supplies them anyway.
- When the user tells you to leave something out, that is an instruction to you and not text for the prompt. Never write "no inset" or "without speech bubbles" into the base or a box, because naming a thing tokenises it and invites it in. Drop it silently, and reach for undesired content only when it would ghost in regardless.

NAMED SUBJECTS
- A character the user names by name is the single most load bearing thing in the prompt. Never generalize one away. If the user writes Anis, the prompt says Anis; it does not say a young woman, a girl or the subject.
- Every named character contributes two tags to the base tag line: their danbooru character tag and the series it belongs to. Use danbooru form, lowercase, with the disambiguator when the bare name is ambiguous: anis (nikke) and goddess of victory: nikke, cirno and touhou, 2b (nier:automata) and nier:automata.
- Name them in the natural language body too. Write what Anis is doing, not what a woman is doing.
- Then write their canonical appearance out in full in their character box anyway: hair colour and style, eye colour, and the two or three features they are recognised by. The name tag alone is a weak anchor and drifts. The explicit tags are what hold it.
- When the user puts character A in character B's outfit, the person is A and only the clothing comes from B. A keeps their own hair, eyes and face, and B's own hair, eyes and face go into undesired content.
- If you do not know a name the user used, keep the name tag anyway, build the box from whatever the user described, and say which name you did not recognise in a NOTE: line at the very end. Never quietly substitute a character you do know, and never fall back to a generic person.

FORBIDDEN CHARACTERS
- Never use an em dash or an en dash anywhere in any field. Use a comma or a period instead.

V5 CUSTOM TAGS
These are trained V5 tokens and are available to you: depthness, attractive male, low complexity, medium complexity, high complexity, ultra complexity, transparent background, has alpha, alpha transparency, meta:novel era, meta:golden era, visual novel art, bg, cg, chibi, sprite.
- Strengthen a weak alpha with 2.1::transparent background:: rather than repeating the tag.

DATASET PREFIXES
- Furry and kemono work starts the base tag line with fur dataset.
- Landscapes, still life and other photo leaning scenes with nobody in them start it with background dataset.
- A scene with no people in it also needs no humans, or the explicit zero count such as 0girls. Without it, figures wander in.

INTERACTION TAGS
- For an action between two characters, tag the actor source#action in their box and the recipient target#action in theirs, or give both mutual#action when it is shared. So source#hug and target#hug, one in each box.
- The same pattern covers kiss, pointing, holding hands and the rest.
- Always back the pair up in the natural language body. The tags settle who is doing what to whom; the prose settles where they are standing.

TEXT AND COMIC TAGS
- Lettering needs the text tag and its language tag, such as english text or japanese text.
- Add speech bubble or thought bubble when the words sit in one, and border when the page is panelled.
- Sound effects are ordinary tags or prose. They do not go in the Text: block unless you want them rendered as lettering.
- If short lettering is the whole point of the image, the quality stack's own no text fights it. Say so in a NOTE: line and suggest turning the quality toggle off for that generation.

V5 TAG NAMES
- V5 renamed several danbooru staples. Write peace sign rather than v, double peace for both hands, bar eyes, neutral face, and character image rather than tachi-e.`;

const TECHNIQUE = `CAST HANDLING
- One or two characters is comfortable. Three is the usual party size. V5 has been driven to twenty or more in testing, so a large cast is possible rather than forbidden, but every box costs budget and raises the risk of attributes bleeding between them.
- Past four, keep every box thin and make the silhouettes unmistakable: one distinctive hair colour each, one dominant garment colour each, no shared descriptors you can avoid.
- When you are over budget, compress in this order: per character emphasis first, then the base natural language body, then quality tag extras, then drop a character.
- Prevent bleed by giving each character a distinct hair colour, a distinct dominant colour and a distinct silhouette.
- Snipe strays with per character undesired content, or with -1::trait:: in the box itself.
- Order character boxes left to right as they appear in the scene.`;

/**
 * What the model is allowed to draw on, which depends on what it was given.
 *
 * Blind is the default and the stricter of the two: with no prompt in front of
 * it, anything that reads like a half-remembered earlier draft is a hallucination
 * and has to be named as one. Once the user hands their prompt over, the same
 * instinct becomes the job, and the rule that matters instead is that a revision
 * must return the parts it was not asked to touch unchanged.
 */
/**
 * What to do with the attached images.
 *
 * The point of the feature is wording, not resemblance: a user attaches a photo
 * because they cannot name what they are looking at, so the model's job is to
 * name it for them in the prompt. Hence the two rules that carry the block --
 * describe it concretely, and never refer to the image in the answer, since the
 * image is not there at render time.
 *
 * Empty when nothing is attached, so the ordinary rewrite is unchanged.
 */
function referenceDirective(references: string[]): string {
  if (references.length === 0) return "";
  const n = references.length;
  return `REFERENCE IMAGES
- ${n} image${n === 1 ? " is" : "s are"} attached to this turn, in the order the user lists them below. Look at ${n === 1 ? "it" : "them"}.
- They are references, not pictures to reproduce. Take from each one only what the user's message asks you to take. Whatever else happens to be in frame -- the background, the other characters, the art style, the crop -- is not part of the request unless they say it is.
- Your job with them is wording. The user attached an image because they could not put it into words, so put it into words for them: garment names, colours, materials, cut, trim, accessories, hair, pose, whatever the instruction points at, in the concrete detail a prompt needs.
- Never mention the images in your answer. "the outfit from image 1" or "as shown in the reference" describes nothing to the model that renders this, so the description has to be complete on its own.
- If you cannot see the images, write the prompt from the text alone and say so in the NOTE line rather than guessing at what they showed.`;
}

function sourceFidelity(
  existing: NaiExistingPrompt | null,
  hasReferences: boolean,
): string {
  if (existing) {
    return `SOURCE FIDELITY
- The user's current prompt is given to you below, and their message is an instruction for changing it. This is a revision, not a fresh start.
- Return every field in full, including the ones you did not change. Your answer replaces their prompt outright, so anything you leave out is deleted.
- Carry the untouched parts over word for word wherever the wording still works. The user tuned those parts on purpose, and quietly rewording them is the failure this mode exists to avoid.
- Return one CHAR block per box they already have, in the same order, so that box three still means the same character afterwards. Add a box only if the instruction calls for a new character.
- Apply the instruction completely, and nothing beyond it. Do not take the chance to add a location, a time of day, weather, a wardrobe, a mood or a camera angle they did not ask for.
- If the current prompt is not in V5 shape, put it in V5 shape as you go. That is repair, and it is expected of you; it is not licence to invent.
- A named character keeps their name, their tags and their canon through the revision. Never generalize one away while editing.`;
  }
  const input = hasReferences
    ? "The user's text and the attached reference images are the whole of your input. You cannot see their current prompt, any earlier rewrite or their character boxes"
    : "The user's text is the whole of your input. You cannot see their current prompt, any earlier rewrite, their character boxes or the image they are working on";
  const trace = hasReferences
    ? "to something the user wrote, to something visible in a reference image, or to the established appearance of a subject they named by name"
    : "to something the user wrote, or to the established appearance of a subject they named by name";
  return `SOURCE FIDELITY
- ${input}, and none of it is being withheld from you: it was never sent.
- So do not write as though you remember one. Every element of your answer must trace back ${trace}. The last of those is not a loophole, it is a duty: a named character's canon is yours to supply in full.
- Do not invent a location, a time of day, weather, a wardrobe, a mood or a camera angle the user did not ask for. Where the idea is too thin to render without one, choose the plainest option that works and keep it to a few words.
- Reproduce named things exactly. A bus stop is not a train platform, a phone box is not a phone. If the user names a place, an object or a piece of lettering, it survives into the prompt unchanged.
- Named people, places and objects survive in full. Dropping a detail the user gave you is as wrong as inventing one they did not.
- Text that reads like a revision, such as "make it night" or "change the sign", is still all you have. Build a complete prompt around it rather than guessing at what it was revising.`;
}

const VARIANT_BLOCKS: Record<NaiVariant, string> = {
  curated: `VARIANT: V5 CURATED
- Prompt budget: about 703 tokens for the whole prompt, base and character boxes together. Stay under it.
- Rendered lettering has its own budget of about 374 characters, counted separately from the prompt.
- The natural language body is one to three sentences. Curated rewards precision over volume.
- Use high complexity for detailed work. Do not use ultra complexity, because Curated does not hold it.
- For comics, write two or three panels with short dialogue.
- If the idea genuinely does not fit the budget, compress it and say so in one line at the very end, after all the fields, prefixed with NOTE:. Suggest V5 Full if that is the honest answer.`,
  full: `VARIANT: V5 FULL
- Prompt budget: about 1471 tokens for the whole prompt, base and character boxes together. That figure is also NovelAI's hard ceiling, so treat it as a wall rather than a target.
- Rendered lettering has its own budget of about 750 characters, counted separately from the prompt.
- The natural language body can be rich and run to several paragraphs. Full rewards detail.
- Use high complexity for detailed work, and ultra complexity for ornate illustration and comics.
- For comics, write full pages: name the speaker in each panel, and include sound effects.
- If the idea does not fit the budget, compress it in place. Do not suggest another model.`,
};

/**
 * Quality filler is never the model's job, whichever way the toggle is set.
 *
 * NovelAI prepends its own stack when the quality toggle is on, and the toggle
 * is on by default, so writing the words as well doubles them and flattens the
 * image. With the toggle off the user has said they want no stack, which is
 * theirs to say and not the rewrite's to overrule.
 */
const QUALITY_TAGS = `QUALITY TAGS
Never write quality filler. Never include masterpiece, best quality, very aesthetic, absurdres, amazing quality or high quality anywhere in your answer. That stack belongs to NovelAI's own quality toggle, which the user controls: writing it yourself doubles it when the toggle is on, and overrules them when it is off.`;

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
The user is on the ${ucPresetName(ucPreset)} undesired content preset, so the generic quality, anatomy and artefact negatives are already applied server side. Do not repeat them: no bad hands, no worst quality, no jpeg artifacts, no lowres.

Write the motif specific half instead, the part no preset can know. Work through these and include every one that applies:
- The canonical appearance you displaced. When a character wears someone else's outfit, negate that someone else by name along with their hair colour, eye colour and signature features. When a character is out of their usual clothes, negate those clothes by name.
- Lookalike characters the description could collapse into.
- Setting elements one word away from the one asked for, such as train platform when the scene is a bus stop.
- Framing you do not want, when the composition matters.
- Text failures, whenever the image contains written words: garbled text, misspelled text, watermark, signature.

This list is usually ten to thirty comma separated tags, and it is rarely empty. Leave UC blank only when the idea has no displaced canon, no lookalikes, no lettering and no framing risk.`;
}

const OUTPUT_CONTRACT = `OUTPUT FORMAT
Answer with labelled fields and nothing else. No preamble, no closing remark, no markdown fences, no explanation.

BASE is built in three parts, in this order, separated by blank lines.

First a tag line: the count, then series and character tags, then the V5 complexity tokens you are using, then framing and composition, then the setting. Comma separated, no sentences. If the image contains written words, end this line with text and the language tags for it, such as english text or japanese text.

Then the natural language body, describing the scene as prose: who the focus is, what they are doing, the light, the surfaces, the depth. This is the part V5 was trained for, so write it properly rather than reciting the tag line back.

Then, only if the image contains written words, a Text: sub-block: the literal word Text: on its own line, followed by the lettering to be rendered, written exactly as it should appear. A line break inside a string is a line break in the image. A blank line starts a separate string, which is how a second sign or a second speech bubble is written. Everything after Text: is treated as lettering, so it is always the last thing in BASE and nothing may follow it.

Each CHAR block is comma separated tags grouped into sections, one section per line, in this order: identity, then body, then face and expression, then outfit head to toe, then pose and placement in frame. Identity starts with girl, boy or other, then the danbooru character tag if the user named one, then hair, eyes and distinguishing features spelled out. A section with nothing to say is simply left out.

BASE:
1girl, solo, <series>, <character>, high complexity, depthness, <framing>, <setting tags>

<the natural language body>

Text:
<the lettering, a blank line between separate strings, and nothing after it>

UC:
<motif specific undesired content, as instructed above>

CHAR 1:
girl, <name>, <hair>, <eyes>, <features>,
<body>,
<expression>, <gaze>,
<outfit head to toe>,
<pose>, <placement in frame>

CHAR 2:
boy, <the same five sections>

Write one CHAR block per character that needs its own box, numbered in order. Write no CHAR blocks at all if the image has no distinct characters to separate. BASE is required. Everything else is optional.`;

/** The system turn for one rewrite. */
export function naiRewriteSystemPrompt(ctx: NaiPromptContext): string {
  return [
    SHARED_RULES,
    VARIANT_BLOCKS[ctx.variant],
    QUALITY_TAGS,
    ucPresetDirective(ctx.ucPreset),
    TECHNIQUE,
    sourceFidelity(ctx.existing, ctx.references.length > 0),
    referenceDirective(ctx.references),
    naiLanguageDirective(ctx.language),
    OUTPUT_CONTRACT,
  ]
    .filter((block) => block !== "")
    .join("\n\n");
}

/**
 * The attached images, named in the order they are sent.
 *
 * The order is the whole mechanism. Images go over the wire as an ordered list
 * with no captions of their own, so this line is what ties "image 2" in the
 * user's instruction to the second thing the model is looking at. A label, when
 * the user gave one, rides along so they can write "the outfit" instead of
 * counting.
 */
function referenceManifest(references: string[]): string {
  if (references.length === 0) return "";
  const listed = references
    .map((label, i) => {
      const name = label.trim();
      return name ? `image ${i + 1} (${name})` : `image ${i + 1}`;
    })
    .join(", ");
  const n = references.length;
  return `

${n} reference image${n === 1 ? "" : "s"} attached, in this order: ${listed}.`;
}

/**
 * What the instruction says when the user attached images and typed nothing.
 *
 * An image with no words is not an empty request, it is the most literal one
 * there is. The surrounding turn already tells the model the images are its
 * input, so the only thing missing is a verb.
 *
 * This lives here rather than as a pre-filled textarea because a placeholder
 * the user has to delete is worse than one they can ignore, and because only
 * this layer knows whether any images were attached at all.
 */
const IMPLIED_INSTRUCTION = "Do this.";

function instruction(prompt: string, references: string[]): string {
  const text = prompt.trim();
  if (text) return text;
  return references.length > 0 ? IMPLIED_INSTRUCTION : text;
}

/** The user turn: the raw prompt plus what the model needs to know about the scene. */
export function naiUserPrompt(prompt: string, ctx: NaiPromptContext): string {
  if (ctx.existing) return naiEditUserPrompt(prompt, ctx.existing, ctx.references);

  const boxes =
    ctx.characterCount > 0
      ? `\n\nThe user has ${ctx.characterCount} character box${
          ctx.characterCount === 1 ? "" : "es"
        } open, whose contents you have not been given. Return at least that many CHAR blocks, written from the idea above alone. Add more only if the scene needs them.`
      : "";
  const refs = referenceManifest(ctx.references);
  const inputs = ctx.references.length
    ? "This text and the attached images are your only input, so build the whole prompt from them and invent nothing beyond them"
    : "This text is your only input, so build the whole prompt from it and invent nothing beyond it";
  return `Rewrite this into a V5 prompt. ${inputs}:

${instruction(prompt, ctx.references)}${refs}${boxes}`;
}

/**
 * The user turn when the current prompt came along with the instruction.
 *
 * Laid out in the same labelled shape the answer has to come back in, so that
 * "return every field" is a shape the model has already seen once rather than a
 * rule it has to reconstruct. Empty fields are marked rather than omitted: a
 * missing UC label reads as an instruction to leave undesired content alone,
 * where an empty one correctly reads as room to fill.
 */
function naiEditUserPrompt(
  prompt: string,
  existing: NaiExistingPrompt,
  references: string[],
): string {
  const mark = (text: string) => text.trim() || "(empty)";
  const boxes = existing.characters
    .map((box, i) => `CHAR ${i + 1}:\n${mark(box)}`)
    .join("\n\n");

  return `This is the user's prompt as it stands now.

BASE:
${mark(existing.base)}

UC:
${mark(existing.uc)}${boxes ? `\n\n${boxes}` : ""}

Apply this instruction to it, and return the whole prompt again in the output format with every field present, changed or not:

${instruction(prompt, references)}${referenceManifest(references)}`;
}
