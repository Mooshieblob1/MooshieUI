import {
  h3FormatOf,
  h3InstructionLine,
  h3RewriteSystemPrompt,
  validateH3Response,
} from "./h3Prompt.js";
import type { H3PromptContext, H3ValidationResult } from "./h3Prompt.js";

/**
 * Live2D-style idle animation for H3.
 *
 * The common case for an uploaded character image is not a scene: it is "make
 * this picture breathe". H3 will happily invent a walk cycle from a one-line
 * prompt, which destroys the pose the user liked, so idle mode constrains the
 * prompt rather than the model — H3 has no motion-amplitude input, and the
 * prompt is the only lever we have.
 *
 * Everything here layers on `h3Prompt.ts`: the same six-section (ref) or
 * three-field (base) contract, the same validator, plus idle-specific rules.
 * That keeps one format implementation rather than two that drift apart.
 */

/** The idle envelope, stated once and reused by the system prompt and the guide. */
const IDLE_MOTION_RULES = `Allowed motion. Describe a natural mix of these, scaled to what is actually visible in the reference:
- Slow, complete blinks every 3 to 4 seconds: the eyelids close fully and reopen in a soft natural cycle, with the gaze drifting gently between blinks. This must be stated explicitly.
- Continuously flowing hair driven by a named ambient force (still indoor air, a soft draught, gentle wind): strands lift, cross and settle, never freezing into a fixed shape.
- Quiet breathing: a soft rise and fall of the chest and shoulders, with the clothing over them responding.
- Independent secondary motion on tails, ears, ribbons, hoods, straps, cloth and any other hanging or attached element.
- Very small head tilts and shifts of a few degrees. The overall pose is held.
- Micro hand motion if hands are visible: a finger flexing, a grip settling, a small idle adjustment.
- Ambient background life: drifting smoke or dust, flickering firelight, falling leaves or snow, distant movement.
- The camera holds a fixed position with at most the faintest handheld sway.

Forbidden. These break the idle loop and must never appear, even if the user asked for them:
- Locomotion of any kind: walking, running, stepping, turning around, standing up, sitting down.
- Large gestures, fighting, dancing, waving, or any change to the overall pose.
- Deliberate camera work: pan, tilt, zoom, push in, pull out, dolly, truck, arc, roll, or a tracking shot.
- Cuts. The whole video is a single continuous [Shot 1].
- Dialogue, singing, and speaker ids such as (S1), unless the user explicitly asked for a spoken line.

If the user's idea calls for forbidden motion, rewrite it down into the idle envelope instead of obeying it. "She walks through the market" becomes her standing in the market with hair and cloth moving in the passing air.`;

/**
 * System prompt for an idle rewrite: the standard format contract for the task
 * type, with the idle envelope appended so the constraints are the last thing
 * the model reads.
 */
export function h3IdleRewriteSystemPrompt(ctx: H3PromptContext): string {
  const isRef = h3FormatOf(ctx.taskType) === "ref";
  const body = isRef ? "detailed_description" : "integrated_multimodal_description";
  const budget = isRef
    ? `Because this is an idle loop rather than a scene, ${body} runs 250 to 450 words and never exceeds 550. Spend that budget on continuous physical detail — how the hair moves, how the breath reads, what the background is doing — not on plot.`
    : `Because this is an idle loop rather than a scene, ${body} runs 200 to 350 words. Spend that budget on continuous physical detail — how the hair moves, how the breath reads, what the background is doing — not on plot.`;

  return `${h3RewriteSystemPrompt(ctx)}

---

This is a Live2D-style IDLE animation. The subject keeps the exact pose and framing of the reference image for the whole video; only micro-motion and ambient motion are added. Match the reference's art style exactly.

${IDLE_MOTION_RULES}

${budget}

Write one continuous [Shot 1] only. State that the shot begins from the reference image and that the pose, framing, appearance, clothing and environment are preserved. Then describe the micro-motion in playback order. Fixed structural elements (furniture, walls, props the subject is not touching) stay locked.

overall_soundscape carries quiet ambience only: room tone, soft wind, faint cloth and hair movement, distant environment. No dialogue.`;
}

/**
 * Format check for an idle rewrite: everything the standard validator enforces,
 * plus the two rules that make the result actually idle. Both failures are
 * quotable straight back to the model on the retry turn.
 */
export function validateH3IdleResponse(
  text: string,
  ctx: H3PromptContext,
): H3ValidationResult {
  const base = validateH3Response(text, ctx);
  if (!base.ok) return base;

  const body = text.toLowerCase();

  if (!/\bblink(s|ing)?\b/.test(body))
    return {
      ok: false,
      rule: "This is an idle animation, so the description must state the blink cycle explicitly: the eyelids close fully and reopen every 3 to 4 seconds, with the gaze drifting between blinks.",
    };

  // Deliberate camera work is the single most common way an idle rewrite goes
  // wrong, and it is the one thing that cannot be salvaged after generation.
  const cameraMove =
    /\b(pans?|panning|tilts?|tilting|zooms?|zooming|dollys?|dollies|dollying|trucks? (?:left|right)|pushes in|pushing in|pulls? out|pulling out|pedestals? (?:up|down)|arc shot|tracking shot|rolls? (?:clockwise|counterclockwise))\b/.exec(
      body,
    );
  if (cameraMove)
    return {
      ok: false,
      rule: `The camera must hold a fixed position with at most the faintest handheld sway, but the description contains deliberate camera motion ("${cameraMove[0]}"). Remove it and state that the camera is locked off.`,
    };

  if (/\[shot 2\]/.test(body))
    return {
      ok: false,
      rule: "An idle loop is a single continuous shot. Remove [Shot 2] and every later shot, and describe the whole video as [Shot 1].",
    };

  return { ok: true, rule: null };
}

