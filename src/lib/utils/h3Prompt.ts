import { H3_FPS } from "./videoParams.js";

/**
 * MiniMax H3 prompt formatting.
 *
 * H3 is trained on a strict, machine-readable prompt format, and the official
 * repo ships two *different* ones — `docs/VIDEO_PROMPT_WRITING_GUIDE_base_en.md`
 * for the image/frame-conditioned graph (T2VA / I2VA / FL2VA / L2VA) and
 * `docs/VIDEO_PROMPT_WRITING_GUIDE_ref_en.md` for the full-reference graph
 * (ref2va). They are not interchangeable: base mode has three fields and states
 * the style *after* `[Shot 1]`, while ref mode has six sections in a fixed order
 * and states the style *before* `[Shot 1]`.
 *
 * Note that "H3-Context-IR" — the schema named in the original design doc — is
 * MiniMax's hosted-API preprocessor. The open weights we run never see it; they
 * see the two formats below. Everything here targets the final format directly.
 *
 * This module is pure string construction plus a validator. It holds no state
 * and touches no store, so the caller supplies the runtime facts (snapped frame
 * count, which frames are connected, how many references are attached) that
 * decide which instruction line the model must emit.
 */

/**
 * The literal em dash the official align lines use. Kept as an escape so the
 * source stays ASCII and nobody "fixes" it later: this is model-facing training
 * format, and substituting a hyphen changes the prompt the model was tuned on.
 */
const ALIGN_DASH = "—";

/**
 * H3's five prompting task types. The first four are base mode and are chosen
 * by which keyframes are connected; `ref2va` is the separate reference graph.
 */
export type H3TaskType = "t2va" | "i2va" | "fl2va" | "l2va" | "ref2va";

/** Which of the two official guides a task type follows. */
export type H3PromptFormat = "base" | "ref";

/** Runtime facts the system prompt has to be told; none are derivable from prose. */
export interface H3PromptContext {
  taskType: H3TaskType;
  /** Snapped 17n+5 frame count actually submitted, not the requested seconds. */
  frames: number;
  /** `frames / 24`, two decimals — the `S.SS` the align line must carry. */
  durationSeconds: string;
  /** ref2va only: how many `<Picture N>` labels are available to number. */
  referenceImageCount: number;
}

/** Inputs the generation store already has; everything else is derived. */
export interface H3PromptContextInput {
  variant: "fl2va" | "ref2va";
  /** Snapped frame count (`generation.videoFrameLength`). */
  frames: number;
  hasFirstFrame: boolean;
  hasLastFrame: boolean;
  referenceImageCount: number;
}

/**
 * Effective duration to two decimals. H3 snaps the frame count to 17n+5, so the
 * duration the model must be told is `frames / 24` — never the slider value.
 * 124 frames -> "5.17", not "5.00".
 */
export function h3EffectiveDuration(frames: number): string {
  return (Math.max(1, frames) / H3_FPS).toFixed(2);
}

/**
 * Which task type a video submission is prompting for. fl2va with no frames
 * connected is plain text-to-video; with only a last frame it is L2VA, which
 * has its own instruction line and puts `<Picture 1>` on the *final* shot.
 */
export function resolveH3TaskType(input: H3PromptContextInput): H3TaskType {
  if (input.variant === "ref2va") return "ref2va";
  if (input.hasFirstFrame && input.hasLastFrame) return "fl2va";
  if (input.hasLastFrame) return "l2va";
  if (input.hasFirstFrame) return "i2va";
  return "t2va";
}

export function h3FormatOf(taskType: H3TaskType): H3PromptFormat {
  return taskType === "ref2va" ? "ref" : "base";
}

export function buildH3Context(input: H3PromptContextInput): H3PromptContext {
  return {
    taskType: resolveH3TaskType(input),
    frames: input.frames,
    durationSeconds: h3EffectiveDuration(input.frames),
    referenceImageCount: input.referenceImageCount,
  };
}

/**
 * The instruction line that must be the very first line of a base-mode prompt,
 * followed by one blank line. T2VA has none and starts straight at the fields.
 * Bracketing is inconsistent between the forms in the official guide (FL2VA
 * writes bare `Picture 1 (from Shot 1)`); it is reproduced verbatim on purpose.
 */
