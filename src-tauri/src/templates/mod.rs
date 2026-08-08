pub mod controlnet;
pub mod facefix;
pub mod image_edit;
pub mod img2img;
pub mod inpainting;
pub mod segment_detail;
pub mod style_transfer;
pub mod txt2img;
pub mod upscale;
pub mod video;

use serde_json::{json, Value};

use crate::comfyui::types::{GenerationParams, PromptSegment};

/// Validate generation parameters before workflow construction.
///
/// Catches missing input images for modes that require them, and ControlNet
/// configurations with no reference image. Without these guards the request
/// reaches ComfyUI's `LoadImage` node with an empty filename, which it
/// resolves to the input directory and crashes with `[Errno 21] Is a directory`.
///
/// Both the Tauri `generate` command and the LAN web server `generate` route
/// must call this before `build_workflow`.
pub fn validate_generation_params(params: &GenerationParams) -> Result<(), String> {
    // Video mode has its own parameter set; validate it and return early so
    // image-only guards (input images, ControlNet, style transfer) never
    // fire on stale image-mode state.
    if params.mode == "video" {
        if !matches!(params.video_variant.as_str(), "fl2va" | "ref2va") {
            return Err(format!(
                "Unknown video variant \"{}\" — expected \"fl2va\" or \"ref2va\".",
                params.video_variant
            ));
        }
        for (label, file) in [
            ("a diffusion model", params.video_diffusion_model.as_deref()),
            ("a text encoder", params.video_clip_model.as_deref()),
            ("a video VAE", params.video_vae_model.as_deref()),
            ("an audio VAE", params.video_audio_vae_model.as_deref()),
        ] {
            if file.map(str::trim).unwrap_or("").is_empty() {
                return Err(format!(
                    "Video generation requires {} — open the model panel to download the MiniMax H3 files.",
                    label
                ));
            }
        }
        let diffusion = params
            .video_diffusion_model
            .as_deref()
            .map(str::trim)
            .unwrap_or("");
        let diffusion_lower = diffusion.to_lowercase();
        if !video::H3_DIFFUSION_MARKERS
            .iter()
            .any(|marker| diffusion_lower.contains(marker))
        {
            return Err(format!(
                "\"{}\" does not look like a MiniMax H3 model — only MiniMax H3 is supported for video in this version. If this file really is one, rename it to include \"minimax\" or \"h3\".",
                diffusion
            ));
        }
        let other_variant = if params.video_variant == "fl2va" {
            "ref2va"
        } else {
            "fl2va"
        };
        if diffusion_lower.contains(other_variant) {
            return Err(format!(
                "\"{}\" is a {} model, but the {} variant is selected — pick the matching diffusion model or switch variants.",
                diffusion, other_variant, params.video_variant
            ));
        }
        if !(1.0..=15.0).contains(&params.video_duration_seconds) {
            return Err(format!(
                "Video duration must be between 1 and 15 seconds (got {}).",
                params.video_duration_seconds
            ));
        }
        if params.video_variant == "ref2va" {
            // The Director builds its own reference set from the timeline (shot
            // stills and cast photos), so the settings panel's slots are only
            // mandatory when the timeline is not driving the graph.
            let timeline_drives = params
                .video_timeline_data
                .as_deref()
                .is_some_and(|data| !data.trim().is_empty());
            let ref_count = params
                .video_ref_images
                .iter()
                .filter(|s| !s.trim().is_empty())
                .count();
            if ref_count == 0 && !timeline_drives {
                return Err(
                    "Reference-to-video needs at least one reference image — please upload one before generating.".into(),
                );
            }
            if ref_count > 9 {
                return Err("Reference-to-video supports at most 9 reference images.".into());
            }
        }
        return Ok(());
    }

    let needs_input_image =
        matches!(params.mode.as_str(), "img2img" | "inpainting") || params.refine_only;

    if needs_input_image
        && params
            .input_image
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
    {
        return Err(format!(
            "{} mode requires an input image — please upload one before generating.",
            if params.refine_only {
                "refine"
            } else {
                params.mode.as_str()
            }
        ));
    }

    if matches!(params.mode.as_str(), "inpainting")
        && params
            .mask_image
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
    {
        return Err(
            "Inpainting mode requires a mask image — please paint a mask before generating.".into(),
        );
    }

    if matches!(params.mode.as_str(), "image_edit")
        && params
            .edit_reference_images
            .first()
            .map(|s| s.trim())
            .unwrap_or("")
            .is_empty()
    {
        return Err(
            "Image Edit mode requires a reference image — please upload one before generating."
                .into(),
        );
    }

    if let Some(cn) = params.controlnet.as_ref() {
        if cn.enabled && cn.image.as_deref().map(str::trim).unwrap_or("").is_empty() {
            return Err(
                "ControlNet is enabled but no reference image was provided — please upload one or disable ControlNet.".into(),
            );
        }
        if cn.enabled
            && cn.preset.as_deref().is_some_and(|p| p == "inpainting")
            && params
                .mask_image
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            return Err(
                "The Anima inpainting ControlNet preset requires a mask — please paint a mask before generating.".into(),
            );
        }
    }

    if params.style_transfer_enabled {
        if params.model_architecture != "anima" {
            return Err(
                "Style transfer (Untwisting RoPE) is only supported for Anima models.".into(),
            );
        }
        if params.mode != "txt2img" {
            return Err(
                "Style transfer is only available in txt2img mode — switch mode or disable style transfer.".into(),
            );
        }
        if params
            .style_reference_image
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            return Err(
                "Style transfer is enabled but no style reference image was provided — please upload one.".into(),
            );
        }
        if params.controlnet.as_ref().is_some_and(|cn| cn.enabled) {
            return Err(
                "Style transfer cannot be used with ControlNet enabled — disable one of them."
                    .into(),
            );
        }
        if params.upscale_enabled {
            return Err(
                "Style transfer cannot be used with upscale enabled in this version — disable upscale.".into(),
            );
        }
        if params.facefix_enabled {
            return Err(
                "Style transfer cannot be used with face fix enabled in this version — disable face fix.".into(),
            );
        }
        if !params.detail_segments.is_empty() {
            return Err(
                "Style transfer cannot be used with <segment> refinement in this version — remove segment tags from the prompt.".into(),
            );
        }
    }

    // Krea 2 hard-requires the Qwen3-VL 4B text encoder (12x2560=30720-dim
    // conditioning). With any other encoder, ComfyUI fails deep inside sampling
    // with a cryptic feature-count error, so fail fast here instead.
    if params.model_architecture == "krea2" && params.use_split_model {
        if params.clip_type.as_deref() != Some("krea2") {
            return Err(
                "Krea 2 requires the CLIP loader type \"krea2\" (ComfyUI 0.26.0 or newer). Re-select the Krea 2 model so the text encoder settings refresh, or update ComfyUI.".into(),
            );
        }
        let clip_model = params.clip_model.as_deref().map(str::trim).unwrap_or("");
        if clip_model.is_empty() {
            return Err(
                "Krea 2 requires the Qwen3-VL 4B text encoder, but none is selected. Open the model panel to download it (qwen3vl_4b_fp8_scaled.safetensors), or pick it under Text Encoder.".into(),
            );
        }
        let clip_lower = clip_model.to_lowercase();
        if !crate::commands::api::KREA2_TEXT_ENCODER_MARKERS
            .iter()
            .any(|marker| clip_lower.contains(marker))
        {
            return Err(format!(
                "Krea 2 requires the Qwen3-VL 4B text encoder, but \"{}\" is selected — other encoders produce a conditioning-size error in ComfyUI. Download qwen3vl_4b_fp8_scaled.safetensors from the model panel, or if this file really is a Qwen3-VL 4B encoder, rename it to include \"qwen3vl_4b\".",
                clip_model
            ));
        }
    }

    Ok(())
}

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

