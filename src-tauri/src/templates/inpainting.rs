use serde_json::json;

use super::{
    build_scheduled_conditioning, insert_vae_decode, is_vpred_model, load_model_nodes,
    WorkflowResult,
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
    let (pos_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.positive_prompt,
        &params.positive_segments,
    );
    next_id = nid;

    // Negative conditioning (with optional timestep scheduling)
    let (neg_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.negative_prompt,
        &params.negative_segments,
    );
    next_id = nid;

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

    // Optionally grow (dilate) the mask so the inpaint blends past the hard
    // mask edge. The user-configured `grow_mask_by` was previously read into
    // GenerationParams but never wired into the graph. Skipped when 0/None.
    let mask_source = if params.grow_mask_by.unwrap_or(0) > 0 {
        let grow_id = next_id.to_string();
        workflow.insert(
            grow_id.clone(),
            json!({
                "class_type": "GrowMask",
                "inputs": {
                    "mask": [load_mask_id, 0],
                    "expand": params.grow_mask_by.unwrap_or(0),
                    "tapered_corners": true
                }
            }),
        );
        next_id += 1;
        grow_id
    } else {
        load_mask_id
    };

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
                "mask": [mask_source, 0]
            }
        }),
    );
    next_id += 1;

    let sampler_name_lc = params.sampler_name.to_lowercase();
    let is_cfgpp_sampler = sampler_name_lc.contains("cfg_pp");
    let is_vpred_or_anima = is_vpred_model(params) || params.model_architecture == "anima";

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
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
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

    // VAE Decode — VAEDecodeTiled for Mugen (Flux2VAE SDXL), VAEDecode otherwise
    let (decode_id, next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        // Expose the model the KSampler is actually wired to (the
        // DifferentialDiffusion node when enabled), not the raw checkpoint.
        // The post-build injectors (vpred/zsnr, cascade, smart-guidance, ...)
        // chain new model patches onto `model_source` and rewire the sampler
        // to them. Returning the raw model here let those injectors re-point
        // the sampler past the DifferentialDiffusion node, silently dropping
        // it — which is exactly the v-pred/Anima inpaint case where it is
        // auto-enabled. Anchoring on the wired model keeps it in the chain.
        model_source: sampler_model_source,
        clip_source,
        positive_source: pos_source,
        negative_source: neg_source,
        vae_source,
        sampler_id,
    }
}
