//! Style reference injection: Flux Redux (core nodes), IP-Adapter+ (SD1.5/SDXL)
//! and Anima character reference (Reference Adapter or SnapRef).
//!
//! Flux Redux: StyleModelLoader + CLIPVisionLoader + CLIPVisionEncode + StyleModelApply
//! IP-Adapter: IPAdapterUnifiedLoader + IPAdapter (IPAdapterAdvanced)
//! Anima: LoadImage (+ a second LoadImage and ImageStitch for two characters) +
//!   AnimaRefAdapter (ComfyUI-Anima-Ref-Adapter, auto-installed with the
//!   style-transfer packs), which patches the model for the existing KSampler.
//! All insert into txt2img, img2img, and inpainting workflows (not video, not image_edit).

use serde_json::json;

use crate::comfyui::nodes::ANIMA_REF_ADAPTER_LORA_FILENAME;
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

fn is_anima_family(arch: &str) -> bool {
    arch == "anima"
}

/// True when the current model family supports style reference.
pub fn family_supports_style_ref(arch: &str) -> bool {
    is_flux1_family(arch) || is_sd15_family(arch) || is_sdxl_family(arch) || is_anima_family(arch)
}

/// True when this request runs the Anima character reference, which patches
/// the model itself (callers keep other model patches such as TeaCache off).
pub fn anima_ref_active(params: &GenerationParams) -> bool {
    params.style_ref_enabled
        && params.mode != "video"
        && params.mode != "image_edit"
        && is_anima_family(&params.model_architecture)
}

pub fn snapref_active(params: &GenerationParams) -> bool {
    anima_ref_active(params) && params.anima_ref_method == "snapref"
}

pub fn validate_snapref(params: &GenerationParams) -> Result<(), String> {
    if !["", "anima_ref_adapter", "snapref"].contains(&params.anima_ref_method.as_str()) {
        return Err("Unknown Anima reference method.".into());
    }
    if !snapref_active(params) {
        return Ok(());
    }
    if trimmed_image(params.snapref_adapter.as_deref()).is_none() {
        return Err("Select or download SnapRef adapter weights before generating.".into());
    }
    if !["Character & Style", "Character"].contains(&params.snapref_mode.as_str()) {
        return Err("Unknown SnapRef mode.".into());
    }
    if params.snapref_references.len() > 8
        || !params
            .snapref_references
            .iter()
            .any(|r| trimmed_image(r.image.as_deref()).is_some())
    {
        return Err(
            "SnapRef requires between one and eight reference images of the same character.".into(),
        );
    }
    if !params.snapref_fidelity.is_finite()
        || !(0.0..=1.0).contains(&params.snapref_fidelity)
        || params
            .snapref_references
            .iter()
            .any(|r| !r.strength.is_finite() || !(0.0..=2.0).contains(&r.strength))
    {
        return Err("SnapRef strength must be 0–2 and fidelity must be 0–1.".into());
    }
    Ok(())
}

pub fn inject_snapref(result: &mut WorkflowResult, params: &GenerationParams) {
    let id = result.next_id.to_string();
    let mut inputs = json!({
        "model": [result.model_source.0, result.model_source.1],
        "clip": [result.clip_source.0, result.clip_source.1],
        "vae": [result.vae_source.0, result.vae_source.1],
        "adapter": params.snapref_adapter.as_deref().unwrap_or("").trim(),
        "mode": params.snapref_mode,
        "fidelity": params.snapref_fidelity,
    });
    for (i, (image, strength)) in params
        .snapref_references
        .iter()
        .filter_map(|r| trimmed_image(r.image.as_deref()).map(|image| (image, r.strength)))
        .enumerate()
    {
        let suffix = if i == 0 {
            String::new()
        } else {
            format!("_{}", i + 1)
        };
        inputs[format!("reference{suffix}")] = json!(image);
        inputs[format!("strength{suffix}")] = json!(strength);
    }
    result.workflow.insert(
        id.clone(),
        json!({"class_type": "SnapRefSimple", "inputs": inputs}),
    );
    result.model_source = (id.clone(), 0);
    result.next_id += 1;
    if let Some(sampler) = result.workflow.get_mut(&result.sampler_id) {
        sampler["inputs"]["model"] = json!([id, 0]);
    }
}

