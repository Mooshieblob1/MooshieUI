use serde_json::json;

use super::{
    build_scheduled_conditioning_for_arch, insert_vae_decode, is_vpred_model, load_model_nodes,
    WorkflowResult,
};
use crate::comfyui::types::GenerationParams;

/// Whether the model benefits from `InpaintModelConditioning` (concat latent +
/// noise mask) instead of plain `VAEEncode` + `SetLatentNoiseMask`.
///
/// Flux, SD3, Qwen, MOSS, and dedicated inpaint/fill checkpoints expect
/// conditioning-level mask concatenation, whereas standard 4-channel SD1.5 /
/// SDXL / Illustrious / Pony / Anima models blend cleanly with
/// `VAEEncode` + `SetLatentNoiseMask` (plus `DifferentialDiffusion`).
pub fn uses_inpaint_model_conditioning(params: &GenerationParams) -> bool {
    let arch = params.model_architecture.as_str();
    let is_dit_inpaint_family = matches!(
        arch,
        "flux"
            | "flux1d"
            | "flux1s"
            | "flux1krea"
            | "flux1kontext"
            | "flux2"
            | "flux2klein4b"
            | "flux2klein9b"
            | "flux3klein"
            | "flux3kleinbase"
            | "sd3"
            | "sd35l"
            | "sd35m"
            | "mossimage"
            | "qwen"
            | "qwen_edit"
            | "qwen_edit_plus"
            | "qwen21"
    ) || arch.starts_with("flux")
        || arch.starts_with("sd3");

    if is_dit_inpaint_family {
        return true;
    }

    let model_name = if params.use_split_model {
        params.diffusion_model.as_deref().unwrap_or("")
    } else {
        params.checkpoint.as_str()
    }
    .to_lowercase();

    model_name.contains("inpaint")
        || model_name.contains("flux-fill")
        || model_name.contains("flux_fill")
}

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    // Load model (checkpoint or split UNETLoader + CLIPLoader + VAELoader)
    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    // Positive conditioning (with optional timestep scheduling and CLIP BREAK chunking)
    let (mut pos_source, nid) = build_scheduled_conditioning_for_arch(
        &mut workflow,
        next_id,
        &clip_source,
        &params.positive_prompt,
        &params.positive_segments,
        &params.model_architecture,
    );
    next_id = nid;

    // Negative conditioning (with optional timestep scheduling and CLIP BREAK chunking)
    let (mut neg_source, nid) = build_scheduled_conditioning_for_arch(
        &mut workflow,
        next_id,
        &clip_source,
        &params.negative_prompt,
        &params.negative_segments,
        &params.model_architecture,
    );
    next_id = nid;

    // Load source image
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

    // Automatically pad image and produce a feathered outpainting mask
    let pad_id = next_id.to_string();
    workflow.insert(
        pad_id.clone(),
        json!({
            "class_type": "ImagePadForOutpaint",
            "inputs": {
                "image": [load_img_id, 0],
                "left": params.outpaint_left,
                "top": params.outpaint_top,
                "right": params.outpaint_right,
                "bottom": params.outpaint_bottom,
                "feathering": params.outpaint_feather
            }
        }),
    );
    next_id += 1;

    let (masked_latent_id, use_inpaint_cond) = if uses_inpaint_model_conditioning(params) {
        let inpaint_cond_id = next_id.to_string();
        workflow.insert(
            inpaint_cond_id.clone(),
            json!({
                "class_type": "InpaintModelConditioning",
                "inputs": {
                    "positive": [pos_source.0.clone(), pos_source.1],
                    "negative": [neg_source.0.clone(), neg_source.1],
                    "vae": [vae_source.0.clone(), vae_source.1],
                    "pixels": [pad_id, 0],
                    "mask": [pad_id, 1],
                    "noise_mask": true
                }
            }),
        );
        next_id += 1;
        pos_source = (inpaint_cond_id.clone(), 0);
        neg_source = (inpaint_cond_id.clone(), 1);
        (inpaint_cond_id, true)
    } else {
        // Encode padded image to latent space, then apply the feathered noise mask
        let encode_id = next_id.to_string();
        workflow.insert(
            encode_id.clone(),
            json!({
                "class_type": "VAEEncode",
                "inputs": {
                    "pixels": [pad_id, 0],
                    "vae": [vae_source.0.clone(), vae_source.1]
                }
            }),
        );
        next_id += 1;

        let mask_latent_id = next_id.to_string();
        workflow.insert(
            mask_latent_id.clone(),
            json!({
                "class_type": "SetLatentNoiseMask",
                "inputs": {
                    "samples": [encode_id, 0],
                    "mask": [pad_id, 1]
                }
            }),
        );
        next_id += 1;
        (mask_latent_id, false)
    };

    let latent_port = if use_inpaint_cond { 2 } else { 0 };
    let latent_source = if params.batch_size > 1 {
        let repeat_id = next_id.to_string();
        workflow.insert(
            repeat_id.clone(),
            json!({
                "class_type": "RepeatLatentBatch",
                "inputs": {
                    "samples": [masked_latent_id, latent_port],
                    "amount": params.batch_size
                }
            }),
        );
        next_id += 1;
        (repeat_id, 0)
    } else {
        (masked_latent_id, latent_port)
    };

    let sampler_name_lc = params.sampler_name.to_lowercase();
    let is_cfgpp_sampler = sampler_name_lc.contains("cfg_pp");
    let is_vpred_or_anima = is_vpred_model(params) || params.model_architecture == "anima";
    let use_differential_diffusion = !use_inpaint_cond
        && (params.differential_diffusion || (is_vpred_or_anima && !is_cfgpp_sampler));

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
                "latent_image": [latent_source.0, latent_source.1],
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

    let (decode_id, next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        model_source: sampler_model_source,
        clip_source,
        positive_source: pos_source,
        negative_source: neg_source,
        vae_source,
        sampler_id,
        refiner_model_source: None,
        base_sources: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::templates::graph_test_util::{build, linked, nodes, params, single};

    #[test]
    fn sdxl_outpaint_wires_pad_vae_encode_and_noise_mask() {
        let mut p = params("outpainting", "sdxl");
        p.input_image = Some("scene.png".into());
        p.outpaint_left = 128;
        p.outpaint_right = 256;
        p.outpaint_top = 64;
        p.outpaint_bottom = 0;
        p.outpaint_feather = 48;

        let workflow = build(&p);
        let pad = single(&workflow, "ImagePadForOutpaint");
        assert_eq!(pad["inputs"]["left"], 128);
        assert_eq!(pad["inputs"]["right"], 256);
        assert_eq!(pad["inputs"]["top"], 64);
        assert_eq!(pad["inputs"]["bottom"], 0);
        assert_eq!(pad["inputs"]["feathering"], 48);

        let sampler = single(&workflow, "KSampler");
        let mask_node = linked(&workflow, &sampler["inputs"]["latent_image"]);
        assert_eq!(mask_node["class_type"], "SetLatentNoiseMask");
        let encode = linked(&workflow, &mask_node["inputs"]["samples"]);
        assert_eq!(encode["class_type"], "VAEEncode");
        assert_eq!(
            linked(&workflow, &encode["inputs"]["pixels"])["class_type"],
            "ImagePadForOutpaint"
        );
        assert_eq!(
            linked(&workflow, &mask_node["inputs"]["mask"])["class_type"],
            "ImagePadForOutpaint"
        );
    }

    #[test]
    fn flux_outpaint_uses_inpaint_model_conditioning_and_flux_guidance() {
        let mut p = params("outpainting", "flux1d");
        p.input_image = Some("scene.png".into());
        p.outpaint_left = 256;
        p.flux_guidance = 3.5;

        let workflow = build(&p);
        let inpaint_cond = single(&workflow, "InpaintModelConditioning");
        assert_eq!(inpaint_cond["inputs"]["noise_mask"], json!(true));
        assert_eq!(
            linked(&workflow, &inpaint_cond["inputs"]["pixels"])["class_type"],
            "ImagePadForOutpaint"
        );
        assert_eq!(
            linked(&workflow, &inpaint_cond["inputs"]["mask"])["class_type"],
            "ImagePadForOutpaint"
        );

        let sampler = single(&workflow, "KSampler");
        let guided = linked(&workflow, &sampler["inputs"]["positive"]);
        assert_eq!(guided["class_type"], "FluxGuidance");
        assert_eq!(
            linked(&workflow, &guided["inputs"]["conditioning"])["class_type"],
            "InpaintModelConditioning"
        );
        assert_eq!(
            linked(&workflow, &sampler["inputs"]["negative"])["class_type"],
            "InpaintModelConditioning"
        );
        assert_eq!(
            linked(&workflow, &sampler["inputs"]["latent_image"])["class_type"],
            "InpaintModelConditioning"
        );
    }

    #[test]
    fn inpaint_checkpoint_name_triggers_inpaint_model_conditioning() {
        let mut p = params("outpainting", "sdxl");
        p.checkpoint = "sdxl_inpaint_v1.safetensors".into();
        p.input_image = Some("scene.png".into());
        p.outpaint_right = 128;

        let workflow = build(&p);
        assert_eq!(nodes(&workflow, "InpaintModelConditioning").len(), 1);
        assert!(nodes(&workflow, "SetLatentNoiseMask").is_empty());
    }

    #[test]
    fn batch_size_repeats_the_outpaint_latent() {
        let mut p = params("outpainting", "sdxl");
        p.input_image = Some("scene.png".into());
        p.outpaint_bottom = 128;
        p.batch_size = 3;

        let workflow = build(&p);
        let sampler = single(&workflow, "KSampler");
        let repeat = linked(&workflow, &sampler["inputs"]["latent_image"]);
        assert_eq!(repeat["class_type"], "RepeatLatentBatch");
        assert_eq!(repeat["inputs"]["amount"], 3);
    }

    #[test]
    fn differential_diffusion_wires_for_sdxl_outpaint_when_enabled() {
        let mut p = params("outpainting", "sdxl");
        p.input_image = Some("scene.png".into());
        p.outpaint_top = 128;
        p.differential_diffusion = true;

        let workflow = build(&p);
        let sampler = single(&workflow, "KSampler");
        assert_eq!(
            linked(&workflow, &sampler["inputs"]["model"])["class_type"],
            "DifferentialDiffusion"
        );
    }
}
