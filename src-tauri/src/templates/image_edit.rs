//! Image Edit mode: instruction-driven editing of one or more reference images.
//!
//! Two paths, keyed on the resolved model family:
//!   * Qwen Image Edit / Edit Plus (`qwen_edit`, `qwen_edit_plus`) —
//!     `TextEncodeQwenImageEdit` / `TextEncodeQwenImageEditPlus` fold the reference
//!     image(s) into the conditioning (Qwen2.5-VL vision tokens + a reference latent
//!     when a VAE is wired). Sampling starts from an empty SD3 latent at the target
//!     resolution.
//!   * Flux.1 Kontext (`flux1kontext`) — the reference is scaled with
//!     `FluxKontextImageScale`, encoded with `VAEEncode`, and attached to the positive
//!     conditioning via `ReferenceLatent` + `FluxGuidance`. The KSampler denoises from
//!     the encoded reference latent (denoise 1.0) so the output matches Kontext's
//!     preferred scaled resolution.
//!
//! All required nodes are core ComfyUI — no custom node install.

use serde_json::json;

use super::{build_scheduled_conditioning, insert_vae_decode, load_model_nodes, WorkflowResult};
use crate::comfyui::types::GenerationParams;

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    match params.model_architecture.as_str() {
        "flux1kontext" => build_kontext(params, seed),
        "qwen_edit" | "qwen_edit_plus" => build_qwen(params, seed),
        // Validation guarantees a supported edit family reaches here; fall back to
        // txt2img defensively rather than panicking on an unexpected architecture.
        _ => super::txt2img::build(params, seed),
    }
}

/// Non-empty reference-image filenames (ComfyUI input names), slot 0 first.
fn reference_images(params: &GenerationParams) -> Vec<&str> {
    params
        .edit_reference_images
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Qwen Image Edit / Edit Plus path.
fn build_qwen(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    // Edit Plus accepts up to three references; single Edit uses only slot 0.
    let is_plus = params.model_architecture == "qwen_edit_plus";
    let mut refs = reference_images(params);
    if refs.is_empty() {
        // Validation should prevent this; keep the workflow well-formed regardless.
        refs.push("");
    }
    if !is_plus {
        refs.truncate(1);
    }
    refs.truncate(3);

    // LoadImage per reference slot.
    let mut load_ids: Vec<String> = Vec::with_capacity(refs.len());
    for image_name in &refs {
        let load_id = next_id.to_string();
        workflow.insert(
            load_id.clone(),
            json!({
                "class_type": "LoadImage",
                "inputs": { "image": image_name }
            }),
        );
        load_ids.push(load_id);
        next_id += 1;
    }

    // Positive and negative conditioning both fold in the reference image(s) and VAE,
    // matching ComfyUI's bundled Qwen Image Edit templates.
    let (pos_source, nid) = encode_qwen_conditioning(
        &mut workflow,
        next_id,
        is_plus,
        &clip_source,
        &vae_source,
        &load_ids,
        &params.positive_prompt,
    );
    next_id = nid;

    let (neg_source, nid) = encode_qwen_conditioning(
        &mut workflow,
        next_id,
        is_plus,
        &clip_source,
        &vae_source,
        &load_ids,
        &params.negative_prompt,
    );
    next_id = nid;

    // Empty latent at the target resolution (Qwen uses the SD3 latent node).
    let latent_id = next_id.to_string();
    workflow.insert(
        latent_id.clone(),
        json!({
            "class_type": "EmptySD3LatentImage",
            "inputs": {
                "width": params.width,
                "height": params.height,
                "batch_size": params.batch_size
            }
        }),
    );
    next_id += 1;

    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [latent_id, 0],
                "seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": 1.0
            }
        }),
    );
    next_id += 1;

    let (decode_id, next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        model_source,
        clip_source,
        positive_source: pos_source,
        negative_source: neg_source,
        vae_source,
        sampler_id,
    }
}

