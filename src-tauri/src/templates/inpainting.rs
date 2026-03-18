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

    // Resize input image to target dimensions
    let resize_id = next_id.to_string();
    workflow.insert(
        resize_id.clone(),
        json!({
            "class_type": "ImageScale",
            "inputs": {
                "image": [load_img_id, 0],
                "width": params.width,
                "height": params.height,
                "upscale_method": "lanczos",
                "crop": "disabled"
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
                "channel": "red"
            }
        }),
    );
    next_id += 1;

    // Encode source image to latent space.
    let encode_id = next_id.to_string();
    workflow.insert(
        encode_id.clone(),
        json!({
            "class_type": "VAEEncode",
            "inputs": {
                "pixels": [resize_id, 0],
                "vae": [vae_source.0.clone(), vae_source.1]
            }
        }),
    );
    next_id += 1;

    // Apply noise mask so only masked areas get denoised/re-sampled.
    let masked_latent_id = next_id.to_string();
    workflow.insert(
        masked_latent_id.clone(),
        json!({
            "class_type": "SetLatentNoiseMask",
            "inputs": {
                "samples": [encode_id, 0],
                "mask": [load_mask_id, 0]
            }
        }),
    );
    next_id += 1;

    let sampler_name_lc = params.sampler_name.to_lowercase();
    let is_cfgpp_sampler = sampler_name_lc.contains("cfg_pp");
    let checkpoint_lc = params.checkpoint.to_lowercase();
    let diffusion_lc = params
        .diffusion_model
        .as_ref()
        .map(|m| m.to_lowercase())
        .unwrap_or_default();
    let is_vpred_or_anima = checkpoint_lc.contains("vpred")
        || checkpoint_lc.contains("anima")
        || diffusion_lc.contains("vpred")
        || diffusion_lc.contains("anima");

    let use_differential_diffusion =
        params.differential_diffusion || (is_vpred_or_anima && !is_cfgpp_sampler);

    let mut sampler_model_source = model_source.clone();
    if use_differential_diffusion {
        let differential_id = next_id.to_string();
        workflow.insert(
            differential_id.clone(),
            json!({
                "class_type": "DifferentialDiffusion",
                "inputs": {
                    "model": [model_source.0.clone(), model_source.1]
                }
            }),
        );
        sampler_model_source = (differential_id, 0);
        next_id += 1;
    }

    // KSampler
    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [sampler_model_source.0.clone(), sampler_model_source.1],
                "positive": [pos_id.clone(), 0],
                "negative": [neg_id.clone(), 0],
                "latent_image": [masked_latent_id, 0],
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
