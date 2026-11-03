//! Image to 3D Gaussian splat with ComfyUI's native TripoSplat nodes.
//!
//! Idol (speridlabs/idol, repackaged as Comfy-Org/Idol) is a fine-tune of
//! TripoSplat and runs on the same nodes, so one graph serves both. The graph
//! mirrors ComfyUI's "Image to Gaussian Splat (TripoSplat)" blueprint:
//! optional BiRefNet background removal, the TripoSplat crop, DINOv3 + FLUX.2
//! VAE conditioning, a KSampler pass and the splat decoder. The result is
//! written as a standard 3DGS `.ply` through `SplatToFile3D` + `SaveGLB`,
//! which needs no viewport state (unlike `SaveGaussianSplat`).
//!
//! Like music, the graph has its own terminal node and never enters
//! `finish_workflow`'s image chain, so nothing lands in the image gallery.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Every node the graph uses. All ship with ComfyUI v0.41.1.
pub const REQUIRED_NODES: &[&str] = &[
    "LoadImage",
    "InvertMask",
    "LoadBackgroundRemovalModel",
    "RemoveBackground",
    "TripoSplatPreprocessImage",
    "UNETLoader",
    "CLIPVisionLoader",
    "VAELoader",
    "TripoSplatConditioning",
    "KSampler",
    "VAEDecodeTripoSplat",
    "SplatToFile3D",
    "SaveGLB",
];

/// ComfyUI output subfolder and filename prefix of the saved splat. `status`
/// only accepts outputs that match both, so a hostile or confused worker
/// cannot point the app at another file.
pub const OUTPUT_SUBFOLDER: &str = "3d";
pub const OUTPUT_PREFIX: &str = "mooshie_splat_";

pub const MIN_GAUSSIANS: u32 = 32_768;
pub const MAX_GAUSSIANS: u32 = 1_048_576;
pub const DEFAULT_GAUSSIANS: u32 = 262_144;
pub const MIN_STEPS: u32 = 4;
pub const MAX_STEPS: u32 = 100;

/// Node ids the frontend maps to progress stages.
pub const NODE_BACKGROUND: &str = "3";
pub const NODE_CONDITIONING: &str = "10";
pub const NODE_SAMPLER: &str = "11";
pub const NODE_DECODE: &str = "12";
pub const NODE_SAVE: &str = "14";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplatParams {
    /// Diffusion model in `diffusion_models`, e.g. `idol_fp16.safetensors`.
    pub model: String,
    #[serde(default = "default_true")]
    pub remove_background: bool,
    #[serde(default = "default_steps")]
    pub steps: u32,
    #[serde(default = "default_cfg")]
    pub cfg: f64,
    #[serde(default = "default_gaussians")]
    pub num_gaussians: u32,
    #[serde(with = "crate::comfyui::types::seed_string")]
    pub seed: i64,
}

fn default_true() -> bool {
    true
}
fn default_steps() -> u32 {
    20
}
fn default_cfg() -> f64 {
    3.0
}
fn default_gaussians() -> u32 {
    DEFAULT_GAUSSIANS
}

/// The model files one run needs, resolved from the worker's model lists.
#[derive(Debug, Clone, PartialEq)]
pub struct SplatModels {
    pub unet: String,
    pub clip_vision: String,
    pub decoder: String,
    pub flux2_vae: String,
    /// Only needed when background removal is on.
    pub bg_removal: Option<String>,
}

fn lower(name: &str) -> String {
    name.to_lowercase().replace('\\', "/")
}

fn base(name: &str) -> String {
    let l = lower(name);
    l.rsplit('/').next().unwrap_or(&l).to_string()
}

/// TripoSplat-family denoisers in `diffusion_models` (Idol or TripoSplat).
pub fn is_splat_unet(name: &str) -> bool {
    let b = base(name);
    (b.contains("idol") || b.contains("triposplat"))
        && !b.contains("decoder")
        && !b.contains("vae")
        && b.ends_with(".safetensors")
}

/// The splat decoder in `vae` (Idol's or TripoSplat's).
pub fn is_splat_decoder(name: &str) -> bool {
    let b = base(name);
    (b.contains("idol") || b.contains("triposplat")) && b.contains("decoder")
}

/// DINOv3 ViT-H/16+ image encoder in `clip_vision`.
pub fn is_dinov3(name: &str) -> bool {
    let b = base(name);
    b.contains("dino_v3") || b.contains("dinov3")
}

/// The FLUX.2 VAE in `vae` (shared with the FLUX.2 / Klein image families).
pub fn is_flux2_vae(name: &str) -> bool {
    let b = base(name);
    (b.contains("flux2") || b.contains("flux.2")) && b.contains("vae") && !is_splat_decoder(name)
}

pub fn is_birefnet(name: &str) -> bool {
    base(name).contains("birefnet")
}

/// The decoder trained with this denoiser: Idol's for Idol, TripoSplat's for
/// TripoSplat, otherwise whichever splat decoder is installed.
pub fn pick_decoder<'a>(unet: &str, decoders: &'a [String]) -> Option<&'a String> {
    let family = if base(unet).contains("idol") {
        "idol"
    } else {
        "triposplat"
    };
    decoders
        .iter()
        .find(|d| base(d).contains(family))
        .or_else(|| decoders.first())
}

