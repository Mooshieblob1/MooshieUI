use serde_json::{json, Map, Value};

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
                    "inputs": preprocessor_inputs(preprocessor, &load_img_id, 0)
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

/// Inject Anima ControlNet-LLLite via kohya-ss/ComfyUI-Anima-LLLite.
/// Unlike standard ControlNet, the LLLite node patches MODEL and returns MODEL.
pub fn inject_anima_lllite(
    result: &mut WorkflowResult,
    params: &ControlNetParam,
    mask_image: Option<&str>,
) {
    let model_source = current_sampler_model_source(result);
    let next_id = &mut result.next_id;
    let workflow = &mut result.workflow;

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

    let image_source: (String, u32) = if let Some(ref preprocessor) = params.preprocessor {
        if !preprocessor.is_empty() {
            let preprocess_id = next_id.to_string();
            workflow.insert(
                preprocess_id.clone(),
                json!({
                    "class_type": preprocessor,
                    "inputs": preprocessor_inputs(preprocessor, &load_img_id, 0)
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

    let needs_mask = params
        .preset
        .as_deref()
        .is_some_and(|preset| preset == "inpainting")
        || params
            .controlnet_model
            .as_deref()
            .is_some_and(|name| name.to_lowercase().contains("inpainting"));

    let mask_source = if needs_mask {
        mask_image
            .filter(|image| !image.trim().is_empty())
            .map(|image| {
                let load_mask_id = next_id.to_string();
                workflow.insert(
                    load_mask_id.clone(),
                    json!({
                        "class_type": "LoadImageMask",
                        "inputs": {
                            "image": image,
                            "channel": "red"
                        }
                    }),
                );
                *next_id += 1;
                (load_mask_id, 0u32)
            })
    } else {
        None
    };

    let mut inputs = serde_json::Map::new();
    inputs.insert("model".into(), json!([model_source.0, model_source.1]));
    inputs.insert(
        "lllite_name".into(),
        json!(params.controlnet_model.as_deref().unwrap_or("")),
    );
    inputs.insert("image".into(), json!([image_source.0, image_source.1]));
    inputs.insert("strength".into(), json!(params.strength));
    inputs.insert("start_percent".into(), json!(params.start_percent));
    inputs.insert("end_percent".into(), json!(params.end_percent));
    if let Some((mask_node, mask_output)) = mask_source {
        inputs.insert("mask".into(), json!([mask_node, mask_output]));
    }

    let apply_id = next_id.to_string();
    workflow.insert(
        apply_id.clone(),
        json!({
            "class_type": "AnimaLLLiteApply",
            "inputs": inputs
        }),
    );
    *next_id += 1;

    result.model_source = (apply_id, 0);
    rewire_sampler_model(result);
}

fn current_sampler_model_source(result: &WorkflowResult) -> (String, u32) {
    result
        .workflow
        .get(&result.sampler_id)
        .and_then(|node| node.get("inputs"))
        .and_then(|inputs| inputs.get("model"))
        .and_then(|model| model.as_array())
        .and_then(|source| {
            let node_id = source.first()?.as_str()?.to_string();
            let output = source.get(1)?.as_u64()? as u32;
            Some((node_id, output))
        })
        .unwrap_or_else(|| result.model_source.clone())
}

fn rewire_sampler_model(result: &mut WorkflowResult) {
    if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
        if let Some(inputs) = sampler_node.get_mut("inputs") {
            inputs["model"] = json!([result.model_source.0, result.model_source.1]);
        }
    }
}

fn preprocessor_inputs(
    preprocessor: &str,
    image_node: &str,
    image_output: u32,
) -> serde_json::Map<String, Value> {
    let mut inputs = serde_json::Map::new();
    inputs.insert("image".into(), json!([image_node, image_output]));
    inputs.insert("resolution".into(), json!(1024));

    match preprocessor {
        "OpenposePreprocessor" => {
            inputs.insert("detect_hand".into(), json!("enable"));
            inputs.insert("detect_body".into(), json!("enable"));
            inputs.insert("detect_face".into(), json!("enable"));
            inputs.insert("scale_stick_for_xinsr_cn".into(), json!("disable"));
        }
        "DWPreprocessor" => {
            inputs.insert("detect_hand".into(), json!("enable"));
            inputs.insert("detect_body".into(), json!("enable"));
            inputs.insert("detect_face".into(), json!("enable"));
            inputs.insert("bbox_detector".into(), json!("yolox_l.onnx"));
            inputs.insert("pose_estimator".into(), json!("dw-ll_ucoco_384.onnx"));
            inputs.insert("scale_stick_for_xinsr_cn".into(), json!("disable"));
        }
        "LineArtPreprocessor" => {
            inputs.insert("coarse".into(), json!("disable"));
        }
        "HEDPreprocessor" | "FakeScribblePreprocessor" => {
            inputs.insert("safe".into(), json!("enable"));
        }
        _ => {}
    }

    inputs
}

fn append_preprocessor_preview_save(
    workflow: &mut Map<String, Value>,
    next_id: &mut u32,
    image_source: &(String, u32),
) {
    let save_id = next_id.to_string();
    workflow.insert(
        save_id,
        json!({
            "class_type": "MooshieSaveImage",
            "inputs": {
                "images": [image_source.0.clone(), image_source.1],
                "bit_depth": "8bit",
                "output_format": "png",
                "output_role": "controlnet_preprocessor"
            }
        }),
    );
    *next_id += 1;
}

pub fn build_preprocessor_preview_workflow(image: &str, preprocessor: &str) -> Value {
    let mut workflow = Map::new();
    let mut next_id = 1u32;

    let load_img_id = next_id.to_string();
    workflow.insert(
        load_img_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": {
                "image": image
            }
        }),
    );
    next_id += 1;

    let image_source = if preprocessor.trim().is_empty() {
        (load_img_id, 0)
    } else {
        let preprocess_id = next_id.to_string();
        workflow.insert(
            preprocess_id.clone(),
            json!({
                "class_type": preprocessor,
                "inputs": preprocessor_inputs(preprocessor, &load_img_id, 0)
            }),
        );
        next_id += 1;
        (preprocess_id, 0)
    };

    append_preprocessor_preview_save(&mut workflow, &mut next_id, &image_source);

    Value::Object(workflow)
}
