//! Anima txt2img workflow using Untwisting RoPE training-free style transfer.
//! Ported from https://github.com/BigStationW/ComfyUi-Untwisting-RoPE workflows/workflow_anima.json

use serde_json::json;

use super::{build_scheduled_conditioning, insert_vae_decode, load_model_nodes, WorkflowResult};
use crate::comfyui::types::GenerationParams;

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let style_image = params
        .style_reference_image
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .expect("style_reference_image required when style_transfer_enabled");

    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    let (pos_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.positive_prompt,
        &params.positive_segments,
    );
    next_id = nid;

    let (neg_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.negative_prompt,
        &params.negative_segments,
    );
    next_id = nid;

    // Load style reference image
    let load_image_id = next_id.to_string();
    workflow.insert(
        load_image_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": {
                "image": style_image
            }
        }),
    );
    next_id += 1;

    // Scale reference to target megapixels; outputs width/height for empty latent
    let scale_id = next_id.to_string();
    workflow.insert(
        scale_id.clone(),
        json!({
            "class_type": "ImageScaleToTotalPixelsX",
            "inputs": {
                "image": [load_image_id, 0],
                "megapixels": params.style_transfer_megapixels,
                "multiple_of": 16,
                "resize_mode": "crop",
                "upscale_method": "lanczos"
            }
        }),
    );
    next_id += 1;

    // Encode reference latent for RF inversion
    let vae_encode_id = next_id.to_string();
    workflow.insert(
        vae_encode_id.clone(),
        json!({
            "class_type": "VAEEncode",
            "inputs": {
                "pixels": [scale_id.clone(), 0],
                "vae": [vae_source.0.clone(), vae_source.1]
            }
        }),
    );
    next_id += 1;

    // RF inversion — target prompt as ref_conditioning (per upstream recommendation)
    let rf_id = next_id.to_string();
    workflow.insert(
        rf_id.clone(),
        json!({
            "class_type": "RFInversion",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "reference_latent": [vae_encode_id, 0],
                "ref_conditioning": [pos_source.0.clone(), pos_source.1],
                "rf_mode": params.style_transfer_rf_mode,
                "gamma": params.style_transfer_gamma,
                "gamma_curve": params.style_transfer_gamma_curve,
                "norm_strength": params.style_transfer_norm_strength,
                "pmi_alpha": params.style_transfer_pmi_alpha,
                "verbose": false
            }
        }),
    );
    next_id += 1;

    let rf_model_source = (rf_id.clone(), 0);
    let rf_latent_source = (rf_id, 1);

    // Untwisting RoPE model patch
    let untwist_id = next_id.to_string();
    workflow.insert(
        untwist_id.clone(),
        json!({
            "class_type": "UntwistingRoPE",
            "inputs": {
                "model": [rf_model_source.0.clone(), rf_model_source.1],
                "rf_inversion": [rf_latent_source.0, rf_latent_source.1],
                "beta": params.style_transfer_beta,
                "high_scale_start": params.style_transfer_high_scale_start,
                "high_scale_end": 0.0,
                "low_scale_start": 1.0,
                "low_scale_end": params.style_transfer_low_scale_end,
                "start_block": 0,
                "end_block": 999,
                "adain_strength": params.style_transfer_adain_strength,
                "verbose": false
            }
        }),
    );
    next_id += 1;

    let patched_model = (untwist_id, 0);

    // Empty latent sized from reference image
    let latent_id = next_id.to_string();
    workflow.insert(
        latent_id.clone(),
        json!({
            "class_type": "EmptySD3LatentImage",
            "inputs": {
                "width": [scale_id.clone(), 1],
                "height": [scale_id.clone(), 2],
                "batch_size": params.batch_size
            }
        }),
    );
    next_id += 1;

    // CFGGuider + SamplerCustomAdvanced (reference workflow)
    let guider_id = next_id.to_string();
    workflow.insert(
        guider_id.clone(),
        json!({
            "class_type": "CFGGuider",
            "inputs": {
                "model": [patched_model.0.clone(), patched_model.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "cfg": params.cfg
            }
        }),
    );
    next_id += 1;

    let sampler_select_id = next_id.to_string();
    workflow.insert(
        sampler_select_id.clone(),
        json!({
            "class_type": "KSamplerSelect",
            "inputs": {
                "sampler_name": params.sampler_name
            }
        }),
    );
    next_id += 1;

    let noise_id = next_id.to_string();
    workflow.insert(
        noise_id.clone(),
        json!({
            "class_type": "RandomNoise",
            "inputs": {
                "noise_seed": seed,
                "control_after_generate": "fixed"
            }
        }),
    );
    next_id += 1;

    let scheduler_id = next_id.to_string();
    workflow.insert(
        scheduler_id.clone(),
        json!({
            "class_type": "BasicScheduler",
            "inputs": {
                "model": [patched_model.0.clone(), patched_model.1],
                "scheduler": params.scheduler,
                "steps": params.steps,
                "denoise": 1.0
            }
        }),
    );
    next_id += 1;

    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "SamplerCustomAdvanced",
            "inputs": {
                "noise": [noise_id, 0],
                "guider": [guider_id, 0],
                "sampler": [sampler_select_id, 0],
                "sigmas": [scheduler_id, 0],
                "latent_image": [latent_id, 0]
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
        model_source: patched_model,
        clip_source,
        positive_source: pos_source,
        negative_source: neg_source,
        vae_source,
        sampler_id,
    }
}
