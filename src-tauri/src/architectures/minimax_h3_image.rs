//! MiniMax-H3-Image (MiniMax, ComfyUI core since v0.45.0).
//!
//! The 33.1B MiniMax H3 DiT without its audio stream, distilled to 6 steps at
//! CFG 1 on its own sigma grid (ComfyUI's stock schedulers land on it at 6
//! steps). Text-to-image and editing with up to 16 references are one
//! checkpoint. It shares the MiniMax H3 video model's Qwen3-VL-32B text
//! encoder (CLIP type `minimax`) but decodes with its own 2D image VAE.
//! Prompts and reference images go through `MiniMaxH3ImageConditioning`,
//! which also returns the image latent (one 24-channel frame at 1/16 size).
//! Graphs follow Comfy-Org's `image_minimax_h3_image_{t2i,edit}` templates.
//!
//! It works best from 4 to 20 MP (native around 16 MP, 4096x4096) and the
//! model runs one image per sampler (`MiniMax H3 supports batch size 1`), so a
//! batch is several samplers on the same conditioning.
//!
//! The pDMD 2-step LoRA (`minimax_h3_pdmd_2nfe_comfyui.safetensors`, or a
//! checkpoint with it merged in) samples on a plain flow grid with the video
//! shift 12, so either one adds `MiniMaxH3SigmaShift`.
//!
//! Weights are under the MiniMax-H3-Image Community License: written
//! permission from MiniMax is needed above US$1M annual revenue or to offer it
//! as a hosted service.

use serde_json::{json, Map, Value};

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, ModeSpec, ModelFile, PromptStyle,
    RecommendedModel, Sampling, VaeSpec,
};
use crate::comfyui::types::GenerationParams;
use crate::templates::{insert_vae_decode, load_model_nodes, strip_lora_tags, WorkflowResult};

/// Width and height multiple of `MiniMaxH3ImageConditioning` (16x VAE, 2x2 patch).
pub const CANVAS_MULTIPLE: u32 = 32;

/// References MiniMax documents for editing (the node's autogrow takes 100).
pub const MAX_REFERENCES: usize = 16;

/// Area bounds for the edit target, in megapixels as `ImageScaleToTotalPixels`
/// counts them (1024x1024). MiniMax gives 4 to 20 MP as the working range
/// and Comfy-Org's edit template scales the source to 4 MP; the node caps at
/// 16. Image Edit uploads arrive downscaled to 1536 px and set the size from
/// that, so the floor is what usually applies.
const EDIT_MIN_MEGAPIXELS: f64 = 4.0;
const EDIT_MAX_MEGAPIXELS: f64 = 16.0;

/// Video and audio shifts the pDMD 2-step LoRA samples with.
const PDMD_SHIFT_VIDEO: f64 = 12.0;
const PDMD_SHIFT_AUDIO: f64 = 3.0;

/// Filename marker of the pDMD 2-step LoRA and of checkpoints with it merged.
const PDMD_MARKER: &str = "pdmd";

/// The video stack's encoder (`H3_TEXT_ENCODER` in `utils/h3Models.ts`):
/// same file, same URL, so a copy downloaded for video counts.
const ENCODER: ModelFile = ModelFile {
    filename: "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
    url: "https://huggingface.co/Comfy-Org/MiniMax-H3/resolve/main/text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
    category: "text_encoders",
    size: Some("~15.7 GB"),
    sha256: Some("35a88d51044231fe332301d7a62aa81e3f2cba62febeb446e2c1e3e0ef76f2c6"),
};

const VAE: ModelFile = ModelFile {
    filename: "minimax_h3_image_vae_fp16.safetensors",
    url: "https://huggingface.co/Comfy-Org/MiniMax-H3-Image/resolve/main/vae/minimax_h3_image_vae_fp16.safetensors",
    category: "vae",
    size: Some("~1.2 GB"),
    sha256: Some("d80e9c2e5cfc9844869961ff5a58c9a9e52182df6bfb30b26018dd7f348721af"),
};

