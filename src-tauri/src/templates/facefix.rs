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
        .unwrap_or("face_yolov8m.pt");

    let detailer_id = next_id.to_string();
    workflow.insert(
        detailer_id.clone(),
        json!({
            "class_type": "MooshieFaceDetailer",
            "inputs": {
                "image": [current_image.0, current_image.1],
                "model": [result.model_source.0.clone(), result.model_source.1],
                "vae": [result.vae_source.0.clone(), result.vae_source.1],
                "positive": [result.positive_source.0.clone(), result.positive_source.1],
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
                "feather": 20
            }
        }),
    );
    *next_id += 1;

    (detailer_id, 0)
}
