import { ipcStore } from "../utils/ipc.js";
import { triggerSync } from "../utils/syncTrigger.js";
import {
  buildRegionalContextPrompt,
  mergeRegionalPromptText,
  parseRegionalPrompt,
  parseScheduledPrompt,
} from "../utils/promptSchedule.js";
import { parseSegmentDetailPrompt } from "../utils/promptSegmentDetail.js";
import {
  MODEL_FAMILIES,
  TURBO_MODEL_VARIANTS,
  signalsIndicateVPred,
} from "../utils/modelFamily.js";
import type { ModelFamily, TurboModelVariant } from "../utils/modelFamily.js";
import type {
  GenerationParams,
  LoraEntry,
  RegionalPromptSelection,
  RegionalPromptStrategy,
} from "../types/index.js";
import { autocomplete } from "./autocomplete.svelte.js";
import { models } from "./models.svelte.js";
import { styles } from "./styles.svelte.js";
import { promptPresets } from "./promptPresets.svelte.js";

const STORE_KEY = "generation-settings";
const PROMPT_HISTORY_KEY = "mooshieui.promptHistory.v1";
const MAX_PROMPT_HISTORY = 100;

export interface GenerationToParamsOptions {
  fixedPresetChoices?: ReadonlyMap<string, string>;
  /** When false, positive_regions is omitted (regional inpaint chain). */
  includeConditioningRegions?: boolean;
  overrides?: Partial<
    Pick<
      GenerationParams,
      | "mode"
      | "input_image"
      | "mask_image"
      | "positive_prompt"
      | "denoise"
      | "differential_diffusion"
    >
  >;
}

interface ModelPreset {
  steps: number;
  cfg: number;
  samplerName: string;
  scheduler: string;
  width: number;
  height: number;
  upscaleDenoise?: number;
}

function isModelFamily(value: unknown): value is ModelFamily {
  return typeof value === "string" && MODEL_FAMILIES.includes(value as ModelFamily);
}

/**
 * Translate NAI-style weight brackets to ComfyUI (tag:weight) syntax.
 * - {text} → (text:1.05)   — each layer multiplies by 1.05
 * - [text] → (text:0.9524)  — each layer divides by 1.05
 * - 1.1::text:: → (text:1.1) — A1111-style weight prefix
 * Processes innermost brackets first, so nesting works: {{tag}} → ((tag:1.05):1.05)
 */
function translateNaiWeightSyntax(prompt: string): string {
  // Process A1111-style weight::text:: syntax first
  prompt = prompt.replace(/(\d+\.?\d*)::([^:]+)::/g, (_m, weight, text) => {
    return `(${text.trim()}:${parseFloat(weight).toFixed(2)})`;
  });

  // Process innermost {text} → (text:1.05) repeatedly
  let prev: string;
  do {
    prev = prompt;
    prompt = prompt.replace(/\{([^{}]+)\}/g, (_m, inner) => `(${inner}:1.05)`);
  } while (prompt !== prev);

  // Process innermost [text] → (text:0.95) repeatedly
  // Skip escaped brackets \[ and \]
  do {
    prev = prompt;
    prompt = prompt.replace(/(?<!\\)\[([^\[\]]+)\]/g, (_m, inner) => `(${inner}:0.95)`);
  } while (prompt !== prev);

  return prompt;
}

type StylePresetId = "none" | "anime" | "cinematic" | "photoreal" | "digital_art" | "line_art";

const GENERATION_MODES = ["txt2img", "img2img", "inpainting"] as const;
type GenerationMode = (typeof GENERATION_MODES)[number];

interface ModeToggleState {
  differentialDiffusion: boolean;
  upscaleEnabled: boolean;
  controlnetEnabled: boolean;
  facefixEnabled: boolean;
  smartGuidance: boolean;
}

type ModeToggleStates = Record<GenerationMode, ModeToggleState>;

function isGenerationMode(value: unknown): value is GenerationMode {
  return typeof value === "string" && GENERATION_MODES.includes(value as GenerationMode);
}

function defaultModeToggleState(): ModeToggleState {
  return {
    differentialDiffusion: false,
    upscaleEnabled: false,
    controlnetEnabled: false,
    facefixEnabled: false,
    smartGuidance: false,
  };
}

function createDefaultModeToggles(): ModeToggleStates {
  return {
    txt2img: defaultModeToggleState(),
    img2img: defaultModeToggleState(),
    inpainting: defaultModeToggleState(),
  };
}

