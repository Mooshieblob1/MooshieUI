//! PixArt Sigma and PixArt Alpha (PixArt-alpha/PixArt-Sigma, openrail++).
//!
//! Both ship as a bare DiT with no text encoder inside, so they always load
//! split: the DiT through `UNETLoader` (or the path loader when the file sits
//! in `checkpoints/`), T5-XXL through `CLIPLoader` type `pixart`, and a 4-channel
//! VAE. ComfyUI loads the original `.pth` keys and the diffusers
//! `transformer/` safetensors alike, and needs no custom nodes.
//!
//! The two differ where it matters for pairing: Sigma uses the SDXL latent
//! space (SDXL VAE), Alpha the SD 1.5 one (SD 1.5 VAE), and Alpha adds a
//! resolution / aspect micro-condition that ComfyUI's PixArt model fills in
//! from the latent size when the prompt node gives none, so the plain
//! `CLIPTextEncode` serves both.
//!
//! Alpha is its own spec, listed before Sigma in `ALL`: its filename markers
//! are the narrower ones, and structural detection tells the two apart by the
//! micro-conditioning weights (`pixart_alpha` vs `pixart`).

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, ModeSpec, ModelFile, PromptStyle,
    RecommendedModel, Sampling, VaeSpec,
};

/// T5 v1.1 XXL as Comfy-Org / comfyanonymous name it (`t5xxl_fp16`,
/// `t5xxl_fp8_e4m3fn`...) and as Google / city96 name it (`t5-v1_1-xxl...`,
/// GGUF included). Deliberately no `t5_xxl` / `t5-xxl`: those also match Wan's
/// `umt5_xxl`, a different model with the same width.
const T5_XXL: EncoderSpec = EncoderSpec {
    clip_type: "pixart",
    markers: &["t5xxl", "t5-v1_1-xxl", "t5_v1_1_xxl", "t5-v1.1-xxl"],
    strict: true,
    download: Some(ModelFile {
        filename: "t5xxl_fp8_e4m3fn.safetensors",
        url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/t5xxl_fp8_e4m3fn.safetensors",
        category: "text_encoders",
        size: Some("~4.9 GB"),
        sha256: Some("7d330da4816157540d6bb7838bf63a0f02f573fc48ca4d8de34bb0cbfd514f09"),
    }),
};

/// Both PixArt models were trained with 20 DPM-Solver steps at CFG 4.5.
const SAMPLING_1024: Sampling = Sampling {
    steps: 20,
    cfg: 4.5,
    sampler: "dpmpp_2m",
    scheduler: "normal",
    width: 1024,
    height: 1024,
};

