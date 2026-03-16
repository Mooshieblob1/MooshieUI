use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let mut next_id: u32 = 1;

    // 1: Checkpoint loader
    let checkpoint_id = next_id.to_string();
    workflow.insert(
        checkpoint_id.clone(),
        json!({
            "class_type": "CheckpointLoaderSimple",
            "inputs": {
                "ckpt_name": params.checkpoint
            }
        }),
    );
    next_id += 1;

    let mut model_source = (checkpoint_id.clone(), 0);
    let mut clip_source = (checkpoint_id.clone(), 1);
    let mut vae_source: (String, u32) = (checkpoint_id.clone(), 2);

    // LoRA chain
    for lora in &params.loras {
        let lora_id = next_id.to_string();
        workflow.insert(
            lora_id.clone(),
            json!({
                "class_type": "LoraLoader",
                "inputs": {
                    "model": [model_source.0, model_source.1],
                    "clip": [clip_source.0, clip_source.1],
                    "lora_name": lora.name,
                    "strength_model": lora.strength_model,
                    "strength_clip": lora.strength_clip
                }
            }),
        );
        model_source = (lora_id.clone(), 0);
        clip_source = (lora_id, 1);
        next_id += 1;
    }

    // Optional separate VAE
    if let Some(ref vae_name) = params.vae {
        if !vae_name.is_empty() {
            let vae_id = next_id.to_string();
            workflow.insert(
                vae_id.clone(),
                json!({
                    "class_type": "VAELoader",
                    "inputs": {
                        "vae_name": vae_name
                    }
                }),
            );
            vae_source = (vae_id, 0);
            next_id += 1;
        }
    }

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

    // Load input image
    let load_img_id = next_id.to_string();
    workflow.insert(
        load_img_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": {
                "image": params.input_image.as_deref().unwrap_or("")
            }
        }),
    );
    next_id += 1;

    // Load mask
    let load_mask_id = next_id.to_string();
    workflow.insert(
        load_mask_id.clone(),
        json!({
            "class_type": "LoadImageMask",
            "inputs": {
                "image": params.mask_image.as_deref().unwrap_or(""),
                "channel": "alpha"
            }
        }),
    );
    next_id += 1;

    // VAE Encode for Inpaint
    let encode_id = next_id.to_string();
    workflow.insert(
        encode_id.clone(),
        json!({
            "class_type": "VAEEncodeForInpaint",
            "inputs": {
                "pixels": [load_img_id, 0],
                "vae": [vae_source.0.clone(), vae_source.1],
                "mask": [load_mask_id, 0],
                "grow_mask_by": params.grow_mask_by.unwrap_or(6)
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
                "latent_image": [encode_id, 0],
                "seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": params.denoise
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
        positive_id: pos_id,
        negative_id: neg_id,
        vae_source,
    }
}