/// `AnimaRefAdapter.correction` choices.
const ANIMA_REF_CORRECTIONS: &[&str] = &["apg", "rescale", "none"];

fn anima_ref_correction(correction: &str) -> &str {
    if ANIMA_REF_CORRECTIONS.contains(&correction) {
        correction
    } else {
        "apg"
    }
}

fn trimmed_image(image: Option<&str>) -> Option<&str> {
    image.map(str::trim).filter(|s| !s.is_empty())
}

/// `StyleModelApply.strength_type` choices in ComfyUI core.
const REDUX_STRENGTH_TYPES: &[&str] = &["multiply", "attn_bias"];

/// `IPAdapterAdvanced.weight_type` choices (ComfyUI_IPAdapter_plus `WEIGHT_TYPES`).
const IPADAPTER_WEIGHT_TYPES: &[&str] = &[
    "linear",
    "ease in",
    "ease out",
    "ease in-out",
    "reverse in-out",
    "weak input",
    "weak output",
    "weak middle",
    "strong input",
    "strong output",
    "strong middle",
    "style transfer",
    "composition",
    "strong style transfer",
    "style and composition",
    "style transfer precise",
    "composition precise",
];

/// Both branches share one `style_ref_weight_type` setting, whose default
/// (`linear`) is an IP-Adapter value. Sending a value the target node does not
/// list fails ComfyUI's prompt validation, so anything foreign to the branch
/// falls back to that branch's default.
fn redux_strength_type(weight_type: &str) -> &str {
    if REDUX_STRENGTH_TYPES.contains(&weight_type) {
        weight_type
    } else {
        "multiply"
    }
}

fn ipadapter_weight_type(weight_type: &str) -> &str {
    if IPADAPTER_WEIGHT_TYPES.contains(&weight_type) {
        weight_type
    } else {
        "linear"
    }
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
    let strength_type = redux_strength_type(&params.style_ref_weight_type);
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
    let weight_type = ipadapter_weight_type(&params.style_ref_weight_type);
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

/// Inject the Anima Reference Adapter: the reference image (or two, side by
/// side) patches the model, and the existing sampler keeps sampling as usual.
pub fn inject_anima_ref_adapter(result: &mut WorkflowResult, params: &GenerationParams) {
    let image = trimmed_image(params.style_ref_image.as_deref()).unwrap_or("");
    let mut next_id = result.next_id;

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
    let mut reference = (load_image_id, 0);

    // Two characters: the second reference goes to the right of the first, as
    // in the adapter's own two-character workflow.
    if let Some(second) = trimmed_image(params.style_ref_image_2.as_deref()) {
        let second_id = next_id.to_string();
        result.workflow.insert(
            second_id.clone(),
            json!({
                "class_type": "LoadImage",
                "inputs": {
                    "image": second
                }
            }),
        );
        next_id += 1;

        let stitch_id = next_id.to_string();
        result.workflow.insert(
            stitch_id.clone(),
            json!({
                "class_type": "ImageStitch",
                "inputs": {
                    "image1": [reference.0, reference.1],
                    "image2": [second_id, 0],
                    "direction": "right",
                    "match_image_size": true,
                    "spacing_width": 0,
                    "spacing_color": "white"
                }
            }),
        );
        next_id += 1;
        reference = (stitch_id, 0);
    }

    let adapter_id = next_id.to_string();
    result.workflow.insert(
        adapter_id.clone(),
        json!({
            "class_type": "AnimaRefAdapter",
            "inputs": {
                "model": [result.model_source.0.clone(), result.model_source.1],
                "vae": [result.vae_source.0.clone(), result.vae_source.1],
                "image": [reference.0, reference.1],
                "lora_name": ANIMA_REF_ADAPTER_LORA_FILENAME,
                "ref_guidance": params.anima_ref_guidance.clamp(0.0, 20.0),
                "correction": anima_ref_correction(&params.anima_ref_correction),
                "strength": 1.0,
                "ref_end": params.style_ref_end.clamp(0.0, 1.0),
                "ref_megapixels": 1.0
            }
        }),
    );
    next_id += 1;

    result.model_source = (adapter_id, 0);
    result.next_id = next_id;

    if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
        if let Some(inputs) = sampler_node.get_mut("inputs") {
            inputs["model"] = json!([result.model_source.0, result.model_source.1]);
        }
    }
}