export function h3InstructionLine(ctx: H3PromptContext): string | null {
  const s = ctx.durationSeconds;
  switch (ctx.taskType) {
    case "i2va":
      return "For the target video, at 0.00 seconds into the target video, <Picture 1> (from [Shot 1]) is fully referenced.";
    case "fl2va":
      return `How the reference pictures align with the target video ${ALIGN_DASH} Picture 1 (from Shot 1) aligns with the 0.00-second mark of the target video; Picture 2 (from Shot N) aligns with the ${s}-second mark of the target video.`;
    case "l2va":
      return `How the reference pictures align with the target video ${ALIGN_DASH} <Picture 1> (from [Shot N]) aligns with the ${s}-second mark of the target video.`;
    default:
      return null;
  }
}

/** Shot, camera, speaker and on-screen-text syntax — identical in both guides. */
const SHARED_SYNTAX_RULES = `Shot and timing syntax:
- Every shot opens with "[Shot N]". Shot 1 carries no timestamp. Later shots begin "[Shot 2] At 00:03.500, the camera cuts to ...", with strictly increasing cut times that all fall inside the video duration.
- Ordinary cut verbs: "the camera cuts to", "the shot cuts to", "the shot transitions to", "the shot changes to", "the shot switches to". Use a cross-dissolve, fade or wipe only if the user asked for one.
- A cut must introduce new information. If only the distance or the angle changes slightly, use camera motion instead of a cut.
- A cut discards the visual context established before it. A style stated only in [Shot 1] does not survive into [Shot 2]. Every shot after the first restates the style in its opening clause, after the timestamp and before the cut verb: "[Shot 2] At 00:03.500, still live-action cinematic, the camera cuts to the empty platform edge." Restate the lighting and colour palette alongside it whenever they carry the look.

Camera motion is written as natural English inside the shot, never as trailing labels:
- Motion type: Zoom In / Zoom Out, Push In / Pull Out, Pan Left / Pan Right, Truck Left / Truck Right, Tilt Up / Tilt Down, Pedestal Up / Pedestal Down, Arc Shot, Tracking Shot, Static Shot, Shake Slightly / Shake Strongly, POV, Roll Clockwise / Roll Counterclockwise.
- Amplitude: "with small amplitude", "with large amplitude". Speed: "at slow speed", "at fast speed". Omit both when the motion is of medium amplitude at normal speed.
- Example: "The camera pushes in with small amplitude at slow speed toward the folded letter in her hands."

Speakers, dialogue and singing:
- Give each vocalizing character a stable id: (S1), (S2). Two already-numbered speakers vocalizing at once are written (S1,S2). Ids persist across shots. Characters who never vocalize get no id.
- On a speaker's first appearance, establish identity: character type, age, gender, on-screen or off-screen, and voice qualities (pitch, timbre, rate, accent).
- The identifying phrase, the id, the action and the delivery all go OUTSIDE the tag. Only the language tag and the verbatim spoken words go inside:
  The young woman with a quiet, breathy voice (S1) says: <d>[English] I get off at the next station.</d>
  The two children (S1,S2) shout together, <d>[English] Wait for us!</d>
- Preserve every word and punctuation mark of user-supplied dialogue exactly. Never translate, paraphrase, shorten or invent lines. If the user supplied no dialogue, do not add any.
- Voiceover uses the exact phrase "says in an off-screen voiceover", and immediately after the tag you must state that the on-screen character's lips stay closed:
  The man (S1) says in an off-screen voiceover: <d>[English] I still remember that road.</d> while his lips remain completely closed.
- When a line crosses a cut, mark the connecting point with <scenetrans> and state that the audio continues ("continues seamlessly across the cut", "continues uninterrupted into the next shot", "carries over from the previous shot", "remains audible across the transition"). Mark speech truncated by the end of the video with <cutoff>.

On-screen text: any banner, sign, label, subtitle or neon text visible in frame is reproduced verbatim inside English double quotation marks and is never translated. Example: A red neon sign reading "OPEN" glows above the doorway.`;

const AUDIO_FIELD_RULES = `overall_soundscape: one continuous paragraph of 1 to 4 English sentences covering ambience, physical action sounds and non-verbal human sounds (wind, rain, traffic, footsteps, fabric, impacts, breathing, laughter, panting). Dialogue, singing and diegetic music belong in the description above and must not be repeated here. Write N/A only when the user explicitly asked for complete silence.

non_diegetic_music: 1 to 3 English sentences describing background music only the audience hears. Describe instrumentation, speed, rhythm and dynamic changes. Do not use abstract mood words and do not explain the music's emotional function. Singing, on-set instruments, and music from a radio, TV or phone are diegetic and belong in the description above, not here. Write N/A when there is none.`;

