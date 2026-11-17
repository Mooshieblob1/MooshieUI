//! Stable Diffusion 3 / 3.5 from split files.
//!
//! An all-in-one SD3 checkpoint (Comfy-Org's `sd3.5_large_fp8_scaled`, or
//! `sd3.5_medium_incl_clips_*`) bakes its three text encoders in and loads with
//! `CheckpointLoaderSimple` like any other checkpoint. Everything else ships the
//! encoders apart and needs ComfyUI's SD3 recipe instead:
//!
//! - MODEL from `UNETLoader` (or `UnetLoaderGGUF` for city96's GGUFs).
//! - CLIP from `TripleCLIPLoader` with CLIP-L, CLIP-G and T5-XXL. SD3 runs on
//!   any subset of the three (ComfyUI zero-fills the missing ones), so two
//!   files load through `DualCLIPLoader` and one through `CLIPLoader`, both
//!   with type `sd3`. Dropping T5 is Stability's documented low-memory setup.
//! - VAE from `VAELoader`, unless the file carries its own. Stability's own
//!   `sd3_medium` / `sd3.5_large` / `sd3.5_medium` / `sd3.5_large_turbo` files
//!   hold the MMDiT and the VAE but no encoders, so detection treats them as
//!   diffusion models with a baked VAE. When such a file sits in
//!   `checkpoints/` and no VAE is picked, one `CheckpointLoaderSimple` gives
//!   both MODEL and VAE, as in ComfyUI's own SD3.5 example workflow.

use serde_json::{json, Map, Value};

use crate::comfyui::types::GenerationParams;

/// `architecture_files` slot holding the CLIP-G (OpenCLIP bigG) encoder.
pub const CLIP_G_SLOT: &str = "clip_g";
/// `architecture_files` slot holding the T5-XXL encoder.
pub const T5XXL_SLOT: &str = "t5xxl";

/// Filename markers for each SD3 encoder, as Stability and Comfy-Org name
/// them (`clip_l.safetensors`, `clip_g.safetensors`, `t5xxl_fp16.safetensors`,
/// `t5xxl_fp8_e4m3fn_scaled.safetensors`). Used only to recommend files, never
/// to reject one: fine-tuned CLIP-L encoders keep their own names.
pub const CLIP_L_MARKERS: [&str; 2] = ["clip_l", "clip-l"];
pub const CLIP_G_MARKERS: [&str; 2] = ["clip_g", "clip-g"];
pub const T5XXL_MARKERS: [&str; 3] = ["t5xxl", "t5_xxl", "t5-xxl"];

/// SD3 VAE filename markers. The SD3 VAE has 16 latent channels like the Flux 1
/// VAE but different weights and scaling, so the two are not interchangeable.
pub const VAE_MARKERS: [&str; 4] = ["sd3", "sd35", "sd_3", "stable-diffusion-3"];

/// True when this request should load SD3 through the split recipe.
pub fn is_split_sd3(params: &GenerationParams) -> bool {
    params.use_split_model && params.model_architecture == "sd3"
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

/// Pre-LoRA sources from the SD3 split loaders.
pub struct Sd3Base {
    pub model: (String, u32),
    pub clip: (String, u32),
    pub vae: (String, u32),
    pub next_id: u32,
}

/// Emit the SD3 split loaders. [`validate`] has already checked that an
/// encoder is picked and that a VAE is either picked or baked into the file.
pub fn load_base(
    workflow: &mut Map<String, Value>,
    mut next_id: u32,
    params: &GenerationParams,
) -> Sd3Base {
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

    Sd3Base {
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
            "SD3 needs its text encoders: CLIP-L, CLIP-G and T5-XXL (clip_l.safetensors, clip_g.safetensors, t5xxl_fp8_e4m3fn_scaled.safetensors). Open the model panel to download them. Any one or two of them also works, at lower quality."
                .into(),
        );
    }
    if trimmed(params.vae.as_deref()).is_empty() && !vae_from_model_file(params) {
        return Err(
            "This SD3 file has no VAE of its own. Pick the SD3 VAE under VAE, or download it from the model panel."
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

    fn build(p: &GenerationParams) -> (Map<String, Value>, Sd3Base) {
        let mut wf = Map::new();
        let base = load_base(&mut wf, 1, p);
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
