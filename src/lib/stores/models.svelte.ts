import { detectH3VideoLoras, getConfig, getModels, getSamplers, getEmbeddings, listModelFiles } from "../utils/api.js";
import { localOnlyModels } from "../utils/modelAvailability.js";

class ModelsStore {
  checkpoints = $state<string[]>([]);
  vaes = $state<string[]>([]);
  loras = $state<string[]>([]);
  /** The entries of `loras` that are MiniMax H3 video LoRAs (header-checked). */
  h3VideoLoras = $state<string[]>([]);
  samplers = $state<string[]>([]);
  schedulers = $state<string[]>([]);
  embeddings = $state<string[]>([]);
  upscaleModels = $state<string[]>([]);
  diffusionModels = $state<string[]>([]);
  textEncoders = $state<string[]>([]);
  controlnetModels = $state<string[]>([]);
  ultralyticsModels = $state<string[]>([]);
  /** ModelPatchLoader weights (`models/model_patches/`), e.g. Anima LLLite. */
  modelPatches = $state<string[]>([]);
  loading = $state(false);
  remote = $state(false);
  serverUrl = $state("");
  cacheScope = $state("");
  serverModels = $state<Record<string, string[]>>({});
  localOnly = $state<Record<string, string[]>>({});
  private refreshId = 0;

  private clearInventory() {
    this.serverModels = {};
    this.localOnly = {};
    this.checkpoints = [];
    this.vaes = [];
    this.loras = [];
    this.h3VideoLoras = [];
    this.embeddings = [];
    this.upscaleModels = [];
    this.diffusionModels = [];
    this.textEncoders = [];
    this.controlnetModels = [];
    this.ultralyticsModels = [];
    this.modelPatches = [];
    this.samplers = [];
    this.schedulers = [];
  }

  /**
   * LoRAs for the image model: everything except the H3 video LoRAs. MiniMax-H3-Image
   * shares the H3 DiT, so its LoRAs (pDMD 2-step among them) can't be told apart
   * from video ones and it keeps the whole list.
   */
  imageLorasFor(family: string): string[] {
    if (this.h3VideoLoras.length === 0 || family.startsWith("minimaxh3image")) return this.loras;
    const video = new Set(this.h3VideoLoras);
    return this.loras.filter((name) => !video.has(name));
  }

  /**
   * Sort the LoRA list into image and H3 video LoRAs. Runs after the list is
   * shown rather than before: it reads every LoRA's header, and a picker that
   * briefly lists an H3 LoRA among the image ones is better than a late list.
   */
  private async classifyLoras(refreshId: number, loras: string[]) {
    try {
      const video = loras.length > 0 ? await detectH3VideoLoras(loras) : [];
      if (refreshId === this.refreshId) this.h3VideoLoras = video;
    } catch (e) {
      console.warn("ModelsStore: H3 LoRA detection failed:", e);
    }
  }

  async refresh(): Promise<boolean> {
    const refreshId = ++this.refreshId;
    this.loading = true;
    try {
      const config = await getConfig();
      if (refreshId !== this.refreshId) return false;
      const cacheScope = JSON.stringify([config.server_url, config.comfyui_path, config.extra_model_paths]);
      if (this.cacheScope !== cacheScope) this.clearInventory();
      this.remote = config.server_mode === "remote";
      this.serverUrl = config.server_url;
      this.cacheScope = cacheScope;
      console.log("ModelsStore: fetching models...");
      // Text encoders may live in either `text_encoders/` (modern split-file
      // layout) or `clip/` (legacy ComfyUI / Forge layout). Fetch both and
      // merge so the picker doesn't miss encoders in the legacy directory
      // (e.g. `qwen_3_8b_fp4mixed.safetensors` placed under `clip/`).
      // ComfyUI lists only its standard weight extensions per folder, so .gguf
      // files are invisible under `diffusion_models`/`text_encoders`. The
      // ComfyUI-GGUF pack exposes them as `unet_gguf`/`clip_gguf` (same folders,
      // same relative names); those categories 404 when the pack is missing.
      const [checkpoints, vaes, loras, samplerInfo, embeddings, upscaleModels, diffusionModels, unetModels, ggufDiffusionModels, textEncoders, clipEncoders, ggufEncoders, controlnetModels, ultralyticsModels, modelPatches] =
        await Promise.all([
          getModels("checkpoints"),
          getModels("vae"),
          getModels("loras"),
          getSamplers(),
          getEmbeddings(),
          getModels("upscale_models"),
          getModels("diffusion_models").catch(() => [] as string[]),
          getModels("unet").catch(() => [] as string[]),
          getModels("unet_gguf").catch(() => [] as string[]),
          getModels("text_encoders").catch(() => [] as string[]),
          getModels("clip").catch(() => [] as string[]),
          getModels("clip_gguf").catch(() => [] as string[]),
          getModels("controlnet").catch(() => [] as string[]),
          getModels("ultralytics").catch(() => [] as string[]),
          getModels("model_patches").catch(() => [] as string[]),
        ]);

      console.log("ModelsStore: got checkpoints:", checkpoints);
      console.log("ModelsStore: got samplers:", samplerInfo);

      const mergedEncoders = Array.from(new Set([...(textEncoders ?? []), ...(clipEncoders ?? []), ...(ggufEncoders ?? [])]));
      const mergedDiffusionModels = Array.from(new Set([...(diffusionModels ?? []), ...(unetModels ?? []), ...(ggufDiffusionModels ?? [])]));
      const inventory: Record<string, string[]> = {
        checkpoints: checkpoints ?? [], vae: vaes ?? [], loras: loras ?? [],
        embeddings: embeddings ?? [], upscale_models: upscaleModels ?? [],
        diffusion_models: mergedDiffusionModels, unet: mergedDiffusionModels,
        text_encoders: mergedEncoders, clip: mergedEncoders,
        controlnet: controlnetModels ?? [], ultralytics: ultralyticsModels ?? [],
        model_patches: modelPatches ?? [],
      };
      const localOnly = Object.fromEntries(await Promise.all(
        Object.entries(inventory).map(async ([category, available]) => {
          const disk = await listModelFiles(category).catch(() => []);
          return [category, localOnlyModels(disk.map((file) => file.filename), available)];
        }),
      ));
      if (refreshId !== this.refreshId) return false;
      this.serverModels = inventory;
      this.localOnly = localOnly;
      this.checkpoints = inventory.checkpoints;
      this.vaes = inventory.vae;
      this.loras = inventory.loras;
      void this.classifyLoras(refreshId, inventory.loras);
      this.embeddings = inventory.embeddings;
      this.upscaleModels = inventory.upscale_models;
      this.diffusionModels = mergedDiffusionModels;
      this.textEncoders = mergedEncoders;
      this.controlnetModels = inventory.controlnet;
      this.ultralyticsModels = inventory.ultralytics;
      this.modelPatches = inventory.model_patches;
      this.samplers = samplerInfo.samplers;
      this.schedulers = samplerInfo.schedulers;
      return true;
    } catch (e) {
      console.error("Failed to refresh models:", e);
      if (refreshId === this.refreshId) {
        this.clearInventory();
      }
      return false;
    } finally {
      if (refreshId === this.refreshId) this.loading = false;
    }
  }
}

export const models = new ModelsStore();