function baseSystemPrompt(ctx: H3PromptContext, frameNote: string): string {
  const instruction = h3InstructionLine(ctx);
  const keyframeRules = h3KeyframeRules(ctx.taskType);
  const header =
    instruction === null
      ? `This is a text-to-video-with-audio (T2VA) task. Output NO instruction line: begin directly with the "integrated_multimodal_description:" field.`
      : `This is a ${ctx.taskType.toUpperCase()} task. The FIRST line of your output must be exactly this instruction line, reproduced character for character, followed by ONE blank line:

${instruction}

"Shot N" in that line means the index of the actual final shot in your description. Replace it with the real number.`;

  return `You rewrite a user's plain-language video idea into the exact prompt format MiniMax H3 was trained on. Output only the finished prompt. No preamble, no explanation, no markdown fences, no headings of your own.

The target video is ${ctx.frames} frames at ${H3_FPS} fps, so its duration is exactly ${ctx.durationSeconds} seconds. Every timestamp you write must fall inside that window, and the described action must fill it.

${header}

After the instruction line, output exactly these three fields, in this order, with one blank line between them and nothing else in the output:

integrated_multimodal_description: [Shot 1] ...

overall_soundscape: ...

non_diegetic_music: ...

integrated_multimodal_description carries the whole audiovisual timeline in playback order: composition, subject appearance and position, environment, lighting, actions, state changes, camera movement, and the sound occurring at that moment. State the visual style immediately AFTER "[Shot 1]" and before anything else, for example "[Shot 1] Live-action, cinematic, a medium-wide shot frames ...". Restate that same style at the head of every later shot: a cut does not carry it forward, so the shot rules below require it again each time. Common styles: Cinematic, live-action, 2D-animated, 3D CG, claymation, watercolor, vintage film.

${keyframeRules}${frameNote}${SHARED_SYNTAX_RULES}

${AUDIO_FIELD_RULES}

Write everything in English. Be concrete and physical: describe what is visible and audible, never intent, symbolism or emotional interpretation.`;
}

/** Per-task keyframe guidance. Returns "" for T2VA, which has no keyframes. */
function h3KeyframeRules(taskType: H3TaskType): string {
  switch (taskType) {
    case "i2va":
      return `<Picture 1> is the video's first frame. [Shot 1] must open in the exact state, framing and composition it establishes, then develop forward. Structure the description as: first-frame anchor, action onset, continuous development, result or reaction.

`;
    case "fl2va":
      return `Picture 1 is the first frame and Picture 2 is the last frame. Describe one continuous path from the first state to the last: first-frame state, observable intermediate changes, progressively narrowing differences, last-frame state. Prefer a SINGLE shot unless the user explicitly asked for cuts, so the model can interpolate continuously, and make sure the final [Shot N] ends on the pose, spacing and composition Picture 2 establishes.

`;
    case "l2va":
      return `<Picture 1> is the video's LAST frame and belongs to the final [Shot N], not to [Shot 1]. Open from a plausible preceding state, give an explicit action and transition path, then converge gradually in the final shot so the video lands exactly on that frame.

`;
    default:
      return "";
  }
}

