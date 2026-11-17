//! Stable Cascade (Würstchen v3) two-stage workflows.
//!
//! Stable Cascade does not sample in one latent space. Stage C, the model the
//! user picks and the one LoRAs train, samples a tiny 16-channel semantic
//! latent (1024 px becomes 24x24 at the default compression of 42). Stage B
//! then samples the 4-channel stage A latent at a quarter of the image size,
//! guided by that stage C result, and stage A decodes it to pixels. The graph
//! mirrors ComfyUI's official Stable Cascade examples:
//!
//! ```text
//! stage C loader ─ CLIPTextEncode ─────────────┐
//! StableCascade_EmptyLatentImage ─ stage_c ─ KSampler (stage C) ─┐
//!                                                                StableCascade_StageB_Conditioning
//!                                └ stage_b ─ KSampler (stage B) ─┘ ─ VAEDecode (stage A)
//! ```
//!
//! img2img swaps the empty latent for `StableCascade_StageC_VAEEncode`, which
//! encodes the input with the stage C checkpoint's EfficientNet encoder.
//!
//! Stage B gets its own fixed sampler settings (the official 10 steps at CFG
//! 1.1); the user's steps, CFG, sampler and scheduler drive stage C. The
//! refinement chains (hires fix, face fix, segment detail) resample in stage
//! B, which is the only stage that works on full-size images; see
//! [`refiner_cfg`] and `facefix::without_spatial_conditioning`.

use serde_json::json;

use super::{build_scheduled_conditioning, load_model_nodes, WorkflowResult};
use crate::comfyui::types::GenerationParams;

/// `architecture_files` slot of a stage B ComfyUI checkpoint (the
/// all-in-one `stable_cascade_stage_b.safetensors`, which carries stage A).
pub const STAGE_B_CHECKPOINT_SLOT: &str = "stage_b";
/// `architecture_files` slot of a bare stage B diffusion model
/// (`stage_b_bf16.safetensors` and friends), loaded with UNETLoader.
pub const STAGE_B_UNET_SLOT: &str = "stage_b_unet";
/// `architecture_files` slot of the stage A VAE that decodes a bare stage B.
pub const STAGE_A_SLOT: &str = "stage_a";

/// Spatial compression of stage C relative to the image. 42 is what Stable
/// Cascade was trained at (1024 px to a 24x24 stage C latent).
pub const STAGE_C_COMPRESSION: u32 = 42;
/// Stage B sampling, as in the official workflow. Stage B is a decoder that
/// barely needs guidance; more steps or CFG mostly add artifacts.
pub const STAGE_B_STEPS: u32 = 10;
pub const STAGE_B_CFG: f64 = 1.1;
pub const STAGE_B_SAMPLER: &str = "euler_ancestral";
pub const STAGE_B_SCHEDULER: &str = "simple";

/// Whether this request uses the Stable Cascade workflows.
pub fn is_cascade(params: &GenerationParams) -> bool {
    params.model_architecture == "cascade"
}

/// CFG for the refinement samplers (hires fix, face fix, segment detail).
/// They resample with the main model, which for Stable Cascade is stage B, so
/// they take stage B's CFG instead of the user's stage C CFG.
pub fn refiner_cfg(params: &GenerationParams) -> f64 {
    if is_cascade(params) {
        STAGE_B_CFG
    } else {
        params.cfg
    }
}

