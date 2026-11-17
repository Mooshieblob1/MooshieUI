//! Qwen-Image 2.1 (Alibaba Qwen, ComfyUI core since v0.37.0).
//!
//! A 7B DiT with a 64-channel, 16x latent, conditioned on the full Qwen3-VL 8B
//! (CLIP type `qwen_image`, which ComfyUI builds into the 2.1 encoder only for
//! a Qwen3-VL 8B file; the Qwen-Image 1 `qwen_2.5_vl` encoder and VAE do not
//! fit). One checkpoint does text-to-image and editing with up to 16
//! references. Prompts go through `TextEncodeQwenImage21` rather than
//! `CLIPTextEncode`. Graphs follow Comfy-Org's bundled templates
//! (`image_qwen_image_2_1_{t2i,turbo_t2i,image_edit,turbo_image_edit}`): base
//! 25 steps at CFG 1 (euler, simple), Turbo 8 steps on a fixed sigma schedule.
//!
//! Weights are under the Qwen Research license (not Apache like Qwen-Image 1).

use serde_json::{json, Map, Value};

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, ModeSpec, ModelFile, PromptStyle,
    RecommendedModel, Sampling, VaeSpec,
};
use crate::comfyui::types::GenerationParams;
use crate::templates::{insert_vae_decode, load_model_nodes, strip_lora_tags, WorkflowResult};

/// `TextEncodeQwenImage21` input `resolution`: references are resized to about
/// this many pixels squared, and an encode without references uses it too.
const ENCODE_RESOLUTION: u32 = 1024;

/// References `TextEncodeQwenImage21` takes (its `images` autogrow has 16 slots).
pub const MAX_REFERENCES: usize = 16;

/// Turbo's distilled schedule, from Comfy-Org's Turbo templates.
const TURBO_SIGMAS: &str =
    "1.0, 0.978453, 0.95418, 0.926626, 0.89508, 0.845148, 0.704534, 0.414568, 0";

const ENCODER: ModelFile = ModelFile {
    filename: "qwen3vl_8b_int8_convrot.safetensors",
    url: "https://huggingface.co/Comfy-Org/Qwen-Image-2.1/resolve/main/text_encoders/qwen3vl_8b_int8_convrot.safetensors",
    category: "text_encoders",
    size: Some("~9.4 GB"),
    sha256: Some("8bfd0f6e12abf2d2d697ecc888e5e90b0d6741d6708f05799f53afa560452e8f"),
};

const VAE: ModelFile = ModelFile {
    filename: "qwen_image_2.1_vae_bf16.safetensors",
    url: "https://huggingface.co/Comfy-Org/Qwen-Image-2.1/resolve/main/vae/qwen_image_2.1_vae_bf16.safetensors",
    category: "vae",
    size: Some("~0.7 GB"),
    sha256: Some("bb21f7473051e1ac368515dd3f2e15cd44d7a11748ee8823e1ddca3e4876b7c9"),
};

/// Spellings of "Qwen-Image 2.1" in Comfy-Org, community and CivitAI names.
const NAME_MARKERS: &[&[&str]] = &[
    &["qwen_image_2.1"],
    &["qwen-image-2.1"],
    &["qwen image 2.1"],
    &["qwen-image 2.1"],
    &["qwenimage2.1"],
    &["qwenimage_2.1"],
    &["qwen_image_21"],
    &["qwen-image-21"],
    &["qwenimage21"],
    &["qwen_image2.1"],
    &["qwen-image2.1"],
];

/// Not a diffusion model: the prompt-enhancer encoders and the 2.1 VAE carry
/// the name too.
const NOT_A_MODEL: &[&str] = &["_pe_", "qwen3.5", "_vae", "-vae", "controlnet", "lora"];