function refSystemPrompt(ctx: H3PromptContext): string {
  const n = Math.max(1, ctx.referenceImageCount);
  const pictureRange = n === 1 ? "<Picture 1>" : `<Picture 1> through <Picture ${n}>`;

  return `You rewrite a user's plain-language video idea into the exact full-reference prompt format MiniMax H3 was trained on. Output only the finished prompt. No preamble, no explanation, no markdown fences, no headings of your own.

The target video is ${ctx.frames} frames at ${H3_FPS} fps, so its duration is exactly ${ctx.durationSeconds} seconds. Every timestamp you write must fall inside that window.

${n} reference image${n === 1 ? " is" : "s are"} attached, so the available picture labels are ${pictureRange}. Never refer to a picture number outside that range. There are no reference videos and no reference audio, so do not invent <Video N> or <Audio N> labels.

Output exactly these six sections, in this order, each on its own line, with a blank line between them:

subject_definitions:
summary:
retention_analysis:
detailed_description:
overall_soundscape:
non_diegetic_music:

Labels. Four kinds, numbered independently within their own kind. Once a label is assigned its meaning is fixed and must stay identical in every section:
- <Subject N>: visible content abstracted from a reference and reused or modified in the target video (people, animals, objects, scenes, backgrounds, clothing, props, interfaces, effects, styles, actions, expressions, poses).
- <Picture N>: a reference image used as a concrete target frame or as a shot-planning anchor.
- <Video N>: a reference video. <Audio N>: a referenced audio signal. Neither applies here.

subject_definitions: one line per tracked item, stating what the label denotes, its reference role, and the main features to follow. If a picture only identifies where another item came from and is not analyzed on its own, cite it inside that item's line instead of giving it a line. Examples:
<Subject 1> is the young woman in <Picture 1>, with long dark hair, a blue cardigan, and a thin silver necklace.
<Picture 2> is the first frame of [Shot 1], showing a woman seated beside a cafe window.

summary: one short English paragraph that begins with a square-bracketed task-type prefix. Allowed types: keyframe completion, reference generation, video editing, video continuation, audio reuse, audio reference. Combine with " + " and never repeat a type, for example "[reference generation + keyframe completion]". Use only labels already defined above; introduce none.

retention_analysis: one line per reference label, in the same meaning as subject_definitions, using these fixed English markers and no others. For <Subject N>, <Picture N> and <Video N>: fully_preserved, partially_preserved, attribute_transfer, weak_reference. For <Audio N>: fully_copy, partially_copy, reference, weak_reference. Format:
<Subject 1> (appears in [Shot 1], [Shot 3]): fully_preserved - ...
<Picture 2> ([Shot 1] first frame): fully_preserved - ...
Newly added actions, backgrounds or plot events are not losses of reference fidelity. Do not write speaker ids such as (S1) in this section.

detailed_description: the main body, normally 350 to 500 English words even when the video is a single shot. State the visual style in one or two sentences BEFORE "[Shot 1]", not after it:
The target video is in a cinematic, literary style with soft lighting and a slightly desaturated color palette.
[Shot 1] The scene opens in a crowded urban street ...
Then describe every shot explicitly and in playback order: composition, subject appearance and position, environment, lighting, actions, state changes, camera movement, the sound at that moment, and where referenced content takes effect. Never reduce this to a plot summary or a list of reference relationships. Anchor frames with phrasings such as "the shot begins from <Picture 1>", "the shot's keyframe corresponds to <Picture 2>", "the shot ends on <Picture 3>". Every shot after the first must also name where it starts from, so the cut does not drop the referenced look: "the shot begins from <Picture 2>", or where no picture fits, "the shot continues the appearance of <Subject 1>". When a referenced subject speaks, keep BOTH labels: <Subject 2> (S1) turns toward the woman and says, <d>[English] Last summer, I went to my grandfather's house.</d>. Assign each (Sx) once, in the order vocal events actually occur. Write [unclear] for spans you cannot make out; never guess. Standardize dialogue punctuation to , . ? ! and end every line with . ? or ! before </d>.

${SHARED_SYNTAX_RULES}

${AUDIO_FIELD_RULES}
Complete dialogue and lyrics appear only inside <d> in detailed_description and are never repeated in these two sections.

Write all six sections in English. The only text that keeps its original language is dialogue or lyrics inside <d> and text visibly present in the scene.`;
}

/**
 * Extra rules for a rewrite that carries the first frame as a real image.
 *
 * A vision model handed an attachment with no framing describes it: "a
 * photograph shows a woman at a window". That is a caption, not an H3 prompt,
 * and it misreads what the frame is for. This says what the image means and
 * how far its authority reaches - it decides how things look, the user's text
 * still decides what happens.
 *
 * Only I2VA and FL2VA condition on a first frame. Every other task type gets
 * `""`, so a first frame left over from an earlier setup cannot describe
 * itself into an unrelated rewrite.
 */
