use serde_json::json;

use super::{
    build_regional_context_prompt, build_scheduled_conditioning, insert_vae_decode,
    load_model_nodes, merge_regional_encode_text, needs_sd3_latent, WorkflowResult,
};
use crate::comfyui::types::GenerationParams;

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    // Load model (checkpoint or split UNETLoader + CLIPLoader + VAELoader)
    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    // Positive conditioning (with optional timestep scheduling)
    let (mut pos_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.positive_prompt,
        &params.positive_segments,
    );
    next_id = nid;

    // Regional prompting (syntax-first): <region:x1,y1,x2,y2>text</region>
    // Scope-gated to SDXL-family txt2img to keep v1 risk contained.
    let supports_regions = matches!(params.model_architecture.as_str(), "sdxl" | "illustrious");
    if params.mode == "txt2img" && supports_regions {
        let regional_context = build_regional_context_prompt(params);
        for region in &params.positive_regions {
            let text = merge_regional_encode_text(&regional_context, &region.text)
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }

            let x = region.x.clamp(0.0, 1.0);
            let y = region.y.clamp(0.0, 1.0);
            let w = region.width.clamp(0.0, 1.0 - x);
            let h = region.height.clamp(0.0, 1.0 - y);
            if w <= 0.0 || h <= 0.0 {
                continue;
            }

            let region_encode_id = next_id.to_string();
            workflow.insert(
                region_encode_id.clone(),
                json!({
                    "class_type": "CLIPTextEncode",
                    "inputs": {
                        "text": text,
                        "clip": [clip_source.0.clone(), clip_source.1]
                    }
                }),
            );
            next_id += 1;

            let region_area_id = next_id.to_string();
            workflow.insert(
                region_area_id.clone(),
                json!({
                    "class_type": "ConditioningSetAreaPercentage",
                    "inputs": {
                        "conditioning": [region_encode_id, 0],
                        "x": x,
                        "y": y,
                        "width": w,
                        "height": h,
                        "strength": region.strength.clamp(0.0, 2.0)
                    }
                }),
            );
            next_id += 1;

            let region_combine_id = next_id.to_string();
            workflow.insert(
                region_combine_id.clone(),
                json!({
                    "class_type": "ConditioningCombine",
                    "inputs": {
                        "conditioning_1": [pos_source.0.clone(), pos_source.1],
                        "conditioning_2": [region_area_id, 0]
                    }
                }),
            );
            pos_source = (region_combine_id, 0);
            next_id += 1;
        }
    }

    // Negative conditioning (with optional timestep scheduling)
    let (neg_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.negative_prompt,
        &params.negative_segments,
    );
    next_id = nid;

    // Empty latent image — use SD3 variant (16 channels) for SD3/Flux and Anima/WAN models
    let latent_id = next_id.to_string();
    let use_sd3_latent = needs_sd3_latent(params);
    workflow.insert(
        latent_id.clone(),
        json!({
            "class_type": if use_sd3_latent { "EmptySD3LatentImage" } else { "EmptyLatentImage" },
            "inputs": {
                "width": params.width,
                "height": params.height,
                "batch_size": params.batch_size
            }
        }),
    );
    next_id += 1;

    // KSampler
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

    // VAE Decode — VAEDecodeTiled for Mugen (Flux2VAE SDXL), VAEDecode otherwise
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
