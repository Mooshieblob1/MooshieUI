use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::ControlNetParam;

/// Inject ControlNet nodes into an existing workflow.
/// Rewires positive_source and negative_source so downstream nodes
/// (KSampler, upscale) automatically use the ControlNet-conditioned output.
pub fn inject_controlnet(result: &mut WorkflowResult, params: &ControlNetParam) {
    let next_id = &mut result.next_id;
    let workflow = &mut result.workflow;

    // 1. LoadImage — the control/reference image
    let load_img_id = next_id.to_string();
    workflow.insert(
        load_img_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": {
                "image": params.image.as_deref().unwrap_or("")
            }
        }),
    );
    *next_id += 1;

    // 2. Preprocessor (optional — only when a preset with preprocessing is selected)
    let image_source: (String, u32) = if let Some(ref preprocessor) = params.preprocessor {
        if !preprocessor.is_empty() {
            let preprocess_id = next_id.to_string();
            workflow.insert(
                preprocess_id.clone(),
                json!({
                    "class_type": preprocessor,
                    "inputs": {
                        "image": [load_img_id, 0],
                        "resolution": 1024
                    }
                }),
            );
            *next_id += 1;
            (preprocess_id, 0)
        } else {
            (load_img_id, 0)
        }
    } else {
        (load_img_id, 0)
    };

    // 3. ControlNetLoader
    let cn_loader_id = next_id.to_string();
    workflow.insert(
        cn_loader_id.clone(),
        json!({
            "class_type": "ControlNetLoader",
            "inputs": {
                "control_net_name": params.controlnet_model.as_deref().unwrap_or("")
            }
        }),
    );
    *next_id += 1;

    // 4. ControlNetApplyAdvanced — rewires positive/negative conditioning
    //    Outputs: slot 0 = positive, slot 1 = negative
    let cn_apply_id = next_id.to_string();
    workflow.insert(
        cn_apply_id.clone(),
        json!({
            "class_type": "ControlNetApplyAdvanced",
            "inputs": {
                "positive": [result.positive_source.0.clone(), result.positive_source.1],
                "negative": [result.negative_source.0.clone(), result.negative_source.1],
                "control_net": [cn_loader_id, 0],
                "image": [image_source.0, image_source.1],
                "strength": params.strength,
                "start_percent": params.start_percent,
                "end_percent": params.end_percent
            }
        }),
    );
    *next_id += 1;

    // Update sources so KSampler (and upscale) use ControlNet output
    result.positive_source = (cn_apply_id.clone(), 0);
    result.negative_source = (cn_apply_id, 1);
}
