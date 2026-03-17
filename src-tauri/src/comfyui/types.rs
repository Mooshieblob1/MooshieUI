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
pub struct GenerationParams {
    pub mode: String,
    pub positive_prompt: String,
    pub negative_prompt: String,
    pub checkpoint: String,
    pub vae: Option<String>,
    pub loras: Vec<LoraParam>,
    pub sampler_name: String,
    pub scheduler: String,
    pub steps: u32,
    pub cfg: f64,
    pub seed: i64,
    pub width: u32,
    pub height: u32,
    pub batch_size: u32,
    pub denoise: f64,
    pub input_image: Option<String>,
    pub mask_image: Option<String>,
    pub grow_mask_by: Option<u32>,
    pub upscale_enabled: bool,
    pub upscale_method: String,
    pub upscale_model: Option<String>,
    pub upscale_scale: f64,
    pub upscale_denoise: f64,
    pub upscale_steps: u32,
    pub upscale_tile_size: u32,
    pub upscale_tiling: bool,
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
}
