import { getModels, getSamplers, getEmbeddings } from "../utils/api.js";

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
  loading = $state(false);

  async refresh() {
    this.loading = true;
    try {
      console.log("ModelsStore: fetching models...");
      // Text encoders may live in either `text_encoders/` (modern split-file
      // layout) or `clip/` (legacy ComfyUI / Forge layout). Fetch both and
      // merge so the picker doesn't miss encoders in the legacy directory
      // (e.g. `qwen_3_8b_fp4mixed.safetensors` placed under `clip/`).
      const [checkpoints, vaes, loras, samplerInfo, embeddings, upscaleModels, diffusionModels, unetModels, textEncoders, clipEncoders, controlnetModels, ultralyticsModels] =
        await Promise.all([
          getModels("checkpoints"),
          getModels("vae"),
          getModels("loras"),
          getSamplers(),
          getEmbeddings(),
          getModels("upscale_models"),
          getModels("diffusion_models").catch(() => [] as string[]),
          // ComfyUI also exposes UNET/diffusion weights under `unet/` on some installs.
          getModels("unet").catch(() => [] as string[]),
          getModels("text_encoders").catch(() => [] as string[]),
          getModels("clip").catch(() => [] as string[]),
          getModels("controlnet").catch(() => [] as string[]),
          getModels("ultralytics").catch(() => [] as string[]),
        ]);

      console.log("ModelsStore: got checkpoints:", checkpoints);
      console.log("ModelsStore: got samplers:", samplerInfo);

      this.checkpoints = checkpoints;
      this.vaes = vaes;
      this.loras = loras;
      this.samplers = samplerInfo.samplers;
      this.schedulers = samplerInfo.schedulers;
      this.embeddings = embeddings;
      this.upscaleModels = upscaleModels;
      this.diffusionModels = Array.from(new Set([...diffusionModels, ...unetModels]));
      // De-duplicate by basename — ComfyUI sometimes returns the same file under
      // both `clip` and `text_encoders` when both directories are mapped.
      this.textEncoders = Array.from(new Set([...textEncoders, ...clipEncoders]));
      this.controlnetModels = controlnetModels;
      this.ultralyticsModels = ultralyticsModels;
    } catch (e) {
      console.error("Failed to refresh models:", e);
    } finally {
      this.loading = false;
    }
  }
}

export const models = new ModelsStore();
