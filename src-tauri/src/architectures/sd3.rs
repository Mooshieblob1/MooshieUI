//! Stable Diffusion 3 / 3.5 (SD3 Medium, SD3.5 Large, Large Turbo, Medium).
//!
//! An all-in-one SD3 checkpoint (Comfy-Org's `sd3.5_large_fp8_scaled`, or
//! `sd3.5_medium_incl_clips_*`) bakes its three text encoders in and loads with
//! `CheckpointLoaderSimple` like any other checkpoint. Everything else ships the
//! encoders apart and goes through [`load`], ComfyUI's SD3 recipe:
//!
//! - MODEL from `UNETLoader` (or `UnetLoaderGGUF` for city96's GGUFs).
//! - CLIP from `TripleCLIPLoader` with CLIP-L (`clip_model`), CLIP-G and T5-XXL
//!   (the `clip_g` / `t5xxl` model slots). SD3 runs on any subset of the three
//!   (ComfyUI zero-fills the missing ones), so two files load through
//!   `DualCLIPLoader` and one through `CLIPLoader`, both with type `sd3`.
//!   Dropping T5 is Stability's documented low-memory setup.
//! - VAE from `VAELoader`, unless the file carries its own. Stability's own
//!   `sd3_medium` / `sd3.5_large` / `sd3.5_medium` / `sd3.5_large_turbo` files
//!   hold the MMDiT and the VAE but no encoders, so detection treats them as
//!   diffusion models with a baked VAE (`baked_vae` in `commands::api`). When
//!   such a file sits in `checkpoints/` and no VAE is picked, one
//!   `CheckpointLoaderSimple` gives both MODEL and VAE, as in ComfyUI's own
//!   SD3.5 example workflow.
//!
//! The SD3 VAE is only published in Stability's gated repos, so it has no
//! auto-download here; the Model Hub links it ("HF access").

use serde_json::{json, Map, Value};

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, LoaderOutput, ModeSpec, ModelFile,
    ModelSlot, PromptStyle, Sampling, VaeSpec,
};
use crate::comfyui::types::GenerationParams;

/// True for every SD3 family. Shared code that treats SD3 specially (latent
/// format, `ModelSamplingSD3`) asks this rather than naming the ids.
pub fn is_sd3(family: &str) -> bool {
    matches!(family, "sd3" | "sd3turbo")
}

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "sd3",
    label: "Stable Diffusion 3",
    families: &[
        // First: structural detection (`joint_blocks`) resolves to it.
        FamilySpec {
            family: "sd3",
            label: "SD3",
            detect: Detect {
                all_of: &[
                    &["sd3"],
                    &["sd 3"],
                    &["stable diffusion 3"],
                    &["stable-diffusion-3"],
                    &["stable_diffusion_3"],
                ],
                none_of: &["turbo"],
                base_models: &["sd 3", "sd 3.5", "sd 3.5 large", "sd 3.5 medium"],
            },
            turbo: false,
            sampling: Some(Sampling {
                steps: 28,
                cfg: 4.5,
                sampler: "euler",
                scheduler: "sgm_uniform",
                width: 1024,
                height: 1024,
            }),
            recommended: &[],
            text_encoder: None,
            vae: None,
        },
        // SD3.5 Large Turbo: distilled to 4 steps without guidance (Stability's card).
        FamilySpec {
            family: "sd3turbo",
            label: "SD3.5 Large Turbo",
            detect: Detect {
                all_of: &[
                    &["sd3", "turbo"],
                    &["sd 3", "turbo"],
                    &["stable diffusion 3", "turbo"],
                    &["stable-diffusion-3", "turbo"],
                    &["stable_diffusion_3", "turbo"],
                ],
                none_of: &[],
                base_models: &["sd 3.5 large turbo"],
            },
            turbo: true,
            sampling: Some(Sampling {
                steps: 4,
                cfg: 1.0,
                sampler: "euler",
                scheduler: "sgm_uniform",
                width: 1024,
                height: 1024,
            }),
            recommended: &[],
            text_encoder: None,
            vae: None,
        },
    ],
    split_only: false,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("stable-diffusion-3-medium"),
    text_encoder: Some(EncoderSpec {
        clip_type: "sd3",
        markers: &CLIP_L_MARKERS,
        strict: true,
        download: Some(ModelFile {
            filename: "clip_l.safetensors",
            url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5/resolve/main/text_encoders/clip_l.safetensors",
            category: "text_encoders",
            size: Some("~0.25 GB"),
            sha256: None,
        }),
    }),
    vae: Some(VaeSpec {
        basenames: &["sd3.5_vae.safetensors", "sd3_vae.safetensors"],
        markers: &VAE_MARKERS,
        strict: true,
        download: None,
    }),
    model_slots: &[
        ModelSlot {
            key: CLIP_G_SLOT,
            label: "CLIP-G",
            category: "text_encoders",
            markers: &CLIP_G_MARKERS,
            strict: false,
            download: Some(ModelFile {
                filename: "clip_g.safetensors",
                url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5/resolve/main/text_encoders/clip_g.safetensors",
                category: "text_encoders",
                size: Some("~1.4 GB"),
                sha256: None,
            }),
        },
        ModelSlot {
            key: T5XXL_SLOT,
            label: "T5-XXL",
            category: "text_encoders",
            markers: &T5XXL_MARKERS,
            strict: false,
            download: Some(ModelFile {
                filename: "t5xxl_fp8_e4m3fn_scaled.safetensors",
                url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5/resolve/main/text_encoders/t5xxl_fp8_e4m3fn_scaled.safetensors",
                category: "text_encoders",
                size: Some("~5.2 GB"),
                sha256: None,
            }),
        },
    ],
    custom_nodes: &[],
    core_nodes: &["TripleCLIPLoader", "ModelSamplingSD3", "EmptySD3LatentImage"],
    min_comfyui: None,
    int8_fast_model_type: None,
    modes: &[
        ModeSpec::generic("txt2img"),
        ModeSpec::generic("img2img"),
        ModeSpec::generic("inpainting"),
    ],
    negative_prompt: true,
    resolution_multiple: 16,
    loader: Some(load),
    text_encode: None,
};