export function h3FirstFrameNote(taskType: H3TaskType): string {
  if (taskType !== "i2va" && taskType !== "fl2va") return "";
  // Bracketed for I2VA, bare for FL2VA, matching the labels the two forms use
  // elsewhere in this file.
  const label = taskType === "i2va" ? "<Picture 1>" : "Picture 1";
  return `An image is attached to this request. It IS ${label}, the video's first frame - not a separate photograph, not a mood board. Look at it and write the opening of [Shot 1] from what is actually there: who or what is in frame, their appearance and clothing, their pose and position, the setting, the lighting, the shot size and the camera angle. Describe that content as the video's opening state; never call it an image, a photo, a picture or a reference in your output.

Where the attached frame and the user's text disagree about how something looks, the frame wins. What happens after the opening is still the user's to decide. Do not invent details the frame does not show and the user did not ask for.

`;
}

/**
 * The system prompt for a rewrite, chosen by task type and stamped with the
 * runtime facts. Pass the result to `callExternalLlm()` as the system turn and
 * the user's prose as the user turn.
 *
 * Set `hasFirstFrameImage` only when the frame really is travelling with the
 * request. Promising the model an attachment it cannot see is worse than
 * saying nothing: it will describe a frame it is imagining.
 */
export function h3RewriteSystemPrompt(ctx: H3PromptContext, hasFirstFrameImage = false): string {
  if (h3FormatOf(ctx.taskType) === "ref") return refSystemPrompt(ctx);
  return baseSystemPrompt(ctx, hasFirstFrameImage ? h3FirstFrameNote(ctx.taskType) : "");
}

/**
 * Generous ceiling for a rewrite. ref2va alone targets 350-500 words across six
 * sections; the backend clamps this to [64, 4096] regardless.
 */
export const H3_MAX_TOKENS = 2048;

/**
 * Copy-pasteable skeleton for the manual path, for users with no LLM configured
 * or whose model failed the format check. Filled with the real instruction line
 * and duration so the timestamps are already correct.
 */
export function h3ManualTemplate(ctx: H3PromptContext): string {
  if (h3FormatOf(ctx.taskType) === "ref") {
    const n = Math.max(1, ctx.referenceImageCount);
    const defs = Array.from(
      { length: n },
      (_, i) =>
        `<Subject ${i + 1}> is the <what it is> in <Picture ${i + 1}>, with <the features to follow>.`,
    ).join("\n");
    const retention = Array.from(
      { length: n },
      (_, i) =>
        `<Subject ${i + 1}> (appears in [Shot 1]): fully_preserved - <what stays identical to the reference>`,
    ).join("\n");
    return `subject_definitions:
${defs}

summary:
[reference generation] The target video shows <what happens>, using <Subject 1> as <its role>.

retention_analysis:
${retention}

detailed_description:
The target video is in a <style> style with <lighting> and <color palette>.
[Shot 1] <composition, subject position, environment, lighting>. <Subject 1> <action>. The camera <motion type> with <amplitude> at <speed>. <what is audible now>.
[Shot 2] At 00:0X.XXX, still <the same style>, the shot cuts to <new information>, continuing the appearance of <Subject 1>. <continued action through to ${ctx.durationSeconds} seconds>.

overall_soundscape:
<ambience, physical action sounds, non-verbal human sounds. No dialogue, no music.>

non_diegetic_music:
<instrumentation, speed, rhythm, dynamics. Or N/A.>
`;
  }

  const instruction = h3InstructionLine(ctx);
  const head = instruction === null ? "" : `${instruction}\n\n`;
  return `${head}integrated_multimodal_description: [Shot 1] <style, for example Live-action, cinematic>, <composition, subject appearance and position, environment, lighting>. <action and state changes>. The camera <motion type> with <amplitude> at <speed>. <the character description> (S1) says: <d>[English] <exact spoken words>.</d>
[Shot 2] At 00:0X.XXX, still <the same style>, the camera cuts to <new information>. <action continuing through to ${ctx.durationSeconds} seconds>.

overall_soundscape: <ambience, physical action sounds, non-verbal human sounds. No dialogue, no music.>

non_diegetic_music: <instrumentation, speed, rhythm, dynamics. Or N/A.>
`;
}

const REF_SECTIONS = [
  "subject_definitions",
  "summary",
  "retention_analysis",
  "detailed_description",
  "overall_soundscape",
  "non_diegetic_music",
] as const;

const BASE_FIELDS = [
  "integrated_multimodal_description",
  "overall_soundscape",
  "non_diegetic_music",
] as const;