/// The stage B file this request names, if any.
fn named(params: &GenerationParams, slot: &str) -> Option<String> {
    params
        .architecture_files
        .get(slot)
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// Whether a file name looks like a stage B model (`stage_b`, `stage-b`,
/// `stageb`), matching `isCascadeStageB` in `src/lib/utils/cascade.ts`.
fn is_stage_b_name(name: &str) -> bool {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    ["stage_b", "stage-b", "stageb"]
        .iter()
        .any(|marker| base.contains(marker))
}

/// Why this request cannot run the Stable Cascade workflows, if it cannot.
/// Called from `validate_generation_params` for every Cascade request.
pub fn validate(params: &GenerationParams) -> Result<(), String> {
    let main = if params.use_split_model {
        params.diffusion_model.as_deref().unwrap_or("")
    } else {
        params.checkpoint.as_str()
    };
    if is_stage_b_name(main) {
        return Err("That's Stable Cascade's stage B. Pick the stage C model instead; stage B is added on its own.".into());
    }
    if named(params, STAGE_B_CHECKPOINT_SLOT).is_none() {
        let unet = named(params, STAGE_B_UNET_SLOT).is_some();
        let stage_a = named(params, STAGE_A_SLOT).is_some();
        if !unet {
            return Err("Stable Cascade needs its stage B model next to stage C. Open the model panel to download it.".into());
        }
        if !stage_a {
            return Err("Stable Cascade's stage B diffusion model needs the stage A VAE (stage_a.safetensors). Open the model panel to download it.".into());
        }
    }
    match params.mode.as_str() {
        "txt2img" | "img2img" => {}
        "inpainting" => {
            return Err("Stable Cascade can't inpaint. Use img2img, or switch to another model for inpainting.".into())
        }
        "image_edit" => {
            return Err("Stable Cascade isn't an image edit model. Pick an edit model such as Qwen-Image Edit or FLUX.2 Klein.".into())
        }
        _ => {}
    }
    if params.controlnet.as_ref().is_some_and(|cn| cn.enabled) {
        return Err(
            "ControlNet doesn't work with Stable Cascade. Turn ControlNet off for this model."
                .into(),
        );
    }
    if params.style_ref_enabled {
        return Err(
            "Style reference doesn't work with Stable Cascade. Turn it off for this model.".into(),
        );
    }
    if params.pause_at_step.is_some() || !params.resume_stages.is_empty() {
        return Err("Pause and resume isn't available for Stable Cascade, which samples in two separate stages.".into());
    }
    Ok(())
}

/// Stage B loader: `(model, vae)` sources.
fn load_stage_b(
    workflow: &mut serde_json::Map<String, serde_json::Value>,
    next_id: &mut u32,
    params: &GenerationParams,
) -> ((String, u32), (String, u32)) {
    if let Some(ckpt) = named(params, STAGE_B_CHECKPOINT_SLOT) {
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({
                "class_type": "CheckpointLoaderSimple",
                "inputs": { "ckpt_name": ckpt }
            }),
        );
        *next_id += 1;
        return ((id.clone(), 0), (id, 2));
    }

    let unet = named(params, STAGE_B_UNET_SLOT).unwrap_or_default();
    let unet_id = next_id.to_string();
    let unet_node = if unet.to_ascii_lowercase().ends_with(".gguf") {
        json!({
            "class_type": "UnetLoaderGGUF",
            "inputs": { "unet_name": unet }
        })
    } else {
        json!({
            "class_type": "UNETLoader",
            "inputs": { "unet_name": unet, "weight_dtype": "default" }
        })
    };
    workflow.insert(unet_id.clone(), unet_node);
    *next_id += 1;

    let vae_id = next_id.to_string();
    workflow.insert(
        vae_id.clone(),
        json!({
            "class_type": "VAELoader",
            "inputs": { "vae_name": named(params, STAGE_A_SLOT).unwrap_or_default() }
        }),
    );
    *next_id += 1;
    ((unet_id, 0), (vae_id, 0))
}

/// Shared stage C front: loaders, prompts, then the stage C and stage B
/// latents from `latents` (the empty latent node, or the img2img encode).
struct Front {
    workflow: serde_json::Map<String, serde_json::Value>,
    next_id: u32,
    model_c: (String, u32),
    clip: (String, u32),
    vae_c: (String, u32),
    positive: (String, u32),
    negative: (String, u32),
}

