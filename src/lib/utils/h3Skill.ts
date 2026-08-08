import type { H3PromptContext } from "./h3Prompt.js";
import { h3FormatOf } from "./h3Prompt.js";

/**
 * Model-authored operating notes for the H3 rewrite.
 *
 * The format spec in `h3Prompt.ts` is fixed, and has to be: `validateH3Response`
 * checks the output against it. What is not fixed is how a given model should go
 * about satisfying it. A 4B local model, a 32B VLM and a frontier API model fail
 * in different places - one drifts into summary, one loses field order two
 * hundred words in, one paraphrases dialogue it was told to copy - and a single
 * hardcoded paragraph of advice cannot be right for all of them.
 *
 * So the model writes its own. Once per backend it is asked what it, specifically,
 * should watch for on this job; the answer is cached and appended to every later
 * rewrite as subordinate text. Everything here is best-effort: a backend that
 * cannot produce usable notes simply runs without them.
 *
 * Nothing cached here is secret - it is the model's own advice to itself - so
 * localStorage is the right home for it, next to the prompt history.
 */

/**
 * Bump to invalidate every cached skill at once, after changing the authoring
 * prompt or the format spec it is written against. Old entries are left behind
 * rather than swept: they are a few kB, and a sweep would have to guess at keys.
 */
const SKILL_VERSION = 1;

const CACHE_PREFIX = "mooshieui.h3skill.";

/**
 * Ceiling on cached notes. Generous for a dozen bullets, and low enough that a
 * model which ignores the brief and writes an essay cannot crowd out the format
 * spec it is supposed to be subordinate to.
 */
const MAX_SKILL_CHARS = 4000;

/**
 * Field labels that mean the model wrote a prompt instead of notes about writing
 * one. Letting those through would put a second, stray field label in the system
 * turn of every rewrite.
 */
const OUTPUT_FIELD_TELLS = [
  "integrated_multimodal_description:",
  "subject_definitions:",
  "retention_analysis:",
  "overall_soundscape:",
  "non_diegetic_music:",
];

/**
 * Cache key for one backend and one shape of the job.
 *
 * Keyed on the backend because the whole point is that notes are model-specific,
 * and on format plus modality because those change what the advice is about: the
 * six-section reference format and the three-field base format have different
 * failure modes, and reading a frame is a different skill from working blind.
 *
 * `backend` is `provider/model` for an external provider, or `"local"` for the
 * bundled llama-server.
 */
export function h3SkillKey(
  backend: string,
  ctx: H3PromptContext,
  hasImage: boolean,
): string {
  const shape = `${h3FormatOf(ctx.taskType)}.${hasImage ? "vision" : "text"}`;
  return `${CACHE_PREFIX}v${SKILL_VERSION}.${shape}.${backend}`;
}

/**
 * Cached notes, `""` if this backend already tried and produced nothing usable,
 * or `null` if it has never been asked.
 *
 * The empty string is a real answer, not a miss: without it, a model that cannot
 * write usable notes would be asked again before every single rewrite.
 */
