use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct PromptResponse {
    pub prompt_id: String,
    pub number: Option<i64>,
    pub node_errors: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemStats {
    pub system: SystemInfo,
    pub devices: Vec<DeviceInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub ram_total: u64,
    pub ram_free: u64,
    pub comfyui_version: Option<String>,
    pub python_version: Option<String>,
    pub pytorch_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub r#type: String,
    pub vram_total: u64,
    pub vram_free: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueueInfo {
    pub queue_running: Vec<serde_json::Value>,
    pub queue_pending: Vec<serde_json::Value>,
    /// Ordered queue positions from the internal fair-queue tracker.
    /// Empty when not populated (e.g. raw ComfyUI response).
    #[serde(default)]
    pub queue_positions: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub name: String,
    pub subfolder: String,
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SamplerInfo {
    pub samplers: Vec<String>,
    pub schedulers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraParam {
    pub name: String,
    pub strength_model: f64,
    pub strength_clip: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// A `<segment:...>` auto-refinement region parsed from the positive prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailSegment {
    /// Detection target: free text (CLIPSeg) or "yolo-<model filename>[-<match index>]".
    pub target: String,
    /// Refinement prompt for the detected region (may be empty).
    pub prompt: String,
    /// Denoise strength for the re-sample, (0, 1].
    pub creativity: f64,
    /// Detection threshold, (0, 1).
    pub threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositiveRegion {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default = "default_region_strength")]
    pub strength: f64,
}

/// Seeds are 63-bit and exceed JavaScript's 2^53 safe-integer range, so they
/// must cross the IPC/JSON boundary as strings. Serializes an i64 as a decimal
/// string; accepts a string or a bare number (old persisted settings/clients).
pub mod seed_string {
    use serde::{de, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &i64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        struct SeedVisitor;
        impl<'de> de::Visitor<'de> for SeedVisitor {
            type Value = i64;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a seed as an integer or string")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<i64, E> {
                Ok(v)
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<i64, E> {
                Ok(v as i64)
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<i64, E> {
                Ok(v as i64)
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<i64, E> {
                v.trim().parse::<i64>().map_err(de::Error::custom)
            }
        }
        d.deserialize_any(SeedVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationParams {
    pub mode: String,
    pub positive_prompt: String,
    pub negative_prompt: String,
    #[serde(default)]
    pub positive_segments: Vec<PromptSegment>,
    #[serde(default)]
    pub negative_segments: Vec<PromptSegment>,
    #[serde(default)]
    pub detail_segments: Vec<DetailSegment>,
    #[serde(default)]
    pub positive_regions: Vec<PositiveRegion>,
    pub checkpoint: String,
    pub vae: Option<String>,
    pub loras: Vec<LoraParam>,
    pub sampler_name: String,
    pub scheduler: String,
    pub steps: u32,
    pub cfg: f64,
    #[serde(with = "seed_string")]
    pub seed: i64,
    pub width: u32,
    pub height: u32,
    pub batch_size: u32,
    pub denoise: f64,
    #[serde(default)]
    pub differential_diffusion: bool,
    pub input_image: Option<String>,
    pub mask_image: Option<String>,
    pub grow_mask_by: Option<u32>,
    pub upscale_enabled: bool,
    pub upscale_method: String,
    pub upscale_model: Option<String>,
    pub upscale_scale: f64,
    /// Post-model-upscale downscale ratio (<= 1.0) applied when the frontend's
    /// target-scale cap is enabled, so the refine pass never runs above the
    /// requested scale even though the upscale model's native factor is higher.
    #[serde(default = "default_upscale_model_downscale_ratio")]
    pub upscale_model_downscale_ratio: f64,
    pub upscale_denoise: f64,
    pub upscale_steps: u32,
    pub upscale_tile_size: u32,
    pub upscale_tiling: bool,
    /// Skip MultiDiffusion and tiled VAE during refine (faster; may OOM on Anima).
    #[serde(default)]
    pub upscale_fast_refine: bool,
    /// "Refine" mode — when true with mode="img2img", skip the main img2img
    /// KSampler/VAE round-trip and feed the loaded input image directly into
    /// the upscale chain. Mirrors SwarmUI's "Refine Image" button: a single
    /// low-denoise second pass at higher resolution.
    #[serde(default)]
    pub refine_only: bool,
    /// Also save the base image as it was before the upscale chain ran.
    #[serde(default)]
    pub save_pre_upscale_image: bool,
    /// Enable Soft Guidance (CFG rescaling) for upscale pass to prevent hallucination
    #[serde(default)]
    pub upscale_soft_guidance: bool,
    /// Soft Guidance multiplier (0.0=off, 0.4=recommended for upscale, 0.7=general)
    #[serde(default = "default_soft_guidance_multiplier")]
    pub upscale_soft_guidance_multiplier: f64,
    /// Quality-only positive prompt for tiled upscale KSampler (reduces tile artifacts)
    #[serde(default)]
    pub upscale_positive_prompt: Option<String>,
    /// Quality-only negative prompt for tiled upscale KSampler (reduces tile artifacts)
    #[serde(default)]
    pub upscale_negative_prompt: Option<String>,
    /// When true, use separate UNETLoader + CLIPLoader + VAELoader instead of CheckpointLoaderSimple
    #[serde(default)]
    pub use_split_model: bool,
    /// Diffusion model filename (in models/diffusion_models/)
    #[serde(default)]
    pub diffusion_model: Option<String>,
    /// CLIP/text encoder filename (in models/text_encoders/)
    #[serde(default)]
    pub clip_model: Option<String>,
    /// CLIP model type for CLIPLoader (e.g. "wan", "sd3", etc.)
    #[serde(default)]
    pub clip_type: Option<String>,
    /// Folder the active model physically lives in, set only when it disagrees
    /// with what the file actually is (a diffusion model in `checkpoints/`, or a
    /// full checkpoint in `diffusion_models/`). `None` for correctly-placed files.
    ///
    /// ComfyUI's stock loaders validate the filename against their own folder's
    /// listing, so a misplaced file cannot be named to them — it is loaded by
    /// absolute path instead (see `resolved_model_path`).
    #[serde(default)]
    pub model_source_category: Option<String>,
    /// Absolute path to the active model, resolved backend-side when
    /// `model_source_category` is set. Never sent by the frontend.
    #[serde(default)]
    pub resolved_model_path: Option<String>,
    // --- Video generation (MiniMax H3) ---
    /// "fl2va" (first/last frame -> video+audio) or "ref2va" (reference images).
    /// Empty string when the client predates video support.
    #[serde(default)]
    pub video_variant: String,
    #[serde(default = "default_video_duration_seconds")]
    pub video_duration_seconds: f64,
    #[serde(default = "default_video_megapixels")]
    pub video_megapixels: f64,
    #[serde(default = "default_video_aspect_ratio")]
    pub video_aspect_ratio: String,
    /// First-frame image for fl2va, same encoding as `input_image`.
    #[serde(default)]
    pub video_first_frame: Option<String>,
    #[serde(default)]
    pub video_last_frame: Option<String>,
    /// Up to 9 reference images for ref2va.
    #[serde(default)]
    pub video_ref_images: Vec<String>,
    #[serde(default)]
    pub video_rife_enabled: bool,
    #[serde(default = "default_rife_multiplier")]
    pub video_rife_multiplier: u32,
    #[serde(default = "default_rife_scale_factor")]
    pub video_rife_scale_factor: f64,
    #[serde(default = "default_rife_fast_mode")]
    pub video_rife_fast_mode: bool,
    #[serde(default = "default_rife_ensemble")]
    pub video_rife_ensemble: bool,
    /// MiniMax-H3 Turbo LoRA — distilled few-step sampling (4-8 steps instead
    /// of 20). Requires the `ComfyUI-MiniMax-H3-Turbo` custom node pack.
    #[serde(default)]
    pub video_turbo_enabled: bool,
    /// Sampling steps used when the Turbo LoRA is on (clamped to 4..=8 by the
    /// video validation arm). Ignored when `video_turbo_enabled` is false.
    #[serde(default = "default_video_turbo_steps")]
    pub video_turbo_steps: u32,
    /// Turbo LoRA filename inside `models/loras/`. Defaults to the recommended
    /// checkpoint when the client omits it.
    #[serde(default)]
    pub video_turbo_lora: Option<String>,
    /// TeaCache — reuses the previous step's model output while the
    /// accumulated input delta stays under threshold, skipping the real
    /// forward pass. Requires the `ComfyUI-MiniMaxH3-TeaCache` custom node
    /// pack.
    #[serde(default)]
    pub video_teacache_enabled: bool,
    #[serde(default)]
    pub video_diffusion_model: Option<String>,
    #[serde(default)]
    pub video_clip_model: Option<String>,
    #[serde(default)]
    pub video_vae_model: Option<String>,
    #[serde(default)]
    pub video_audio_vae_model: Option<String>,
    /// Compiled H3 Director `timeline_data` JSON. `None` (or an empty string)
    /// selects the plain native H3 graph; anything else routes the video build
    /// through `MooshieH3Director`. Kept as an opaque string because the node
    /// is the authority on the schema - Rust only forwards it.
    #[serde(default)]
    pub video_timeline_data: Option<String>,
    /// Drives the Director's `use_custom_motion` widget: true when the timeline
    /// has at least one motion clip. A widget rather than a `timeline_data` key,
    /// so it travels separately.
    #[serde(default)]
    pub video_timeline_custom_motion: bool,
    /// Drives the Director's `use_custom_audio` widget. Also decides whether the
    /// Director's `combined_audio` output replaces H3's jointly-generated
    /// stereo track — with no audio segments that output is digital silence.
    #[serde(default)]
    pub video_timeline_custom_audio: bool,
    /// Optional ControlNet parameters
    #[serde(default)]
    pub controlnet: Option<ControlNetParam>,
    /// Detected model architecture from the frontend (e.g. "sd3", "sdxl", "sd15", "illustrious", "unknown")
    #[serde(default)]
    pub model_architecture: String,
    /// True when the resolved family belongs to the SDXL-like bucket.
    #[serde(default)]
    pub is_sdxl_like: bool,
    /// True when metadata or filename indicates a v-pred SDXL variant.
    #[serde(default)]
    pub is_vpred_model: bool,
    /// Enable Smart Guidance (positive-biased) — patches model for all generation passes
    #[serde(default)]
    pub smart_guidance: bool,
    /// FluxGuidance value for Flux Dev / Flux 2 Klein family. Default 3.5.
    #[serde(default = "default_flux_guidance")]
    pub flux_guidance: f32,
    /// Face fix (FaceDetailer) — detect faces with YOLOv8 and re-denoise them
    #[serde(default)]
    pub facefix_enabled: bool,
    #[serde(default)]
    pub facefix_detector: Option<String>,
    #[serde(default = "default_facefix_denoise")]
    pub facefix_denoise: f64,
    #[serde(default = "default_facefix_steps")]
    pub facefix_steps: u32,
    #[serde(default = "default_facefix_guide_size")]
    pub facefix_guide_size: u32,
    #[serde(default = "default_facefix_max_faces")]
    pub facefix_max_faces: u32,
    /// When set, condition the face detailer on a face-only subset of the prompt
    /// (auto-extracted) instead of the full prompt, so scene/pose/background tags
    /// don't bleed into the re-denoised face region.
    #[serde(default)]
    pub facefix_auto_prompt: bool,
    /// Output image bit depth — "8bit" (default) or "16bit"
    #[serde(default = "default_output_bit_depth")]
    pub output_bit_depth: String,
    /// Storage format the Rust bridge will produce for this generation.
    /// "png" (default, backward compatible), "jxl" or "webp" (raw pixels out of
    /// ComfyUI, encoded to JPEG XL / lossless WebP in the Tauri backend).
    #[serde(default = "default_output_format")]
    pub output_format: String,
    /// Anima Untwisting RoPE training-free style transfer (txt2img only in v1).
    #[serde(default)]
    pub style_transfer_enabled: bool,
    /// ComfyUI input filename for the style reference image.
    #[serde(default)]
    pub style_reference_image: Option<String>,
    /// UntwistingRoPE low_scale_end — primary style strength knob (default 1.5).
    #[serde(default = "default_style_transfer_low_scale_end")]
    pub style_transfer_low_scale_end: f64,
    /// UntwistingRoPE high_scale_start — structure match (default 1.0).
    #[serde(default = "default_style_transfer_high_scale_start")]
    pub style_transfer_high_scale_start: f64,
    #[serde(default = "default_style_transfer_beta")]
    pub style_transfer_beta: f64,
    #[serde(default = "default_style_transfer_adain_strength")]
    pub style_transfer_adain_strength: f64,
    /// RF inversion mode: linear | rf_gamma | rf_gamma_rk2 | fireflow
    #[serde(default = "default_style_transfer_rf_mode")]
    pub style_transfer_rf_mode: String,
    #[serde(default = "default_style_transfer_gamma")]
    pub style_transfer_gamma: f64,
    #[serde(default = "default_style_transfer_gamma_curve")]
    pub style_transfer_gamma_curve: f64,
    #[serde(default = "default_style_transfer_norm_strength")]
    pub style_transfer_norm_strength: f64,
    #[serde(default = "default_style_transfer_pmi_alpha")]
    pub style_transfer_pmi_alpha: f64,
    /// Target megapixels for ImageScaleToTotalPixelsX (default 1.05).
    #[serde(default = "default_style_transfer_megapixels")]
    pub style_transfer_megapixels: f64,
    /// Blocks range for UntwistingRoPE node (e.g. "0-999").
    #[serde(default = "default_style_transfer_blocks")]
    pub style_transfer_blocks: String,
    /// Anima TeaCache — reuses the previous step's DiT output while the
    /// accumulated input delta stays under threshold, skipping the forward
    /// pass. MooshieUI-authored node, always deployed (no lazy install).
    #[serde(default)]
    pub anima_teacache_enabled: bool,
    /// Reference images for Image Edit mode (ComfyUI input filenames). Slot 0 is
    /// the primary edit source; slots 1-2 are Qwen Image Edit Plus extras.
    #[serde(default)]
    pub edit_reference_images: Vec<String>,
}

fn default_output_bit_depth() -> String {
    "8bit".to_string()
}

fn default_output_format() -> String {
    "png".to_string()
}

fn default_style_transfer_low_scale_end() -> f64 {
    1.5
}

fn default_style_transfer_high_scale_start() -> f64 {
    1.0
}

fn default_style_transfer_beta() -> f64 {
    50.0
}

fn default_style_transfer_adain_strength() -> f64 {
    0.5
}

fn default_style_transfer_rf_mode() -> String {
    "rf_gamma_rk2".to_string()
}

fn default_style_transfer_gamma() -> f64 {
    0.5
}

fn default_style_transfer_gamma_curve() -> f64 {
    2.0
}

fn default_style_transfer_norm_strength() -> f64 {
    1.0
}

fn default_style_transfer_pmi_alpha() -> f64 {
    0.5
}

fn default_style_transfer_megapixels() -> f64 {
    1.05
}

fn default_style_transfer_blocks() -> String {
    "0-999".to_string()
}

fn default_region_strength() -> f64 {
    1.0
}

fn default_video_duration_seconds() -> f64 {
    5.0
}

fn default_video_megapixels() -> f64 {
    0.4
}

fn default_video_aspect_ratio() -> String {
    "16:9".to_string()
}

fn default_video_turbo_steps() -> u32 {
    6
}

fn default_rife_multiplier() -> u32 {
    2
}

fn default_rife_scale_factor() -> f64 {
    1.0
}

fn default_rife_fast_mode() -> bool {
    true
}

fn default_rife_ensemble() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlNetParam {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub preset: Option<String>,
    pub controlnet_model: Option<String>,
    pub image: Option<String>,
    pub preprocessor: Option<String>,
    #[serde(default = "default_strength")]
    pub strength: f64,
    #[serde(default)]
    pub start_percent: f64,
    #[serde(default = "default_end_percent")]
    pub end_percent: f64,
}

fn default_strength() -> f64 {
    1.0
}

fn default_end_percent() -> f64 {
    1.0
}

fn default_facefix_denoise() -> f64 {
    0.4
}

fn default_flux_guidance() -> f32 {
    3.5
}

fn default_soft_guidance_multiplier() -> f64 {
    0.4
}

fn default_upscale_model_downscale_ratio() -> f64 {
    1.0
}

fn default_facefix_steps() -> u32 {
    20
}

fn default_facefix_guide_size() -> u32 {
    512
}

fn default_facefix_max_faces() -> u32 {
    8
}