/// Spellings of "MiniMax-H3-Image" in Comfy-Org, community and CivitAI names.
const NAME_MARKERS: &[&[&str]] = &[
    &["minimax_h3_image"],
    &["minimax-h3-image"],
    &["minimax h3 image"],
    &["minimax-h3 image"],
    &["minimaxh3image"],
    &["minimaxh3_image"],
    &["h3_image"],
    &["h3-image"],
];

const PDMD_MARKERS: &[&[&str]] = &[
    &["minimax_h3_image", PDMD_MARKER],
    &["minimax-h3-image", PDMD_MARKER],
    &["minimaxh3image", PDMD_MARKER],
    &["h3_image", PDMD_MARKER],
    &["h3-image", PDMD_MARKER],
];

/// Not this diffusion model: Pruna's P-H3-Image (another graph and license),
/// the image VAE, LoRAs and the prompt-enhancer encoders.
const NOT_A_MODEL: &[&str] = &[
    "p_h3", "p-h3", "ph3", "_vae", "-vae", "lora", "_pe", "-pe", "qwen3",
];

/// The base family leaves pDMD checkpoints to their own family.
const NOT_BASE: &[&str] = &[
    "p_h3",
    "p-h3",
    "ph3",
    "_vae",
    "-vae",
    "lora",
    "_pe",
    "-pe",
    "qwen3",
    PDMD_MARKER,
];

const fn diffusion_model(
    filename: &'static str,
    url: &'static str,
    size: &'static str,
    sha256: &'static str,
) -> ModelFile {
    ModelFile {
        filename,
        url,
        category: "diffusion_models",
        size: Some(size),
        sha256: Some(sha256),
    }
}

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "minimax_h3_image",
    label: "MiniMax-H3-Image",
    families: &[
        FamilySpec {
            family: "minimaxh3image",
            label: "MiniMax-H3-Image",
            detect: Detect {
                all_of: NAME_MARKERS,
                none_of: NOT_BASE,
                base_models: &["minimax h3 image", "minimax-h3-image"],
            },
            turbo: true,
            sampling: Some(Sampling {
                steps: 6,
                cfg: 1.0,
                sampler: "euler",
                scheduler: "simple",
                width: 2048,
                height: 2048,
            }),
            recommended: &[
                RecommendedModel {
                    label: "MiniMax-H3-Image (INT8)",
                    size: "~37.9 GB",
                    diffusion_model: diffusion_model(
                        "minimax_h3_image_pruned_int8_convrot.safetensors",
                        "https://huggingface.co/Comfy-Org/MiniMax-H3-Image/resolve/main/diffusion_models/minimax_h3_image_pruned_int8_convrot.safetensors",
                        "~21.0 GB",
                        "83eb140f1ff95c068f8a6886e6d3f3d0d4ab2e07bb2fe6d2ef992a51146ee622",
                    ),
                    min_compute_capability: None,
                },
                RecommendedModel {
                    label: "MiniMax-H3-Image (W4A8, smaller)",
                    size: "~29.4 GB",
                    diffusion_model: diffusion_model(
                        "minimax_h3_image_pruned_w4a8.safetensors",
                        "https://huggingface.co/Comfy-Org/MiniMax-H3-Image/resolve/main/diffusion_models/minimax_h3_image_pruned_w4a8.safetensors",
                        "~12.5 GB",
                        "517cc19b85348386f4df7c806dce80d839d0899e7cdaa3e1dbb09de964381dbb",
                    ),
                    min_compute_capability: None,
                },
            ],
            text_encoder: None,
            vae: None,
        },
        FamilySpec {
            family: "minimaxh3imagepdmd",
            label: "MiniMax-H3-Image pDMD 2-step",
            detect: Detect {
                all_of: PDMD_MARKERS,
                none_of: NOT_A_MODEL,
                base_models: &[],
            },
            turbo: true,
            sampling: Some(Sampling {
                steps: 2,
                cfg: 1.0,
                sampler: "euler",
                scheduler: "simple",
                width: 2048,
                height: 2048,
            }),
            recommended: &[],
            text_encoder: None,
            vae: None,
        },
    ],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("minimax_h3_image"),
    text_encoder: Some(EncoderSpec {
        clip_type: "minimax",
        // The H3 conditioning encoder (Qwen3-VL-32B truncated to 50 layers),
        // in any of its published precisions.
        markers: &[
            "qwen3vl_32b_minimax_h3",
            "qwen3vl-32b-minimax-h3",
            "minimax_h3_text_encoder",
            "minimax-h3-text-encoder",
            "minimax_h3_te",
            "h3_image_text_encoder",
            "h3-image-text-encoder",
        ],
        strict: true,
        download: Some(ENCODER),
    }),
    vae: Some(VaeSpec {
        basenames: &["minimax_h3_image_vae_fp16.safetensors"],
        markers: &[
            "minimax_h3_image_vae",
            "minimax-h3-image-vae",
            "h3_image_vae",
            "h3-image-vae",
        ],
        strict: true,
        download: Some(VAE),
    }),
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &[
        "MiniMaxH3ImageConditioning",
        "MiniMaxH3SigmaShift",
        "ConditioningZeroOut",
        "ImageScaleToTotalPixels",
        "GetImageSize",
    ],
    min_comfyui: Some("0.45.0"),
    // ComfyUI loads the INT8 ConvRot and W4A8 files natively.
    int8_fast_model_type: None,
    modes: &[
        ModeSpec {
            mode: "txt2img",
            max_references: 0,
            required_files: &[],
            sampling: None,
            build: Some(build_txt2img),
        },
        ModeSpec {
            mode: "image_edit",
            max_references: MAX_REFERENCES as u32,
            required_files: &[],
            sampling: None,
            build: Some(build_edit),
        },
    ],
    // Guidance-distilled at CFG 1: the negative prompt does nothing.
    negative_prompt: false,
    resolution_multiple: CANVAS_MULTIPLE,
    loader: None,
    text_encode: Some(text_encode),
};

