use serde_json::json;

use super::{
    build_scheduled_conditioning, insert_vae_decode, is_sdxl_like_family, load_model_nodes,
    needs_flux2_latent, needs_sd3_latent, strip_lora_tags, WorkflowResult,
};
use crate::comfyui::types::GenerationParams;

/// Latent upscale between the two Moody Moss 4K HD passes (`LatentUpscaleBy`,
/// bicubic), as in the author's V3 workflow.
pub const MOODY_MOSS_LATENT_SCALE: f64 = 1.5;

/// Seed offset of the second Moody Moss pass. The workflow adds 72 to the
/// first pass's seed; the author notes MOSS gives better detail when the two
/// seeds differ.
pub const MOODY_MOSS_SECOND_SEED_OFFSET: u64 = 72;

/// Step layout of the two Moody Moss 4K HD passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoodyMossPasses {
    /// Length of the first pass's schedule.
    pub first_steps: u32,
    /// Step the first pass stops at, keeping the leftover noise.
    pub first_end: u32,
    /// Step of the full schedule the second pass starts from.
    pub second_start: u32,
}

/// The author's workflow, at 25 steps, samples 15 of an 18-step schedule,
/// then runs the 25-step schedule from step 6 on the upscaled latent. Other
/// step counts keep those proportions.
pub fn moody_moss_passes(steps: u32) -> MoodyMossPasses {
    let steps = steps.max(2);
    let scale = |part: u32| (steps * part + 12) / 25;
    let first_steps = scale(18).max(2);
    let first_end = scale(15).clamp(1, first_steps - 1);
    let second_start = scale(6).min(steps - 1);
    MoodyMossPasses {
        first_steps,
        first_end,
        second_start,
    }
}

