export interface LoraEntry {
  name: string;
  strength_model: number;
  strength_clip: number;
  enabled: boolean;
  /** Trigger words inserted into the prompt via the LoRA's trigger-word chips, tracked so they can be removed on deselect. */
  insertedWords?: string[];
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

/** A <segment:...> auto-refinement region parsed from the positive prompt. */
export interface DetailSegment {
  /** Detection target: free text (CLIPSeg) or "yolo-<model filename>[-<match index>]". */
  target: string;
  /** Refinement prompt for the detected region (may be empty). */
  prompt: string;
  /** Denoise strength for the re-sample, (0, 1]. */
  creativity: number;
  /** Detection threshold, (0, 1). */
  threshold: number;
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

/** An extra, user-named prompt box appended below the main positive/negative box. */
export interface ExtraPromptBox {
  /** uuid — Svelte {#each} key and textarea height storageKey. */
  id: string;
  /** User-editable display name (may be empty). */
  name: string;
  /** Raw prompt text; sanitized and concatenated only at send time. */
  content: string;
}

/**
 * Generation modes the app supports. Exported so every consumer (gallery
 * filename parsing, progress tracking, the save-to-gallery wrappers) shares one
 * definition — hand-copied unions silently drift when a mode is added.
 */
export type GenerationMode = "txt2img" | "img2img" | "inpainting" | "image_edit" | "video";

/**
 * MiniMax H3 workflow variants. `fl2va` drives the first/last-frame graph (which
 * doubles as text-to-video when neither frame is set); `ref2va` drives the
 * reference-image graph. Each variant needs its own diffusion model file.
 */
export type VideoVariant = "fl2va" | "ref2va";

/**
 * Aspect ratio selections offered in the UI. `auto` is a UI-only value: the
 * store resolves it to the uploaded frame's own `W:H` before the params leave
 * the frontend, so the backend only ever sees a numeric ratio.
 */
export type VideoAspectRatio = "auto" | "16:9" | "9:16" | "1:1" | "4:3" | "3:4";

export interface GenerationParams {
  mode: GenerationMode;
  positive_prompt: string;
  negative_prompt: string;
  positive_segments: PromptSegment[];
  negative_segments: PromptSegment[];
  detail_segments: DetailSegment[];
  positive_regions?: PositiveRegion[];
  checkpoint: string;
  vae: string | null;
  loras: LoraPayloadEntry[];
  sampler_name: string;
  scheduler: string;
  steps: number;
  cfg: number;
  /** Decimal string ("-1" = random) — 63-bit seeds exceed JS's safe-integer range. */
  seed: string;
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
  /** Post-model-upscale downscale ratio (<= 1.0) applied when the target-scale cap is
   *  enabled, so the diffusion refine pass never runs at more than the requested scale. */
  upscale_model_downscale_ratio: number;
  upscale_denoise: number;
  upscale_steps: number;
  upscale_tile_size: number;
  upscale_tiling: boolean;
  upscale_fast_refine?: boolean;
  upscale_soft_guidance: boolean;
  upscale_soft_guidance_multiplier: number;
  /** Quality-prompt override for the upscale pass (family-specific). Null falls back to base conditioning. */
  upscale_positive_prompt: string | null;
  /** Negative quality-prompt override for the upscale pass. Null falls back to base conditioning. */
  upscale_negative_prompt: string | null;
  /** Also save the base image before the upscale chain runs. */
  save_pre_upscale_image?: boolean;
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
  /** Folder the active model physically lives in when it contradicts its detected
   *  kind ("checkpoints" / "diffusion_models"); null when correctly placed. The
   *  backend resolves it to an absolute path for the Mooshie path loader nodes. */
  model_source_category: string | null;
  /** Auto face-refinement pass after the main sample. */
  facefix_enabled: boolean;
  /** YOLO/segmentation detector model filename, or null for the default. */
  facefix_detector: string | null;
  facefix_denoise: number;
  facefix_steps: number;
  facefix_guide_size: number;
  /** Cap on faces refined per image; 0 means no limit. */
  facefix_max_faces: number;
  /** Reuse the positive prompt for each detected face instead of a generic prompt. */
  facefix_auto_prompt: boolean;
  controlnet: ControlNetPayload | null;
  model_architecture: string;
  is_sdxl_like?: boolean;
  is_vpred_model?: boolean;
  output_bit_depth: string;
  /** Storage format for this generation: "png" (default), "jxl", or "webp". */
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
  style_transfer_blocks?: string;
  /** Anima TeaCache: reuses the previous step's DiT output when little changed. */
  anima_teacache_enabled?: boolean;
  /** Image Edit mode reference images (ComfyUI input filenames); slot 0 primary. */
  edit_reference_images?: string[];
  // --- Video generation (MiniMax H3) ---
  video_variant?: VideoVariant;
  /** 1-15 s; the backend snaps this to the nearest 17n+5 frame count at 24 fps. */
  video_duration_seconds?: number;
  /** Pixel budget; the backend derives width/height from this plus the aspect ratio. */
  video_megapixels?: number;
  /** A preset (`16:9`) or a literal `W:H` resolved from an uploaded frame. */
  video_aspect_ratio?: string;
  /** fl2va first-frame image (ComfyUI input filename). */
  video_first_frame?: string | null;
  /** fl2va last-frame image (ComfyUI input filename). */
  video_last_frame?: string | null;
  /** ref2va reference images (ComfyUI input filenames), at most 9. */
  video_ref_images?: string[];
  video_rife_enabled?: boolean;
  video_rife_multiplier?: number;
  video_rife_scale_factor?: number;
  video_rife_fast_mode?: boolean;
  video_rife_ensemble?: boolean;
  /** MiniMax-H3 Turbo LoRA (distilled few-step sampling). */
  video_turbo_enabled?: boolean;
  /** Sampling steps while Turbo is on; the backend clamps to 4..8. */
  video_turbo_steps?: number;
  /** Turbo adapter filename inside `models/loras/`. */
  video_turbo_lora?: string | null;
  /** MiniMax-H3 TeaCache: reuses the previous step's model output when little changed. */
  video_teacache_enabled?: boolean;
  video_diffusion_model?: string | null;
  video_clip_model?: string | null;
  video_vae_model?: string | null;
  video_audio_vae_model?: string | null;
  /**
   * Compiled H3 Director `timeline_data` JSON, or null to build the plain
   * native H3 graph. Produced by `compileTimeline()` in `timelineProvider.ts`.
   */
  video_timeline_data?: string | null;
  /** Director `use_custom_motion` widget: the timeline has motion clips. */
  video_timeline_custom_motion?: boolean;
  /** Director `use_custom_audio` widget: the timeline has audio cues. */
  video_timeline_custom_audio?: boolean;
}

export interface OutputImage {
  filename: string;
  subfolder: string;
  type: string;
  prompt_id: string;
  generation_mode?: GenerationMode;
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
  /** Playback length in seconds. Present only for `.mp4` gallery entries. */
  duration_seconds?: number;
  /** Frame rate for video entries; the player falls back to 24 when absent. */
  fps?: number;
  /** Total wall-clock generation time in ms for this image (top-left badge). */
  generationTimeMs?: number;
  metadata?: Record<string, string> | null;
}

export interface GalleryImageEntry {
  filename: string;
  size_bytes: number;
  modified_ms: number;
  /** Playback length in seconds. Present only for `.mp4` entries. */
  duration_seconds?: number;
  /** Frame rate for video entries; the player falls back to 24 when absent. */
  fps?: number;
}

/** How the A/B comparison viewer blends the two images. */
export type CompareMode = "slider" | "fade" | "difference" | "side_by_side";

/** Divider axis for slider mode (also the split axis for side-by-side). */
export type CompareOrientation = "horizontal" | "vertical";

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
  prompt_assistant_model_id: string | null;
  prompt_assistant_idle_timeout_secs: number;
  prompt_assistant_setup_done: boolean;
  civitai_api_key: string | null;
  /** Present in browser mode for non-admin users when a server-side key is configured. */
  civitai_api_key_configured?: boolean;
  /** When set, in-app error reports POST here (Sub-project B proxy) instead of opening a prefilled GitHub issue. */
  report_endpoint?: string | null;
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
  /** Prompt assistant: use an external OpenAI-compatible endpoint instead of the local llama-server. */
  llm_external_enabled: boolean;
  /** External LLM provider id: anthropic | openai | xai | openrouter | custom. */
  llm_provider: string;
  /** External LLM API root, e.g. http://localhost:1234/v1 or https://api.openai.com/v1. */
  llm_external_base_url: string;
  /** External LLM API key (Bearer token; empty for keyless local servers). */
  llm_external_api_key: string;
  /** Blanked for non-admin browser clients; true when a key is stored server-side. */
  llm_external_api_key_configured?: boolean;
  /** External LLM model name (e.g. gpt-4o-mini). */
  llm_external_model: string;
  /** Disable the 7-day gallery image auto-expiry entirely (default: false). */
  gallery_never_expire: boolean;
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

export interface LlmGpu {
  name: string;
  vram_mb: number;
  vendor: string;
}

export interface LlmHardware {
  gpus: LlmGpu[];
  total_vram_mb: number;
  system_ram_mb: number;
  recommended_model_id: string;
}

export interface LlmVariant {
  format: "gguf";
  quant: string | null;
  size_mb: number;
  vram_mb: number;
  repo: string;
  file: string;
}

export interface LlmCatalogEntry {
  id: string;
  name: string;
  purpose: "tag_upsampler" | "natural_language";
  families: string[];
  variants: LlmVariant[];
  pros: string;
  cons: string;
  best_for: string;
}

export interface LlmStatus {
  installed_models: string[];
  active_model: string | null;
  server_running: boolean;
  external_enabled: boolean;
}

export interface PromptAssistantOpts {
  length?: "short" | "medium" | "detailed";
  include_artists?: boolean;
}

/** Ids of the external LLM providers the backend registry knows about. */
export type LlmProviderId =
  | "anthropic"
  | "openai"
  | "xai"
  | "openrouter"
  | "custom";

/**
 * Provider settings as the backend reports them. The API key is deliberately
 * absent: it is stored in Rust config and never crosses into the frontend.
 */
export interface LlmProviderState {
  provider: LlmProviderId | string;
  base_url: string;
  model: string;
  api_key_configured: boolean;
  /** The provider has a sign-in flow this build implements. */
  oauth: boolean;
  /** The external provider, not the bundled local model, is what runs. */
  enabled: boolean;
}
