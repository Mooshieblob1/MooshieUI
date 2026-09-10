//! Style reference injection: Flux Redux (core nodes) and IP-Adapter+ (SD1.5/SDXL).
//!
//! Flux Redux: StyleModelLoader + CLIPVisionLoader + CLIPVisionEncode + StyleModelApply
//! IP-Adapter: IPAdapterUnifiedLoader + IPAdapter (IPAdapterAdvanced)
//! Both insert into txt2img, img2img, and inpainting workflows (not video, not image_edit).

use serde_json::json;

use crate::comfyui::types::GenerationParams;

use super::WorkflowResult;

/// Supported families for style reference.
fn is_flux1_family(arch: &str) -> bool {
    matches!(arch, "flux1d" | "flux1s" | "flux1krea")
}

fn is_sd15_family(arch: &str) -> bool {
    arch == "sd15"
}

fn is_sdxl_family(arch: &str) -> bool {
    matches!(arch, "sdxl" | "illustrious" | "pony")
}

/// True when the current model family supports style reference.
pub fn family_supports_style_ref(arch: &str) -> bool {
    is_flux1_family(arch) || is_sd15_family(arch) || is_sdxl_family(arch)
}

/// Inject Flux Redux style reference into the workflow.
/// Appends nodes and updates result.positive_source to the styled conditioning.
pub fn inject_flux_redux(result: &mut WorkflowResult, params: &GenerationParams) {
    let image = params
        .style_ref_image
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");

    let redux_model = params
        .style_ref_redux_model
        .as_deref()
        .unwrap_or("flux1-redux-dev.safetensors");

    let clip_vision = params
        .style_ref_clip_vision
        .as_deref()
        .unwrap_or("sigclip_vision_patch14_384.safetensors");

    let mut next_id = result.next_id;

    // StyleModelLoader
    let style_model_id = next_id.to_string();
    result.workflow.insert(
        style_model_id.clone(),
        json!({
            "class_type": "StyleModelLoader",
            "inputs": {
                "style_model_name": redux_model
            }
        }),
    );
    next_id += 1;

    // CLIPVisionLoader
    let clip_vision_loader_id = next_id.to_string();
    result.workflow.insert(
        clip_vision_loader_id.clone(),
        json!({
            "class_type": "CLIPVisionLoader",
            "inputs": {
                "clip_name": clip_vision
            }
        }),
    );
    next_id += 1;

    // LoadImage
    let load_image_id = next_id.to_string();
    result.workflow.insert(
        load_image_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": {
                "image": image
            }
        }),
    );
    next_id += 1;

    // CLIPVisionEncode
    let clip_vision_encode_id = next_id.to_string();
    result.workflow.insert(
        clip_vision_encode_id.clone(),
        json!({
            "class_type": "CLIPVisionEncode",
            "inputs": {
                "clip_vision": [clip_vision_loader_id, 0],
                "image": [load_image_id, 0],
                "crop": "center"
            }
        }),
    );
    next_id += 1;

    // StyleModelApply — applies style conditioning to the positive conditioning.
    let strength_type = params.style_ref_weight_type.as_str();
    let style_apply_id = next_id.to_string();
    result.workflow.insert(
        style_apply_id.clone(),
        json!({
            "class_type": "StyleModelApply",
            "inputs": {
                "conditioning": [result.positive_source.0.clone(), result.positive_source.1],
                "style_model": [style_model_id, 0],
                "clip_vision_output": [clip_vision_encode_id, 0],
                "strength": params.style_ref_strength,
                "strength_type": strength_type
            }
        }),
    );
    next_id += 1;

    // Update positive_source to use the styled conditioning
    result.positive_source = (style_apply_id, 0);
    result.next_id = next_id;

    // Rewire the sampler to use the new positive source
    if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
        if let Some(inputs) = sampler_node.get_mut("inputs") {
            inputs["positive"] = json!([result.positive_source.0, result.positive_source.1]);
        }
    }
}

/// Inject IP-Adapter style reference into the workflow.
/// Uses IPAdapterUnifiedLoader + IPAdapterAdvanced (from ComfyUI_IPAdapter_plus).
/// The adapter wraps the model; patched model is used for sampling.
pub fn inject_ipadapter(result: &mut WorkflowResult, params: &GenerationParams) {
    let image = params
        .style_ref_image
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");

    let arch = &params.model_architecture;
    // PLUS (high strength) works for both SD1.5 and SDXL via ViT-H
    let preset = "PLUS (high strength)";

    let mut next_id = result.next_id;

    // LoadImage for style reference
    let load_image_id = next_id.to_string();
    result.workflow.insert(
        load_image_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": {
                "image": image
            }
        }),
    );
    next_id += 1;

    // IPAdapterUnifiedLoader: auto-selects adapter + clip_vision based on preset
    let loader_id = next_id.to_string();
    result.workflow.insert(
        loader_id.clone(),
        json!({
            "class_type": "IPAdapterUnifiedLoader",
            "inputs": {
                "model": [result.model_source.0.clone(), result.model_source.1],
                "preset": preset
            }
        }),
    );
    next_id += 1;

    // IPAdapterAdvanced: applies IP-Adapter conditioning to the model
    let weight_type = params.style_ref_weight_type.as_str();
    let apply_id = next_id.to_string();
    result.workflow.insert(
        apply_id.clone(),
        json!({
            "class_type": "IPAdapterAdvanced",
            "inputs": {
                "model": [loader_id.clone(), 0],
                "ipadapter": [loader_id.clone(), 1],
                "image": [load_image_id, 0],
                "weight": params.style_ref_strength,
                "weight_type": weight_type,
                "combine_embeds": "concat",
                "start_at": params.style_ref_start,
                "end_at": params.style_ref_end,
                "embeds_scaling": "V only"
            }
        }),
    );
    next_id += 1;

    // Update model_source to the patched model
    let old_model_source = result.model_source.clone();
    result.model_source = (apply_id.clone(), 0);
    result.next_id = next_id;

    // Rewire the sampler to use the patched model
    if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
        if let Some(inputs) = sampler_node.get_mut("inputs") {
            inputs["model"] = json!([result.model_source.0, result.model_source.1]);
        }
    }

    let _ = old_model_source;
    let _ = arch;
}