const SUMMARY_TASK_TYPES = [
  "keyframe completion",
  "reference generation",
  "video editing",
  "video continuation",
  "audio reuse",
  "audio reference",
];

const VISUAL_MARKERS = [
  "fully_preserved",
  "partially_preserved",
  "attribute_transfer",
  "weak_reference",
];

const AUDIO_MARKERS = ["fully_copy", "partially_copy", "reference", "weak_reference"];

/**
 * Result of a format check. `rule` is the violated rule, phrased so it can be
 * quoted straight back to the model on the retry turn.
 */
export interface H3ValidationResult {
  ok: boolean;
  rule: string | null;
}

/** Outcome of a full rewrite, including the one automatic retry. */
export interface H3RewriteResult {
  /** Best text produced. Offered to the user even when `ok` is false. */
  text: string;
  /** Whether that text passed the format check. */
  ok: boolean;
  /** The rule still violated when `ok` is false. */
  rule: string | null;
  /**
   * Whether the prompt asked for a Live2D-style idle loop, which swaps in a
   * stricter contract. Surfaced so the caller can say so: a rewrite that quietly
   * discards the motion the user described would otherwise read as a bug.
   */
  idle: boolean;
}

const OK: H3ValidationResult = { ok: true, rule: null };

function fail(rule: string): H3ValidationResult {
  return { ok: false, rule };
}

/**
 * Strict format check on a model's rewrite.
 *
 * Small local models (a 7B via Ollama, say) will not reliably hit the six-section
 * ref2va shape on the first try, so the caller uses this to drive exactly one
 * retry that quotes the violated rule back. If the retry also fails, show the raw
 * output next to the manual template rather than pretending it worked.
 */
export function validateH3Response(text: string, ctx: H3PromptContext): H3ValidationResult {
  const body = text.trim();
  if (!body) return fail("The output was empty. Produce the full prompt.");
  if (body.includes("```"))
    return fail("The output contained a markdown code fence. Output the prompt text only, with no fences.");

  const fields = h3FormatOf(ctx.taskType) === "ref" ? REF_SECTIONS : BASE_FIELDS;
  let cursor = -1;
  for (const field of fields) {
    const at = body.indexOf(`${field}:`);
    if (at === -1) return fail(`The output is missing the "${field}:" field.`);
    if (at < cursor)
      return fail(
        `The fields are out of order. They must appear as: ${fields.map((f) => `${f}:`).join(", ")}.`,
      );
    cursor = at;
  }

  if (!body.includes("[Shot 1]"))
    return fail('The description must open its first shot with the marker "[Shot 1]".');

  const dialogueCheck = validateDialogueTags(body);
  if (dialogueCheck) return fail(dialogueCheck);

  const pictureCheck = validatePictureLabels(body, ctx);
  if (pictureCheck) return fail(pictureCheck);

  return h3FormatOf(ctx.taskType) === "ref"
    ? validateRefSpecific(body)
    : validateBaseSpecific(body, ctx);
}

/** Every <d> must close, and open with a bracketed language tag. */
function validateDialogueTags(body: string): string | null {
  const opens = (body.match(/<d>/g) ?? []).length;
  const closes = (body.match(/<\/d>/g) ?? []).length;
  if (opens !== closes)
    return "Every <d> dialogue tag must be closed with a matching </d>.";
  for (const match of body.matchAll(/<d>([\s\S]*?)<\/d>/g)) {
    if (!/^\s*\[[A-Za-z][A-Za-z \-]*\]/.test(match[1]))
      return 'Every <d> tag must begin with a bracketed language tag, for example <d>[English] the spoken words.</d>.';
  }
  return null;
}

/** No picture label may exceed the number of references actually attached. */
function validatePictureLabels(body: string, ctx: H3PromptContext): string | null {
  const available =
    ctx.taskType === "ref2va"
      ? Math.max(1, ctx.referenceImageCount)
      : ctx.taskType === "fl2va"
        ? 2
        : ctx.taskType === "i2va" || ctx.taskType === "l2va"
          ? 1
          : 0;
  const used: number[] = [];
  for (const match of body.matchAll(/<?Picture (\d+)>?/g)) used.push(Number(match[1]));
  const highest = used.length ? Math.max(...used) : 0;
  if (highest > available)
    return available === 0
      ? "This task has no reference pictures, so the prompt must not mention Picture labels at all."
      : `Only ${available} reference picture${available === 1 ? "" : "s"} exist, so Picture ${highest} is invalid. Use labels 1 through ${available} only.`;
  return null;
}

