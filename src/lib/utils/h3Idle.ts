import {
  h3FormatOf,
  h3RewriteSystemPrompt,
  validateH3Response,
} from "./h3Prompt.js";
import type { H3PromptContext, H3ValidationResult } from "./h3Prompt.js";

/**
 * Live2D-style idle animation for H3, triggered by what the user typed.
 *
 * The common case for an uploaded character image is not a scene: it is "make
 * this picture breathe". H3 will happily invent a walk cycle from a one-line
 * prompt, which destroys the pose the user liked, so idle mode constrains the
 * prompt rather than the model - H3 has no motion-amplitude input, and the
 * prompt is the only lever we have.
 *
 * This shipped once as a settings-panel toggle and was pulled, because a mode
 * switch sitting in the panel is something every user has to understand before
 * they can ignore it. The behaviour was never the problem; the discoverability
 * was backwards. So the trigger now lives in the prompt box: a user who writes
 * "live2D this image" gets it, and a user who has never heard of Live2D never
 * meets it at all.
 *
 * Everything here layers on `h3Prompt.ts`: the same six-section (ref) or
 * three-field (base) contract, the same validator, plus idle-specific rules.
 * That keeps one format implementation rather than two that drift apart.
 */

/**
 * Phrasings that mean "hold the pose and add small motion".
 *
 * Each pattern has to be unambiguous on its own, because a false positive
 * silently rewrites the user's scene into a still frame. That rules out a bare
 * `\bidle\b` - "the engine sits idle" is a scene - and rules out describing
 * actions ("she blinks") as opposed to asking for a mode.
 *
 * Global on purpose: every one of these is used through `String.replace`, never
 * `.test`, so there is no `lastIndex` to leak between calls.
 */
const IDLE_TRIGGERS: RegExp[] = [
  // "live2d", "live 2d", "live-2d". Nobody spells this in a video prompt for
  // any reason other than asking for exactly this.
  /\blive[\s._-]*2[\s._-]*d\b/gi,
  // "idle animation", "idle loop", "idle motion", "idle clip", "idle only"
  /\bidle\s+(?:animation|anim|loop|motion|movement|video|clip|mode|only)\b/gi,
  // "subtle idle", "gentle idle", "just idle"
  /\b(?:subtle|gentle|slight|light|small|simple|just|only)\s+idle\b/gi,
  // "keep it idle", "make her idle", "leave them idle"
  /\b(?:keep|make|leave|hold)\s+(?:it|this|that|her|him|them)\s+idle\b/gi,
  // "breathing loop", "breathing animation", "breathe only"
  /\bbreath(?:e|ing)\s+(?:loop|animation|anim|only)\b/gi,
  // "make it breathe", "just let her breathe"
  /\b(?:make|let|have)\s+(?:it|this|that|her|him|them)\s+(?:just\s+)?breathe\b/gi,
  // The other name users reach for when they mean the same thing.
  /\bv-?tuber\s*(?:style\s*)?(?:idle|loop|animation)\b/gi,
];

/**
 * Words that only ever point at the request itself, never at what is in the
 * video. If nothing but these survives trigger removal, the user wrote a pure
 * instruction and there is no scene left to rewrite.
 */
const DIRECTIVE_WORDS = new Set([
  "a",
  "an",
  "and",
  "animate",
  "animated",
  "animation",
  "as",
  "clip",
  "convert",
  "do",
  "for",
  "frame",
  "from",
  "he",
  "her",
  "him",
  "his",
  "image",
  "in",
  "into",
  "it",
  "its",
  "just",
  "make",
  "me",
  "my",
  "of",
  "on",
  "one",
  "only",
  "photo",
  "pic",
  "picture",
  "please",
  "pls",
  "she",
  "style",
  "subject",
  "that",
  "the",
  "them",
  "these",
  "they",
  "this",
  "to",
  "turn",
  "up",
  "version",
  "video",
  "with",
]);

function stripTriggers(prompt: string): string {
  let out = prompt;
  for (const re of IDLE_TRIGGERS) out = out.replace(re, " ");
  return out;
}

/** Tidy the seam left behind by a removed phrase. */
function tidy(text: string): string {
  return text
    .replace(/[^\S\n]{2,}/g, " ")
    .replace(/\s+([,.;:!?])/g, "$1")
    .replace(/^[\s,.;:!?-]+/, "")
    .replace(/[\s,.;:-]+$/, "")
    .trim();
}

function hasSceneContent(text: string): boolean {
  const tokens = text.toLowerCase().match(/[a-z0-9]+/g) ?? [];
  return tokens.some((t) => !DIRECTIVE_WORDS.has(t));
}

/**
 * Whether the user asked for an idle loop.
 *
 * Detected from the prompt text alone, so it costs nothing and needs no stored
 * setting: the request and its trigger travel together, and re-running the same
 * prompt gets the same treatment.
 */
export function detectH3IdleIntent(prompt: string): boolean {
  return stripTriggers(prompt ?? "") !== (prompt ?? "");
}

/**
 * What to send as the user turn once idle mode has fired.
 *
 * The trigger phrase is removed rather than passed through, because the model
 * has no idea what Live2D is and will otherwise write it into the video as a
 * visual style. When nothing but the trigger was typed - which is the whole
 * point of "live2D this image" - a brief stands in, so the model is asked to
 * describe the reference instead of being handed an empty turn.
 */
