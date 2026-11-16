//! The finishing half of the Moody Moss 4K HD workflow (V3), rebuilt from
//! core ComfyUI nodes and MooshieUI's own nodes instead of the rgthree,
//! Impact Pack and SeedVR2 custom node packs it was published with.
//!
//! After the two sampling passes (`txt2img::moody_moss_hd_active`) the
//! workflow runs, each step optional and in this order:
//!
//! 1. an automatic face detailer (`MooshieFaceDetailer`) with its own prompt
//!    and LoRAs on the bare model,
//! 2. a 1x skin contrast model blended over the image (`MooshieSkinContrast`,
//!    the CC0 weights ship with mooshie-nodes),
//! 3. a SeedVR2 restoration upscale to a 4096 px long edge, blended with a 4x
//!    model upscale of the same image.
//!
//! Its manual "Detail Anything" detailer, where the user paints over part of
//! a finished image, is an inpainting request here (`build_area_detail`).

use serde_json::json;

use super::upscale::{seedvr2_restore, Seedvr2Restore, SEEDVR2_UNET_FILE};
use super::{load_model_nodes, strip_lora_tags, WorkflowResult};
use crate::comfyui::types::{GenerationParams, LoraParam, MoodyMossFinish};

/// Face detector of the workflow (a bbox YOLO model from Bingsu/adetailer).
pub const FACE_DETECTOR: &str = "face_yolov8m.pt";
/// 4x upscaler the SeedVR2 output is blended with.
pub const NOMOS_UPSCALER: &str = "4xNomosWebPhoto_RealPLKSR.pth";
/// The 7B SeedVR2 model, the size the workflow uses.
pub const SEEDVR2_7B_UNET_FILE: &str = "seedvr2_7b_fp8_e4m3fn.safetensors";

/// Both detailers sample this way in the workflow.
const DETAIL_STEPS: u32 = 20;
const DETAIL_CFG: f64 = 1.0;
const DETAIL_SAMPLER: &str = "er_sde";
const DETAIL_SCHEDULER: &str = "beta";
const DETAIL_GUIDE_SIZE: u32 = 1440;

/// Seed offsets, clear of upscale (+1), face fix (+2) and segments (+3 + i).
const FACE_SEED_OFFSET: u64 = 200;
const AREA_SEED_OFFSET: u64 = 201;
const SEEDVR2_SEED_OFFSET: u64 = 202;

fn is_moss(params: &GenerationParams) -> bool {
    matches!(
        params.model_architecture.as_str(),
        "mossimage" | "mossimagebase"
    )
}

/// The finishing settings when this txt2img request should run them.
pub fn finish_settings(params: &GenerationParams) -> Option<&MoodyMossFinish> {
    params
        .moody_moss
        .as_ref()
        .filter(|_| is_moss(params) && params.mode == "txt2img")
}

/// Whether this inpainting request is the workflow's "Detail Anything" pass.
pub fn area_detail_active(params: &GenerationParams) -> bool {
    params.mode == "inpainting"
        && is_moss(params)
        && params.moody_moss.as_ref().is_some_and(|s| s.area_detail)
        && params.input_image.as_deref().is_some_and(|i| !i.is_empty())
        && params.mask_image.as_deref().is_some_and(|m| !m.is_empty())
}

/// The workflow's detailer negative: the main negative plus "text, logo".
fn detail_negative(params: &GenerationParams) -> String {
    let negative = params.negative_prompt.trim().trim_end_matches(',').trim();
    if negative.is_empty() {
        "text, logo".to_string()
    } else {
        format!("{negative}, text, logo")
    }
}

fn insert(result: &mut WorkflowResult, node: serde_json::Value) -> String {
    let id = result.next_id.to_string();
    result.workflow.insert(id.clone(), node);
    result.next_id += 1;
    id
}