/// Main entry point: dispatch to the correct injector based on model family.
/// Returns Ok(()) if injected, Err(msg) if the family is unsupported.
pub fn inject_style_ref(result: &mut WorkflowResult, params: &GenerationParams) {
    let arch = params.model_architecture.as_str();
    if is_flux1_family(arch) {
        inject_flux_redux(result, params);
    } else if is_sd15_family(arch) || is_sdxl_family(arch) {
        inject_ipadapter(result, params);
    }
    // Unsupported families are caught by validate_params before reaching here.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comfyui::types::GenerationParams;
    use crate::templates::WorkflowResult;
    use serde_json::json;

    fn base_result() -> WorkflowResult {
        let mut workflow = serde_json::Map::new();
        // Minimal sampler node so rewire logic has something to update
        workflow.insert(
            "1".to_string(),
            json!({
                "class_type": "KSampler",
                "inputs": {
                    "model": ["0", 0],
                    "positive": ["2", 0],
                    "negative": ["3", 0]
                }
            }),
        );
        WorkflowResult {
            workflow,
            next_id: 10,
            image_output: ("99".to_string(), 0),
            model_source: ("0".to_string(), 0),
            clip_source: ("0".to_string(), 1),
            positive_source: ("2".to_string(), 0),
            negative_source: ("3".to_string(), 0),
            vae_source: ("0".to_string(), 2),
            sampler_id: "1".to_string(),
            refiner_model_source: None,
            base_sources: None,
        }
    }

    fn style_ref_params(arch: &str) -> GenerationParams {
        let mut p = GenerationParams::default();
        p.model_architecture = arch.to_string();
        p.style_ref_enabled = true;
        p.style_ref_image = Some("style_ref.png".to_string());
        p.style_ref_strength = 0.6;
        p.style_ref_weight_type = "linear".to_string();
        p.style_ref_start = 0.0;
        p.style_ref_end = 1.0;
        p
    }

    #[test]
    fn test_flux1_redux_nodes_emitted() {
        let mut result = base_result();
        let params = style_ref_params("flux1d");
        inject_flux_redux(&mut result, &params);

        // StyleModelLoader, CLIPVisionLoader, LoadImage, CLIPVisionEncode, StyleModelApply
        let classes: Vec<&str> = result
            .workflow
            .values()
            .filter_map(|v| v["class_type"].as_str())
            .collect();
        assert!(
            classes.contains(&"StyleModelLoader"),
            "missing StyleModelLoader"
        );
        assert!(
            classes.contains(&"CLIPVisionLoader"),
            "missing CLIPVisionLoader"
        );
        assert!(
            classes.contains(&"CLIPVisionEncode"),
            "missing CLIPVisionEncode"
        );
        assert!(
            classes.contains(&"StyleModelApply"),
            "missing StyleModelApply"
        );

        // positive_source should now point at StyleModelApply
        assert_eq!(
            result.workflow[&result.positive_source.0]["class_type"],
            "StyleModelApply"
        );
    }

    #[test]
    fn test_sdxl_ipadapter_nodes_emitted() {
        let mut result = base_result();
        let params = style_ref_params("sdxl");
        inject_ipadapter(&mut result, &params);

        let classes: Vec<&str> = result
            .workflow
            .values()
            .filter_map(|v| v["class_type"].as_str())
            .collect();
        assert!(
            classes.contains(&"IPAdapterUnifiedLoader"),
            "missing IPAdapterUnifiedLoader"
        );
        assert!(
            classes.contains(&"IPAdapterAdvanced"),
            "missing IPAdapterAdvanced"
        );
        assert!(classes.contains(&"LoadImage"), "missing LoadImage");

        // model_source should now point at IPAdapterAdvanced
        assert_eq!(
            result.workflow[&result.model_source.0]["class_type"],
            "IPAdapterAdvanced"
        );
    }

    #[test]
    fn test_unsupported_family_rejected_by_family_check() {
        // Unsupported families: flux2, qwen, anima, sd3, etc.
        for arch in &["flux2d", "qwen", "sd3", "anima", "wan", "chroma"] {
            assert!(
                !family_supports_style_ref(arch),
                "family {} should be unsupported for style_ref",
                arch
            );
        }
        // Supported families
        for arch in &[
            "flux1d",
            "flux1s",
            "flux1krea",
            "sd15",
            "sdxl",
            "illustrious",
            "pony",
        ] {
            assert!(
                family_supports_style_ref(arch),
                "family {} should be supported for style_ref",
                arch
            );
        }
    }
}
