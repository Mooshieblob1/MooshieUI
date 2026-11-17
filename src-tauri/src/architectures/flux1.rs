//! Flux.1 (dev, schnell, Krea, Kontext) from split files.
//!
//! Flux.1 conditions on two text encoders: T5-XXL for the token sequence and
//! CLIP-L for the pooled vector that modulates every block. ComfyUI's recipe
//! is `DualCLIPLoader` with type `flux` and both files
//! (comfyanonymous/flux_text_encoders), whose Flux tokenizer also pads the T5
//! prompt to 256 tokens as the model was trained.
//!
//! Split Flux.1 used to load only T5, through `CLIPLoader` type `chroma`. That
//! runs, but with Chroma's unpadded T5 tokenizer and no CLIP-L, and Flux's
//! forward pass zero-fills the missing pooled vector, so every image came out
//! degraded without an error. Chroma itself is T5-only by design and keeps its
//! hand-written `CLIPLoader` type `chroma` path.
//!
//! T5 stays in `clip_model` (so settings saved before keep working) and CLIP-L
//! is the [`CLIP_L_SLOT`] model slot. The loader type is decided by the hook,
//! not by `clip_type`. Everything else about Flux.1 (detection, guidance,
//! ModelSamplingFlux, Kontext's reference latent, presets) is still
//! hand-written under the same family ids; the specs here declare the encoder,
//! VAE and modes, and the loader hook builds the dual-encoder graph.
//!
//! All-in-one Flux.1 checkpoints (encoders baked in) load through
//! `CheckpointLoaderSimple` as before: the hook only runs for split models.

use serde_json::{json, Map, Value};

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, LoaderOutput, ModeSpec, ModelFile,
    ModelSlot, PromptStyle, VaeSpec,
};
use crate::comfyui::types::GenerationParams;

/// `architecture_files` slot holding the CLIP-L encoder.
pub const CLIP_L_SLOT: &str = "clip_l";

/// T5-XXL filename markers: Comfy-Org's `t5xxl_fp16` / `t5xxl_fp8_e4m3fn_scaled`,
/// city96's `t5-v1_1-xxl-encoder-*` GGUFs and Google's FLAN-T5 XXL (a T5-XXL
/// fine-tune that loads the same way).
pub const T5XXL_MARKERS: &[&str] = &[
    "t5xxl",
    "t5_xxl",
    "t5-xxl",
    "t5-v1_1-xxl",
    "t5_v1_1_xxl",
    "t5-v1.1-xxl",
];

/// CLIP-L filename markers: Comfy-Org's `clip_l.safetensors` and the common
/// ViT-L/14 text fine-tunes (zer0int's `ViT-L-14-TEXT-detail-improved-*`,
/// LongCLIP-L).
pub const CLIP_L_MARKERS: &[&str] = &[
    "clip_l",
    "clip-l",
    "clipl",
    "vit-l-14",
    "vit_l_14",
    "longclip-l",
];

/// Flux 1 VAE markers. `ae.safetensors` is matched by basename; a `flux` name
/// counts only with a Flux.1 marker, so the 32-channel Flux.2 VAE
/// (`flux2-vae`) never does.
const VAE_MARKERS: &[&str] = &[
    "flux1", "flux.1", "flux_ae", "flux-ae", "flux_vae", "flux-vae",
];

const T5XXL_FILE: ModelFile = ModelFile {
    filename: "t5xxl_fp8_e4m3fn_scaled.safetensors",
    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/t5xxl_fp8_e4m3fn_scaled.safetensors",
    category: "text_encoders",
    size: Some("~5.2 GB"),
    sha256: Some("a498f0485dc9536735258018417c3fd7758dc3bccc0a645feaa472b34955557a"),
};

const CLIP_L_FILE: ModelFile = ModelFile {
    filename: "clip_l.safetensors",
    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/clip_l.safetensors",
    category: "text_encoders",
    size: Some("~246 MB"),
    sha256: Some("660c6f5b1abae9dc498ac2d21e1347d2abdb0cf6c0c0c8576cd796491d9a6cdd"),
};

/// The Flux 1 VAE, from Comfy-Org's ungated repackage (BFL's own repo is gated).
/// The same file Z-Image downloads.
const VAE_FILE: ModelFile = ModelFile {
    filename: "ae.safetensors",
    url:
        "https://huggingface.co/Comfy-Org/z_image_turbo/resolve/main/split_files/vae/ae.safetensors",
    category: "vae",
    size: Some("~335 MB"),
    sha256: Some("afc8e28272cd15db3919bacdb6918ce9c1ed22e96cb12c4d5ed0fba823529e38"),
};

