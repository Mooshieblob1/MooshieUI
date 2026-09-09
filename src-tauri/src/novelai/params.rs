//! Frontend-facing NovelAI parameters.
//!
//! These arrive nested under `GenerationParams::novelai` so the NovelAI-only
//! surface never pollutes the ComfyUI parameter set. Everything here is plain
//! data with no `tauri` dependency, so it compiles in both the desktop and
//! server builds.

use serde::{Deserialize, Serialize};

/// A character position, normalised to the 0..1 coordinate space the API
/// expects. Since V5 the UI places characters freely on the canvas, so any
/// value in range is valid; the 5x5 grid helper below survives because the
/// grid's cell centres live in the same space, which is what keeps positions
/// saved by the old grid picker loading unchanged.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct NovelAiCoord {
    pub x: f64,
    pub y: f64,
}

impl Default for NovelAiCoord {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5 }
    }
}

impl NovelAiCoord {
    /// Convert a zero-based 5x5 grid cell into NovelAI's normalised centre.
    ///
    /// NovelAI's grid uses cell centres, so column 0 maps to 0.1 and column 4
    /// to 0.9. Out-of-range cells clamp rather than error: a malformed client
    /// should place the character in the middle, not fail a paid generation.
    pub fn from_grid(col: i32, row: i32) -> Self {
        let clamp = |v: i32| v.clamp(0, 4) as f64;
        Self {
            x: (clamp(col) * 2.0 + 1.0) / 10.0,
            y: (clamp(row) * 2.0 + 1.0) / 10.0,
        }
    }
}

/// One character prompt, matching NovelAI's per-character caption slots.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NovelAiCharacter {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub negative_prompt: String,
    /// Canvas position. Only sent when `use_coords` is on for the generation.
    #[serde(default)]
    pub center: NovelAiCoord,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// A vibe-transfer reference.
///
/// `encoding` is a cached `.naiv4vibe` payload; when present no encode is
/// charged. `image` is the raw base64 PNG used the first time a vibe is
/// encoded, which costs 2 Anlas on V4 and later.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NovelAiVibe {
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default = "default_vibe_strength")]
    pub strength: f64,
    #[serde(default = "default_information_extracted")]
    pub information_extracted: f64,
    /// Model the `encoding` was minted for.
    ///
    /// A token is only good for the model and the extraction level it was
    /// made with, so these two travel with it and a change to either forces
    /// a fresh encode. The client persists all three and sends them back,
    /// which is what keeps a restart from paying for the same vibe twice.
    #[serde(default)]
    pub encoded_model: Option<String>,
    #[serde(default)]
    pub encoded_information_extracted: Option<f64>,
}

/// A Precise Reference (`director_reference_*`) entry. V4.5 only.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NovelAiDirectorReference {
    /// Base64 PNG at whatever size the user picked. `reference_canvas`
    /// letterboxes it onto one of NovelAI's three accepted canvases while the
    /// payload is built; the output canvas is unaffected.
    #[serde(default)]
    pub image: String,
    /// What to take from the reference, e.g. "character" or "character&style".
    #[serde(default = "default_reference_description")]
    pub description: String,
    #[serde(default = "default_reference_strength")]
    pub strength: f64,
    /// How closely the result tracks the reference. Reaches NovelAI inverted,
    /// as `director_reference_secondary_strength_values`, which is the form
    /// their own client sends.
    #[serde(default = "default_reference_fidelity")]
    pub fidelity: f64,
}