/// `architecture_files` slot holding the CLIP-G (OpenCLIP bigG) encoder.
pub const CLIP_G_SLOT: &str = "clip_g";
/// `architecture_files` slot holding the T5-XXL encoder.
pub const T5XXL_SLOT: &str = "t5xxl";

/// Filename markers for each SD3 encoder, as Stability and Comfy-Org name
/// them (`clip_l.safetensors`, `clip_g.safetensors`, `t5xxl_fp16.safetensors`,
/// `t5xxl_fp8_e4m3fn_scaled.safetensors`). Used only to recommend files, never
/// to reject one, except CLIP-L, which the registry checks (the encoder is
/// strict). `t5_xxl` is left out: Chroma's `flan_t5_xxl` would match it.
pub const CLIP_L_MARKERS: [&str; 4] = ["clip_l", "clip-l", "vit-l-14", "vit_l_14"];
pub const CLIP_G_MARKERS: [&str; 2] = ["clip_g", "clip-g"];
pub const T5XXL_MARKERS: [&str; 3] = ["t5xxl", "t5-xxl", "t5-v1_1-xxl"];

/// SD3 VAE filename markers. The SD3 VAE has 16 latent channels like the Flux 1
/// VAE but different weights and scaling, so the two are not interchangeable.
pub const VAE_MARKERS: [&str; 4] = ["sd3", "sd35", "sd_3", "stable-diffusion-3"];

/// True when this request should load SD3 through the split recipe.
pub fn is_split_sd3(params: &GenerationParams) -> bool {
    params.use_split_model && is_sd3(&params.model_architecture)
}

fn trimmed(value: Option<&str>) -> &str {
    value.map(str::trim).unwrap_or("")
}

/// The picked encoders in CLIP-L, CLIP-G, T5-XXL order, without blanks or
/// repeats (picking one file twice would load it twice for nothing).
pub fn encoder_files(params: &GenerationParams) -> Vec<String> {
    let slots = [
        trimmed(params.clip_model.as_deref()),
        trimmed(
            params
                .architecture_files
                .get(CLIP_G_SLOT)
                .map(String::as_str),
        ),
        trimmed(
            params
                .architecture_files
                .get(T5XXL_SLOT)
                .map(String::as_str),
        ),
    ];
    let mut files: Vec<String> = Vec::new();
    for name in slots {
        if !name.is_empty() && !files.iter().any(|f| f == name) {
            files.push(name.to_string());
        }
    }
    files
}

fn is_gguf(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".gguf")
}

/// The text-encoder loader node for the picked files, or `None` when there are none.
/// GGUF encoders (city96's `t5-v1_1-xxl-encoder-Q8_0.gguf`) need ComfyUI-GGUF's
/// loaders, which take the same inputs as the core ones.
pub fn text_encoder_loader(files: &[String]) -> Option<Value> {
    let gguf = files.iter().any(|f| is_gguf(f));
    let class = |core: &str, gguf_class: &str| if gguf { gguf_class } else { core }.to_string();
    match files {
        [] => None,
        [only] => Some(json!({
            "class_type": class("CLIPLoader", "CLIPLoaderGGUF"),
            "inputs": { "clip_name": only, "type": "sd3" }
        })),
        [first, second] => Some(json!({
            "class_type": class("DualCLIPLoader", "DualCLIPLoaderGGUF"),
            "inputs": { "clip_name1": first, "clip_name2": second, "type": "sd3" }
        })),
        [first, second, third, ..] => Some(json!({
            "class_type": class("TripleCLIPLoader", "TripleCLIPLoaderGGUF"),
            "inputs": { "clip_name1": first, "clip_name2": second, "clip_name3": third }
        })),
    }
}

