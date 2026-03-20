use serde_json::json;

use super::{WorkflowResult, load_model_nodes};
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

    // Positive CLIP encode
    let pos_id = next_id.to_string();
    workflow.insert(
        pos_id.clone(),
        json!({
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": [clip_source.0, clip_source.1],
                "text": params.positive_prompt
            }
        }),
    );
    next_id += 1;

    // Negative CLIP encode
    let neg_id = next_id.to_string();
    workflow.insert(
        neg_id.clone(),
        json!({
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": [clip_source.0, clip_source.1],
                "text": params.negative_prompt
            }
        }),
    );
    next_id += 1;

    // Empty latent image
    let latent_id = next_id.to_string();
    workflow.insert(
        latent_id.clone(),
        json!({
            "class_type": "EmptyLatentImage",
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
                "positive": [pos_id.clone(), 0],
                "negative": [neg_id.clone(), 0],
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

    // VAE Decode
    let decode_id = next_id.to_string();
    workflow.insert(
        decode_id.clone(),
        json!({
            "class_type": "VAEDecode",
            "inputs": {
                "samples": [sampler_id, 0],
                "vae": [vae_source.0.clone(), vae_source.1]
            }
        }),
    );
    next_id += 1;

    WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        model_source,
        positive_source: (pos_id, 0),
        negative_source: (neg_id, 0),
        vae_source,
        sampler_id,
    }
}
