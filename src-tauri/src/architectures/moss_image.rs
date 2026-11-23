//! MOSS-Image (OpenMOSS): SFT and Base, Comfy-Org's repackaged files.
//!
//! Qwen3.5 9B text encoder under ComfyUI's `moss_image` CLIP type (nothing else
//! builds the MOSS encoder), the Flux 2 VAE, ComfyUI 0.43.0 or newer. txt2img,
//! img2img and inpainting use the generic templates (the Flux 2 latent and the
//! Moody Moss Mix two-pass live there); Image Edit has its own graph, below.
//!
//! Image Edit follows Comfy-Org's `image_moss_image_edit` template: the
//! MOSS-Image Edit LoRA (chinoll/Moss-Image-Edit, rank 64) on the model, Skip
//! Layer Guidance on single layer 22, and core `TextEncodeMOSSImageEdit`
//! folding up to four references into both conditionings as reference latents.

use serde_json::json;

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, ModeSpec, ModelFile, PromptStyle,
    RecommendedModel, Sampling, VaeSpec,
};
use crate::comfyui::types::GenerationParams;
use crate::templates::{
    build_scheduled_conditioning, insert_vae_decode, load_model_nodes, strip_lora_tags,
    WorkflowResult,
};

/// Text-encoder filename markers for Qwen3.5 9B (`qwen3.5_9b_*` from Comfy-Org).
const ENCODER_MARKERS: &[&str] = &[
    "qwen3.5_9b",
    "qwen3.5-9b",
    "qwen35_9b",
    "qwen3_5_9b",
    "qwen3.5 9b",
];

/// Filename / CivitAI base-model markers. Moody Moss Mix (a MOSS-Image SFT
/// fine-tune) names its files `Moody-Moss-Mix-V4.1_...`.
const MARKER_GROUPS: &[&[&str]] = &[
    &["moss_image"],
    &["moss-image"],
    &["moss image"],
    &["mossimage"],
    &["moss.image"],
    &["moody", "moss"],
];

/// The same markers with "base": only that selects the pretraining checkpoint
/// (tensor keys cannot tell SFT and Base apart).
const BASE_MARKER_GROUPS: &[&[&str]] = &[
    &["moss_image", "base"],
    &["moss-image", "base"],
    &["moss image", "base"],
    &["mossimage", "base"],
    &["moss.image", "base"],
    &["moody", "moss", "base"],
];

/// Comfy-Org's bundled templates: 25 steps at CFG 2.5, er_sde on simple.
/// Moody Moss Mix overrides this by filename in the generation store.
const SAMPLING: Sampling = Sampling {
    steps: 25,
    cfg: 2.5,
    sampler: "er_sde",
    scheduler: "simple",
    width: 1024,
    height: 1024,
};

/// What the edit LoRA author ships (and Comfy-Org's edit template uses).
const EDIT_SAMPLING: Sampling = Sampling {
    steps: 30,
    cfg: 4.0,
    sampler: "euler",
    scheduler: "normal",
    width: 1024,
    height: 1024,
};

/// Most references the edit LoRA was trained on.
const MAX_REFERENCES: usize = 4;

