use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

/// Appends MooshieFaceDetailer node to an existing workflow.
/// Returns the (node_id, output_index) of the final IMAGE with fixed faces.
///
/// Uses our bundled mooshie-nodes custom node which handles YOLOv8 detection,
/// per-face cropping, re-denoising, and compositing in a single node.
pub fn append_facefix_chain(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    current_image: (String, u32),
    seed: i64,
) -> (String, u32) {
    let next_id = &mut result.next_id;
    let workflow = &mut result.workflow;

    let detector_model = params
        .facefix_detector
        .as_deref()
        .unwrap_or("Anzhc Face seg 640 v4 y11n.pt");

    // Optionally condition the detailer on a face-only subset of the prompt so
    // scene/pose/background tags don't bleed into the re-denoised face. Falls back
    // to the full positive conditioning when the prompt has no face-relevant tags.
    let positive_source = if params.facefix_auto_prompt {
        let face_prompt =
            crate::prompt_assistant::grounding::extract_face_tags(&params.positive_prompt);
        if face_prompt.trim().is_empty() {
            (result.positive_source.0.clone(), result.positive_source.1)
        } else {
            let encode_id = next_id.to_string();
            workflow.insert(
                encode_id.clone(),
                json!({
                    "class_type": "CLIPTextEncode",
                    "inputs": {
                        "clip": [result.clip_source.0.clone(), result.clip_source.1],
                        "text": face_prompt
                    }
                }),
            );
            *next_id += 1;
            (encode_id, 0)
        }
    } else {
        (result.positive_source.0.clone(), result.positive_source.1)
    };

    let detailer_id = next_id.to_string();
    workflow.insert(
        detailer_id.clone(),
        json!({
            "class_type": "MooshieFaceDetailer",
            "inputs": {
                "image": [current_image.0, current_image.1],
                "model": [result.model_source.0.clone(), result.model_source.1],
                "vae": [result.vae_source.0.clone(), result.vae_source.1],
                "positive": [positive_source.0, positive_source.1],
                "negative": [result.negative_source.0.clone(), result.negative_source.1],
                "detector_model": detector_model,
                "seed": seed + 2,
                "steps": params.facefix_steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": params.facefix_denoise,
                "guide_size": params.facefix_guide_size,
                "bbox_threshold": 0.5,
                "bbox_padding": 1.5,
                "feather": 20,
                "max_faces": params.facefix_max_faces
            }
        }),
    );
    *next_id += 1;

    (detailer_id, 0)
}
