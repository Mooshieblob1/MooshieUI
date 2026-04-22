pub mod controlnet;
pub mod facefix;
pub mod img2img;
pub mod inpainting;
pub mod txt2img;
pub mod upscale;

use serde_json::{json, Value};

use crate::comfyui::types::{GenerationParams, PromptSegment};

pub struct WorkflowResult {
    pub workflow: serde_json::Map<String, Value>,
    pub next_id: u32,
    pub image_output: (String, u32),
    pub model_source: (String, u32),
    pub clip_source: (String, u32),
    pub positive_source: (String, u32),
    pub negative_source: (String, u32),
    pub vae_source: (String, u32),
    /// The KSampler node ID — needed to rewire positive/negative after ControlNet injection.
    pub sampler_id: String,
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

    if is_nanosaur_architecture(params) {
        // NanoSaurLoader — custom all-in-one loader for Nanosaur models.
        // Outputs: MODEL(0), CLIP(1), VAE(2). Includes its own sampler patch.
        let loader_id = next_id.to_string();
        workflow.insert(
            loader_id.clone(),
            json!({
                "class_type": "NanoSaurLoader",
                "inputs": {
                    "unet_name": params.diffusion_model.as_deref().unwrap_or("nanosaur_diffusion_model.safetensors"),
                    "text_encoder_name": params.clip_model.as_deref().unwrap_or("nanosaur_text_encoder.safetensors"),
                    "vae_name": params.vae.as_deref().unwrap_or("nanosaur_vae_decoder.safetensors"),
                    "uncond_crossover_percent": 1.0,
                    "weight_dtype": "default",
                    "clip_device": "default"
                }
            }),
        );
        model_source = (loader_id.clone(), 0);
        clip_source = (loader_id.clone(), 1);
        vae_source = (loader_id, 2);
        next_id += 1;

        return ModelLoadResult {
            model_source,
            clip_source,
            vae_source,
            next_id,
        };
    } else if params.use_split_model {
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

    // Apply rectified flow scheduling for SD3/Flux/AuraFlow (patches model before sampling)
    inject_rectified_flow(&mut result, params);

    // Apply Stable Cascade model sampling if applicable
    inject_cascade_sampling(&mut result, params);

    // Apply FluxGuidance for Flux Dev (positive conditioning guidance)
    inject_flux_guidance(&mut result, params);

    // Apply Smart Guidance (positive-biased adaptive guidance) — patches model so all
    // downstream KSamplers (main, upscale, facefix) inherit it.
    inject_smart_guidance(&mut result, params);

    // Inject ControlNet if enabled
    if let Some(ref cn) = params.controlnet {
        if cn.enabled && cn.controlnet_model.is_some() && cn.image.is_some() {
            controlnet::inject_controlnet(&mut result, cn);

            // Rewire the primary KSampler to use ControlNet-conditioned positive/negative
            if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
                if let Some(inputs) = sampler_node.get_mut("inputs") {
                    inputs["positive"] =
                        json!([result.positive_source.0, result.positive_source.1]);
                    inputs["negative"] =
                        json!([result.negative_source.0, result.negative_source.1]);
                }
            }
        }
    }

    let final_image = if params.upscale_enabled {
        upscale::append_upscale_chain(&mut result, params, seed)
    } else {
        result.image_output.clone()
    };

    // Apply face fix (FaceDetailer) after upscale if enabled
    let final_image = if params.facefix_enabled {
        facefix::append_facefix_chain(&mut result, params, final_image, seed)
    } else {
        final_image
    };

    let save_id = result.next_id.to_string();
    let output_format = match params.output_format.as_str() {
        "jxl" => "jxl_raw",
        _ => "png",
    };
    result.workflow.insert(
        save_id,
        json!({
            "class_type": "MooshieSaveImage",
            "inputs": {
                "images": [final_image.0, final_image.1],
                "bit_depth": params.output_bit_depth,
                "output_format": output_format
            }
        }),
    );

    Value::Object(result.workflow)
}