fn build_front(params: &GenerationParams) -> Front {
    let mut workflow = serde_json::Map::new();
    // Stage C: the user's model and LoRAs, through the shared loader
    // (checkpoint, or split UNETLoader + CLIPLoader "stable_cascade" +
    // VAELoader for the EfficientNet encoder).
    // A checkpoint stage C carries its own encoder; a VAE left selected from
    // another model would replace it and break the img2img encode.
    let mut stage_c = params.clone();
    if !stage_c.use_split_model {
        stage_c.vae = None;
    }
    let ml = load_model_nodes(&mut workflow, 1, &stage_c);
    let mut next_id = ml.next_id;

    let (positive, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &ml.clip_source,
        &params.positive_prompt,
        &params.positive_segments,
    );
    next_id = nid;
    let (negative, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &ml.clip_source,
        &params.negative_prompt,
        &params.negative_segments,
    );
    next_id = nid;

    Front {
        workflow,
        next_id,
        model_c: ml.model_source,
        clip: ml.clip_source,
        vae_c: ml.vae_source,
        positive,
        negative,
    }
}

/// Stage B conditioning from a stage C latent: the prompt with the stage C
/// result attached as the prior, and a zeroed negative (stage B's
/// unconditional pass has no text and no prior, as in Stable Cascade's own
/// inference code).
fn stage_b_conditioning(
    front: &mut Front,
    stage_c_latent: (String, u32),
) -> ((String, u32), (String, u32)) {
    let pos_id = front.next_id.to_string();
    front.workflow.insert(
        pos_id.clone(),
        json!({
            "class_type": "StableCascade_StageB_Conditioning",
            "inputs": {
                "conditioning": [front.positive.0.clone(), front.positive.1],
                "stage_c": [stage_c_latent.0, stage_c_latent.1]
            }
        }),
    );
    front.next_id += 1;

    let neg_id = front.next_id.to_string();
    front.workflow.insert(
        neg_id.clone(),
        json!({
            "class_type": "ConditioningZeroOut",
            "inputs": {
                "conditioning": [front.positive.0.clone(), front.positive.1]
            }
        }),
    );
    front.next_id += 1;
    ((pos_id, 0), (neg_id, 0))
}

/// Stage C sampler, stage B conditioning, stage B sampler and stage A decode,
/// from a pair of stage C / stage B latents.
fn finish_two_stage(
    mut front: Front,
    params: &GenerationParams,
    seed: i64,
    stage_c_latent: (String, u32),
    stage_b_latent: (String, u32),
    denoise: f64,
) -> WorkflowResult {
    let sampler_c = front.next_id.to_string();
    front.workflow.insert(
        sampler_c.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [front.model_c.0.clone(), front.model_c.1],
                "positive": [front.positive.0.clone(), front.positive.1],
                "negative": [front.negative.0.clone(), front.negative.1],
                "latent_image": [stage_c_latent.0, stage_c_latent.1],
                "seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": denoise
            }
        }),
    );
    front.next_id += 1;

    let (positive_b, negative_b) = stage_b_conditioning(&mut front, (sampler_c.clone(), 0));
    let mut next_id = front.next_id;
    let (model_b, vae_a) = load_stage_b(&mut front.workflow, &mut next_id, params);

    let sampler_b = next_id.to_string();
    front.workflow.insert(
        sampler_b.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_b.0.clone(), model_b.1],
                "positive": [positive_b.0.clone(), positive_b.1],
                "negative": [negative_b.0.clone(), negative_b.1],
                "latent_image": [stage_b_latent.0, stage_b_latent.1],
                "seed": seed,
                "steps": STAGE_B_STEPS,
                "cfg": STAGE_B_CFG,
                "sampler_name": STAGE_B_SAMPLER,
                "scheduler": STAGE_B_SCHEDULER,
                "denoise": 1.0
            }
        }),
    );
    next_id += 1;

    let decode_id = next_id.to_string();
    front.workflow.insert(
        decode_id.clone(),
        json!({
            "class_type": "VAEDecode",
            "inputs": {
                "samples": [sampler_b, 0],
                "vae": [vae_a.0.clone(), vae_a.1]
            }
        }),
    );
    next_id += 1;

    WorkflowResult {
        workflow: front.workflow,
        next_id,
        image_output: (decode_id, 0),
        // The main sampler is stage C: model patches (Smart Guidance) go there.
        model_source: front.model_c,
        clip_source: front.clip,
        // Refinement passes run in stage B on full-size images, guided by
        // the stage C result.
        positive_source: positive_b,
        negative_source: negative_b,
        vae_source: vae_a,
        sampler_id: sampler_c,
        refiner_model_source: Some(model_b),
        base_sources: None,
    }
}

