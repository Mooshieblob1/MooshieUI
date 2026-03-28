import { load } from "@tauri-apps/plugin-store";
import type { LoraEntry } from "../types/index.js";
import { autocomplete } from "./autocomplete.svelte.js";

const STORE_KEY = "generation-settings";
const PROMPT_HISTORY_KEY = "mooshieui.promptHistory.v1";
const MAX_PROMPT_HISTORY = 100;

type StylePresetId = "none" | "anime" | "cinematic" | "photoreal" | "digital_art" | "line_art";

interface StylePreset {
  id: StylePresetId;
  label: string;
  positive: string;
  negative: string;
}

interface PromptHistoryEntry {
  id: string;
  positivePrompt: string;
  negativePrompt: string;
  mode: "txt2img" | "img2img" | "inpainting";
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
    negative: "photo, realistic skin texture, grainy"
  },
  {
    id: "cinematic",
    label: "Cinematic",
    positive: "cinematic lighting, dramatic composition, film still, volumetric light",
    negative: "flat lighting, low contrast"
  },
  {
    id: "photoreal",
    label: "Photoreal",
    positive: "photorealistic, ultra-detailed, natural lighting, high dynamic range",
    negative: "cartoon, anime, painting, cgi"
  },
  {
    id: "digital_art",
    label: "Digital Art",
    positive: "digital painting, concept art, painterly details, high detail",
    negative: "low detail, flat colors"
  },
  {
    id: "line_art",
    label: "Line Art",
    positive: "line art, clean outlines, monochrome illustration",
    negative: "heavy shading, photorealistic texture, noisy background"
  },
];

/** Default quality tags for Anima models */
export const DEFAULT_ANIMA_POSITIVE_QUALITY = "newest, masterpiece, best quality, score_9, score_8, safe, highres";
export const DEFAULT_ANIMA_NEGATIVE_QUALITY = "worst quality, low quality, score_1, score_2, score_3, blurry, jpeg artifacts, sepia";

/** Default quality tags for Illustrious/NoobAI family models (SIH, NoobAI vpred, etc.) */
export const DEFAULT_ILLUSTRIOUS_POSITIVE_QUALITY = "best quality, masterpiece, absurdres, newest, very aesthetic";
export const DEFAULT_ILLUSTRIOUS_NEGATIVE_QUALITY = "worst quality, bad quality, low quality, lowres, artistic error, bad anatomy, extra fingers, text, signature, watermark, long body, bad hands, cropped, username";

class GenerationStore {
  mode = $state<"txt2img" | "img2img" | "inpainting">("txt2img");
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
  facefixEnabled = $state(false);
  facefixDetector = $state<string | null>(null);
  facefixDenoise = $state(0.4);
  facefixSteps = $state(20);
  facefixGuideSize = $state(512);
  facefixMaxFaces = $state(8);
  outputBitDepth = $state<"8bit" | "16bit">("8bit");
  metadataMode = $state<"text_chunk" | "stealth" | "both">("both");
  autoQualityTags = $state(true);
  customAnimaPositiveQuality = $state(DEFAULT_ANIMA_POSITIVE_QUALITY);
  customAnimaNegativeQuality = $state(DEFAULT_ANIMA_NEGATIVE_QUALITY);
  customIllustriousPositiveQuality = $state(DEFAULT_ILLUSTRIOUS_POSITIVE_QUALITY);
  customIllustriousNegativeQuality = $state(DEFAULT_ILLUSTRIOUS_NEGATIVE_QUALITY);
  promptHistory = $state<PromptHistoryEntry[]>([]);

  /** Architecture detected from modelspec metadata, or null if not yet read. */
  modelspecArchitecture = $state<string | null>(null);

  /** True when the selected model is an Anima variant (split diffusion model). */
  get isAnima(): boolean {
    return this.useSplitModel && (this.diffusionModel?.includes("anima") ?? false);
  }

  /** True when the selected model is an Illustrious/NoobAI family variant. */
  get isIllustrious(): boolean {
    return this.detectedArchitecture === "illustrious";
  }