function validateBaseSpecific(body: string, ctx: H3PromptContext): H3ValidationResult {
  const instruction = h3InstructionLine(ctx);
  const firstLine = body.split(/\r?\n/, 1)[0]?.trim() ?? "";

  if (instruction === null) {
    if (!firstLine.startsWith("integrated_multimodal_description:"))
      return fail(
        'This is a T2VA task, so there is no instruction line: the output must begin directly with "integrated_multimodal_description:".',
      );
    return OK;
  }

  // The align lines carry a live shot index the model substitutes ("Shot N"),
  // so match the stable stem and check the duration survived rather than
  // demanding a character-for-character match of the whole line.
  const stem =
    ctx.taskType === "i2va"
      ? "For the target video, at 0.00 seconds into the target video,"
      : `How the reference pictures align with the target video ${ALIGN_DASH}`;
  if (!firstLine.startsWith(stem))
    return fail(
      `The first line must be the instruction line, reproduced exactly, followed by one blank line:\n${instruction}`,
    );
  if (ctx.taskType !== "i2va" && !firstLine.includes(`${ctx.durationSeconds}-second mark`))
    return fail(
      `The instruction line must state the video's real duration: "${ctx.durationSeconds}-second mark".`,
    );
  return OK;
}

function validateRefSpecific(body: string): H3ValidationResult {
  const section = (name: string): string => {
    const start = body.indexOf(`${name}:`);
    if (start === -1) return "";
    const rest = body.slice(start + name.length + 1);
    let end = rest.length;
    for (const other of REF_SECTIONS) {
      if (other === name) continue;
      const at = rest.indexOf(`${other}:`);
      if (at !== -1 && at < end) end = at;
    }
    return rest.slice(0, end).trim();
  };

  const summary = section("summary");
  const prefix = /^\[([^\]]+)\]/.exec(summary);
  if (!prefix)
    return fail(
      `The summary must begin with a square-bracketed task-type prefix, one or more of: ${SUMMARY_TASK_TYPES.join(", ")}.`,
    );
  const types = prefix[1].split("+").map((t) => t.trim());
  const bad = types.find((t) => !SUMMARY_TASK_TYPES.includes(t));
  if (bad)
    return fail(
      `"${bad}" is not a valid summary task type. Use only: ${SUMMARY_TASK_TYPES.join(", ")}, joined with " + ".`,
    );
  if (new Set(types).size !== types.length)
    return fail("The summary task-type prefix must not repeat a type.");

  const retention = section("retention_analysis");
  if (!retention) return fail('The "retention_analysis:" section must not be empty.');
  const allMarkers = [...VISUAL_MARKERS, ...AUDIO_MARKERS];
  for (const line of retention.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    if (!allMarkers.some((m) => trimmed.includes(m)))
      return fail(
        `Every retention_analysis line needs one of the fixed relationship markers (${VISUAL_MARKERS.join(", ")} for visual labels; ${AUDIO_MARKERS.join(", ")} for audio). This line has none: "${trimmed}"`,
      );
    if (/\(S\d/.test(trimmed))
      return fail("retention_analysis must not contain speaker ids such as (S1).");
  }

  const detailed = section("detailed_description");
  const styleOpening = detailed.indexOf("[Shot 1]");
  if (styleOpening <= 0)
    return fail(
      'In reference mode the style opening comes BEFORE "[Shot 1]": one or two sentences describing the visual style, then the first shot.',
    );

  if (!section("subject_definitions"))
    return fail('The "subject_definitions:" section must not be empty.');

  const words = detailed.split(/\s+/).filter(Boolean).length;
  if (words < 200)
    return fail(
      `detailed_description is only ${words} words. It must be roughly 350 to 500 English words, describing every shot explicitly rather than summarizing the plot.`,
    );

  return OK;
}

/**
 * The retry turn. Sent as the user message on the second attempt so the model
 * sees its own output alongside the single rule it broke.
 */
export function h3RetryInstruction(rule: string, previous: string): string {
  return `Your previous output violated this rule:

${rule}

Here is what you produced:

${previous}

Rewrite it so it satisfies that rule, keeping everything else that was already correct. Output only the corrected prompt.`;
}
