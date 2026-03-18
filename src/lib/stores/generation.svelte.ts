import { load } from "@tauri-apps/plugin-store";
import type { LoraEntry } from "../types/index.js";

const STORE_KEY = "generation-settings";

/** Quality tags auto-applied for Anima models */
const ANIMA_POSITIVE_QUALITY = "year 2025, newest, masterpiece, best quality, score_9, score_8, safe, highres";
const ANIMA_NEGATIVE_QUALITY = "worst quality, low quality, score_1, score_2, score_3, blurry, jpeg artifacts, sepia";

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

  /** True when the selected model is an Anima variant (split diffusion model). */
  get isAnima(): boolean {
    return this.useSplitModel && (this.diffusionModel?.includes("anima") ?? false);
  }

  private _store: Awaited<ReturnType<typeof load>> | null = null;

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
        console.log("Loaded saved settings, checkpoint:", this.checkpoint);
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
      });
    } catch (e) {
      console.error("Failed to save settings:", e);
    }
  }

  toParams() {
    // Auto-apply quality tags for Anima models
    const positivePrompt = this.isAnima
      ? `${ANIMA_POSITIVE_QUALITY}, ${this.positivePrompt}`
      : this.positivePrompt;
    const negativePrompt = this.isAnima
      ? `${this.negativePrompt}${this.negativePrompt ? ", " : ""}${ANIMA_NEGATIVE_QUALITY}`
      : this.negativePrompt;

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
      use_split_model: this.useSplitModel,
      diffusion_model: this.diffusionModel,
      clip_model: this.clipModel,
      clip_type: this.clipType,
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