export function loadH3Skill(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Persist notes, or `""` to record that this backend was asked and came back empty. */
export function saveH3Skill(key: string, skill: string): void {
  try {
    localStorage.setItem(key, skill);
  } catch (e) {
    console.warn("[h3Skill] could not cache the authored skill", e);
  }
}

/**
 * Token ceiling for the authoring turn. A dozen one-line bullets is a few
 * hundred tokens; a model still running at this limit has stopped writing notes
 * and started writing an essay, and `sanitizeH3Skill` truncates what survives.
 */
export const H3_SKILL_MAX_TOKENS = 800;

/** The system turn that asks a model to write its own notes. */
export function h3SkillAuthoringSystem(): string {
  return `You are writing working notes for yourself.

You are about to be asked to rewrite a user's plain-language video idea into MiniMax H3's strict video prompt format. You will be given the complete format specification when that happens, so you do not need to guess at it, remember it or repeat it here.

What you are writing now is something else: technique notes addressed to yourself, about how you in particular should carry out that job given your own size, your own handling of long structured output, and your own habits. A note earns its place only if it changes what you would otherwise do.

Rules for your answer:
- Plain text only. No markdown fences, no headings, no preamble, no closing remark. Start at the first bullet.
- 6 to 12 bullets, each beginning "- ", each a single line, each an instruction to yourself.
- Technique only: how to read the input, what to settle before you start writing, how to hold a long answer consistent to its last line, and how to check your work before you stop.
- Be specific about your own failure modes. If you tend to summarize where you were asked to describe, drift out of a required structure partway through, paraphrase text you were told to copy verbatim, or pad with abstract language, name it and say what you will do instead.
- Do not restate, invent or contradict any format rule. Do not name output fields, do not show example output, do not write any fragment of a prompt.`;
}

/** The user turn describing the job the notes are for. */
export function h3SkillAuthoringUser(
  ctx: H3PromptContext,
  hasImage: boolean,
): string {
  const ref = h3FormatOf(ctx.taskType) === "ref";
  const shape = ref
    ? "six named sections in a fixed order, several hundred words in total"
    : "three named fields in a fixed order, several hundred words in total";
  const vision = hasImage
    ? "The video's first frame will be attached as an image, and the opening of your description has to match what is actually visible in it."
    : "You will work from the user's text alone. No images are attached.";

  return `The job: rewrite a user's plain-language video idea into the MiniMax H3 ${ref ? "full-reference" : "base"} prompt format.

The output is one continuous English block of ${shape}. It carries the whole audiovisual timeline in playback order - framing, subjects, setting, lighting, action, camera movement and the sound at each moment - and it reproduces any dialogue the user supplied word for word inside required tags. Prose, summary and commentary are all failures.

${vision}

Write your notes now.`;
}

/**
 * Clean a model's authored notes, or return `""` if they are unusable.
 *
 * Strict on purpose. These notes are pasted into the system turn of every later
 * rewrite, so anything ambiguous is cheaper to discard than to keep: running with
 * no notes is the ordinary case, not a degraded one.
 */
export function sanitizeH3Skill(raw: string): string {
  let text = (raw ?? "").trim();
  if (!text) return "";

  // A model that wrapped the whole answer in one fence meant well; unwrap it.
  // Anything else fenced is rejected below, because fences are the exact thing
  // the rewrite output must never contain.
  const fenced = text.match(/^```[a-zA-Z]*\n([\s\S]*?)\n?```$/);
  if (fenced) text = fenced[1].trim();
  if (text.includes("```")) return "";

  const lower = text.toLowerCase();
  if (OUTPUT_FIELD_TELLS.some((tell) => lower.includes(tell))) return "";
  // Bullets are what was asked for, and their absence means the model answered
  // some other question - usually by writing a prompt or an apology.
  if (!/^\s*[-*]\s+\S/m.test(text)) return "";

  if (text.length > MAX_SKILL_CHARS) {
    const cut = text.slice(0, MAX_SKILL_CHARS);
    const lastBreak = cut.lastIndexOf("\n");
    text = (lastBreak > 0 ? cut.slice(0, lastBreak) : cut).trim();
  }
  return text;
}

/**
 * Append authored notes to a rewrite's system prompt, subordinate to it.
 *
 * The order and the framing both matter. The format spec comes first and is
 * named as the authority, because `validateH3Response` enforces the spec and
 * nothing else; notes that quietly overruled it would produce output that reads
 * well and fails the check.
 */
export function h3SystemWithSkill(base: string, skill: string | null): string {
  const notes = (skill ?? "").trim();
  if (!notes) return base;
  return `${base}

Working notes you wrote for yourself for this exact task. They are technique, not format. Follow them where they help you meet the requirements above. Where a note conflicts with those requirements, the requirements win and the note is void.

${notes}`;
}