/// Absolute path to use when the active model is stored in a folder that doesn't
/// match what it actually is (a split-file model in `checkpoints/`, or a full
/// checkpoint in `diffusion_models/`).
///
/// `model_source_category` is set by the frontend when detection reclassified the
/// model; `resolved_model_path` is filled in by the `generate` command. Both are
/// required — without the resolved path there is nothing for the path loaders to
/// open, so the caller falls back to the stock loaders.
fn misplaced_model_path(params: &GenerationParams) -> Option<&str> {
    params.model_source_category.as_deref()?;
    params
        .resolved_model_path
        .as_deref()
        .filter(|p| !p.is_empty())
}

/// Load model nodes — either a single CheckpointLoaderSimple or split UNETLoader + CLIPLoader + VAELoader.
/// Also handles the LoRA chain and optional separate VAE override.
pub fn load_model_nodes(
    workflow: &mut serde_json::Map<String, Value>,
    mut next_id: u32,
    params: &GenerationParams,
) -> ModelLoadResult {
    let (mut model_source, mut clip_source, mut vae_source);

    if params.model_architecture == "nanosaur" {
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
        // UNETLoader for diffusion model. Pass both unet_name and model_name for compatibility
        // across standard ComfyUI and custom nodes (e.g. ComfyUI-Flow-Control).
        // GGUF quantized models cannot be loaded by core UNETLoader — route them
        // through the ComfyUI-GGUF custom node instead (installed at ComfyUI setup).
        let unet_name = params.diffusion_model.as_deref().unwrap_or("");
        let unet_id = next_id.to_string();
        let unet_node = if unet_name.to_ascii_lowercase().ends_with(".gguf") {
            json!({
                "class_type": "UnetLoaderGGUF",
                "inputs": {
                    "unet_name": unet_name
                }
            })
        } else if let Some(path) = misplaced_model_path(params) {
            // Split-file model physically stored in another folder (usually
            // checkpoints/). UNETLoader validates unet_name against its own folder
            // listing and would reject it, so load by absolute path instead.
            json!({
                "class_type": "MooshieDiffusionLoaderPath",
                "inputs": {
                    "unet_path": path,
                    "weight_dtype": "default"
                }
            })
        } else {
            json!({
                "class_type": "UNETLoader",
                "inputs": {
                    "unet_name": unet_name,
                    "model_name": unet_name,
                    "weight_dtype": "default"
                }
            })
        };
        workflow.insert(unet_id.clone(), unet_node);
        model_source = (unet_id, 0);
        next_id += 1;

        // CLIPLoader for text encoder (GGUF-quantized encoders need CLIPLoaderGGUF)
        let clip_id = next_id.to_string();
        let clip_type = params.clip_type.as_deref().unwrap_or("wan");
        let clip_name = params.clip_model.as_deref().unwrap_or("");
        let clip_class = if clip_name.to_ascii_lowercase().ends_with(".gguf") {
            "CLIPLoaderGGUF"
        } else {
            "CLIPLoader"
        };
        workflow.insert(
            clip_id.clone(),
            json!({
                "class_type": clip_class,
                "inputs": {
                    "clip_name": clip_name,
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
        // Standard CheckpointLoaderSimple, or the path-based Mooshie loader when the
        // checkpoint physically lives outside models/checkpoints/.
        let checkpoint_id = next_id.to_string();
        let checkpoint_node = if let Some(path) = misplaced_model_path(params) {
            json!({
                "class_type": "MooshieCheckpointLoaderPath",
                "inputs": {
                    "ckpt_path": path
                }
            })
        } else {
            json!({
                "class_type": "CheckpointLoaderSimple",
                "inputs": {
                    "ckpt_name": params.checkpoint
                }
            })
        };
        workflow.insert(checkpoint_id.clone(), checkpoint_node);
        model_source = (checkpoint_id.clone(), 0);
        clip_source = (checkpoint_id.clone(), 1);
        vae_source = (checkpoint_id.clone(), 2);
        next_id += 1;
    }

    // LoRA chain
    for lora in &params.loras {
        if lora.name.trim().is_empty() {
            log::warn!(
                "Skipping LoRA with empty name — this should have been filtered by the frontend"
            );
            continue;
        }
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

/// `video_metadata_supported` is the result of probing the ComfyUI server for a
/// `MooshieSaveVideo` new enough to declare `metadata_json`. It is ignored for
/// image modes, which embed metadata in Rust after the fact.
pub fn build_workflow(
    params: &GenerationParams,
    seed: i64,
    video_metadata_supported: bool,
) -> Value {
    if params.mode == "video" {
        // Video builds its own complete graph and must bypass finish_workflow
        // (upscale/facefix/segment chains and MooshieSaveImage are image-only).
        return video::build(params, seed, video_metadata_supported);
    }

    if params.style_transfer_enabled && params.model_architecture == "anima" {
        let result = style_transfer::build(params, seed);
        return finish_workflow(result, params, seed);
    }

    let mut result = match params.mode.as_str() {
        "img2img" => img2img::build(params, seed),
        "inpainting" => inpainting::build(params, seed),
        "image_edit" => image_edit::build(params, seed),
        _ => txt2img::build(params, seed),
    };

    // Apply rectified flow scheduling for SD3/Flux/AuraFlow (patches model before sampling)
    inject_rectified_flow(&mut result, params);

    // v-prediction + zero-terminal SNR for NoobAI / Illustrious SDXL variants
    inject_vpred_zsnr_sampling(&mut result, params);

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
            if params.model_architecture == "anima" {
                let mask = params.mask_image.as_deref();
                controlnet::inject_anima_lllite(&mut result, cn, mask);
            } else {
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
    }

    finish_workflow(result, params, seed)
}

fn finish_workflow(mut result: WorkflowResult, params: &GenerationParams, seed: i64) -> Value {
    let pre_upscale_image = result.image_output.clone();
    let final_image = if params.upscale_enabled {
        upscale::append_upscale_chain(&mut result, params, seed)
    } else {
        result.image_output.clone()
    };

    // Optionally save the base image before upscaling. Skipped in refine-only
    // mode, where the pre-upscale image is just the unchanged input image.
    if params.upscale_enabled && params.save_pre_upscale_image && !params.refine_only {
        let pre_save_id = result.next_id.to_string();
        result.next_id += 1;
        let output_format = match params.output_format.as_str() {
            "jxl" => "jxl_raw",
            "webp" => "webp_raw",
            _ => "png",
        };
        result.workflow.insert(
            pre_save_id,
            json!({
                "class_type": "MooshieSaveImage",
                "inputs": {
                    "images": [pre_upscale_image.0, pre_upscale_image.1],
                    "bit_depth": params.output_bit_depth,
                    "output_format": output_format
                }
            }),
        );
    }

    // Apply face fix (FaceDetailer) after upscale if enabled
    let final_image = if params.facefix_enabled {
        facefix::append_facefix_chain(&mut result, params, final_image, seed)
    } else {
        final_image
    };

    // Apply <segment:...> auto-refinement after facefix so face fix results
    // feed into segment detection.
    let final_image = if !params.detail_segments.is_empty() {
        segment_detail::append_segment_chain(&mut result, params, final_image, seed)
    } else {
        final_image
    };

    let save_id = result.next_id.to_string();
    let output_format = match params.output_format.as_str() {
        "jxl" => "jxl_raw",
        "webp" => "webp_raw",
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

/// Returns the frontend v-pred flag.
pub fn is_vpred_model(params: &GenerationParams) -> bool {
    params.is_vpred_model
}

/// Returns true when the resolved architecture uses a 16-channel latent bucket.
/// Anima/Wan (Wan2.1-based) need this — a 4-channel latent fails at decode.
pub fn needs_sd3_latent(params: &GenerationParams) -> bool {
    matches!(
        params.model_architecture.as_str(),
        "sd3"
            | "flux"
            | "flux1d"
            | "flux1s"
            | "flux1krea"
            | "chroma"
            | "zib"
            | "zit"
            | "qwen"
            | "qwen_edit"
            | "qwen_edit_plus"
            | "flux1kontext"
            | "anima"
            | "wan"
            | "krea2"
    )
}

/// Returns true when the resolved architecture uses the Flux.2 latent node.
pub fn needs_flux2_latent(params: &GenerationParams) -> bool {
    matches!(
        params.model_architecture.as_str(),
        "flux2d"
            | "flux2klein9b"
            | "flux2klein9bbase"
            | "flux2klein4b"
            | "flux2klein4bbase"
            | "ideogram4"
    )
}

/// TODO:
/// flux2 uses "Flux2Scheduler" - adding this would be fairly complex, because it is not compatible with the current scheduler UI and would require a separate workflow path.

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
    if params.model_architecture == "mugen" {
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

/// Positive prompt context shared by every regional CLIP encode (main prompt, schedule segments, LoRA tags).
pub fn build_regional_context_prompt(params: &GenerationParams) -> String {
    let mut parts: Vec<String> = Vec::new();

    let base = params.positive_prompt.trim();
    if !base.is_empty() {
        parts.push(base.to_string());
    }

    for segment in &params.positive_segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }
        if parts.iter().any(|p| p == text) {
            continue;
        }
        parts.push(text.to_string());
    }

    let mut combined = parts.join(", ");
    for lora in &params.loras {
        if lora.name.trim().is_empty() {
            continue;
        }
        if prompt_contains_lora_tag(&combined, &lora.name) {
            continue;
        }
        let strength = format_lora_tag_strength(lora.strength_clip);
        let tag = format!("<lora:{}:{}>", lora.name.trim(), strength);
        if combined.is_empty() {
            combined = tag;
        } else {
            combined.push_str(", ");
            combined.push_str(&tag);
        }
    }

    combined
}

/// Merge global context with a region's local prompt for area conditioning.
pub fn merge_regional_encode_text(context: &str, region_text: &str) -> String {
    let context = context.trim();
    let local = region_text.trim();
    if local.is_empty() {
        return context.to_string();
    }
    if context.is_empty() {
        return local.to_string();
    }
    if local.contains(context) || context.contains(local) {
        return if local.len() >= context.len() {
            local.to_string()
        } else {
            format!("{context}, {local}")
        };
    }
    format!("{context}, {local}")
}

fn format_lora_tag_strength(strength: f64) -> String {
    let s = strength.clamp(0.0, 2.0);
    if (s - s.round()).abs() < f64::EPSILON {
        format!("{}", s.round() as i32)
    } else {
        let formatted = format!("{s:.2}");
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn prompt_contains_lora_tag(prompt: &str, lora_name: &str) -> bool {
    let name = lora_name.trim();
    if name.is_empty() {
        return false;
    }
    let needle = format!("<lora:{}", name.to_lowercase());
    prompt.to_lowercase().contains(&needle)
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
    if params.model_architecture == "nanosaur" {
        return;
    }

    if params.model_architecture == "mugen" {
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
    } else if params.model_architecture == "sd3" {
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
    } else if matches!(
        params.model_architecture.as_str(),
        "flux" | "flux1d" | "flux1s" | "flux1krea" | "chroma"
    ) {
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
    } else if params.model_architecture == "auraflow" {
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

/// Patch SDXL-family v-prediction models with zero-terminal SNR discrete sampling.
/// ComfyUI model loader already detects most v-pred models on its own, except when the header (in .safetensors) does not contain a top-level v_pred entry.
fn inject_vpred_zsnr_sampling(result: &mut WorkflowResult, params: &GenerationParams) {
    if !params.is_sdxl_like || !is_vpred_model(params) {
        return;
    }

    let node_id = result.next_id.to_string();
    result.workflow.insert(
        node_id.clone(),
        json!({
            "class_type": "ModelSamplingDiscrete",
            "inputs": {
                "model": [result.model_source.0.clone(), result.model_source.1],
                "sampling": "v_prediction",
                "zsnr": true
            }
        }),
    );
    result.model_source = (node_id, 0);
    result.next_id += 1;

    if let Some(sampler_node) = result.workflow.get_mut(&result.sampler_id) {
        if let Some(inputs) = sampler_node.get_mut("inputs") {
            inputs["model"] = json!([result.model_source.0, result.model_source.1]);
        }
    }
}

/// Inject Stable Cascade model sampling (shift 2.0) for Cascade architecture models.
fn inject_cascade_sampling(result: &mut WorkflowResult, params: &GenerationParams) {
    if params.model_architecture != "cascade" {
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
    if !matches!(
        params.model_architecture.as_str(),
        "flux" | "flux1d" | "flux1krea"
    ) {
        return;
    }

    let node_id = result.next_id.to_string();
    result.workflow.insert(
        node_id.clone(),
        json!({
            "class_type": "FluxGuidance",
            "inputs": {
                "conditioning": [result.positive_source.0.clone(), result.positive_source.1],
                "guidance": params.flux_guidance
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
