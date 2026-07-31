import { getModels, getSamplers, getEmbeddings, listModelFiles } from "../utils/api.js";

function modelBasename(filename: string): string {
  const normalized = filename.replace(/\\/g, "/");
  const slash = normalized.lastIndexOf("/");
  return slash >= 0 ? normalized.slice(slash + 1) : normalized;
}

/**
 * Merge ComfyUI API model names with on-disk files from extra paths.
 * API entries win when basename matches; disk-only files are appended once.
 */
async function mergeWithDiskModels(category: string, apiModels: string[]): Promise<string[]> {
  const safeApiModels = apiModels ?? [];
  try {
    const disk = await listModelFiles(category);
    const diskNames = (disk ?? [])
      .filter((f) => f && typeof f.filename === "string")
      .map((f) => f.filename);

    const apiBasenames = new Set(safeApiModels.map(modelBasename));
    const merged = [...safeApiModels];
    for (const name of diskNames) {
      const base = modelBasename(name);
      if (!apiBasenames.has(base)) {
        merged.push(name);
        apiBasenames.add(base);
      }
    }
    return merged.sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
  } catch {
    return safeApiModels;
  }
}

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

  async refresh() {
    this.loading = true;
    try {
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
      let diffusionModelFiles: string[];
      let unetModelFiles: string[];

      [
        this.checkpoints,
        this.vaes,
        this.loras,
        this.embeddings,
        this.upscaleModels,
        diffusionModelFiles,
        unetModelFiles,
        this.textEncoders,
        this.controlnetModels,
        this.ultralyticsModels,
        this.modelPatches,
      ] = await Promise.all([
        mergeWithDiskModels("checkpoints", checkpoints),
        mergeWithDiskModels("vae", vaes),
        mergeWithDiskModels("loras", loras),
        mergeWithDiskModels("embeddings", embeddings),
        mergeWithDiskModels("upscale_models", upscaleModels),
        mergeWithDiskModels("diffusion_models", mergedDiffusionModels),
        mergeWithDiskModels("unet", unetModels),
        mergeWithDiskModels("text_encoders", mergedEncoders),
        mergeWithDiskModels("controlnet", controlnetModels),
        mergeWithDiskModels("ultralytics", ultralyticsModels),
        mergeWithDiskModels("model_patches", modelPatches),
      ]);
      this.diffusionModels = Array.from(new Set([...diffusionModelFiles, ...unetModelFiles])).sort((a, b) =>
        a.localeCompare(b, undefined, { sensitivity: "base" }),
      );
      this.samplers = samplerInfo.samplers;
      this.schedulers = samplerInfo.schedulers;
    } catch (e) {
      console.error("Failed to refresh models:", e);
    } finally {
      this.loading = false;
    }
  }
}

export const models = new ModelsStore();