const MODES: &[ModeSpec] = &[
    ModeSpec::generic("txt2img"),
    ModeSpec::generic("img2img"),
    ModeSpec::generic("inpainting"),
];

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "pixart_sigma",
    label: "PixArt Sigma",
    families: &[FamilySpec {
        // The id PixArt files have always been tagged with, so saved settings
        // and characters keep working.
        family: "pixart",
        label: "PixArt Sigma",
        detect: Detect {
            all_of: &[&["pixart"]],
            none_of: &[],
            base_models: &["pixart e", "pixart σ", "pixart sigma"],
        },
        turbo: false,
        sampling: Some(SAMPLING_1024),
        recommended: &[RecommendedModel {
            label: "PixArt Sigma 1024",
            size: "~7.7 GB",
            diffusion_model: ModelFile {
                filename: "PixArt-Sigma-XL-2-1024-MS.safetensors",
                url: "https://huggingface.co/PixArt-alpha/PixArt-Sigma-XL-2-1024-MS/resolve/main/transformer/diffusion_pytorch_model.safetensors",
                category: "diffusion_models",
                size: Some("~2.4 GB"),
                sha256: Some("2fa4dee9229c02b03163f57bdb8e80c7a5ee364b7161796abe9c05e8dd13f239"),
            },
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("pixart"),
    text_encoder: Some(T5_XXL),
    vae: Some(VaeSpec {
        basenames: &["sdxl_vae.safetensors"],
        markers: &["sdxl"],
        strict: true,
        download: Some(ModelFile {
            filename: "sdxl_vae.safetensors",
            url: "https://huggingface.co/stabilityai/sdxl-vae/resolve/main/sdxl_vae.safetensors",
            category: "vae",
            size: Some("~335 MB"),
            sha256: Some("63aeecb90ff7bc1c115395962d3e803571385b61938377bc7089b36e81e92e2e"),
        }),
    }),
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &[],
    min_comfyui: None,
    int8_fast_model_type: None,
    modes: MODES,
    negative_prompt: true,
    // Patch size 2 on an 8x VAE.
    resolution_multiple: 16,
    loader: None,
    text_encode: None,
};

pub static ALPHA_SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "pixart_alpha",
    label: "PixArt Alpha",
    families: &[FamilySpec {
        family: "pixartalpha",
        label: "PixArt Alpha",
        detect: Detect {
            // PixArt-alpha's own files are `PixArt-XL-2-1024-MS` /
            // `PixArt-XL-2-512x512`; Sigma's add `Sigma` before `XL`.
            all_of: &[
                &["pixart", "alpha"],
                &["pixart", "α"],
                &["pixart-xl-2"],
                &["pixart_xl_2"],
            ],
            none_of: &["sigma", "σ"],
            base_models: &["pixart a", "pixart α", "pixart alpha"],
        },
        turbo: false,
        sampling: Some(SAMPLING_1024),
        recommended: &[RecommendedModel {
            label: "PixArt Alpha 1024",
            size: "~7.7 GB",
            diffusion_model: ModelFile {
                filename: "PixArt-XL-2-1024-MS.safetensors",
                url: "https://huggingface.co/PixArt-alpha/PixArt-XL-2-1024-MS/resolve/main/transformer/diffusion_pytorch_model.safetensors",
                category: "diffusion_models",
                size: Some("~2.4 GB"),
                sha256: Some("809a92d52a4a228f381a4b4f4b76051294b73285fb0cbb02f0ad24f9372217a8"),
            },
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("pixart_alpha"),
    text_encoder: Some(T5_XXL),
    vae: Some(VaeSpec {
        basenames: &[
            "vae-ft-ema-560000-ema-pruned.safetensors",
            "vae-ft-mse-840000-ema-pruned.safetensors",
        ],
        markers: &["vae-ft-ema", "vae-ft-mse", "sd15", "sd1.5", "sd-vae"],
        strict: true,
        download: Some(ModelFile {
            filename: "vae-ft-ema-560000-ema-pruned.safetensors",
            url: "https://huggingface.co/stabilityai/sd-vae-ft-ema-original/resolve/main/vae-ft-ema-560000-ema-pruned.safetensors",
            category: "vae",
            size: Some("~335 MB"),
            sha256: Some("95f26a5ab04779d5467d1fcecaf93160ffa523afe399b835b3e1bb77ff2d937a"),
        }),
    }),
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &[],
    min_comfyui: None,
    int8_fast_model_type: None,
    modes: MODES,
    negative_prompt: true,
    resolution_multiple: 16,
    loader: None,
    text_encode: None,
};

#[cfg(test)]
mod tests {
    use crate::architectures;

    fn list(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn filenames_and_base_models_pick_sigma_or_alpha() {
        for (name, family) in [
            ("PixArt-Sigma-XL-2-1024-MS.pth", "pixart"),
            ("PixArt-Sigma-XL-2-2K-MS.safetensors", "pixart"),
            ("pixart_sigma_900m_v2.safetensors", "pixart"),
            ("PixArt-XL-2-1024-MS.pth", "pixartalpha"),
            ("PixArt-XL-2-512x512.safetensors", "pixartalpha"),
            ("pixart_alpha_anime.safetensors", "pixartalpha"),
            ("PixArt Sigma", "pixart"),
            ("PixArt E", "pixart"),
            ("PixArt a", "pixartalpha"),
        ] {
            assert_eq!(
                architectures::family_from_text(name),
                Some(family),
                "{name}"
            );
        }
        // Not PixArt at all.
        assert_eq!(
            architectures::family_from_text("sd_xl_base_1.0.safetensors"),
            None
        );
    }

    #[test]
    fn structural_detection_resolves_each_variant() {
        assert_eq!(
            architectures::family_from_tensor_architecture("pixart"),
            Some("pixart")
        );
        assert_eq!(
            architectures::family_from_tensor_architecture("pixart_alpha"),
            Some("pixartalpha")
        );
        assert_ne!(
            architectures::architecture_class("pixart"),
            architectures::architecture_class("pixartalpha")
        );
    }

    #[test]
    fn always_loads_split_with_t5_xxl_as_pixart() {
        for family in ["pixart", "pixartalpha"] {
            assert_eq!(architectures::requires_separate_clip(family), Some(true));
            let encoders = list(&[
                "umt5_xxl_fp8_e4m3fn_scaled.safetensors",
                "qwen_3_4b.safetensors",
                "t5xxl_fp16.safetensors",
            ]);
            assert_eq!(
                architectures::recommended_clip(family, &encoders),
                Some((Some("t5xxl_fp16.safetensors".to_string()), "pixart")),
                "{family}"
            );
            // Wan's UMT5 is never paired, even when it is the only XXL encoder.
            assert_eq!(
                architectures::recommended_clip(family, &encoders[..2]),
                Some((None, "pixart")),
                "{family}"
            );
        }
        assert_eq!(
            architectures::recommended_clip("pixart", &list(&["t5-v1_1-xxl-encoder-Q8_0.gguf"])),
            Some((Some("t5-v1_1-xxl-encoder-Q8_0.gguf".to_string()), "pixart"))
        );
    }

    #[test]
    fn sigma_decodes_with_the_sdxl_vae_and_alpha_with_the_sd15_one() {
        let vaes = list(&[
            "ae.safetensors",
            "vae-ft-mse-840000-ema-pruned.safetensors",
            "sdxl_vae.safetensors",
        ]);
        assert_eq!(
            architectures::recommended_vae("pixart", &vaes),
            Some(Some("sdxl_vae.safetensors".to_string()))
        );
        assert_eq!(
            architectures::recommended_vae("pixartalpha", &vaes),
            Some(Some("vae-ft-mse-840000-ema-pruned.safetensors".to_string()))
        );
        // Strict: the 16-channel Flux VAE is never substituted.
        assert_eq!(
            architectures::recommended_vae("pixart", &vaes[..1]),
            Some(None)
        );
    }

    fn split_params(
        family: &str,
        clip: &str,
        clip_type: &str,
    ) -> crate::comfyui::types::GenerationParams {
        let mut params = crate::templates::graph_test_util::params("txt2img", family);
        params.use_split_model = true;
        params.diffusion_model = Some("PixArt-Sigma-XL-2-1024-MS.safetensors".to_string());
        params.clip_model = Some(clip.to_string());
        params.clip_type = Some(clip_type.to_string());
        params.vae = Some("sdxl_vae.safetensors".to_string());
        params
    }

    #[test]
    fn workflow_loads_the_dit_t5_and_vae_separately() {
        use crate::templates::graph_test_util::{build, linked, single};
        let params = split_params("pixart", "t5xxl_fp16.safetensors", "pixart");
        crate::templates::validate_generation_params(&params).expect("valid");
        let workflow = build(&params);
        assert!(
            crate::templates::graph_test_util::nodes(&workflow, "CheckpointLoaderSimple")
                .is_empty()
        );
        let unet = single(&workflow, "UNETLoader");
        assert_eq!(
            unet["inputs"]["unet_name"],
            "PixArt-Sigma-XL-2-1024-MS.safetensors"
        );
        let clip = single(&workflow, "CLIPLoader");
        assert_eq!(clip["inputs"]["type"], "pixart");
        assert_eq!(clip["inputs"]["clip_name"], "t5xxl_fp16.safetensors");
        let ksampler = single(&workflow, "KSampler");
        let positive = linked(&workflow, &ksampler["inputs"]["positive"]);
        assert_eq!(positive["class_type"], "CLIPTextEncode");
    }

    #[test]
    fn generation_refuses_a_wrong_encoder_or_type() {
        let wrong_type = split_params("pixart", "t5xxl_fp16.safetensors", "wan");
        assert!(crate::templates::validate_generation_params(&wrong_type).is_err());
        let umt5 = split_params("pixart", "umt5_xxl_fp16.safetensors", "pixart");
        assert!(crate::templates::validate_generation_params(&umt5).is_err());
        let mut edit = split_params("pixart", "t5xxl_fp16.safetensors", "pixart");
        edit.mode = "image_edit".to_string();
        assert!(crate::templates::validate_generation_params(&edit).is_err());
    }
}