function booleanOrDefault(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function normalizeModeToggles(value: unknown): ModeToggleStates {
  const normalized = createDefaultModeToggles();
  if (!value || typeof value !== "object") return normalized;

  const rawStates = value as Record<string, Partial<ModeToggleState> | undefined>;
  for (const mode of GENERATION_MODES) {
    const rawState = rawStates[mode];
    if (!rawState || typeof rawState !== "object") continue;
    const defaults = normalized[mode];
    normalized[mode] = {
      differentialDiffusion: booleanOrDefault(rawState.differentialDiffusion, defaults.differentialDiffusion),
      upscaleEnabled: booleanOrDefault(rawState.upscaleEnabled, defaults.upscaleEnabled),
      controlnetEnabled: booleanOrDefault(rawState.controlnetEnabled, defaults.controlnetEnabled),
      facefixEnabled: booleanOrDefault(rawState.facefixEnabled, defaults.facefixEnabled),
      smartGuidance: booleanOrDefault(rawState.smartGuidance, defaults.smartGuidance),
    };
  }

  return normalized;
}

interface StylePreset {
  id: StylePresetId;
  label: string;
  positive: string;
  negative: string;
}

/** Signature/watermark tags merged into default negative quality and style presets. */
export const STANDARD_NEGATIVE_SIGNATURE_TAGS =
  "watermark, patreon username, patreon logo, artist name, artist logo, copyright name, copyright notice";

function splitPromptTags(text: string): string[] {
  return text
    .split(",")
    .map((tag) => tag.trim())
    .filter(Boolean);
}

function appendMissingNegativeTags(base: string): string {
  const trimmed = base.trim();
  if (!trimmed) return trimmed;

  const seen = new Set(splitPromptTags(trimmed).map((tag) => tag.toLowerCase()));
  const merged = [...splitPromptTags(trimmed)];

  for (const tag of splitPromptTags(STANDARD_NEGATIVE_SIGNATURE_TAGS)) {
    const normalized = tag.toLowerCase();
    if (!seen.has(normalized)) {
      seen.add(normalized);
      merged.push(tag);
    }
  }

  return merged.join(", ");
}

interface PromptHistoryEntry {
  id: string;
  positivePrompt: string;
  negativePrompt: string;
  mode: GenerationMode;
  stylePreset: StylePresetId;
  createdAt: number;
  favorite: boolean;
}

const STYLE_PRESETS: StylePreset[] = [
  {
    id: "none",
    label: "None",
    positive: "",
    negative: "",
  },
  {
    id: "anime",
    label: "Anime",
    positive: "anime style, vibrant colors, clean linework, detailed illustration",
    negative: appendMissingNegativeTags("photo, realistic skin texture, grainy"),
  },
  {
    id: "cinematic",
    label: "Cinematic",
    positive: "cinematic lighting, dramatic composition, film still, volumetric light",
    negative: appendMissingNegativeTags("flat lighting, low contrast"),
  },
  {
    id: "photoreal",
    label: "Photoreal",
    positive: "photorealistic, ultra-detailed, natural lighting, high dynamic range",
    negative: appendMissingNegativeTags("cartoon, anime, painting, cgi"),
  },
  {
    id: "digital_art",
    label: "Digital Art",
    positive: "digital painting, concept art, painterly details, high detail",
    negative: appendMissingNegativeTags("low detail, flat colors"),
  },
  {
    id: "line_art",
    label: "Line Art",
    positive: "line art, clean outlines, monochrome illustration",
    negative: appendMissingNegativeTags("heavy shading, photorealistic texture, noisy background"),
  },
];

/** Default quality tags for Anima models */
export const DEFAULT_ANIMA_POSITIVE_QUALITY = "newest, masterpiece, best quality, score_9, score_8, safe, highres";
export const DEFAULT_ANIMA_NEGATIVE_QUALITY = appendMissingNegativeTags(
  "worst quality, low quality, score_1, score_2, score_3, blurry, jpeg artifacts, sepia",
);

/** Default quality tags for Illustrious/NoobAI family models (SIH, NoobAI vpred, etc.) */
export const DEFAULT_ILLUSTRIOUS_POSITIVE_QUALITY = "best quality, masterpiece, absurdres, newest, very aesthetic";
export const DEFAULT_ILLUSTRIOUS_NEGATIVE_QUALITY = appendMissingNegativeTags(
  "worst quality, bad quality, low quality, lowres, artistic error, bad anatomy, extra fingers, text, signature, watermark, long body, bad hands, cropped, username",
);

/** Default quality tags for Pony Diffusion models */
export const DEFAULT_PONY_POSITIVE_QUALITY = "score_9, score_8_up, score_7_up, source_anime";
export const DEFAULT_PONY_NEGATIVE_QUALITY = appendMissingNegativeTags(
  "score_1, score_2, score_3, worst quality, low quality",
);

/** Default quality tags for Nanosaur models */
export const DEFAULT_NANOSAUR_POSITIVE_QUALITY = "newest, masterpiece, best quality, absurdres";
export const DEFAULT_NANOSAUR_NEGATIVE_QUALITY = appendMissingNegativeTags(
  "oldest, low quality, cartoon, blurry, sketch, monochrome, flat color, text, watermark",
);

class GenerationStore {
  _mode = $state<GenerationMode>("txt2img");
  modeToggles = $state<ModeToggleStates>(createDefaultModeToggles());
  positivePrompt = $state("");
  negativePrompt = $state("");
  checkpoint = $state("");
  vae = $state("");
  loras = $state<LoraEntry[]>([]);
  samplerName = $state("euler_cfg_pp");
  scheduler = $state("sgm_uniform");
  steps = $state(20);
  cfg = $state(1.4);
  seed = $state(-1);
  width = $state(512);
  height = $state(512);
  batchSize = $state(1);
  denoise = $state(0.7);
  inputImage = $state<string | null>(null);
  maskImage = $state<string | null>(null);
  growMaskBy = $state(6);
  differentialDiffusion = $state(false);
  upscaleEnabled = $state(false);
  upscaleMethod = $state<"algorithmic" | "model">("algorithmic");
  upscaleModel = $state<string | null>(null);
  upscaleScale = $state(2.0);
  upscaleDenoise = $state(0.4);
  upscaleSteps = $state(15);
  upscaleTileSize = $state(1024);
  upscaleTiling = $state(true);
  upscaleFastRefine = $state(false);
  upscaleSoftGuidance = $state(true);
  upscaleSoftGuidanceMultiplier = $state(0.4);
  /** Advanced: also save the base image before the upscale chain runs. */
  savePreUpscaleImage = $state(false);
  /**
   * img2img only: skip the base img2img sampling pass and feed the input image
   * directly into the upscale/refine chain (SwarmUI "Refine Image" semantics).
   */
  refineOnly = $state(false);
  smartGuidance = $state(false);
  /**
   * FluxGuidance value (used by Flux Dev / Flux 2 Klein family). Replaces
   * CFG for those models since they're guidance-distilled and ignore CFG.
   * Range: 0-10, sweet spot 2-4. Default matches ComfyUI's FluxGuidance node.
   */
  fluxGuidance = $state(3.5);
  useSplitModel = $state(false);
  diffusionModel = $state<string | null>(null);
  clipModel = $state<string | null>(null);
  clipType = $state<string | null>(null);
  stylePreset = $state<StylePresetId>("none");
  stylePresetsEnabled = $state(false);
  controlnetEnabled = $state(false);
  controlnetMode = $state<"preset" | "custom">("preset");
  controlnetPreset = $state<string | null>(null);
  controlnetModel = $state<string | null>(null);
  controlnetPreprocessor = $state<string | null>(null);
  controlnetImage = $state<string | null>(null);
  controlnetStrength = $state(1.0);
  controlnetStartPercent = $state(0.0);
  controlnetEndPercent = $state(1.0);
  styleTransferEnabled = $state(false);
  styleReferenceImage = $state<string | null>(null);
  styleTransferLowScaleEnd = $state(1.5);
  styleTransferHighScaleStart = $state(1.0);
  styleTransferBeta = $state(50);
  styleTransferAdainStrength = $state(0.5);
  styleTransferRfMode = $state("rf_gamma_rk2");
  styleTransferGamma = $state(0.5);
  styleTransferGammaCurve = $state(2);
  styleTransferNormStrength = $state(1);
  styleTransferPmiAlpha = $state(0.5);
  styleTransferMegapixels = $state(1.05);
  styleTransferBlocks = $state("0-999");
  facefixEnabled = $state(false);
  facefixDetector = $state<string | null>(null);
  facefixDenoise = $state(0.4);
  facefixSteps = $state(20);
  facefixGuideSize = $state(512);
  facefixMaxFaces = $state(8);
  facefixAutoPrompt = $state(false);
  outputBitDepth = $state<"8bit" | "16bit">("8bit");
  outputFormat = $state<"png" | "jxl">("png");
  metadataMode = $state<"text_chunk" | "stealth" | "both">("both");
  autoQualityTags = $state(true);
  customAnimaPositiveQuality = $state(DEFAULT_ANIMA_POSITIVE_QUALITY);
  customAnimaNegativeQuality = $state(DEFAULT_ANIMA_NEGATIVE_QUALITY);
  customIllustriousPositiveQuality = $state(DEFAULT_ILLUSTRIOUS_POSITIVE_QUALITY);
  customIllustriousNegativeQuality = $state(DEFAULT_ILLUSTRIOUS_NEGATIVE_QUALITY);
  customPonyPositiveQuality = $state(DEFAULT_PONY_POSITIVE_QUALITY);
  customPonyNegativeQuality = $state(DEFAULT_PONY_NEGATIVE_QUALITY);
  customNanosaurPositiveQuality = $state(DEFAULT_NANOSAUR_POSITIVE_QUALITY);
  customNanosaurNegativeQuality = $state(DEFAULT_NANOSAUR_NEGATIVE_QUALITY);
  promptHistory = $state<PromptHistoryEntry[]>([]);
  /** When true, images are NOT auto-saved to the internal gallery — user saves manually. */
  manualSaveMode = $state(false);
  /** Directories to auto-save images to when manualSaveMode is enabled. */
  autoSaveDirs = $state<string[]>([]);
  regionalPrompts = $state<RegionalPromptSelection[]>([]);
  /** SDXL/Illustrious: conditioning areas vs sequential inpaint. Anima always uses inpaint chain. */
  regionalPromptStrategy = $state<RegionalPromptStrategy>("conditioning");

  /** Whether the developer mode section in Settings has been unlocked (10 version taps). Not persisted. */
  devModeUnlocked = $state(false);
  /** Developer mode: bypasses checkpoint selector restrictions. Not persisted. */
  devMode = $state(false);
  /** Show the terminal log panel in the sidebar. Not persisted. */
  showTerminalLog = $state(false);

  /** Raw ModelSpec prediction type signal (e.g. "v", "epsilon"). */
  modelspecPredictionType = $state<string | null>(null);
  /** Alternate ModelSpec predict key used by some files. */
  modelspecPredictKey = $state<string | null>(null);
  /** True when the safetensors header has a top-level `v_pred` entry. */
  modelspecHeaderVPred = $state(false);
  /** Model family resolved by the backend from sidecars/CivitAI. */
  modelFamily = $state<ModelFamily>("unknown");
  /** Backend-resolved SDXL-like family bucket. */
  modelIsSdxlLike = $state(false);
  /** Backend-resolved turbo/lightning/lcm/hyper/dmd model variant. */
  modelTurboVariant = $state<TurboModelVariant>("none");
  /** Identity of the model the family preset was last applied for. Persisted so a
   *  generation-page remount (tab switch) or app restart doesn't re-apply model
   *  defaults over the user's tweaked settings. */
  modelPresetAppliedKey = $state<string | null>(null);
  /** Backend-resolved recommended VAE for split-model pipelines. */
  modelRecommendedVae = $state<string | null>(null);
  /** Backend-resolved recommended text encoder for split-model pipelines. */
  modelRecommendedClipModel = $state<string | null>(null);
  /** Backend-resolved CLIPLoader type for split-model pipelines. */
  modelRecommendedClipType = $state<string | null>(null);
  /** Manual per-model family override keyed by `category::filename`. */
  modelFamilyOverrides = $state<Record<string, ModelFamily>>({});
  get mode(): GenerationMode {
    return this._mode;
  }

  set mode(mode: GenerationMode) {
    this.setMode(mode);
  }

  setMode(mode: GenerationMode): void {
    if (mode === this._mode) return;

    this.modeToggles = {
      ...this.modeToggles,
      [this._mode]: this.readModeToggleState(),
    };
    this._mode = mode;
    this.applyModeToggleState(this.modeToggles[mode] ?? defaultModeToggleState());
  }

  readModeToggleState(): ModeToggleState {
    return {
      differentialDiffusion: this.differentialDiffusion,
      upscaleEnabled: this.upscaleEnabled,
      controlnetEnabled: this.controlnetEnabled,
      facefixEnabled: this.facefixEnabled,
      smartGuidance: this.smartGuidance,
    };
  }

  modeTogglesWithCurrent(): ModeToggleStates {
    return {
      ...this.modeToggles,
      [this._mode]: this.readModeToggleState(),
    };
  }

  applyModeToggleState(state: ModeToggleState): void {
    this.differentialDiffusion = state.differentialDiffusion;
    this.upscaleEnabled = state.upscaleEnabled;
    this.controlnetEnabled = state.controlnetEnabled;
    this.facefixEnabled = state.facefixEnabled;
    this.smartGuidance = state.smartGuidance;
  }

  /** True when the selected model is an Anima variant (split diffusion model). */
  get isAnima(): boolean {
    return this.modelFamily === "anima";
  }

  /** True when the selected model is an Illustrious/NoobAI family variant. */
  get isIllustrious(): boolean {
    return this.modelFamily === "illustrious";
  }

  /** True when the selected model is an SD3/SD3.5 variant. */
  get isSd3(): boolean {
    return this.modelFamily === "sd3";
  }

  /** True when the selected model is a Flux-family variant. */
  get isFlux(): boolean {
    return [
      "flux",
      "flux1d",
      "flux1s",
      "flux1krea",
      "chroma",
    ].includes(this.modelFamily);
  }

  /** True when the selected model is a Flux.2-family variant. */
  get isFlux2(): boolean {
    return [
      "flux2d",
      "flux2klein9b",
      "flux2klein9bbase",
      "flux2klein4b",
      "flux2klein4bbase",
    ].includes(this.modelFamily);
  }

  /** True when the selected model is a Z-Image Base variant. */
  get isZib(): boolean {
    return this.modelFamily === "zib";
  }

  /** True when the selected model is a Z-Image Turbo variant. */
  get isZit(): boolean {
    return this.modelFamily === "zit";
  }

  /** True when the selected model is a Wan variant. */
  get isWan(): boolean {
    return this.modelFamily === "wan";
  }

  /** True when the selected model is a Qwen variant. */
  get isQwen(): boolean {
    return this.modelFamily === "qwen";
  }

  /** True when the selected model is a Pony Diffusion variant. */
  get isPony(): boolean {
    return this.modelFamily === "pony";
  }

  /** True when the selected model is AuraFlow. */
  get isAuraFlow(): boolean {
    return this.modelFamily === "auraflow";
  }

  /** True when the selected model is PixArt. */
  get isPixArt(): boolean {
    return this.modelFamily === "pixart";
  }

  /** True when the selected model is HunyuanDiT. */
  get isHunyuanDit(): boolean {
    return this.modelFamily === "hunyuandit";
  }

  /** True when the selected model is Stable Cascade. */
  get isCascade(): boolean {
    return this.modelFamily === "cascade";
  }

  /** True when the selected model is Kolors. */
  get isKolors(): boolean {
    return this.modelFamily === "kolors";
  }

  /** True when the selected model is Mugen (SDXL with Flux2 VAE + rectified flow). */
  get isMugen(): boolean {
    return this.modelFamily === "mugen";
  }

  /** True when the selected model is Nanosaur (custom 1.2B DiT with DINOv3 VAE). */
  get isNanosaur(): boolean {
    return this.modelFamily === "nanosaur";
  }

  /** True when the model belongs to the SDXL-like family bucket. */
  get isSdxlLike(): boolean {
    return this.modelIsSdxlLike;
  }

  /** True when the selected model uses a fast/turbo-style variant preset. */
  get hasTurboModelVariant(): boolean {
    return this.modelTurboVariant !== "none";
  }

  /** True when the selected model family ignores negative prompts. */
  get disablesNegativePrompt(): boolean {
    return [
      "flux1d",
      "flux1s",
      "flux1krea",
      "zit",
      "flux2klein9b",
      "flux2klein4b",
    ].includes(this.modelFamily);
  }

  /** True when the model uses rectified flow scheduling (SD3, Flux, AuraFlow, Mugen, Nanosaur). */
  get usesRectifiedFlow(): boolean {
    return this.isSd3 || this.isFlux || this.isAuraFlow || this.isMugen || this.isNanosaur;
  }

  private modelFamilySignals() {
    return {
      filename: this.diffusionModel ?? this.checkpoint,
      modelspecPredictionType: this.modelspecPredictionType,
      modelspecPredictKey: this.modelspecPredictKey,
      headerVPred: this.modelspecHeaderVPred,
      modelFamily: this.modelFamily,
    };
  }

  /**
   * Apply runtime metadata after async load and refresh autocomplete tags.
   */
  applyModelMetadata(meta: {
    modelspecPredictionType?: string | null;
    modelspecPredictKey?: string | null;
    modelspecHeaderVPred?: boolean;
    modelFamily?: ModelFamily | null;
    modelIsSdxlLike?: boolean;
    modelTurboVariant?: TurboModelVariant | null;
    modelRecommendedVae?: string | null;
    modelRecommendedClipModel?: string | null;
    modelRecommendedClipType?: string | null;
  }) {
    if (meta.modelspecPredictionType !== undefined) {
      this.modelspecPredictionType = meta.modelspecPredictionType;
    }
    if (meta.modelspecPredictKey !== undefined) {
      this.modelspecPredictKey = meta.modelspecPredictKey;
    }
    if (meta.modelspecHeaderVPred !== undefined) {
      this.modelspecHeaderVPred = meta.modelspecHeaderVPred;
    }
    if (meta.modelFamily !== undefined) {
      this.modelFamily = meta.modelFamily ?? "unknown";
    }
    if (meta.modelIsSdxlLike !== undefined) {
      this.modelIsSdxlLike = meta.modelIsSdxlLike;
    }
    if (meta.modelTurboVariant !== undefined) {
      this.modelTurboVariant = meta.modelTurboVariant ?? "none";
    }
    if (meta.modelRecommendedVae !== undefined) {
      this.modelRecommendedVae = meta.modelRecommendedVae ?? null;
    }
    if (meta.modelRecommendedClipModel !== undefined) {
      this.modelRecommendedClipModel = meta.modelRecommendedClipModel ?? null;
    }
    if (meta.modelRecommendedClipType !== undefined) {
      this.modelRecommendedClipType = meta.modelRecommendedClipType ?? null;
    }
    autocomplete.notifyModelChanged(this.isAnima);
  }

  setModelFamilyOverride(modelKey: string, family: ModelFamily | null): void {
    const next = { ...this.modelFamilyOverrides };
    if (!family) {
      delete next[modelKey];
    } else {
      next[modelKey] = family;
    }
    this.modelFamilyOverrides = next;
    this.saveSettings();
  }

  ensureRecommendedSplitClip(encoders: string[], save = false): void {
    if (!this.useSplitModel) return;

    const recommendedModel = this.modelRecommendedClipModel?.trim();
    const recommendedType = this.modelRecommendedClipType?.trim();
    if (!recommendedModel || !recommendedType) return;

    const currentModel = this.clipModel?.trim() ?? "";
    const currentType = this.clipType?.trim() ?? "";
    const currentMissing = !!currentModel && !encoders.includes(currentModel);

    if (!currentModel || currentMissing || currentType !== recommendedType) {
      this.clipModel = recommendedModel;
      this.clipType = recommendedType;
      if (save) this.saveSettings();
    }
  }

  ensureRecommendedSplitVae(vaes: string[], save = false): void {
    if (!this.useSplitModel) return;

    const recommended = this.modelRecommendedVae?.trim();
    if (!recommended) return;

    const current = this.vae.trim();
    if (current !== recommended) {
      this.vae = recommended;
      if (save) this.saveSettings();
    }
  }

  /** SDXL-style area conditioning (ConditioningSetArea). */
  get supportsRegionalConditioning(): boolean {
    if (this.mode !== "txt2img") return false;
    return this.isSdxlLike;
  }

  /** Sequential masked inpaint per region (works on Anima + optional SDXL). */
  get supportsRegionalInpaintChain(): boolean {
    return this.mode === "txt2img" && (this.isAnima || this.supportsRegionalConditioning);
  }

  get effectiveRegionalStrategy(): RegionalPromptStrategy {
    if (!this.supportsRegionalInpaintChain && !this.supportsRegionalConditioning) {
      return "conditioning";
    }
    if (this.isAnima) return "inpaint_chain";
    if (this.supportsRegionalConditioning && this.regionalPromptStrategy === "conditioning") {
      return "conditioning";
    }
    return "inpaint_chain";
  }

  get canChooseRegionalStrategy(): boolean {
    return this.supportsRegionalConditioning && this.supportsRegionalInpaintChain && !this.isAnima;
  }

  /** txt2img regional prompting (GUI regions + <region> tags). */
  get supportsRegionalPrompting(): boolean {
    return this.supportsRegionalConditioning || this.supportsRegionalInpaintChain;
  }

  /** GUI + inline `<region>` tags with valid geometry and prompt text (for inpaint chain). */
  getValidRegionalSelectionsForInpaint(): RegionalPromptSelection[] {
    const fromGui = this.regionalPrompts.filter(
      (r) => r.text.trim() && r.width > 0 && r.height > 0,
    );
    if (fromGui.length > 0) {
      const seen = new Set<string>();
      return fromGui.filter((r) => {
        const key = r.id || `${r.x},${r.y},${r.width},${r.height},${r.text.trim()}`;
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
    }
    const parsed = parseRegionalPrompt(this.positivePrompt);
    return parsed.regions.map((region, index) => ({
      id: `region-tag-${index}`,
      shape: "box" as const,
      text: region.text,
      strength: 1,
      x: region.x,
      y: region.y,
      width: region.width,
      height: region.height,
    }));
  }

  private _storeReady = false;

  constructor() {
    this.loadPromptHistory();
  }

  get stylePresetOptions(): StylePreset[] {
    return STYLE_PRESETS;
  }

  private splitTags(text: string): string[] {
    return text
      .split(",")
      .map((part) => part.trim())
      .filter((part) => !!part);
  }

  private mergeTagPrompts(base: string, extra: string): string {
    if (!extra) return base;
    const existing = this.splitTags(base);
    const seen = new Set(existing.map((tag) => tag.toLowerCase()));
    const merged = [...existing];

    for (const tag of this.splitTags(extra)) {
      const normalized = tag.toLowerCase();
      if (!seen.has(normalized)) {
        seen.add(normalized);
        merged.push(tag);
      }
    }

    return merged.join(", ");
  }

  private loadPromptHistory() {
    try {
      const raw = localStorage.getItem(PROMPT_HISTORY_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as PromptHistoryEntry[];
      if (!Array.isArray(parsed)) return;
      this.promptHistory = parsed
        .filter((entry) => !!entry?.id)
        .slice(0, MAX_PROMPT_HISTORY);
    } catch (e) {
      console.error("Failed to load prompt history:", e);
    }
  }

  private savePromptHistory() {
    try {
      localStorage.setItem(PROMPT_HISTORY_KEY, JSON.stringify(this.promptHistory.slice(0, MAX_PROMPT_HISTORY)));
      triggerSync();
    } catch (e) {
      console.error("Failed to save prompt history:", e);
    }
  }

  saveCurrentPromptToHistory() {
    const positivePrompt = this.positivePrompt.trim();
    const negativePrompt = this.negativePrompt.trim();
    if (!positivePrompt && !negativePrompt) return;

    const existing = this.promptHistory.find(
      (entry) =>
        entry.positivePrompt === positivePrompt &&
        entry.negativePrompt === negativePrompt &&
        entry.mode === this.mode &&
        entry.stylePreset === this.stylePreset
    );

    const nextEntry: PromptHistoryEntry = {
      id: existing?.id ?? (crypto.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`),
      positivePrompt,
      negativePrompt,
      mode: this.mode,
      stylePreset: this.stylePreset,
      createdAt: Date.now(),
      favorite: existing?.favorite ?? false,
    };

    this.promptHistory = [
      nextEntry,
      ...this.promptHistory.filter((entry) => entry.id !== nextEntry.id),
    ].slice(0, MAX_PROMPT_HISTORY);

    this.savePromptHistory();
  }

  togglePromptFavorite(id: string) {
    this.promptHistory = this.promptHistory.map((entry) =>
      entry.id === id ? { ...entry, favorite: !entry.favorite } : entry
    );
    this.savePromptHistory();
  }

  removePromptHistoryEntry(id: string) {
    this.promptHistory = this.promptHistory.filter((entry) => entry.id !== id);
    this.savePromptHistory();
  }

  applyPromptHistoryEntry(id: string) {
    const entry = this.promptHistory.find((item) => item.id === id);
    if (!entry) return;

    this.positivePrompt = entry.positivePrompt;
    this.negativePrompt = entry.negativePrompt;
    this.mode = entry.mode;
    this.stylePreset = entry.stylePreset;

    this.promptHistory = [
      { ...entry, createdAt: Date.now() },
      ...this.promptHistory.filter((item) => item.id !== entry.id),
    ];
    this.savePromptHistory();
  }

  private resolveAvailableOption(options: string[], preferred: string, fallback: string): string {
    if (options.includes(preferred)) return preferred;
    if (options.includes(fallback)) return fallback;
    return options[0] ?? preferred;
  }

  private applyResolvedPreset(preset: ModelPreset) {
    this.steps = preset.steps;
    this.cfg = preset.cfg;
    this.samplerName = this.resolveAvailableOption(models.samplers, preset.samplerName, "euler");
    this.scheduler = this.resolveAvailableOption(models.schedulers, preset.scheduler, "normal");
    this.width = preset.width;
    this.height = preset.height;
    this.facefixSteps = Math.ceil(preset.steps / 3);
    this.upscaleSteps = Math.ceil(preset.steps / 3);
    if (preset.upscaleDenoise !== undefined) {
      this.upscaleDenoise = preset.upscaleDenoise;
    }
  }

  applyModelSpecificPreset() {
    const isAnimaLike = this.isAnima || this.isWan || this.isQwen;
    autocomplete.notifyModelChanged(isAnimaLike);

    // Only apply defaults when the selected model actually changed. Metadata
    // reloads for the same model (page remount on tab switch, app restart)
    // must not clobber settings the user has tweaked since.
    const presetKey = [
      this.useSplitModel && this.diffusionModel ? `dm:${this.diffusionModel}` : `cp:${this.checkpoint}`,
      this.modelFamily,
      this.modelTurboVariant,
    ].join("|");
    if (presetKey === this.modelPresetAppliedKey) return;
    this.modelPresetAppliedKey = presetKey;

    let preset: ModelPreset;
    switch (this.modelFamily) {
      // Nanosaur uses a custom DiT/VAE combo and prefers a taller default canvas.
      case "nanosaur":
        preset = {
          steps: 40,
          cfg: 7,
          samplerName: "euler",
          scheduler: "simple",
          width: 896,
          height: 1152,
          upscaleDenoise: 0.5,
        };
        break;

      // SD3 family prefers moderate CFG with SGM uniform scheduling.
      case "sd3":
        preset = {
          steps: this.modelTurboVariant === "turbo" ? 6 : 28,
          cfg: this.modelTurboVariant === "turbo" ? 1.0 : 4.5,
          samplerName: "euler",
          scheduler: "sgm_uniform",
          width: 1024,
          height: 1024,
        };
        break;

      // Flux is guidance-distilled, so keep CFG low and scheduler simple.
      case "flux1d":
        preset = {
          steps: 20,
          cfg: 1.0,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
        };
        break;

      // Flux flux1krea and chroma.
      case "flux1krea":
      case "chroma":
        preset = {
          steps: 20,
          cfg: 3.0,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
        };
        break;

      // Flux.1 Schnell is a separate distilled family.
      case "flux1s":
        preset = {
          steps: 4,
          cfg: 1.0,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
        };
        break;

      // Flux.2 base.
      case "flux2d":
      case "flux2klein9bbase":
      case "flux2klein4bbase":
        preset = {
          steps: 20,
          cfg: 4.0,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
        };
        break;

      // Flux.2 klein.
      case "flux2klein9b":
      case "flux2klein4b":
        preset = {
          steps: 9,
          cfg: 1.5,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
        };
        break;

      // Z-Image Base defaults.
      case "zib":
        preset = {
          steps: 30,
          cfg: 4.0,
          samplerName: "euler",
          scheduler: "normal",
          width: 1024,
          height: 1024,
        };
        break;

      // Z-Image Turbo defaults.
      case "zit":
        preset = {
          steps: 8,
          cfg: 1.0,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
        };
        break;

      // AuraFlow defaults target rectified-flow style inference.
      case "auraflow":
        preset = {
          steps: 28,
          cfg: 3.5,
          samplerName: "euler",
          scheduler: "normal",
          width: 1024,
          height: 1024,
        };
        break;

      // PixArt ships best with conservative Euler-based defaults.
      case "pixart":
        preset = {
          steps: 20,
          cfg: 4.5,
          samplerName: "euler",
          scheduler: "normal",
          width: 1024,
          height: 1024,
        };
        break;

      // HunyuanDiT benefits from a higher step/count CFG preset.
      case "hunyuandit":
        preset = {
          steps: 30,
          cfg: 6.0,
          samplerName: "euler",
          scheduler: "normal",
          width: 1024,
          height: 1024,
        };
        break;

      // Stable Cascade keeps a simple scheduler preset for the base stage.
      case "cascade":
        preset = {
          steps: 20,
          cfg: 4.0,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
        };
        break;

      // Kolors stays in the SDXL-like resolution bucket with a slightly higher CFG.
      case "kolors":
        preset = {
          steps: 25,
          cfg: 5.0,
          samplerName: "euler",
          scheduler: "normal",
          width: 1024,
          height: 1024,
        };
        break;

      // SD 1.5 keeps the smaller canvas and classic DPM++/Karras combo.
      case "sd15":
        preset = {
          steps: this.hasTurboModelVariant ? 8 : 20,
          cfg: this.hasTurboModelVariant ? 1.5 : 5.0,
          samplerName: this.hasTurboModelVariant ? "euler" : "dpmpp_2m",
          scheduler: this.hasTurboModelVariant ? "normal" : "karras",
          width: 512,
          height: 512,
        };
        break;


      case "pony":
        preset = {
          steps: this.hasTurboModelVariant ? 10 : 25,
          cfg: this.hasTurboModelVariant ? 1.0 : 6.0,
          samplerName: this.hasTurboModelVariant ? "euler" : "euler_a",
          scheduler: "normal",
          width: 1024,
          height: 1024,
        };
        break;

      case "illustrious":
        preset = {
          steps: this.hasTurboModelVariant ? 10 : 20,
          cfg: this.hasTurboModelVariant ? 1.0 : 5.0,
          samplerName: this.hasTurboModelVariant ? "euler" : "euler_cfg_pp",
          scheduler: this.hasTurboModelVariant ? "normal" : "sgm_uniform",
          width: 1024,
          height: 1024,
        };
        break;

      // Anima, Wan, and Qwen share the same 16-channel latent workflow bucket.
      case "anima":
      case "wan":
      case "qwen":
        preset = {
          steps: 30,
          cfg: 4.0,
          samplerName: "er_sde",
          scheduler: "sgm_uniform",
          width: 1024,
          height: 1024,
        };
        break;

      case "sdxl":
      case "mugen":
      case "unknown":
      default:
        preset = {
          steps: this.hasTurboModelVariant ? 10 : 20,
          cfg: this.hasTurboModelVariant ? 1.0 : 5.0,
          samplerName: this.hasTurboModelVariant ? "euler" : "euler_cfg_pp",
          scheduler: this.hasTurboModelVariant ? "normal" : "sgm_uniform",
          width: 1024,
          height: 1024,
        };
        break;
    }

    this.applyResolvedPreset(preset);
  }

  async loadSettings() {
    try {
      this._storeReady = true;
      const saved = await ipcStore.get<Record<string, any>>(STORE_KEY);
      if (saved) {
        const savedMode = isGenerationMode(saved.mode) ? saved.mode : this._mode;
        if (saved.checkpoint) this.checkpoint = saved.checkpoint;
        if (saved.modelPresetAppliedKey !== undefined) this.modelPresetAppliedKey = saved.modelPresetAppliedKey;
        if (saved.vae !== undefined) this.vae = saved.vae;
        if (saved.samplerName) this.samplerName = saved.samplerName;
        if (saved.scheduler) this.scheduler = saved.scheduler;
        if (saved.steps) this.steps = saved.steps;
        if (saved.cfg !== undefined) this.cfg = saved.cfg;
        if (saved.seed !== undefined) this.seed = saved.seed;
        if (saved.width) this.width = saved.width;
        if (saved.height) this.height = saved.height;
        if (saved.batchSize) this.batchSize = saved.batchSize;
        if (saved.denoise !== undefined) this.denoise = saved.denoise;
        if (saved.differentialDiffusion !== undefined) this.differentialDiffusion = saved.differentialDiffusion;
        if (saved.positivePrompt) this.positivePrompt = saved.positivePrompt;
        if (saved.negativePrompt) this.negativePrompt = saved.negativePrompt;
        if (Array.isArray(saved.loras)) {
          this.loras = saved.loras.map((l: any) => ({
            name: l.name || "",
            strength_model: l.strength_model ?? 1.0,
            strength_clip: l.strength_clip ?? 1.0,
            enabled: l.enabled ?? true,
          }));
        }
        if (saved.upscaleEnabled !== undefined) this.upscaleEnabled = saved.upscaleEnabled;
        if (saved.upscaleMethod) this.upscaleMethod = saved.upscaleMethod;
        if (saved.upscaleModel !== undefined) this.upscaleModel = saved.upscaleModel;
        if (saved.upscaleScale !== undefined) this.upscaleScale = saved.upscaleScale;
        if (saved.upscaleDenoise !== undefined) this.upscaleDenoise = saved.upscaleDenoise;
        if (saved.upscaleSteps !== undefined) this.upscaleSteps = saved.upscaleSteps;
        if (saved.upscaleTileSize !== undefined) this.upscaleTileSize = saved.upscaleTileSize;
        if (saved.upscaleTiling !== undefined) this.upscaleTiling = saved.upscaleTiling;
        if (saved.upscaleFastRefine !== undefined) this.upscaleFastRefine = saved.upscaleFastRefine;
        if (saved.upscaleSoftGuidance !== undefined) this.upscaleSoftGuidance = saved.upscaleSoftGuidance;
        if (saved.upscaleSoftGuidanceMultiplier !== undefined) this.upscaleSoftGuidanceMultiplier = saved.upscaleSoftGuidanceMultiplier;
        if (saved.refineOnly !== undefined) this.refineOnly = saved.refineOnly;
        if (saved.savePreUpscaleImage !== undefined) this.savePreUpscaleImage = saved.savePreUpscaleImage;
        if (saved.smartGuidance !== undefined) this.smartGuidance = saved.smartGuidance;
        if (saved.fluxGuidance !== undefined) this.fluxGuidance = saved.fluxGuidance;
        if (saved.useSplitModel !== undefined) this.useSplitModel = saved.useSplitModel;
        if (saved.diffusionModel !== undefined) this.diffusionModel = saved.diffusionModel;
        if (saved.clipModel !== undefined) this.clipModel = saved.clipModel;
        if (saved.clipType !== undefined) this.clipType = saved.clipType;
        if (saved.stylePreset !== undefined) this.stylePreset = saved.stylePreset;
        if (saved.stylePresetsEnabled !== undefined) this.stylePresetsEnabled = !!saved.stylePresetsEnabled;
        if (saved.controlnetEnabled !== undefined) this.controlnetEnabled = saved.controlnetEnabled;
        if (saved.controlnetMode) this.controlnetMode = saved.controlnetMode;
        if (saved.controlnetPreset !== undefined) this.controlnetPreset = saved.controlnetPreset;
        if (saved.controlnetModel !== undefined) this.controlnetModel = saved.controlnetModel;
        if (saved.controlnetPreprocessor !== undefined) this.controlnetPreprocessor = saved.controlnetPreprocessor;
        if (saved.controlnetStrength !== undefined) this.controlnetStrength = saved.controlnetStrength;
        if (saved.controlnetStartPercent !== undefined) this.controlnetStartPercent = saved.controlnetStartPercent;
        if (saved.controlnetEndPercent !== undefined) this.controlnetEndPercent = saved.controlnetEndPercent;
        if (saved.styleTransferEnabled !== undefined) this.styleTransferEnabled = saved.styleTransferEnabled;
        if (saved.styleReferenceImage !== undefined) this.styleReferenceImage = saved.styleReferenceImage;
        if (saved.styleTransferLowScaleEnd !== undefined) this.styleTransferLowScaleEnd = saved.styleTransferLowScaleEnd;
        if (saved.styleTransferHighScaleStart !== undefined) this.styleTransferHighScaleStart = saved.styleTransferHighScaleStart;
        if (saved.styleTransferBeta !== undefined) this.styleTransferBeta = saved.styleTransferBeta;
        if (saved.styleTransferAdainStrength !== undefined) this.styleTransferAdainStrength = saved.styleTransferAdainStrength;
        if (saved.styleTransferRfMode !== undefined) this.styleTransferRfMode = saved.styleTransferRfMode;
        if (saved.styleTransferGamma !== undefined) this.styleTransferGamma = saved.styleTransferGamma;
        if (saved.styleTransferGammaCurve !== undefined) this.styleTransferGammaCurve = saved.styleTransferGammaCurve;
        if (saved.styleTransferNormStrength !== undefined) this.styleTransferNormStrength = saved.styleTransferNormStrength;
        if (saved.styleTransferPmiAlpha !== undefined) this.styleTransferPmiAlpha = saved.styleTransferPmiAlpha;
        if (saved.styleTransferMegapixels !== undefined) this.styleTransferMegapixels = saved.styleTransferMegapixels;
        if (saved.styleTransferBlocks !== undefined) this.styleTransferBlocks = saved.styleTransferBlocks;
        if (saved.facefixEnabled !== undefined) this.facefixEnabled = saved.facefixEnabled;
        if (saved.facefixDetector !== undefined) this.facefixDetector = saved.facefixDetector;
        if (saved.facefixDenoise !== undefined) this.facefixDenoise = saved.facefixDenoise;
        if (saved.facefixSteps !== undefined) this.facefixSteps = saved.facefixSteps;
        if (saved.facefixGuideSize !== undefined) this.facefixGuideSize = saved.facefixGuideSize;
        if (saved.facefixMaxFaces !== undefined) this.facefixMaxFaces = saved.facefixMaxFaces;
        if (saved.facefixAutoPrompt !== undefined) this.facefixAutoPrompt = saved.facefixAutoPrompt;
        if (saved.modeToggles !== undefined) {
          this.modeToggles = normalizeModeToggles(saved.modeToggles);
        } else {
          this.modeToggles = {
            ...createDefaultModeToggles(),
            [savedMode]: this.readModeToggleState(),
          };
        }
        this._mode = savedMode;
        this.applyModeToggleState(this.modeToggles[savedMode] ?? defaultModeToggleState());
        if (saved.outputBitDepth) this.outputBitDepth = saved.outputBitDepth;
        if (saved.outputFormat === "png" || saved.outputFormat === "jxl") this.outputFormat = saved.outputFormat;
        if (saved.metadataMode) this.metadataMode = saved.metadataMode;
        if (saved.autoQualityTags !== undefined) this.autoQualityTags = saved.autoQualityTags;
        if (saved.customAnimaPositiveQuality !== undefined) this.customAnimaPositiveQuality = saved.customAnimaPositiveQuality;
        if (saved.customAnimaNegativeQuality !== undefined) this.customAnimaNegativeQuality = saved.customAnimaNegativeQuality;
        if (saved.customIllustriousPositiveQuality !== undefined) this.customIllustriousPositiveQuality = saved.customIllustriousPositiveQuality;
        if (saved.customIllustriousNegativeQuality !== undefined) this.customIllustriousNegativeQuality = saved.customIllustriousNegativeQuality;
        if (saved.customPonyPositiveQuality !== undefined) this.customPonyPositiveQuality = saved.customPonyPositiveQuality;
        if (saved.customPonyNegativeQuality !== undefined) this.customPonyNegativeQuality = saved.customPonyNegativeQuality;
        if (saved.customNanosaurPositiveQuality !== undefined) this.customNanosaurPositiveQuality = saved.customNanosaurPositiveQuality;
        if (saved.customNanosaurNegativeQuality !== undefined) this.customNanosaurNegativeQuality = saved.customNanosaurNegativeQuality;
        if (saved.modelFamilyOverrides && typeof saved.modelFamilyOverrides === "object") {
          this.modelFamilyOverrides = Object.fromEntries(
            Object.entries(saved.modelFamilyOverrides as Record<string, unknown>).filter(
              ([key, value]) => !!key && isModelFamily(value) && value !== "unknown",
            ),
          ) as Record<string, ModelFamily>;
        }
        if (saved.manualSaveMode !== undefined) this.manualSaveMode = saved.manualSaveMode;
        if (Array.isArray(saved.autoSaveDirs)) this.autoSaveDirs = saved.autoSaveDirs;
        if (saved.regionalPromptStrategy === "conditioning" || saved.regionalPromptStrategy === "inpaint_chain") {
          this.regionalPromptStrategy = saved.regionalPromptStrategy;
        }
        if (Array.isArray(saved.regionalPrompts)) {
          this.regionalPrompts = saved.regionalPrompts
            .filter((item: unknown) => !!item && typeof item === "object")
            .map((item: any) => ({
              id: typeof item.id === "string" && item.id ? item.id : (crypto.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`),
              shape: item.shape === "circle" || item.shape === "lasso" ? item.shape : "box",
              text: typeof item.text === "string" ? item.text : "",
              strength: typeof item.strength === "number" ? item.strength : 1.0,
              x: typeof item.x === "number" ? item.x : 0,
              y: typeof item.y === "number" ? item.y : 0,
              width: typeof item.width === "number" ? item.width : 0,
              height: typeof item.height === "number" ? item.height : 0,
              points: Array.isArray(item.points)
                ? item.points
                    .filter((point: unknown) => !!point && typeof point === "object")
                    .map((point: any) => ({
                      x: typeof point.x === "number" ? point.x : 0,
                      y: typeof point.y === "number" ? point.y : 0,
                    }))
                : undefined,
            }));
        }
        // Migrate: old default was "text_chunk", new default is "both" (stealth + text)
        if (!localStorage.getItem("mooshieui.metadataMode.v2")) {
          this.metadataMode = "both";
          localStorage.setItem("mooshieui.metadataMode.v2", "1");
        }
        console.log("Loaded saved settings, checkpoint:", this.checkpoint);
        // Sync autocomplete tag list with restored model
        autocomplete.notifyModelChanged(this.isAnima);
      }
    } catch (e) {
      console.error("Failed to load settings:", e);
    }
  }

  async saveSettings() {
    if (!this._storeReady) return;
    try {
      const modeToggles = this.modeTogglesWithCurrent();
      this.modeToggles = modeToggles;
      await ipcStore.set(STORE_KEY, {
        mode: this.mode,
        modeToggles,
        positivePrompt: this.positivePrompt,
        negativePrompt: this.negativePrompt,
        checkpoint: this.checkpoint,
        modelPresetAppliedKey: this.modelPresetAppliedKey,
        vae: this.vae,
        loras: this.loras,
        samplerName: this.samplerName,
        scheduler: this.scheduler,
        steps: this.steps,
        cfg: this.cfg,
        seed: this.seed,
        width: this.width,
        height: this.height,
        batchSize: this.batchSize,
        denoise: this.denoise,
        differentialDiffusion: this.differentialDiffusion,
        upscaleEnabled: this.upscaleEnabled,
        upscaleMethod: this.upscaleMethod,
        upscaleModel: this.upscaleModel,
        upscaleScale: this.upscaleScale,
        upscaleDenoise: this.upscaleDenoise,
        upscaleSteps: this.upscaleSteps,
        upscaleTileSize: this.upscaleTileSize,
        upscaleTiling: this.upscaleTiling,
        upscaleFastRefine: this.upscaleFastRefine,
        upscaleSoftGuidance: this.upscaleSoftGuidance,
        upscaleSoftGuidanceMultiplier: this.upscaleSoftGuidanceMultiplier,
        refineOnly: this.refineOnly,
        savePreUpscaleImage: this.savePreUpscaleImage,
        smartGuidance: this.smartGuidance,
        fluxGuidance: this.fluxGuidance,
        useSplitModel: this.useSplitModel,
        diffusionModel: this.diffusionModel,
        clipModel: this.clipModel,
        clipType: this.clipType,
        stylePreset: this.stylePreset,
        stylePresetsEnabled: this.stylePresetsEnabled,
        controlnetEnabled: this.controlnetEnabled,
        controlnetMode: this.controlnetMode,
        controlnetPreset: this.controlnetPreset,
        controlnetModel: this.controlnetModel,
        controlnetPreprocessor: this.controlnetPreprocessor,
        controlnetStrength: this.controlnetStrength,
        controlnetStartPercent: this.controlnetStartPercent,
        controlnetEndPercent: this.controlnetEndPercent,
        styleTransferEnabled: this.styleTransferEnabled,
        styleReferenceImage: this.styleReferenceImage,
        styleTransferLowScaleEnd: this.styleTransferLowScaleEnd,
        styleTransferHighScaleStart: this.styleTransferHighScaleStart,
        styleTransferBeta: this.styleTransferBeta,
        styleTransferAdainStrength: this.styleTransferAdainStrength,
        styleTransferRfMode: this.styleTransferRfMode,
        styleTransferGamma: this.styleTransferGamma,
        styleTransferGammaCurve: this.styleTransferGammaCurve,
        styleTransferNormStrength: this.styleTransferNormStrength,
        styleTransferPmiAlpha: this.styleTransferPmiAlpha,
        styleTransferMegapixels: this.styleTransferMegapixels,
        styleTransferBlocks: this.styleTransferBlocks,
        facefixEnabled: this.facefixEnabled,
        facefixDetector: this.facefixDetector,
        facefixDenoise: this.facefixDenoise,
        facefixSteps: this.facefixSteps,
        facefixGuideSize: this.facefixGuideSize,
        facefixMaxFaces: this.facefixMaxFaces,
        facefixAutoPrompt: this.facefixAutoPrompt,
        outputBitDepth: this.outputBitDepth,
        outputFormat: this.outputFormat,
        metadataMode: this.metadataMode,
        autoQualityTags: this.autoQualityTags,
        customAnimaPositiveQuality: this.customAnimaPositiveQuality,
        customAnimaNegativeQuality: this.customAnimaNegativeQuality,
        customIllustriousPositiveQuality: this.customIllustriousPositiveQuality,
        customIllustriousNegativeQuality: this.customIllustriousNegativeQuality,
        customPonyPositiveQuality: this.customPonyPositiveQuality,
        customPonyNegativeQuality: this.customPonyNegativeQuality,
        customNanosaurPositiveQuality: this.customNanosaurPositiveQuality,
        customNanosaurNegativeQuality: this.customNanosaurNegativeQuality,
        modelFamilyOverrides: this.modelFamilyOverrides,
        manualSaveMode: this.manualSaveMode,
        autoSaveDirs: this.autoSaveDirs,
        regionalPrompts: this.regionalPrompts,
        regionalPromptStrategy: this.regionalPromptStrategy,
      });
      triggerSync();
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  }

  /** Collect generation settings for server-side sync. */
  collectPrefs(): Record<string, unknown> {
    const modeToggles = this.modeTogglesWithCurrent();
    return {
      mode: this.mode,
      modeToggles,
      positivePrompt: this.positivePrompt,
      negativePrompt: this.negativePrompt,
      checkpoint: this.checkpoint,
      modelPresetAppliedKey: this.modelPresetAppliedKey,
      vae: this.vae,
      loras: this.loras,
      samplerName: this.samplerName,
      scheduler: this.scheduler,
      steps: this.steps,
      cfg: this.cfg,
      seed: this.seed,
      width: this.width,
      height: this.height,
      batchSize: this.batchSize,
      denoise: this.denoise,
      differentialDiffusion: this.differentialDiffusion,
      upscaleEnabled: this.upscaleEnabled,
      upscaleMethod: this.upscaleMethod,
      upscaleModel: this.upscaleModel,
      upscaleScale: this.upscaleScale,
      upscaleDenoise: this.upscaleDenoise,
      upscaleSteps: this.upscaleSteps,
      upscaleTileSize: this.upscaleTileSize,
      upscaleTiling: this.upscaleTiling,
      upscaleFastRefine: this.upscaleFastRefine,
      upscaleSoftGuidance: this.upscaleSoftGuidance,
      upscaleSoftGuidanceMultiplier: this.upscaleSoftGuidanceMultiplier,
      savePreUpscaleImage: this.savePreUpscaleImage,
      smartGuidance: this.smartGuidance,
      fluxGuidance: this.fluxGuidance,
      useSplitModel: this.useSplitModel,
      diffusionModel: this.diffusionModel,
      clipModel: this.clipModel,
      clipType: this.clipType,
      stylePreset: this.stylePreset,
      stylePresetsEnabled: this.stylePresetsEnabled,
      controlnetEnabled: this.controlnetEnabled,
      controlnetMode: this.controlnetMode,
      controlnetPreset: this.controlnetPreset,
      controlnetModel: this.controlnetModel,
      controlnetPreprocessor: this.controlnetPreprocessor,
      controlnetStrength: this.controlnetStrength,
      controlnetStartPercent: this.controlnetStartPercent,
      controlnetEndPercent: this.controlnetEndPercent,
      styleTransferEnabled: this.styleTransferEnabled,
      styleReferenceImage: this.styleReferenceImage,
      styleTransferLowScaleEnd: this.styleTransferLowScaleEnd,
      styleTransferHighScaleStart: this.styleTransferHighScaleStart,
      styleTransferBeta: this.styleTransferBeta,
      styleTransferAdainStrength: this.styleTransferAdainStrength,
      styleTransferRfMode: this.styleTransferRfMode,
      styleTransferGamma: this.styleTransferGamma,
      styleTransferGammaCurve: this.styleTransferGammaCurve,
      styleTransferNormStrength: this.styleTransferNormStrength,
      styleTransferPmiAlpha: this.styleTransferPmiAlpha,
      styleTransferMegapixels: this.styleTransferMegapixels,
      facefixEnabled: this.facefixEnabled,
      facefixDetector: this.facefixDetector,
      facefixDenoise: this.facefixDenoise,
      facefixSteps: this.facefixSteps,
      facefixGuideSize: this.facefixGuideSize,
      facefixMaxFaces: this.facefixMaxFaces,
      facefixAutoPrompt: this.facefixAutoPrompt,
      outputBitDepth: this.outputBitDepth,
      outputFormat: this.outputFormat,
      metadataMode: this.metadataMode,
      autoQualityTags: this.autoQualityTags,
      customAnimaPositiveQuality: this.customAnimaPositiveQuality,
      customAnimaNegativeQuality: this.customAnimaNegativeQuality,
      customIllustriousPositiveQuality: this.customIllustriousPositiveQuality,
      customIllustriousNegativeQuality: this.customIllustriousNegativeQuality,
      customPonyPositiveQuality: this.customPonyPositiveQuality,
      customPonyNegativeQuality: this.customPonyNegativeQuality,
      customNanosaurPositiveQuality: this.customNanosaurPositiveQuality,
      customNanosaurNegativeQuality: this.customNanosaurNegativeQuality,
      manualSaveMode: this.manualSaveMode,
      autoSaveDirs: this.autoSaveDirs,
      regionalPrompts: this.regionalPrompts,
      regionalPromptStrategy: this.regionalPromptStrategy,
    };
  }

  /** Collect prompt history for server-side sync. */
  collectPromptHistory(): unknown[] {
    return this.promptHistory.slice(0, MAX_PROMPT_HISTORY);
  }

  /** Apply generation settings from the server. Writes to ipcStore and re-hydrates. */
  async applyServerPrefs(data: Record<string, any>): Promise<void> {
    try {
      await ipcStore.set(STORE_KEY, data);
      await this.loadSettings();
    } catch (e) {
      console.error("generation: applyServerPrefs failed", e);
    }
  }

  /** Apply prompt history from the server. */
  applyPromptHistory(entries: any[]): void {
    try {
      const valid = entries
        .filter((e) => !!e?.id)
        .slice(0, MAX_PROMPT_HISTORY) as PromptHistoryEntry[];
      localStorage.setItem(PROMPT_HISTORY_KEY, JSON.stringify(valid));
      this.promptHistory = valid;
    } catch (e) {
      console.error("generation: applyPromptHistory failed", e);
    }
  }

  toParams(options: GenerationToParamsOptions = {}) {
    if (this.useSplitModel) {
      if (!this.diffusionModel) {
        throw new Error("Split model is selected, but no diffusion model is resolved yet.");
      }
      if (!this.clipModel) {
        throw new Error("Split model text encoder is still loading.");
      }
      if (!this.clipType) {
        throw new Error("Split model text encoder type is still loading.");
      }
      if (!this.vae) {
        throw new Error("Split model VAE is still loading.");
      }
    }

    const style = this.stylePresetsEnabled
      ? (STYLE_PRESETS.find((preset) => preset.id === this.stylePreset) ?? STYLE_PRESETS[0])
      : STYLE_PRESETS[0];

    // Expand inline `@preset:<slug>` directives in the user-typed prompts
    // first, so wildcard rolls happen before any merging/dedup logic. Each
    // occurrence rolls independently.
    const inlinePositiveIds = promptPresets.inlinePresetIds(this.positivePrompt);
    const inlineNegativeIds = promptPresets.inlinePresetIds(this.negativePrompt);
    const inlinePresetIds = new Set([...inlinePositiveIds, ...inlineNegativeIds]);
    const inlinePositive = promptPresets.resolveInline(this.positivePrompt, {
      fixedChoices: options.fixedPresetChoices,
    });
    const inlineNegative = promptPresets.resolveInline(this.negativePrompt, {
      fixedChoices: options.fixedPresetChoices,
    });

    // Parse <segment:...> auto-refinement tags from the user-typed prompt before
    // system fragments (style presets, artist styles, preset appends, quality
    // tags) are merged in — a trailing-form segment must not swallow them.
    const parsedSegmentDetails = parseSegmentDetailPrompt(inlinePositive);

    let positivePrompt = this.mergeTagPrompts(parsedSegmentDetails.baseText, style.positive);
    let negativePrompt = this.mergeTagPrompts(inlineNegative, style.negative);

    // Inject tags contributed by any currently-active Artist Styles. These are
    // not visible in the prompt textbox — they flow straight into the payload
    // so the user sees badges in the UI instead.
    const styleFragment = styles.buildPromptFragment();
    if (styleFragment) {
      positivePrompt = this.mergeTagPrompts(positivePrompt, styleFragment);
    }

    // Inject active Prompt Presets (prepend / append / wildcard). Wildcards
    // pick a random choice per generation — mergeTagPrompts dedupes against
    // whatever the user has already typed.
    const preset = promptPresets.resolve({
      fixedChoices: options.fixedPresetChoices,
      skipIds: inlinePresetIds,
      advanceFixedOrdered: false,
    });
    if (preset.prepend) {
      positivePrompt = this.mergeTagPrompts(preset.prepend, positivePrompt);
    }
    if (preset.append) {
      positivePrompt = this.mergeTagPrompts(positivePrompt, preset.append);
    }

    // Auto-apply quality tags for supported model families
    if (this.autoQualityTags) {
      // Anima models (positive before, negative after)
      if (this.isAnima) {
        positivePrompt = this.mergeTagPrompts(this.customAnimaPositiveQuality, positivePrompt);
        negativePrompt = this.mergeTagPrompts(negativePrompt, this.customAnimaNegativeQuality);
      }

      // Illustrious/NoobAI family (positive before, negative after)
      if (this.isIllustrious) {
        positivePrompt = this.mergeTagPrompts(this.customIllustriousPositiveQuality, positivePrompt);
        negativePrompt = this.mergeTagPrompts(negativePrompt, this.customIllustriousNegativeQuality);
      }

      // Pony Diffusion (score-based quality tags)
      if (this.isPony) {
        positivePrompt = this.mergeTagPrompts(this.customPonyPositiveQuality, positivePrompt);
        negativePrompt = this.mergeTagPrompts(negativePrompt, this.customPonyNegativeQuality);
      }

      // Nanosaur (newest/oldest quality tags)
      if (this.isNanosaur) {
        positivePrompt = this.mergeTagPrompts(this.customNanosaurPositiveQuality, positivePrompt);
        negativePrompt = this.mergeTagPrompts(negativePrompt, this.customNanosaurNegativeQuality);
      }
    }

    // Build quality-only prompts for tiled upscale (reduces tile seam artifacts)
    let upscalePositivePrompt: string | null = null;
    let upscaleNegativePrompt: string | null = null;
    const upscaleUsesTiling =
      this.upscaleEnabled &&
      !this.upscaleFastRefine &&
      (this.upscaleTiling || this.useSplitModel);
    if (upscaleUsesTiling && this.autoQualityTags) {
      if (this.isAnima) {
        upscalePositivePrompt = this.customAnimaPositiveQuality;
        upscaleNegativePrompt = this.customAnimaNegativeQuality;
      } else if (this.isIllustrious) {
        upscalePositivePrompt = this.customIllustriousPositiveQuality;
        upscaleNegativePrompt = this.customIllustriousNegativeQuality;
      } else if (this.isPony) {
        upscalePositivePrompt = this.customPonyPositiveQuality;
        upscaleNegativePrompt = this.customPonyNegativeQuality;
      } else if (this.isNanosaur) {
        upscalePositivePrompt = this.customNanosaurPositiveQuality;
        upscaleNegativePrompt = this.customNanosaurNegativeQuality;
      }
    }

    const regionalPromptingSupported = this.supportsRegionalPrompting;
    const configuredRegionCount = this.regionalPrompts.filter(
      (r) => r.text.trim() && r.width > 0 && r.height > 0,
    ).length;
    if (configuredRegionCount > 0 && !regionalPromptingSupported) {
      console.warn(
        "[regional] Dropping",
        configuredRegionCount,
        "GUI region(s): unsupported for",
        this.modelFamily,
        "mode",
        this.mode,
        "checkpoint",
        this.checkpoint,
      );
    }
    // Parse syntax-first regional prompting tags before schedule parsing, but only
    // when the current model/mode supports regional prompting. Otherwise keep
    // tags in the main prompt text so user intent is not silently dropped.
    const parsedRegions = regionalPromptingSupported
      ? parseRegionalPrompt(positivePrompt)
      : { baseText: positivePrompt, regions: [] as Array<{ text: string; x: number; y: number; width: number; height: number }> };
    positivePrompt = parsedRegions.baseText;
    const guiRegions = regionalPromptingSupported
      ? this.regionalPrompts
        .map((region) => {
          const x = Math.max(0, Math.min(1, region.x));
          const y = Math.max(0, Math.min(1, region.y));
          const maxWidth = Math.max(0, 1 - x);
          const maxHeight = Math.max(0, 1 - y);
          const width = Math.max(0, Math.min(maxWidth, region.width));
          const height = Math.max(0, Math.min(maxHeight, region.height));
          const text = region.text.trim();
          if (!text || width <= 0 || height <= 0) return null;
          return {
            text,
            x,
            y,
            width,
            height,
            strength: Number.isFinite(region.strength) ? Math.max(0, Math.min(2, region.strength)) : 1.0,
          };
        })
        .filter((region): region is NonNullable<typeof region> => region !== null)
      : [];

    if (this.disablesNegativePrompt) {
      negativePrompt = "";
    }

    // Parse timestep scheduling tags from prompts before NAI weight translation.
    const parsedPositive = parseScheduledPrompt(positivePrompt);
    const parsedNegative = parseScheduledPrompt(negativePrompt);

    const translatedPositiveBase = translateNaiWeightSyntax(parsedPositive.baseText);
    const translatedPositiveSegments = parsedPositive.segments.map((s) => ({
      text: translateNaiWeightSyntax(s.text),
      start: s.start,
      end: s.end,
    }));
    const regionalContext = regionalPromptingSupported
      ? buildRegionalContextPrompt(
          translatedPositiveBase,
          translatedPositiveSegments,
          this.loras.filter((l) => l.enabled && l.name),
        )
      : "";

    const mergeRegionText = (localText: string): string =>
      regionalPromptingSupported
        ? mergeRegionalPromptText(regionalContext, localText)
        : localText;

    const includeConditioningRegions =
      regionalPromptingSupported &&
      (options.includeConditioningRegions ??
        this.effectiveRegionalStrategy === "conditioning");

    const builtRegions = includeConditioningRegions
      ? parsedRegions.regions.map((region) => ({
          text: mergeRegionText(region.text),
          x: region.x,
          y: region.y,
          width: region.width,
          height: region.height,
          strength: 1.0,
        })).concat(
          guiRegions.map((region) => ({
            text: mergeRegionText(region.text),
            x: region.x,
            y: region.y,
            width: region.width,
            height: region.height,
            strength: region.strength,
          })),
        )
      : [];

    const params: GenerationParams = {
      mode: this.mode,
      positive_prompt: translatedPositiveBase,
      negative_prompt: translateNaiWeightSyntax(parsedNegative.baseText),
      positive_segments: translatedPositiveSegments,
      negative_segments: parsedNegative.segments.map((s) => ({
        text: translateNaiWeightSyntax(s.text),
        start: s.start,
        end: s.end,
      })),
      detail_segments: parsedSegmentDetails.segments.map((s) => ({
        target: s.target,
        prompt: translateNaiWeightSyntax(s.prompt),
        creativity: s.creativity,
        threshold: s.threshold,
      })),
      raw_positive_prompt: translateNaiWeightSyntax(positivePrompt),
      positive_regions: builtRegions,
      raw_negative_prompt: translateNaiWeightSyntax(negativePrompt),
      checkpoint: this.checkpoint,
      vae: this.vae || null,
      loras: this.loras
        .filter((l) => l.enabled && l.name)
        .map(({ name, strength_model, strength_clip }) => ({
          name,
          strength_model,
          strength_clip,
        })),
      sampler_name: this.samplerName,
      scheduler: this.scheduler,
      steps: this.steps,
      cfg: this.cfg,
      seed: this.seed,
      width: this.width,
      height: this.height,
      batch_size: this.batchSize,
      denoise: this.denoise,
      differential_diffusion: this.differentialDiffusion,
      input_image: this.inputImage,
      mask_image: this.maskImage,
      grow_mask_by: this.growMaskBy,
      upscale_enabled: this.upscaleEnabled,
      upscale_method: this.upscaleMethod,
      upscale_model: this.upscaleModel,
      upscale_scale: this.upscaleScale,
      upscale_denoise: this.upscaleDenoise,
      upscale_steps: this.upscaleSteps,
      upscale_tile_size: this.upscaleTileSize,
      upscale_tiling: this.upscaleTiling,
      upscale_fast_refine: this.upscaleFastRefine,
      upscale_soft_guidance: this.upscaleSoftGuidance,
      upscale_soft_guidance_multiplier: this.upscaleSoftGuidanceMultiplier,
      refine_only: this.mode === "img2img" && this.upscaleEnabled && this.refineOnly,
      save_pre_upscale_image: this.savePreUpscaleImage,
      smart_guidance: this.smartGuidance,
      flux_guidance: this.fluxGuidance,
      upscale_positive_prompt: upscalePositivePrompt,
      upscale_negative_prompt: upscaleNegativePrompt,
      use_split_model: this.useSplitModel,
      diffusion_model: this.diffusionModel,
      clip_model: this.clipModel,
      clip_type: this.clipType,
      controlnet: this.controlnetEnabled
        ? {
            enabled: true,
            preset: this.controlnetMode === "preset" ? this.controlnetPreset : null,
            controlnet_model: this.controlnetModel,
            preprocessor:
              this.controlnetMode === "preset" ? this.controlnetPreprocessor : null,
            image: this.controlnetImage,
            strength: this.controlnetStrength,
            start_percent: this.controlnetStartPercent,
            end_percent: this.controlnetEndPercent,
          }
        : null,
      facefix_enabled: this.facefixEnabled,
      facefix_detector: this.facefixDetector,
      facefix_denoise: this.facefixDenoise,
      facefix_steps: this.facefixSteps,
      facefix_guide_size: this.facefixGuideSize,
      facefix_max_faces: this.facefixMaxFaces,
      facefix_auto_prompt: this.facefixAutoPrompt,
      model_architecture: this.modelFamily,
      is_sdxl_like: this.isSdxlLike,
      is_vpred_model: signalsIndicateVPred(this.modelFamilySignals()),
      output_bit_depth: this.outputBitDepth,
      output_format: this.outputFormat,
      style_transfer_enabled: this.styleTransferEnabled,
      style_reference_image: this.styleReferenceImage,
      style_transfer_low_scale_end: this.styleTransferLowScaleEnd,
      style_transfer_high_scale_start: this.styleTransferHighScaleStart,
      style_transfer_beta: this.styleTransferBeta,
      style_transfer_adain_strength: this.styleTransferAdainStrength,
      style_transfer_rf_mode: this.styleTransferRfMode,
      style_transfer_gamma: this.styleTransferGamma,
      style_transfer_gamma_curve: this.styleTransferGammaCurve,
      style_transfer_norm_strength: this.styleTransferNormStrength,
      style_transfer_pmi_alpha: this.styleTransferPmiAlpha,
      style_transfer_megapixels: this.styleTransferMegapixels,
      style_transfer_blocks: this.styleTransferBlocks,
    };

    if (options.overrides) {
      Object.assign(params, options.overrides);
    }
    return params;
  }

  addLora() {
    this.loras = [
      ...this.loras,
      { name: "", strength_model: 1.0, strength_clip: 1.0, enabled: true },
    ];
  }

  removeLora(index: number) {
    this.loras = this.loras.filter((_, i) => i !== index);
  }

  toggleLora(index: number) {
    this.loras = this.loras.map((l, i) =>
      i === index ? { ...l, enabled: !l.enabled } : l
    );
  }

  /** Apply defaults if no checkpoint is selected yet (first run). */
  applyDefaultsIfNeeded(checkpoints: string[], vaes: string[]) {
    // Always fix empty VAE for split-model users — VAELoader requires a real file.
    // This covers existing users whose saved settings pre-date the VAE field.
    // Pick a VAE that matches the diffusion model's latent channel layout, NOT
    // the SDXL 4-channel VAE (which would crash VAEDecode with a channel
    // mismatch on Anima/Qwen/Flux split models that produce 16-channel latents).
    this.ensureRecommendedSplitVae(vaes, true);
    if (this.checkpoint) return;

    if (checkpoints.length > 0) {
      this.checkpoint = checkpoints[0];
    }

    this.saveSettings();
  }
}

export const generation = new GenerationStore();
