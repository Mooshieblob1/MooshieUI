use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

/// Appends one MooshieSegmentDetailer per `<segment:...>` tag, in prompt order,
/// each with its own CLIPTextEncode (global regional context + segment prompt).
/// Returns the (node_id, output_index) of the final refined IMAGE.
///
/// Like the face detailer, each segment is re-sampled on a crop, so the
/// whole-frame layers of the generation (a ControlNet hint on the negative, an
/// Anima ControlNet-LLLite model patch) are left out rather than rescaled onto
/// the crop.
pub fn append_segment_chain(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    current_image: (String, u32),
    seed: i64,
) -> (String, u32) {
    let context = super::build_regional_context_prompt(params);
    let mut image = current_image;
    let refiner_model =
        super::facefix::without_full_image_model_patch(&result.workflow, result.refiner_model());
    let negative = super::facefix::without_spatial_conditioning(
        &mut result.workflow,
        &mut result.next_id,
        result.negative_source.clone(),
    );

    for (i, segment) in params.detail_segments.iter().enumerate() {
        // Core CLIPTextEncode would read a `<lora:...>` tag as literal text.
        let encode_text = super::strip_lora_tags(&super::merge_regional_encode_text(
            &context,
            &segment.prompt,
        ));

        let clip_id = result.next_id.to_string();
        result.workflow.insert(
            clip_id.clone(),
            json!({
                "class_type": "CLIPTextEncode",
                "inputs": {
                    "clip": [result.clip_source.0.clone(), result.clip_source.1],
                    "text": encode_text
                }
            }),
        );
        result.next_id += 1;

        let detailer_id = result.next_id.to_string();
        result.workflow.insert(
            detailer_id.clone(),
            json!({
                "class_type": "MooshieSegmentDetailer",
                "inputs": {
                    "image": [image.0, image.1],
                    "model": [refiner_model.0.clone(), refiner_model.1],
                    "vae": [result.vae_source.0.clone(), result.vae_source.1],
                    "positive": [clip_id, 0],
                    "negative": [negative.0.clone(), negative.1],
                    "detection": segment.target,
                    // seed+2 is taken by facefix
                    "seed": super::offset_seed(seed, 3 + i as u64),
                    "steps": params.facefix_steps,
                    "cfg": params.cfg,
                    "sampler_name": params.sampler_name,
                    "scheduler": params.scheduler,
                    "denoise": segment.creativity,
                    "guide_size": params.facefix_guide_size,
                    "threshold": segment.threshold,
                    "mask_grow": 16,
                    "mask_blur": 8
                }
            }),
        );
        result.next_id += 1;

        image = (detailer_id, 0);
    }

    image
}

#[cfg(test)]
mod tests {
    use crate::comfyui::types::{ControlNetParam, DetailSegment, GenerationParams};
    use crate::templates::graph_test_util::{build, linked, params, single};

    fn with_controlnet_and_segment(mut p: GenerationParams) -> GenerationParams {
        p.controlnet = Some(ControlNetParam {
            enabled: true,
            preset: None,
            controlnet_model: Some("pose.safetensors".to_string()),
            image: Some("pose.png".to_string()),
            preprocessor: None,
            strength: 1.0,
            start_percent: 0.0,
            end_percent: 1.0,
        });
        p.detail_segments = vec![DetailSegment {
            target: "hand".to_string(),
            prompt: "detailed hand".to_string(),
            creativity: 0.4,
            threshold: 0.5,
        }];
        p
    }

    #[test]
    fn segment_detailer_skips_the_full_image_controlnet() {
        let workflow = build(&with_controlnet_and_segment(params("txt2img", "sdxl")));

        let sampler = single(&workflow, "KSampler");
        let cn = linked(&workflow, &sampler["inputs"]["negative"]);
        assert_eq!(cn["class_type"], "ControlNetApplyAdvanced");

        let detailer = single(&workflow, "MooshieSegmentDetailer");
        assert_eq!(detailer["inputs"]["negative"], cn["inputs"]["negative"]);
        let negative = linked(&workflow, &detailer["inputs"]["negative"]);
        assert_eq!(negative["class_type"], "CLIPTextEncode");
        assert_eq!(negative["inputs"]["text"], "blurry");
        assert_eq!(detailer["inputs"]["model"], sampler["inputs"]["model"]);
    }

    #[test]
    fn anima_segment_detailer_uses_the_model_before_lllite() {
        let workflow = build(&with_controlnet_and_segment(params("txt2img", "anima")));

        let sampler = single(&workflow, "KSampler");
        let lllite = linked(&workflow, &sampler["inputs"]["model"]);
        assert_eq!(lllite["class_type"], "AnimaLLLiteApply");
        let detailer = single(&workflow, "MooshieSegmentDetailer");
        assert_eq!(detailer["inputs"]["model"], lllite["inputs"]["model"]);
    }
}