/// The base family excludes Turbo files on top of that.
const NOT_BASE: &[&str] = &[
    "_pe_",
    "qwen3.5",
    "_vae",
    "-vae",
    "controlnet",
    "lora",
    "turbo",
];

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "qwen_image_21",
    label: "Qwen-Image 2.1",
    families: &[
        FamilySpec {
            family: "qwenimage21",
            label: "Qwen-Image 2.1",
            detect: Detect {
                all_of: NAME_MARKERS,
                none_of: NOT_BASE,
                base_models: &[],
            },
            turbo: false,
            sampling: Some(Sampling {
                steps: 25,
                cfg: 1.0,
                sampler: "euler",
                scheduler: "simple",
                width: 1024,
                height: 1024,
            }),
            recommended: &[RecommendedModel {
                label: "Qwen-Image 2.1 (INT8)",
                size: "~17.4 GB",
                diffusion_model: ModelFile {
                    filename: "qwen_image_2.1_int8_convrot.safetensors",
                    url: "https://huggingface.co/Comfy-Org/Qwen-Image-2.1/resolve/main/diffusion_models/qwen_image_2.1_int8_convrot.safetensors",
                    category: "diffusion_models",
                    size: Some("~7.3 GB"),
                    sha256: Some("cb74113cb03faecd79611b01fd7fd642f0aa60d6f0b95086abee214d75eaa57d"),
                },
                min_compute_capability: None,
            }],
            text_encoder: None,
            vae: None,
            manual_sigmas: None,
        },
        FamilySpec {
            family: "qwenimage21turbo",
            label: "Qwen-Image 2.1 Turbo",
            detect: Detect {
                all_of: &[
                    &["qwen_image_2.1", "turbo"],
                    &["qwen-image-2.1", "turbo"],
                    &["qwen image 2.1", "turbo"],
                    &["qwen-image 2.1", "turbo"],
                    &["qwenimage2.1", "turbo"],
                    &["qwen_image_21", "turbo"],
                    &["qwen-image-21", "turbo"],
                    &["qwenimage21", "turbo"],
                ],
                none_of: NOT_A_MODEL,
                base_models: &[],
            },
            turbo: true,
            sampling: Some(Sampling {
                steps: 8,
                cfg: 1.0,
                sampler: "euler",
                scheduler: "simple",
                width: 1024,
                height: 1024,
            }),
            recommended: &[RecommendedModel {
                label: "Qwen-Image 2.1 Turbo (INT8)",
                size: "~17.4 GB",
                diffusion_model: ModelFile {
                    filename: "qwen_image_2.1_turbo_int8_convrot.safetensors",
                    url: "https://huggingface.co/Comfy-Org/Qwen-Image-2.1/resolve/main/diffusion_models/qwen_image_2.1_turbo_int8_convrot.safetensors",
                    category: "diffusion_models",
                    size: Some("~7.3 GB"),
                    sha256: Some("8144a0182964aa8403ad6dbc011db837304167d235161b9aabaa8532e967314c"),
                },
                min_compute_capability: None,
            }],
            text_encoder: None,
            vae: None,
            manual_sigmas: Some(TURBO_SIGMAS),
        },
    ],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("qwen_image21"),
    text_encoder: Some(EncoderSpec {
        clip_type: "qwen_image",
        // The stock Qwen3-VL 8B. Ideogram 4 and Flux.2 Klein 9B load the same
        // weights, so a copy downloaded for either counts.
        markers: &[
            "qwen3vl_8b",
            "qwen3vl-8b",
            "qwen3-vl-8b",
            "qwen3_vl_8b",
            "qwen3vl8b",
        ],
        strict: true,
        download: Some(ENCODER),
    }),
    vae: Some(VaeSpec {
        basenames: &["qwen_image_2.1_vae_bf16.safetensors"],
        markers: &[
            "qwen_image_2.1_vae",
            "qwen-image-2.1-vae",
            "qwen_image_21_vae",
            "qwenimage21_vae",
        ],
        strict: true,
        download: Some(VAE),
    }),
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &["TextEncodeQwenImage21"],
    min_comfyui: Some("0.37.0"),
    // ComfyUI loads the INT8 ConvRot files natively.
    int8_fast_model_type: None,
    modes: &[
        ModeSpec::generic("txt2img"),
        ModeSpec::generic("img2img"),
        ModeSpec::generic("inpainting"),
        ModeSpec {
            mode: "image_edit",
            max_references: 10,
            required_files: &[],
            sampling: None,
            build: Some(build_edit),
        },
    ],
    // Both checkpoints sample at CFG 1, where the negative prompt does nothing.
    negative_prompt: false,
    resolution_multiple: 16,
    loader: None,
    text_encode: Some(text_encode),
};