/// The NovelAI-mode face detailer block.
///
/// Detection is always local YOLO inside ComfyUI; `detailer_engine` picks who
/// repaints the crop. `novelai` sends each face back to NovelAI as img2img,
/// which keeps the style of the base render; `local` hands off to the existing
/// ComfyUI face-fix chain, which needs [`NovelAiParams::local_checkpoint`].
///
/// This block deliberately does not reuse the top-level `facefix_*` fields.
/// Those belong to the local-mode FaceFix panel, which is hidden in NovelAI
/// mode, so their persisted values would arm a pass the user cannot see.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovelAiFaceDetail {
    #[serde(default)]
    pub enabled: bool,
    /// `novelai` or `local`.
    #[serde(default = "default_detailer_engine")]
    pub detailer_engine: String,
    /// YOLO weight in `models/ultralytics`.
    #[serde(default = "default_face_detector")]
    pub detector_model: String,
    /// Minimum detection confidence.
    #[serde(default = "default_face_threshold")]
    pub threshold: f64,
    /// Crop side as a multiple of the bounding box's long side.
    #[serde(default = "default_face_padding")]
    pub padding: f64,
    /// 0 means every detection.
    #[serde(default = "default_face_max_faces")]
    pub max_faces: u32,
    /// Long side the crop is scaled to before repainting.
    #[serde(default = "default_face_guide_size")]
    pub guide_size: u32,
    /// img2img strength for the crop pass.
    #[serde(default = "default_face_strength")]
    pub strength: f64,
    /// Steps for the crop pass. Clamped to 28 on the NovelAI engine so a free
    /// pass does not silently become a paid one.
    #[serde(default = "default_face_steps")]
    pub steps: u32,
    /// Composite feather in pixels. The effective value is at least a sixth of
    /// the crop's short side.
    #[serde(default = "default_face_feather")]
    pub feather: u32,
    /// `auto`, `generic` or `custom`.
    #[serde(default = "default_face_prompt_mode")]
    pub prompt_mode: String,
    /// Used verbatim when `prompt_mode` is `custom`.
    #[serde(default)]
    pub custom_prompt: String,
    /// `fit_free` downscales crops to stay inside the Opus free window;
    /// `allow_paid` sends them at native size.
    #[serde(default = "default_anlas_policy")]
    pub anlas_policy: String,
    /// General-tag confidence floor for the tagger run on the crop. Higher
    /// than the interrogator default on purpose: a wrong tag read off a
    /// malformed face gets painted in confidently.
    #[serde(default = "default_tagger_threshold")]
    pub tagger_threshold: f64,
}

impl Default for NovelAiFaceDetail {
    fn default() -> Self {
        Self {
            enabled: false,
            detailer_engine: default_detailer_engine(),
            detector_model: default_face_detector(),
            threshold: default_face_threshold(),
            padding: default_face_padding(),
            max_faces: default_face_max_faces(),
            guide_size: default_face_guide_size(),
            strength: default_face_strength(),
            steps: default_face_steps(),
            feather: default_face_feather(),
            prompt_mode: default_face_prompt_mode(),
            custom_prompt: String::new(),
            anlas_policy: default_anlas_policy(),
            tagger_threshold: default_tagger_threshold(),
        }
    }
}

impl NovelAiFaceDetail {
    /// The face is repainted by NovelAI rather than a local checkpoint.
    pub fn uses_novelai_engine(&self) -> bool {
        !self.detailer_engine.eq_ignore_ascii_case("local")
    }

    /// The pass is on and hands off to the local ComfyUI chain.
    pub fn uses_local_engine(&self) -> bool {
        self.enabled && !self.uses_novelai_engine()
    }

    /// Crops are shrunk to stay inside the Opus free window.
    pub fn fits_free_window(&self) -> bool {
        !self.anlas_policy.eq_ignore_ascii_case("allow_paid")
    }
}

