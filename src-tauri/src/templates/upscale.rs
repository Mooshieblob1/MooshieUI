use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

/// Appends the upscale node chain to an existing workflow.
/// Returns the (node_id, output_index) of the final upscaled IMAGE.
pub fn append_upscale_chain(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    seed: i64,
) -> (String, u32) {
    let next_id = &mut result.next_id;
    let workflow = &mut result.workflow;

    // Determine effective method — fall back to algorithmic if no model specified
    let use_model = params.upscale_method == "model"
        && params.upscale_model.as_ref().is_some_and(|m| !m.is_empty());

    // Step 1: Upscale image in pixel space
    let upscaled_image: (String, u32) = if use_model {
        let loader_id = next_id.to_string();
        workflow.insert(
            loader_id.clone(),
            json!({
                "class_type": "UpscaleModelLoader",
                "inputs": {
                    "model_name": params.upscale_model.as_deref().unwrap_or("")
                }
            }),
        );
        *next_id += 1;

        let upscale_id = next_id.to_string();
        workflow.insert(
            upscale_id.clone(),
            json!({
                "class_type": "ImageUpscaleWithModel",
                "inputs": {
                    "upscale_model": [loader_id, 0],
                    "image": [result.image_output.0.clone(), result.image_output.1]
                }
            }),
        );
        *next_id += 1;

        // Optional target-scale cap: resize back down toward a lower multiplier
        // instead of always refining at the model's full native scale.
        if params.upscale_model_downscale_ratio < 0.999 {
            let downscale_id = next_id.to_string();
            workflow.insert(
                downscale_id.clone(),
                json!({
                    "class_type": "ImageScaleBy",
                    "inputs": {
                        "image": [upscale_id, 0],
                        "upscale_method": "lanczos",
                        "scale_by": params.upscale_model_downscale_ratio
                    }
                }),
            );
            *next_id += 1;
            (downscale_id, 0)
        } else {
            (upscale_id, 0)
        }
    } else {
        let scale_id = next_id.to_string();
        workflow.insert(
            scale_id.clone(),
            json!({
                "class_type": "ImageScaleBy",
                "inputs": {
                    "image": [result.image_output.0.clone(), result.image_output.1],
                    "upscale_method": "lanczos",
                    "scale_by": params.upscale_scale
                }
            }),
        );
        *next_id += 1;
        (scale_id, 0)
    };

    // Fast refine skips MultiDiffusion and tiled VAE (user opt-in; may OOM on Anima).
    // Otherwise split models (Anima/COSMOS) require tiled diffusion for 5D latents.
    let use_tiling =
        !params.upscale_fast_refine && (params.upscale_tiling || params.use_split_model);
    let use_tiled_vae =
        !params.upscale_fast_refine && (params.upscale_tiling || params.use_split_model);

    let latent_source: (String, u32) = if use_tiled_vae {
        let tiled_encode_id = next_id.to_string();
        workflow.insert(
            tiled_encode_id.clone(),
            json!({
                "class_type": "VAEEncodeTiled",
                "inputs": {
                    "pixels": [upscaled_image.0, upscaled_image.1],
                    "vae": [result.vae_source.0.clone(), result.vae_source.1],
                    "tile_size": params.upscale_tile_size,
                    "overlap": 64,
                    "temporal_size": 64,
                    "temporal_overlap": 8
                }
            }),
        );
        *next_id += 1;
        (tiled_encode_id, 0)
    } else {
        let encode_id = next_id.to_string();
        workflow.insert(
            encode_id.clone(),
            json!({
                "class_type": "VAEEncode",
                "inputs": {
                    "pixels": [upscaled_image.0, upscaled_image.1],
                    "vae": [result.vae_source.0.clone(), result.vae_source.1]
                }
            }),
        );
        *next_id += 1;
        (encode_id, 0)
    };

    let model_for_sampler = if use_tiling {
        let tiled_model_id = next_id.to_string();
        workflow.insert(
            tiled_model_id.clone(),
            json!({
                "class_type": "ApplyTiledDiffusion",
                "inputs": {
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "method": "MultiDiffusion",
                    "tile_width": params.upscale_tile_size,
                    "tile_height": params.upscale_tile_size,
                    "tile_overlap": 256
                }
            }),
        );
        *next_id += 1;
        (tiled_model_id, 0u32)
    } else {
        (result.model_source.0.clone(), result.model_source.1)
    };

    // Apply Soft Guidance (CFG rescaling) to prevent hallucination during upscale.
    let model_after_soft = if params.upscale_soft_guidance {
        let soft_id = next_id.to_string();
        workflow.insert(
            soft_id.clone(),
            json!({
                "class_type": "MooshieSoftGuidance",
                "inputs": {
                    "model": [model_for_sampler.0.clone(), model_for_sampler.1],
                    "multiplier": params.upscale_soft_guidance_multiplier
                }
            }),
        );
        *next_id += 1;
        (soft_id, 0u32)
    } else {
        model_for_sampler.clone()
    };

    // For tiled upscales, use quality-only prompts to reduce tile seam artifacts.
    // Each override is applied independently: a positive-only or negative-only
    // override is honoured on its own. The old paired `if let` required BOTH the
    // positive AND negative override to be set, so supplying just one silently
    // discarded it and fell back to the original generation conditioning.
    let pos_source = if use_tiling {
        if let Some(ref pos_text) = params.upscale_positive_prompt {
            let up_pos_id = next_id.to_string();
            workflow.insert(
                up_pos_id.clone(),
                json!({
                    "class_type": "CLIPTextEncode",
                    "inputs": {
                        "clip": [result.clip_source.0.clone(), result.clip_source.1],
                        "text": pos_text
                    }
                }),
            );
            *next_id += 1;
            (up_pos_id, 0u32)
        } else {
            (result.positive_source.0.clone(), result.positive_source.1)
        }
    } else {
        (result.positive_source.0.clone(), result.positive_source.1)
    };

    let neg_source = if use_tiling {
        if let Some(ref neg_text) = params.upscale_negative_prompt {
            let up_neg_id = next_id.to_string();
            workflow.insert(
                up_neg_id.clone(),
                json!({
                    "class_type": "CLIPTextEncode",
                    "inputs": {
                        "clip": [result.clip_source.0.clone(), result.clip_source.1],
                        "text": neg_text
                    }
                }),
            );
            *next_id += 1;
            (up_neg_id, 0u32)
        } else {
            (result.negative_source.0.clone(), result.negative_source.1)
        }
    } else {
        (result.negative_source.0.clone(), result.negative_source.1)
    };

    // Second KSampler pass at low denoise
    let sampler_id = next_id.to_string();
    let is_cfgpp_sampler = params.sampler_name.to_lowercase().contains("cfg_pp");
    // Halve CFG for the low-denoise refine pass, but keep a floor so a low base
    // CFG (or a Flux/distilled model at cfg 0-1) can't drive the upscale sampler
    // to a near-zero CFG, which disables guidance and yields noise.
    let upscale_cfg = if is_cfgpp_sampler {
        (params.cfg / 2.0).max(2.0)
    } else {
        (params.cfg / 2.0).max(1.0)
    };
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_after_soft.0, model_after_soft.1],
                "positive": [pos_source.0, pos_source.1],
                "negative": [neg_source.0, neg_source.1],
                "latent_image": [latent_source.0.clone(), latent_source.1],
                "seed": seed + 1,
                "steps": params.upscale_steps,
                "cfg": upscale_cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": params.upscale_denoise
            }
        }),
    );
    *next_id += 1;

    if use_tiled_vae {
        let tiled_decode_id = next_id.to_string();
        workflow.insert(
            tiled_decode_id.clone(),
            json!({
                "class_type": "VAEDecodeTiled",
                "inputs": {
                    "samples": [sampler_id, 0],
                    "vae": [result.vae_source.0.clone(), result.vae_source.1],
                    "tile_size": params.upscale_tile_size,
                    "overlap": 64,
                    "temporal_size": 64,
                    "temporal_overlap": 8
                }
            }),
        );
        *next_id += 1;
        (tiled_decode_id, 0)
    } else {
        let decode_id = next_id.to_string();
        workflow.insert(
            decode_id.clone(),
            json!({
                "class_type": "VAEDecode",
                "inputs": {
                    "samples": [sampler_id, 0],
                    "vae": [result.vae_source.0.clone(), result.vae_source.1]
                }
            }),
        );
        *next_id += 1;
        (decode_id, 0)
    }
}