  /** Detect the base model architecture from modelspec (authoritative) or filename (fallback).
   *  Returns "illustrious" for NoobAI/Illustrious/vpred SDXL variants (need different ControlNets),
   *  "sdxl" for standard SDXL eps, "sd15" for SD1.5, or "unknown". */
  get detectedArchitecture(): "sdxl" | "illustrious" | "sd15" | "unknown" {
    const name = (this.diffusionModel ?? this.checkpoint ?? "").toLowerCase();

    // 1. Use modelspec architecture if available (definitive)
    if (this.modelspecArchitecture) {
      const arch = this.modelspecArchitecture.toLowerCase();
      // Check for Illustrious/NoobAI family first (they report as SDXL arch but need special ControlNets)
      if (name.includes("illustrious") || name.includes("noobai") || name.includes("noob") || name.includes("sih")) return "illustrious";
      if (arch.includes("xl") || arch.includes("sdxl") || arch.includes("pony")) return "sdxl";
      if (arch.includes("sd-1") || arch.includes("sd1") || arch.includes("v1-")) return "sd15";
    }

    // 2. Fall back to filename heuristics
    if (!name) return "unknown";
    // Illustrious/NoobAI/vpred SDXL variants — need dedicated ControlNets
    if (name.includes("illustrious") || name.includes("noobai") || name.includes("noob") || name.includes("sih")) return "illustrious";
    if (name.includes("sdxl") || name.includes("xl") || name.includes("flux") || name.includes("pony")) return "sdxl";
    if (name.includes("1.5") || name.includes("sd15") || name.includes("sd_15")) return "sd15";
    return "unknown";
  }

  private _store: Awaited<ReturnType<typeof load>> | null = null;

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
      id: existing?.id ?? crypto.randomUUID(),
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

  applyModelSpecificPreset(modelName?: string | null) {
    const name = (modelName ?? this.diffusionModel ?? this.checkpoint ?? "").toLowerCase();
    if (!name) return;

    const isAnima = name.includes("anima") || name.includes("qwen") || name.includes("wan");
    autocomplete.notifyModelChanged(isAnima);

    if (isAnima) {
      this.steps = 30;
      this.cfg = 4.0;
      this.samplerName = "er_sde";
      this.scheduler = "sgm_uniform";
      this.width = 1024;
      this.height = 1024;
      // Face fix and upscale steps are 1/3 of main image steps
      this.facefixSteps = Math.ceil(30 / 3);
      this.upscaleSteps = Math.ceil(30 / 3);
      return;
    }

    if (name.includes("sdxl") || name.includes("flux") || name.includes("sih") || name.includes("xl")) {
      this.steps = 20;
      this.cfg = 1.4;
      this.samplerName = "euler_cfg_pp";
      this.scheduler = "sgm_uniform";
      this.width = 1024;
      this.height = 1024;
      // Face fix and upscale steps are 1/3 of main image steps
      this.facefixSteps = Math.ceil(20 / 3);
      this.upscaleSteps = Math.ceil(20 / 3);
      return;
    }

    if (name.includes("1.5") || name.includes("sd15") || name.includes("sd_15")) {
      this.steps = 28;
      this.cfg = 7.0;
      this.samplerName = "dpmpp_2m";
      this.scheduler = "karras";
      this.width = 512;
      this.height = 512;
      // Face fix and upscale steps are 1/3 of main image steps
      this.facefixSteps = Math.ceil(28 / 3);
      this.upscaleSteps = Math.ceil(28 / 3);
    }
  }