/// `TextEncodeQwenImage21` in place of `CLIPTextEncode`: the prompt alone, with
/// an empty negative (output 0 is the positive conditioning).
fn text_encode(_params: &GenerationParams, clip: &Value, text: &str) -> Value {
    json!({
        "class_type": "TextEncodeQwenImage21",
        "inputs": {
            "clip": clip,
            "prompt": text,
            "negative_prompt": "",
            "resolution": ENCODE_RESOLUTION
        }
    })
}

/// Non-empty reference images, the edit source (slot 0) first.
fn references(params: &GenerationParams) -> Vec<&str> {
    let mut refs: Vec<&str> = params
        .edit_reference_images
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if refs.is_empty() {
        if let Some(input) = params.input_image.as_deref().map(str::trim) {
            if !input.is_empty() {
                refs.push(input);
            }
        }
    }
    refs.truncate(MAX_REFERENCES);
    refs
}

/// Image Edit: every reference goes into one `TextEncodeQwenImage21` with the
/// VAE, which returns the positive, the negative and an empty latent at the
/// first reference's size (another size shifts the edit).
fn build_edit(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow: Map<String, Value> = Map::new();
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    let mut inputs = Map::new();
    inputs.insert("clip".into(), json!([clip_source.0, clip_source.1]));
    inputs.insert(
        "prompt".into(),
        json!(strip_lora_tags(&params.positive_prompt)),
    );
    inputs.insert(
        "negative_prompt".into(),
        json!(strip_lora_tags(&params.negative_prompt)),
    );
    inputs.insert("vae".into(), json!([vae_source.0, vae_source.1]));
    inputs.insert("resolution".into(), json!(ENCODE_RESOLUTION));
    for (index, image) in references(params).into_iter().enumerate() {
        let load_id = next_id.to_string();
        next_id += 1;
        workflow.insert(
            load_id.clone(),
            json!({"class_type": "LoadImage", "inputs": {"image": image}}),
        );
        inputs.insert(format!("images.image_{}", index + 1), json!([load_id, 0]));
    }
    let encode_id = next_id.to_string();
    next_id += 1;
    workflow.insert(
        encode_id.clone(),
        json!({"class_type": "TextEncodeQwenImage21", "inputs": Value::Object(inputs)}),
    );

    let mut latent = (encode_id.clone(), 2u32);
    if params.batch_size > 1 {
        let repeat_id = next_id.to_string();
        next_id += 1;
        workflow.insert(
            repeat_id.clone(),
            json!({"class_type": "RepeatLatentBatch", "inputs": {
                "samples": [latent.0, latent.1],
                "amount": params.batch_size
            }}),
        );
        latent = (repeat_id, 0);
    }

    let sampler_id = next_id.to_string();
    next_id += 1;
    workflow.insert(
        sampler_id.clone(),
        json!({"class_type": "KSampler", "inputs": {
            "model": [model_source.0, model_source.1],
            "positive": [encode_id, 0],
            "negative": [encode_id, 1],
            "latent_image": [latent.0, latent.1],
            "seed": seed,
            "steps": params.steps,
            "cfg": params.cfg,
            "sampler_name": params.sampler_name,
            "scheduler": params.scheduler,
            "denoise": 1.0
        }}),
    );

    let (decode_id, next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        model_source,
        clip_source,
        positive_source: (encode_id.clone(), 0),
        negative_source: (encode_id, 1),
        vae_source,
        sampler_id,
        refiner_model_source: None,
        base_sources: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architectures;

    fn params(mode: &str, family: &str) -> GenerationParams {
        GenerationParams {
            mode: mode.into(),
            model_architecture: family.into(),
            use_split_model: true,
            diffusion_model: Some("qwen_image_2.1_int8_convrot.safetensors".into()),
            clip_model: Some("qwen3vl_8b_int8_convrot.safetensors".into()),
            clip_type: Some("qwen_image".into()),
            vae: Some("qwen_image_2.1_vae_bf16.safetensors".into()),
            positive_prompt: "a red fox".into(),
            negative_prompt: "blurry".into(),
            sampler_name: "euler".into(),
            scheduler: "simple".into(),
            steps: 25,
            cfg: 1.0,
            width: 1024,
            height: 1024,
            batch_size: 1,
            denoise: 1.0,
            ..Default::default()
        }
    }

    fn nodes<'a>(workflow: &'a Value, class: &str) -> Vec<&'a Value> {
        workflow
            .as_object()
            .unwrap()
            .values()
            .filter(|n| n["class_type"] == class)
            .collect()
    }

    #[test]
    fn detection_tells_qwen_image_21_from_qwen_image_1() {
        for (name, family) in [
            (
                "qwen_image_2.1_int8_convrot.safetensors",
                Some("qwenimage21"),
            ),
            ("qwen_image_2.1_bf16.safetensors", Some("qwenimage21")),
            ("Qwen-Image-2.1-Q8_0.gguf", Some("qwenimage21")),
            (
                "qwen_image_2.1_turbo_int8_convrot.safetensors",
                Some("qwenimage21turbo"),
            ),
            (
                "qwen_image_2.1_turbo_bf16.safetensors",
                Some("qwenimage21turbo"),
            ),
            ("Qwen-Image 2.1", Some("qwenimage21")),
            ("qwen_image_2.1_vae_bf16.safetensors", None),
            (
                "qwen3.5_9b_qwen_image_2.1_pe_t2i.int8_convrot.safetensors",
                None,
            ),
            (
                "qwen_image_2.1_turbo_lora_avg_rank_178_bf16.safetensors",
                None,
            ),
            ("qwen_image_fp8_e4m3fn.safetensors", None),
            ("qwen_image_edit_2511_bf16.safetensors", None),
        ] {
            assert_eq!(architectures::family_from_text(name), family, "{name}");
        }
        // Tensor keys cannot tell Turbo from base: structural detection means base.
        assert_eq!(
            architectures::family_from_tensor_architecture("qwen_image21"),
            Some("qwenimage21")
        );
    }

    #[test]
    fn pairs_only_the_qwen3_vl_8b_encoder_and_the_21_vae() {
        let encoders = vec![
            "qwen_2.5_vl_7b_fp8_scaled.safetensors".to_string(),
            "qwen3vl_8b_w4a8.safetensors".to_string(),
        ];
        assert_eq!(
            architectures::recommended_clip("qwenimage21", &encoders),
            Some((Some("qwen3vl_8b_w4a8.safetensors".into()), "qwen_image"))
        );
        assert_eq!(
            architectures::recommended_clip("qwenimage21", &encoders[..1]),
            Some((None, "qwen_image"))
        );
        let vaes = vec![
            "qwen_image_vae.safetensors".to_string(),
            "qwen_image_2.1_vae_bf16.safetensors".to_string(),
        ];
        assert_eq!(
            architectures::recommended_vae("qwenimage21", &vaes),
            Some(Some("qwen_image_2.1_vae_bf16.safetensors".into()))
        );
        assert_eq!(
            architectures::recommended_vae("qwenimage21", &vaes[..1]),
            Some(None)
        );
    }

    #[test]
    fn validation_refuses_the_qwen_image_1_encoder() {
        let mut p = params("txt2img", "qwenimage21");
        assert!(architectures::validate(&p).is_ok());
        p.clip_model = Some("qwen_2.5_vl_7b_fp8_scaled.safetensors".into());
        let err = architectures::validate(&p).unwrap_err();
        assert!(err.contains("qwen3vl_8b_int8_convrot.safetensors"), "{err}");
    }

    #[test]
    fn txt2img_encodes_with_text_encode_qwen_image_21() {
        let p = params("txt2img", "qwenimage21");
        let workflow = crate::templates::build_workflow(&p, 1, false);
        assert!(nodes(&workflow, "CLIPTextEncode").is_empty());
        let encodes = nodes(&workflow, "TextEncodeQwenImage21");
        assert_eq!(encodes.len(), 2);
        assert!(encodes.iter().any(|n| n["inputs"]["prompt"] == "a red fox"));
        assert!(encodes.iter().all(|n| n["inputs"]["negative_prompt"] == ""));
        let loader = &nodes(&workflow, "CLIPLoader")[0]["inputs"];
        assert_eq!(loader["type"], "qwen_image");
        assert_eq!(nodes(&workflow, "KSampler").len(), 1);
        assert!(nodes(&workflow, "SamplerCustomAdvanced").is_empty());
    }

    #[test]
    fn turbo_samples_on_its_fixed_sigmas() {
        let mut p = params("txt2img", "qwenimage21turbo");
        p.steps = 8;
        let workflow = crate::templates::build_workflow(&p, 1, false);
        assert!(nodes(&workflow, "KSampler").is_empty());
        assert_eq!(nodes(&workflow, "SamplerCustomAdvanced").len(), 1);
        assert_eq!(
            nodes(&workflow, "ManualSigmas")[0]["inputs"]["sigmas"],
            TURBO_SIGMAS
        );
    }

    #[test]
    fn turbo_img2img_keeps_its_denoise_on_ksampler() {
        let mut p = params("img2img", "qwenimage21turbo");
        p.input_image = Some("in.png".into());
        p.denoise = 0.6;
        let workflow = crate::templates::build_workflow(&p, 1, false);
        assert_eq!(nodes(&workflow, "KSampler").len(), 1);
        assert!(nodes(&workflow, "ManualSigmas").is_empty());
    }

    #[test]
    fn edit_feeds_every_reference_and_samples_on_the_encoder_latent() {
        let mut p = params("image_edit", "qwenimage21");
        p.edit_reference_images = vec!["a.png".into(), "".into(), "b.png".into()];
        p.batch_size = 2;
        assert!(crate::templates::validate_generation_params(&{
            let mut v = p.clone();
            v.input_image = Some("a.png".into());
            v
        })
        .is_ok());
        let workflow = crate::templates::build_workflow(&p, 1, false);
        let encodes = nodes(&workflow, "TextEncodeQwenImage21");
        assert_eq!(encodes.len(), 1);
        let encode = &encodes[0]["inputs"];
        assert_eq!(encode["prompt"], "a red fox");
        assert_eq!(encode["negative_prompt"], "blurry");
        assert!(encode.get("images.image_1").is_some());
        assert!(encode.get("images.image_2").is_some());
        assert!(encode.get("images.image_3").is_none());
        assert_eq!(nodes(&workflow, "LoadImage").len(), 2);
        let repeat = &nodes(&workflow, "RepeatLatentBatch")[0]["inputs"];
        assert_eq!(repeat["amount"], 2);
        assert_eq!(repeat["samples"][1], 2);
        let sampler = &nodes(&workflow, "KSampler")[0]["inputs"];
        assert_eq!(sampler["positive"][1], 0);
        assert_eq!(sampler["negative"][1], 1);
    }

    #[test]
    fn image_edit_is_supported_and_counts_references() {
        assert!(crate::templates::image_edit::supports_family("qwenimage21"));
        assert!(crate::templates::image_edit::supports_family(
            "qwenimage21turbo"
        ));
        let mut p = params("image_edit", "qwenimage21");
        p.input_image = Some("a.png".into());
        p.edit_reference_images = (0..11).map(|i| format!("{i}.png")).collect();
        assert!(architectures::validate(&p)
            .unwrap_err()
            .contains("up to 10"));
    }
}
