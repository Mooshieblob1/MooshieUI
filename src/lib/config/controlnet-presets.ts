export interface ControlNetModelEntry {
  filename: string;
  url: string;
  preprocessor?: string | null;
  defaults?: ControlNetDefaults;
}

export interface ControlNetDefaults {
  strength?: number;
  startPercent?: number;
  endPercent?: number;
}

export interface ControlNetPreset {
  id: string;
  label: string;
  description: string;
  preprocessor: string | null;
  preprocessorParams?: Record<string, number>;
  defaults?: ControlNetDefaults;
  requiresMode?: "inpainting";
  models: {
    /** Standard SDXL (eps prediction) */
    sdxl?: ControlNetModelEntry | null;
    /** Illustrious / NoobAI / vpred SDXL variants */
    illustrious?: ControlNetModelEntry | null;
    /** Stable Diffusion 1.5 */
    sd15?: ControlNetModelEntry | null;
    /** Flux.1 family */
    flux?: ControlNetModelEntry | null;
    /** Stable Diffusion 3 / 3.5 */
    sd3?: ControlNetModelEntry | null;
    /** Anima LLLite (Wan2.1 fine-tune) */
    anima?: ControlNetModelEntry | null;
  };
}

export const CONTROLNET_PRESETS: ControlNetPreset[] = [
  {
    id: "canny",
    label: "Canny Edge",
    description: "Detects edges — best for preserving structure and composition",
    preprocessor: "CannyEdgePreprocessor",
    preprocessorParams: { low_threshold: 100, high_threshold: 200 },
    models: {
      sdxl: {
        filename: "diffusers_xl_canny_full.safetensors",
        url: "https://huggingface.co/diffusers/controlnet-canny-sdxl-1.0/resolve/main/diffusion_pytorch_model.fp16.safetensors",
      },
      illustrious: {
        filename: "noob_sdxl_controlnet_canny.fp16.safetensors",
        url: "https://huggingface.co/Eugeoter/noob-sdxl-controlnet-canny/resolve/main/noob_sdxl_controlnet_canny.fp16.safetensors",
      },
      sd15: {
        filename: "control_v11p_sd15_canny_fp16.safetensors",
        url: "https://huggingface.co/comfyanonymous/ControlNet-v1-1_fp16_safetensors/resolve/main/control_v11p_sd15_canny_fp16.safetensors",
      },
      flux: {
        filename: "flux-canny-controlnet-v3.safetensors",
        url: "https://huggingface.co/XLabs-AI/flux-controlnet-collections/resolve/main/flux-canny-controlnet-v3.safetensors",
      },
      sd3: {
        filename: "sd3.5_large_controlnet_canny.safetensors",
        url: "https://huggingface.co/stabilityai/stable-diffusion-3.5-controlnets/resolve/main/sd3.5_large_controlnet_canny.safetensors",
      },
    },
  },
  {
    id: "depth",
    label: "Depth Map",
    description: "Estimates depth — good for maintaining spatial layout",
    preprocessor: "DepthAnythingV2Preprocessor",
    models: {
      sdxl: {
        filename: "diffusers_xl_depth_full.safetensors",
        url: "https://huggingface.co/diffusers/controlnet-depth-sdxl-1.0/resolve/main/diffusion_pytorch_model.fp16.safetensors",
      },
      illustrious: {
        filename: "noob_sdxl_controlnet_depth.fp16.safetensors",
        url: "https://huggingface.co/Eugeoter/noob-sdxl-controlnet-depth/resolve/main/diffusion_pytorch_model.fp16.safetensors",
      },
      sd15: {
        filename: "control_v11f1p_sd15_depth_fp16.safetensors",
        url: "https://huggingface.co/comfyanonymous/ControlNet-v1-1_fp16_safetensors/resolve/main/control_v11f1p_sd15_depth_fp16.safetensors",
      },
      flux: {
        filename: "flux-depth-controlnet-v3.safetensors",
        url: "https://huggingface.co/XLabs-AI/flux-controlnet-collections/resolve/main/flux-depth-controlnet-v3.safetensors",
      },
      sd3: {
        filename: "sd3.5_large_controlnet_depth.safetensors",
        url: "https://huggingface.co/stabilityai/stable-diffusion-3.5-controlnets/resolve/main/sd3.5_large_controlnet_depth.safetensors",
      },
      anima: {
        filename: "anima-lllite-depth-1.safetensors",
        url: "https://huggingface.co/Mooshie/Anima-LLLite/resolve/main/anima-lllite-depth-1.safetensors",
        defaults: { strength: 1.2, startPercent: 0, endPercent: 1 },
      },
    },
  },
  {
    id: "openpose",
    label: "OpenPose",
    description: "Detects human poses — match body position and stance",
    preprocessor: "OpenposePreprocessor",
    models: {
      sdxl: {
        filename: "thibaud_xl_openpose.safetensors",
        url: "https://huggingface.co/thibaud/controlnet-openpose-sdxl-1.0/resolve/main/OpenPoseXL2.safetensors",
      },
      illustrious: {
        filename: "noob_openpose.safetensors",
        url: "https://huggingface.co/Laxhar/noob_openpose/resolve/main/openpose_pre.safetensors",
      },
      sd15: {
        filename: "control_v11p_sd15_openpose_fp16.safetensors",
        url: "https://huggingface.co/comfyanonymous/ControlNet-v1-1_fp16_safetensors/resolve/main/control_v11p_sd15_openpose_fp16.safetensors",
      },
    },
  },
  {
    id: "lineart",
    label: "LineArt",
    description: "Extracts clean line drawings — great for illustrations",
    preprocessor: "LineArtPreprocessor",
    models: {
      illustrious: {
        filename: "noob_sdxl_controlnet_lineart_anime.fp16.safetensors",
        url: "https://huggingface.co/Eugeoter/noob-sdxl-controlnet-lineart_anime/resolve/main/diffusion_pytorch_model.fp16.safetensors",
      },
      sd15: {
        filename: "control_v11p_sd15_lineart_fp16.safetensors",
        url: "https://huggingface.co/comfyanonymous/ControlNet-v1-1_fp16_safetensors/resolve/main/control_v11p_sd15_lineart_fp16.safetensors",
      },
    },
  },
  {
    id: "scribble",
    label: "Scribble",
    description: "Hand-drawn sketch guidance — turn rough drawings into art",
    preprocessor: "ScribblePreprocessor",
    models: {
      sd15: {
        filename: "control_v11p_sd15_scribble_fp16.safetensors",
        url: "https://huggingface.co/comfyanonymous/ControlNet-v1-1_fp16_safetensors/resolve/main/control_v11p_sd15_scribble_fp16.safetensors",
      },
    },
  },
  {
    id: "anytest_1000",
    label: "AnyTest v1 (step 1000)",
    description: "Anima multi-task ControlNet — step 1000 checkpoint",
    preprocessor: null,
    models: {
      anima: {
        filename: "anima-lllite-any-test-like-1-step1000.safetensors",
        url: "https://huggingface.co/Mooshie/Anima-LLLite/resolve/main/anima-lllite-any-test-like-1-step1000.safetensors",
        defaults: { strength: 1.25, startPercent: 0, endPercent: 1 },
      },
    },
  },
  {
    id: "anytest_2000",
    label: "AnyTest v1 (step 2000)",
    description: "Anima multi-task ControlNet — step 2000 checkpoint",
    preprocessor: null,
    models: {
      anima: {
        filename: "anima-lllite-any-test-like-1-step2000.safetensors",
        url: "https://huggingface.co/Mooshie/Anima-LLLite/resolve/main/anima-lllite-any-test-like-1-step2000.safetensors",
        defaults: { strength: 1.0, startPercent: 0, endPercent: 1 },
      },
    },
  },
  {
    id: "inpainting",
    label: "Inpainting",
    description: "Anima inpainting ControlNet — requires a mask",
    preprocessor: null,
    requiresMode: "inpainting",
    models: {
      anima: {
        filename: "anima-lllite-inpainting-v1.safetensors",
        url: "https://huggingface.co/Mooshie/Anima-LLLite/resolve/main/anima-lllite-inpainting-v1.safetensors",
        defaults: { strength: 1.0, startPercent: 0, endPercent: 1 },
      },
    },
  },
  {
    id: "softedge",
    label: "Soft Edge",
    description: "Soft structural edges (HED) — natural edge preservation",
    preprocessor: "HEDPreprocessor",
    models: {
      illustrious: {
        filename: "noob_sdxl_controlnet_softedge_hed.fp16.safetensors",
        url: "https://huggingface.co/Eugeoter/noob-sdxl-controlnet-softedge_hed/resolve/main/diffusion_pytorch_model.fp16.safetensors",
      },
      sd15: {
        filename: "control_v11p_sd15_softedge_fp16.safetensors",
        url: "https://huggingface.co/comfyanonymous/ControlNet-v1-1_fp16_safetensors/resolve/main/control_v11p_sd15_softedge_fp16.safetensors",
      },
    },
  },
];

