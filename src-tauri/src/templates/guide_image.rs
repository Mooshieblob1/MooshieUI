//! Guide image for txt2img: a model-agnostic composition and colour guide.
//!
//! ComfyUI's `LatentOperationBlend` (v0.40.0+) pulls each step's prediction
//! toward a reference latent, and `LatentApplyOperationCFG` limits it to the
//! start of the schedule. Sampling still begins from pure noise at full
//! denoise, so the layout and palette follow the reference while the detail
//! is new, unlike img2img. Both are core nodes and need no extra model, so
//! every family that samples through `KSampler` gets it.

use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

/// The guide image's filename, when the guide applies to this request.
pub fn guide_image(params: &GenerationParams) -> Option<&str> {
    if !params.guide_image_enabled || params.mode != "txt2img" {
        return None;
    }
    params
        .guide_image
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn add(result: &mut WorkflowResult, node: serde_json::Value) -> String {
    let id = result.next_id.to_string();
    result.workflow.insert(id.clone(), node);
    result.next_id += 1;
    id
}

/// Encode the guide image at the output size and patch the primary sampler's
/// model with the blend.
///
/// Only the primary sampler is rewired; `model_source` is left as it was, so
/// the hires, face and segment passes appended later never see a guide sized
/// for the base generation. Runs after every other model patch, so the
/// schedule percentages read the final model sampling (shift included).
pub fn inject_guide_image(result: &mut WorkflowResult, params: &GenerationParams) {
    let Some(image) = guide_image(params) else {
        return;
    };
    let strength = params.guide_strength.clamp(0.0, 1.0);
    let end_percent = params.guide_end_percent.clamp(0.0, 1.0);
    if strength <= 0.0 || end_percent <= 0.0 {
        return;
    }

    let vae = json!([result.vae_source.0.clone(), result.vae_source.1]);
    let load_id = add(
        result,
        json!({ "class_type": "LoadImage", "inputs": { "image": image } }),
    );
    // Cropped to fill the output frame, so the guide's layout is not squashed.
    let scale_id = add(
        result,
        json!({
            "class_type": "ImageScale",
            "inputs": {
                "image": [load_id, 0],
                "width": params.width,
                "height": params.height,
                "upscale_method": "lanczos",
                "crop": "center"
            }
        }),
    );
    let encode_id = add(
        result,
        json!({
            "class_type": "VAEEncode",
            "inputs": {
                "pixels": [scale_id, 0],
                "vae": vae
            }
        }),
    );
    let blend_id = add(
        result,
        json!({
            "class_type": "LatentOperationBlend",
            "inputs": { "reference": [encode_id, 0], "strength": strength }
        }),
    );

    let Some(sampler_model) = result
        .workflow
        .get(&result.sampler_id)
        .and_then(|node| node["inputs"].get("model"))
        .cloned()
    else {
        return;
    };
    let apply_id = add(
        result,
        json!({
            "class_type": "LatentApplyOperationCFG",
            "inputs": {
                "model": sampler_model,
                "operation": [blend_id, 0],
                "start_percent": 0.0,
                "end_percent": end_percent
            }
        }),
    );
    if let Some(inputs) = result
        .workflow
        .get_mut(&result.sampler_id)
        .and_then(|node| node.get_mut("inputs"))
    {
        inputs["model"] = json!([apply_id, 0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_result() -> WorkflowResult {
        let mut workflow = serde_json::Map::new();
        workflow.insert(
            "1".to_string(),
            json!({ "class_type": "KSampler", "inputs": { "model": ["7", 0] } }),
        );
        WorkflowResult {
            workflow,
            next_id: 10,
            image_output: ("2".to_string(), 0),
            model_source: ("0".to_string(), 0),
            clip_source: ("0".to_string(), 1),
            positive_source: ("3".to_string(), 0),
            negative_source: ("4".to_string(), 0),
            vae_source: ("0".to_string(), 2),
            sampler_id: "1".to_string(),
            refiner_model_source: None,
            base_sources: None,
        }
    }

    fn guide_params() -> GenerationParams {
        GenerationParams {
            mode: "txt2img".to_string(),
            guide_image_enabled: true,
            guide_image: Some("guide.png".to_string()),
            guide_strength: 0.6,
            guide_end_percent: 0.35,
            width: 832,
            height: 1216,
            ..Default::default()
        }
    }

    fn node_of<'a>(result: &'a WorkflowResult, class: &str) -> &'a serde_json::Value {
        result
            .workflow
            .values()
            .find(|n| n["class_type"] == class)
            .unwrap_or_else(|| panic!("no {class} node"))
    }

    #[test]
    fn wraps_the_samplers_model_and_leaves_model_source_alone() {
        let mut result = base_result();
        inject_guide_image(&mut result, &guide_params());

        let apply = node_of(&result, "LatentApplyOperationCFG");
        // Wraps whatever the sampler had (here a patched model "7"), not model_source.
        assert_eq!(apply["inputs"]["model"], json!(["7", 0]));
        assert_eq!(apply["inputs"]["start_percent"], json!(0.0));
        assert_eq!(apply["inputs"]["end_percent"], json!(0.35));

        let apply_id = result
            .workflow
            .iter()
            .find(|(_, n)| n["class_type"] == "LatentApplyOperationCFG")
            .map(|(id, _)| id.clone())
            .unwrap();
        assert_eq!(
            result.workflow["1"]["inputs"]["model"],
            json!([apply_id, 0])
        );
        assert_eq!(result.model_source, ("0".to_string(), 0));
    }

    #[test]
    fn encodes_the_guide_at_the_output_size_with_the_run_vae() {
        let mut result = base_result();
        inject_guide_image(&mut result, &guide_params());

        let load = node_of(&result, "LoadImage");
        assert_eq!(load["inputs"]["image"], json!("guide.png"));
        let scale = node_of(&result, "ImageScale");
        assert_eq!(scale["inputs"]["width"], json!(832));
        assert_eq!(scale["inputs"]["height"], json!(1216));
        assert_eq!(scale["inputs"]["crop"], json!("center"));
        let encode = node_of(&result, "VAEEncode");
        assert_eq!(encode["inputs"]["vae"], json!(["0", 2]));
        let blend = node_of(&result, "LatentOperationBlend");
        assert_eq!(blend["inputs"]["strength"], json!(0.6));
    }

    #[test]
    fn skipped_when_off_empty_zero_or_outside_txt2img() {
        let cases: [fn(&mut GenerationParams); 7] = [
            |p| p.guide_image_enabled = false,
            |p| p.guide_image = None,
            |p| p.guide_image = Some("  ".to_string()),
            |p| p.guide_strength = 0.0,
            |p| p.guide_end_percent = 0.0,
            |p| p.mode = "img2img".to_string(),
            |p| p.mode = "image_edit".to_string(),
        ];
        for tweak in cases {
            let mut params = guide_params();
            tweak(&mut params);
            let mut result = base_result();
            inject_guide_image(&mut result, &params);
            assert_eq!(result.workflow.len(), 1, "guide nodes added for {params:?}");
            assert_eq!(result.workflow["1"]["inputs"]["model"], json!(["7", 0]));
        }
    }

    #[test]
    fn clamps_out_of_range_values() {
        let mut params = guide_params();
        params.guide_strength = 3.0;
        params.guide_end_percent = 1.5;
        let mut result = base_result();
        inject_guide_image(&mut result, &params);
        assert_eq!(
            node_of(&result, "LatentOperationBlend")["inputs"]["strength"],
            json!(1.0)
        );
        assert_eq!(
            node_of(&result, "LatentApplyOperationCFG")["inputs"]["end_percent"],
            json!(1.0)
        );
    }

    #[test]
    fn full_build_wraps_the_flux_shifted_model_for_the_main_sampler_only() {
        let mut params = guide_params();
        params.model_architecture = "flux1d".to_string();
        params.checkpoint = "flux1-dev.safetensors".to_string();
        params.steps = 20;
        params.cfg = 1.0;
        params.sampler_name = "euler".to_string();
        params.scheduler = "simple".to_string();
        let workflow = super::super::build_workflow(&params, 1, false);
        let nodes = workflow.as_object().unwrap();

        let (_, sampler) = nodes
            .iter()
            .find(|(_, n)| n["class_type"] == "KSampler")
            .expect("KSampler");
        let apply_id = sampler["inputs"]["model"][0].as_str().unwrap();
        assert_eq!(nodes[apply_id]["class_type"], "LatentApplyOperationCFG");
        let wrapped = nodes[apply_id]["inputs"]["model"][0].as_str().unwrap();
        assert_eq!(nodes[wrapped]["class_type"], "ModelSamplingFlux");
    }
}
