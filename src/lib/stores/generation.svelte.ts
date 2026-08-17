import { ipcStore } from "../utils/ipc.js";
import { triggerSync } from "../utils/syncTrigger.js";
import { compileTimeline, isTimelineActive } from "../utils/timelineProvider.js";
import {
  buildRegionalContextPrompt,
  mergeRegionalPromptText,
  parseRegionalPrompt,
  parseScheduledPrompt,
} from "../utils/promptSchedule.js";
import { parseSegmentDetailPrompt } from "../utils/promptSegmentDetail.js";
import { joinPromptBoxes, sanitizePromptForSend } from "../utils/promptSanitize.js";
import { extractScaleFromModel } from "../utils/upscalers.js";
import {
  MODEL_FAMILIES,
  TURBO_MODEL_VARIANTS,
  signalsIndicateVPred,
  familyRequiresSeparateClip,
  familyIsSdxlLike,
  toTurboModelVariant,
} from "../utils/modelFamily.js";
import { readModelSpec, type ModelSpec } from "../utils/api.js";
import { H3_TURBO_LORA } from "../utils/h3Models.js";
import {
  H3_DIFFUSION_MARKERS,
  H3_MAX_REF_IMAGES,
  H3_TURBO_DEFAULT_STEPS,
  H3_TURBO_MAX_STEPS,
  H3_TURBO_MIN_STEPS,
  clampH3Megapixels,
  computeH3Dimensions,
  computeH3FrameLength,
} from "../utils/videoParams.js";
import type { ModelFamily, TurboModelVariant } from "../utils/modelFamily.js";
import type {
  ExtraPromptBox,
  GenerationMode,
  GenerationParams,
  LoraEntry,
  RegionalPromptSelection,
  RegionalPromptStrategy,
  VideoAspectRatio,
  VideoVariant,
} from "../types/index.js";
import { models } from "./models.svelte.js";
import { styles } from "./styles.svelte.js";
import { promptPresets } from "./promptPresets.svelte.js";

const STORE_KEY = "generation-settings";
const PROMPT_HISTORY_KEY = "mooshieui.promptHistory.v1";
const MAX_PROMPT_HISTORY = 100;

/** ModelSpec fields the model info panel renders; a spec with none is treated as empty. */
const MODEL_SPEC_DISPLAY_FIELDS = [
  "title",
  "author",
  "description",
  "architecture",
  "hash",
  "resolution",
  "prediction_type",
  "trigger_phrase",
  "usage_hint",
  "tags",
  "license",
  "thumbnail",
  "date",
  "implementation",
  "hash_sha256",
  "merged_from",
  "preprocessor",
  "encoder_layer",
  "sai_model_spec",
  // Any declared modelspec.* field at all is worth opening the panel for: the
  // panel renders unrecognised fields in its catch-all section.
  "modelspec_keys",
] as const;

/** Every model signal cleared — used for no model, unsupported files, and failed detection. */
const UNKNOWN_MODEL_METADATA = {
  modelspecPredictionType: null,
  modelspecPredictKey: null,
  modelspecHeaderVPred: false,
  modelFamily: "unknown" as ModelFamily,
  modelIsSdxlLike: false,
  modelTurboVariant: "none" as TurboModelVariant,
  modelRecommendedVae: null,
  modelRecommendedClipModel: null,
  modelRecommendedClipType: null,
};

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
  /** FluxGuidance override (guidance-distilled families that read it in the template, e.g. Kontext). */
  fluxGuidance?: number;
  /** Sampler to use when `samplerName` is absent from the backend's enumerated list. Defaults to "euler". */
  samplerFallback?: string;
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

/**
 * Translate InvokeAI/compel weight + emphasis syntax to ComfyUI (tag:weight).
 * - (group)0.8      -> (group:0.8)      explicit weight, number OUTSIDE the paren
 * - (group)+ / ++   -> (group:1.10) / (group:1.21)   group emphasis, 1.1^n
 * - (group)- / --   -> (group:0.90) / (group:0.81)   group de-emphasis, 0.9^n
 * - word+ / word--  -> (word:1.10) / (word:0.81)     bareword emphasis
 * Guard: bareword rewrite only fires when the base (token minus the trailing
 * +/- run) contains an ASCII letter, so emoticon tags like +_+ and bare ++ / 1+
 * are left untouched. Blend/swap operators are intentionally not handled.
 *
 * Escape: a backslash directly before a trailing +/- run keeps it a literal
 * character instead of emphasis — e.g. a crossover tag "nero (bride)\+astolfo"
 * or a character name "La\+ darkness" whose name is not emphasis syntax. The
 * escaping backslash is stripped from the output either way.
 */
function translateInvokeAiWeightSyntax(prompt: string): string {
  const emphasisWeight = (marks: string): string => {
    const base = marks[0] === "+" ? 1.1 : 0.9;
    return Math.pow(base, marks.length).toFixed(2);
  };

  // 1. Group trailing explicit weight: (group)0.8 -> (group:0.8)
  prompt = prompt.replace(
    /\(([^()]+)\)(\d+\.?\d*)(?=[\s,(){}\[\]]|$)/g,
    (_m, inner, weight) => `(${inner}:${weight})`,
  );

  // 2. Group emphasis: (group)+++ / (group)--- -> (group:W). Innermost-first.
  // An escaped run is left untouched here (returned byte-for-byte) so the loop
  // still converges; the escaping backslash is stripped once, after the loop,
  // so the now-literal +/- doesn't get re-matched and wrongly converted on a
  // later pass.
  let prev: string;
  do {
    prev = prompt;
    prompt = prompt.replace(
      /\(([^()]+)\)(\\(?:\++|-+)|\++|-+)/g,
      (_m, inner, marks) => {
        if (marks[0] === "\\") return `(${inner})${marks}`;
        return `(${inner}:${emphasisWeight(marks)})`;
      },
    );
  } while (prompt !== prev);
  prompt = prompt.replace(/\)\\(\++|-+)/g, ")$1");

  // 3. Bareword emphasis: tokenize on delimiters, rewrite word+ / word- when the
  // base has a letter. Delimiters: whitespace , ( ) { }
  prompt = prompt.replace(/[^\s,(){}]+/g, (token) => {
    const m = token.match(/^(.*?)(\\?)(\++|-+)$/);
    if (!m) return token;
    const [, base, esc, marks] = m;
    if (esc) return base + marks;
    if (!/[a-zA-Z]/.test(base)) return token;
    return `(${base}:${emphasisWeight(marks)})`;
  });

  return prompt;
}

/** Apply InvokeAI translation, then NAI translation, to a prompt string. */
function translatePromptWeightSyntax(prompt: string): string {
  return translateNaiWeightSyntax(translateInvokeAiWeightSyntax(prompt));
}

type StylePresetId = "none" | "anime" | "cinematic" | "photoreal" | "digital_art" | "line_art";

// `satisfies` ties the runtime list to the shared union in types/index.ts, so a
// mode added here without updating that union is a compile error rather than a
// silent drift across the gallery/progress/save-to-gallery consumers.
const GENERATION_MODES = [
  "txt2img",
  "img2img",
  "inpainting",
  "image_edit",
  "video",
] as const satisfies readonly GenerationMode[];

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
    image_edit: defaultModeToggleState(),
    video: defaultModeToggleState(),
  };
}