/// A detailer's model and CLIP: the loaded model before the main LoRAs, with
/// the detailer's own LoRAs on top.
fn detail_model(
    result: &mut WorkflowResult,
    loras: &[LoraParam],
) -> ((String, u32), (String, u32)) {
    let (mut model, mut clip) = match result.base_sources.as_ref() {
        Some(base) => (base.model.clone(), base.clip.clone()),
        None => (result.model_source.clone(), result.clip_source.clone()),
    };
    for lora in loras.iter().filter(|l| !l.name.trim().is_empty()) {
        let id = insert(
            result,
            json!({
                "class_type": "LoraLoader",
                "inputs": {
                    "model": [model.0, model.1],
                    "clip": [clip.0, clip.1],
                    "lora_name": lora.name,
                    "strength_model": lora.strength_model,
                    "strength_clip": lora.strength_clip
                }
            }),
        );
        model = (id.clone(), 0);
        clip = (id, 1);
    }
    (model, clip)
}

fn encode(result: &mut WorkflowResult, clip: &(String, u32), text: &str) -> (String, u32) {
    let id = insert(
        result,
        json!({
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": [clip.0, clip.1],
                "text": strip_lora_tags(text)
            }
        }),
    );
    (id, 0)
}

/// A detailer's prompt, falling back to the main prompt when left empty.
fn detail_prompt<'a>(params: &'a GenerationParams, prompt: &'a str) -> &'a str {
    if prompt.trim().is_empty() {
        &params.positive_prompt
    } else {
        prompt
    }
}

/// Run the workflow's finishing steps the settings turn on. Returns the
/// final IMAGE (the input when none are on).
pub fn append_finish(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    image: (String, u32),
    seed: i64,
) -> (String, u32) {
    let Some(settings) = finish_settings(params).cloned() else {
        return image;
    };
    let mut image = image;
    if settings.face_detail {
        image = append_face_detail(result, params, &settings, image, seed);
    }
    if settings.skin_contrast && settings.skin_contrast_strength > 0.0 {
        image = append_skin_contrast(result, settings.skin_contrast_strength, image);
    }
    if settings.seedvr2 {
        image = append_seedvr2_final(result, &settings, image, seed);
    }
    image
}

/// The workflow's auto face detailer: the most confident face, cropped at
/// 2.2x its box, detailed at 1440 px and feathered back in.
fn append_face_detail(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    settings: &MoodyMossFinish,
    image: (String, u32),
    seed: i64,
) -> (String, u32) {
    let (model, clip) = detail_model(result, &settings.face_loras);
    let positive = encode(result, &clip, detail_prompt(params, &settings.face_prompt));
    let negative = encode(result, &clip, &detail_negative(params));
    let vae = result.vae_source.clone();
    let id = insert(
        result,
        json!({
            "class_type": "MooshieFaceDetailer",
            "inputs": {
                "image": [image.0, image.1],
                "model": [model.0, model.1],
                "vae": [vae.0, vae.1],
                "positive": [positive.0, positive.1],
                "negative": [negative.0, negative.1],
                "detector_model": FACE_DETECTOR,
                "seed": super::offset_seed(seed, FACE_SEED_OFFSET),
                "steps": DETAIL_STEPS,
                "cfg": DETAIL_CFG,
                "sampler_name": DETAIL_SAMPLER,
                "scheduler": DETAIL_SCHEDULER,
                "denoise": settings.face_denoise,
                "guide_size": DETAIL_GUIDE_SIZE,
                "bbox_threshold": 0.5,
                "bbox_padding": 2.2,
                "feather": 100,
                "max_faces": 1
            }
        }),
    );
    (id, 0)
}

fn append_skin_contrast(
    result: &mut WorkflowResult,
    strength: f64,
    image: (String, u32),
) -> (String, u32) {
    let id = insert(
        result,
        json!({
            "class_type": "MooshieSkinContrast",
            "inputs": {
                "image": [image.0, image.1],
                "strength": strength.clamp(0.0, 1.0)
            }
        }),
    );
    (id, 0)
}

