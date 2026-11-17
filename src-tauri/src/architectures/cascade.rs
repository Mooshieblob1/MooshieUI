//! Stable Cascade (Würstchen v3, Stability AI, ComfyUI core).
//!
//! The model the user picks is stage C, which samples a small semantic
//! latent; stage B then draws the full-size image from it and stage A
//! decodes. The two-stage graphs live in `templates::cascade`.
//!
//! Stage B is chosen by `src/lib/utils/cascade.ts` rather than through
//! [`ModelSlot`](super::ModelSlot)s: it is needed for checkpoint stage C files
//! too (the generic slot picker only runs for split models), and it is either
//! one all-in-one checkpoint or a diffusion model plus stage A, which per-slot
//! picking cannot express. `templates::cascade::validate` checks it.

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, ModeSpec, ModelFile, PromptStyle,
    RecommendedModel, Sampling, VaeSpec,
};
use crate::templates::cascade::{build_img2img, build_txt2img};

/// Stage C's CLIP-G text encoder, saved under a name that says what it is
/// (the repo calls it `text_encoder/model.safetensors`). Mirrors
/// `CASCADE_TEXT_ENCODER` in `src/lib/utils/cascade.ts`.
const TEXT_ENCODER: ModelFile = ModelFile {
    filename: "stable_cascade_text_encoder.safetensors",
    url: "https://huggingface.co/stabilityai/stable-cascade/resolve/main/text_encoder/model.safetensors",
    category: "text_encoders",
    size: Some("~1.4 GB"),
    sha256: None,
};

/// The EfficientNet encoder: stage C's "VAE", which img2img encodes with.
const EFFNET: ModelFile = ModelFile {
    filename: "effnet_encoder.safetensors",
    url:
        "https://huggingface.co/stabilityai/stable-cascade/resolve/main/effnet_encoder.safetensors",
    category: "vae",
    size: Some("~82 MB"),
    sha256: Some("e23c1fe17ba8cdd6c49a5b292d72502bd4cf729e7de9f2060bb4dcc26737c0a0"),
};

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "stable_cascade",
    label: "Stable Cascade",
    families: &[FamilySpec {
        family: "cascade",
        label: "Stable Cascade",
        detect: Detect {
            all_of: &[&["cascade"]],
            none_of: &[],
            base_models: &["stable cascade"],
        },
        turbo: false,
        // The official workflow's stage C settings. Stage B has its own.
        sampling: Some(Sampling {
            steps: 20,
            cfg: 4.0,
            sampler: "euler_ancestral",
            scheduler: "simple",
            width: 1024,
            height: 1024,
        }),
        // Split entry; the full stage C ships as a checkpoint, offered by the
        // hand-written "Stable Cascade" entry in ModelSelector.svelte.
        recommended: &[RecommendedModel {
            label: "Stable Cascade Lite",
            size: "~7.9 GB",
            diffusion_model: ModelFile {
                filename: "stable_cascade_stage_c_lite_bf16.safetensors",
                url: "https://huggingface.co/stabilityai/stable-cascade/resolve/main/stage_c_lite_bf16.safetensors",
                category: "diffusion_models",
                size: Some("~2.1 GB"),
                sha256: None,
            },
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    split_only: false,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("stable_cascade"),
    text_encoder: Some(EncoderSpec {
        clip_type: "stable_cascade",
        // SDXL's clip_g is the same CLIP-G.
        markers: &["cascade", "clip_g"],
        strict: true,
        download: Some(TEXT_ENCODER),
    }),
    vae: Some(VaeSpec {
        basenames: &["effnet_encoder.safetensors"],
        markers: &["effnet"],
        strict: true,
        download: Some(EFFNET),
    }),
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &[
        "StableCascade_EmptyLatentImage",
        "StableCascade_StageB_Conditioning",
        "StableCascade_StageC_VAEEncode",
    ],
    min_comfyui: None,
    int8_fast_model_type: None,
    modes: &[
        ModeSpec {
            build: Some(build_txt2img),
            ..ModeSpec::generic("txt2img")
        },
        ModeSpec {
            build: Some(build_img2img),
            ..ModeSpec::generic("img2img")
        },
    ],
    negative_prompt: true,
    resolution_multiple: 8,
    loader: None,
    text_encode: None,
};