/**
 * A complete, ready-to-generate idle prompt. Unlike `h3ManualTemplate`, this is
 * not a skeleton: it is written so that submitting it unedited produces a
 * sensible idle loop of whatever was uploaded. The only angle brackets left are
 * the handful of facts we genuinely cannot know from the frontend, and each one
 * reads as a hint rather than a required edit.
 */
export function h3IdleTemplate(ctx: H3PromptContext): string {
  return h3FormatOf(ctx.taskType) === "ref"
    ? refIdleTemplate(ctx)
    : baseIdleTemplate(ctx);
}

/** The motion paragraph, shared by both templates so they cannot drift apart. */
const IDLE_MOTION_BODY = `The camera holds a completely fixed position with only the faintest handheld sway; there is no pan, tilt, zoom, push, pull or tracking movement at any point. A soft continuous current of air moves through the scene: the character's hair flows steadily, individual strands lifting, crossing one another and settling again without ever freezing into a static shape, and any ribbons, hood, ears, tail, straps or loose fabric drift with their own slower, independent rhythm. The character blinks slowly and completely every three to four seconds, the eyelids closing fully in a soft natural cycle before reopening, and between blinks the gaze drifts gently across the frame and returns. Quiet breathing lifts and lowers the chest and shoulders in a slow steady rhythm, and the clothing over them shifts and creases in response. The head makes very small tilts and shifts of only a few degrees, and the overall pose is held throughout. <If hands are visible: the fingers flex and settle in small idle adjustments.> <Ambient background motion, for example drifting dust in the light, a flickering flame, leaves stirring, distant movement.> Fixed structural elements stay locked in place. There is no locomotion, no large gesture and no change of pose at any point in the video.`;

function baseIdleTemplate(ctx: H3PromptContext): string {
  const instruction = h3InstructionLine(ctx);
  const head = instruction === null ? "" : `${instruction}\n\n`;
  const anchor =
    ctx.taskType === "t2va"
      ? "The shot holds a single steady framing."
      : "The shot begins from <Picture 1> and holds its exact pose, framing, appearance, clothing and environment for the entire video.";

  return `${head}integrated_multimodal_description: [Shot 1] <Art style of the reference, for example 2D anime illustration, cinematic live-action, 3D CG>, a steady shot of <the character: appearance, outfit, distinguishing features> in <the setting and its lighting>. ${anchor} ${IDLE_MOTION_BODY}

overall_soundscape: Quiet continuous room tone with a soft movement of air, the faint rustle of hair and fabric, and gentle breathing. <Any distant ambience the setting suggests.> No dialogue.

non_diegetic_music: <A low, slow atmospheric pad, or N/A for none.>
`;
}

function refIdleTemplate(ctx: H3PromptContext): string {
  const n = Math.max(1, ctx.referenceImageCount);
  const extraDefs =
    n > 1
      ? Array.from(
          { length: n - 1 },
          (_, i) =>
            `<Picture ${i + 2}> is an additional reference for <Subject 1>, showing <what it adds>.`,
        ).join("\n")
      : "";
  const extraRetention =
    n > 1
      ? "\n" +
        Array.from(
          { length: n - 1 },
          (_, i) =>
            `<Picture ${i + 2}> (reference for [Shot 1]): fully_preserved - appearance details taken from this reference are reproduced exactly.`,
        ).join("\n")
      : "";

  return `subject_definitions:
<Subject 1> is the character in <Picture 1>, with <appearance, outfit and distinguishing features>.
<Subject 2> is the environment in <Picture 1>, with <set dressing and lighting>.
<Picture 1> is the first frame and pose anchor for [Shot 1], showing <Subject 1> in the held idle pose within <Subject 2>.${extraDefs ? `\n${extraDefs}` : ""}

summary:
[reference generation + keyframe completion] The target video holds <Subject 1> in the same pose and framing as <Picture 1> for its full ${ctx.durationSeconds} seconds, adding Live2D-style idle micro-motion — blinking, breathing, flowing hair, shifting cloth and ambient background life — with a fixed camera.

retention_analysis:
<Subject 1> (appears in [Shot 1]): fully_preserved - identity, outfit and overall pose are retained; only additive idle micro-motion is introduced.
<Subject 2> (appears in [Shot 1]): fully_preserved - environment, set dressing and lighting are retained, with gentle ambient motion only.
<Picture 1> ([Shot 1] first frame and pose anchor): fully_preserved - the opening composition and pose match the reference exactly.${extraRetention}

detailed_description:
The target video matches the <art style of the reference, for example 2D anime illustration, cinematic live-action, 3D CG> of the reference exactly, with stable framing suitable for a Live2D-style idle loop.
[Shot 1] The shot begins from <Picture 1>. <Subject 1>, <brief visible traits>, remains in the same pose and framing inside <Subject 2>, preserving every important detail of appearance, clothing and environment. ${IDLE_MOTION_BODY}

overall_soundscape:
Quiet continuous room tone with a soft movement of air, the faint rustle of hair and fabric, and gentle breathing. <Any distant ambience the setting suggests.> No dialogue.

non_diegetic_music:
<A low, slow atmospheric pad, or N/A for none.>
`;
}