/// Stable Cascade txt2img.
pub fn build_txt2img(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut front = build_front(params);
    let latent_id = front.next_id.to_string();
    front.workflow.insert(
        latent_id.clone(),
        json!({
            "class_type": "StableCascade_EmptyLatentImage",
            "inputs": {
                "width": params.width,
                "height": params.height,
                "compression": STAGE_C_COMPRESSION,
                "batch_size": params.batch_size.max(1)
            }
        }),
    );
    front.next_id += 1;
    finish_two_stage(
        front,
        params,
        seed,
        (latent_id.clone(), 0),
        (latent_id, 1),
        1.0,
    )
}

/// Stable Cascade img2img: the input is encoded to a stage C latent and
/// resampled at the user's denoise; stage B then redraws the full-size image.
/// Refine-only hands the loaded image straight to the upscale chain, with
/// the encoded stage C latent as the prior its stage B pass follows.
pub fn build_img2img(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut front = build_front(params);

    let load_id = front.next_id.to_string();
    front.workflow.insert(
        load_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": { "image": params.input_image.as_deref().unwrap_or("") }
        }),
    );
    front.next_id += 1;

    let encode_input = if params.refine_only {
        (load_id.clone(), 0)
    } else {
        let resize_id = front.next_id.to_string();
        front.workflow.insert(
            resize_id.clone(),
            json!({
                "class_type": "ImageScale",
                "inputs": {
                    "image": [load_id.clone(), 0],
                    "width": params.width,
                    "height": params.height,
                    "upscale_method": "lanczos",
                    "crop": "disabled"
                }
            }),
        );
        front.next_id += 1;
        (resize_id, 0)
    };

    let encode_id = front.next_id.to_string();
    front.workflow.insert(
        encode_id.clone(),
        json!({
            "class_type": "StableCascade_StageC_VAEEncode",
            "inputs": {
                "image": [encode_input.0, encode_input.1],
                "vae": [front.vae_c.0.clone(), front.vae_c.1],
                "compression": STAGE_C_COMPRESSION
            }
        }),
    );
    front.next_id += 1;

    if params.refine_only {
        let (positive_b, negative_b) = stage_b_conditioning(&mut front, (encode_id, 0));
        let mut next_id = front.next_id;
        let (model_b, vae_a) = load_stage_b(&mut front.workflow, &mut next_id, params);
        return WorkflowResult {
            workflow: front.workflow,
            next_id,
            image_output: (load_id, 0),
            model_source: front.model_c,
            clip_source: front.clip,
            positive_source: positive_b,
            negative_source: negative_b,
            vae_source: vae_a,
            // No main sampler runs in refine-only mode.
            sampler_id: String::new(),
            refiner_model_source: Some(model_b),
            base_sources: None,
        };
    }

    // The encode gives one latent per input image; repeat both for a batch.
    let (stage_c, stage_b) = if params.batch_size > 1 {
        let repeat = |output: u32, front: &mut Front| {
            let id = front.next_id.to_string();
            front.workflow.insert(
                id.clone(),
                json!({
                    "class_type": "RepeatLatentBatch",
                    "inputs": {
                        "samples": [encode_id.clone(), output],
                        "amount": params.batch_size
                    }
                }),
            );
            front.next_id += 1;
            (id, 0)
        };
        let c = repeat(0, &mut front);
        let b = repeat(1, &mut front);
        (c, b)
    } else {
        ((encode_id.clone(), 0), (encode_id, 1))
    };

    finish_two_stage(front, params, seed, stage_c, stage_b, params.denoise)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::graph_test_util::{build, linked, nodes, params, single};

    fn cascade(mode: &str) -> GenerationParams {
        let mut p = params(mode, "cascade");
        p.checkpoint = "stable_cascade_stage_c.safetensors".into();
        p.cfg = 4.0;
        p.sampler_name = "euler_ancestral".into();
        p.scheduler = "simple".into();
        p.architecture_files.insert(
            STAGE_B_CHECKPOINT_SLOT.into(),
            "stable_cascade_stage_b.safetensors".into(),
        );
        p
    }

    /// Stage C runs the user's 20 steps, stage B its fixed 10.
    fn samplers(workflow: &serde_json::Value) -> (&serde_json::Value, &serde_json::Value) {
        let by_steps = |steps: u32| {
            nodes(workflow, "KSampler")
                .into_iter()
                .find(|(_, n)| n["inputs"]["steps"] == steps)
                .map(|(_, n)| n)
                .unwrap_or_else(|| panic!("no KSampler with {steps} steps"))
        };
        (by_steps(20), by_steps(STAGE_B_STEPS))
    }

    #[test]
    fn txt2img_runs_stage_c_then_stage_b() {
        let w = build(&cascade("txt2img"));
        let latent = single(&w, "StableCascade_EmptyLatentImage");
        assert_eq!(latent["inputs"]["compression"], STAGE_C_COMPRESSION);
        assert_eq!(latent["inputs"]["width"], 1024);
        assert!(nodes(&w, "EmptyLatentImage").is_empty());

        let (c, b) = samplers(&w);
        assert_eq!(c["inputs"]["latent_image"][1], 0);
        assert_eq!(c["inputs"]["cfg"], 4.0);
        assert_eq!(b["inputs"]["latent_image"][1], 1);
        assert_eq!(b["inputs"]["cfg"], STAGE_B_CFG);
        assert_eq!(b["inputs"]["sampler_name"], STAGE_B_SAMPLER);

        // Stage B is conditioned on the stage C result.
        let cond = linked(&w, &b["inputs"]["positive"]);
        assert_eq!(cond["class_type"], "StableCascade_StageB_Conditioning");
        assert_eq!(
            linked(&w, &cond["inputs"]["stage_c"]),
            c,
            "stage B prior is the stage C sampler output"
        );
        assert_eq!(
            linked(&w, &b["inputs"]["negative"])["class_type"],
            "ConditioningZeroOut"
        );

        // Two loaders: stage C (the user's model) and stage B.
        let ckpts: Vec<_> = nodes(&w, "CheckpointLoaderSimple")
            .into_iter()
            .map(|(_, n)| n["inputs"]["ckpt_name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ckpts.len(), 2);
        assert!(ckpts.contains(&"stable_cascade_stage_b.safetensors".to_string()));
        assert_eq!(
            linked(&w, &b["inputs"]["model"])["inputs"]["ckpt_name"],
            "stable_cascade_stage_b.safetensors"
        );

        // Decoded by stage B's stage A VAE.
        let decode = single(&w, "VAEDecode");
        assert_eq!(linked(&w, &decode["inputs"]["samples"]), b);
        assert_eq!(decode["inputs"]["vae"][1], 2);
        assert_eq!(
            linked(&w, &decode["inputs"]["vae"])["inputs"]["ckpt_name"],
            "stable_cascade_stage_b.safetensors"
        );
    }

    #[test]
    fn split_stage_b_loads_unet_and_stage_a() {
        let mut p = cascade("txt2img");
        p.architecture_files.clear();
        p.architecture_files
            .insert(STAGE_B_UNET_SLOT.into(), "stage_b_bf16.safetensors".into());
        p.architecture_files
            .insert(STAGE_A_SLOT.into(), "stage_a.safetensors".into());
        assert!(validate(&p).is_ok());
        let w = build(&p);
        let (_, b) = samplers(&w);
        assert_eq!(
            linked(&w, &b["inputs"]["model"])["inputs"]["unet_name"],
            "stage_b_bf16.safetensors"
        );
        let decode = single(&w, "VAEDecode");
        assert_eq!(
            linked(&w, &decode["inputs"]["vae"])["inputs"]["vae_name"],
            "stage_a.safetensors"
        );
    }

    #[test]
    fn img2img_encodes_with_the_stage_c_encoder() {
        let mut p = cascade("img2img");
        p.denoise = 0.6;
        // A VAE left over from another model must not replace the encoder.
        p.vae = Some("sdxl_vae.safetensors".into());
        let w = build(&p);
        assert!(nodes(&w, "VAELoader").is_empty());
        let encode = single(&w, "StableCascade_StageC_VAEEncode");
        // The stage C checkpoint's VAE is the EfficientNet encoder.
        assert_eq!(
            linked(&w, &encode["inputs"]["vae"])["inputs"]["ckpt_name"],
            "stable_cascade_stage_c.safetensors"
        );
        assert!(nodes(&w, "VAEEncode").is_empty());
        let (c, b) = samplers(&w);
        assert_eq!(c["inputs"]["denoise"], 0.6);
        assert_eq!(b["inputs"]["denoise"], 1.0);
        assert_eq!(b["inputs"]["latent_image"][1], 1);
    }

    #[test]
    fn hires_fix_resamples_in_stage_b_without_tiling() {
        let mut p = cascade("txt2img");
        p.upscale_enabled = true;
        p.upscale_method = "latent".into();
        p.upscale_scale = 1.5;
        p.upscale_steps = 8;
        p.upscale_denoise = 0.4;
        p.upscale_tiling = true;
        let w = build(&p);
        let refine = nodes(&w, "KSampler")
            .into_iter()
            .find(|(_, n)| n["inputs"]["steps"] == 8)
            .expect("refine sampler")
            .1;
        assert_eq!(refine["inputs"]["cfg"], STAGE_B_CFG);
        assert_eq!(
            linked(&w, &refine["inputs"]["model"])["inputs"]["ckpt_name"],
            "stable_cascade_stage_b.safetensors"
        );
        assert_eq!(
            linked(&w, &refine["inputs"]["positive"])["class_type"],
            "StableCascade_StageB_Conditioning"
        );
        assert!(nodes(&w, "ApplyTiledDiffusion").is_empty());
    }

    #[test]
    fn face_fix_drops_the_full_image_prior() {
        let mut p = cascade("txt2img");
        p.facefix_enabled = true;
        let w = build(&p);
        let detailer = single(&w, "MooshieFaceDetailer");
        assert_eq!(detailer["inputs"]["cfg"], STAGE_B_CFG);
        assert_eq!(
            linked(&w, &detailer["inputs"]["positive"])["class_type"],
            "CLIPTextEncode"
        );
        assert_eq!(
            linked(&w, &detailer["inputs"]["model"])["inputs"]["ckpt_name"],
            "stable_cascade_stage_b.safetensors"
        );
    }

    #[test]
    fn refine_only_uses_the_encoded_input_as_prior() {
        let mut p = cascade("img2img");
        p.refine_only = true;
        p.upscale_enabled = true;
        p.upscale_method = "latent".into();
        p.upscale_scale = 2.0;
        p.upscale_steps = 8;
        let w = build(&p);
        let refine = single(&w, "KSampler");
        let cond = linked(&w, &refine["inputs"]["positive"]);
        assert_eq!(
            linked(&w, &cond["inputs"]["stage_c"])["class_type"],
            "StableCascade_StageC_VAEEncode"
        );
    }

    #[test]
    fn validation_requires_stage_b_and_rejects_unsupported_modes() {
        let mut p = cascade("txt2img");
        assert!(validate(&p).is_ok());
        p.architecture_files.clear();
        assert!(validate(&p).unwrap_err().contains("stage B"));
        p.architecture_files
            .insert(STAGE_B_UNET_SLOT.into(), "stage_b_bf16.safetensors".into());
        assert!(validate(&p).unwrap_err().contains("stage A"));

        for mode in ["inpainting", "image_edit"] {
            assert!(validate(&cascade(mode)).is_err(), "{mode}");
        }
        let mut wrong = cascade("txt2img");
        wrong.checkpoint = "models\\stable_cascade_stage_b.safetensors".into();
        assert!(validate(&wrong).unwrap_err().contains("stage C"));

        let mut paused = cascade("txt2img");
        paused.pause_at_step = Some(10);
        assert!(validate(&paused).is_err());
    }
}
