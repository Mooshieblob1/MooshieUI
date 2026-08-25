import { generation } from "./generation.svelte.js";
import { novelai } from "./novelai.svelte.js";
import {
  clampMagnitude,
  enhanceTargetSize,
  magnitudeToDenoise,
  maxEnhanceScale,
  MAGNITUDE_DEFAULT,
  type EnhanceDenoise,
} from "../utils/novelaiEnhance.js";
import type { OutputImage } from "../types/index.js";

/**
 * State for the NovelAI image Enhance modal.
 *
 * A feature store, so it may read `generation`; nothing may depend on it in the
 * other direction. It holds no reference to the API: the modal owns both the
 * source read and the `novelaiGenerate` call, the same way `DirectorToolsModal`
 * owns its augment, so this stays a plain state machine over one form.
 *
 * There is no prompt state here on purpose. An enhance is an img2img pass, and
 * the prompt, undesired content and characters it uses are the ones in the
 * generation panel -- the same fields a normal generation would send. Anything
 * the user wants added to the enhance is typed there.
 *
 * There is no result state either. The backend reports through the same
 * synthetic `nai-` prompt id and `comfyui:*` events a generation uses, so the
 * image arrives in the session grid and the gallery on its own.
 */

/** Which of the two upscale buttons is selected. */
export type EnhanceScaleChoice = "1x" | "max";

/**
 * A "Max" that works out to the source's own size is not an option.
 *
 * Below this much extra area the second button would quote the same resolution
 * as 1x, so the modal offers 1x alone instead of two buttons that do the same
 * thing. A source already at or past the 3MP ceiling lands here.
 */
const MIN_MEANINGFUL_MAX_SCALE = 1.05;

class NaiImageEnhanceStore {
  /** The image to enhance. Non-null exactly while the modal is open. */
  source = $state<OutputImage | null>(null);
  /** A URL for the modal's thumbnail. Never the source of the bytes sent. */
  previewUrl = $state<string | null>(null);

  /**
   * The source's true pixel size, read from the decoded bytes rather than from
   * metadata: `size` records what was *requested*, which an upscaled or
   * imported image no longer matches, and the whole cost quote hangs off this.
   */
  sourceWidth = $state(0);
  sourceHeight = $state(0);
  /** Base64 PNG of the source, read once on open and reused on submit. */
  imageBase64 = $state<string | null>(null);
  /** True while the source is being decoded, before any size can be quoted. */
  loadingSource = $state(false);

  scaleChoice = $state<EnhanceScaleChoice>("1x");
  magnitude = $state(MAGNITUDE_DEFAULT);

  /** Whether the raw strength/noise controls are showing instead of magnitude. */
  showAdvanced = $state(false);
  strength = $state(magnitudeToDenoise(MAGNITUDE_DEFAULT).strength);
  noise = $state(magnitudeToDenoise(MAGNITUDE_DEFAULT).noise);

  /** True from the click until the request is accepted, not until it finishes. */
  busy = $state(false);
  error = $state<string | null>(null);

  get isOpen(): boolean {
    return this.source !== null;
  }

  get hasSourceSize(): boolean {
    return this.sourceWidth > 0 && this.sourceHeight > 0;
  }

  get maxScale(): number {
    if (!this.hasSourceSize) return 1;
    return maxEnhanceScale(this.sourceWidth, this.sourceHeight);
  }

  /** Whether "Max" would reach a size 1x does not already cover. */
  get maxScaleAvailable(): boolean {
    return this.maxScale >= MIN_MEANINGFUL_MAX_SCALE;
  }

  get scale(): number {
    return this.scaleChoice === "max" && this.maxScaleAvailable
      ? this.maxScale
      : 1;
  }

  /** The canvas the enhance will be generated at, for both the quote and the request. */
  get targetSize(): { width: number; height: number } {
    if (!this.hasSourceSize) return { width: 0, height: 0 };
    return enhanceTargetSize(this.sourceWidth, this.sourceHeight, this.scale);
  }

  /** Size the 1x button quotes. Snapped, so it can differ slightly from the source. */
  get oneXSize(): { width: number; height: number } {
    if (!this.hasSourceSize) return { width: 0, height: 0 };
    return enhanceTargetSize(this.sourceWidth, this.sourceHeight, 1);
  }

  get maxSize(): { width: number; height: number } {
    if (!this.hasSourceSize) return { width: 0, height: 0 };
    return enhanceTargetSize(
      this.sourceWidth,
      this.sourceHeight,
      this.maxScale,
    );
  }

  /**
   * The strength/noise pair the request will carry.
   *
   * Advanced wins when it is showing, because at that point the two sliders are
   * what the user is looking at and a magnitude they cannot see must not
   * override them.
   */
  get denoise(): EnhanceDenoise {
    if (this.showAdvanced)
      return { strength: this.strength, noise: this.noise };
    return magnitudeToDenoise(this.magnitude);
  }

  get canRun(): boolean {
    return (
      this.isOpen &&
      !this.busy &&
      !this.loadingSource &&
      this.imageBase64 !== null
    );
  }

  /**
   * Open on an image.
   *
   * Every field resets rather than carrying over: the last image's magnitude is
   * rarely the right one for this image, and a stale setting quietly spending
   * Anlas on the wrong result is worse than re-picking it.
   */
  open(image: OutputImage, previewUrl: string | null): void {
    this.source = image;
    this.previewUrl = previewUrl;
    this.sourceWidth = 0;
    this.sourceHeight = 0;
    this.imageBase64 = null;
    this.loadingSource = false;
    this.scaleChoice = "1x";
    this.magnitude = MAGNITUDE_DEFAULT;
    this.showAdvanced = false;
    const denoise = magnitudeToDenoise(MAGNITUDE_DEFAULT);
    this.strength = denoise.strength;
    this.noise = denoise.noise;
    this.busy = false;
    this.error = null;
  }

  setMagnitude(value: number): void {
    this.magnitude = clampMagnitude(value);
  }

  /**
   * Show or hide the raw controls.
   *
   * Opening seeds them from the current magnitude so the sliders start where
   * the user left the simple control, rather than jumping to some default the
   * moment the section unfolds.
   */
  toggleAdvanced(): void {
    if (!this.showAdvanced) {
      const denoise = magnitudeToDenoise(this.magnitude);
      this.strength = denoise.strength;
      this.noise = denoise.noise;
    }
    this.showAdvanced = !this.showAdvanced;
  }

  dismiss(): void {
    this.source = null;
    this.previewUrl = null;
    this.imageBase64 = null;
    this.loadingSource = false;
    this.busy = false;
    this.error = null;
  }
}

export const naiImageEnhance = new NaiImageEnhanceStore();

/**
 * Whether the Enhance entry points should be offered at all.
 *
 * Both halves matter: the pass runs on NovelAI's servers, so it is pointless
 * while a local backend is selected, and it needs the key that the same account
 * pays the Anlas from. Offering it without either produces a request that can
 * only fail.
 */
export function naiImageEnhanceAvailable(): boolean {
  return generation.isNovelAi && novelai.apiKeyConfigured;
}