/// SeedVR2 to the long edge, colour-matched in LAB, then blended with a 4x
/// model upscale scaled to the same size (the workflow's "SEEDVR2 STRENGTH"
/// blend, where the factor is the model upscale's share).
fn append_seedvr2_final(
    result: &mut WorkflowResult,
    settings: &MoodyMossFinish,
    image: (String, u32),
    seed: i64,
) -> (String, u32) {
    let long_edge = settings.long_edge.clamp(1024, 8192);
    let scale_to_edge = |result: &mut WorkflowResult, source: &(String, u32)| {
        let id = insert(
            result,
            json!({
                "class_type": "ImageScaleToMaxDimension",
                "inputs": {
                    "image": [source.0, source.1],
                    "upscale_method": "lanczos",
                    "largest_size": long_edge
                }
            }),
        );
        (id, 0)
    };

    let resized = scale_to_edge(result, &image);
    let unet = if settings.seedvr2_7b {
        SEEDVR2_7B_UNET_FILE
    } else {
        SEEDVR2_UNET_FILE
    };
    let restored = seedvr2_restore(
        result,
        resized,
        &Seedvr2Restore {
            unet,
            color_correction: "lab",
            seed: super::offset_seed(seed, SEEDVR2_SEED_OFFSET),
        },
    );

    let mix = settings.seedvr2_mix.clamp(0.0, 1.0);
    if mix <= 0.0 {
        return restored;
    }
    let loader = insert(
        result,
        json!({
            "class_type": "UpscaleModelLoader",
            "inputs": { "model_name": NOMOS_UPSCALER }
        }),
    );
    let upscaled = insert(
        result,
        json!({
            "class_type": "ImageUpscaleWithModel",
            "inputs": {
                "upscale_model": [loader, 0],
                "image": [image.0, image.1]
            }
        }),
    );
    let upscaled = scale_to_edge(result, &(upscaled, 0));
    let blend = insert(
        result,
        json!({
            "class_type": "ImageBlend",
            "inputs": {
                "image1": [restored.0, restored.1],
                "image2": [upscaled.0, upscaled.1],
                "blend_factor": mix,
                "blend_mode": "normal"
            }
        }),
    );
    (blend, 0)
}

/// The workflow's manual "Detail Anything" detailer as an inpainting request:
/// the painted area, grown by 8 px and cropped at 6.1x its box, is detailed
/// at 1440 px with its own prompt and LoRAs and blended back. The image keeps
/// its size; nothing else in the request runs.
pub fn build_area_detail(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let settings = params.moody_moss.clone().unwrap_or_default();
    let mut workflow = serde_json::Map::new();
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut result = WorkflowResult {
        workflow,
        next_id: ml.next_id,
        image_output: (String::new(), 0),
        model_source: ml.model_source,
        clip_source: ml.clip_source,
        positive_source: (String::new(), 0),
        negative_source: (String::new(), 0),
        vae_source: ml.vae_source,
        sampler_id: String::new(),
        refiner_model_source: None,
        base_sources: Some(ml.base),
    };

    let image = insert(
        &mut result,
        json!({
            "class_type": "LoadImage",
            "inputs": { "image": params.input_image.as_deref().unwrap_or("") }
        }),
    );
    let mask = insert(
        &mut result,
        json!({
            "class_type": "LoadImageMask",
            "inputs": {
                "image": params.mask_image.as_deref().unwrap_or(""),
                "channel": "red"
            }
        }),
    );
    let (model, clip) = detail_model(&mut result, &settings.area_loras);
    let positive = encode(
        &mut result,
        &clip,
        detail_prompt(params, &settings.area_prompt),
    );
    let negative = encode(&mut result, &clip, &detail_negative(params));
    result.positive_source = positive.clone();
    result.negative_source = negative.clone();
    let vae = result.vae_source.clone();

    let detailer = insert(
        &mut result,
        json!({
            "class_type": "MooshieMaskDetailer",
            "inputs": {
                "image": [image, 0],
                "mask": [mask, 0],
                "model": [model.0, model.1],
                "vae": [vae.0, vae.1],
                "positive": [positive.0, positive.1],
                "negative": [negative.0, negative.1],
                "seed": super::offset_seed(seed, AREA_SEED_OFFSET),
                "steps": DETAIL_STEPS,
                "cfg": DETAIL_CFG,
                "sampler_name": DETAIL_SAMPLER,
                "scheduler": DETAIL_SCHEDULER,
                "denoise": settings.area_denoise,
                "guide_size": DETAIL_GUIDE_SIZE,
                "crop_factor": 6.1,
                "mask_dilation": 8,
                "feather": 16
            }
        }),
    );
    result.sampler_id = detailer.clone();
    result.image_output = (detailer, 0);
    result
}

