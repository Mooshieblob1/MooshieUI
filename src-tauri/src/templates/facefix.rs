use serde_json::{json, Map, Value};

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

/// Parse a `[node_id, output]` link.
fn link(value: &Value) -> Option<(String, u32)> {
    let node = value.get(0)?.as_str()?.to_string();
    let output = u32::try_from(value.get(1)?.as_u64()?).ok()?;
    Some((node, output))
}

/// Conditioning for a face crop, without the whole-frame layers stacked on the
/// generation's conditioning: a ControlNet hint (`ControlNetApplyAdvanced`) and
/// regional areas (`ConditioningCombine` with a `ConditioningSetAreaPercentage`).
///
/// Both are laid out for the full image. The detailer rescales them onto each
/// face crop, which pastes the whole pose/depth map, or a region meant for
/// another part of the picture, over the face. Walking back past them leaves
/// the CLIP-encoded prompt the template built. Wrappers that are not spatial
/// (`FluxGuidance`, a Flux Redux `StyleModelApply`) are kept, re-applied to
/// the stripped conditioning when a spatial layer sat underneath. Conditioning
/// without spatial layers comes back unchanged.
///
/// Shared with the `<segment:...>` detailer, which crops the same way.
pub(super) fn without_spatial_conditioning(
    workflow: &mut Map<String, Value>,
    next_id: &mut u32,
    source: (String, u32),
) -> (String, u32) {
    let Some(node) = workflow.get(&source.0).cloned() else {
        return source;
    };
    let inputs = &node["inputs"];
    match node["class_type"].as_str().unwrap_or_default() {
        "ControlNetApplyAdvanced" => {
            let side = if source.1 == 1 {
                "negative"
            } else {
                "positive"
            };
            match link(&inputs[side]) {
                Some(inner) => without_spatial_conditioning(workflow, next_id, inner),
                None => source,
            }
        }
        "ConditioningCombine" => {
            let is_region = link(&inputs["conditioning_2"])
                .and_then(|(id, _)| workflow.get(&id))
                .is_some_and(|area| area["class_type"] == "ConditioningSetAreaPercentage");
            match link(&inputs["conditioning_1"]).filter(|_| is_region) {
                Some(inner) => without_spatial_conditioning(workflow, next_id, inner),
                None => source,
            }
        }
        "FluxGuidance" | "StyleModelApply" => {
            let Some(inner) = link(&inputs["conditioning"]) else {
                return source;
            };
            let stripped = without_spatial_conditioning(workflow, next_id, inner.clone());
            if stripped == inner {
                return source;
            }
            let mut rebuilt = node;
            rebuilt["inputs"]["conditioning"] = json!([stripped.0, stripped.1]);
            let id = next_id.to_string();
            workflow.insert(id.clone(), rebuilt);
            *next_id += 1;
            (id, 0)
        }
        _ => source,
    }
}

/// The model before an Anima ControlNet-LLLite patch, which (like a ControlNet
/// hint) steers the whole frame and would be rescaled onto each face crop.
pub(super) fn without_full_image_model_patch(
    workflow: &Map<String, Value>,
    model: (String, u32),
) -> (String, u32) {
    workflow
        .get(&model.0)
        .filter(|node| node["class_type"] == "AnimaLLLiteApply")
        .and_then(|node| link(&node["inputs"]["model"]))
        .unwrap_or(model)
}

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
    let refiner_model = without_full_image_model_patch(&result.workflow, result.refiner_model());
    let next_id = &mut result.next_id;
    let workflow = &mut result.workflow;
    let face_negative =
        without_spatial_conditioning(workflow, next_id, result.negative_source.clone());

    let detector_model = params
        .facefix_detector
        .as_deref()
        .unwrap_or("Anzhc Face seg 640 v4 y11n.pt");

    // Optionally condition the detailer on a face-only subset of the prompt so
    // scene/pose/background tags don't bleed into the re-denoised face. Falls back
    // to the full positive conditioning when the prompt has no face-relevant tags.
    // An override is a decision already made by whoever set it (the NovelAI
    // face-detail panel), including its emptiness, so it skips both the
    // extraction and the full-prompt fallback below.
    let positive_source = if let Some(face_prompt) = params.facefix_prompt_override.clone() {
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
    } else if params.facefix_auto_prompt {
        let face_prompt =
            crate::prompt_assistant::grounding::extract_face_tags(&params.positive_prompt);
        if face_prompt.trim().is_empty() {
            without_spatial_conditioning(workflow, next_id, result.positive_source.clone())
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
        without_spatial_conditioning(workflow, next_id, result.positive_source.clone())
    };

    let detailer_id = next_id.to_string();
    workflow.insert(
        detailer_id.clone(),
        json!({
            "class_type": "MooshieFaceDetailer",
            "inputs": {
                "image": [current_image.0, current_image.1],
                "model": [refiner_model.0, refiner_model.1],
                "vae": [result.vae_source.0.clone(), result.vae_source.1],
                "positive": [positive_source.0, positive_source.1],
                "negative": [face_negative.0, face_negative.1],
                "detector_model": detector_model,
                "seed": super::offset_seed(seed, 2),
                "steps": params.facefix_steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": params.facefix_denoise,
                "guide_size": params.facefix_guide_size,
                "bbox_threshold": params.facefix_bbox_threshold,
                "bbox_padding": params.facefix_bbox_padding,
                "feather": params.facefix_feather,
                "max_faces": params.facefix_max_faces
            }
        }),
    );
    *next_id += 1;

    (detailer_id, 0)
}