/// Main entry point: dispatch to the correct injector based on model family.
/// Returns Ok(()) if injected, Err(msg) if the family is unsupported.
pub fn inject_style_ref(result: &mut WorkflowResult, params: &GenerationParams) {
    let arch = params.model_architecture.as_str();
    if is_flux1_family(arch) {
        inject_flux_redux(result, params);
    } else if is_sd15_family(arch) || is_sdxl_family(arch) {
        inject_ipadapter(result, params);
    } else if is_anima_family(arch) {
        if snapref_active(params) {
            inject_snapref(result, params);
        } else {
            inject_anima_ref_adapter(result, params);
        }
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

    fn style_apply_input(result: &WorkflowResult, class_type: &str, input: &str) -> String {
        result
            .workflow
            .values()
            .find(|v| v["class_type"] == class_type)
            .unwrap_or_else(|| panic!("missing {class_type}"))["inputs"][input]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn test_redux_strength_type_only_emits_values_the_node_accepts() {
        // "linear" is the store default (an IP-Adapter value); "average" was
        // offered by the old Redux dropdown. Neither is a StyleModelApply choice.
        for (sent, expected) in [
            ("linear", "multiply"),
            ("average", "multiply"),
            ("", "multiply"),
            ("multiply", "multiply"),
            ("attn_bias", "attn_bias"),
        ] {
            let mut result = base_result();
            let mut params = style_ref_params("flux1d");
            params.style_ref_weight_type = sent.to_string();
            inject_flux_redux(&mut result, &params);
            assert_eq!(
                style_apply_input(&result, "StyleModelApply", "strength_type"),
                expected,
                "Redux strength_type for {sent:?}"
            );
        }
    }

    #[test]
    fn test_ipadapter_weight_type_only_emits_values_the_node_accepts() {
        for (sent, expected) in [
            ("multiply", "linear"),
            ("attn_bias", "linear"),
            ("", "linear"),
            ("linear", "linear"),
            ("style transfer", "style transfer"),
            ("strong style transfer", "strong style transfer"),
        ] {
            let mut result = base_result();
            let mut params = style_ref_params("sdxl");
            params.style_ref_weight_type = sent.to_string();
            inject_ipadapter(&mut result, &params);
            assert_eq!(
                style_apply_input(&result, "IPAdapterAdvanced", "weight_type"),
                expected,
                "IP-Adapter weight_type for {sent:?}"
            );
        }
    }

    #[test]
    fn test_unsupported_family_rejected_by_family_check() {
        // Unsupported families: flux2, qwen, sd3, etc.
        for arch in &["flux2d", "qwen", "sd3", "wan", "chroma"] {
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
            "anima",
        ] {
            assert!(
                family_supports_style_ref(arch),
                "family {} should be supported for style_ref",
                arch
            );
        }
    }

    fn node<'a>(result: &'a WorkflowResult, class_type: &str) -> &'a serde_json::Value {
        result
            .workflow
            .values()
            .find(|v| v["class_type"] == class_type)
            .unwrap_or_else(|| panic!("missing {class_type}"))
    }

    fn anima_params() -> GenerationParams {
        let mut p = style_ref_params("anima");
        p.mode = "txt2img".to_string();
        p.anima_ref_guidance = 4.5;
        p.anima_ref_correction = "apg".to_string();
        p
    }

    #[test]
    fn anima_ref_adapter_patches_the_sampler_model() {
        let mut result = base_result();
        inject_style_ref(&mut result, &anima_params());

        let adapter = node(&result, "AnimaRefAdapter");
        let inputs = &adapter["inputs"];
        assert_eq!(inputs["model"], json!(["0", 0]));
        assert_eq!(inputs["vae"], json!(["0", 2]));
        assert_eq!(inputs["lora_name"], ANIMA_REF_ADAPTER_LORA_FILENAME);
        assert_eq!(inputs["ref_guidance"], 4.5);
        assert_eq!(inputs["correction"], "apg");
        assert_eq!(inputs["ref_end"], 1.0);

        let reference = &result.workflow[inputs["image"][0].as_str().unwrap()];
        assert_eq!(reference["class_type"], "LoadImage");
        assert_eq!(reference["inputs"]["image"], "style_ref.png");

        assert_eq!(
            result.workflow[&result.model_source.0]["class_type"],
            "AnimaRefAdapter"
        );
        assert_eq!(
            result.workflow["1"]["inputs"]["model"],
            json!([result.model_source.0, 0])
        );
        // The conditioning is left alone.
        assert_eq!(result.positive_source, ("2".to_string(), 0));
        assert!(!result
            .workflow
            .values()
            .any(|v| v["class_type"] == "ImageStitch"));
    }

    #[test]
    fn anima_two_characters_stitch_the_second_reference_on_the_right() {
        let mut result = base_result();
        let mut params = anima_params();
        params.style_ref_image_2 = Some("second.png".to_string());
        inject_style_ref(&mut result, &params);

        let adapter = node(&result, "AnimaRefAdapter");
        let stitch = &result.workflow[adapter["inputs"]["image"][0].as_str().unwrap()];
        assert_eq!(stitch["class_type"], "ImageStitch");
        assert_eq!(stitch["inputs"]["direction"], "right");
        let left = &result.workflow[stitch["inputs"]["image1"][0].as_str().unwrap()];
        let right = &result.workflow[stitch["inputs"]["image2"][0].as_str().unwrap()];
        assert_eq!(left["inputs"]["image"], "style_ref.png");
        assert_eq!(right["inputs"]["image"], "second.png");
    }

    #[test]
    fn anima_blank_second_reference_is_ignored() {
        let mut result = base_result();
        let mut params = anima_params();
        params.style_ref_image_2 = Some("  ".to_string());
        inject_style_ref(&mut result, &params);
        assert!(!result
            .workflow
            .values()
            .any(|v| v["class_type"] == "ImageStitch"));
    }

    #[test]
    fn anima_ref_inputs_stay_inside_what_the_node_accepts() {
        let mut result = base_result();
        let mut params = anima_params();
        params.anima_ref_guidance = 99.0;
        params.anima_ref_correction = "linear".to_string();
        params.style_ref_end = 3.0;
        inject_anima_ref_adapter(&mut result, &params);

        let inputs = &node(&result, "AnimaRefAdapter")["inputs"];
        assert_eq!(inputs["ref_guidance"], 20.0);
        assert_eq!(inputs["correction"], "apg");
        assert_eq!(inputs["ref_end"], 1.0);
    }

    #[test]
    fn anima_ref_active_only_for_anima_image_modes() {
        let mut params = anima_params();
        assert!(anima_ref_active(&params));
        params.mode = "image_edit".to_string();
        assert!(!anima_ref_active(&params));
        params.mode = "txt2img".to_string();
        params.style_ref_enabled = false;
        assert!(!anima_ref_active(&params));
        assert!(!anima_ref_active(&style_ref_params("sdxl")));
    }
    #[test]
    fn older_requests_keep_the_existing_adapter_and_snapref_defaults() {
        let mut value = serde_json::to_value(anima_params()).unwrap();
        for key in [
            "anima_ref_method",
            "snapref_adapter",
            "snapref_mode",
            "snapref_fidelity",
            "snapref_references",
        ] {
            value.as_object_mut().unwrap().remove(key);
        }
        let params: GenerationParams = serde_json::from_value(value).unwrap();
        assert!(!snapref_active(&params));
        assert_eq!(params.snapref_mode, "Character & Style");
        assert_eq!(params.snapref_fidelity, 1.0);
        let mut result = base_result();
        inject_style_ref(&mut result, &params);
        assert_eq!(
            node(&result, "AnimaRefAdapter")["class_type"],
            "AnimaRefAdapter"
        );
    }

    fn snapref_params() -> GenerationParams {
        let mut p = anima_params();
        p.anima_ref_method = "snapref".into();
        p.snapref_adapter = Some("subfolder/snapref_v2a.safetensors".into());
        p.snapref_mode = "Character & Style".into();
        p.snapref_fidelity = 1.0;
        p.style_ref_image = None;
        p.snapref_references = vec![crate::comfyui::types::SnapRefReference {
            image: Some("character.png".into()),
            strength: 1.0,
        }];
        p
    }

    #[test]
    fn snapref_uses_the_existing_sampler_in_all_image_modes() {
        for mode in ["txt2img", "img2img", "inpainting"] {
            for adapter in [
                "snapref_v1a.safetensors",
                "subfolder/snapref_v2a_fp8.safetensors",
            ] {
                let mut p = snapref_params();
                p.mode = mode.into();
                p.snapref_adapter = Some(adapter.into());
                assert!(validate_snapref(&p).is_ok());
                let mut result = base_result();
                inject_style_ref(&mut result, &p);
                let inputs = &node(&result, "SnapRefSimple")["inputs"];
                assert_eq!(inputs["adapter"], adapter);
                assert_eq!(inputs["model"], json!(["0", 0]));
                assert_eq!(inputs["clip"], json!(["0", 1]));
                assert_eq!(inputs["vae"], json!(["0", 2]));
                assert_eq!(inputs["reference"], "character.png");
                assert_eq!(
                    result.workflow["1"]["inputs"]["model"],
                    json!([result.model_source.0, 0])
                );
                assert_eq!(result.positive_source, ("2".into(), 0));
                assert!(!result
                    .workflow
                    .values()
                    .any(|n| n["class_type"] == "AnimaRefAdapter"
                        || n["class_type"] == "ImageStitch"));
            }
        }
    }

    #[test]
    fn snapref_preserves_eight_weights_and_compacts_empty_slots() {
        let mut p = snapref_params();
        p.snapref_references = (0..8)
            .map(|i| crate::comfyui::types::SnapRefReference {
                image: Some(format!("ref_{i}.png")),
                strength: i as f32 / 4.0,
            })
            .collect();
        let mut result = base_result();
        inject_style_ref(&mut result, &p);
        let inputs = &node(&result, "SnapRefSimple")["inputs"];
        assert_eq!(inputs["strength"], 0.0);
        assert_eq!(inputs["reference_8"], "ref_7.png");
        assert_eq!(inputs["strength_8"], 1.75);
        p.snapref_references[0].image = Some("  ".into());
        let mut result = base_result();
        inject_style_ref(&mut result, &p);
        let inputs = &node(&result, "SnapRefSimple")["inputs"];
        assert_eq!(inputs["reference"], "ref_1.png");
        assert_eq!(inputs["strength"], 0.25);
        assert!(inputs.get("reference_8").is_none());
    }

    #[test]
    fn snapref_validation_rejects_missing_inputs_and_invalid_ranges() {
        let p = snapref_params();
        let mut invalid = p.clone();
        invalid.snapref_references.clear();
        assert!(validate_snapref(&invalid).is_err());
        let mut invalid = p.clone();
        invalid.snapref_adapter = None;
        assert!(validate_snapref(&invalid).is_err());
        let mut invalid = p.clone();
        invalid.snapref_mode = "cs".into();
        assert!(validate_snapref(&invalid).is_err());
        let mut invalid = p.clone();
        invalid.snapref_references = vec![p.snapref_references[0].clone(); 9];
        assert!(validate_snapref(&invalid).is_err());
        let mut invalid = p.clone();
        invalid.snapref_fidelity = 1.1;
        assert!(validate_snapref(&invalid).is_err());
        let mut invalid = p.clone();
        invalid.snapref_references[0].strength = -0.1;
        assert!(validate_snapref(&invalid).is_err());
        let mut zero = p.clone();
        zero.snapref_references[0].strength = 0.0;
        assert!(validate_snapref(&zero).is_ok());
        for mode in ["image_edit", "video"] {
            let mut inactive = p.clone();
            inactive.mode = mode.into();
            assert!(!snapref_active(&inactive));
        }
    }
}
