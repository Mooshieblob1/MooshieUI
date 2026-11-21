//! Flux.2 Klein 4B and 9B (Black Forest Labs), distilled and Base.
//!
//! One spec per size and training, because the two things that differ between
//! them are spec-level: the text encoder size (4B vs 9B) and whether the
//! negative prompt is used (Base runs real CFG, distilled runs at CFG 1).
//! The four share everything else: the `flux2` CLIP type, the 32-channel
//! Flux 2 VAE, the Flux 2 latent, INT8-Fast's `flux2` model type and the
//! multi-reference Image Edit graph ([`crate::templates::image_edit::build_klein`],
//! shared with FLUX 3 Klein), which mirrors ComfyUI's bundled
//! `image_flux2_klein_image_edit_*` templates. The 9B KV checkpoint also runs
//! through core `FluxKVCache` there.
//!
//! Tensor keys tell the sizes apart (`flux2_klein_4b` / `flux2_klein_9b` in
//! `commands::api::infer_architecture_from_key_names`); base and distilled
//! share their keys, so the distilled spec is listed first and wins a purely
//! structural guess, and a filename's base/distilled call stands.

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, ModeSpec, ModelFile, PromptStyle,
    RecommendedModel, Sampling, VaeSpec,
};
use crate::templates::image_edit::{build_klein, KLEIN_MAX_REFERENCES};

/// FLUX 3 Klein filenames carry "flux" and "klein" too; they have their own family.
const NOT_FLUX3: &[&str] = &["flux3", "flux-3", "flux.3", "flux_3", "flux 3"];
const NOT_FLUX3_OR_BASE: &[&str] = &["flux3", "flux-3", "flux.3", "flux_3", "flux 3", "base"];

/// Distilled: Black Forest Labs' 4 steps without guidance, as in ComfyUI's
/// bundled Klein templates.
const DISTILLED_SAMPLING: Sampling = Sampling {
    steps: 4,
    cfg: 1.0,
    sampler: "euler",
    scheduler: "simple",
    width: 1024,
    height: 1024,
};

/// Base: ComfyUI's bundled Klein Base templates, 20 steps at CFG 5.
const BASE_SAMPLING: Sampling = Sampling {
    steps: 20,
    cfg: 5.0,
    sampler: "euler",
    scheduler: "simple",
    width: 1024,
    height: 1024,
};

/// Stock Qwen3 4B (shared with Z-Image), or the language half of Qwen3-VL 4B
/// (Krea 2 / FLUX 3 Klein's encoder): ComfyUI's `flux2` CLIP type builds the
/// 4B Klein encoder from either.
const ENCODER_4B: EncoderSpec = EncoderSpec {
    clip_type: "flux2",
    markers: &[
        "zimage",
        "qwen3-4b",
        "qwen34b",
        "qwen_3_4b",
        "qwen3_4b",
        "qwen3vl-4b",
        "qwen3vl_4b",
        "qwen3-vl-4b",
        "qwen3_vl_4b",
        "qwen3vl4b",
    ],
    strict: true,
    download: Some(ModelFile {
        filename: "qwen_3_4b.safetensors",
        url: "https://huggingface.co/Comfy-Org/z_image_turbo/resolve/main/split_files/text_encoders/qwen_3_4b.safetensors",
        category: "text_encoders",
        size: Some("~8.0 GB"),
        sha256: None,
    }),
};

/// Stock Qwen3 8B (`qwen_3_8b*` from Comfy-Org), or Qwen3-VL 8B (Ideogram 4 /
/// Qwen-Image 2.1's encoder), which the `flux2` CLIP type also accepts.
const ENCODER_9B: EncoderSpec = EncoderSpec {
    clip_type: "flux2",
    markers: &[
        "qwen_3_8b",
        "qwen3_8b",
        "qwen3-8b",
        "qwen38b",
        "qwen3vl-8b",
        "qwen3vl_8b",
        "qwen3-vl-8b",
        "qwen3_vl_8b",
        "qwen3vl8b",
    ],
    strict: true,
    download: Some(ModelFile {
        filename: "qwen_3_8b_fp8mixed.safetensors",
        url: "https://huggingface.co/Comfy-Org/flux2-klein-9B/resolve/main/split_files/text_encoders/qwen_3_8b_fp8mixed.safetensors",
        category: "text_encoders",
        size: Some("~8.7 GB"),
        sha256: None,
    }),
};

/// The 32-channel Flux 2 VAE. Not strict, as before the registry: a renamed
/// copy (`flux-vae.safetensors`) keeps working.
const VAE: VaeSpec = VaeSpec {
    basenames: &["flux2-vae.safetensors"],
    markers: &["flux2-vae", "flux2_vae"],
    strict: false,
    download: Some(ModelFile {
        filename: "flux2-vae.safetensors",
        url: "https://huggingface.co/Comfy-Org/flux2-klein-4B/resolve/main/split_files/vae/flux2-vae.safetensors",
        category: "vae",
        size: Some("~0.34 GB"),
        sha256: None,
    }),
};

