import { getModels, getSamplers, getEmbeddings } from "../utils/api.js";

class ModelsStore {
  checkpoints = $state<string[]>([]);
  vaes = $state<string[]>([]);
  loras = $state<string[]>([]);
  samplers = $state<string[]>([]);
  schedulers = $state<string[]>([]);
  embeddings = $state<string[]>([]);
  upscaleModels = $state<string[]>([]);
  loading = $state(false);

  async refresh() {
    this.loading = true;
    try {
      console.log("ModelsStore: fetching models...");
      const [checkpoints, vaes, loras, samplerInfo, embeddings, upscaleModels] =
        await Promise.all([
          getModels("checkpoints"),
          getModels("vae"),
          getModels("loras"),
          getSamplers(),
          getEmbeddings(),
          getModels("upscale_models"),
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
    } catch (e) {
      console.error("Failed to refresh models:", e);
    } finally {
      this.loading = false;
    }
  }
}

export const models = new ModelsStore();
