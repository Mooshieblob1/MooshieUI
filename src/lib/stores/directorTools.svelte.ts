import { generation } from "./generation.svelte.js";
import { novelai } from "./novelai.svelte.js";
import type { DirectorTool } from "../utils/api.js";
import type { OutputImage } from "../types/index.js";

/**
 * State for the NovelAI Director Tools modal.
 *
 * A feature store, so it may read `generation`; nothing may depend on it in the
 * other direction. It holds no reference to the API: the modal owns the
 * `novelaiAugment` call, the same way `NaiEnhanceModal` owns its rewrite, so
 * this stays a plain state machine over one form.
 *
 * There is no result state here. A Director Tool reports through the same
 * synthetic `nai-` prompt id and `comfyui:*` events a generation uses, so the
 * images arrive in the session grid and the gallery on their own.
 */

/** Display order in the modal. Also the wire names the endpoint expects. */
export const DIRECTOR_TOOLS: DirectorTool[] = [
  "bg-removal",
  "lineart",
  "sketch",
  "colorize",
  "emotion",
  "declutter",
];

/** Which tools read the `defry` and `prompt` fields. Mirrors the Rust side. */
export function toolTakesExtras(tool: DirectorTool): boolean {
  return tool === "colorize" || tool === "emotion";
}

/**
 * Background Removal is the one Director Tool NovelAI bills for at any size.
 *
 * Every tool is priced as a 28-step generation at the input's normalised size,
 * so cost follows the source image rather than the tool: a 1MP or smaller
 * source falls inside Opus's free allowance, a 3MP upscale does not and is
 * charged on every plan. Background Removal is excluded from the allowance
 * outright and billed at roughly triple the rate, so it is never free.
 *
 * Only this second rule is expressible here. The size rule is not: nothing in
 * `OutputImage` carries the source's pixel dimensions, so the modal states the
 * 1MP threshold in words instead. See docs/NOVELAI.md.
 */
export function toolAlwaysCostsAnlas(tool: DirectorTool): boolean {
  return tool === "bg-removal";
}

/** NovelAI's `defry` range, and the middle of it as a starting point. */
export const DEFRY_MAX = 5;
const DEFRY_DEFAULT = 0;

class DirectorToolsStore {
  /** The image the tool will run on. Non-null exactly while the modal is open. */
  source = $state<OutputImage | null>(null);
  /** A URL for the modal's thumbnail. Never the source of the bytes sent. */
  previewUrl = $state<string | null>(null);
  tool = $state<DirectorTool>("bg-removal");
  defry = $state(DEFRY_DEFAULT);
  prompt = $state("");
  mood = $state("");
  /** True from the click until the request is accepted, not until it finishes. */
  busy = $state(false);
  error = $state<string | null>(null);

  get isOpen(): boolean {
    return this.source !== null;
  }

  get takesExtras(): boolean {
    return toolTakesExtras(this.tool);
  }

  /**
   * Whether the form is complete enough to send.
   *
   * Only Emotion has a required field: without a mood there is nothing before
   * the `;;` separator, and the endpoint has nothing to work from. Colorize's
   * prompt is optional guidance.
   */
  get canRun(): boolean {
    if (!this.source || this.busy) return false;
    if (this.tool === "emotion") return this.mood.trim() !== "";
    return true;
  }

  /**
   * Open on an image.
   *
   * The tool selection and its fields are reset every time rather than
   * remembered: the previous run's mood or colour note almost never applies to
   * the next image, and a stale prompt silently steering a result is worse than
   * retyping it.
   */
  open(image: OutputImage, previewUrl: string | null): void {
    this.source = image;
    this.previewUrl = previewUrl;
    this.tool = "bg-removal";
    this.defry = DEFRY_DEFAULT;
    this.prompt = "";
    this.mood = "";
    this.busy = false;
    this.error = null;
  }

  /** Switch tools, clearing the fields the new tool does not use. */
  selectTool(tool: DirectorTool): void {
    this.tool = tool;
    this.error = null;
    if (!toolTakesExtras(tool)) {
      this.defry = DEFRY_DEFAULT;
      this.prompt = "";
    }
    if (tool !== "emotion") this.mood = "";
  }

  dismiss(): void {
    this.source = null;
    this.previewUrl = null;
    this.busy = false;
    this.error = null;
  }
}

export const directorTools = new DirectorToolsStore();

/**
 * Whether the Director Tools entry points should be offered at all.
 *
 * Both halves matter: the endpoint is NovelAI's, so it is pointless while a
 * local backend is selected, and it needs the key that the same account pays
 * the Anlas from. Offering it without either produces a request that can only
 * fail.
 */
export function directorToolsAvailable(): boolean {
  return generation.isNovelAi && novelai.apiKeyConfigured;
}