const TEXT_ENCODER: EncoderSpec = EncoderSpec {
    clip_type: "flux",
    markers: T5XXL_MARKERS,
    strict: true,
    download: Some(T5XXL_FILE),
};

const VAE: VaeSpec = VaeSpec {
    basenames: &["ae.safetensors"],
    markers: VAE_MARKERS,
    strict: false,
    download: Some(VAE_FILE),
};

const MODEL_SLOTS: &[ModelSlot] = &[ModelSlot {
    key: CLIP_L_SLOT,
    label: "CLIP-L",
    category: "text_encoders",
    markers: CLIP_L_MARKERS,
    strict: true,
    download: Some(CLIP_L_FILE),
}];

/// Flux.1 is detected by the hand-written filename / CivitAI / tensor chain in
/// `commands::api`, which tells dev, schnell, Krea, Kontext and Flux.2 apart.
const fn family(family: &'static str, label: &'static str, turbo: bool) -> FamilySpec {
    FamilySpec {
        family,
        label,
        detect: Detect::NONE,
        turbo,
        sampling: None,
        recommended: &[],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }
}

const GENERIC_MODES: &[ModeSpec] = &[
    ModeSpec::generic("txt2img"),
    ModeSpec::generic("img2img"),
    ModeSpec::generic("inpainting"),
];

/// What every Flux.1 spec shares: encoders, VAE, CLIP-L slot and loader.
const BASE: ArchitectureSpec = ArchitectureSpec {
    id: "flux1",
    label: "Flux.1",
    families: &[],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: None,
    text_encoder: Some(TEXT_ENCODER),
    vae: Some(VAE),
    model_slots: MODEL_SLOTS,
    custom_nodes: &[],
    core_nodes: &[],
    min_comfyui: None,
    int8_fast_model_type: None,
    modes: GENERIC_MODES,
    negative_prompt: true,
    resolution_multiple: 16,
    loader: Some(load),
    text_encode: None,
};

/// Flux.1 dev, schnell and Krea: guidance-distilled, sampled at CFG 1, so the
/// negative prompt does nothing and the panel hides it (as it did before).
pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    families: &[
        family("flux1d", "Flux.1 Dev", false),
        family("flux1s", "Flux.1 Schnell", true),
        family("flux1krea", "Flux.1 Krea", false),
    ],
    negative_prompt: false,
    ..BASE
};

/// A Flux.1 model detection could not pin to a variant (often a de-distilled
/// fine-tune run with real CFG), so the negative prompt stays.
pub static GENERIC_SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "flux1_other",
    label: "Flux",
    families: &[family("flux", "Flux", false)],
    ..BASE
};

/// Flux.1 Kontext: single-reference Image Edit (`templates::image_edit`'s
/// Kontext branch) on top of the generic modes.
pub static KONTEXT_SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "flux1kontext",
    label: "Flux.1 Kontext",
    families: &[family("flux1kontext", "Flux.1 Kontext", false)],
    modes: &[
        ModeSpec::generic("txt2img"),
        ModeSpec::generic("img2img"),
        ModeSpec::generic("inpainting"),
        ModeSpec {
            mode: "image_edit",
            max_references: 1,
            required_files: &[],
            sampling: None,
            build: None,
        },
    ],
    ..BASE
};

fn trimmed(value: Option<&str>) -> &str {
    value.map(str::trim).unwrap_or("")
}

