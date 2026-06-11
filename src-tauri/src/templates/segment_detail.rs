use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

/// Appends one MooshieSegmentDetailer per `<segment:...>` tag, in prompt order,
/// each with its own CLIPTextEncode (global regional context + segment prompt).
/// Returns the (node_id, output_index) of the final refined IMAGE.
pub fn append_segment_chain(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    current_image: (String, u32),
    seed: i64,
) -> (String, u32) {
    let context = super::build_regional_context_prompt(params);
    let mut image = current_image;

    for (i, segment) in params.detail_segments.iter().enumerate() {
        let encode_text = super::merge_regional_encode_text(&context, &segment.prompt);

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
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "vae": [result.vae_source.0.clone(), result.vae_source.1],
                    "positive": [clip_id, 0],
                    "negative": [result.negative_source.0.clone(), result.negative_source.1],
                    "detection": segment.target,
                    // seed+2 is taken by facefix
                    "seed": seed + 3 + i as i64,
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