/// Whether MODEL and VAE come from one checkpoint loader: no VAE is picked and
/// the file sits in `checkpoints/`, which only happens for a file detection
/// found to carry a VAE but no encoders.
fn vae_from_model_file(params: &GenerationParams) -> bool {
    trimmed(params.vae.as_deref()).is_empty()
        && params.model_source_category.as_deref() == Some("checkpoints")
}

/// The SD3 loader hook: emits the split loaders. [`validate`] has already checked that an
/// encoder is picked and that a VAE is either picked or baked into the file.
pub fn load(
    workflow: &mut Map<String, Value>,
    mut next_id: u32,
    params: &GenerationParams,
) -> LoaderOutput {
    let unet_name = trimmed(params.diffusion_model.as_deref());
    let resolved_path = params
        .model_source_category
        .as_deref()
        .and(params.resolved_model_path.as_deref())
        .filter(|p| !p.is_empty());

    let model;
    let vae;
    if vae_from_model_file(params) {
        // Stability's own release files: one load gives MODEL and VAE. The
        // checkpoint loader's CLIP output is `None` for them and goes unused.
        let id = next_id.to_string();
        let node = match resolved_path {
            Some(path) => json!({
                "class_type": "MooshieCheckpointLoaderPath",
                "inputs": { "ckpt_path": path }
            }),
            None => json!({
                "class_type": "CheckpointLoaderSimple",
                "inputs": { "ckpt_name": unet_name }
            }),
        };
        workflow.insert(id.clone(), node);
        model = (id.clone(), 0);
        vae = (id, 2);
        next_id += 1;
    } else {
        let id = next_id.to_string();
        let node = if is_gguf(unet_name) {
            json!({
                "class_type": "UnetLoaderGGUF",
                "inputs": { "unet_name": unet_name }
            })
        } else if let Some(path) = resolved_path {
            json!({
                "class_type": "MooshieDiffusionLoaderPath",
                "inputs": { "unet_path": path, "weight_dtype": "default" }
            })
        } else {
            json!({
                "class_type": "UNETLoader",
                "inputs": { "unet_name": unet_name, "weight_dtype": "default" }
            })
        };
        workflow.insert(id.clone(), node);
        model = (id, 0);
        next_id += 1;

        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({
                "class_type": "VAELoader",
                "inputs": { "vae_name": trimmed(params.vae.as_deref()) }
            }),
        );
        vae = (id, 0);
        next_id += 1;
    }

    let id = next_id.to_string();
    // `validate` guarantees at least one encoder; an empty list cannot reach here.
    let loader = text_encoder_loader(&encoder_files(params)).unwrap_or_else(|| {
        json!({
            "class_type": "CLIPLoader",
            "inputs": { "clip_name": "", "type": "sd3" }
        })
    });
    workflow.insert(id.clone(), loader);
    let clip = (id, 0);
    next_id += 1;

    LoaderOutput {
        model,
        clip,
        vae,
        next_id,
    }
}