function booleanOrDefault(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

/**
 * Which prompt the UI is editing.
 *
 * Only the image/video boundary splits, not every mode: H3 is prompted in prose
 * and the image models in danbooru tags, so carrying one across is never what
 * anyone meant. Within the image side people move a prompt between txt2img,
 * img2img and inpainting constantly, so those keep sharing one bucket.
 */
type PromptBucketId = "image" | "video";

/** Everything that swaps together when crossing the image/video boundary. */
interface PromptBucket {
  positivePrompt: string;
  negativePrompt: string;
  extraPositiveBoxes: ExtraPromptBox[];
  extraNegativeBoxes: ExtraPromptBox[];
}

type PromptBuckets = Record<PromptBucketId, PromptBucket>;

function newBoxId(): string {
  return crypto.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function sanitizePromptBoxes(raw: unknown): ExtraPromptBox[] {
  return (Array.isArray(raw) ? raw : [])
    .filter((b: unknown) => !!b && typeof b === "object")
    .map((b: any) => ({
      id: typeof b.id === "string" && b.id ? b.id : newBoxId(),
      name: typeof b.name === "string" ? b.name : "",
      content: typeof b.content === "string" ? b.content : "",
    }));
}

function promptBucketFor(mode: GenerationMode): PromptBucketId {
  return mode === "video" ? "video" : "image";
}

function emptyPromptBucket(): PromptBucket {
  return {
    positivePrompt: "",
    negativePrompt: "",
    extraPositiveBoxes: [],
    extraNegativeBoxes: [],
  };
}

function createDefaultPromptBuckets(): PromptBuckets {
  return { image: emptyPromptBucket(), video: emptyPromptBucket() };
}

function normalizePromptBuckets(value: unknown): PromptBuckets {
  const normalized = createDefaultPromptBuckets();
  if (!value || typeof value !== "object") return normalized;

  const raw = value as Record<string, Partial<PromptBucket> | undefined>;
  for (const id of ["image", "video"] as PromptBucketId[]) {
    const bucket = raw[id];
    if (!bucket || typeof bucket !== "object") continue;
    normalized[id] = {
      positivePrompt: typeof bucket.positivePrompt === "string" ? bucket.positivePrompt : "",
      negativePrompt: typeof bucket.negativePrompt === "string" ? bucket.negativePrompt : "",
      extraPositiveBoxes: sanitizePromptBoxes(bucket.extraPositiveBoxes),
      extraNegativeBoxes: sanitizePromptBoxes(bucket.extraNegativeBoxes),
    };
  }

  return normalized;
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
  extraPositiveBoxes = $state<ExtraPromptBox[]>([]);
  extraNegativeBoxes = $state<ExtraPromptBox[]>([]);
  /**
   * The prompt bucket that is *not* currently being edited, parked here until
   * the user switches back. The live fields above are always the active bucket,
   * so everything that reads or writes a prompt keeps doing so unchanged.
   */
  promptBuckets = $state<PromptBuckets>(createDefaultPromptBuckets());
  checkpoint = $state("");
  vae = $state("");
  loras = $state<LoraEntry[]>([]);
  samplerName = $state("euler_cfg_pp");
  scheduler = $state("sgm_uniform");
  steps = $state(20);
  cfg = $state(1.4);
  // Decimal string ("-1" = random): 63-bit seeds exceed JS's safe-integer range.
  seed = $state("-1");
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
  /** Optional cap on the effective upscale multiplier when using a model upscaler
   *  (e.g. refine only to 2x after a 4x model), so users aren't nudged into always
   *  taking the full native scale of whatever model they picked. */
  upscaleTargetScaleEnabled = $state(false);
  upscaleTargetScale = $state(2.0);
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
  /**
   * Folder the active model physically lives in when it doesn't match what the
   * model actually is (a split-file model dropped into `checkpoints/`, or a full
   * checkpoint in `diffusion_models/`). `null` when the file is where it belongs.
   *
   * Detection flips `useSplitModel` silently; this records the physical folder so
   * metadata lookups keep using the real path and the backend can resolve an
   * absolute path for the Mooshie path-based loader nodes (ComfyUI's stock
   * loaders only accept filenames from their own folder listing).
   */
  modelSourceCategory = $state<string | null>(null);
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
  /** Image Edit mode reference images; slot 0 primary, slots 1-2 Qwen Edit Plus extras. */
  editReferenceImages = $state<(string | null)[]>([null, null, null]);
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
  /** Anima TeaCache: reuses the previous step's DiT output while the
   *  accumulated input delta stays under threshold. MooshieUI-authored node,
   *  always available (no lazy install, unlike video's H3 TeaCache). */
  animaTeacacheEnabled = $state(false);
  facefixEnabled = $state(false);
  facefixDetector = $state<string | null>(null);
  facefixDenoise = $state(0.4);
  facefixSteps = $state(20);
  facefixGuideSize = $state(512);
  facefixMaxFaces = $state(8);
  facefixAutoPrompt = $state(false);
  outputBitDepth = $state<"8bit" | "16bit">("8bit");
  /** WebP is lossless VP8L and always 8-bit (no 16-bit VP8L variant exists). */
  outputFormat = $state<"png" | "jxl" | "webp">("png");
  metadataMode = $state<"text_chunk" | "stealth" | "both">("both");
  autoQualityTags = $state(true);
  /** UI-reveal toggle for the custom quality tags editor (values apply based on autoQualityTags). */
  customQualityTagsEnabled = $state(false);
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
  /** When true, swapping checkpoints no longer auto-applies per-model generation params
   *  (steps/cfg/sampler/scheduler/dimensions). Family/metadata detection still runs.
   *  The first-ever preset application (while `modelPresetAppliedKey` is still unset) is
   *  exempt so a fresh profile still gets sane defaults; every later swap preserves. */
  advancedMode = $state(false);
  /** When true, checkpoint/model swaps never overwrite width/height, regardless of advancedMode. */
  resolutionLocked = $state(false);
  regionalPrompts = $state<RegionalPromptSelection[]>([]);
  /** SDXL/Illustrious: conditioning areas vs sequential inpaint. Anima always uses inpaint chain. */
  regionalPromptStrategy = $state<RegionalPromptStrategy>("conditioning");

  // --- Video mode (MiniMax H3) ---
  /** "fl2va" (first/last frame, also plain text-to-video) or "ref2va" (reference images). */
  videoVariant = $state<VideoVariant>("fl2va");
  /** Requested duration; the backend snaps it to the nearest 17n+5 frame count at 24 fps. */
  videoDurationSeconds = $state(5);
  /** Pixel budget the backend turns into width/height together with the aspect ratio. */
  videoMegapixels = $state(0.4);
  videoAspectRatio = $state<VideoAspectRatio>("16:9");
  /** fl2va first/last frame slots (ComfyUI input filenames); both optional. */
  videoFirstFrame = $state<string | null>(null);
  videoLastFrame = $state<string | null>(null);
  /** True pixel dimensions of the uploaded frames as `"W:H"`, recorded at upload
   *  time so the `"auto"` aspect ratio can reproduce the source framing. The
   *  upload is downscaled uniformly, so the ratio survives even though the
   *  numbers are the original image's. */
  videoFirstFrameAspect = $state<string | null>(null);
  videoLastFrameAspect = $state<string | null>(null);
  /** Submit the first frame as the last frame too, so the clip ends where it
   *  started. Kept separate from `videoLastFrame` rather than overwriting it, so
   *  a separately uploaded last frame survives toggling this off again. */
  videoFirstFrameAsLast = $state(false);
  /** ref2va reference image slots (ComfyUI input filenames); at most 9, holes allowed. */
  videoRefImages = $state<(string | null)[]>(Array(H3_MAX_REF_IMAGES).fill(null));
  /** RIFE 2x frame interpolation. Only ever true once the lazy install has put
   *  the pack and its checkpoint on disk. */
  videoRifeEnabled = $state(false);
  /** RIFE factor: output plays at 24 x this. Capped at 4, see utils/rife.ts. */
  videoRifeMultiplier = $state(2);
  /** RIFE flow scale: one of 0.25, 0.5, 1, 2, 4. Lower needs less memory. */
  videoRifeScaleFactor = $state(1);
  videoRifeFastMode = $state(true);
  videoRifeEnsemble = $state(true);
  /** MiniMax-H3 Turbo LoRA: distilled few-step sampling. Only ever true once the
   *  lazy install has put the node pack and the adapter on disk. */
  videoTurboEnabled = $state(false);
  /** Sampling steps while Turbo is on; clamped to 4..8 by the backend too. */
  videoTurboSteps = $state(H3_TURBO_DEFAULT_STEPS);
  /** TeaCache: reuses the previous step's model output while the accumulated
   *  input delta stays under threshold. Only ever true once the lazy install
   *  has put the node pack on disk. */
  videoTeacacheEnabled = $state(false);
  videoDiffusionModel = $state<string | null>(null);
  videoClipModel = $state<string | null>(null);
  videoVaeModel = $state<string | null>(null);
  videoAudioVaeModel = $state<string | null>(null);

  /** Frame count the backend will use for the current duration. */
  get videoFrameLength(): number {
    return computeH3FrameLength(this.videoDurationSeconds);
  }

  /** `"W:H"` of the frame the clip should be framed after, or null when there is
   *  nothing to match. Only fl2va sends frames, so ref2va never matches. */
  get videoFrameAspect(): string | null {
    if (this.videoVariant !== "fl2va") return null;
    return this.videoFirstFrameAspect ?? this.videoLastFrameAspect;
  }

  /** The ratio actually sent to the backend: `"auto"` becomes the uploaded
   *  frame's own `"W:H"`, or 16:9 when there is no frame to match. */
  get resolvedVideoAspectRatio(): string {
    if (this.videoAspectRatio !== "auto") return this.videoAspectRatio;
    return this.videoFrameAspect ?? "16:9";
  }

  /** Width/height the backend will derive from the current ratio and megapixels. */
  get videoDimensions(): { width: number; height: number } {
    return computeH3Dimensions(this.resolvedVideoAspectRatio, this.videoMegapixels);
  }

  /** Filename actually submitted as the fl2va last frame, honouring the
   *  "use the first frame as the last frame" toggle. */
  get videoEffectiveLastFrame(): string | null {
    return this.videoFirstFrameAsLast ? this.videoFirstFrame : this.videoLastFrame;
  }

  /** Non-empty ref2va reference filenames, in slot order. */
  get videoRefImageFilenames(): string[] {
    return this.videoRefImages.filter((v): v is string => !!v && !!v.trim());
  }

  /** Whether the four MiniMax H3 model files are all selected. */
  get videoModelsReady(): boolean {
    return [
      this.videoDiffusionModel,
      this.videoClipModel,
      this.videoVaeModel,
      this.videoAudioVaeModel,
    ].every((v) => !!v && !!v.trim());
  }

  /** Whether the diffusion model file looks like the selected variant's H3 weights. */
  get videoDiffusionModelMatchesVariant(): boolean {
    const name = (this.videoDiffusionModel ?? "").toLowerCase();
    if (!name) return true;
    const other = this.videoVariant === "fl2va" ? "ref2va" : "fl2va";
    return !name.includes(other);
  }

  /** Whether the diffusion model file carries a MiniMax H3 fingerprint. */
  get videoDiffusionModelLooksLikeH3(): boolean {
    const name = (this.videoDiffusionModel ?? "").toLowerCase();
    if (!name) return true;
    return H3_DIFFUSION_MARKERS.some((marker) => name.includes(marker));
  }

  /** Whether the video mode has everything it needs to submit a generation. */
  get videoReady(): boolean {
    if (!this.videoModelsReady) return false;
    if (!this.videoDiffusionModelLooksLikeH3) return false;
    if (!this.videoDiffusionModelMatchesVariant) return false;
    // The timeline supplies its own references (shot stills, cast photos), so
    // the settings panel's slots are only mandatory when it is not driving.
    if (
      this.videoVariant === "ref2va" &&
      this.videoRefImageFilenames.length === 0 &&
      !isTimelineActive()
    ) {
      return false;
    }
    return true;
  }

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
  /** True while a read_modelspec call is in flight for the current model. */
  isModelMetadataLoading = $state(false);
  /** ModelSpec for the current model, or null when it carries no display fields. */
  modelSpec = $state<ModelSpec | null>(null);
  /** True when the backend returned no usable spec for the current model. */
  modelSpecUnavailable = $state(false);
  /** `category::filename` the metadata above was resolved for. */
  private _loadedModelMetadataKey = "";
  /** Guards against out-of-order read_modelspec responses. */
  private _latestModelMetadataRequestId = 0;
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

    const from = promptBucketFor(this._mode);
    const to = promptBucketFor(mode);
    if (from !== to) {
      this.promptBuckets = { ...this.promptBuckets, [from]: this.readPromptBucket() };
      this.applyPromptBucket(this.promptBuckets[to] ?? emptyPromptBucket());
    }

    this._mode = mode;
    this.applyModeToggleState(this.modeToggles[mode] ?? defaultModeToggleState());
  }

  readPromptBucket(): PromptBucket {
    return {
      positivePrompt: this.positivePrompt,
      negativePrompt: this.negativePrompt,
      extraPositiveBoxes: this.extraPositiveBoxes,
      extraNegativeBoxes: this.extraNegativeBoxes,
    };
  }

  applyPromptBucket(bucket: PromptBucket): void {
    this.positivePrompt = bucket.positivePrompt;
    this.negativePrompt = bucket.negativePrompt;
    this.extraPositiveBoxes = bucket.extraPositiveBoxes;
    this.extraNegativeBoxes = bucket.extraNegativeBoxes;
  }

  /** Buckets with the live fields folded back into the active side, for persisting. */
  promptBucketsWithCurrent(): PromptBuckets {
    return {
      ...this.promptBuckets,
      [promptBucketFor(this._mode)]: this.readPromptBucket(),
    };
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

  /** Ratio applied after model-based upscaling to cap the effective scale below the
   *  model's native factor (e.g. 0.5 to bring a 4x model's output down to 2x).
   *  1.0 (no-op) when the cap is off, no model is selected, or its scale can't be
   *  detected from the filename. */
  get upscaleModelDownscaleRatio(): number {
    if (!this.upscaleTargetScaleEnabled || this.upscaleMethod !== "model" || !this.upscaleModel) {
      return 1.0;
    }
    const nativeScale = extractScaleFromModel(this.upscaleModel);
    if (!nativeScale || nativeScale <= 0) return 1.0;
    return Math.min(this.upscaleTargetScale / nativeScale, 1.0);
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

  /** True when the selected model is Qwen Image Edit (single reference image). */
  get isQwenEdit(): boolean {
    return this.modelFamily === "qwen_edit";
  }

  /** True when the selected model is Qwen Image Edit Plus (up to 3 reference images). */
  get isQwenEditPlus(): boolean {
    return this.modelFamily === "qwen_edit_plus";
  }

  /** True when the selected model is Flux.1 Kontext (image edit). Kept out of isFlux. */
  get isFluxKontext(): boolean {
    return this.modelFamily === "flux1kontext";
  }

  /** True when the selected model is an Image Edit family. */
  get supportsImageEditMode(): boolean {
    return this.isQwenEdit || this.isQwenEditPlus || this.isFluxKontext;
  }

  /** Number of reference-image slots the current edit family accepts (3 for Plus, else 1). */
  get editReferenceSlotCount(): number {
    return this.isQwenEditPlus ? 3 : 1;
  }

  /** True when the selected model is a Pony Diffusion variant. */
  get isPony(): boolean {
    return this.modelFamily === "pony";
  }

  /** True when the selected model is AuraFlow. */
  get isAuraFlow(): boolean {
    return this.modelFamily === "auraflow";
  }

  /** In img2img/inpainting/image_edit, the loaded source image owns width/height. */
  get hasAuthoritativeEditSource(): boolean {
    if (this._mode === "image_edit") return !!this.editReferenceImages[0];
    return (this._mode === "img2img" || this._mode === "inpainting") && !!this.inputImage;
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

  /**
   * True when the selected model ignores negative prompts.
   *
   * Video is unconditional here rather than family-keyed: the H3 workflow guides
   * with `BasicGuider`, which has one conditioning input and no negative branch
   * at all, so nothing typed in that box could reach ComfyUI whatever checkpoint
   * the image side happens to have loaded.
   */
  get disablesNegativePrompt(): boolean {
    if (this.mode === "video") return true;
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
    // Autocomplete tag-set sync is handled by an $effect in App.svelte that
    // watches the model family (stores must not import each other).
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

  /**
   * `category::filename` identifying the currently selected model, keyed on the
   * folder the file physically lives in. Using the physical folder matters after a
   * silent reclassification: the loaded-metadata key and the manual family
   * override key must both survive a restart that restores `useSplitModel: true`
   * for a model that still sits in `checkpoints/`.
   */
  currentModelMetadataKey(): string {
    if (this.useSplitModel && this.diffusionModel) {
      return `${this.modelSourceCategory ?? "diffusion_models"}::${this.diffusionModel}`;
    }
    if (this.checkpoint) return `${this.modelSourceCategory ?? "checkpoints"}::${this.checkpoint}`;
    return "";
  }

  /** Reset every model signal to its unknown-model default. */
  clearModelMetadata(): void {
    this._latestModelMetadataRequestId += 1;
    this._loadedModelMetadataKey = "";
    this.isModelMetadataLoading = false;
    this.modelSpec = null;
    this.modelSpecUnavailable = false;
    this.modelSourceCategory = null;
    this.applyModelMetadata(UNKNOWN_MODEL_METADATA);
  }

  /**
   * Record a manual family override as the resolved metadata for `cacheKey` so
   * the App-level effect treats it as loaded instead of re-running detection.
   */
  prepareManualOverride(cacheKey: string): void {
    this._latestModelMetadataRequestId += 1;
    this._loadedModelMetadataKey = cacheKey;
    this.isModelMetadataLoading = false;
    this.modelSpec = null;
    this.modelSpecUnavailable = false;
  }

  /** Drop the cached key so the next fetch re-runs backend detection. */
  invalidateModelMetadataCache(): void {
    this._latestModelMetadataRequestId += 1;
    this._loadedModelMetadataKey = "";
    this.isModelMetadataLoading = false;
  }

  /**
   * Resolve and apply family/spec metadata for the selected model. Driven by an
   * $effect in App.svelte rather than ModelSelector: that component is unmounted
   * whenever the Model panel is collapsed, and quality-tag injection in
   * toParams() keys off modelFamily, so it was silently skipped in that state.
   */
  async fetchAndApplyModelMetadata(category: string, filename: string): Promise<void> {
    // GGUF carries no safetensors header, but the backend still resolves
    // family/turbo/recommended-encoder info from the filename and sidecars.
    const supportsSpec =
      filename.endsWith(".safetensors") || filename.toLowerCase().endsWith(".gguf");
    if (!filename || !supportsSpec) {
      this.clearModelMetadata();
      return;
    }

    const metadataKey = `${category}::${filename}`;
    const manualOverride = this.modelFamilyOverrides[metadataKey] ?? null;
    if (manualOverride) {
      this.prepareManualOverride(metadataKey);
      this.applyModelMetadata({
        ...UNKNOWN_MODEL_METADATA,
        modelFamily: manualOverride,
        modelIsSdxlLike: familyIsSdxlLike(manualOverride),
      });
      this.applyModelSpecificPreset();
      return;
    }

    if (metadataKey === this._loadedModelMetadataKey && this.modelFamily !== "unknown") return;

    const requestId = ++this._latestModelMetadataRequestId;
    this.isModelMetadataLoading = true;
    try {
      const spec = await readModelSpec(category, filename);
      if (requestId !== this._latestModelMetadataRequestId) return;
      if (this.currentModelMetadataKey() !== metadataKey) return;

      this.modelSpec = MODEL_SPEC_DISPLAY_FIELDS.some((field) => !!spec?.[field]) ? spec : null;
      this.modelSpecUnavailable = !this.modelSpec;

      const family = (spec?.family as ModelFamily | undefined) ?? "unknown";
      this._loadedModelMetadataKey = metadataKey;
      this.isModelMetadataLoading = false;
      this.applyModelMetadata({
        modelspecPredictionType: spec?.prediction_type ?? null,
        modelspecPredictKey: spec?.predict_key ?? null,
        modelspecHeaderVPred: spec?.header_v_pred === "true",
        modelFamily: family,
        modelIsSdxlLike: spec?.is_sdxl_like === "true",
        modelTurboVariant: toTurboModelVariant(spec?.turbo_model_variant),
        modelRecommendedVae: spec?.recommended_vae ?? null,
        modelRecommendedClipModel: spec?.recommended_clip_model ?? null,
        modelRecommendedClipType: spec?.recommended_clip_type ?? null,
      });
      this.applyDetectedModelKind(category, filename, spec?.model_kind ?? null);
      this.ensureRecommendedSplitClip(models.textEncoders);
      this.ensureRecommendedSplitVae(models.vaes);
      this.applyModelSpecificPreset();
      // Nothing resolved — clear the key so a later selection retries detection.
      if (family === "unknown") this._loadedModelMetadataKey = "";
    } catch {
      if (requestId !== this._latestModelMetadataRequestId) return;
      if (this.currentModelMetadataKey() !== metadataKey) return;

      this._loadedModelMetadataKey = "";
      this.isModelMetadataLoading = false;
      this.modelSpec = null;
      this.modelSpecUnavailable = true;
      this.applyModelMetadata(UNKNOWN_MODEL_METADATA);
    }
  }

  /**
   * Silently switch loader mode when detection says the file isn't what its folder
   * claims. Backend `model_kind` comes from safetensors tensor keys (baked CLIP/VAE
   * trees mean a full checkpoint, diffusion-only keys mean a split file), from the
   * container for GGUF, or from the resolved family as a fallback.
   *
   * No dialog, no banner: the user picked a file and it just works. `modelSourceCategory`
   * keeps pointing at the physical folder so the backend can load it by absolute path.
   */
  private applyDetectedModelKind(
    category: string,
    filename: string,
    modelKind: string | null,
  ): void {
    // GGUF stays on the existing error path: UnetLoaderGGUF is a third-party node
    // with no absolute-path input, so a misplaced .gguf can't be loaded anyway.
    if (!modelKind || filename.toLowerCase().endsWith(".gguf")) {
      this.modelSourceCategory = null;
      return;
    }

    if (modelKind === "diffusion_model" && category === "checkpoints") {
      this.modelSourceCategory = "checkpoints";
      // Order matters: diffusionModel must be set before useSplitModel so no
      // reactive effect ever observes split mode with an unresolved model.
      this.diffusionModel = filename;
      this.useSplitModel = true;
      return;
    }

    if (modelKind === "checkpoint" && category === "diffusion_models") {
      this.modelSourceCategory = "diffusion_models";
      this.checkpoint = filename;
      this.useSplitModel = false;
      this.diffusionModel = null;
      this.clipModel = null;
      this.clipType = null;
      return;
    }

    this.modelSourceCategory = null;
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
    // Snapshot the full concatenated prompt (main box + extra boxes) so history
    // entries stay self-contained — restoring one replays everything the user saw.
    const positivePrompt = joinPromptBoxes([
      this.positivePrompt,
      ...this.extraPositiveBoxes.map((b) => b.content),
    ]);
    const negativePrompt = joinPromptBoxes([
      this.negativePrompt,
      ...this.extraNegativeBoxes.map((b) => b.content),
    ]);
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

    // Mode first: crossing the image/video boundary swaps the prompt buckets, so
    // writing the prompt before the switch would park it in the bucket we are
    // leaving and then overwrite it with the one we arrive in.
    this.mode = entry.mode;
    this.positivePrompt = entry.positivePrompt;
    this.negativePrompt = entry.negativePrompt;
    // The stored prompt already includes any extra-box content (concatenated at
    // save time), so clear the boxes to avoid duplicating it back on top.
    this.extraPositiveBoxes = [];
    this.extraNegativeBoxes = [];
    this.stylePreset = entry.stylePreset;

    this.promptHistory = [
      { ...entry, createdAt: Date.now() },
      ...this.promptHistory.filter((item) => item.id !== entry.id),
    ];
    this.savePromptHistory();
    this.saveSettings();
  }

  private newBoxId(): string {
    return newBoxId();
  }

  addPositiveBox() {
    this.extraPositiveBoxes = [
      ...this.extraPositiveBoxes,
      { id: this.newBoxId(), name: "", content: "" },
    ];
    this.saveSettings();
  }

  removePositiveBox(id: string) {
    this.extraPositiveBoxes = this.extraPositiveBoxes.filter((b) => b.id !== id);
    this.saveSettings();
  }

  updatePositiveBox(id: string, patch: Partial<Pick<ExtraPromptBox, "name" | "content">>) {
    this.extraPositiveBoxes = this.extraPositiveBoxes.map((b) =>
      b.id === id ? { ...b, ...patch } : b
    );
    this.saveSettings();
  }

  addNegativeBox() {
    this.extraNegativeBoxes = [
      ...this.extraNegativeBoxes,
      { id: this.newBoxId(), name: "", content: "" },
    ];
    this.saveSettings();
  }

  removeNegativeBox(id: string) {
    this.extraNegativeBoxes = this.extraNegativeBoxes.filter((b) => b.id !== id);
    this.saveSettings();
  }

  updateNegativeBox(id: string, patch: Partial<Pick<ExtraPromptBox, "name" | "content">>) {
    this.extraNegativeBoxes = this.extraNegativeBoxes.map((b) =>
      b.id === id ? { ...b, ...patch } : b
    );
    this.saveSettings();
  }

  private resolveAvailableOption(options: string[], preferred: string, fallback: string): string {
    if (options.includes(preferred)) return preferred;
    if (options.includes(fallback)) return fallback;
    return options[0] ?? preferred;
  }

  private applyResolvedPreset(preset: ModelPreset) {
    this.steps = preset.steps;
    this.cfg = preset.cfg;
    this.samplerName = this.resolveAvailableOption(models.samplers, preset.samplerName, preset.samplerFallback ?? "euler");
    this.scheduler = this.resolveAvailableOption(models.schedulers, preset.scheduler, "normal");
    if (!this.hasAuthoritativeEditSource && !this.resolutionLocked) {
      this.width = preset.width;
      this.height = preset.height;
    }
    this.facefixSteps = Math.ceil(preset.steps / 3);
    this.upscaleSteps = Math.ceil(preset.steps / 3);
    if (preset.upscaleDenoise !== undefined) {
      this.upscaleDenoise = preset.upscaleDenoise;
    }
    if (preset.fluxGuidance !== undefined) {
      this.fluxGuidance = preset.fluxGuidance;
    }
  }

  applyModelSpecificPreset() {
    // Autocomplete tag-set sync on model-family change is handled by an
    // $effect in App.svelte (stores must not import each other).

    // Only apply defaults when the selected model actually changed. Metadata
    // reloads for the same model (page remount on tab switch, app restart)
    // must not clobber settings the user has tweaked since.
    const presetKey = [
      this.useSplitModel && this.diffusionModel ? `dm:${this.diffusionModel}` : `cp:${this.checkpoint}`,
      this.modelFamily,
      this.modelTurboVariant,
    ].join("|");
    if (presetKey === this.modelPresetAppliedKey) return;
    const isFirstPresetApplication = !this.modelPresetAppliedKey;
    // Advanced Mode: after the first-ever application, preserve the user's generation
    // params on checkpoint swaps. Family/metadata detection runs separately in
    // applyModelMetadata(); only the param writes below are skipped.
    // Record the key only once the preset is actually applied (below) — recording
    // it here would let the idempotency guard suppress the preset forever if the
    // user later disables Advanced Mode on the same model.
    if (this.advancedMode && !isFirstPresetApplication) return;

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
          // euler_ancestral_cfg_pp is a CFG++ sampler tuned for low CFG (~1.5-2.2);
          // CFG 2.0 keeps it inside its band. Falls back to plain euler_ancestral
          // on older ComfyUI builds that lack the cfg_pp variant.
          cfg: this.hasTurboModelVariant ? 1.0 : 2.0,
          samplerName: this.hasTurboModelVariant ? "euler" : "euler_ancestral_cfg_pp",
          samplerFallback: this.hasTurboModelVariant ? "euler" : "euler_ancestral",
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

      // Qwen Image Edit / Edit Plus reuse the Qwen sampler preset.
      case "qwen_edit":
      case "qwen_edit_plus":
        preset = {
          steps: 30,
          cfg: 4.0,
          samplerName: "er_sde",
          scheduler: "sgm_uniform",
          width: 1024,
          height: 1024,
        };
        break;

      // Flux.1 Kontext is guidance-distilled; low CFG with FluxGuidance in the template.
      case "flux1kontext":
        preset = {
          steps: 28,
          cfg: 1.0,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
          fluxGuidance: 2.5,
        };
        break;

      case "ideogram4":
        preset = {
          steps: 20,
          cfg: 4.0,
          samplerName: "euler",
          scheduler: "simple",
          width: 1024,
          height: 1024,
        };
        break;

      case "krea2":
        preset = {
          steps: this.hasTurboModelVariant ? 8 : 30,
          cfg: this.hasTurboModelVariant ? 1.0 : 4.0,
          samplerName: this.hasTurboModelVariant ? "euler" : "er_sde",
          scheduler: this.hasTurboModelVariant ? "simple" : "sgm_uniform",
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
        // String(...) coerces seeds persisted as numbers by older versions.
        if (saved.seed !== undefined) this.seed = String(saved.seed);
        if (saved.width) this.width = saved.width;
        if (saved.height) this.height = saved.height;
        if (saved.batchSize) this.batchSize = saved.batchSize;
        if (saved.denoise !== undefined) this.denoise = saved.denoise;
        if (saved.differentialDiffusion !== undefined) this.differentialDiffusion = saved.differentialDiffusion;
        // The parked bucket. The active one is loaded from the flat fields below,
        // which also carries a pre-split store forward: its single prompt lands
        // on whichever side the saved mode was on, and the other starts empty.
        if (saved.promptBuckets) this.promptBuckets = normalizePromptBuckets(saved.promptBuckets);
        if (saved.positivePrompt) this.positivePrompt = saved.positivePrompt;
        if (saved.negativePrompt) this.negativePrompt = saved.negativePrompt;
        if (Array.isArray(saved.extraPositiveBoxes)) {
          this.extraPositiveBoxes = sanitizePromptBoxes(saved.extraPositiveBoxes);
        }
        if (Array.isArray(saved.extraNegativeBoxes)) {
          this.extraNegativeBoxes = sanitizePromptBoxes(saved.extraNegativeBoxes);
        }
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
        if (saved.upscaleTargetScaleEnabled !== undefined)
          this.upscaleTargetScaleEnabled = saved.upscaleTargetScaleEnabled;
        if (saved.upscaleTargetScale !== undefined) this.upscaleTargetScale = saved.upscaleTargetScale;
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
        if (saved.modelSourceCategory !== undefined)
          this.modelSourceCategory = saved.modelSourceCategory;
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
        if (Array.isArray(saved.editReferenceImages)) {
          const slots = saved.editReferenceImages
            .slice(0, 3)
            .map((v: unknown) => (typeof v === "string" && v ? v : null));
          while (slots.length < 3) slots.push(null);
          this.editReferenceImages = slots;
        }
        if (saved.videoVariant === "fl2va" || saved.videoVariant === "ref2va")
          this.videoVariant = saved.videoVariant;
        if (saved.videoDurationSeconds !== undefined)
          this.videoDurationSeconds = saved.videoDurationSeconds;
        // Clamped rather than taken as-is: settings written before the pixel
        // budget became a slider can hold values off the current range/step.
        if (saved.videoMegapixels !== undefined)
          this.videoMegapixels = clampH3Megapixels(saved.videoMegapixels);
        if (saved.videoAspectRatio !== undefined) this.videoAspectRatio = saved.videoAspectRatio;
        if (saved.videoFirstFrame !== undefined) this.videoFirstFrame = saved.videoFirstFrame;
        if (saved.videoLastFrame !== undefined) this.videoLastFrame = saved.videoLastFrame;
        if (saved.videoFirstFrameAspect !== undefined)
          this.videoFirstFrameAspect = saved.videoFirstFrameAspect;
        if (saved.videoLastFrameAspect !== undefined)
          this.videoLastFrameAspect = saved.videoLastFrameAspect;
        if (saved.videoFirstFrameAsLast !== undefined)
          this.videoFirstFrameAsLast = saved.videoFirstFrameAsLast;
        if (Array.isArray(saved.videoRefImages)) {
          const slots = saved.videoRefImages
            .slice(0, H3_MAX_REF_IMAGES)
            .map((v: unknown) => (typeof v === "string" && v ? v : null));
          while (slots.length < H3_MAX_REF_IMAGES) slots.push(null);
          this.videoRefImages = slots;
        }
        if (saved.videoRifeEnabled !== undefined) this.videoRifeEnabled = saved.videoRifeEnabled;
        if (saved.videoRifeMultiplier !== undefined)
          this.videoRifeMultiplier = saved.videoRifeMultiplier;
        if (saved.videoRifeScaleFactor !== undefined)
          this.videoRifeScaleFactor = saved.videoRifeScaleFactor;
        if (saved.videoRifeFastMode !== undefined) this.videoRifeFastMode = saved.videoRifeFastMode;
        if (saved.videoRifeEnsemble !== undefined) this.videoRifeEnsemble = saved.videoRifeEnsemble;
        if (saved.videoTurboEnabled !== undefined)
          this.videoTurboEnabled = saved.videoTurboEnabled;
        if (saved.videoTeacacheEnabled !== undefined)
          this.videoTeacacheEnabled = saved.videoTeacacheEnabled;
        if (saved.videoTurboSteps !== undefined)
          this.videoTurboSteps = Math.min(
            H3_TURBO_MAX_STEPS,
            Math.max(H3_TURBO_MIN_STEPS, Math.round(saved.videoTurboSteps)),
          );
        if (saved.videoDiffusionModel !== undefined)
          this.videoDiffusionModel = saved.videoDiffusionModel;
        if (saved.videoClipModel !== undefined) this.videoClipModel = saved.videoClipModel;
        if (saved.videoVaeModel !== undefined) this.videoVaeModel = saved.videoVaeModel;
        if (saved.videoAudioVaeModel !== undefined)
          this.videoAudioVaeModel = saved.videoAudioVaeModel;
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
        if (saved.animaTeacacheEnabled !== undefined)
          this.animaTeacacheEnabled = saved.animaTeacacheEnabled;
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
        if (saved.outputFormat === "png" || saved.outputFormat === "jxl" || saved.outputFormat === "webp") this.outputFormat = saved.outputFormat;
        if (saved.metadataMode) this.metadataMode = saved.metadataMode;
        if (saved.autoQualityTags !== undefined) this.autoQualityTags = saved.autoQualityTags;
        if (saved.customQualityTagsEnabled !== undefined) this.customQualityTagsEnabled = saved.customQualityTagsEnabled;
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
        if (saved.advancedMode !== undefined) this.advancedMode = saved.advancedMode;
        if (saved.resolutionLocked !== undefined) this.resolutionLocked = saved.resolutionLocked;
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
        // Autocomplete tag-set sync with the restored model is handled by an
        // $effect in App.svelte (stores must not import each other).
      }
    } catch (e) {
      console.error("Failed to load settings:", e);
    } finally {
      // Must stay after the awaited read: saveSettings() is a no-op until this
      // flips, so a keystroke during startup can't persist defaults over the
      // restored values. Set on the error path too, or saves break for good.
      this._storeReady = true;
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
        promptBuckets: this.promptBucketsWithCurrent(),
        // The active bucket is also written flat, both for older builds reading
        // this store and as the load-time source for the live fields.
        positivePrompt: this.positivePrompt,
        negativePrompt: this.negativePrompt,
        extraPositiveBoxes: this.extraPositiveBoxes,
        extraNegativeBoxes: this.extraNegativeBoxes,
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
        upscaleTargetScaleEnabled: this.upscaleTargetScaleEnabled,
        upscaleTargetScale: this.upscaleTargetScale,
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
        modelSourceCategory: this.modelSourceCategory,
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
        editReferenceImages: this.editReferenceImages,
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
        animaTeacacheEnabled: this.animaTeacacheEnabled,
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
        customQualityTagsEnabled: this.customQualityTagsEnabled,
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
        advancedMode: this.advancedMode,
        resolutionLocked: this.resolutionLocked,
        autoSaveDirs: this.autoSaveDirs,
        regionalPrompts: this.regionalPrompts,
        regionalPromptStrategy: this.regionalPromptStrategy,
        videoVariant: this.videoVariant,
        videoDurationSeconds: this.videoDurationSeconds,
        videoMegapixels: this.videoMegapixels,
        videoAspectRatio: this.videoAspectRatio,
        videoFirstFrame: this.videoFirstFrame,
        videoLastFrame: this.videoLastFrame,
        videoFirstFrameAspect: this.videoFirstFrameAspect,
        videoLastFrameAspect: this.videoLastFrameAspect,
        videoFirstFrameAsLast: this.videoFirstFrameAsLast,
        videoRefImages: this.videoRefImages,
        videoRifeEnabled: this.videoRifeEnabled,
        videoRifeMultiplier: this.videoRifeMultiplier,
        videoRifeScaleFactor: this.videoRifeScaleFactor,
        videoRifeFastMode: this.videoRifeFastMode,
        videoRifeEnsemble: this.videoRifeEnsemble,
        videoTurboEnabled: this.videoTurboEnabled,
        videoTurboSteps: this.videoTurboSteps,
        videoTeacacheEnabled: this.videoTeacacheEnabled,
        videoDiffusionModel: this.videoDiffusionModel,
        videoClipModel: this.videoClipModel,
        videoVaeModel: this.videoVaeModel,
        videoAudioVaeModel: this.videoAudioVaeModel,
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
      promptBuckets: this.promptBucketsWithCurrent(),
      positivePrompt: this.positivePrompt,
      negativePrompt: this.negativePrompt,
      extraPositiveBoxes: this.extraPositiveBoxes,
      extraNegativeBoxes: this.extraNegativeBoxes,
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
      refineOnly: this.refineOnly,
      differentialDiffusion: this.differentialDiffusion,
      upscaleEnabled: this.upscaleEnabled,
      upscaleMethod: this.upscaleMethod,
      upscaleModel: this.upscaleModel,
      upscaleScale: this.upscaleScale,
      upscaleTargetScaleEnabled: this.upscaleTargetScaleEnabled,
      upscaleTargetScale: this.upscaleTargetScale,
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
      modelSourceCategory: this.modelSourceCategory,
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
      editReferenceImages: this.editReferenceImages,
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
      animaTeacacheEnabled: this.animaTeacacheEnabled,
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
      customQualityTagsEnabled: this.customQualityTagsEnabled,
      customAnimaPositiveQuality: this.customAnimaPositiveQuality,
      customAnimaNegativeQuality: this.customAnimaNegativeQuality,
      customIllustriousPositiveQuality: this.customIllustriousPositiveQuality,
      customIllustriousNegativeQuality: this.customIllustriousNegativeQuality,
      customPonyPositiveQuality: this.customPonyPositiveQuality,
      customPonyNegativeQuality: this.customPonyNegativeQuality,
      customNanosaurPositiveQuality: this.customNanosaurPositiveQuality,
      customNanosaurNegativeQuality: this.customNanosaurNegativeQuality,
      manualSaveMode: this.manualSaveMode,
      advancedMode: this.advancedMode,
      resolutionLocked: this.resolutionLocked,
      autoSaveDirs: this.autoSaveDirs,
      regionalPrompts: this.regionalPrompts,
      regionalPromptStrategy: this.regionalPromptStrategy,
      modelFamilyOverrides: this.modelFamilyOverrides,
      videoVariant: this.videoVariant,
      videoDurationSeconds: this.videoDurationSeconds,
      videoMegapixels: this.videoMegapixels,
      videoAspectRatio: this.videoAspectRatio,
      videoFirstFrame: this.videoFirstFrame,
      videoLastFrame: this.videoLastFrame,
      videoFirstFrameAspect: this.videoFirstFrameAspect,
      videoLastFrameAspect: this.videoLastFrameAspect,
      videoFirstFrameAsLast: this.videoFirstFrameAsLast,
      videoRefImages: this.videoRefImages,
      videoRifeEnabled: this.videoRifeEnabled,
      videoRifeMultiplier: this.videoRifeMultiplier,
      videoRifeScaleFactor: this.videoRifeScaleFactor,
      videoRifeFastMode: this.videoRifeFastMode,
      videoRifeEnsemble: this.videoRifeEnsemble,
      videoTurboEnabled: this.videoTurboEnabled,
      videoTurboSteps: this.videoTurboSteps,
      videoTeacacheEnabled: this.videoTeacacheEnabled,
      videoDiffusionModel: this.videoDiffusionModel,
      videoClipModel: this.videoClipModel,
      videoVaeModel: this.videoVaeModel,
      videoAudioVaeModel: this.videoAudioVaeModel,
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
    // Video mode loads its own UNet/CLIP/VAE trio from the `video_*` fields and
    // never touches `checkpoint`, so the image-pipeline model guards below would
    // block generation over state video does not use.
    const isVideo = this._mode === "video";

    if (!isVideo && this.useSplitModel) {
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

    // Diffusion-only families (Flux, Anima, Wan, Qwen, Chroma, ...) carry no
    // text encoder in a single checkpoint. If one was placed in `checkpoints/`
    // and loaded via CheckpointLoaderSimple, ComfyUI returns a None CLIP and
    // fails with "clip input is invalid: None". Surface an actionable error.
    if (!isVideo && !this.useSplitModel && familyRequiresSeparateClip(this.modelFamily)) {
      throw new Error(
        `"${this.checkpoint}" is a ${this.modelFamily} diffusion model with no built-in text encoder. ` +
          `Move it to ComfyUI's diffusion_models/ folder and select it as a diffusion model, ` +
          `or choose a full checkpoint instead.`,
      );
    }

    const style = this.stylePresetsEnabled
      ? (STYLE_PRESETS.find((preset) => preset.id === this.stylePreset) ?? STYLE_PRESETS[0])
      : STYLE_PRESETS[0];

    // Concatenate the main prompt with any extra boxes (chronological order,
    // like chained ComfyUI string-concatenate nodes), then strip BREAK/<break>
    // and layout newlines. This runs before inline preset resolution so that
    // `@preset:` tokens inside extra boxes still flow through the pipeline.
    const effectivePositive = sanitizePromptForSend(
      joinPromptBoxes([this.positivePrompt, ...this.extraPositiveBoxes.map((b) => b.content)])
    );
    const effectiveNegative = sanitizePromptForSend(
      joinPromptBoxes([this.negativePrompt, ...this.extraNegativeBoxes.map((b) => b.content)])
    );

    // Expand inline `@preset:<slug>` directives in the user-typed prompts
    // first, so wildcard rolls happen before any merging/dedup logic. Each
    // occurrence rolls independently.
    const inlinePositiveIds = promptPresets.inlinePresetIds(effectivePositive);
    const inlineNegativeIds = promptPresets.inlinePresetIds(effectiveNegative);
    const inlinePresetIds = new Set([...inlinePositiveIds, ...inlineNegativeIds]);
    const inlinePositive = promptPresets.resolveInline(effectivePositive, {
      fixedChoices: options.fixedPresetChoices,
    });
    const inlineNegative = promptPresets.resolveInline(effectiveNegative, {
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

    // Auto-apply quality tags for supported model families.
    //
    // Skipped in video mode: the family getters read `modelFamily`, which comes
    // from the image checkpoint and stays selected while video is active, so
    // without this guard H3 prose picks up `masterpiece, best quality` and
    // friends from whatever SDXL model happens to be loaded.
    if (!isVideo && this.autoQualityTags) {
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
    if (!isVideo && upscaleUsesTiling && this.autoQualityTags) {
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

    const translatedPositiveBase = translatePromptWeightSyntax(parsedPositive.baseText);
    const translatedPositiveSegments = parsedPositive.segments.map((s) => ({
      text: translatePromptWeightSyntax(s.text),
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

    // Compiled H3 Director timeline, or null when the timeline is off or empty
    // - the backend then builds the plain native H3 graph. Routed through a
    // registration seam so this hub store never imports the feature store
    // (see `utils/timelineProvider.ts`).
    const timeline = isVideo ? compileTimeline(translatedPositiveBase) : null;

    const params: GenerationParams = {
      mode: this.mode,
      positive_prompt: translatedPositiveBase,
      negative_prompt: translatePromptWeightSyntax(parsedNegative.baseText),
      positive_segments: translatedPositiveSegments,
      negative_segments: parsedNegative.segments.map((s) => ({
        text: translatePromptWeightSyntax(s.text),
        start: s.start,
        end: s.end,
      })),
      detail_segments: parsedSegmentDetails.segments.map((s) => ({
        target: s.target,
        prompt: translatePromptWeightSyntax(s.prompt),
        creativity: s.creativity,
        threshold: s.threshold,
      })),
      positive_regions: builtRegions,
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
      upscale_model_downscale_ratio: this.upscaleModelDownscaleRatio,
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
      model_source_category: this.modelSourceCategory,
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
      anima_teacache_enabled: this.animaTeacacheEnabled,
      edit_reference_images: this.editReferenceImages.filter((v): v is string => !!v),
      video_variant: this.videoVariant,
      video_duration_seconds: this.videoDurationSeconds,
      video_megapixels: this.videoMegapixels,
      // "auto" is UI-only: resolve it to the uploaded frame's own W:H so the
      // backend always receives a numeric ratio it can parse.
      video_aspect_ratio: this.resolvedVideoAspectRatio,
      // fl2va's frame slots are optional (no frames = text-to-video), so a
      // stale ref2va-only state must not leak first/last frames and vice versa.
      video_first_frame: this.videoVariant === "fl2va" ? this.videoFirstFrame : null,
      video_last_frame: this.videoVariant === "fl2va" ? this.videoEffectiveLastFrame : null,
      video_ref_images: this.videoVariant === "ref2va" ? this.videoRefImageFilenames : [],
      video_rife_enabled: this.videoRifeEnabled,
      video_rife_multiplier: this.videoRifeMultiplier,
      video_rife_scale_factor: this.videoRifeScaleFactor,
      video_rife_fast_mode: this.videoRifeFastMode,
      video_rife_ensemble: this.videoRifeEnsemble,
      video_turbo_enabled: this.videoTurboEnabled,
      video_turbo_steps: this.videoTurboSteps,
      video_turbo_lora: this.videoTurboEnabled ? H3_TURBO_LORA.filename : null,
      video_teacache_enabled: this.videoTeacacheEnabled,
      video_diffusion_model: this.videoDiffusionModel,
      video_clip_model: this.videoClipModel,
      video_vae_model: this.videoVaeModel,
      video_audio_vae_model: this.videoAudioVaeModel,
      video_timeline_data: timeline?.data ?? null,
      // Node widgets rather than `timeline_data` keys, so they travel beside it.
      video_timeline_custom_motion: timeline?.useCustomMotion ?? false,
      video_timeline_custom_audio: timeline?.useCustomAudio ?? false,
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
    const removed = this.loras[index];
    this.loras = this.loras.filter((_, i) => i !== index);
    if (removed?.insertedWords?.length) {
      this.removeInsertedWordsFromPrompt(removed.insertedWords);
    }
  }

  toggleLora(index: number) {
    const target = this.loras[index];
    const disabling = !!target?.enabled;
    this.loras = this.loras.map((l, i) =>
      i === index ? { ...l, enabled: !l.enabled } : l
    );
    if (disabling && target?.insertedWords?.length) {
      this.removeInsertedWordsFromPrompt(target.insertedWords);
    }
  }

  /** Record a trigger word inserted into the prompt via a LoRA's trigger-word chip, so it can be removed on deselect. */
  recordInsertedLoraWord(loraName: string, word: string) {
    this.loras = this.loras.map((l) =>
      l.name === loraName && !(l.insertedWords ?? []).includes(word)
        ? { ...l, insertedWords: [...(l.insertedWords ?? []), word] }
        : l
    );
  }

  /** Strip trigger words previously inserted via addTriggerWord/recordInsertedLoraWord, removing each as its own comma-delimited segment so surrounding text is untouched. */
  private removeInsertedWordsFromPrompt(words: string[]) {
    let text = this.positivePrompt;
    for (const word of words) {
      const trimmed = word.trim();
      if (!trimmed) continue;
      const segments = text.split(",");
      const idx = segments.findIndex((s) => s.trim() === trimmed);
      if (idx === -1) continue;
      segments.splice(idx, 1);
      text = segments.join(",").replace(/^\s*,\s*/, "").replace(/,\s*$/, "").trim();
    }
    if (text !== this.positivePrompt) {
      this.positivePrompt = text;
    }
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