/// The edit graph loads it by this exact name from `models/loras/`.
pub const EDIT_LORA: ModelFile = ModelFile {
    filename: "moss_image_edit_lora_rank_64_bf16.safetensors",
    url: "https://huggingface.co/Comfy-Org/MOSS-Image/resolve/main/loras/moss_image_edit_lora_rank_64_bf16.safetensors",
    category: "loras",
    size: Some("~297 MB"),
    sha256: None,
};

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "moss_image",
    label: "MOSS-Image",
    // SFT first: structural detection (`moss_image` tensors) resolves to it.
    families: &[
        FamilySpec {
            family: "mossimage",
            label: "MOSS-Image",
            detect: Detect {
                all_of: MARKER_GROUPS,
                none_of: &["base"],
                base_models: &[],
            },
            turbo: false,
            manual_sigmas: None,
            sampling: Some(SAMPLING),
            recommended: &[RecommendedModel {
                label: "MOSS-Image (INT8)",
                size: "~13 GB",
                diffusion_model: ModelFile {
                    filename: "moss_image_sft_int8_convrot.safetensors",
                    url: "https://huggingface.co/Comfy-Org/MOSS-Image/resolve/main/diffusion_models/moss_image_sft_int8_convrot.safetensors",
                    category: "diffusion_models",
                    size: Some("~5.4 GB"),
                    sha256: None,
                },
                min_compute_capability: None,
            }],
            text_encoder: None,
            vae: None,
        },
        FamilySpec {
            family: "mossimagebase",
            label: "MOSS-Image Base",
            detect: Detect {
                all_of: BASE_MARKER_GROUPS,
                none_of: &[],
                base_models: &[],
            },
            turbo: false,
            manual_sigmas: None,
            sampling: Some(SAMPLING),
            recommended: &[RecommendedModel {
                label: "MOSS-Image Base (INT8)",
                size: "~13 GB",
                diffusion_model: ModelFile {
                    filename: "moss_image_base_int8_convrot.safetensors",
                    url: "https://huggingface.co/Comfy-Org/MOSS-Image/resolve/main/diffusion_models/moss_image_base_int8_convrot.safetensors",
                    category: "diffusion_models",
                    size: Some("~5.4 GB"),
                    sha256: None,
                },
                min_compute_capability: None,
            }],
            text_encoder: None,
            vae: None,
        },
    ],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("moss_image"),
    text_encoder: Some(EncoderSpec {
        clip_type: "moss_image",
        markers: ENCODER_MARKERS,
        strict: true,
        download: Some(ModelFile {
            filename: "qwen3.5_9b_w4a8_gptq.safetensors",
            url: "https://huggingface.co/Comfy-Org/MOSS-Image/resolve/main/text_encoders/qwen3.5_9b_w4a8_gptq.safetensors",
            category: "text_encoders",
            size: Some("~7.3 GB"),
            sha256: None,
        }),
    }),
    vae: Some(VaeSpec {
        basenames: &["flux2-vae_small.safetensors", "flux2-vae.safetensors"],
        markers: &["flux2-vae", "flux2_vae"],
        strict: false,
        download: Some(ModelFile {
            filename: "flux2-vae_small.safetensors",
            url: "https://huggingface.co/Comfy-Org/MOSS-Image/resolve/main/vae/flux2-vae_small.safetensors",
            category: "vae",
            size: Some("~250 MB"),
            sha256: None,
        }),
    }),
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &["TextEncodeMOSSImageEdit"],
    min_comfyui: Some("0.43.0"),
    // ComfyUI loads the INT8 ConvRot files natively.
    int8_fast_model_type: None,
    modes: &[
        ModeSpec::generic("txt2img"),
        ModeSpec::generic("img2img"),
        ModeSpec::generic("inpainting"),
        ModeSpec::generic("outpainting"),
        ModeSpec {
            mode: "image_edit",
            max_references: MAX_REFERENCES as u32,
            required_files: &[EDIT_LORA],
            sampling: Some(EDIT_SAMPLING),
            build: Some(build_edit),
        },
    ],
    negative_prompt: true,
    resolution_multiple: 16,
    // No ControlNet has been released for MOSS-Image, so a request with one on
    // gets a clear refusal instead of a ComfyUI shape error.
    controlnet: false,
    loader: None,
    text_encode: None,
};

/// `edit_format` of the MOSS-Image Edit LoRA: references go in as reference
/// latents only, and the text encoder never sees them.
const MOSS_EDIT_FORMAT: &str = "Moss-Image-Edit";