/// Fail fast on a split SD3 setup ComfyUI would reject with a less helpful error.
pub fn validate(params: &GenerationParams) -> Result<(), String> {
    if !is_split_sd3(params) {
        return Ok(());
    }
    if encoder_files(params).is_empty() {
        return Err(
            "SD3 needs its text encoders: CLIP-L, CLIP-G and T5-XXL (clip_l.safetensors, clip_g.safetensors, t5xxl_fp8_e4m3fn_scaled.safetensors). Open the model panel to download them."
                .into(),
        );
    }
    if trimmed(params.vae.as_deref()).is_empty() && !vae_from_model_file(params) {
        return Err(
            "This SD3 file has no VAE of its own. Pick the SD3 VAE under VAE, or download \"SD3.5 VAE (HF access)\" from the Model Hub."
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> GenerationParams {
        let mut p = GenerationParams {
            model_architecture: "sd3".into(),
            use_split_model: true,
            diffusion_model: Some("sd3.5_large_turbo-Q8_0.gguf".into()),
            clip_model: Some("clip_l.safetensors".into()),
            clip_type: Some("sd3".into()),
            vae: Some("sd3.5_vae.safetensors".into()),
            ..Default::default()
        };
        p.architecture_files
            .insert(CLIP_G_SLOT.into(), "clip_g.safetensors".into());
        p.architecture_files.insert(
            T5XXL_SLOT.into(),
            "t5xxl_fp8_e4m3fn_scaled.safetensors".into(),
        );
        p
    }

    fn build(p: &GenerationParams) -> (Map<String, Value>, LoaderOutput) {
        let mut wf = Map::new();
        let base = load(&mut wf, 1, p);
        (wf, base)
    }

    #[test]
    fn three_encoders_use_triple_clip_loader() {
        let (wf, base) = build(&params());
        let clip = &wf[&base.clip.0];
        assert_eq!(clip["class_type"], "TripleCLIPLoader");
        assert_eq!(clip["inputs"]["clip_name1"], "clip_l.safetensors");
        assert_eq!(clip["inputs"]["clip_name2"], "clip_g.safetensors");
        assert_eq!(
            clip["inputs"]["clip_name3"],
            "t5xxl_fp8_e4m3fn_scaled.safetensors"
        );
        assert_eq!(wf[&base.model.0]["class_type"], "UnetLoaderGGUF");
        assert_eq!(wf[&base.vae.0]["class_type"], "VAELoader");
        assert_eq!(base.next_id, 4);
        assert!(validate(&params()).is_ok());
    }

    #[test]
    fn fewer_encoders_use_dual_or_single_loader_with_sd3_type() {
        let mut p = params();
        p.architecture_files.remove(T5XXL_SLOT);
        let (wf, base) = build(&p);
        assert_eq!(wf[&base.clip.0]["class_type"], "DualCLIPLoader");
        assert_eq!(wf[&base.clip.0]["inputs"]["type"], "sd3");

        p.clip_model = None;
        let (wf, base) = build(&p);
        assert_eq!(wf[&base.clip.0]["class_type"], "CLIPLoader");
        assert_eq!(
            wf[&base.clip.0]["inputs"]["clip_name"],
            "clip_g.safetensors"
        );
        assert_eq!(wf[&base.clip.0]["inputs"]["type"], "sd3");
    }

    #[test]
    fn repeated_and_blank_encoders_are_dropped() {
        let mut p = params();
        p.architecture_files
            .insert(CLIP_G_SLOT.into(), "clip_l.safetensors".into());
        p.architecture_files.insert(T5XXL_SLOT.into(), "  ".into());
        assert_eq!(encoder_files(&p), vec!["clip_l.safetensors".to_string()]);
    }

    #[test]
    fn gguf_encoder_switches_to_gguf_loader() {
        let mut p = params();
        p.architecture_files
            .insert(T5XXL_SLOT.into(), "t5-v1_1-xxl-encoder-Q8_0.gguf".into());
        let (wf, base) = build(&p);
        assert_eq!(wf[&base.clip.0]["class_type"], "TripleCLIPLoaderGGUF");
    }

    #[test]
    fn baked_vae_file_in_checkpoints_loads_model_and_vae_from_one_loader() {
        let mut p = params();
        p.diffusion_model = Some("sd3.5_large.safetensors".into());
        p.model_source_category = Some("checkpoints".into());
        p.vae = None;
        let (wf, base) = build(&p);
        let loader = &wf[&base.model.0];
        assert_eq!(loader["class_type"], "CheckpointLoaderSimple");
        assert_eq!(loader["inputs"]["ckpt_name"], "sd3.5_large.safetensors");
        assert_eq!(base.vae, (base.model.0.clone(), 2));
        assert!(validate(&p).is_ok());

        // Resolved to an absolute path locally: the path loader instead.
        p.resolved_model_path = Some("/models/checkpoints/sd3.5_large.safetensors".into());
        let (wf, base) = build(&p);
        assert_eq!(
            wf[&base.model.0]["class_type"],
            "MooshieCheckpointLoaderPath"
        );
    }

    #[test]
    fn picked_vae_wins_over_the_baked_one() {
        let mut p = params();
        p.diffusion_model = Some("sd3.5_large.safetensors".into());
        p.model_source_category = Some("checkpoints".into());
        p.resolved_model_path = Some("/models/checkpoints/sd3.5_large.safetensors".into());
        let (wf, base) = build(&p);
        assert_eq!(
            wf[&base.model.0]["class_type"],
            "MooshieDiffusionLoaderPath"
        );
        assert_eq!(
            wf[&base.vae.0]["inputs"]["vae_name"],
            "sd3.5_vae.safetensors"
        );
    }

    #[test]
    fn validate_requires_an_encoder_and_a_vae() {
        let mut p = params();
        p.clip_model = None;
        p.architecture_files.clear();
        assert!(validate(&p).unwrap_err().contains("text encoders"));

        let mut p = params();
        p.vae = Some(String::new());
        assert!(validate(&p).unwrap_err().contains("no VAE"));

        // Not split SD3: nothing to check.
        let mut p = params();
        p.use_split_model = false;
        p.clip_model = None;
        p.architecture_files.clear();
        assert!(validate(&p).is_ok());
    }
}
