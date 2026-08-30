import { generation } from "./generation.svelte.js";
import { novelai } from "./novelai.svelte.js";
import {
  clampMagnitude,
  clampVariationCount,
  enhanceScaleFits,
  enhanceTargetSize,
  magnitudeToDenoise,
  maxEnhanceScale,
  upscaleFits,
  upscaleTargetSize,
  ENHANCE_MID_SCALE,
  MAGNITUDE_DEFAULT,
  VARIATION_COUNT_DEFAULT,
  VARIETY_DEFAULT,
  type EnhanceDenoise,
  type EnhanceScaleChoice,
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

/** Which of the upscale buttons is selected. Defined in the utils module so
 *  the hub `generation` store can persist it without importing this store. */
export type { EnhanceScaleChoice } from "../utils/novelaiEnhance.js";

/**
 * Which of the three things this modal can do to the image is showing.
 *
 * One store and one modal rather than three, because all three start from the
 * same expensive step -- decoding the source and reading its true size -- and
 * splitting them would mean paying for that read again on every switch, plus
 * three copies of the cost quote that has to agree with it.
 */
export type NaiImageAction = "enhance" | "upscale" | "variations";

/**
 * How much bigger a button has to be than the one before it to be offered.
 *
 * Below this much extra area a button quotes the same resolution as its
 * neighbour, so the modal drops it rather than showing two buttons that do the
 * same thing at the same price. A source already at or past the 3MP ceiling
 * loses both 1.5x and Max this way and is offered 1x alone.
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

  /** Which action is showing. Set by whichever entry point opened the modal. */
  action = $state<NaiImageAction>("enhance");

  scaleChoice = $state<EnhanceScaleChoice>("1x");
  magnitude = $state(MAGNITUDE_DEFAULT);

  /** How many variations one run asks for, each billed separately. */
  variationCount = $state(VARIATION_COUNT_DEFAULT);
  /** img2img strength for a variation run. */
  variety = $state(VARIETY_DEFAULT);

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

  /** The source at its real size, which is what the upscaler is handed. */
  get sourceSize(): { width: number; height: number } {
    return { width: this.sourceWidth, height: this.sourceHeight };
  }

  /** What a 4x upscale of the source comes back as. */
  get upscaleSize(): { width: number; height: number } {
    if (!this.hasSourceSize) return { width: 0, height: 0 };
    return upscaleTargetSize(this.sourceWidth, this.sourceHeight);
  }

  /**
   * Whether the upscaler will accept this source.
   *
   * Its input ceiling is a megapixel, well below what Enhance can produce, so
   * an enhanced image is usually too big to then upscale. Checked here so the
   * modal can say that before the click rather than after the request fails.
   */
  get upscaleAvailable(): boolean {
    return upscaleFits(this.sourceWidth, this.sourceHeight);
  }

  get maxScale(): number {
    if (!this.hasSourceSize) return 1;
    return maxEnhanceScale(this.sourceWidth, this.sourceHeight);
  }

  /** Whether 1.5x still fits under the 3MP ceiling once snapped. */
  get midScaleAvailable(): boolean {
    if (!this.hasSourceSize) return false;
    return enhanceScaleFits(
      this.sourceWidth,
      this.sourceHeight,
      ENHANCE_MID_SCALE,
    );
  }

  /**
   * Whether "Max" reaches a size the buttons before it do not already cover.
   *
   * The floor it has to clear is 1.5x when that button is showing, and 1x when
   * it is not: on a source whose ceiling is 1.5x exactly, "Max" and "1.5x"
   * would be the same image for the same Anlas.
   */
  get maxScaleAvailable(): boolean {
    const floor = this.midScaleAvailable ? ENHANCE_MID_SCALE : 1;
    return this.maxScale >= floor * MIN_MEANINGFUL_MAX_SCALE;
  }

  get scale(): number {
    if (this.scaleChoice === "max" && this.maxScaleAvailable)
      return this.maxScale;
    if (this.scaleChoice === "1.5x" && this.midScaleAvailable)
      return ENHANCE_MID_SCALE;
    return 1;
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

  /** Size the 1.5x button quotes. */
  get midSize(): { width: number; height: number } {
    if (!this.hasSourceSize) return { width: 0, height: 0 };
    return enhanceTargetSize(
      this.sourceWidth,
      this.sourceHeight,
      ENHANCE_MID_SCALE,
    );
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
    if (
      !this.isOpen ||
      this.busy ||
      this.loadingSource ||
      this.imageBase64 === null
    )
      return false;
    // The only action with a limit of its own. The other two work on anything
    // that decoded.
    if (this.action === "upscale") return this.upscaleAvailable;
    return true;
  }

  /**
   * Open on an image.
   *
   * The scale and magnitude open on what the last enhance was run with, read
   * from the persisted generation settings so they survive a relaunch; the
   * modal shows both before anything is spent. Everything else resets: the
   * variation and advanced controls describe one image rather than a habit.
   */
  open(
    image: OutputImage,
    previewUrl: string | null,
    action: NaiImageAction = "enhance",
  ): void {
    this.source = image;
    this.previewUrl = previewUrl;
    this.action = action;
    this.sourceWidth = 0;
    this.sourceHeight = 0;
    this.imageBase64 = null;
    this.loadingSource = false;
    this.scaleChoice = generation.naiEnhanceScaleChoice;
    this.magnitude = clampMagnitude(generation.naiEnhanceMagnitude);
    this.variationCount = VARIATION_COUNT_DEFAULT;
    this.variety = VARIETY_DEFAULT;
    this.showAdvanced = false;
    const denoise = magnitudeToDenoise(this.magnitude);
    this.strength = denoise.strength;
    this.noise = denoise.noise;
    this.busy = false;
    this.error = null;
  }

  setMagnitude(value: number): void {
    this.magnitude = clampMagnitude(value);
  }

  setVariationCount(value: number): void {
    this.variationCount = clampVariationCount(value);
  }

  /**
   * Switch actions without touching the source.
   *
   * The decoded bytes and the size stay put deliberately: they describe the
   * image, not the action, and re-reading them would make every tab click cost
   * a decode and blank out the price while it ran.
   */
  setAction(action: NaiImageAction): void {
    if (this.busy) return;
    this.action = action;
    this.error = null;
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