export function h3IdleUserPrompt(prompt: string, ctx: H3PromptContext): string {
  const rest = tidy(stripTriggers(prompt ?? ""));
  if (hasSceneContent(rest)) return rest;

  return ctx.taskType === "t2va"
    ? "Write an idle loop for a single character held in one steady framing: the pose never changes, and the only motion is breathing, blinking, flowing hair, drifting cloth and quiet ambient life."
    : "Turn the attached reference into an idle loop. Describe what is actually in it, and keep the pose, framing, appearance, clothing and setting exactly as they are; the only motion added is breathing, blinking, flowing hair, drifting cloth and quiet ambient life.";
}

/** The idle envelope, stated once so the prompt and the validator agree. */
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
 * type, with the idle envelope appended so the constraints are read last.
 *
 * `hasFirstFrameImage` is forwarded rather than defaulted away, because idle
 * mode is exactly where an attached frame matters most - the whole job is to
 * preserve what is in it.
 */
export function h3IdleRewriteSystemPrompt(
  ctx: H3PromptContext,
  hasFirstFrameImage = false,
): string {
  const isRef = h3FormatOf(ctx.taskType) === "ref";
  const body = isRef ? "detailed_description" : "integrated_multimodal_description";
  const budget = isRef
    ? `Because this is an idle loop rather than a scene, ${body} runs 250 to 450 words and never exceeds 550. Spend that budget on continuous physical detail - how the hair moves, how the breath reads, what the background is doing - not on plot.`
    : `Because this is an idle loop rather than a scene, ${body} runs 200 to 350 words. Spend that budget on continuous physical detail - how the hair moves, how the breath reads, what the background is doing - not on plot.`;
  const anchor = hasFirstFrameImage
    ? "The reference image is attached. Read it and write the description from what is actually in it: the same character, outfit, pose, framing, lighting and setting."
    : "";

  return `${h3RewriteSystemPrompt(ctx, hasFirstFrameImage)}

---

The user asked for a Live2D-style IDLE animation. The subject keeps the exact pose and framing of the reference image for the whole video; only micro-motion and ambient motion are added. Match the reference's art style exactly.${anchor ? `\n\n${anchor}` : ""}

${IDLE_MOTION_RULES}

${budget}

Write one continuous [Shot 1] only. State that the shot begins from the reference image and that the pose, framing, appearance, clothing and environment are preserved. Then describe the micro-motion in playback order. Fixed structural elements (furniture, walls, props the subject is not touching) stay locked.

overall_soundscape carries quiet ambience only: room tone, soft wind, faint cloth and hair movement, distant environment. No dialogue.

Never write the words "Live2D", "idle mode" or any other name for this instruction into the output. Describe the motion, not the request.`;
}

/**
 * Negation cues, and how far their reach extends.
 *
 * The idle prompt asks the model to state what the camera does *not* do, so a
 * good answer reliably contains "there is no pan, tilt or zoom at any point".
 * Scanning for camera verbs without discounting those spans would reject the
 * best rewrites and burn the one retry on nothing.
 */
const NEGATED_SPAN =
  /\b(?:no|not|never|without|neither|nor|avoid|avoids|avoiding)\b(?:[^.;:\n]{0,120})/gi;

/**
 * Deliberate camera work that no phrasing makes idle-legal.
 *
 * Bare "pan", "tilt" and "roll" are deliberately absent: the idle envelope
 * permits small head tilts, and a rewrite that says so is correct.
 */
const CAMERA_MOVES =
  /\b(?:zooms?|zooming|dollys?|dollies|dollying|cranes? (?:up|down)|arc shot|tracking shot|crane shot|whip pans?|pushes in|pushing in|push in|pulls? out|pulling out|pedestals? (?:up|down)|trucks? (?:left|right))\b/i;

/** Pan, tilt and roll, but only where they are unmistakably the camera's. */
const CAMERA_SUBJECT_MOVES =
  /\b(?:camera|lens|viewpoint|point of view)\b[^.;:\n]{0,40}?\b(?:pans?|panning|tilts?|tilting|rolls?|rolling|slides?|sliding|drifts? (?:left|right))\b|\b(?:pans?|panning|tilts?|tilting)\s+(?:slowly\s+|gently\s+)?(?:left|right|up|down|across)\b/i;

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

  if (!/\bblink(?:s|ed|ing)?\b/.test(body))
    return {
      ok: false,
      rule: "This is an idle animation, so the description must state the blink cycle explicitly: the eyelids close fully and reopen every 3 to 4 seconds, with the gaze drifting between blinks.",
    };

  // Deliberate camera work is the single most common way an idle rewrite goes
  // wrong, and it is the one thing that cannot be salvaged after generation.
  const asserted = body.replace(NEGATED_SPAN, " ");
  const cameraMove =
    CAMERA_MOVES.exec(asserted) ?? CAMERA_SUBJECT_MOVES.exec(asserted);
  if (cameraMove)
    return {
      ok: false,
      rule: `The camera must hold a fixed position with at most the faintest handheld sway, but the description contains deliberate camera motion ("${cameraMove[0].trim()}"). Remove it and state that the camera is locked off.`,
    };

  if (/\[shot\s*[2-9]\d*\]/.test(body))
    return {
      ok: false,
      rule: "An idle loop is a single continuous shot. Remove [Shot 2] and every later shot, and describe the whole video as [Shot 1].",
    };

  return { ok: true, rule: null };
}
