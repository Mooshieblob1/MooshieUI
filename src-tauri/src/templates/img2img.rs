use serde_json::json;

use super::{build_scheduled_conditioning, insert_vae_decode, load_model_nodes, WorkflowResult};
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

    // Refine-only mode: skip the main img2img round-trip and feed the loaded
    // image directly into whatever runs next (typically the upscale chain).
    // This implements SwarmUI's "Refine Image" semantics — a single low-denoise
    // second pass at higher resolution rather than two sequential samplings.
    if params.refine_only {
        return WorkflowResult {
            workflow,
            next_id,
            image_output: (load_img_id, 0),
            model_source,
            clip_source,
            positive_source: pos_source,
            negative_source: neg_source,
            vae_source,
            // Empty sampler_id — no main sampler exists. inject_*/controlnet
            // rewiring is keyed on `workflow.get_mut(&sampler_id)` and a
            // missing key is a safe no-op.
            sampler_id: String::new(),
        };
    }

    // Resize input image to target dimensions (avoids VRAM issues with large images)
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

    // VAE Encode the resized image
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

    // KSampler with denoise < 1.0
    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
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
    }
}
