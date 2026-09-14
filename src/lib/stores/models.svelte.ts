import { getConfig, getModels, getSamplers, getEmbeddings, listModelFiles } from "../utils/api.js";
import { localOnlyModels } from "../utils/modelAvailability.js";

class ModelsStore {
  checkpoints = $state<string[]>([]);
  vaes = $state<string[]>([]);
  loras = $state<string[]>([]);
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
      const [checkpoints, vaes, loras, samplerInfo, embeddings, upscaleModels, diffusionModels, unetModels, textEncoders, clipEncoders, controlnetModels, ultralyticsModels, modelPatches] =
        await Promise.all([
          getModels("checkpoints"),
          getModels("vae"),
          getModels("loras"),
          getSamplers(),
          getEmbeddings(),
          getModels("upscale_models"),
          getModels("diffusion_models").catch(() => [] as string[]),
          getModels("unet").catch(() => [] as string[]),
          getModels("text_encoders").catch(() => [] as string[]),
          getModels("clip").catch(() => [] as string[]),
          getModels("controlnet").catch(() => [] as string[]),
          getModels("ultralytics").catch(() => [] as string[]),
          getModels("model_patches").catch(() => [] as string[]),
        ]);

      console.log("ModelsStore: got checkpoints:", checkpoints);
      console.log("ModelsStore: got samplers:", samplerInfo);

      const mergedEncoders = Array.from(new Set([...(textEncoders ?? []), ...(clipEncoders ?? [])]));
      const mergedDiffusionModels = Array.from(new Set([...(diffusionModels ?? []), ...(unetModels ?? [])]));
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
