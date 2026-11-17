//! Wan 2.x as an image model: a one-frame text-to-video run.
//!
//! Three checkpoint lines are usable for images, all through core ComfyUI
//! nodes with the umT5-XXL text encoder (CLIP type `wan`), one family each:
//! - `wan`: **Wan 2.1 T2V** (1.3B / 14B) and fine-tunes, Wan 2.1 VAE. Also
//!   whatever tensor-key detection finds (`head.modulation`).
//! - `wan22ti2v`: **Wan 2.2 TI2V 5B**, the 48-channel Wan 2.2 VAE. The empty
//!   latent stays the generic one; ComfyUI resizes an empty latent to the
//!   model's channel count and 16x downscale before sampling.
//! - `wan22a14b`: **Wan 2.2 A14B T2V**, a mixture of two experts. The
//!   high-noise model lays out the image over the first half of the schedule
//!   and the low-noise model finishes it, as in Comfy-Org's Wan 2.2 template.
//!   The frontend pairs the low-noise file into the `low_noise_model` slot.
//!
//! Image-to-video, Fun, VACE, S2V and Animate checkpoints need inputs a text
//! prompt cannot give them, so [`validate`] turns them away with a clear
//! message instead of a shape error from deep inside ComfyUI.
//!
//! CivitAI files tagged "Wan Video ..." stay mapped to Anima on purpose (Anima
//! fine-tunes were uploaded under that base model); no base-model name is
//! listed here, and tensor keys correct a real Wan file either way.

use serde_json::json;

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, LoaderOutput, ModeSpec, ModelFile,
    ModelSlot, PromptStyle, RecommendedModel, Sampling, VaeSpec,
};
use crate::comfyui::types::GenerationParams;
use crate::templates::{
    build_scheduled_conditioning, insert_vae_decode, load_model_nodes, misplaced_model_path,
    split_loader_nodes, txt2img, unet_loader_node, WorkflowResult,
};

const ENCODER: ModelFile = ModelFile {
    filename: "umt5_xxl_fp8_e4m3fn_scaled.safetensors",
    url: "https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/main/split_files/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors",
    category: "text_encoders",
    size: Some("~6.7 GB"),
    sha256: None,
};

const WAN21_VAE: ModelFile = ModelFile {
    filename: "wan_2.1_vae.safetensors",
    url: "https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/main/split_files/vae/wan_2.1_vae.safetensors",
    category: "vae",
    size: Some("~0.25 GB"),
    sha256: None,
};