/// The NovelAI-only half of a generation request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NovelAiParams {
    /// NovelAI model id, e.g. `nai-diffusion-4-5-full`.
    pub model: String,
    /// `generate`, `img2img` or `infill`. Defaults to `generate`.
    #[serde(default = "default_action")]
    pub action: String,
    /// NovelAI sampler name, e.g. `k_euler_ancestral`.
    ///
    /// Held here rather than reusing `GenerationParams::sampler_name` because
    /// that field names a ComfyUI sampler and is still what the free local
    /// post-process pass samples with. NovelAI's names would be rejected there.
    #[serde(default = "default_sampler")]
    pub sampler: String,
    #[serde(default = "default_noise_schedule")]
    pub noise_schedule: String,
    #[serde(default)]
    pub cfg_rescale: f64,
    #[serde(default = "default_uncond_scale")]
    pub uncond_scale: f64,
    #[serde(default)]
    pub dynamic_thresholding: bool,
    /// "Variety+" — suppresses CFG above a sigma threshold.
    #[serde(default)]
    pub variety_plus: bool,
    /// "Transparent BG" — ask V5 for a real alpha channel.
    ///
    /// NovelAI has no request field for this. The feature is a prompt tag their
    /// own UI inserts for you, so the toggle is honoured by appending the tag
    /// while the request body is built. See `payload::with_transparency`.
    #[serde(default)]
    pub transparent_background: bool,
    #[serde(default = "default_true")]
    pub quality_toggle: bool,
    /// NovelAI's built-in undesired-content preset index.
    #[serde(default)]
    pub uc_preset: u8,
    #[serde(default)]
    pub legacy_uc: bool,
    #[serde(default)]
    pub characters: Vec<NovelAiCharacter>,
    /// When false, NovelAI infers placement and character centres are omitted.
    #[serde(default)]
    pub use_coords: bool,
    /// img2img denoise. NovelAI's `strength` is the inverse of ComfyUI's
    /// denoise convention only in naming; the value is passed through as-is.
    #[serde(default = "default_img2img_strength")]
    pub strength: f64,
    #[serde(default)]
    pub noise: f64,
    /// Infill: keep the untouched region pixel-identical to the input.
    #[serde(default = "default_true")]
    pub add_original_image: bool,
    #[serde(default)]
    pub vibes: Vec<NovelAiVibe>,
    /// Scale the vibe strengths down so they sum to 1. NovelAI's own
    /// client offers this as a checkbox next to the strength sliders.
    #[serde(default)]
    pub normalize_reference_strength: bool,
    #[serde(default)]
    pub director_references: Vec<NovelAiDirectorReference>,
    /// Run the local ComfyUI upscale/facefix chain on the returned image.
    ///
    /// This costs no Anlas: NovelAI has already been paid for the base image
    /// and the second pass runs entirely on the user's own GPU.
    #[serde(default)]
    pub local_post_process: bool,
    /// Local checkpoint the post-process pass samples with. Required for the
    /// pass to run at all: `checkpoint` names a NovelAI model in this mode and
    /// ComfyUI cannot load it.
    #[serde(default)]
    pub local_checkpoint: Option<String>,
    /// Architecture of `local_checkpoint`, in the same vocabulary as
    /// `GenerationParams::model_architecture` (e.g. "anima", "illustrious").
    /// Drives the v-pred / cascade / rectified-flow injections.
    #[serde(default)]
    pub local_architecture: Option<String>,
    /// `local_checkpoint` is a v-prediction SDXL variant.
    #[serde(default)]
    pub local_is_vpred: bool,
    /// Folder `local_checkpoint` physically lives in, "checkpoints" or
    /// "diffusion_models". Compared against the loader mode below to decide
    /// whether the file has to be opened by absolute path.
    #[serde(default)]
    pub local_model_category: Option<String>,
    /// Load `local_checkpoint` with UNETLoader + CLIPLoader + VAELoader
    /// instead of CheckpointLoaderSimple. Split-file models (Anima, Flux,
    /// Chroma, ...) carry no text encoder or VAE of their own.
    #[serde(default)]
    pub local_use_split_model: bool,
    /// Text encoder for the split load, and the CLIPLoader type it needs.
    #[serde(default)]
    pub local_clip_model: Option<String>,
    #[serde(default)]
    pub local_clip_type: Option<String>,
    /// VAE for the split load.
    #[serde(default)]
    pub local_vae: Option<String>,
    /// Prompt in ComfyUI weight syntax, for the local pass only.
    ///
    /// By the time params reach the backend the top-level `positive_prompt`
    /// has been rewritten into NovelAI's `1.1::tag::` syntax, which
    /// `CLIPTextEncode` would take literally.
    #[serde(default)]
    pub local_positive_prompt: Option<String>,
    /// Negative counterpart of [`Self::local_positive_prompt`].
    #[serde(default)]
    pub local_negative_prompt: Option<String>,
    /// Sampler for the local pass, filled from the picked model's
    /// recommendation.
    ///
    /// Step count and denoise are deliberately absent: those come from the
    /// upscale and face-fix panels, which the user can see and set. Sampler,
    /// schedule and guidance have no such control in NovelAI mode, where the
    /// sampler panel is hidden and the top-level values still describe the
    /// NovelAI request. `None` leaves the top-level value alone.
    #[serde(default)]
    pub local_sampler: Option<String>,
    /// Noise schedule for the local pass. See [`Self::local_sampler`].
    #[serde(default)]
    pub local_scheduler: Option<String>,
    /// Guidance scale for the local pass. See [`Self::local_sampler`].
    #[serde(default)]
    pub local_cfg: Option<f64>,
    /// The NovelAI-mode face detailer panel's settings.
    #[serde(default)]
    pub face_detail: NovelAiFaceDetail,
}