/** Look up a preset by ID */
export function getPreset(id: string): ControlNetPreset | undefined {
  return CONTROLNET_PRESETS.find((p) => p.id === id);
}

/** Get the model entry for a preset + architecture, or null if not available */
export function getPresetModel(
  presetId: string,
  arch: string,
): ControlNetModelEntry | null {
  const preset = getPreset(presetId);
  if (!preset) return null;

  // Map architectures to their ControlNet compatibility
  switch (arch) {
    case "anima":
      return preset.models.anima ?? null;
    case "flux":
      return preset.models.flux ?? null;
    case "sd3":
      return preset.models.sd3 ?? null;
    case "illustrious":
      return preset.models.illustrious ?? preset.models.sdxl ?? null;
    case "sdxl":
    case "pony":
    case "kolors":
      return preset.models.sdxl ?? null;
    case "sd15":
      return preset.models.sd15 ?? null;
    default:
      // Unknown or unsupported (auraflow, pixart, hunyuandit, cascade) — try best match
      return preset.models.illustrious ?? preset.models.sdxl ?? preset.models.sd15 ?? null;
  }
}

/** Get the preprocessor for a preset, respecting any per-model override */
export function getPresetPreprocessor(
  presetId: string,
  arch: string,
): string | null {
  const preset = getPreset(presetId);
  if (!preset) return null;

  // Per-model override takes priority
  const modelEntry = getPresetModel(presetId, arch);
  if (modelEntry && modelEntry.preprocessor !== undefined) {
    return modelEntry.preprocessor;
  }

  return preset.preprocessor;
}

/** Get default ControlNet strength/range for a preset, respecting any per-model override */
export function getPresetDefaults(
  presetId: string,
  arch: string,
): ControlNetDefaults | null {
  const preset = getPreset(presetId);
  if (!preset) return null;

  const modelEntry = getPresetModel(presetId, arch);
  return modelEntry?.defaults ?? preset.defaults ?? null;
}