const WAN22_VAE: ModelFile = ModelFile {
    filename: "wan2.2_vae.safetensors",
    url: "https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/main/split_files/vae/wan2.2_vae.safetensors",
    category: "vae",
    size: Some("~1.4 GB"),
    sha256: None,
};

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "wan",
    label: "Wan",
    families: &[
        // First: tensor-key detection resolves to it.
        FamilySpec {
            family: "wan",
            label: "Wan 2.1",
            detect: Detect {
                all_of: &[&["wan2"], &["wan 2"], &["wan_2."], &["wan-2."]],
                none_of: &[
                    "ti2v", "5b", "high_noise", "highnoise", "high-noise", "low_noise",
                    "lownoise", "low-noise", "_vae",
                ],
                base_models: &[],
            },
            turbo: false,
            sampling: Some(Sampling {
                steps: 30,
                cfg: 6.0,
                sampler: "uni_pc",
                scheduler: "simple",
                width: 1024,
                height: 1024,
            }),
            recommended: &[RecommendedModel {
                label: "Wan 2.1 T2V 1.3B",
                size: "~9.8 GB",
                diffusion_model: ModelFile {
                    filename: "wan2.1_t2v_1.3B_fp16.safetensors",
                    url: "https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/diffusion_models/wan2.1_t2v_1.3B_fp16.safetensors",
                    category: "diffusion_models",
                    size: Some("~2.8 GB"),
                    sha256: None,
                },
                min_compute_capability: None,
            }],
            text_encoder: None,
            vae: None,
        },
        FamilySpec {
            family: "wan22a14b",
            label: "Wan 2.2 A14B",
            detect: Detect {
                all_of: &[
                    &["wan", "high_noise"],
                    &["wan", "highnoise"],
                    &["wan", "high-noise"],
                    &["wan", "low_noise"],
                    &["wan", "lownoise"],
                    &["wan", "low-noise"],
                ],
                none_of: &["_vae"],
                base_models: &[],
            },
            turbo: false,
            sampling: Some(Sampling {
                steps: 20,
                cfg: 3.5,
                sampler: "euler",
                scheduler: "simple",
                width: 1024,
                height: 1024,
            }),
            recommended: &[],
            text_encoder: None,
            vae: None,
        },
        FamilySpec {
            family: "wan22ti2v",
            label: "Wan 2.2 TI2V 5B",
            detect: Detect {
                all_of: &[
                    &["wan", "ti2v"],
                    &["wan2", "5b"],
                    &["wan 2", "5b"],
                    &["wan_2", "5b"],
                ],
                none_of: &["_vae"],
                base_models: &[],
            },
            turbo: false,
            sampling: Some(Sampling {
                steps: 20,
                cfg: 5.0,
                sampler: "uni_pc",
                scheduler: "simple",
                width: 1024,
                height: 1024,
            }),
            recommended: &[RecommendedModel {
                label: "Wan 2.2 TI2V 5B",
                size: "~18.1 GB",
                diffusion_model: ModelFile {
                    filename: "wan2.2_ti2v_5B_fp16.safetensors",
                    url: "https://huggingface.co/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/main/split_files/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors",
                    category: "diffusion_models",
                    size: Some("~10 GB"),
                    sha256: None,
                },
                min_compute_capability: None,
            }],
            text_encoder: None,
            vae: Some(VaeSpec {
                basenames: &["wan2.2_vae.safetensors"],
                markers: &WAN22_VAE_MARKERS,
                strict: true,
                download: Some(WAN22_VAE),
            }),
        },
    ],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("wan2.1"),
    text_encoder: Some(EncoderSpec {
        clip_type: "wan",
        markers: &TEXT_ENCODER_MARKERS,
        strict: true,
        download: Some(ENCODER),
    }),
    vae: Some(VaeSpec {
        // The Qwen Image VAE is a decoder fine-tune of the Wan 2.1 VAE.
        basenames: &["wan_2.1_vae.safetensors", "qwen_image_vae.safetensors"],
        markers: &WAN21_VAE_MARKERS,
        strict: true,
        download: Some(WAN21_VAE),
    }),
    // Picked by name in the frontend (`findWanLowNoisePartner`). Never
    // downloaded on its own: it is a 14 GB file the user chooses.
    model_slots: &[ModelSlot {
        key: LOW_NOISE_MODEL_SLOT,
        label: "Wan 2.2 low-noise model",
        category: "diffusion_models",
        markers: &["t2v_low_noise", "t2v-low-noise", "lownoise"],
        strict: false,
        download: None,
    }],
    custom_nodes: &[],
    core_nodes: &[],
    min_comfyui: None,
    int8_fast_model_type: None,
    modes: &[
        ModeSpec {
            mode: "txt2img",
            max_references: 0,
            required_files: &[],
            sampling: None,
            build: Some(build_txt2img_hook),
        },
        ModeSpec::generic("img2img"),
        ModeSpec::generic("inpainting"),
    ],
    negative_prompt: true,
    resolution_multiple: 16,
    loader: Some(loader),
    text_encode: None,
};

/// The Wan families.
pub const FAMILIES: [&str; 3] = ["wan", "wan22a14b", "wan22ti2v"];

/// The stock split loaders, with the A14B low-noise expert as the main model
/// when one is paired (see [`low_noise_model`]).
fn loader(
    workflow: &mut serde_json::Map<String, serde_json::Value>,
    next_id: u32,
    params: &GenerationParams,
) -> LoaderOutput {
    match low_noise_model(params) {
        Some(low) => split_loader_nodes(workflow, next_id, params, low, None),
        None => split_loader_nodes(
            workflow,
            next_id,
            params,
            params.diffusion_model.as_deref().unwrap_or(""),
            misplaced_model_path(params),
        ),
    }
}

/// txt2img: the two-expert graph for a paired A14B, the generic one otherwise.
fn build_txt2img_hook(params: &GenerationParams, seed: i64) -> WorkflowResult {
    if two_expert_active(params) {
        build_txt2img(params, seed)
    } else {
        txt2img::build(params, seed)
    }
}

/// `GenerationParams::architecture_files` slot holding the Wan 2.2 A14B
/// low-noise expert.
pub const LOW_NOISE_MODEL_SLOT: &str = "low_noise_model";

