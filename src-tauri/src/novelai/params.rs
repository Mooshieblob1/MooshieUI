//! Frontend-facing NovelAI parameters.
//!
//! These arrive nested under `GenerationParams::novelai` so the NovelAI-only
//! surface never pollutes the ComfyUI parameter set. Everything here is plain
//! data with no `tauri` dependency, so it compiles in both the desktop and
//! server builds.

use serde::{Deserialize, Serialize};

/// A point on NovelAI's 5x5 character-position grid, already normalised to the
/// 0..1 coordinate space the API expects.
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
    /// Grid centre. Only sent when `use_coords` is on for the generation.
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
}

/// A Precise Reference (`director_reference_*`) entry. V4.5 only.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NovelAiDirectorReference {
    /// Base64 PNG, already normalised client-side to one of NovelAI's accepted
    /// reference aspect ratios. The output canvas is unaffected.
    #[serde(default)]
    pub image: String,
    /// What to take from the reference, e.g. "character" or "character&style".
    #[serde(default = "default_reference_description")]
    pub description: String,
    #[serde(default = "default_information_extracted")]
    pub information_extracted: f64,
    #[serde(default = "default_reference_strength")]
    pub strength: f64,
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

fn default_reference_description() -> String {
    "character".to_string()
}

fn default_img2img_strength() -> f64 {
    0.7
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