/// Whether this request runs the Moody Moss 4K HD two-pass txt2img. The
/// frontend only sets the flag for a Moody Moss Mix checkpoint; the checks
/// here keep a stray flag from reshaping any other model or mode, and a
/// paused run keeps its single-sampler stages.
pub fn moody_moss_hd_active(params: &GenerationParams) -> bool {
    params.moody_moss_hd
        && params.mode == "txt2img"
        && params.stage.is_none()
        && matches!(
            params.model_architecture.as_str(),
            "mossimage" | "mossimagebase"
        )
}

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    // A later stage of a paused run allocates its node IDs after the stages
    // before it, whose nodes it shares the graph with.
    let next_id: u32 = params.stage.as_ref().map_or(1, |s| s.first_id.max(1));

    // Load model (checkpoint or split UNETLoader + CLIPLoader + VAELoader)
    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;
    let base_sources = ml.base;

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
    let supports_regions = is_sdxl_like_family(&params.model_architecture);
    if params.mode == "txt2img" && supports_regions {
        for region in &params.positive_regions {
            // The frontend sends each region already merged with the prompt
            // context (without scheduled segments, which must stay inside
            // their timestep window), so it is encoded as is. Only the LoRA
            // tags that context carries are removed.
            let text = strip_lora_tags(&region.text).trim().to_string();
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

    let stage = params.stage.as_ref();

    // Latent: a resumed stage continues from the previous stage's sampler
    // output; otherwise an empty latent of the architecture-specific kind.
    let latent_source: (String, u32) = if let Some(latent) = stage.and_then(|s| s.latent.clone()) {
        latent
    } else {
        let latent_id = next_id.to_string();
        let use_flux2_latent = needs_flux2_latent(params);
        let use_sd3_latent = needs_sd3_latent(params);
        workflow.insert(
            latent_id.clone(),
            json!({
                "class_type":
                if use_flux2_latent {
                    "EmptyFlux2LatentImage"
                } else if use_sd3_latent {
                    "EmptySD3LatentImage"
                } else {
                    "EmptyLatentImage"
                },
                "inputs": {
                    "width": params.width,
                    "height": params.height,
                    "batch_size": params.batch_size
                }
            }),
        );
        next_id += 1;
        (latent_id, 0)
    };

    // Sampler. A plain KSampler runs the whole schedule; a paused or resumed
    // stage uses KSamplerAdvanced so it can stop early and hand the leftover
    // noise to the next stage, or pick up from a stage that did.
    let start_step = stage.map_or(0, |s| s.start_step);
    let end_step = stage.and_then(|s| s.end_step);
    let custom_tail = stage.is_some_and(|s| s.custom_tail);

    // A finishing stage on a different scheduler or step count samples an
    // explicit sigma tail. `inject_resume_stage` builds the tail from the
    // patched model and fills in `sigmas`; the placeholder never reaches
    // ComfyUI.
    let sampler_select_id = if custom_tail {
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({
                "class_type": "KSamplerSelect",
                "inputs": { "sampler_name": params.sampler_name }
            }),
        );
        next_id += 1;
        Some(id)
    } else {
        None
    };

    let moody_passes = moody_moss_hd_active(params).then(|| moody_moss_passes(params.steps));

    let sampler_id = next_id.to_string();
    let sampler_node = if let Some(select_id) = sampler_select_id {
        json!({
            "class_type": "SamplerCustom",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [latent_source.0, latent_source.1],
                "add_noise": false,
                "noise_seed": seed,
                "cfg": params.cfg,
                "sampler": [select_id, 0],
                "sigmas": ["0", 0]
            }
        })
    } else if let Some(passes) = moody_passes {
        // Moody Moss 4K HD first pass: part of a shorter schedule, stopping
        // with the leftover noise still in the latent.
        json!({
            "class_type": "KSamplerAdvanced",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [latent_source.0, latent_source.1],
                "add_noise": "enable",
                "noise_seed": seed,
                "steps": passes.first_steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "start_at_step": 0,
                "end_at_step": passes.first_end,
                "return_with_leftover_noise": "enable"
            }
        })
    } else if start_step == 0 && end_step.is_none() {
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [latent_source.0, latent_source.1],
                "seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": 1.0
            }
        })
    } else {
        json!({
            "class_type": "KSamplerAdvanced",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [latent_source.0, latent_source.1],
                // Noise is added once, by the stage that starts the schedule.
                "add_noise": if start_step == 0 { "enable" } else { "disable" },
                "noise_seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "start_at_step": start_step,
                "end_at_step": end_step.unwrap_or(10000),
                // A stage that stops early must keep the remaining noise in
                // the latent, or the next stage would resume from a clean
                // latent at the wrong noise level.
                "return_with_leftover_noise": if end_step.is_some() { "enable" } else { "disable" }
            }
        })
    };
    workflow.insert(sampler_id.clone(), sampler_node);
    next_id += 1;

    // Moody Moss 4K HD: the node above is the short first pass, and a
    // latent upscale plus a second pass on the full schedule produce the
    // image. The second pass is what gets decoded, patched and refined, so it
    // takes over `sampler_id`.
    let sampler_id = if let Some(passes) = moody_passes {
        let upscale_id = next_id.to_string();
        workflow.insert(
            upscale_id.clone(),
            json!({
                "class_type": "LatentUpscaleBy",
                "inputs": {
                    "samples": [sampler_id.clone(), 0],
                    "upscale_method": "bicubic",
                    "scale_by": MOODY_MOSS_LATENT_SCALE
                }
            }),
        );
        next_id += 1;

        let second_id = next_id.to_string();
        workflow.insert(
            second_id.clone(),
            json!({
                "class_type": "KSamplerAdvanced",
                "inputs": {
                    "model": [model_source.0.clone(), model_source.1],
                    "positive": [pos_source.0.clone(), pos_source.1],
                    "negative": [neg_source.0.clone(), neg_source.1],
                    "latent_image": [upscale_id, 0],
                    // The workflow adds fresh noise on top of the leftover
                    // noise, from a seed of its own.
                    "add_noise": "enable",
                    "noise_seed": super::offset_seed(seed, MOODY_MOSS_SECOND_SEED_OFFSET),
                    "steps": params.steps,
                    "cfg": params.cfg,
                    "sampler_name": params.sampler_name,
                    "scheduler": params.scheduler,
                    "start_at_step": passes.second_start,
                    "end_at_step": 10000,
                    "return_with_leftover_noise": "disable"
                }
            }),
        );
        next_id += 1;
        second_id
    } else {
        sampler_id
    };

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
        refiner_model_source: None,
        base_sources: Some(base_sources),
    }
}