pub fn validate(params: &SplatParams) -> Result<(), String> {
    if !is_splat_unet(&params.model) {
        return Err(
            "Select an Idol or TripoSplat model from ComfyUI's diffusion_models folder.".into(),
        );
    }
    if !(MIN_STEPS..=MAX_STEPS).contains(&params.steps) {
        return Err(format!(
            "Steps must be between {MIN_STEPS} and {MAX_STEPS}."
        ));
    }
    if !params.cfg.is_finite() || !(1.0..=10.0).contains(&params.cfg) {
        return Err("Guidance must be between 1 and 10.".into());
    }
    if !(MIN_GAUSSIANS..=MAX_GAUSSIANS).contains(&params.num_gaussians)
        || !params.num_gaussians.is_multiple_of(32)
    {
        return Err(format!(
            "Gaussian count must be a multiple of 32 between {MIN_GAUSSIANS} and {MAX_GAUSSIANS}."
        ));
    }
    Ok(())
}

/// Build the API-format graph. `image` is a filename already uploaded to the
/// worker's input folder.
pub fn build(params: &SplatParams, models: &SplatModels, image: &str, seed: i64) -> Value {
    let mut graph = json!({
        "1": {"class_type": "LoadImage", "inputs": {"image": image}},
        "5": {"class_type": "TripoSplatPreprocessImage", "inputs": {
            "image": ["1", 0], "erode_radius": 1, "size": 1024}},
        "6": {"class_type": "UNETLoader", "inputs": {
            "unet_name": models.unet, "weight_dtype": "default"}},
        "7": {"class_type": "CLIPVisionLoader", "inputs": {"clip_name": models.clip_vision}},
        "8": {"class_type": "VAELoader", "inputs": {"vae_name": models.decoder}},
        "9": {"class_type": "VAELoader", "inputs": {"vae_name": models.flux2_vae}},
        NODE_CONDITIONING: {"class_type": "TripoSplatConditioning", "inputs": {
            "clip_vision": ["7", 0], "vae": ["9", 0], "image": ["5", 0]}},
        NODE_SAMPLER: {"class_type": "KSampler", "inputs": {
            "model": ["6", 0], "seed": seed, "steps": params.steps, "cfg": params.cfg,
            "sampler_name": "dpmpp_2m", "scheduler": "simple",
            "positive": [NODE_CONDITIONING, 0], "negative": [NODE_CONDITIONING, 1],
            "latent_image": [NODE_CONDITIONING, 2], "denoise": 1.0}},
        NODE_DECODE: {"class_type": "VAEDecodeTripoSplat", "inputs": {
            "samples": [NODE_SAMPLER, 0], "vae": ["8", 0],
            "num_gaussians": params.num_gaussians, "seed": seed}},
        "13": {"class_type": "SplatToFile3D", "inputs": {"splat": [NODE_DECODE, 0], "format": "ply"}},
        NODE_SAVE: {"class_type": "SaveGLB", "inputs": {
            "mesh": ["13", 0],
            "filename_prefix": format!("{OUTPUT_SUBFOLDER}/{}", OUTPUT_PREFIX.trim_end_matches('_'))}},
    });
    // The crop needs a foreground mask. BiRefNet's mask is already foreground;
    // LoadImage's mask is inverted alpha, so flip it to get the subject of a
    // transparent PNG (an opaque image becomes a full-frame subject).
    match (params.remove_background, &models.bg_removal) {
        (true, Some(bg)) => {
            graph["2"] = json!({"class_type": "LoadBackgroundRemovalModel",
                "inputs": {"bg_removal_name": bg}});
            graph[NODE_BACKGROUND] = json!({"class_type": "RemoveBackground",
                "inputs": {"bg_removal_model": ["2", 0], "image": ["1", 0]}});
            graph["5"]["inputs"]["mask"] = json!([NODE_BACKGROUND, 0]);
        }
        _ => {
            graph["4"] = json!({"class_type": "InvertMask", "inputs": {"mask": ["1", 1]}});
            graph["5"]["inputs"]["mask"] = json!(["4", 0]);
        }
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> SplatParams {
        SplatParams {
            model: "idol_fp16.safetensors".into(),
            remove_background: true,
            steps: 20,
            cfg: 3.0,
            num_gaussians: DEFAULT_GAUSSIANS,
            seed: 7,
        }
    }

    fn models() -> SplatModels {
        SplatModels {
            unet: "idol_fp16.safetensors".into(),
            clip_vision: "dino_v3_vit_h_bf16.safetensors".into(),
            decoder: "idol_decoder_fp16.safetensors".into(),
            flux2_vae: "flux2-vae.safetensors".into(),
            bg_removal: Some("birefnet.safetensors".into()),
        }
    }

    #[test]
    fn classifies_comfy_org_and_triposplat_files() {
        assert!(is_splat_unet("idol_fp16.safetensors"));
        assert!(is_splat_unet("3d/triposplat_fp16.safetensors"));
        assert!(!is_splat_unet("idol_decoder_fp16.safetensors"));
        assert!(!is_splat_unet("flux2_klein_9b.safetensors"));
        assert!(is_splat_decoder("idol_decoder_fp16.safetensors"));
        assert!(is_splat_decoder("triposplat_vae_decoder_fp16.safetensors"));
        assert!(is_dinov3("dino_v3_vit_h_bf16.safetensors"));
        assert!(is_dinov3("sub\\dinov3_vith16plus.safetensors"));
        assert!(is_flux2_vae("flux2-vae.safetensors"));
        assert!(!is_flux2_vae("ae.safetensors"));
        assert!(is_birefnet("birefnet.safetensors"));
    }

    #[test]
    fn decoder_follows_the_denoiser_family() {
        let decoders = vec![
            "triposplat_vae_decoder_fp16.safetensors".to_string(),
            "idol_decoder_fp16.safetensors".to_string(),
        ];
        assert_eq!(
            pick_decoder("idol_fp16.safetensors", &decoders).unwrap(),
            "idol_decoder_fp16.safetensors"
        );
        assert_eq!(
            pick_decoder("triposplat_fp16.safetensors", &decoders).unwrap(),
            "triposplat_vae_decoder_fp16.safetensors"
        );
        assert!(pick_decoder("idol_fp16.safetensors", &[]).is_none());
    }

    #[test]
    fn validation_rejects_out_of_range_settings() {
        assert!(validate(&params()).is_ok());
        assert!(validate(&SplatParams {
            model: "sdxl.safetensors".into(),
            ..params()
        })
        .is_err());
        assert!(validate(&SplatParams {
            steps: 0,
            ..params()
        })
        .is_err());
        assert!(validate(&SplatParams {
            cfg: f64::NAN,
            ..params()
        })
        .is_err());
        assert!(validate(&SplatParams {
            num_gaussians: 262_145,
            ..params()
        })
        .is_err());
        assert!(validate(&SplatParams {
            num_gaussians: 16_384,
            ..params()
        })
        .is_err());
        assert!(validate(&SplatParams {
            num_gaussians: MAX_GAUSSIANS,
            ..params()
        })
        .is_ok());
    }

    #[test]
    fn graph_wires_background_removal_into_the_crop() {
        let g = build(&params(), &models(), "in.png", 42);
        assert_eq!(g["3"]["class_type"], "RemoveBackground");
        assert_eq!(g["5"]["inputs"]["mask"], json!(["3", 0]));
        assert!(g.get("4").is_none());
        assert_eq!(g["11"]["inputs"]["seed"], 42);
        assert_eq!(g["11"]["inputs"]["latent_image"], json!(["10", 2]));
        assert_eq!(g["12"]["inputs"]["num_gaussians"], DEFAULT_GAUSSIANS);
        assert_eq!(
            g["8"]["inputs"]["vae_name"],
            "idol_decoder_fp16.safetensors"
        );
        assert_eq!(g["9"]["inputs"]["vae_name"], "flux2-vae.safetensors");
        assert_eq!(g["13"]["inputs"]["format"], "ply");
        assert_eq!(g["14"]["inputs"]["filename_prefix"], "3d/mooshie_splat");
        // Every link points at a node that exists.
        for (_, node) in g.as_object().unwrap() {
            for (_, input) in node["inputs"].as_object().unwrap() {
                if let Some(link) = input.as_array() {
                    let target = link[0].as_str().unwrap();
                    assert!(g.get(target).is_some(), "dangling link to {target}");
                }
            }
        }
    }

    #[test]
    fn graph_uses_the_alpha_mask_without_background_removal() {
        let p = SplatParams {
            remove_background: false,
            ..params()
        };
        let g = build(&p, &models(), "in.png", 1);
        assert_eq!(g["4"]["class_type"], "InvertMask");
        assert_eq!(g["4"]["inputs"]["mask"], json!(["1", 1]));
        assert_eq!(g["5"]["inputs"]["mask"], json!(["4", 0]));
        assert!(g.get("2").is_none() && g.get("3").is_none());
        // Removal requested but no BiRefNet installed falls back to alpha too.
        let g = build(
            &params(),
            &SplatModels {
                bg_removal: None,
                ..models()
            },
            "in.png",
            1,
        );
        assert_eq!(g["5"]["inputs"]["mask"], json!(["4", 0]));
    }

    #[test]
    fn graph_uses_only_required_nodes() {
        let g = build(&params(), &models(), "in.png", 1);
        for (_, node) in g.as_object().unwrap() {
            let class = node["class_type"].as_str().unwrap();
            assert!(
                REQUIRED_NODES.contains(&class),
                "{class} missing from REQUIRED_NODES"
            );
        }
    }

    #[test]
    fn params_deserialize_with_defaults() {
        let p: SplatParams =
            serde_json::from_value(json!({"model": "idol_fp16.safetensors", "seed": "-1"}))
                .unwrap();
        assert!(p.remove_background);
        assert_eq!(p.steps, 20);
        assert_eq!(p.num_gaussians, DEFAULT_GAUSSIANS);
        assert_eq!(p.seed, -1);
    }
}