/// Text-encoder filename marker for Wan's umT5-XXL (`umt5_xxl_fp8_e4m3fn_scaled`,
/// `umt5_xxl_fp16`, GGUF `umt5-xxl-encoder-Q8_0`).
pub const TEXT_ENCODER_MARKERS: [&str; 1] = ["umt5"];

/// VAE filename markers for the 16-channel Wan 2.1 VAE. The Qwen Image VAE is
/// a decoder fine-tune of it, so it decodes the same latents.
pub const WAN21_VAE_MARKERS: [&str; 4] = ["wan_2.1_vae", "wan2.1_vae", "wan2_1_vae", "wan21_vae"];

/// VAE filename markers for the 48-channel Wan 2.2 VAE (TI2V 5B only).
pub const WAN22_VAE_MARKERS: [&str; 4] = ["wan2.2_vae", "wan_2.2_vae", "wan2_2_vae", "wan22_vae"];

/// Which Wan checkpoint a diffusion-model file is, read from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WanVariant {
    /// Wan 2.1 T2V (1.3B or 14B), or a fine-tune of it.
    Wan21,
    /// Wan 2.2 TI2V 5B, which decodes with the Wan 2.2 VAE.
    Ti2v5b,
    /// Wan 2.2 A14B high-noise expert.
    A14bHighNoise,
    /// Wan 2.2 A14B low-noise expert. Runs alone as an image model too.
    A14bLowNoise,
    /// Image-to-video, Fun, VACE, S2V, Animate and similar: not text-to-image.
    NotTextToImage,
}

