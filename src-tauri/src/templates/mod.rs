pub mod controlnet;
pub mod img2img;
pub mod inpainting;
pub mod txt2img;
pub mod upscale;

use serde_json::{json, Value};

use crate::comfyui::types::GenerationParams;

pub struct WorkflowResult {
    pub workflow: serde_json::Map<String, Value>,
    pub next_id: u32,
    pub image_output: (String, u32),
    pub model_source: (String, u32),
    pub positive_source: (String, u32),
    pub negative_source: (String, u32),
    pub vae_source: (String, u32),
}

/// Outputs from the model loading stage (checkpoint or split model).
pub struct ModelLoadResult {
    pub model_source: (String, u32),
    pub clip_source: (String, u32),
    pub vae_source: (String, u32),
    pub next_id: u32,
}

/// Load model nodes — either a single CheckpointLoaderSimple or split UNETLoader + CLIPLoader + VAELoader.
/// Also handles the LoRA chain and optional separate VAE override.
pub fn load_model_nodes(
    workflow: &mut serde_json::Map<String, Value>,
    mut next_id: u32,
    params: &GenerationParams,
) -> ModelLoadResult {
    let (mut model_source, mut clip_source, mut vae_source);

    if params.use_split_model {
        // UNETLoader for diffusion model
        let unet_id = next_id.to_string();
        workflow.insert(
            unet_id.clone(),
            json!({
                "class_type": "UNETLoader",
                "inputs": {
                    "unet_name": params.diffusion_model.as_deref().unwrap_or(""),
                    "weight_dtype": "default"
                }
            }),
        );
        model_source = (unet_id, 0);
        next_id += 1;

        // CLIPLoader for text encoder
        let clip_id = next_id.to_string();
        let clip_type = params.clip_type.as_deref().unwrap_or("wan");
        workflow.insert(
            clip_id.clone(),
            json!({
                "class_type": "CLIPLoader",
                "inputs": {
                    "clip_name": params.clip_model.as_deref().unwrap_or(""),
                    "type": clip_type
                }
            }),
        );
        clip_source = (clip_id, 0);
        next_id += 1;

        // VAELoader — always needed for split models (use params.vae or a default)
        let vae_id = next_id.to_string();
        let vae_name = params.vae.as_deref().unwrap_or("");
        workflow.insert(
            vae_id.clone(),
            json!({
                "class_type": "VAELoader",
                "inputs": {
                    "vae_name": vae_name
                }
            }),
        );
        vae_source = (vae_id, 0);
        next_id += 1;
    } else {
        // Standard CheckpointLoaderSimple
        let checkpoint_id = next_id.to_string();
        workflow.insert(
            checkpoint_id.clone(),
            json!({
                "class_type": "CheckpointLoaderSimple",
                "inputs": {
                    "ckpt_name": params.checkpoint
                }
            }),
        );
        model_source = (checkpoint_id.clone(), 0);
        clip_source = (checkpoint_id.clone(), 1);
        vae_source = (checkpoint_id.clone(), 2);
        next_id += 1;
    }

    // LoRA chain
    for lora in &params.loras {
        let lora_id = next_id.to_string();
        workflow.insert(
            lora_id.clone(),
            json!({
                "class_type": "LoraLoader",
                "inputs": {
                    "model": [model_source.0, model_source.1],
                    "clip": [clip_source.0, clip_source.1],
                    "lora_name": lora.name,
                    "strength_model": lora.strength_model,
                    "strength_clip": lora.strength_clip
                }
            }),
        );
        model_source = (lora_id.clone(), 0);
        clip_source = (lora_id, 1);
        next_id += 1;
    }

    // Optional separate VAE override (only for non-split models, split already has its own VAE)
    if !params.use_split_model {
        if let Some(ref vae_name) = params.vae {
            if !vae_name.is_empty() {
                let vae_id = next_id.to_string();
                workflow.insert(
                    vae_id.clone(),
                    json!({
                        "class_type": "VAELoader",
                        "inputs": {
                            "vae_name": vae_name
                        }
                    }),
                );
                vae_source = (vae_id, 0);
                next_id += 1;
            }
        }
    }

    ModelLoadResult {
        model_source,
        clip_source,
        vae_source,
        next_id,
    }
}

pub fn build_workflow(params: &GenerationParams, seed: i64) -> Value {
    let mut result = match params.mode.as_str() {
        "img2img" => img2img::build(params, seed),
        "inpainting" => inpainting::build(params, seed),
        _ => txt2img::build(params, seed),
    };

    // Inject ControlNet if enabled
    if let Some(ref cn) = params.controlnet {
        if cn.enabled && cn.controlnet_model.is_some() && cn.image.is_some() {
            controlnet::inject_controlnet(&mut result, cn);
        }
    }

    let final_image = if params.upscale_enabled {
        upscale::append_upscale_chain(&mut result, params, seed)
    } else {
        result.image_output.clone()
    };

    let save_id = result.next_id.to_string();
    result.workflow.insert(
        save_id,
        json!({
            "class_type": "SaveImage",
            "inputs": {
                "images": [final_image.0, final_image.1],
                "filename_prefix": "ComfyUI"
            }
        }),
    );

    Value::Object(result.workflow)
}