/// `MiniMaxH3ImageConditioning` in place of `CLIPTextEncode` (the hires fix,
/// face fix and segment detail chains): the prompt alone, output 0 is the
/// conditioning and the latent output goes unused.
fn text_encode(_params: &GenerationParams, clip: &Value, text: &str) -> Value {
    json!({
        "class_type": "MiniMaxH3ImageConditioning",
        "inputs": {
            "clip": clip,
            "prompt": text,
            "width": 0,
            "height": 0
        }
    })
}

/// Rounds a side to the canvas multiple, never below one multiple.
pub fn snap(side: u32) -> u32 {
    let m = CANVAS_MULTIPLE;
    ((side + m / 2) / m).max(1) * m
}

/// True when the pDMD 2-step adapter is in play: the merged checkpoint family
/// or the LoRA in the LoRA list.
pub fn pdmd_active(params: &GenerationParams) -> bool {
    params.model_architecture == "minimaxh3imagepdmd"
        || params
            .loras
            .iter()
            .any(|lora| lora.name.to_lowercase().contains(PDMD_MARKER))
}

/// Edit target area: the requested width x height, clamped to the range the
/// model and `ImageScaleToTotalPixels` accept.
fn edit_megapixels(params: &GenerationParams) -> f64 {
    let area = f64::from(params.width) * f64::from(params.height) / (1024.0 * 1024.0);
    let mp = if area > 0.0 { area } else { 4.0 };
    (mp.clamp(EDIT_MIN_MEGAPIXELS, EDIT_MAX_MEGAPIXELS) * 100.0).round() / 100.0
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

fn next(next_id: &mut u32) -> String {
    let id = next_id.to_string();
    *next_id += 1;
    id
}

/// Shared tail of both modes: the pDMD shift, a negative that is the zeroed
/// positive, one sampler per batch image on the conditioning node's latent,
/// decoded and batched.
/// Loader outputs after the LoRA chain.
struct Sources {
    model: (String, u32),
    clip: (String, u32),
    vae: (String, u32),
}

fn sample_and_decode(
    mut workflow: Map<String, Value>,
    mut next_id: u32,
    params: &GenerationParams,
    seed: i64,
    sources: Sources,
    conditioning_id: String,
) -> WorkflowResult {
    let Sources {
        model: model_source,
        clip: clip_source,
        vae: vae_source,
    } = sources;
    let model_source = if pdmd_active(params) {
        let shift_id = next(&mut next_id);
        workflow.insert(
            shift_id.clone(),
            json!({"class_type": "MiniMaxH3SigmaShift", "inputs": {
                "model": [model_source.0, model_source.1],
                "shift_video": PDMD_SHIFT_VIDEO,
                "shift_audio": PDMD_SHIFT_AUDIO
            }}),
        );
        (shift_id, 0)
    } else {
        model_source
    };

    let negative_id = next(&mut next_id);
    workflow.insert(
        negative_id.clone(),
        json!({"class_type": "ConditioningZeroOut", "inputs": {
            "conditioning": [conditioning_id, 0]
        }}),
    );

    let mut sampler_ids: Vec<String> = Vec::new();
    let mut image: Option<(String, u32)> = None;
    for index in 0..params.batch_size.max(1) {
        let sampler_id = next(&mut next_id);
        workflow.insert(
            sampler_id.clone(),
            json!({"class_type": "KSampler", "inputs": {
                "model": [model_source.0, model_source.1],
                "positive": [conditioning_id, 0],
                "negative": [negative_id, 0],
                "latent_image": [conditioning_id, 1],
                "seed": seed.wrapping_add(i64::from(index)),
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": 1.0
            }}),
        );
        let (decode_id, id) =
            insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);
        next_id = id;
        image = Some(match image {
            None => (decode_id, 0),
            Some(previous) => {
                let batch_id = next(&mut next_id);
                workflow.insert(
                    batch_id.clone(),
                    json!({"class_type": "ImageBatch", "inputs": {
                        "image1": [previous.0, previous.1],
                        "image2": [decode_id, 0]
                    }}),
                );
                (batch_id, 0)
            }
        });
        sampler_ids.push(sampler_id);
    }

    WorkflowResult {
        workflow,
        next_id,
        image_output: image.expect("at least one sampler"),
        model_source,
        clip_source,
        positive_source: (conditioning_id, 0),
        negative_source: (negative_id, 0),
        vae_source,
        sampler_id: sampler_ids.swap_remove(0),
        refiner_model_source: None,
        base_sources: None,
    }
}

