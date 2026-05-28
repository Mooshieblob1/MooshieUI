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

export interface ControlNetPayload {
  enabled: boolean;
  preset: string | null;
  controlnet_model: string | null;
  preprocessor: string | null;
  image: string | null;
  strength: number;
  start_percent: number;
  end_percent: number;
}

export interface PromptSegment {
  text: string;
  start: number;
  end: number;
}

export interface PositiveRegion {
  text: string;
  x: number;
  y: number;
  width: number;
  height: number;
  strength: number;
}

export type RegionalPromptShape = "box" | "circle" | "lasso";

/** How regional prompts are applied in txt2img. */
export type RegionalPromptStrategy = "conditioning" | "inpaint_chain";

export interface RegionalPromptPoint {
  x: number;
  y: number;
}

export interface RegionalPromptSelection {
  id: string;
  shape: RegionalPromptShape;
  text: string;
  strength: number;
  x: number;
  y: number;
  width: number;
  height: number;
  points?: RegionalPromptPoint[];
}

export interface GenerationParams {
  mode: "txt2img" | "img2img" | "inpainting";
  positive_prompt: string;
  negative_prompt: string;
  positive_segments: PromptSegment[];
  negative_segments: PromptSegment[];
  positive_regions?: PositiveRegion[];
  /** Raw prompt text with scheduling tags intact, for metadata embedding */
  raw_positive_prompt: string;
  /** Raw prompt text with scheduling tags intact, for metadata embedding */
  raw_negative_prompt: string;
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
  upscale_soft_guidance: boolean;
  upscale_soft_guidance_multiplier: number;
  smart_guidance: boolean;
  /** FluxGuidance value (Flux Dev / Flux 2 Klein only). Default 3.5. */
  flux_guidance?: number;
  /** "Refine" mode: skip the main img2img sampler and feed the loaded image
   *  straight into the upscale chain. Mirrors SwarmUI's Refine button. */
  refine_only?: boolean;
  use_split_model: boolean;
  diffusion_model: string | null;
  clip_model: string | null;
  clip_type: string | null;
  controlnet: ControlNetPayload | null;
  model_architecture: string;
  output_bit_depth: string;
  /** Storage format for this generation: "png" (default) or "jxl". */
  output_format: string;
  /** Anima Untwisting RoPE style transfer (txt2img only). */
  style_transfer_enabled?: boolean;
  style_reference_image?: string | null;
  style_transfer_low_scale_end?: number;
  style_transfer_high_scale_start?: number;
  style_transfer_beta?: number;
  style_transfer_adain_strength?: number;
  style_transfer_rf_mode?: string;
  style_transfer_gamma?: number;
  style_transfer_gamma_curve?: number;
  style_transfer_norm_strength?: number;
  style_transfer_pmi_alpha?: number;
  style_transfer_megapixels?: number;
}

export interface OutputImage {
  filename: string;
  subfolder: string;
  type: string;
  prompt_id: string;
  generation_mode?: "txt2img" | "img2img" | "inpainting";
  is_upscaled?: boolean;
  url?: string;
  thumbnailUrl?: string;
  /** Full-resolution image URL served by the backend (with metadata). */
  fullImageUrl?: string;
  gallery_filename?: string;
  /** In-memory bytes for this session-only image. Avoids fetching blob: URLs in browser mode. */
  sessionBlob?: Blob;
  /** Server temp image filename for browser-mode generated images before they are persisted. */
  tempFilename?: string;
  /** Browser-display temp image filename when canonical output is not browser-decodable (for example JXL). */
  displayTempFilename?: string;
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
  auto_start: boolean;
  theme: string;
  theme_palette: string;
  font_scale: number;
  setup_complete: boolean;
  extra_model_paths: string | null;
  interrogator_general_threshold: number;
  interrogator_character_threshold: number;
  civitai_api_key: string | null;
  gallery_path: string | null;
  browser_mode: boolean;
  ui_server_port: number;
  lan_enabled: boolean;
  attention_backend: string;
  gpu_workers: Array<{
    gpu_index: number;
    port: number | null;
    enabled: boolean;
    label: string | null;
    vram_mode: string | null;
  }>;
  /** HTTP(S) proxy for git/pip when installing ControlNet custom nodes (optional). */
  network_proxy: string | null;
  /** PyPI index URL for pip/uv installs, e.g. a regional mirror (optional). */
  pip_index_url: string | null;
  /** Optional gallery output filename template. */
  output_filename_template: string | null;
  /** Optional webhook endpoint URL for generation lifecycle events. */
  webhook_url: string | null;
  webhook_events: string[];
  webhook_include_sensitive: boolean;
  webhook_allow_private_targets: boolean;
  theme_profile_id: string | null;
  theme_profiles: ThemeProfile[];
}

export interface ThemeTone {
  main: string;
  sub: string;
  trim: string;
  background: string;
  text: string;
}

export interface ThemeProfile {
  id: string;
  name: string;
  palette: "mooshie" | "nord" | "solarized" | "gruvbox" | "catppuccin" | "custom";
  dark: ThemeTone;
  light: ThemeTone;
  background_image: string | null;
  background_fade: number;
  logo_image: string | null;
  hide_branding: boolean;
}

export interface QueueInfo {
  queue_running: unknown[];
  queue_pending: unknown[];
  /** Ordered queue positions from the server's fair-queue tracker. */
  queue_positions?: Array<{
    prompt_id: string;
    position: number;
    /** Only present for admin/moderator callers. */
    username?: string | null;
  }>;
}

export interface QueueDisplayItem {
  id: string;
  promptId: string;
  number?: number;
  mode?: string;
  summary: string;
  raw: unknown;
}

export interface TagResult {
  name: string;
  confidence: number;
}

export interface InterrogationResult {
  character_tags: TagResult[];
  artist_tags: TagResult[];
  general_tags: TagResult[];
  copyright_tags: TagResult[];
  rating_tags: TagResult[];
}

export interface GpuWorkerInfo {
  worker_id: number;
  port: number;
  status: string;
  reserved: boolean;
  label: string;
}

export interface GpuStats {
  index: number;
  name: string;
  vram_used_mb: number;
  vram_total_mb: number;
  gpu_util: number;
  temperature: number;
  power_draw_w: number;
  worker: GpuWorkerInfo | null;
}
