export interface LoraEntry {
  name: string;
  strength_model: number;
  strength_clip: number;
  enabled: boolean;
}

export interface GenerationParams {
  mode: "txt2img" | "img2img" | "inpainting";
  positive_prompt: string;
  negative_prompt: string;
  checkpoint: string;
  vae: string | null;
  loras: LoraEntry[];
  sampler_name: string;
  scheduler: string;
  steps: number;
  cfg: number;
  seed: number;
  width: number;
  height: number;
  batch_size: number;
  denoise: number;
  input_image: string | null;
  mask_image: string | null;
  grow_mask_by: number | null;
  upscale_enabled: boolean;
  upscale_method: string;
  upscale_model: string | null;
  upscale_scale: number;
  upscale_denoise: number;
  upscale_steps: number;
  upscale_tile_size: number;
  upscale_tiling: boolean;
}

export interface OutputImage {
  filename: string;
  subfolder: string;
  type: string;
  prompt_id: string;
  url?: string;
  gallery_filename?: string;
}

export interface SamplerInfo {
  samplers: string[];
  schedulers: string[];
}

export interface SystemStats {
  system: {
    os: string;
    ram_total: number;
    ram_free: number;
    comfyui_version?: string;
    python_version?: string;
    pytorch_version?: string;
  };
  devices: {
    name: string;
    type: string;
    vram_total: number;
    vram_free: number;
  }[];
}

export interface QueueInfo {
  queue_running: unknown[];
  queue_pending: unknown[];
}