impl NovelAiParams {
    /// Characters the user actually enabled, with blank slots dropped.
    pub fn active_characters(&self) -> Vec<&NovelAiCharacter> {
        self.characters
            .iter()
            .filter(|c| c.enabled && !c.prompt.trim().is_empty())
            .collect()
    }
}

fn default_true() -> bool {
    true
}

fn default_action() -> String {
    "generate".to_string()
}

fn default_sampler() -> String {
    "k_euler_ancestral".to_string()
}

fn default_noise_schedule() -> String {
    "karras".to_string()
}

fn default_uncond_scale() -> f64 {
    1.0
}

fn default_vibe_strength() -> f64 {
    0.6
}

fn default_information_extracted() -> f64 {
    1.0
}

fn default_reference_strength() -> f64 {
    1.0
}

fn default_reference_fidelity() -> f64 {
    1.0
}

fn default_reference_description() -> String {
    "character".to_string()
}

fn default_img2img_strength() -> f64 {
    0.7
}

fn default_detailer_engine() -> String {
    "novelai".to_string()
}

/// Same weight the FaceFix panel recommends, and the same
/// `models/ultralytics` folder.
fn default_face_detector() -> String {
    "Anzhc Face seg 640 v4 y11n.pt".to_string()
}

fn default_face_threshold() -> f64 {
    0.5
}

fn default_face_padding() -> f64 {
    1.5
}

fn default_face_max_faces() -> u32 {
    3
}

/// 1024 keeps a square crop exactly at the free window's one-megapixel edge.
fn default_face_guide_size() -> u32 {
    1024
}

/// Low enough that img2img reshapes the face without inventing a new one.
fn default_face_strength() -> f64 {
    0.35
}

/// The free window's ceiling, so the default never costs Anlas.
fn default_face_steps() -> u32 {
    28
}

fn default_face_feather() -> u32 {
    20
}

fn default_face_prompt_mode() -> String {
    "auto".to_string()
}

fn default_anlas_policy() -> String {
    "fit_free".to_string()
}

fn default_tagger_threshold() -> f64 {
    0.4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_maps_to_cell_centres() {
        assert_eq!(
            NovelAiCoord::from_grid(0, 0),
            NovelAiCoord { x: 0.1, y: 0.1 }
        );
        assert_eq!(
            NovelAiCoord::from_grid(2, 2),
            NovelAiCoord { x: 0.5, y: 0.5 }
        );
        assert_eq!(
            NovelAiCoord::from_grid(4, 4),
            NovelAiCoord { x: 0.9, y: 0.9 }
        );
    }

    #[test]
    fn grid_clamps_instead_of_failing() {
        assert_eq!(
            NovelAiCoord::from_grid(-3, 9),
            NovelAiCoord { x: 0.1, y: 0.9 }
        );
    }

    #[test]
    fn active_characters_drops_disabled_and_blank() {
        let params = NovelAiParams {
            characters: vec![
                NovelAiCharacter {
                    prompt: "1girl".into(),
                    enabled: true,
                    ..Default::default()
                },
                NovelAiCharacter {
                    prompt: "1boy".into(),
                    enabled: false,
                    ..Default::default()
                },
                NovelAiCharacter {
                    prompt: "   ".into(),
                    enabled: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let active = params.active_characters();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].prompt, "1girl");
    }
}