const MODES: &[ModeSpec] = &[
    ModeSpec::generic("txt2img"),
    ModeSpec::generic("img2img"),
    ModeSpec::generic("inpainting"),
    ModeSpec::generic("outpainting"),
    ModeSpec {
        mode: "image_edit",
        max_references: KLEIN_MAX_REFERENCES as u32,
        required_files: &[],
        sampling: None,
        build: Some(build_klein),
    },
];

const fn bfl_fp8(filename: &'static str, url: &'static str, size: &'static str) -> ModelFile {
    ModelFile {
        filename,
        url,
        category: "diffusion_models",
        size: Some(size),
        sha256: None,
    }
}

const fn spec(
    id: &'static str,
    label: &'static str,
    families: &'static [FamilySpec],
    tensor_architecture: &'static str,
    text_encoder: EncoderSpec,
    negative_prompt: bool,
) -> ArchitectureSpec {
    ArchitectureSpec {
        id,
        label,
        families,
        split_only: true,
        prompt_style: PromptStyle::Natural,
        tensor_architecture: Some(tensor_architecture),
        text_encoder: Some(text_encoder),
        vae: Some(VAE),
        model_slots: &[],
        custom_nodes: &[],
        core_nodes: &[],
        min_comfyui: None,
        int8_fast_model_type: Some("flux2"),
        modes: MODES,
        negative_prompt,
        resolution_multiple: 16,
        controlnet: true,
        loader: None,
        text_encode: None,
    }
}