#[cfg(test)]
mod tests {
    use crate::comfyui::types::{ControlNetParam, GenerationParams, PositiveRegion};
    use crate::templates::graph_test_util::{build, linked, params, single};

    fn with_controlnet(mut p: GenerationParams) -> GenerationParams {
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
        p
    }

    fn with_facefix(mut p: GenerationParams) -> GenerationParams {
        p.facefix_enabled = true;
        p
    }

    #[test]
    fn detailer_skips_the_full_image_controlnet() {
        let workflow = build(&with_facefix(with_controlnet(params("txt2img", "sdxl"))));

        // The main sampler still uses the ControlNet conditioning.
        let sampler = single(&workflow, "KSampler");
        let cn = linked(&workflow, &sampler["inputs"]["positive"]);
        assert_eq!(cn["class_type"], "ControlNetApplyAdvanced");

        // The face crops get the plain prompt encodes it was built from.
        let detailer = single(&workflow, "MooshieFaceDetailer");
        assert_eq!(detailer["inputs"]["positive"], cn["inputs"]["positive"]);
        assert_eq!(detailer["inputs"]["negative"], cn["inputs"]["negative"]);
        let positive = linked(&workflow, &detailer["inputs"]["positive"]);
        assert_eq!(positive["class_type"], "CLIPTextEncode");
        assert_eq!(positive["inputs"]["text"], "1girl, smiling");
        let negative = linked(&workflow, &detailer["inputs"]["negative"]);
        assert_eq!(negative["inputs"]["text"], "blurry");
    }

    #[test]
    fn detailer_skips_regional_areas() {
        let mut p = with_facefix(params("txt2img", "sdxl"));
        p.positive_regions = vec![PositiveRegion {
            text: "1girl, smiling, red hair".to_string(),
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
            strength: 1.0,
        }];
        let workflow = build(&p);

        let sampler = single(&workflow, "KSampler");
        assert_eq!(
            linked(&workflow, &sampler["inputs"]["positive"])["class_type"],
            "ConditioningCombine"
        );
        let detailer = single(&workflow, "MooshieFaceDetailer");
        let positive = linked(&workflow, &detailer["inputs"]["positive"]);
        assert_eq!(positive["class_type"], "CLIPTextEncode");
        assert_eq!(positive["inputs"]["text"], "1girl, smiling");
    }

    #[test]
    fn detailer_keeps_the_sampler_conditioning_without_spatial_layers() {
        let workflow = build(&with_facefix(params("txt2img", "sdxl")));
        let sampler = single(&workflow, "KSampler");
        let detailer = single(&workflow, "MooshieFaceDetailer");
        assert_eq!(
            detailer["inputs"]["positive"],
            sampler["inputs"]["positive"]
        );
        assert_eq!(
            detailer["inputs"]["negative"],
            sampler["inputs"]["negative"]
        );
        assert_eq!(detailer["inputs"]["model"], sampler["inputs"]["model"]);
    }

    #[test]
    fn flux_detailer_keeps_guidance_and_redux_style_but_not_controlnet() {
        let mut p = with_facefix(with_controlnet(params("txt2img", "flux1d")));
        p.style_ref_enabled = true;
        p.style_ref_image = Some("style.png".to_string());
        p.style_ref_weight_type = "multiply".to_string();
        let workflow = build(&p);

        let detailer = single(&workflow, "MooshieFaceDetailer");
        let style = linked(&workflow, &detailer["inputs"]["positive"]);
        assert_eq!(style["class_type"], "StyleModelApply");
        let guidance = linked(&workflow, &style["inputs"]["conditioning"]);
        assert_eq!(guidance["class_type"], "FluxGuidance");
        assert_eq!(
            linked(&workflow, &guidance["inputs"]["conditioning"])["class_type"],
            "CLIPTextEncode"
        );

        // The main sampler's Redux node still wraps the ControlNet output.
        let sampler = single(&workflow, "KSampler");
        let sampler_style = linked(&workflow, &sampler["inputs"]["positive"]);
        assert_eq!(
            linked(&workflow, &sampler_style["inputs"]["conditioning"])["class_type"],
            "ControlNetApplyAdvanced"
        );
    }

    #[test]
    fn anima_detailer_uses_the_model_before_lllite() {
        let workflow = build(&with_facefix(with_controlnet(params("txt2img", "anima"))));

        let sampler = single(&workflow, "KSampler");
        let lllite = linked(&workflow, &sampler["inputs"]["model"]);
        assert_eq!(lllite["class_type"], "AnimaLLLiteApply");
        let detailer = single(&workflow, "MooshieFaceDetailer");
        assert_eq!(detailer["inputs"]["model"], lllite["inputs"]["model"]);
    }
}
