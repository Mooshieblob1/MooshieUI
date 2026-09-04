use serde_json::json;

use super::WorkflowResult;
use crate::comfyui::types::GenerationParams;

/// Model files from the official `Comfy-Org/SeedVR2` HF repo — the exact
/// filenames ComfyUI's bundled "SeedVR2 3B Int8" template loads. The frontend
/// download UI must fetch these same names into diffusion_models/ and vae/.
pub const SEEDVR2_UNET_FILE: &str = "seedvr2_3b_int8_convrot.safetensors";
pub const SEEDVR2_VAE_FILE: &str = "seedvr2_ema_vae_fp16.safetensors";

/// Appends the upscale node chain to an existing workflow.
/// Returns the (node_id, output_index) of the final upscaled IMAGE.
pub fn append_upscale_chain(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    seed: i64,
) -> (String, u32) {
    if params.upscale_method == "seedvr2" {
        return append_seedvr2_chain(result, params, seed);
    }

    let refiner_model = result.refiner_model();
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
                    "overlap": (params.upscale_tile_size / 8).max(64),
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
                    "model": [refiner_model.0.clone(), refiner_model.1],
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
        refiner_model
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
    //
    // Refine-only skips the base sampling pass entirely, so this KSampler is the
    // only guidance the image ever receives. Halving there just under-guides the
    // refine and leaves the upscaler's high-frequency noise in place, so keep the
    // model's full CFG instead.
    let upscale_cfg = if params.refine_only {
        params.cfg
    } else if is_cfgpp_sampler {
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
                    "overlap": (params.upscale_tile_size / 8).max(64),
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

/// SeedVR2 restoration upscale: a self-contained chain that ignores the base
/// checkpoint entirely and runs the image through the dedicated SeedVR2 3B
/// restoration model instead. Node chain and sampler settings mirror
/// ComfyUI's official "SeedVR2 3B Int8: Upscale Image" template:
/// resize -> SeedVR2Preprocess -> VAEEncodeTiled -> SeedVR2Conditioning ->
/// KSampler(1 step, cfg 1, euler/simple, denoise 1) -> VAEDecodeTiled ->
/// SeedVR2PostProcessing. `original_resized_images` takes the resize output
/// (pre-Preprocess), matching the template's wiring.
fn append_seedvr2_chain(
    result: &mut WorkflowResult,
    params: &GenerationParams,
    seed: i64,
) -> (String, u32) {
    let next_id = &mut result.next_id;
    let workflow = &mut result.workflow;

    let resize_id = next_id.to_string();
    workflow.insert(
        resize_id.clone(),
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

    let preprocess_id = next_id.to_string();
    workflow.insert(
        preprocess_id.clone(),
        json!({
            "class_type": "SeedVR2Preprocess",
            "inputs": {
                "resized_images": [resize_id.clone(), 0]
            }
        }),
    );
    *next_id += 1;

    let unet_id = next_id.to_string();
    workflow.insert(
        unet_id.clone(),
        json!({
            "class_type": "UNETLoader",
            "inputs": {
                "unet_name": SEEDVR2_UNET_FILE,
                "weight_dtype": "default"
            }
        }),
    );
    *next_id += 1;

    let vae_id = next_id.to_string();
    workflow.insert(
        vae_id.clone(),
        json!({
            "class_type": "VAELoader",
            "inputs": {
                "vae_name": SEEDVR2_VAE_FILE
            }
        }),
    );
    *next_id += 1;

    let encode_id = next_id.to_string();
    workflow.insert(
        encode_id.clone(),
        json!({
            "class_type": "VAEEncodeTiled",
            "inputs": {
                "pixels": [preprocess_id, 0],
                "vae": [vae_id.clone(), 0],
                "tile_size": 512,
                "overlap": 128,
                "temporal_size": 4096,
                "temporal_overlap": 8
            }
        }),
    );
    *next_id += 1;

    let conditioning_id = next_id.to_string();
    workflow.insert(
        conditioning_id.clone(),
        json!({
            "class_type": "SeedVR2Conditioning",
            "inputs": {
                "model": [unet_id.clone(), 0],
                "vae_conditioning": [encode_id.clone(), 0]
            }
        }),
    );
    *next_id += 1;

    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [unet_id, 0],
                "positive": [conditioning_id.clone(), 0],
                "negative": [conditioning_id, 1],
                "latent_image": [encode_id, 0],
                "seed": seed + 1,
                "steps": 1,
                "cfg": 1.0,
                "sampler_name": "euler",
                "scheduler": "simple",
                "denoise": 1.0
            }
        }),
    );
    *next_id += 1;

    let decode_id = next_id.to_string();
    workflow.insert(
        decode_id.clone(),
        json!({
            "class_type": "VAEDecodeTiled",
            "inputs": {
                "samples": [sampler_id, 0],
                "vae": [vae_id, 0],
                "tile_size": 512,
                "overlap": 128,
                "temporal_size": 4096,
                "temporal_overlap": 8
            }
        }),
    );
    *next_id += 1;

    let postprocess_id = next_id.to_string();
    workflow.insert(
        postprocess_id.clone(),
        json!({
            "class_type": "SeedVR2PostProcessing",
            "inputs": {
                "images": [decode_id, 0],
                "original_resized_images": [resize_id, 0],
                "color_correction_method": "none"
            }
        }),
    );
    *next_id += 1;

    (postprocess_id, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_result() -> WorkflowResult {
        WorkflowResult {
            workflow: serde_json::Map::new(),
            next_id: 10,
            image_output: ("8".into(), 0),
            model_source: ("1".into(), 0),
            clip_source: ("1".into(), 1),
            positive_source: ("2".into(), 0),
            negative_source: ("3".into(), 0),
            vae_source: ("1".into(), 2),
            sampler_id: "7".into(),
            refiner_model_source: None,
            base_sources: None,
        }
    }

    fn seedvr2_params() -> GenerationParams {
        serde_json::from_value(serde_json::json!({
            "mode": "txt2img",
            "positive_prompt": "girl",
            "negative_prompt": "bad",
            "checkpoint": "model.safetensors",
            "loras": [],
            "sampler_name": "euler",
            "scheduler": "normal",
            "steps": 20,
            "cfg": 7.0,
            "seed": "42",
            "width": 1024,
            "height": 1024,
            "batch_size": 1,
            "denoise": 1.0,
            "upscale_enabled": true,
            "upscale_method": "seedvr2",
            "upscale_scale": 2.0,
            "upscale_denoise": 0.4,
            "upscale_steps": 20,
            "upscale_tile_size": 1024,
            "upscale_tiling": false,
        }))
        .expect("params")
    }

    fn nodes_of_type<'a>(
        result: &'a WorkflowResult,
        class_type: &str,
    ) -> Vec<&'a serde_json::Value> {
        result
            .workflow
            .values()
            .filter(|n| n["class_type"] == class_type)
            .collect()
    }

    #[test]
    fn seedvr2_method_builds_the_dedicated_restoration_chain() {
        let mut result = base_result();
        let (out_id, out_port) = append_upscale_chain(&mut result, &seedvr2_params(), 42);

        for class in [
            "ImageScaleBy",
            "SeedVR2Preprocess",
            "UNETLoader",
            "VAELoader",
            "VAEEncodeTiled",
            "SeedVR2Conditioning",
            "KSampler",
            "VAEDecodeTiled",
            "SeedVR2PostProcessing",
        ] {
            assert_eq!(nodes_of_type(&result, class).len(), 1, "missing {class}");
        }
        assert_eq!(out_port, 0);
        assert_eq!(
            result.workflow[&out_id]["class_type"],
            "SeedVR2PostProcessing"
        );

        // SeedVR2 is a one-step restoration model: the official template pins
        // these sampler settings, and the user's own sampler/steps/cfg must
        // not leak into this pass.
        let sampler = nodes_of_type(&result, "KSampler")[0];
        assert_eq!(sampler["inputs"]["steps"], 1);
        assert_eq!(sampler["inputs"]["cfg"], 1.0);
        assert_eq!(sampler["inputs"]["sampler_name"], "euler");
        assert_eq!(sampler["inputs"]["scheduler"], "simple");
        assert_eq!(sampler["inputs"]["denoise"], 1.0);

        // The base checkpoint's model/vae/conditioning are never referenced.
        let loader = nodes_of_type(&result, "UNETLoader")[0];
        assert_eq!(loader["inputs"]["unet_name"], SEEDVR2_UNET_FILE);
        let vae = nodes_of_type(&result, "VAELoader")[0];
        assert_eq!(vae["inputs"]["vae_name"], SEEDVR2_VAE_FILE);

        // PostProcessing compares against the raw resize output, not the
        // Preprocess output (matches the official template wiring).
        let post = nodes_of_type(&result, "SeedVR2PostProcessing")[0];
        let resize_ref = &post["inputs"]["original_resized_images"][0];
        assert_eq!(
            result.workflow[resize_ref.as_str().unwrap()]["class_type"],
            "ImageScaleBy"
        );
    }
}
