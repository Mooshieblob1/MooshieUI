export interface LoraEntry {
  name: string;
  strength_model: number;
  strength_clip: number;
  enabled: boolean;
}

export interface LoraPayloadEntry {
  name: string;
  strength_model: number;
  strength_clip: number;
}

export interface GenerationParams {
  mode: "txt2img" | "img2img" | "inpainting";
  positive_prompt: string;
  negative_prompt: string;
  checkpoint: string;
  vae: string | null;
  loras: LoraPayloadEntry[];
  sampler_name: string;
  scheduler: string;
  steps: number;
  cfg: number;
  seed: number;
  width: number;
  height: number;
  batch_size: number;
  denoise: number;
  differential_diffusion: boolean;
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
  use_split_model: boolean;
  diffusion_model: string | null;
  clip_model: string | null;
  clip_type: string | null;
}

export interface OutputImage {
  filename: string;
  subfolder: string;
  type: string;
  prompt_id: string;
  generation_mode?: "txt2img" | "img2img" | "inpainting";
  url?: string;
  gallery_filename?: string;
  file_size_bytes?: number;
  generated_at_ms?: number;
  metadata?: Record<string, string> | null;
}

export interface GalleryImageEntry {
  filename: string;
  size_bytes: number;
  modified_ms: number;
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

export interface AppConfig {
  server_mode: "autolaunch" | "remote";
  server_url: string;
  server_port: number;
  comfyui_path: string;
  venv_path: string;
  extra_args: string[];
  default_checkpoint: string | null;
  default_sampler: string;
  default_scheduler: string;
  default_steps: number;
  default_cfg: number;
  default_width: number;
  default_height: number;
  vram_mode: string;
  keep_alive: boolean;
  theme: string;
  font_scale: number;
  setup_complete: boolean;
  extra_model_paths: string | null;
}

export interface QueueInfo {
  queue_running: unknown[];
  queue_pending: unknown[];
}

export interface QueueDisplayItem {
  id: string;
  promptId: string;
  number?: number;
  mode?: string;
  summary: string;
  raw: unknown;
}