/// MOSS-Image Edit path (Comfy-Org's `image_moss_image_edit` template).
///
/// The edit LoRA rides on the user's model and LoRA chain, followed by Skip
/// Layer Guidance on single layer 22 (what the LoRA author ships). References
/// enter the diffusion model at exactly the size they arrive, and the template
/// samples the output at the first reference's size, so reference 1 is scaled
/// to the output size and the others to one megapixel (16 px steps). Both
/// conditionings come from `TextEncodeMOSSImageEdit` with the same references,
/// as the template does: the negative describes what to avoid, not what to
/// remove from the scene.
fn build_edit(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    let lora_id = next_id.to_string();
    workflow.insert(
        lora_id.clone(),
        json!({
            "class_type": "LoraLoaderModelOnly",
            "inputs": {
                "model": [ml.model_source.0.clone(), ml.model_source.1],
                "lora_name": EDIT_LORA.filename,
                "strength_model": 1.0
            }
        }),
    );
    next_id += 1;

    let slg_id = next_id.to_string();
    workflow.insert(
        slg_id.clone(),
        json!({
            "class_type": "SkipLayerGuidanceDiTSimple",
            "inputs": {
                "model": [lora_id, 0],
                "double_layers": "",
                "single_layers": "22",
                "start_percent": 0.0,
                "end_percent": 1.0
            }
        }),
    );
    next_id += 1;
    let model_source = (slg_id, 0u32);

    let mut refs = crate::templates::image_edit::reference_images(params);
    if refs.is_empty() {
        // Validation should prevent this; keep the workflow well-formed regardless.
        refs.push("");
    }
    refs.truncate(MAX_REFERENCES);

    let mut scaled_ids: Vec<String> = Vec::with_capacity(refs.len());
    for (idx, image_name) in refs.iter().enumerate() {
        let load_id = next_id.to_string();
        workflow.insert(
            load_id.clone(),
            json!({
                "class_type": "LoadImage",
                "inputs": { "image": image_name }
            }),
        );
        next_id += 1;

        let scale_id = next_id.to_string();
        let scale = if idx == 0 {
            json!({
                "class_type": "ImageScale",
                "inputs": {
                    "image": [load_id, 0],
                    "width": params.width,
                    "height": params.height,
                    "upscale_method": "lanczos",
                    "crop": "center"
                }
            })
        } else {
            json!({
                "class_type": "ImageScaleToTotalPixels",
                "inputs": {
                    "image": [load_id, 0],
                    "upscale_method": "lanczos",
                    "megapixels": 1.0,
                    "resolution_steps": 16
                }
            })
        };
        workflow.insert(scale_id.clone(), scale);
        next_id += 1;
        scaled_ids.push(scale_id);
    }

    let mut encode = |next_id: &mut u32, prompt: &str| -> (String, u32) {
        let mut inputs = serde_json::Map::new();
        inputs.insert("clip".into(), json!([clip_source.0.clone(), clip_source.1]));
        inputs.insert("prompt".into(), json!(strip_lora_tags(prompt)));
        inputs.insert("vae".into(), json!([vae_source.0.clone(), vae_source.1]));
        inputs.insert("edit_format".into(), json!(MOSS_EDIT_FORMAT));
        inputs.insert("image_strength".into(), json!(1.0));
        // Autogrow inputs travel as dotted keys in API prompts.
        for (idx, scaled_id) in scaled_ids.iter().enumerate() {
            inputs.insert(format!("images.image_{idx}"), json!([scaled_id.clone(), 0]));
        }
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({ "class_type": "TextEncodeMOSSImageEdit", "inputs": inputs }),
        );
        *next_id += 1;
        (id, 0)
    };
    let pos_source = encode(&mut next_id, &params.positive_prompt);
    let neg_source = encode(&mut next_id, &params.negative_prompt);

    // The refinement chains (upscale, face fix) run on the plain model without
    // the edit LoRA, at other sizes; references would only pull them off the
    // edit. They get plain text conditioning, which ComfyUI never runs unless a
    // chain is wired to it.
    let (refiner_positive, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.positive_prompt,
        &[],
    );
    next_id = nid;
    let (refiner_negative, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.negative_prompt,
        &[],
    );
    next_id = nid;

    let latent_id = next_id.to_string();
    workflow.insert(
        latent_id.clone(),
        json!({
            "class_type": "EmptyFlux2LatentImage",
            "inputs": {
                "width": params.width,
                "height": params.height,
                "batch_size": params.batch_size
            }
        }),
    );
    next_id += 1;

    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [latent_id, 0],
                "seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": 1.0
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
        model_source,
        clip_source,
        positive_source: refiner_positive,
        negative_source: refiner_negative,
        vae_source,
        sampler_id,
        refiner_model_source: Some(ml.model_source.clone()),
        base_sources: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes_of_type<'a>(
        result: &'a WorkflowResult,
        class_type: &str,
    ) -> Vec<&'a serde_json::Value> {
        result
            .workflow
            .values()
            .filter(|n| n["class_type"] == class_type)
            .collect()
    }

    fn moss_params(family: &str, refs: usize) -> GenerationParams {
        let mut p: GenerationParams = serde_json::from_value(json!({
            "mode": "image_edit",
            "model_architecture": family,
            "positive_prompt": "",
            "negative_prompt": "",
            "checkpoint": "",
            "loras": [],
            "sampler_name": "euler",
            "scheduler": "normal",
            "steps": 30,
            "cfg": 4.0,
            "seed": "42",
            "width": 1024,
            "height": 1024,
            "batch_size": 1,
            "denoise": 1.0,
            "upscale_enabled": false,
            "upscale_method": "algorithmic",
            "upscale_scale": 2.0,
            "upscale_denoise": 0.4,
            "upscale_steps": 20,
            "upscale_tile_size": 1024,
            "upscale_tiling": false
        }))
        .expect("params");
        p.use_split_model = true;
        p.diffusion_model = Some("moss_image_sft_int8_convrot.safetensors".into());
        p.clip_model = Some("qwen3.5_9b_w4a8_gptq.safetensors".into());
        p.clip_type = Some("moss_image".into());
        p.vae = Some("flux2-vae_small.safetensors".into());
        p.positive_prompt = "Change only the jacket to a black wool coat.".into();
        p.negative_prompt = "nsfw, worst quality".into();
        p.edit_reference_images = (0..refs).map(|i| format!("ref{i}.png")).collect();
        p.width = 1216;
        p.height = 832;
        p.sampler_name = "euler".into();
        p.scheduler = "normal".into();
        p
    }

    fn input_node<'a>(
        result: &'a WorkflowResult,
        link: &serde_json::Value,
    ) -> &'a serde_json::Value {
        &result.workflow[link[0].as_str().unwrap()]
    }

    #[test]
    fn moss_edit_follows_the_comfy_template() {
        for family in ["mossimage", "mossimagebase"] {
            assert!(crate::templates::image_edit::supports_family(family));
            let result = build_edit(&moss_params(family, 2), 7);

            // Sampler model: SLG on single layer 22 over the edit LoRA.
            let sampler = &result.workflow[&result.sampler_id]["inputs"];
            let slg = input_node(&result, &sampler["model"]);
            assert_eq!(slg["class_type"], "SkipLayerGuidanceDiTSimple");
            assert_eq!(slg["inputs"]["single_layers"], "22");
            assert_eq!(slg["inputs"]["double_layers"], "");
            let lora = input_node(&result, &slg["inputs"]["model"]);
            assert_eq!(lora["class_type"], "LoraLoaderModelOnly");
            assert_eq!(lora["inputs"]["lora_name"], EDIT_LORA.filename);
            assert_eq!(lora["inputs"]["strength_model"], 1.0);

            // Both conditionings: the edit encoder with every reference.
            for (key, prompt) in [
                ("positive", "Change only the jacket to a black wool coat."),
                ("negative", "nsfw, worst quality"),
            ] {
                let enc = input_node(&result, &sampler[key]);
                assert_eq!(enc["class_type"], "TextEncodeMOSSImageEdit");
                assert_eq!(enc["inputs"]["edit_format"], "Moss-Image-Edit");
                assert_eq!(enc["inputs"]["prompt"], prompt);
                assert!(enc["inputs"]["vae"].is_array());
                let first = input_node(&result, &enc["inputs"]["images.image_0"]);
                assert_eq!(first["class_type"], "ImageScale");
                assert_eq!(first["inputs"]["width"], 1216);
                assert_eq!(first["inputs"]["height"], 832);
                let second = input_node(&result, &enc["inputs"]["images.image_1"]);
                assert_eq!(second["class_type"], "ImageScaleToTotalPixels");
                assert_eq!(second["inputs"]["megapixels"], 1.0);
                assert_eq!(second["inputs"]["resolution_steps"], 16);
                assert_eq!(
                    input_node(&result, &second["inputs"]["image"])["inputs"]["image"],
                    "ref1.png"
                );
                assert!(enc["inputs"].get("images.image_2").is_none());
            }

            let latent = input_node(&result, &sampler["latent_image"]);
            assert_eq!(latent["class_type"], "EmptyFlux2LatentImage");
            assert_eq!(latent["inputs"]["width"], 1216);
            assert_eq!(latent["inputs"]["height"], 832);
            assert_eq!(sampler["denoise"], 1.0);
        }
    }

    #[test]
    fn moss_edit_caps_references_and_keeps_refiners_off_the_edit() {
        let result = build_edit(&moss_params("mossimage", 6), 7);
        assert_eq!(nodes_of_type(&result, "LoadImage").len(), MAX_REFERENCES);

        // Upscale and face fix get the plain model and plain text conditioning:
        // no edit LoRA, no references.
        let refiner = result.refiner_model();
        let refiner_node = &result.workflow[&refiner.0];
        assert_ne!(refiner_node["class_type"], "SkipLayerGuidanceDiTSimple");
        assert_ne!(refiner_node["inputs"]["lora_name"], EDIT_LORA.filename);
        for source in [&result.positive_source, &result.negative_source] {
            assert_ne!(
                result.workflow[&source.0]["class_type"],
                "TextEncodeMOSSImageEdit"
            );
        }
    }
}