/// Returns true when the model is an SD3/SD3.5 architecture.
pub fn is_sd3_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "sd3" {
        return true;
    }
    let name = model_name_lower(params);
    if name.contains("sd3")
        || name.contains("stable-diffusion-3")
        || name.contains("stable_diffusion_3")
    {
        return true;
    }
    if let Some(ref clip_type) = params.clip_type {
        if clip_type == "sd3" {
            return true;
        }
    }
    false
}

/// Returns true when the model is a Flux architecture.
pub fn is_flux_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "flux" {
        return true;
    }
    model_name_lower(params).contains("flux")
}

/// Returns true when the model is a Pony Diffusion architecture (SDXL-based).
pub fn is_pony_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "pony" {
        return true;
    }
    model_name_lower(params).contains("pony")
}

/// Returns true when the model is AuraFlow architecture.
pub fn is_auraflow_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "auraflow" {
        return true;
    }
    model_name_lower(params).contains("auraflow")
}

/// Returns true when the model is a PixArt architecture.
pub fn is_pixart_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "pixart" {
        return true;
    }
    model_name_lower(params).contains("pixart")
}

/// Returns true when the model is HunyuanDiT architecture.
pub fn is_hunyuandit_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "hunyuandit" {
        return true;
    }
    model_name_lower(params).contains("hunyuan")
}

/// Returns true when the model is Stable Cascade architecture.
pub fn is_cascade_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "cascade" {
        return true;
    }
    model_name_lower(params).contains("cascade")
}

/// Returns true when the model is Kolors architecture.
pub fn is_kolors_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "kolors" {
        return true;
    }
    model_name_lower(params).contains("kolors")
}

/// Returns true when the model is Mugen (SDXL with Flux2 VAE + rectified flow scheduling).
/// Must be checked before is_illustrious since Mugen derives from NoobAI.
pub fn is_mugen_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "mugen" {
        return true;
    }
    model_name_lower(params).contains("mugen")
}

/// Returns true when the model is Nanosaur (custom 1.2B DiT with DINOv3 VAE).
/// Uses the `NanoSaurLoader` custom node instead of standard loaders.
pub fn is_nanosaur_architecture(params: &GenerationParams) -> bool {
    if params.model_architecture == "nanosaur" {
        return true;
    }
    model_name_lower(params).contains("nanosaur")
}

/// Returns true when the model needs a 16-channel latent (SD3, Flux, Anima/WAN).
pub fn needs_sd3_latent(params: &GenerationParams) -> bool {
    if is_sd3_architecture(params) || is_flux_architecture(params) {
        return true;
    }
    let name = model_name_lower(params);
    name.contains("anima") || name.contains("wan")
}

/// Lowercase model name for heuristic checks.
fn model_name_lower(params: &GenerationParams) -> String {
    params
        .diffusion_model
        .as_deref()
        .unwrap_or(&params.checkpoint)
        .to_lowercase()
}

/// Insert a VAE decode node into the workflow.
/// Uses `VAEDecodeTiled` for Mugen (Flux2VAE SDXL requires tiled decode to handle the larger
/// latent space correctly), and standard `VAEDecode` for all other architectures.
/// Returns `(decode_node_id, next_id)`.
pub fn insert_vae_decode(
    workflow: &mut serde_json::Map<String, Value>,
    next_id: u32,
    sampler_id: &str,
    vae_source: &(String, u32),
    params: &GenerationParams,
) -> (String, u32) {
    let decode_id = next_id.to_string();
    if is_mugen_architecture(params) {
        workflow.insert(
            decode_id.clone(),
            json!({
                "class_type": "VAEDecodeTiled",
                "inputs": {
                    "samples": [sampler_id, 0],
                    "vae": [vae_source.0, vae_source.1],
                    "tile_size": 512,
                    "overlap": 64,
                    "temporal_size": 64,
                    "temporal_overlap": 8
                }
            }),
        );
    } else {
        workflow.insert(
            decode_id.clone(),
            json!({
                "class_type": "VAEDecode",
                "inputs": {
                    "samples": [sampler_id, 0],
                    "vae": [vae_source.0, vae_source.1]
                }
            }),
        );
    }
    (decode_id, next_id + 1)
}