pub static SPEC_9B: ArchitectureSpec = spec(
    "flux2_klein_9b",
    "Flux.2 Klein 9B",
    &[FamilySpec {
        family: "flux2klein9b",
        label: "Flux.2 Klein 9B",
        detect: Detect {
            all_of: &[&["flux", "klein", "9b"], &["flux2klein9b"]],
            none_of: NOT_FLUX3_OR_BASE,
            base_models: &["flux.2 klein 9b"],
        },
        turbo: true,
        sampling: Some(DISTILLED_SAMPLING),
        recommended: &[
            // Gated: needs an HF token that has accepted the FLUX Non-Commercial License.
            RecommendedModel {
                label: "Flux.2 Klein 9B (FP8)",
                size: "~18.4 GB",
                diffusion_model: bfl_fp8(
                    "flux-2-klein-9b-fp8.safetensors",
                    "https://huggingface.co/black-forest-labs/FLUX.2-klein-9b-fp8/resolve/main/flux-2-klein-9b-fp8.safetensors",
                    "~9.4 GB",
                ),
                min_compute_capability: None,
            },
            // The KV-cache edit checkpoint: the edit graph adds core FluxKVCache.
            RecommendedModel {
                label: "Flux.2 Klein 9B KV (FP8)",
                size: "~18.8 GB",
                diffusion_model: bfl_fp8(
                    "flux-2-klein-9b-kv-fp8.safetensors",
                    "https://huggingface.co/black-forest-labs/FLUX.2-klein-9b-kv-fp8/resolve/main/flux-2-klein-9b-kv-fp8.safetensors",
                    "~9.8 GB",
                ),
                min_compute_capability: None,
            },
        ],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    "flux2_klein_9b",
    ENCODER_9B,
    false,
);

pub static SPEC_9B_BASE: ArchitectureSpec = spec(
    "flux2_klein_9b_base",
    "Flux.2 Klein 9B Base",
    &[FamilySpec {
        family: "flux2klein9bbase",
        label: "Flux.2 Klein 9B Base",
        detect: Detect {
            all_of: &[&["flux", "klein", "9b", "base"], &["flux2klein9bbase"]],
            none_of: NOT_FLUX3,
            base_models: &["flux.2 klein 9b-base", "flux.2 klein 9b base"],
        },
        turbo: false,
        sampling: Some(BASE_SAMPLING),
        recommended: &[RecommendedModel {
            label: "Flux.2 Klein 9B Base (FP8)",
            size: "~18.6 GB",
            diffusion_model: bfl_fp8(
                "flux-2-klein-base-9b-fp8.safetensors",
                "https://huggingface.co/black-forest-labs/FLUX.2-klein-base-9b-fp8/resolve/main/flux-2-klein-base-9b-fp8.safetensors",
                "~9.6 GB",
            ),
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    "flux2_klein_9b",
    ENCODER_9B,
    true,
);

pub static SPEC_4B: ArchitectureSpec = spec(
    "flux2_klein_4b",
    "Flux.2 Klein 4B",
    &[FamilySpec {
        family: "flux2klein4b",
        label: "Flux.2 Klein 4B",
        detect: Detect {
            all_of: &[&["flux", "klein", "4b"], &["flux2klein4b"]],
            none_of: NOT_FLUX3_OR_BASE,
            base_models: &["flux.2 klein 4b"],
        },
        turbo: true,
        sampling: Some(DISTILLED_SAMPLING),
        // Apache 2.0, ungated.
        recommended: &[RecommendedModel {
            label: "Flux.2 Klein 4B (FP8)",
            size: "~12.5 GB",
            diffusion_model: bfl_fp8(
                "flux-2-klein-4b-fp8.safetensors",
                "https://huggingface.co/black-forest-labs/FLUX.2-klein-4b-fp8/resolve/main/flux-2-klein-4b-fp8.safetensors",
                "~4.1 GB",
            ),
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    "flux2_klein_4b",
    ENCODER_4B,
    false,
);

pub static SPEC_4B_BASE: ArchitectureSpec = spec(
    "flux2_klein_4b_base",
    "Flux.2 Klein 4B Base",
    &[FamilySpec {
        family: "flux2klein4bbase",
        label: "Flux.2 Klein 4B Base",
        detect: Detect {
            all_of: &[&["flux", "klein", "4b", "base"], &["flux2klein4bbase"]],
            none_of: NOT_FLUX3,
            base_models: &["flux.2 klein 4b-base", "flux.2 klein 4b base"],
        },
        turbo: false,
        sampling: Some(BASE_SAMPLING),
        recommended: &[RecommendedModel {
            label: "Flux.2 Klein 4B Base (FP8)",
            size: "~12.5 GB",
            diffusion_model: bfl_fp8(
                "flux-2-klein-base-4b-fp8.safetensors",
                "https://huggingface.co/black-forest-labs/FLUX.2-klein-base-4b-fp8/resolve/main/flux-2-klein-base-4b-fp8.safetensors",
                "~4.1 GB",
            ),
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    "flux2_klein_4b",
    ENCODER_4B,
    true,
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architectures::{
        architecture_class, family_from_tensor_architecture, family_from_text, recommended_clip,
        supports_mode,
    };

    const BFL: &str = "https://huggingface.co/black-forest-labs/";
    const SPECS: [&ArchitectureSpec; 4] = [&SPEC_9B, &SPEC_9B_BASE, &SPEC_4B, &SPEC_4B_BASE];

    #[test]
    fn filenames_and_civitai_names_resolve_to_their_variant() {
        for (text, family) in [
            ("flux-2-klein-4b-fp8.safetensors", "flux2klein4b"),
            ("flux-2-klein-4b.safetensors", "flux2klein4b"),
            ("flux-2-klein-base-4b-fp8.safetensors", "flux2klein4bbase"),
            ("flux-2-klein-9b-fp8.safetensors", "flux2klein9b"),
            ("flux-2-klein-9b-nvfp4.safetensors", "flux2klein9b"),
            ("flux-2-klein-9b-kv-fp8.safetensors", "flux2klein9b"),
            ("flux-2-klein-base-9b-fp8.safetensors", "flux2klein9bbase"),
            ("flux.2 klein 9b", "flux2klein9b"),
            ("flux.2 klein 9b-base", "flux2klein9bbase"),
            ("flux.2 klein 4b", "flux2klein4b"),
            ("flux.2 klein 4b-base", "flux2klein4bbase"),
        ] {
            assert_eq!(family_from_text(text), Some(family), "{text}");
        }
        // FLUX 3 Klein keeps its own families.
        for text in [
            "flux3-image-klein-int8-convrot.safetensors",
            "flux3-image-klein-base-int8-convrot.safetensors",
            "FLUX.3-Image-Klein-Q4_K_M.gguf",
        ] {
            let found = family_from_text(text);
            assert!(
                found.is_none_or(|f| !f.starts_with("flux2klein")),
                "{text} -> {found:?}"
            );
        }
    }

    #[test]
    fn tensor_keys_pick_the_size_and_default_to_distilled() {
        assert_eq!(
            family_from_tensor_architecture("flux2_klein_4b"),
            Some("flux2klein4b")
        );
        assert_eq!(
            family_from_tensor_architecture("flux2_klein_9b"),
            Some("flux2klein9b")
        );
        assert_eq!(
            architecture_class("flux2klein4b"),
            architecture_class("flux2klein4bbase")
        );
        assert_ne!(
            architecture_class("flux2klein4b"),
            architecture_class("flux2klein9b")
        );
    }

    #[test]
    fn each_size_pairs_only_with_an_encoder_of_its_size() {
        let encoders: Vec<String> = [
            "t5xxl_fp16.safetensors",
            "qwen3vl_8b_fp8.safetensors",
            "qwen3vl_4b_fp8_scaled.safetensors",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            recommended_clip("flux2klein4b", &encoders),
            Some((Some("qwen3vl_4b_fp8_scaled.safetensors".into()), "flux2"))
        );
        assert_eq!(
            recommended_clip("flux2klein9bbase", &encoders),
            Some((Some("qwen3vl_8b_fp8.safetensors".into()), "flux2"))
        );
        assert_eq!(
            recommended_clip("flux2klein9b", &encoders[..1]),
            Some((None, "flux2"))
        );
    }

    #[test]
    fn every_variant_edits_with_ten_references_and_base_keeps_the_negative() {
        for arch in SPECS {
            let family = arch.families[0].family;
            assert_eq!(supports_mode(family, "image_edit"), Some(true));
            assert_eq!(arch.mode("image_edit").map(|m| m.max_references), Some(10));
            assert_eq!(arch.negative_prompt, family.ends_with("base"), "{family}");
            for model in arch.families[0].recommended {
                assert!(model.diffusion_model.url.starts_with(BFL));
            }
        }
    }
}