/// Insert a `TextEncodeQwenImageEdit` (single) or `TextEncodeQwenImageEditPlus`
/// (multi) node wiring the CLIP, VAE, prompt text, and the loaded reference
/// image(s). Returns `(conditioning_source, next_id)`.
#[allow(clippy::too_many_arguments)]
fn encode_qwen_conditioning(
    workflow: &mut serde_json::Map<String, serde_json::Value>,
    next_id: u32,
    is_plus: bool,
    clip_source: &(String, u32),
    vae_source: &(String, u32),
    load_ids: &[String],
    prompt: &str,
) -> ((String, u32), u32) {
    let node_id = next_id.to_string();
    let mut inputs = serde_json::Map::new();
    inputs.insert("clip".into(), json!([clip_source.0.clone(), clip_source.1]));
    inputs.insert("prompt".into(), json!(prompt));
    inputs.insert("vae".into(), json!([vae_source.0.clone(), vae_source.1]));

    let class_type = if is_plus {
        // image1..image3 are optional inputs — wire only the slots we have.
        for (idx, load_id) in load_ids.iter().take(3).enumerate() {
            inputs.insert(format!("image{}", idx + 1), json!([load_id.clone(), 0]));
        }
        "TextEncodeQwenImageEditPlus"
    } else {
        if let Some(load_id) = load_ids.first() {
            inputs.insert("image".into(), json!([load_id.clone(), 0]));
        }
        "TextEncodeQwenImageEdit"
    };

    workflow.insert(
        node_id.clone(),
        json!({ "class_type": class_type, "inputs": inputs }),
    );
    ((node_id, 0), next_id + 1)
}

/// Flux.1 Kontext path.
fn build_kontext(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    // Base positive conditioning (plain text encode; no schedule segments in edit mode).
    let (pos_base, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.positive_prompt,
        &[],
    );
    next_id = nid;

    let image_name = reference_images(params)
        .first()
        .copied()
        .unwrap_or_default()
        .to_string();

    let load_id = next_id.to_string();
    workflow.insert(
        load_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": { "image": image_name }
        }),
    );
    next_id += 1;

    // Snap the reference to a Kontext-preferred resolution before encoding.
    let scale_id = next_id.to_string();
    workflow.insert(
        scale_id.clone(),
        json!({
            "class_type": "FluxKontextImageScale",
            "inputs": { "image": [load_id, 0] }
        }),
    );
    next_id += 1;

    let vae_encode_id = next_id.to_string();
    workflow.insert(
        vae_encode_id.clone(),
        json!({
            "class_type": "VAEEncode",
            "inputs": {
                "pixels": [scale_id, 0],
                "vae": [vae_source.0.clone(), vae_source.1]
            }
        }),
    );
    next_id += 1;

    // Attach the reference latent to the positive conditioning.
    let ref_id = next_id.to_string();
    workflow.insert(
        ref_id.clone(),
        json!({
            "class_type": "ReferenceLatent",
            "inputs": {
                "conditioning": [pos_base.0.clone(), pos_base.1],
                "latent": [vae_encode_id.clone(), 0]
            }
        }),
    );
    next_id += 1;

    // Kontext is guidance-distilled — apply FluxGuidance to the referenced positive.
    let guidance_id = next_id.to_string();
    workflow.insert(
        guidance_id.clone(),
        json!({
            "class_type": "FluxGuidance",
            "inputs": {
                "conditioning": [ref_id, 0],
                "guidance": params.flux_guidance
            }
        }),
    );
    let pos_source = (guidance_id, 0u32);
    next_id += 1;

    // Negative is a zeroed copy of the base text conditioning (KSampler ignores it at
    // cfg 1.0 but still requires a valid, shape-compatible input).
    let neg_id = next_id.to_string();
    workflow.insert(
        neg_id.clone(),
        json!({
            "class_type": "ConditioningZeroOut",
            "inputs": { "conditioning": [pos_base.0.clone(), pos_base.1] }
        }),
    );
    let neg_source = (neg_id, 0u32);
    next_id += 1;

    // Denoise the encoded reference latent (denoise 1.0 replaces its content with noise
    // while keeping Kontext's scaled output resolution).
    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [vae_encode_id, 0],
                "seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": 1.0
            }
        }),
    );
    next_id += 1;

    let (decode_id, next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        model_source,
        clip_source,
        positive_source: pos_source,
        negative_source: neg_source,
        vae_source,
        sampler_id,
    }
}
