import type { NaiPromptContext } from "./naiPrompt.js";

/**
 * Model-authored operating notes for the NovelAI V5 rewrite.
 *
 * Same bargain as the H3 skill this is modelled on. The V5 specification in
 * `naiPrompt.ts` is fixed, because `validateNaiResponse` checks the output
 * against it. How a particular model should go about satisfying it is not fixed,
 * and cannot be: a 4B local model forgets to close a `::` span, a mid-size one
 * quietly drifts back into danbooru tag soup, and a frontier model over-writes a
 * Curated prompt three times over budget. One hardcoded paragraph of advice
 * cannot be right for all of them.
 *
 * So the model writes its own. Once per backend and variant it is asked what it,
 * specifically, should watch for; the answer is cached and appended to every
 * later rewrite as subordinate text. Best-effort throughout: a backend that
 * cannot produce usable notes simply runs without them.
 *
 * Nothing cached here is secret, so localStorage is the right home for it.
 */

/**
 * Bump to invalidate every cached skill at once, after changing the authoring
 * prompt or the V5 specification it is written against.
 */
const SKILL_VERSION = 5;

const CACHE_PREFIX = "mooshieui.naiskill.";

/** Ceiling on cached notes, matching the H3 skill for the same reason. */
const MAX_SKILL_CHARS = 4000;

/**
 * Tells that the model wrote a prompt instead of notes about writing one.
 * Letting those through would put a stray second field label, or a stray weight
 * span, into the system turn of every rewrite.
 */
const OUTPUT_FIELD_TELLS = ["base:", "uc:", "char 1:", "char 2:", "1girl", "1boy"];

/**
 * Cache key for one backend and one variant.
 *
 * Keyed on the backend because the whole point is that notes are model-specific,
 * and on the variant because Curated and Full fail differently: Curated is a
 * budget problem, Full is a coherence problem.
 *
 * `backend` is `provider/model` for an external provider, or `local/model` for
 * the bundled llama-server.
 */
export function naiSkillKey(backend: string, variant: string): string {
  return `${CACHE_PREFIX}v${SKILL_VERSION}.${variant}.${backend}`;
}

/**
 * Cached notes, `""` if this backend already tried and produced nothing usable,
 * or `null` if it has never been asked.
 */
export function loadNaiSkill(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Persist notes, or `""` to record that this backend was asked and came back empty. */
export function saveNaiSkill(key: string, skill: string): void {
  try {
    localStorage.setItem(key, skill);
  } catch (e) {
    console.warn("[naiSkill] could not cache the authored skill", e);
  }
}

/**
 * Token ceiling for the authoring turn.
 *
 * A dozen one-line bullets is far less, but a reasoning model bills its thinking
 * against this same ceiling and will happily spend a thousand tokens deciding
 * what to write before writing a word. The headroom is for the thinking, not for
 * the notes.
 */
export const NAI_SKILL_MAX_TOKENS = 2048;

/** The system turn that asks a model to write its own notes. */
export function naiSkillAuthoringSystem(): string {
  return `You are writing working notes for yourself.

You are about to be asked to rewrite a user's image idea into a NovelAI Diffusion V5 prompt. You will be given the complete specification when that happens, including the emphasis syntax, the character box rules and the output format, so you do not need to guess at it, remember it or repeat it here.

What you are writing now is something else: technique notes addressed to yourself, about how you in particular should carry out that job given your own size and your own habits. A note earns its place only if it changes what you would otherwise do.

Rules for your answer:
- Plain text only. No markdown fences, no headings, no preamble, no closing remark. Start at the first bullet.
- 6 to 12 bullets, each beginning "- ", each a single line, each an instruction to yourself.
- Technique only: how to read the user's idea, what to settle before you start writing, how to keep a structured multi-field answer consistent to its last line, and how to check your work before you stop.
- Be specific about your own failure modes. If you tend to fall back into comma-separated tag soup when asked for natural language, forget to close a syntax marker you opened, repeat a detail in both the scene body and a character box, or run long when told to stay short, name it and say what you will do instead.
- Do not restate, invent or contradict any specification rule. Do not name output fields, do not show example output, do not write any fragment of a prompt.`;
}

/** The user turn describing the job the notes are for. */
export function naiSkillAuthoringUser(ctx: NaiPromptContext): string {
  const curated = ctx.variant === "curated";
  const shape = curated
    ? "a prompt of roughly 703 tokens, room enough to be specific but not to ramble"
    : "a prompt of up to roughly 1471 tokens, where detail is rewarded but coherence across a long answer is the risk";

  return `The job: rewrite a user's image idea into a NovelAI Diffusion V5 ${
    curated ? "Curated" : "Full"
  } prompt.

The output is ${shape}, returned as labelled fields: a base prompt that is hybrid (Danbooru tags plus a natural language scene body), an optional custom-motif undesired content list, and one optional box per distinct character. Quality stacks and preset UC junk are forbidden. It uses NovelAI's own emphasis syntax, where markers must be opened and closed in pairs. Prose commentary, markdown, settings tables and explanation are all failures.

The input is either an idea to build a prompt from, or the user's existing prompt together with an instruction for revising it. Reference images are sometimes attached as wording aids. There is no conversation history. In the revision case every field has to come back in full, including the ones the instruction never mentions.

Write your notes now.`;
}

/**
 * Clean a model's authored notes, or return `""` if they are unusable.
 *
 * Strict on purpose. These notes are pasted into the system turn of every later
 * rewrite, so anything ambiguous is cheaper to discard than to keep: running
 * with no notes is the ordinary case, not a degraded one.
 */
export function sanitizeNaiSkill(raw: string): string {
  let text = (raw ?? "").trim();
  if (!text) return "";

  // A model that wrapped the whole answer in one fence meant well; unwrap it.
  const fenced = text.match(/^```[a-zA-Z]*\n([\s\S]*?)\n?```$/);
  if (fenced) text = fenced[1].trim();
  if (text.includes("```")) return "";

  const lower = text.toLowerCase();
  if (OUTPUT_FIELD_TELLS.some((tell) => lower.includes(tell))) return "";
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
 * The specification comes first and is named as the authority, because
 * `validateNaiResponse` enforces the specification and nothing else.
 */
export function naiSystemWithSkill(base: string, skill: string | null): string {
  const notes = (skill ?? "").trim();
  if (!notes) return base;
  return `${base}

Working notes you wrote for yourself for this exact task. They are technique, not format. Follow them where they help you meet the requirements above. Where a note conflicts with those requirements, the requirements win and the note is void.

${notes}`;
}
