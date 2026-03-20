export interface ControlNetModelEntry {
  filename: string;
  url: string;
}

export interface ControlNetPreset {
  id: string;
  label: string;
  description: string;
  preprocessor: string;
  preprocessorParams?: Record<string, number>;
  models: {
    /** Standard SDXL (eps prediction) */
    sdxl?: ControlNetModelEntry | null;
    /** Illustrious / NoobAI / vpred SDXL variants */
    illustrious?: ControlNetModelEntry | null;
    /** Stable Diffusion 1.5 */
    sd15?: ControlNetModelEntry | null;
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
  arch: "sdxl" | "illustrious" | "sd15" | "unknown",
): ControlNetModelEntry | null {
  const preset = getPreset(presetId);
  if (!preset) return null;
  if (arch === "unknown") {
    return preset.models.illustrious ?? preset.models.sdxl ?? preset.models.sd15 ?? null;
  }
  return preset.models[arch] ?? null;
}