  async loadSettings() {
    try {
      this._store = await load("settings.json", { autoSave: true });
      const saved = await this._store.get<Record<string, any>>(STORE_KEY);
      if (saved) {
        if (saved.checkpoint) this.checkpoint = saved.checkpoint;
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
        if (saved.mode) this.mode = saved.mode;
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
        if (saved.facefixEnabled !== undefined) this.facefixEnabled = saved.facefixEnabled;
        if (saved.facefixDetector !== undefined) this.facefixDetector = saved.facefixDetector;
        if (saved.facefixDenoise !== undefined) this.facefixDenoise = saved.facefixDenoise;
        if (saved.facefixSteps !== undefined) this.facefixSteps = saved.facefixSteps;
        if (saved.facefixGuideSize !== undefined) this.facefixGuideSize = saved.facefixGuideSize;
        if (saved.facefixMaxFaces !== undefined) this.facefixMaxFaces = saved.facefixMaxFaces;
        if (saved.outputBitDepth) this.outputBitDepth = saved.outputBitDepth;
        if (saved.metadataMode) this.metadataMode = saved.metadataMode;
        if (saved.autoQualityTags !== undefined) this.autoQualityTags = saved.autoQualityTags;
        if (saved.customAnimaPositiveQuality !== undefined) this.customAnimaPositiveQuality = saved.customAnimaPositiveQuality;
        if (saved.customAnimaNegativeQuality !== undefined) this.customAnimaNegativeQuality = saved.customAnimaNegativeQuality;
        if (saved.customIllustriousPositiveQuality !== undefined) this.customIllustriousPositiveQuality = saved.customIllustriousPositiveQuality;
        if (saved.customIllustriousNegativeQuality !== undefined) this.customIllustriousNegativeQuality = saved.customIllustriousNegativeQuality;
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
    if (!this._store) return;
    try {
      await this._store.set(STORE_KEY, {
        mode: this.mode,
        positivePrompt: this.positivePrompt,
        negativePrompt: this.negativePrompt,
        checkpoint: this.checkpoint,
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
        facefixEnabled: this.facefixEnabled,
        facefixDetector: this.facefixDetector,
        facefixDenoise: this.facefixDenoise,
        facefixSteps: this.facefixSteps,
        facefixGuideSize: this.facefixGuideSize,
        facefixMaxFaces: this.facefixMaxFaces,
        outputBitDepth: this.outputBitDepth,
        metadataMode: this.metadataMode,
        autoQualityTags: this.autoQualityTags,
        customAnimaPositiveQuality: this.customAnimaPositiveQuality,
        customAnimaNegativeQuality: this.customAnimaNegativeQuality,
        customIllustriousPositiveQuality: this.customIllustriousPositiveQuality,
        customIllustriousNegativeQuality: this.customIllustriousNegativeQuality,
      });
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  }

  toParams() {
    const style = this.stylePresetsEnabled
      ? (STYLE_PRESETS.find((preset) => preset.id === this.stylePreset) ?? STYLE_PRESETS[0])
      : STYLE_PRESETS[0];

    let positivePrompt = this.mergeTagPrompts(this.positivePrompt, style.positive);
    let negativePrompt = this.mergeTagPrompts(this.negativePrompt, style.negative);

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
    }

    // Build quality-only prompts for tiled upscale (reduces tile seam artifacts)
    let upscalePositivePrompt: string | null = null;
    let upscaleNegativePrompt: string | null = null;
    if (this.upscaleEnabled && this.upscaleTiling && this.autoQualityTags) {
      if (this.isAnima) {
        upscalePositivePrompt = this.customAnimaPositiveQuality;
        upscaleNegativePrompt = this.customAnimaNegativeQuality;
      } else if (this.isIllustrious) {
        upscalePositivePrompt = this.customIllustriousPositiveQuality;
        upscaleNegativePrompt = this.customIllustriousNegativeQuality;
      }
    }

    return {
      mode: this.mode,
      positive_prompt: positivePrompt,
      negative_prompt: negativePrompt,
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
      output_bit_depth: this.outputBitDepth,
    };
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
    if (this.checkpoint) return;

    // Look for the default SIH checkpoint
    const defaultCkpt = checkpoints.find((c) => c.includes("SIH-1.5"));
    if (defaultCkpt) {
      this.checkpoint = defaultCkpt;
      this.samplerName = "euler_cfg_pp";
      this.scheduler = "sgm_uniform";
      this.cfg = 1.4;
      this.steps = 20;
      this.width = 1024;
      this.height = 1024;
    } else if (checkpoints.length > 0) {
      this.checkpoint = checkpoints[0];
    }

    // Look for SDXL VAE
    if (!this.vae) {
      const defaultVae = vaes.find((v) => v.includes("sdxl_vae"));
      if (defaultVae) {
        this.vae = defaultVae;
      }
    }

    this.saveSettings();
  }
}

export const generation = new GenerationStore();