/// The picked encoders, T5 (`clip_model`) first, then CLIP-L, without blanks
/// or repeats.
pub fn encoder_files(params: &GenerationParams) -> Vec<String> {
    let slots = [
        trimmed(params.clip_model.as_deref()),
        trimmed(
            params
                .architecture_files
                .get(CLIP_L_SLOT)
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

/// The text-encoder loader node. Two files load through `DualCLIPLoader` type
/// `flux` (ComfyUI-GGUF's `DualCLIPLoaderGGUF` when one is a GGUF, such as
/// city96's T5 quants; it takes the same inputs and lists safetensors too).
/// The registry's strict encoder and slot checks guarantee two files reach
/// here; a lone file keeps the old T5-only load so the graph stays well-formed.
pub fn text_encoder_loader(params: &GenerationParams) -> Value {
    let files = encoder_files(params);
    let gguf = files.iter().any(|f| is_gguf(f));
    match files.as_slice() {
        [first, second, ..] => json!({
            "class_type": if gguf { "DualCLIPLoaderGGUF" } else { "DualCLIPLoader" },
            "inputs": {
                "clip_name1": first,
                "clip_name2": second,
                "type": "flux"
            }
        }),
        [only] => json!({
            "class_type": if gguf { "CLIPLoaderGGUF" } else { "CLIPLoader" },
            "inputs": { "clip_name": only, "type": "chroma" }
        }),
        [] => json!({
            "class_type": "CLIPLoader",
            "inputs": { "clip_name": "", "type": "chroma" }
        }),
    }
}

/// Loader hook: diffusion model, T5-XXL + CLIP-L, VAE.
fn load(
    workflow: &mut Map<String, Value>,
    mut next_id: u32,
    params: &GenerationParams,
) -> LoaderOutput {
    let model_id = next_id.to_string();
    workflow.insert(
        model_id.clone(),
        crate::templates::unet_loader_node(
            trimmed(params.diffusion_model.as_deref()),
            crate::templates::misplaced_model_path(params),
            params,
        ),
    );
    next_id += 1;

    let clip_id = next_id.to_string();
    workflow.insert(clip_id.clone(), text_encoder_loader(params));
    next_id += 1;

    let vae_id = next_id.to_string();
    workflow.insert(
        vae_id.clone(),
        json!({
            "class_type": "VAELoader",
            "inputs": { "vae_name": trimmed(params.vae.as_deref()) }
        }),
    );
    next_id += 1;

    LoaderOutput {
        model: (model_id, 0),
        clip: (clip_id, 0),
        vae: (vae_id, 0),
        next_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(family: &str, t5: Option<&str>, clip_l: Option<&str>) -> GenerationParams {
        let mut value = json!({
            "mode": "txt2img",
            "positive_prompt": "a cat",
            "negative_prompt": "",
            "checkpoint": "flux1-dev.safetensors",
            "loras": [],
            "sampler_name": "euler",
            "scheduler": "simple",
            "steps": 20,
            "cfg": 1.0,
            "seed": "1",
            "width": 1024,
            "height": 1024,
            "batch_size": 1,
            "denoise": 1.0,
            "upscale_enabled": false,
            "upscale_method": "latent",
            "upscale_scale": 2.0,
            "upscale_denoise": 0.5,
            "upscale_steps": 10,
            "upscale_tile_size": 512,
            "upscale_tiling": false,
            "use_split_model": true,
            "diffusion_model": "flux1-dev.safetensors",
            "vae": "ae.safetensors",
            "clip_type": "flux",
            "model_architecture": family,
            "is_sdxl_like": false
        });
        if let Some(t5) = t5 {
            value["clip_model"] = json!(t5);
        }
        if let Some(clip_l) = clip_l {
            value["architecture_files"] = json!({ CLIP_L_SLOT: clip_l });
        }
        serde_json::from_value(value).expect("valid params")
    }

    fn nodes_of<'a>(workflow: &'a Value, class: &str) -> Vec<&'a Value> {
        workflow
            .as_object()
            .unwrap()
            .values()
            .filter(|n| n["class_type"] == class)
            .collect()
    }

    #[test]
    fn every_flux1_family_builds_the_dual_encoder_graph() {
        for family in ["flux", "flux1d", "flux1s", "flux1krea", "flux1kontext"] {
            let p = params(
                family,
                Some("t5xxl_fp8_e4m3fn_scaled.safetensors"),
                Some("clip_l.safetensors"),
            );
            assert_eq!(
                crate::templates::validate_generation_params(&p),
                Ok(()),
                "{family}"
            );
            let workflow = crate::templates::build_workflow(&p, 1, false);
            let dual = nodes_of(&workflow, "DualCLIPLoader");
            assert_eq!(dual.len(), 1, "{family}");
            assert_eq!(dual[0]["inputs"]["type"], "flux");
            assert_eq!(
                dual[0]["inputs"]["clip_name1"],
                "t5xxl_fp8_e4m3fn_scaled.safetensors"
            );
            assert_eq!(dual[0]["inputs"]["clip_name2"], "clip_l.safetensors");
            assert!(nodes_of(&workflow, "CLIPLoader").is_empty(), "{family}");
            assert_eq!(nodes_of(&workflow, "UNETLoader").len(), 1, "{family}");
            assert_eq!(
                nodes_of(&workflow, "VAELoader")[0]["inputs"]["vae_name"],
                "ae.safetensors"
            );
        }
    }

    #[test]
    fn flux_dev_keeps_its_guidance_and_shift() {
        let p = params(
            "flux1d",
            Some("t5xxl_fp16.safetensors"),
            Some("clip_l.safetensors"),
        );
        let workflow = crate::templates::build_workflow(&p, 1, false);
        assert_eq!(nodes_of(&workflow, "FluxGuidance").len(), 1);
        assert_eq!(nodes_of(&workflow, "ModelSamplingFlux").len(), 1);
    }

    #[test]
    fn chroma_and_full_checkpoints_keep_their_loaders() {
        let mut chroma = params("chroma", Some("t5xxl_fp16.safetensors"), None);
        chroma.clip_type = Some("chroma".into());
        let workflow = crate::templates::build_workflow(&chroma, 1, false);
        let clip = nodes_of(&workflow, "CLIPLoader");
        assert_eq!(clip.len(), 1);
        assert_eq!(clip[0]["inputs"]["type"], "chroma");
        assert!(nodes_of(&workflow, "DualCLIPLoader").is_empty());

        let mut checkpoint = params("flux1d", None, None);
        checkpoint.use_split_model = false;
        assert_eq!(
            crate::templates::validate_generation_params(&checkpoint),
            Ok(())
        );
        let workflow = crate::templates::build_workflow(&checkpoint, 1, false);
        assert_eq!(nodes_of(&workflow, "CheckpointLoaderSimple").len(), 1);
        assert!(nodes_of(&workflow, "DualCLIPLoader").is_empty());
    }

    #[test]
    fn a_gguf_encoder_switches_to_the_gguf_dual_loader() {
        let p = params(
            "flux1d",
            Some("t5-v1_1-xxl-encoder-Q8_0.gguf"),
            Some("clip_l.safetensors"),
        );
        let loader = text_encoder_loader(&p);
        assert_eq!(loader["class_type"], "DualCLIPLoaderGGUF");
        assert_eq!(loader["inputs"]["type"], "flux");
    }

    #[test]
    fn a_missing_or_wrong_encoder_is_refused_with_the_download() {
        let no_clip_l = params("flux1d", Some("t5xxl_fp16.safetensors"), None);
        let err = crate::templates::validate_generation_params(&no_clip_l).unwrap_err();
        assert!(err.contains("CLIP-L"), "{err}");

        // CLIP-L picked as the main encoder: T5 is what is missing.
        let clip_as_t5 = params(
            "flux1s",
            Some("clip_l.safetensors"),
            Some("clip_l.safetensors"),
        );
        let err = crate::templates::validate_generation_params(&clip_as_t5).unwrap_err();
        assert!(err.contains(T5XXL_FILE.filename), "{err}");

        // The Chroma type saved by older versions is refreshed on re-select.
        let mut stale = params(
            "flux1krea",
            Some("t5xxl_fp16.safetensors"),
            Some("clip_l.safetensors"),
        );
        stale.clip_type = Some("chroma".into());
        assert!(crate::templates::validate_generation_params(&stale).is_err());
    }

    #[test]
    fn encoders_and_vae_pair_through_the_registry() {
        let encoders: Vec<String> = [
            "clip_l.safetensors",
            "flan_t5_xxl_fp16.safetensors",
            "qwen_3_4b.safetensors",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            super::super::recommended_clip("flux1d", &encoders),
            Some((Some("flan_t5_xxl_fp16.safetensors".to_string()), "flux"))
        );
        // Strict: never CLIP-L or an unrelated encoder in the T5 slot.
        assert_eq!(
            super::super::recommended_clip("flux1s", &encoders[..1]),
            Some((None, "flux"))
        );
        // Chroma stays hand-written.
        assert_eq!(super::super::recommended_clip("chroma", &encoders), None);

        let vaes: Vec<String> = [
            "flux2-vae.safetensors",
            "sdxl_vae.safetensors",
            "FLUX1/ae.safetensors",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            super::super::recommended_vae("flux1krea", &vaes),
            Some(Some("FLUX1/ae.safetensors".to_string()))
        );
        let named: Vec<String> = vec![
            "flux2_vae.safetensors".into(),
            "flux_vae.safetensors".into(),
        ];
        assert_eq!(
            super::super::recommended_vae("flux1d", &named),
            Some(Some("flux_vae.safetensors".to_string()))
        );
    }

    #[test]
    fn kontext_is_the_only_flux1_family_with_image_edit() {
        assert_eq!(
            super::super::supports_mode("flux1kontext", "image_edit"),
            Some(true)
        );
        assert_eq!(
            super::super::supports_mode("flux1d", "image_edit"),
            Some(false)
        );
        assert_eq!(
            super::super::supports_mode("flux1d", "inpainting"),
            Some(true)
        );
    }
}
