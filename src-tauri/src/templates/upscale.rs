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
        && params
            .upscale_model
            .as_ref()
            .map_or(false, |m| !m.is_empty());

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
        (upscale_id, 0)
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

    // Step 2: Tiled VAE Encode
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

    // Step 3: Apply Tiled Diffusion
    // For split models (Anima/COSMOS): always use tiled diffusion — required for 5D latents.
    // For standard models: optional, controlled by user toggle.
    let use_tiling = params.upscale_tiling || params.use_split_model;
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
                    "tile_overlap": 128
                }
            }),
        );
        *next_id += 1;
        (tiled_model_id, 0u32)
    } else {
        (result.model_source.0.clone(), result.model_source.1)
    };

    // Step 4: Second KSampler pass at low denoise
    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_for_sampler.0, model_for_sampler.1],
                "positive": [result.positive_id.clone(), 0],
                "negative": [result.negative_id.clone(), 0],
                "latent_image": [tiled_encode_id, 0],
                "seed": seed + 1,
                "steps": params.upscale_steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": params.upscale_denoise
            }
        }),
    );
    *next_id += 1;

    // Step 5: Tiled VAE Decode
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
}
