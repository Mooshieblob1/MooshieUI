//! Kolors (Kwai-Kolors): an SDXL-shaped UNet conditioned on ChatGLM3-6B.
//!
//! Core ComfyUI loads neither part: the UNet carries a 4096 -> 2048 text
//! projection (`encoder_hid_proj`) and a 5632-wide ADM input, and ChatGLM3 is
//! not a core text encoder. MooshieUI bundles its own loader for both
//! (`comfyui-nodes/kolors_support`, deployed by `comfyui::nodes`), so no third
//! party pack is installed. Its outputs work with core `CLIPTextEncode`,
//! `KSampler` and `LoraLoader`; the VAE is a stock SDXL-style VAE.

use serde_json::{json, Map, Value};

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, LoaderOutput, ModeSpec, ModelFile,
    PromptStyle, RecommendedModel, Sampling, VaeSpec,
};
use crate::comfyui::types::GenerationParams;

/// Pinned revision of the official diffusers release.
macro_rules! kolors_url {
    ($path:literal) => {
        concat!(
            "https://huggingface.co/Kwai-Kolors/Kolors-diffusers/resolve/7e091c75199e910a26cd1b51ed52c28de5db3711/",
            $path
        )
    };
}

/// ChatGLM3-6B ships as three shards. The loader is given any one of them and
/// reads the others from the same folder, so the encoder download is the
/// first shard and every mode fetches the other two by name.
const ENCODER_SHARD_1: ModelFile = ModelFile {
    filename: "kolors_chatglm3.fp16-00001-of-00003.safetensors",
    url: kolors_url!("text_encoder/model.fp16-00001-of-00003.safetensors"),
    category: "text_encoders",
    size: Some("~4.9 GB"),
    sha256: None,
};

static ENCODER_OTHER_SHARDS: &[ModelFile] = &[
    ModelFile {
        filename: "kolors_chatglm3.fp16-00002-of-00003.safetensors",
        url: kolors_url!("text_encoder/model.fp16-00002-of-00003.safetensors"),
        category: "text_encoders",
        size: Some("~4.9 GB"),
        sha256: None,
    },
    ModelFile {
        filename: "kolors_chatglm3.fp16-00003-of-00003.safetensors",
        url: kolors_url!("text_encoder/model.fp16-00003-of-00003.safetensors"),
        category: "text_encoders",
        size: Some("~2.7 GB"),
        sha256: None,
    },
];

const fn mode(mode: &'static str) -> ModeSpec {
    ModeSpec {
        mode,
        max_references: 0,
        required_files: ENCODER_OTHER_SHARDS,
        sampling: None,
        build: None,
    }
}

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "kolors",
    label: "Kolors",
    families: &[FamilySpec {
        family: "kolors",
        label: "Kolors",
        detect: Detect {
            all_of: &[&["kolors"]],
            none_of: &[],
            base_models: &["kolors"],
        },
        turbo: false,
        sampling: Some(Sampling {
            steps: 25,
            cfg: 5.0,
            sampler: "euler",
            scheduler: "normal",
            width: 1024,
            height: 1024,
        }),
        recommended: &[RecommendedModel {
            label: "Kolors",
            size: "~17.8 GB",
            diffusion_model: ModelFile {
                filename: "kolors_unet_fp16.safetensors",
                url: kolors_url!("unet/diffusion_pytorch_model.fp16.safetensors"),
                category: "diffusion_models",
                size: Some("~5.2 GB"),
                sha256: None,
            },
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
    }],
    split_only: true,
    // ChatGLM3 reads Chinese and English sentences.
    prompt_style: PromptStyle::Natural,
    // `infer_architecture_from_key_names` keys on the UNet's text projection.
    tensor_architecture: Some("kolors"),
    text_encoder: Some(EncoderSpec {
        // Informational: MooshieKolorsLoader loads the encoder, not CLIPLoader.
        clip_type: "kolors",
        markers: &["chatglm"],
        strict: true,
        download: Some(ENCODER_SHARD_1),
    }),
    vae: Some(VaeSpec {
        basenames: &["kolors_vae_fp16.safetensors"],
        markers: &["kolors", "sdxl"],
        strict: false,
        download: Some(ModelFile {
            filename: "kolors_vae_fp16.safetensors",
            url: kolors_url!("vae/diffusion_pytorch_model.fp16.safetensors"),
            category: "vae",
            size: Some("~167 MB"),
            sha256: None,
        }),
    }),
    model_slots: &[],
    // Bundled: `MooshieKolorsLoader` is in REQUIRED_MOOSHIE_NODE_CLASSES.
    custom_nodes: &[],
    core_nodes: &[],
    min_comfyui: None,
    int8_fast_model_type: None,
    modes: &[mode("txt2img"), mode("img2img"), mode("inpainting")],
    negative_prompt: true,
    resolution_multiple: 8,
    // SDXL ControlNets read the 2048-wide context; Kolors hands the UNet
    // ChatGLM3's 4096-wide one and projects it inside.
    controlnet: false,
    loader: Some(loader),
    text_encode: None,
};

/// `MooshieKolorsLoader` (MODEL 0, CLIP 1) plus a core `VAELoader`.
fn loader(
    workflow: &mut Map<String, Value>,
    next_id: u32,
    params: &GenerationParams,
) -> LoaderOutput {
    let loader_id = next_id.to_string();
    workflow.insert(
        loader_id.clone(),
        json!({
            "class_type": "MooshieKolorsLoader",
            "inputs": {
                "unet_name": params.diffusion_model.as_deref().unwrap_or(""),
                "text_encoder_name": params.clip_model.as_deref().unwrap_or(""),
                "weight_dtype": "default",
                "clip_device": "default",
                // A Kolors UNet kept in checkpoints/ loads by absolute path.
                "unet_path": crate::templates::misplaced_model_path(params).unwrap_or("")
            }
        }),
    );
    let vae_id = (next_id + 1).to_string();
    workflow.insert(
        vae_id.clone(),
        json!({
            "class_type": "VAELoader",
            "inputs": { "vae_name": params.vae.as_deref().unwrap_or("") }
        }),
    );
    LoaderOutput {
        model: (loader_id.clone(), 0),
        clip: (loader_id, 1),
        vae: (vae_id, 0),
        next_id: next_id + 2,
    }
}