/// Text to image: the conditioning node returns the empty latent at the
/// requested size, snapped to 32.
fn build_txt2img(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow: Map<String, Value> = Map::new();
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut next_id = ml.next_id;

    let conditioning_id = next(&mut next_id);
    workflow.insert(
        conditioning_id.clone(),
        json!({"class_type": "MiniMaxH3ImageConditioning", "inputs": {
            "clip": [ml.clip_source.0, ml.clip_source.1],
            "prompt": strip_lora_tags(&params.positive_prompt),
            "width": snap(params.width),
            "height": snap(params.height)
        }}),
    );

    sample_and_decode(
        workflow,
        next_id,
        params,
        seed,
        Sources {
            model: ml.model_source,
            clip: ml.clip_source,
            vae: ml.vae_source,
        },
        conditioning_id,
    )
}

/// Image Edit: the edit source is scaled to the requested area (multiples of
/// 32) and sets the output size; every reference goes into one
/// `MiniMaxH3ImageConditioning` with the VAE, as `<Picture n>` in the prompt.
fn build_edit(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow: Map<String, Value> = Map::new();
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut next_id = ml.next_id;

    let mut inputs = Map::new();
    inputs.insert("clip".into(), json!([ml.clip_source.0, ml.clip_source.1]));
    inputs.insert("vae".into(), json!([ml.vae_source.0, ml.vae_source.1]));
    inputs.insert(
        "prompt".into(),
        json!(strip_lora_tags(&params.positive_prompt)),
    );
    inputs.insert("width".into(), json!(0));
    inputs.insert("height".into(), json!(0));
    for (index, image) in references(params).into_iter().enumerate() {
        let load_id = next(&mut next_id);
        workflow.insert(
            load_id.clone(),
            json!({"class_type": "LoadImage", "inputs": {"image": image}}),
        );
        let mut source = json!([load_id, 0]);
        if index == 0 {
            let scale_id = next(&mut next_id);
            workflow.insert(
                scale_id.clone(),
                json!({"class_type": "ImageScaleToTotalPixels", "inputs": {
                    "image": [load_id, 0],
                    "upscale_method": "lanczos",
                    "megapixels": edit_megapixels(params),
                    "resolution_steps": CANVAS_MULTIPLE
                }}),
            );
            let size_id = next(&mut next_id);
            workflow.insert(
                size_id.clone(),
                json!({"class_type": "GetImageSize", "inputs": {"image": [scale_id, 0]}}),
            );
            inputs.insert("width".into(), json!([size_id, 0]));
            inputs.insert("height".into(), json!([size_id, 1]));
            source = json!([scale_id, 0]);
        }
        inputs.insert(format!("images.image_{}", index + 1), source);
    }

    let conditioning_id = next(&mut next_id);
    workflow.insert(
        conditioning_id.clone(),
        json!({"class_type": "MiniMaxH3ImageConditioning", "inputs": Value::Object(inputs)}),
    );

    sample_and_decode(
        workflow,
        next_id,
        params,
        seed,
        Sources {
            model: ml.model_source,
            clip: ml.clip_source,
            vae: ml.vae_source,
        },
        conditioning_id,
    )
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
            diffusion_model: Some("minimax_h3_image_pruned_int8_convrot.safetensors".into()),
            clip_model: Some("qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors".into()),
            clip_type: Some("minimax".into()),
            vae: Some("minimax_h3_image_vae_fp16.safetensors".into()),
            positive_prompt: "a red fox in the snow".into(),
            negative_prompt: "blurry".into(),
            sampler_name: "euler".into(),
            scheduler: "simple".into(),
            steps: 6,
            cfg: 1.0,
            width: 2048,
            height: 2048,
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
    fn detection_tells_h3_image_from_h3_video_and_p_h3_image() {
        for (name, family) in [
            (
                "minimax_h3_image_pruned_int8_convrot.safetensors",
                Some("minimaxh3image"),
            ),
            (
                "minimax_h3_image_pruned_w4a8.safetensors",
                Some("minimaxh3image"),
            ),
            ("MiniMax-H3-Image-Q4_K_M.gguf", Some("minimaxh3image")),
            ("MiniMax H3 Image", Some("minimaxh3image")),
            (
                "minimax_h3_image_pdmd_2step_bf16.safetensors",
                Some("minimaxh3imagepdmd"),
            ),
            ("minimax_h3_image_vae_fp16.safetensors", None),
            ("minimax_h3_pdmd_2nfe_comfyui.safetensors", None),
            ("minimax_h3_image_restyle_lora.safetensors", None),
            ("p_h3_image_int8_convrot.safetensors", None),
            ("P-H3-Image-w4a8.safetensors", None),
            ("minimax_h3_fl2va_int8_convrot.safetensors", None),
            ("minimax_h3_ref2va_nvfp4.safetensors", None),
            ("qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors", None),
        ] {
            assert_eq!(architectures::family_from_text(name), family, "{name}");
        }
    }

    #[test]
    fn pairs_the_shared_h3_encoder_and_only_the_image_vae() {
        let encoders = vec![
            "qwen_3_4b.safetensors".to_string(),
            "h3/qwen3vl_32b_minimax_h3_int8_convrot.safetensors".to_string(),
        ];
        assert_eq!(
            architectures::recommended_clip("minimaxh3image", &encoders),
            Some((
                Some("h3/qwen3vl_32b_minimax_h3_int8_convrot.safetensors".into()),
                "minimax"
            ))
        );
        assert_eq!(
            architectures::recommended_clip("minimaxh3image", &encoders[..1]),
            Some((None, "minimax"))
        );
        let vaes = vec![
            "minimax_h3_video_vae_fp16.safetensors".to_string(),
            "minimax_h3_image_vae_fp16.safetensors".to_string(),
        ];
        assert_eq!(
            architectures::recommended_vae("minimaxh3image", &vaes),
            Some(Some("minimax_h3_image_vae_fp16.safetensors".into()))
        );
        assert_eq!(
            architectures::recommended_vae("minimaxh3image", &vaes[..1]),
            Some(None)
        );
    }

    #[test]
    fn validation_refuses_other_modes_and_encoders() {
        let p = params("txt2img", "minimaxh3image");
        assert!(crate::templates::validate_generation_params(&p).is_ok());
        let mut p = params("inpainting", "minimaxh3image");
        p.input_image = Some("in.png".into());
        assert!(architectures::validate(&p)
            .unwrap_err()
            .contains("does not support inpainting"));
        let mut p = params("txt2img", "minimaxh3image");
        p.clip_model = Some("qwen3vl_8b_int8_convrot.safetensors".into());
        let err = architectures::validate(&p).unwrap_err();
        assert!(
            err.contains("qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors"),
            "{err}"
        );
    }

    #[test]
    fn txt2img_samples_six_steps_on_the_conditioning_latent() {
        let mut p = params("txt2img", "minimaxh3image");
        p.width = 4100;
        p.height = 3000;
        let workflow = crate::templates::build_workflow(&p, 7, false);
        assert!(nodes(&workflow, "CLIPTextEncode").is_empty());
        assert!(nodes(&workflow, "EmptyLatentImage").is_empty());
        let conds = nodes(&workflow, "MiniMaxH3ImageConditioning");
        assert_eq!(conds.len(), 1);
        let cond = &conds[0]["inputs"];
        assert_eq!(cond["prompt"], "a red fox in the snow");
        assert_eq!(cond["width"], 4096);
        assert_eq!(cond["height"], 3008);
        assert!(cond.get("vae").is_none());
        assert_eq!(
            nodes(&workflow, "CLIPLoader")[0]["inputs"]["type"],
            "minimax"
        );
        let samplers = nodes(&workflow, "KSampler");
        assert_eq!(samplers.len(), 1);
        let sampler = &samplers[0]["inputs"];
        assert_eq!(sampler["steps"], 6);
        assert_eq!(sampler["cfg"], 1.0);
        assert_eq!(sampler["latent_image"][1], 1);
        assert_eq!(sampler["seed"], 7);
        let zero = &nodes(&workflow, "ConditioningZeroOut")[0]["inputs"];
        assert_eq!(zero["conditioning"], cond_link(&workflow));
        assert!(nodes(&workflow, "MiniMaxH3SigmaShift").is_empty());
        assert_eq!(nodes(&workflow, "MooshieSaveImage").len(), 1);
    }

    fn cond_link(workflow: &Value) -> Value {
        let id = workflow
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, n)| n["class_type"] == "MiniMaxH3ImageConditioning")
            .map(|(id, _)| id.clone())
            .unwrap();
        json!([id, 0])
    }

    #[test]
    fn a_batch_is_one_sampler_per_image_with_consecutive_seeds() {
        let mut p = params("txt2img", "minimaxh3image");
        p.batch_size = 3;
        let workflow = crate::templates::build_workflow(&p, 10, false);
        let mut seeds: Vec<i64> = nodes(&workflow, "KSampler")
            .iter()
            .map(|n| n["inputs"]["seed"].as_i64().unwrap())
            .collect();
        seeds.sort();
        assert_eq!(seeds, vec![10, 11, 12]);
        assert_eq!(nodes(&workflow, "VAEDecode").len(), 3);
        assert_eq!(nodes(&workflow, "ImageBatch").len(), 2);
        assert_eq!(nodes(&workflow, "MiniMaxH3ImageConditioning").len(), 1);
    }

    #[test]
    fn pdmd_lora_or_checkpoint_adds_the_sigma_shift() {
        let mut p = params("txt2img", "minimaxh3image");
        p.steps = 2;
        p.loras = vec![crate::comfyui::types::LoraParam {
            name: "minimax_h3_pdmd_2nfe_comfyui.safetensors".into(),
            strength_model: 1.0,
            strength_clip: 1.0,
        }];
        let workflow = crate::templates::build_workflow(&p, 1, false);
        let shift = &nodes(&workflow, "MiniMaxH3SigmaShift")[0]["inputs"];
        assert_eq!(shift["shift_video"], 12.0);
        let lora_id = workflow
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, n)| n["class_type"] == "LoraLoader")
            .map(|(id, _)| id.clone())
            .unwrap();
        assert_eq!(shift["model"], json!([lora_id, 0]));

        let p = params("txt2img", "minimaxh3imagepdmd");
        let workflow = crate::templates::build_workflow(&p, 1, false);
        assert_eq!(nodes(&workflow, "MiniMaxH3SigmaShift").len(), 1);
    }

    #[test]
    fn edit_scales_the_source_and_feeds_every_reference() {
        let mut p = params("image_edit", "minimaxh3image");
        p.input_image = Some("a.png".into());
        p.edit_reference_images = vec!["a.png".into(), "".into(), "b.png".into()];
        p.width = 4096;
        p.height = 4096;
        assert!(crate::templates::validate_generation_params(&p).is_ok());
        let workflow = crate::templates::build_workflow(&p, 1, false);
        let cond = &nodes(&workflow, "MiniMaxH3ImageConditioning")[0]["inputs"];
        assert!(cond.get("vae").is_some());
        assert!(cond.get("images.image_1").is_some());
        assert!(cond.get("images.image_2").is_some());
        assert!(cond.get("images.image_3").is_none());
        let scale = &nodes(&workflow, "ImageScaleToTotalPixels")[0]["inputs"];
        assert_eq!(scale["megapixels"], 16.0);
        assert_eq!(scale["resolution_steps"], 32);
        assert_eq!(
            cond["images.image_1"][0],
            workflow_id_of(&workflow, "ImageScaleToTotalPixels")
        );
        assert_eq!(
            cond["width"],
            json!([workflow_id_of(&workflow, "GetImageSize"), 0])
        );
        assert_eq!(
            cond["height"],
            json!([workflow_id_of(&workflow, "GetImageSize"), 1])
        );
        assert_eq!(nodes(&workflow, "LoadImage").len(), 2);
        assert_eq!(
            nodes(&workflow, "KSampler")[0]["inputs"]["latent_image"][1],
            1
        );
    }

    fn workflow_id_of(workflow: &Value, class: &str) -> String {
        workflow
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, n)| n["class_type"] == class)
            .map(|(id, _)| id.clone())
            .unwrap()
    }

    #[test]
    fn edit_area_stays_in_the_working_range() {
        let mut p = params("image_edit", "minimaxh3image");
        p.width = 1536;
        p.height = 1024;
        assert_eq!(edit_megapixels(&p), 4.0);
        p.width = 3072;
        p.height = 2048;
        assert_eq!(edit_megapixels(&p), 6.0);
        p.width = 8192;
        p.height = 8192;
        assert_eq!(edit_megapixels(&p), 16.0);
    }

    #[test]
    fn edit_takes_up_to_sixteen_references() {
        assert!(crate::templates::image_edit::supports_family(
            "minimaxh3image"
        ));
        let mut p = params("image_edit", "minimaxh3image");
        p.input_image = Some("a.png".into());
        p.edit_reference_images = (0..17).map(|i| format!("{i}.png")).collect();
        assert!(architectures::validate(&p)
            .unwrap_err()
            .contains("up to 16"));
        p.edit_reference_images.pop();
        assert!(architectures::validate(&p).is_ok());
    }

    #[test]
    fn face_fix_encodes_through_the_conditioning_node() {
        let mut p = params("txt2img", "minimaxh3image");
        p.facefix_enabled = true;
        p.facefix_prompt_override = Some("a detailed face".into());
        let workflow = crate::templates::build_workflow(&p, 1, false);
        assert!(nodes(&workflow, "CLIPTextEncode").is_empty());
        assert!(nodes(&workflow, "MiniMaxH3ImageConditioning").len() >= 2);
    }

    #[test]
    fn snapping_rounds_to_32() {
        assert_eq!(snap(2048), 2048);
        assert_eq!(snap(2063), 2048);
        assert_eq!(snap(2064), 2080);
        assert_eq!(snap(1), 32);
    }

    #[test]
    fn pausing_is_refused() {
        let mut p = params("txt2img", "minimaxh3image");
        p.pause_at_step = Some(3);
        let err = crate::templates::validate_generation_params(&p).unwrap_err();
        assert!(err.contains("MiniMax-H3-Image"), "{err}");
    }
}