/// Request settings for finishing an area-detail pass: only the save, since
/// the image already went through the rest when it was generated.
pub fn area_detail_finish_params(params: &GenerationParams) -> GenerationParams {
    let mut finish = params.clone();
    finish.upscale_enabled = false;
    finish.facefix_enabled = false;
    finish.detail_segments.clear();
    finish
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::graph_test_util::{build, linked, nodes, params, single};
    use serde_json::Value;

    fn moody(mode: &str, finish: MoodyMossFinish) -> GenerationParams {
        let mut p = params(mode, "mossimage");
        p.negative_prompt = "worst quality".to_string();
        p.moody_moss = Some(finish);
        p
    }

    fn all_on() -> MoodyMossFinish {
        MoodyMossFinish {
            face_detail: true,
            face_prompt: "Person, cute girl <lora:face:0.8>".to_string(),
            face_loras: vec![LoraParam {
                name: "face.safetensors".to_string(),
                strength_model: 0.8,
                strength_clip: 1.0,
            }],
            skin_contrast: true,
            seedvr2: true,
            ..MoodyMossFinish::default()
        }
    }

    /// The node an IMAGE link leads to, walking back from the save node.
    fn saved_from(workflow: &Value) -> &Value {
        let save = single(workflow, "MooshieSaveImage");
        linked(workflow, &save["inputs"]["images"])
    }

    #[test]
    fn finishing_steps_run_in_workflow_order() {
        let workflow = build(&moody("txt2img", all_on()));

        let blend = saved_from(&workflow);
        assert_eq!(blend["class_type"], "ImageBlend");
        assert_eq!(blend["inputs"]["blend_factor"], json!(0.4));
        let post = linked(&workflow, &blend["inputs"]["image1"]);
        assert_eq!(post["class_type"], "SeedVR2PostProcessing");
        assert_eq!(post["inputs"]["color_correction_method"], "lab");
        let edge = linked(&workflow, &post["inputs"]["original_resized_images"]);
        assert_eq!(edge["class_type"], "ImageScaleToMaxDimension");
        assert_eq!(edge["inputs"]["largest_size"], json!(4096));

        let skin = linked(&workflow, &edge["inputs"]["image"]);
        assert_eq!(skin["class_type"], "MooshieSkinContrast");
        assert_eq!(skin["inputs"]["strength"], json!(0.2));
        let face = linked(&workflow, &skin["inputs"]["image"]);
        assert_eq!(face["class_type"], "MooshieFaceDetailer");
        assert_eq!(face["inputs"]["detector_model"], FACE_DETECTOR);
        assert_eq!(face["inputs"]["sampler_name"], "er_sde");
        assert_eq!(face["inputs"]["scheduler"], "beta");
        assert_eq!(face["inputs"]["cfg"], json!(1.0));
        assert_eq!(face["inputs"]["guide_size"], json!(1440));
        assert_eq!(face["inputs"]["denoise"], json!(0.35));
        assert_eq!(face["inputs"]["max_faces"], json!(1));

        // The 4x model upscale branch starts from the same image as SeedVR2
        // and is scaled to the same long edge.
        let model_edge = linked(&workflow, &blend["inputs"]["image2"]);
        assert_eq!(model_edge["inputs"]["largest_size"], json!(4096));
        let upscale = linked(&workflow, &model_edge["inputs"]["image"]);
        assert_eq!(upscale["inputs"]["image"], edge["inputs"]["image"]);
        assert_eq!(
            linked(&workflow, &upscale["inputs"]["upscale_model"])["inputs"]["model_name"],
            NOMOS_UPSCALER
        );
        let loader = single(&workflow, "UNETLoader");
        assert_eq!(loader["inputs"]["unet_name"], SEEDVR2_UNET_FILE);
    }

    #[test]
    fn face_detailer_uses_its_own_loras_on_the_bare_model() {
        let mut p = moody("txt2img", all_on());
        p.loras = vec![LoraParam {
            name: "main.safetensors".to_string(),
            strength_model: 1.0,
            strength_clip: 1.0,
        }];
        let workflow = build(&p);

        let face = single(&workflow, "MooshieFaceDetailer");
        let lora = linked(&workflow, &face["inputs"]["model"]);
        assert_eq!(lora["class_type"], "LoraLoader");
        assert_eq!(lora["inputs"]["lora_name"], "face.safetensors");
        assert_eq!(
            linked(&workflow, &lora["inputs"]["model"])["class_type"],
            "CheckpointLoaderSimple",
            "the main LoRA stays out of the detailer"
        );
        let positive = linked(&workflow, &face["inputs"]["positive"]);
        assert_eq!(positive["inputs"]["text"], "Person, cute girl");
        assert_eq!(positive["inputs"]["clip"][0], face["inputs"]["model"][0]);
        let negative = linked(&workflow, &face["inputs"]["negative"]);
        assert_eq!(negative["inputs"]["text"], "worst quality, text, logo");
    }

    #[test]
    fn empty_detail_prompt_reuses_the_main_prompt() {
        let mut finish = all_on();
        finish.face_prompt = String::new();
        let workflow = build(&moody("txt2img", finish));
        let face = single(&workflow, "MooshieFaceDetailer");
        let positive = linked(&workflow, &face["inputs"]["positive"]);
        assert_eq!(positive["inputs"]["text"], "1girl, smiling");
    }

    #[test]
    fn seedvr2_options_pick_the_model_and_skip_an_empty_blend() {
        let finish = MoodyMossFinish {
            seedvr2: true,
            seedvr2_7b: true,
            seedvr2_mix: 0.0,
            long_edge: 3072,
            ..MoodyMossFinish::default()
        };
        let workflow = build(&moody("txt2img", finish));
        let post = saved_from(&workflow);
        assert_eq!(post["class_type"], "SeedVR2PostProcessing");
        assert!(nodes(&workflow, "UpscaleModelLoader").is_empty());
        assert!(nodes(&workflow, "ImageBlend").is_empty());
        assert_eq!(
            single(&workflow, "UNETLoader")["inputs"]["unet_name"],
            SEEDVR2_7B_UNET_FILE
        );
        assert_eq!(
            single(&workflow, "ImageScaleToMaxDimension")["inputs"]["largest_size"],
            json!(3072)
        );
    }

    #[test]
    fn finishing_needs_a_moss_txt2img_request() {
        let mut other = moody("txt2img", all_on());
        other.model_architecture = "sdxl".to_string();
        let img2img = moody("img2img", all_on());
        for p in [other, img2img] {
            let workflow = build(&p);
            assert!(nodes(&workflow, "MooshieFaceDetailer").is_empty());
            assert!(nodes(&workflow, "MooshieSkinContrast").is_empty());
            assert!(nodes(&workflow, "SeedVR2PostProcessing").is_empty());
        }
        assert!(nodes(
            &build(&params("txt2img", "mossimage")),
            "MooshieSkinContrast"
        )
        .is_empty());
    }

    #[test]
    fn area_detail_details_the_painted_mask_and_only_saves() {
        let mut p = moody(
            "inpainting",
            MoodyMossFinish {
                area_detail: true,
                area_prompt: "detailed hands".to_string(),
                ..MoodyMossFinish::default()
            },
        );
        p.upscale_enabled = true;
        p.facefix_enabled = true;
        let workflow = build(&p);

        let detailer = saved_from(&workflow);
        assert_eq!(detailer["class_type"], "MooshieMaskDetailer");
        assert_eq!(detailer["inputs"]["denoise"], json!(0.45));
        assert_eq!(detailer["inputs"]["crop_factor"], json!(6.1));
        assert_eq!(detailer["inputs"]["mask_dilation"], json!(8));
        assert_eq!(
            linked(&workflow, &detailer["inputs"]["image"])["inputs"]["image"],
            "input.png"
        );
        assert_eq!(
            linked(&workflow, &detailer["inputs"]["mask"])["inputs"]["image"],
            "mask.png"
        );
        assert_eq!(
            linked(&workflow, &detailer["inputs"]["positive"])["inputs"]["text"],
            "detailed hands"
        );
        assert!(nodes(&workflow, "KSampler").is_empty());
        assert!(nodes(&workflow, "MooshieFaceDetailer").is_empty());
        assert!(nodes(&workflow, "ImageScaleBy").is_empty());
    }

    #[test]
    fn area_detail_off_is_a_normal_inpaint() {
        let workflow = build(&moody("inpainting", all_on()));
        assert!(nodes(&workflow, "MooshieMaskDetailer").is_empty());
        assert_eq!(nodes(&workflow, "KSampler").len(), 1);
    }
}