/// Build a conditioning output that combines a base prompt with optional timestep-scheduled segments.
///
/// When `segments` is empty, this creates a single `CLIPTextEncode` and returns its output —
/// identical to the previous behavior with zero overhead.
///
/// When segments are present, each segment gets its own `CLIPTextEncode` → `ConditioningSetTimestepRange`,
/// then all are chained together with `ConditioningCombine`.
///
/// Returns `(conditioning_source, next_id)`.
pub fn build_scheduled_conditioning(
    workflow: &mut serde_json::Map<String, Value>,
    mut next_id: u32,
    clip_source: &(String, u32),
    base_prompt: &str,
    segments: &[PromptSegment],
) -> ((String, u32), u32) {
    // Base prompt — always encoded (may be empty if user put everything in segments)
    let base_id = next_id.to_string();
    workflow.insert(
        base_id.clone(),
        json!({
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": [clip_source.0, clip_source.1],
                "text": base_prompt
            }
        }),
    );
    next_id += 1;

    if segments.is_empty() {
        return ((base_id, 0), next_id);
    }

    // Start the combine chain with the base conditioning
    let mut combined_source = (base_id, 0u32);

    for segment in segments {
        // Encode segment text
        let seg_clip_id = next_id.to_string();
        workflow.insert(
            seg_clip_id.clone(),
            json!({
                "class_type": "CLIPTextEncode",
                "inputs": {
                    "clip": [clip_source.0, clip_source.1],
                    "text": segment.text
                }
            }),
        );
        next_id += 1;

        // Set timestep range on the segment conditioning
        let range_id = next_id.to_string();
        workflow.insert(
            range_id.clone(),
            json!({
                "class_type": "ConditioningSetTimestepRange",
                "inputs": {
                    "conditioning": [seg_clip_id, 0],
                    "start": segment.start,
                    "end": segment.end
                }
            }),
        );
        next_id += 1;

        // Combine with running chain
        let combine_id = next_id.to_string();
        workflow.insert(
            combine_id.clone(),
            json!({
                "class_type": "ConditioningCombine",
                "inputs": {
                    "conditioning_1": [combined_source.0, combined_source.1],
                    "conditioning_2": [range_id, 0]
                }
            }),
        );
        combined_source = (combine_id, 0);
        next_id += 1;
    }

    (combined_source, next_id)
}

/// Inject rectified flow scheduling for models that use it (SD3, Flux, AuraFlow, Mugen).
/// Patches the model with `ModelSamplingSD3`, `ModelSamplingFlux`, `ModelSamplingAuraFlow`,
/// or for Mugen: `ModelSamplingSD3` with higher shift (8-12 range, default 10).
/// Rewires the KSampler in all cases.
fn inject_rectified_flow(result: &mut WorkflowResult, params: &GenerationParams) {
    // Nanosaur handles flow matching internally via NanoSaurLoader — skip injection
    if is_nanosaur_architecture(params) {
        return;
    }

    if is_mugen_architecture(params) {
        // ModelSamplingSD3 with elevated shift for Flux2VAE SDXL (recommended range: 8-12)
        let node_id = result.next_id.to_string();
        result.workflow.insert(
            node_id.clone(),
            json!({
                "class_type": "ModelSamplingSD3",
                "inputs": {
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "shift": 10.0
                }
            }),
        );
        result.model_source = (node_id, 0);
        result.next_id += 1;

        // Rewire KSampler to use patched model
        if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
            if let Some(inputs) = sampler_node.get_mut("inputs") {
                inputs["model"] = json!([result.model_source.0, result.model_source.1]);
            }
        }
    } else if is_sd3_architecture(params) {
        // ModelSamplingSD3 — discrete flow matching with constant shift
        let node_id = result.next_id.to_string();
        result.workflow.insert(
            node_id.clone(),
            json!({
                "class_type": "ModelSamplingSD3",
                "inputs": {
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "shift": 3.0
                }
            }),
        );
        result.model_source = (node_id, 0);
        result.next_id += 1;

        // Rewire KSampler to use patched model
        if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
            if let Some(inputs) = sampler_node.get_mut("inputs") {
                inputs["model"] = json!([result.model_source.0, result.model_source.1]);
            }
        }
    } else if is_flux_architecture(params) {
        // ModelSamplingFlux — resolution-dependent shift for Flux family
        let node_id = result.next_id.to_string();
        result.workflow.insert(
            node_id.clone(),
            json!({
                "class_type": "ModelSamplingFlux",
                "inputs": {
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "max_shift": 1.15,
                    "base_shift": 0.5,
                    "width": params.width,
                    "height": params.height
                }
            }),
        );
        result.model_source = (node_id, 0);
        result.next_id += 1;

        // Rewire KSampler to use patched model
        if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
            if let Some(inputs) = sampler_node.get_mut("inputs") {
                inputs["model"] = json!([result.model_source.0, result.model_source.1]);
            }
        }
    } else if is_auraflow_architecture(params) {
        // ModelSamplingAuraFlow — discrete flow matching with shift 1.73, multiplier 1.0
        let node_id = result.next_id.to_string();
        result.workflow.insert(
            node_id.clone(),
            json!({
                "class_type": "ModelSamplingAuraFlow",
                "inputs": {
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "shift": 1.73
                }
            }),
        );
        result.model_source = (node_id, 0);
        result.next_id += 1;

        // Rewire KSampler to use patched model
        if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
            if let Some(inputs) = sampler_node.get_mut("inputs") {
                inputs["model"] = json!([result.model_source.0, result.model_source.1]);
            }
        }
    }
}