/// Lowercase alphanumeric tokens of a file's basename.
fn name_tokens(filename: &str) -> Vec<String> {
    let lower = filename.to_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    base.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

/// `high` / `low` as a whole token, or glued to `noise` (`HighNoise`,
/// `wan22_highnoise`).
fn names_expert(tokens: &[String], level: &str) -> bool {
    let glued = format!("{level}noise");
    tokens
        .iter()
        .any(|token| token == level || token.contains(&glued))
}

impl WanVariant {
    pub fn from_filename(filename: &str) -> Self {
        let tokens = name_tokens(filename);
        let has = |marker: &str| tokens.iter().any(|token| token == marker);
        if [
            "i2v", "flf2v", "s2v", "vace", "animate", "camera", "fun", "chrono",
        ]
        .iter()
        .any(|marker| has(marker))
            || tokens.iter().any(|token| token.starts_with("chronoedit"))
        {
            return Self::NotTextToImage;
        }
        if has("ti2v") || has("5b") {
            return Self::Ti2v5b;
        }
        if names_expert(&tokens, "high") {
            return Self::A14bHighNoise;
        }
        if names_expert(&tokens, "low") {
            return Self::A14bLowNoise;
        }
        Self::Wan21
    }
}

/// A Wan split model, as the frontend resolved it.
fn is_wan_split(params: &GenerationParams) -> bool {
    FAMILIES.contains(&params.model_architecture.as_str()) && params.use_split_model
}

/// The selected diffusion model's variant, for a Wan split model.
pub fn variant(params: &GenerationParams) -> Option<WanVariant> {
    is_wan_split(params)
        .then(|| WanVariant::from_filename(params.diffusion_model.as_deref().unwrap_or("")))
}

/// The low-noise expert paired with a selected Wan 2.2 A14B high-noise model.
/// When set, it is the main model of every graph: only the txt2img start runs
/// on the high-noise expert, and img2img, inpainting, hires fix and the
/// detailers all work at noise levels the low-noise expert was trained for.
pub fn low_noise_model(params: &GenerationParams) -> Option<&str> {
    if variant(params) != Some(WanVariant::A14bHighNoise) {
        return None;
    }
    params
        .architecture_files
        .get(LOW_NOISE_MODEL_SLOT)
        .map(String::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

/// Whether this request runs the two-expert Wan 2.2 A14B txt2img. A paused
/// run keeps its single-sampler stages on the low-noise expert.
pub fn two_expert_active(params: &GenerationParams) -> bool {
    params.mode == "txt2img" && params.stage.is_none() && low_noise_model(params).is_some()
}

/// Step where the high-noise expert hands over: half way, as in Comfy-Org's
/// Wan 2.2 A14B template (10 of 20).
pub fn switch_step(steps: u32) -> u32 {
    let steps = steps.max(2);
    (steps / 2).clamp(1, steps - 1)
}

/// Which A14B experts a LoRA belongs on: `(high, low)`. Wan 2.2 LoRAs ship
/// as `..._high_noise` / `..._low_noise` pairs; anything else (a Wan 2.1
/// LoRA, a single-file 2.2 LoRA) goes on both.
pub fn lora_experts(lora_name: &str) -> (bool, bool) {
    let tokens = name_tokens(lora_name);
    match (names_expert(&tokens, "high"), names_expert(&tokens, "low")) {
        (true, false) => (true, false),
        (false, true) => (false, true),
        _ => (true, true),
    }
}

/// Whether a LoRA goes on the main model chain. The main model of an A14B
/// run is the low-noise expert (paired, or selected on its own), so a
/// high-noise-only LoRA stays off it. Every other model takes every LoRA.
pub fn lora_applies_to_main_model(params: &GenerationParams, lora_name: &str) -> bool {
    let main_is_low_noise =
        low_noise_model(params).is_some() || variant(params) == Some(WanVariant::A14bLowNoise);
    !main_is_low_noise || lora_experts(lora_name).1
}

fn lower_contains_any(name: &str, markers: &[&str]) -> bool {
    let lower = name.to_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
}

fn quoted_or_none(name: &str) -> String {
    if name.is_empty() {
        "none".to_string()
    } else {
        format!("\"{name}\"")
    }
}

/// Generate-time guard for Wan split models. Each wrong pick here fails
/// inside ComfyUI with a tensor-shape error that does not say what to change.
pub fn validate(params: &GenerationParams) -> Result<(), String> {
    let Some(variant) = variant(params) else {
        return Ok(());
    };
    if params.mode == "video" {
        return Ok(());
    }
    let model = params.diffusion_model.as_deref().unwrap_or("").trim();

    if variant == WanVariant::NotTextToImage {
        return Err(format!(
            "\"{model}\" is a Wan image-to-video, control or editing model, which cannot make an image from a prompt. Pick a Wan text-to-video model: Wan 2.1 T2V, Wan 2.2 TI2V 5B, or the Wan 2.2 T2V A14B high-noise and low-noise pair."
        ));
    }

    if params.clip_type.as_deref() != Some("wan") {
        return Err(
            "Wan needs the CLIP loader type \"wan\". Re-select the Wan model so the text encoder settings refresh.".into(),
        );
    }
    let clip = params.clip_model.as_deref().unwrap_or("").trim();
    if clip.is_empty() || !lower_contains_any(clip, &TEXT_ENCODER_MARKERS) {
        return Err(format!(
            "Wan needs the umT5-XXL text encoder, but {} is selected. Open the model panel to download it (umt5_xxl_fp8_e4m3fn_scaled.safetensors), or if this file really is umT5-XXL, rename it to include \"umt5\".",
            quoted_or_none(clip)
        ));
    }

    let vae = params.vae.as_deref().unwrap_or("").trim();
    if variant == WanVariant::Ti2v5b {
        if !lower_contains_any(vae, &WAN22_VAE_MARKERS) {
            return Err(format!(
                "Wan 2.2 TI2V 5B decodes with the Wan 2.2 VAE, but {} is selected. Open the model panel to download it (wan2.2_vae.safetensors).",
                quoted_or_none(vae)
            ));
        }
    } else if lower_contains_any(vae, &WAN22_VAE_MARKERS) {
        return Err(format!(
            "\"{vae}\" only fits Wan 2.2 TI2V 5B. This Wan model decodes with the Wan 2.1 VAE (wan_2.1_vae.safetensors)."
        ));
    }

    if variant == WanVariant::A14bHighNoise && low_noise_model(params).is_none() {
        return Err(format!(
            "\"{model}\" is the high-noise half of Wan 2.2 A14B and only lays out the image; the low-noise model has to finish it. Put the matching low-noise file (for example wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors) in diffusion_models, or select the low-noise model on its own."
        ));
    }
    if let Some(low) = low_noise_model(params) {
        if WanVariant::from_filename(low) != WanVariant::A14bLowNoise {
            return Err(format!(
                "\"{low}\" is not a Wan 2.2 A14B low-noise model, so it cannot finish \"{model}\"."
            ));
        }
    }
    Ok(())
}

/// Wan 2.2 A14B txt2img: the high-noise expert samples the first half of the
/// schedule and hands its leftover noise to the low-noise expert, which owns
/// the rest and is what every later pass refines with.
pub fn build_txt2img(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();

    // Text encoder, VAE and the low-noise expert with its LoRAs.
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut next_id = ml.next_id;
    let low_model = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    // High-noise expert. LoRAs already went through the text encoder on the
    // low-noise chain, so this chain patches the model only.
    let high_name = params.diffusion_model.as_deref().unwrap_or("");
    let high_id = next_id.to_string();
    workflow.insert(
        high_id.clone(),
        unet_loader_node(high_name, misplaced_model_path(params), params),
    );
    next_id += 1;
    let mut high_model = (high_id, 0);
    for lora in &params.loras {
        if lora.name.trim().is_empty() || !lora_experts(&lora.name).0 {
            continue;
        }
        let lora_id = next_id.to_string();
        workflow.insert(
            lora_id.clone(),
            json!({
                "class_type": "LoraLoaderModelOnly",
                "inputs": {
                    "model": [high_model.0, high_model.1],
                    "lora_name": lora.name,
                    "strength_model": lora.strength_model
                }
            }),
        );
        high_model = (lora_id, 0);
        next_id += 1;
    }

    let (pos_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.positive_prompt,
        &params.positive_segments,
    );
    next_id = nid;
    let (neg_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.negative_prompt,
        &params.negative_segments,
    );
    next_id = nid;

    // ComfyUI adds the frame axis to a 4D empty latent for a 3D latent model.
    let latent_id = next_id.to_string();
    workflow.insert(
        latent_id.clone(),
        json!({
            "class_type": "EmptySD3LatentImage",
            "inputs": {
                "width": params.width,
                "height": params.height,
                "batch_size": params.batch_size
            }
        }),
    );
    next_id += 1;

    let switch = switch_step(params.steps);
    let high_pass_id = next_id.to_string();
    workflow.insert(
        high_pass_id.clone(),
        json!({
            "class_type": "KSamplerAdvanced",
            "inputs": {
                "model": [high_model.0, high_model.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [latent_id, 0],
                "add_noise": "enable",
                "noise_seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "start_at_step": 0,
                "end_at_step": switch,
                "return_with_leftover_noise": "enable"
            }
        }),
    );
    next_id += 1;

    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSamplerAdvanced",
            "inputs": {
                "model": [low_model.0.clone(), low_model.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [high_pass_id, 0],
                "add_noise": "disable",
                "noise_seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "start_at_step": switch,
                "end_at_step": 10000,
                "return_with_leftover_noise": "disable"
            }
        }),
    );
    next_id += 1;

    let (decode_id, next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        model_source: low_model,
        clip_source,
        positive_source: pos_source,
        negative_source: neg_source,
        vae_source,
        sampler_id,
        refiner_model_source: None,
        base_sources: Some(ml.base),
    }
}

/// Copy the main (low-noise) sampler's conditioning onto the high-noise pass
/// after the shared patches (ControlNet, style reference) rewired only the
/// main sampler. The high-noise pass is found through the main sampler's
/// latent input. The model input stays: it is the other expert.
pub fn sync_high_noise_pass(result: &mut WorkflowResult) {
    let Some(main_inputs) = result
        .workflow
        .get(&result.sampler_id)
        .and_then(|node| node.get("inputs"))
        .cloned()
    else {
        return;
    };
    let Some(high_id) = main_inputs["latent_image"]
        .get(0)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    if let Some(high_inputs) = result
        .workflow
        .get_mut(&high_id)
        .filter(|node| node["class_type"] == "KSamplerAdvanced")
        .and_then(|node| node.get_mut("inputs"))
    {
        for key in ["positive", "negative"] {
            high_inputs[key] = main_inputs[key].clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comfyui::types::LoraParam;
    use serde_json::Value;

    fn wan_params(diffusion_model: &str) -> GenerationParams {
        GenerationParams {
            mode: "txt2img".into(),
            model_architecture: "wan".into(),
            use_split_model: true,
            diffusion_model: Some(diffusion_model.into()),
            clip_model: Some("umt5_xxl_fp8_e4m3fn_scaled.safetensors".into()),
            clip_type: Some("wan".into()),
            vae: Some("wan_2.1_vae.safetensors".into()),
            positive_prompt: "a lighthouse at dusk".into(),
            negative_prompt: "blurry".into(),
            width: 1024,
            height: 1024,
            batch_size: 1,
            steps: 20,
            cfg: 3.5,
            sampler_name: "euler".into(),
            scheduler: "simple".into(),
            ..Default::default()
        }
    }

    fn a14b_params() -> GenerationParams {
        let mut params = wan_params("wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors");
        params.model_architecture = "wan22a14b".into();
        params.architecture_files.insert(
            LOW_NOISE_MODEL_SLOT.into(),
            "wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors".into(),
        );
        params
    }

    fn nodes_of<'a>(workflow: &'a serde_json::Map<String, Value>, class: &str) -> Vec<&'a Value> {
        workflow
            .values()
            .filter(|node| node["class_type"] == class)
            .collect()
    }

    #[test]
    fn variants_follow_comfy_org_and_community_filenames() {
        for (name, expected) in [
            ("wan2.1_t2v_14B_fp8_e4m3fn.safetensors", WanVariant::Wan21),
            ("wan2.1_t2v_1.3B_fp16.safetensors", WanVariant::Wan21),
            ("Wan2.1-T2V-14B-Q8_0.gguf", WanVariant::Wan21),
            ("wan2.2_ti2v_5B_fp16.safetensors", WanVariant::Ti2v5b),
            (
                "wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors",
                WanVariant::A14bHighNoise,
            ),
            (
                "wan2.2_t2v_low_noise_14B_fp16.safetensors",
                WanVariant::A14bLowNoise,
            ),
            (
                "Wan2.2-T2V-A14B-HighNoise-Q6_K.gguf",
                WanVariant::A14bHighNoise,
            ),
            (
                "smoothMixWan22_lowNoise.safetensors",
                WanVariant::A14bLowNoise,
            ),
            ("wan22/t2v_high_fp8.safetensors", WanVariant::A14bHighNoise),
            (
                "wan2.2_i2v_high_noise_14B_fp8_scaled.safetensors",
                WanVariant::NotTextToImage,
            ),
            (
                "wan2.1_i2v_480p_14B_fp16.safetensors",
                WanVariant::NotTextToImage,
            ),
            (
                "wan2.2_fun_control_5B_bf16.safetensors",
                WanVariant::NotTextToImage,
            ),
            (
                "wan2.1_vace_14B_fp16.safetensors",
                WanVariant::NotTextToImage,
            ),
            (
                "wan2.2_s2v_14B_bf16.safetensors",
                WanVariant::NotTextToImage,
            ),
            (
                "wan2.2_animate_14B_bf16.safetensors",
                WanVariant::NotTextToImage,
            ),
            (
                "chrono_edit_14B_fp16.safetensors",
                WanVariant::NotTextToImage,
            ),
        ] {
            assert_eq!(WanVariant::from_filename(name), expected, "{name}");
        }
    }

    #[test]
    fn registry_picks_family_encoder_and_vae() {
        use crate::architectures::{family_from_text, recommended_clip, recommended_vae};
        for (name, family) in [
            ("wan2.1_t2v_14B_fp8_e4m3fn.safetensors", Some("wan")),
            ("Wan2.1-T2V-1.3B-Q8_0.gguf", Some("wan")),
            (
                "wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors",
                Some("wan22a14b"),
            ),
            ("smoothMixWan22_lowNoise.safetensors", Some("wan22a14b")),
            ("wan2.2_ti2v_5B_fp16.safetensors", Some("wan22ti2v")),
            ("wan_2.1_vae.safetensors", None),
            ("anima-base-v1.0.safetensors", None),
            // CivitAI base models stay with the Anima mapping in commands/api.rs.
            ("Wan Video 14B t2v", None),
            ("Wan Video 2.2 T2V-A14B", None),
        ] {
            assert_eq!(family_from_text(name), family, "{name}");
        }

        let encoders: Vec<String> = [
            "qwen_2.5_vl_7b_fp8_scaled.safetensors",
            "umt5_xxl_fp8_e4m3fn_scaled.safetensors",
        ]
        .map(String::from)
        .to_vec();
        for family in FAMILIES {
            assert_eq!(
                recommended_clip(family, &encoders),
                Some((
                    Some("umt5_xxl_fp8_e4m3fn_scaled.safetensors".to_string()),
                    "wan"
                ))
            );
            // No stand-in encoder: the model panel downloads umT5.
            assert_eq!(
                recommended_clip(family, &encoders[..1]),
                Some((None, "wan"))
            );
        }

        let vaes: Vec<String> = [
            "sdxl_vae.safetensors",
            "qwen_image_vae.safetensors",
            "wan2.2_vae.safetensors",
            "wan_2.1_vae.safetensors",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            recommended_vae("wan22ti2v", &vaes),
            Some(Some("wan2.2_vae.safetensors".to_string()))
        );
        assert_eq!(
            recommended_vae("wan22a14b", &vaes),
            Some(Some("wan_2.1_vae.safetensors".to_string()))
        );
        assert_eq!(
            recommended_vae("wan", &vaes[..3]),
            Some(Some("qwen_image_vae.safetensors".to_string()))
        );
        assert_eq!(recommended_vae("wan22ti2v", &vaes[..2]), Some(None));
        assert_eq!(recommended_vae("wan", &vaes[..1]), Some(None));
    }

    #[test]
    fn lora_halves_go_on_their_own_expert() {
        assert_eq!(
            lora_experts("wan2.2_t2v_lightx2v_4steps_lora_v1.1_high_noise.safetensors"),
            (true, false)
        );
        assert_eq!(
            lora_experts("wan2.2_t2v_lightx2v_4steps_lora_v1.1_low_noise.safetensors"),
            (false, true)
        );
        assert_eq!(
            lora_experts("Wan21_T2V_14B_lightx2v_cfg_step_distill.safetensors"),
            (true, true)
        );
        assert_eq!(
            lora_experts("followed_the_highway.safetensors"),
            (true, true)
        );
    }

    #[test]
    fn switch_step_is_half_way_and_leaves_each_expert_a_step() {
        assert_eq!(switch_step(20), 10);
        assert_eq!(switch_step(30), 15);
        assert_eq!(switch_step(7), 3);
        assert_eq!(switch_step(2), 1);
        assert_eq!(switch_step(1), 1);
    }

    #[test]
    fn a14b_txt2img_runs_high_then_low_noise_expert() {
        let params = a14b_params();
        assert!(two_expert_active(&params));
        let result = build_txt2img(&params, 42);
        let workflow = &result.workflow;

        let loaders = nodes_of(workflow, "UNETLoader");
        let names: Vec<&str> = loaders
            .iter()
            .filter_map(|node| node["inputs"]["unet_name"].as_str())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors"));
        assert!(names.contains(&"wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors"));

        let clip = &nodes_of(workflow, "CLIPLoader")[0]["inputs"];
        assert_eq!(clip["type"], "wan");

        let main = &workflow[&result.sampler_id]["inputs"];
        assert_eq!(main["add_noise"], "disable");
        assert_eq!(main["start_at_step"], 10);
        assert_eq!(main["end_at_step"], 10000);
        let high_id = main["latent_image"][0].as_str().unwrap();
        let high = &workflow[high_id]["inputs"];
        assert_eq!(high["add_noise"], "enable");
        assert_eq!(high["end_at_step"], 10);
        assert_eq!(high["return_with_leftover_noise"], "enable");

        // Each sampler runs on its own expert; later passes refine with the low one.
        let loader_of = |id: &str| {
            workflow[id]["inputs"]["unet_name"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert!(loader_of(high["model"][0].as_str().unwrap()).contains("high_noise"));
        assert!(loader_of(main["model"][0].as_str().unwrap()).contains("low_noise"));
        assert_eq!(
            result.model_source,
            (main["model"][0].as_str().unwrap().to_string(), 0)
        );
    }

    #[test]
    fn a14b_loras_split_between_the_experts() {
        let mut params = a14b_params();
        params.loras = vec![
            LoraParam {
                name: "lightx2v_high_noise.safetensors".into(),
                strength_model: 1.0,
                strength_clip: 1.0,
            },
            LoraParam {
                name: "lightx2v_low_noise.safetensors".into(),
                strength_model: 1.0,
                strength_clip: 1.0,
            },
            LoraParam {
                name: "style.safetensors".into(),
                strength_model: 0.8,
                strength_clip: 0.8,
            },
        ];
        let result = build_txt2img(&params, 1);
        let low_chain: Vec<&str> = nodes_of(&result.workflow, "LoraLoader")
            .iter()
            .filter_map(|node| node["inputs"]["lora_name"].as_str())
            .collect();
        let high_chain: Vec<&str> = nodes_of(&result.workflow, "LoraLoaderModelOnly")
            .iter()
            .filter_map(|node| node["inputs"]["lora_name"].as_str())
            .collect();
        assert_eq!(low_chain.len(), 2);
        assert!(low_chain.contains(&"lightx2v_low_noise.safetensors"));
        assert!(low_chain.contains(&"style.safetensors"));
        assert_eq!(high_chain.len(), 2);
        assert!(high_chain.contains(&"lightx2v_high_noise.safetensors"));
        assert!(high_chain.contains(&"style.safetensors"));
    }

    #[test]
    fn single_model_variants_keep_the_generic_graph() {
        for name in [
            "wan2.1_t2v_14B_fp8_e4m3fn.safetensors",
            "wan2.2_ti2v_5B_fp16.safetensors",
            "wan2.2_t2v_low_noise_14B_fp16.safetensors",
        ] {
            let params = wan_params(name);
            assert!(!two_expert_active(&params), "{name}");
            assert_eq!(low_noise_model(&params), None, "{name}");
        }
    }

    #[test]
    fn other_modes_and_paused_runs_use_the_low_noise_expert_alone() {
        let mut params = a14b_params();
        params.mode = "img2img".into();
        assert!(!two_expert_active(&params));
        assert_eq!(
            low_noise_model(&params),
            Some("wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors")
        );
        let mut workflow = serde_json::Map::new();
        load_model_nodes(&mut workflow, 1, &params);
        let names: Vec<&str> = nodes_of(&workflow, "UNETLoader")
            .iter()
            .filter_map(|node| node["inputs"]["unet_name"].as_str())
            .collect();
        assert_eq!(
            names,
            vec!["wan2.2_t2v_low_noise_14B_fp8_scaled.safetensors"]
        );
    }

    #[test]
    fn validation_names_the_fix() {
        assert!(validate(&a14b_params()).is_ok());
        assert!(validate(&wan_params("wan2.1_t2v_14B_fp16.safetensors")).is_ok());

        let mut five_b = wan_params("wan2.2_ti2v_5B_fp16.safetensors");
        assert!(validate(&five_b).unwrap_err().contains("wan2.2_vae"));
        five_b.vae = Some("wan2.2_vae.safetensors".into());
        assert!(validate(&five_b).is_ok());

        let mut wrong_vae = wan_params("wan2.1_t2v_14B_fp16.safetensors");
        wrong_vae.vae = Some("wan2.2_vae.safetensors".into());
        assert!(validate(&wrong_vae).is_err());

        let mut qwen_encoder = wan_params("wan2.1_t2v_14B_fp16.safetensors");
        qwen_encoder.clip_model = Some("qwen_2.5_vl_7b_fp8_scaled.safetensors".into());
        assert!(validate(&qwen_encoder).unwrap_err().contains("umT5"));
        qwen_encoder.clip_model = Some("umt5_xxl_fp16.safetensors".into());
        qwen_encoder.clip_type = Some("qwen_image".into());
        assert!(validate(&qwen_encoder).is_err());

        let lone_high = wan_params("wan2.2_t2v_high_noise_14B_fp8_scaled.safetensors");
        assert!(validate(&lone_high).unwrap_err().contains("low-noise"));

        let i2v = wan_params("wan2.2_i2v_low_noise_14B_fp8_scaled.safetensors");
        assert!(validate(&i2v).unwrap_err().contains("image-to-video"));

        // Anima also loads with CLIP type "wan"; none of this applies to it.
        let mut anima = wan_params("anima-base-v1.0.safetensors");
        anima.model_architecture = "anima".into();
        anima.clip_model = Some("qwen_3_06b_base.safetensors".into());
        assert!(validate(&anima).is_ok());
    }

    #[test]
    fn the_full_image_pipeline_dispatches_the_two_expert_graph() {
        let params = a14b_params();
        assert!(crate::templates::validate_generation_params(&params).is_ok());
        let workflow = crate::templates::build_workflow(&params, 3, false);
        let samplers = workflow
            .as_object()
            .unwrap()
            .values()
            .filter(|node| node["class_type"] == "KSamplerAdvanced")
            .count();
        assert_eq!(samplers, 2);
    }

    #[test]
    fn controlnet_style_rewires_reach_the_high_noise_pass() {
        let params = a14b_params();
        let mut result = build_txt2img(&params, 7);
        let main = result.sampler_id.clone();
        result.workflow.get_mut(&main).unwrap()["inputs"]["positive"] = json!(["999", 0]);
        sync_high_noise_pass(&mut result);
        let high_id = result.workflow[&main]["inputs"]["latent_image"][0]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            result.workflow[&high_id]["inputs"]["positive"],
            json!(["999", 0])
        );
        assert!(
            result.workflow[&high_id]["inputs"]["model"][0]
                != result.workflow[&main]["inputs"]["model"][0]
        );
    }
}