/// Inject Stable Cascade model sampling (shift 2.0) for Cascade architecture models.
fn inject_cascade_sampling(result: &mut WorkflowResult, params: &GenerationParams) {
    if !is_cascade_architecture(params) {
        return;
    }

    let node_id = result.next_id.to_string();
    result.workflow.insert(
        node_id.clone(),
        json!({
            "class_type": "ModelSamplingStableCascade",
            "inputs": {
                "model": [result.model_source.0.clone(), result.model_source.1],
                "shift": 2.0
            }
        }),
    );
    result.model_source = (node_id, 0);
    result.next_id += 1;

    // Rewire KSampler to use patched model
    if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
        if let Some(inputs) = sampler_node.get_mut("inputs") {
            inputs["model"] = json!([result.model_source.0, result.model_source.1]);
        }
    }
}

/// Inject FluxGuidance for Flux Dev models (not Schnell which is guidance-distilled).
/// Patches the positive conditioning with guidance=3.5 and rewires the KSampler.
fn inject_smart_guidance(result: &mut WorkflowResult, params: &GenerationParams) {
    if !params.smart_guidance {
        return;
    }

    let node_id = result.next_id.to_string();
    result.workflow.insert(
        node_id.clone(),
        json!({
            "class_type": "MooshieSmartGuidance",
            "inputs": {
                "model": [result.model_source.0.clone(), result.model_source.1]
            }
        }),
    );
    result.model_source = (node_id, 0);
    result.next_id += 1;

    // Rewire KSampler to use the Smart Guidance-patched model
    if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
        if let Some(inputs) = sampler_node.get_mut("inputs") {
            inputs["model"] = json!([result.model_source.0, result.model_source.1]);
        }
    }
}

fn inject_flux_guidance(result: &mut WorkflowResult, params: &GenerationParams) {
    if !is_flux_architecture(params) {
        return;
    }

    // Skip for Schnell (already guidance-distilled, no guidance needed)
    let name = model_name_lower(params);
    if name.contains("schnell") {
        return;
    }

    let node_id = result.next_id.to_string();
    result.workflow.insert(
        node_id.clone(),
        json!({
            "class_type": "FluxGuidance",
            "inputs": {
                "conditioning": [result.positive_source.0.clone(), result.positive_source.1],
                "guidance": 3.5
            }
        }),
    );
    result.positive_source = (node_id, 0);
    result.next_id += 1;

    // Rewire KSampler to use guided positive conditioning
    if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
        if let Some(inputs) = sampler_node.get_mut("inputs") {
            inputs["positive"] = json!([result.positive_source.0, result.positive_source.1]);
        }
    }
}
