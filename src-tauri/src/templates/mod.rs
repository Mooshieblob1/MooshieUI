pub mod controlnet;
pub mod facefix;
pub mod image_edit;
pub mod img2img;
pub mod inpainting;
pub mod rife;
pub mod segment_detail;
pub mod style_ref;
pub mod style_transfer;
pub mod txt2img;
pub mod upscale;
pub mod upscale_standalone;
pub mod video;
pub mod video_interpolate;

use serde_json::{json, Value};

use crate::comfyui::types::{BaseSources, GenerationParams, PromptSegment, StageContext};

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
    validate_pause_resume(params)?;

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

    if params.style_ref_enabled && params.mode != "video" && params.mode != "image_edit" {
        if !style_ref::family_supports_style_ref(&params.model_architecture) {
            return Err(format!(
                "Style reference is only available for SD1.5, SDXL, and Flux.1 models. \
                 The selected model family ('{}') is not supported. \
                 Disable style reference or switch to a supported model.",
                params.model_architecture
            ));
        }
        if params
            .style_ref_image
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            return Err(
                "Style reference is enabled but no reference image was provided. \
                 Please upload a style reference image."
                    .into(),
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

    // INT8-Fast guard: when enabled for a split-model family, the family must
    // have a known OTUNetLoaderW8A8 model_type mapping. GGUF diffusion models
    // are incompatible with the INT8-Fast loader (they route through
    // UnetLoaderGGUF, not OTUNetLoaderW8A8) so that combination is rejected
    // here as a hard error rather than silently ignored.
    if params.int8_fast_enabled && params.use_split_model {
        let unet = params
            .diffusion_model
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        if unet.ends_with(".gguf") {
            return Err(
                "INT8-Fast loader is not compatible with GGUF diffusion models. \
                 Disable the INT8-Fast loader in Settings before using a GGUF checkpoint."
                    .to_string(),
            );
        }
        if int8_fast_model_type(&params.model_architecture).is_none() {
            return Err(format!(
                "INT8-Fast loader does not support the '{}' model family. \
                 Supported families: Flux.2 (Klein/Dev), Z-Image, Chroma, Krea 2, \
                 Qwen, Anima, Ideogram 4. Disable the INT8-Fast loader or pick a \
                 supported model.",
                params.model_architecture
            ));
        }
    }

    Ok(())
}

/// Settings that every stage of a paused run must share. The first stage's
/// latent is already sampled at its resolution and batch size, on its
/// schedule, so a later stage cannot change any of these without the resumed
/// noise level being wrong.
fn pause_locked_settings(params: &GenerationParams) -> [(&'static str, String); 11] {
    [
        ("steps", params.steps.to_string()),
        ("scheduler", params.scheduler.clone()),
        ("width", params.width.to_string()),
        ("height", params.height.to_string()),
        ("batch size", params.batch_size.to_string()),
        ("checkpoint", params.checkpoint.clone()),
        ("split model", params.use_split_model.to_string()),
        (
            "diffusion model",
            params.diffusion_model.clone().unwrap_or_default(),
        ),
        (
            "text encoder",
            params.clip_model.clone().unwrap_or_default(),
        ),
        ("VAE", params.vae.clone().unwrap_or_default()),
        ("model architecture", params.model_architecture.clone()),
    ]
}

/// Check a pause request or a resumed run before it is built.
///
/// Every stage must stop strictly after the one before it and before the
/// schedule ends, and must agree with this request on the settings the
/// first stage's latent was sampled with (see `pause_locked_settings`).
fn validate_pause_resume(params: &GenerationParams) -> Result<(), String> {
    if params.mode != "txt2img" {
        if !params.resume_stages.is_empty() {
            return Err(
                "A paused generation can only be continued in Text to Image mode.".to_string(),
            );
        }
        return Ok(());
    }

    let steps = params.steps;
    let mut prev_end: u32 = 0;
    let locked = pause_locked_settings(params);

    for (index, stage) in params.resume_stages.iter().enumerate() {
        let number = index + 1;
        let stage_params = &stage.params;
        if stage_params.mode != "txt2img" {
            return Err(format!(
                "Paused stage {number} was not a Text to Image generation."
            ));
        }
        let end = stage_params.pause_at_step.ok_or_else(|| {
            format!("Paused stage {number} has no pause step, so there is nothing to resume from.")
        })?;
        if end <= prev_end || end >= steps {
            return Err(format!(
                "Paused stage {number} stopped at step {end}, which must be after step {prev_end} and before the final step ({steps})."
            ));
        }
        for ((name, wanted), (_, actual)) in pause_locked_settings(stage_params).iter().zip(&locked)
        {
            if wanted != actual {
                return Err(format!(
                    "Cannot change the {name} while a generation is paused (paused stage used \"{wanted}\", current is \"{actual}\"). Discard the paused run to change it."
                ));
            }
        }
        prev_end = end;
    }

    if let Some(pause_at) = params.pause_at_step {
        if pause_at <= prev_end || pause_at >= steps {
            return Err(format!(
                "Pause step {pause_at} must be after step {prev_end} and before the final step ({steps})."
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
    /// Model for the appended refinement samplers (upscale/facefix/segment)
    /// when it must differ from `model_source`. The Anima ReStyler's Cosmos
    /// reference patch concatenates a fixed-size reference latent on every
    /// step, so it only works at the base generation size — reusing it in a
    /// hires or detailer pass crashes with a tensor size mismatch. `None`
    /// means the chains use `model_source` as usual.
    pub refiner_model_source: Option<(String, u32)>,
    /// Loader outputs before the LoRA chain, so a later stage of a paused run
    /// can build its own LoRA chain on the same loaded model. `None` for
    /// templates that cannot be paused.
    pub base_sources: Option<BaseSources>,
}

impl WorkflowResult {
    /// Model source for the appended refinement samplers (upscale/facefix/segment).
    pub fn refiner_model(&self) -> (String, u32) {
        self.refiner_model_source
            .clone()
            .unwrap_or_else(|| self.model_source.clone())
    }
}

/// Outputs from the model loading stage (checkpoint or split model).
pub struct ModelLoadResult {
    pub model_source: (String, u32),
    pub clip_source: (String, u32),
    pub vae_source: (String, u32),
    pub next_id: u32,
    /// Loader outputs before the LoRA chain, for later stages of a paused run.
    pub base: BaseSources,
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

/// Map a model-architecture family string to the `model_type` enum value
/// expected by `OTUNetLoaderW8A8` (ComfyUI-INT8-Fast). Returns `None` for
/// families the node does not support, which causes `validate_params` to
/// reject the combination before workflow construction.
fn int8_fast_model_type(family: &str) -> Option<&'static str> {
    match family {
        // Flux.2 Klein (all variants: 4b, 4b-base, 9b, 9b-base) and Flux.2 Dev
        "flux2d" | "flux2klein9b" | "flux2klein9bbase" | "flux2klein4b" | "flux2klein4bbase" => {
            Some("flux2")
        }
        // Z-Image variants (turbo and base)
        "zit" | "zib" => Some("z-image"),
        // Other mapped families
        "chroma" => Some("chroma"),
        "krea2" => Some("krea2"),
        "qwen" | "qwen_edit" | "qwen_edit_plus" => Some("qwen"),
        "anima" => Some("anima"),
        "ideogram4" => Some("ideogram4"),
        // Everything else is unsupported by the OTUNetLoaderW8A8 node
        _ => None,
    }
}

/// Load model nodes — either a single CheckpointLoaderSimple or split UNETLoader + CLIPLoader + VAELoader.
/// Also handles the LoRA chain and optional separate VAE override.
pub fn load_model_nodes(
    workflow: &mut serde_json::Map<String, Value>,
    mut next_id: u32,
    params: &GenerationParams,
) -> ModelLoadResult {
    let (mut model_source, mut clip_source, mut vae_source);

    // A later stage of a paused run reuses the first stage's loader nodes so
    // the checkpoint is not loaded twice. Only the LoRA chain below is rebuilt,
    // which is what lets the resumed stage add, drop or reweight LoRAs.
    let resumed_base = params.stage.as_ref().and_then(|s| s.base.clone());

    if let Some(base) = resumed_base {
        model_source = base.model;
        clip_source = base.clip;
        vae_source = base.vae;
    } else if params.model_architecture == "nanosaur" {
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
            base: BaseSources {
                model: model_source.clone(),
                clip: clip_source.clone(),
                vae: vae_source.clone(),
            },
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
        } else if params.int8_fast_enabled {
            // INT8/ConvRot pre-quantized model: requires OTUNetLoaderW8A8 from
            // ComfyUI-INT8-Fast (NVIDIA only). validate_params guarantees the
            // family has a known model_type mapping before we reach here.
            let model_type = int8_fast_model_type(&params.model_architecture).unwrap_or("flux2");
            json!({
                "class_type": "OTUNetLoaderW8A8",
                "inputs": {
                    "unet_name": unet_name,
                    "weight_dtype": "default",
                    "model_type": model_type,
                    "on_the_fly_quantization": false,
                    "enable_convrot": params.int8_fast_convrot,
                    "lora_mode": "default"
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

    let base_model = model_source.clone();
    let base_clip = clip_source.clone();

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

    // Optional separate VAE override (only for non-split models, split already has its own VAE).
    // A resumed stage inherits the first stage's VAE source, override included.
    let resumed = params.stage.as_ref().is_some_and(|s| s.base.is_some());
    if !params.use_split_model && !resumed {
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

    // Anima TeaCache — wraps the fully-assembled model (after LoRAs) in a
    // step-caching function. Inserted here rather than as a post-hoc inject_*
    // step in `build_workflow` because this is the one function every
    // Anima-capable template routes through, including `style_transfer::build`,
    // which bypasses the entire `inject_*` pipeline.
    if params.model_architecture == "anima" && params.anima_teacache_enabled {
        let teacache_id = next_id.to_string();
        workflow.insert(
            teacache_id.clone(),
            json!({
                "class_type": "MooshieAnimaTeaCache",
                "inputs": {
                    "model": [model_source.0, model_source.1],
                    "rel_l1_thresh": 0.15,
                    "start_step": 2,
                    "end_step": -2,
                    "total_steps": params.steps
                }
            }),
        );
        model_source = (teacache_id, 0);
        next_id += 1;
    }

    ModelLoadResult {
        base: BaseSources {
            model: base_model,
            clip: base_clip,
            vae: vae_source.clone(),
        },
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

    if pause_resume_active(params) {
        return build_paused_workflow(params, seed);
    }

    if params.style_transfer_enabled && params.model_architecture == "anima" {
        let result = style_transfer::build(params, seed);
        return finish_workflow(result, params, seed);
    }

    let result = build_image_stage(params, seed);
    finish_workflow(result, params, seed)
}

/// Whether this request pauses partway through sampling or resumes a paused run.
fn pause_resume_active(params: &GenerationParams) -> bool {
    params.mode == "txt2img" && (params.pause_at_step.is_some() || !params.resume_stages.is_empty())
}

/// Settings that cannot take part in a paused run.
///
/// Style transfer samples through its own `SamplerCustomAdvanced` graph, which
/// has no start/end step, and Anima TeaCache keeps a step counter inside the
/// cached model patch that would carry over from one stage into the next. Both
/// are switched off for every stage rather than rejected, so a user who pauses
/// simply gets the plain sampler.
fn strip_unpausable_settings(params: &mut GenerationParams) {
    params.style_transfer_enabled = false;
    params.anima_teacache_enabled = false;
}

/// Assemble a paused or resumed txt2img run.
///
/// Every earlier stage is rebuilt from the parameters it originally ran with,
/// producing byte-identical nodes with the same IDs, so ComfyUI's execution
/// cache hands back its latent instead of sampling it again. This request's
/// settings then become the final stage, sampling from the last cached latent.
/// Only the final stage is decoded and saved.
fn build_paused_workflow(params: &GenerationParams, seed: i64) -> Value {
    let mut combined = serde_json::Map::new();
    let mut next_id: u32 = 1;
    let mut start_step: u32 = 0;
    let mut latent: Option<(String, u32)> = None;
    let mut base: Option<BaseSources> = None;

    for stage in &params.resume_stages {
        let mut stage_params = (*stage.params).clone();
        // A stage's own history is already covered by the stages before it.
        stage_params.resume_stages.clear();
        strip_unpausable_settings(&mut stage_params);
        // The frontend never sends `resolved_model_path`; the generate command
        // fills it in per request. The model cannot change across a paused
        // run, so the path resolved for this request is the one the stage
        // was built with, and the loader node must come out identical.
        if stage_params.resolved_model_path.is_none() {
            stage_params.resolved_model_path = params.resolved_model_path.clone();
        }
        let end_step = stage_params.pause_at_step;
        stage_params.stage = Some(StageContext {
            first_id: next_id,
            start_step,
            end_step,
            latent: latent.clone(),
            base: base.clone(),
        });

        let result = build_image_stage(&stage_params, stage.seed);
        let mut workflow = result.workflow;
        // The intermediate was decoded when the stage was first shown; the
        // resumed graph only needs its latent.
        workflow.remove(&result.image_output.0);
        combined.extend(workflow);

        next_id = result.next_id;
        latent = Some((result.sampler_id, 0));
        if base.is_none() {
            base = result.base_sources;
        }
        start_step = end_step.unwrap_or(start_step);
    }

    let mut final_params = params.clone();
    final_params.resume_stages.clear();
    strip_unpausable_settings(&mut final_params);
    final_params.stage = Some(StageContext {
        first_id: next_id,
        start_step,
        end_step: params.pause_at_step,
        latent,
        base,
    });

    let mut result = build_image_stage(&final_params, seed);
    combined.extend(std::mem::take(&mut result.workflow));
    result.workflow = combined;
    finish_workflow(result, &final_params, seed)
}

/// Build one image template plus the model/conditioning patches that every
/// image mode shares. `finish_workflow` appends the post-process chains.
fn build_image_stage(params: &GenerationParams, seed: i64) -> WorkflowResult {
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

    // NAG + APG (core ComfyUI model patchers) for SDXL-family models
    inject_sdxl_guidance_extras(&mut result, params);

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

    // Inject style reference if enabled (IP-Adapter for SD1.5/SDXL, Flux Redux for Flux.1)
    if params.style_ref_enabled && params.mode != "video" && params.mode != "image_edit" {
        if style_ref::family_supports_style_ref(&params.model_architecture) {
            style_ref::inject_style_ref(&mut result, params);
        }
    }

    result
}

fn finish_workflow(mut result: WorkflowResult, params: &GenerationParams, seed: i64) -> Value {
    // A stage that stops partway through the schedule produces a half-denoised
    // preview for the user to look at, not a finished image: upscaling,
    // face fixing or segment refinement would run on noise. Those chains run
    // once, on the stage that completes the schedule.
    let intermediate = params
        .stage
        .as_ref()
        .is_some_and(|stage| stage.end_step.is_some());
    let pre_upscale_image = result.image_output.clone();
    let final_image = if params.upscale_enabled && !intermediate {
        upscale::append_upscale_chain(&mut result, params, seed)
    } else {
        result.image_output.clone()
    };

    // Optionally save the base image before upscaling. Skipped in refine-only
    // mode, where the pre-upscale image is just the unchanged input image.
    if params.upscale_enabled
        && params.save_pre_upscale_image
        && !params.refine_only
        && !intermediate
    {
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
    let final_image = if params.facefix_enabled && !intermediate {
        facefix::append_facefix_chain(&mut result, params, final_image, seed)
    } else {
        final_image
    };

    // Apply <segment:...> auto-refinement after facefix so face fix results
    // feed into segment detection.
    let final_image = if !params.detail_segments.is_empty() && !intermediate {
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

    // RescaleCFG companion: v-pred models oversaturate at normal CFG without a
    // per-step rescale of the guidance vector. MooshieSoftGuidance implements
    // the same math as ComfyUI's core RescaleCFG (Common Diffusion Noise
    // Schedules, Appendix I) and ships with the app, so no extra install.
    if params.vpred_rescale_cfg && params.vpred_rescale_cfg_multiplier > 0.0 {
        let rescale_id = result.next_id.to_string();
        result.workflow.insert(
            rescale_id.clone(),
            json!({
                "class_type": "MooshieSoftGuidance",
                "inputs": {
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "multiplier": params.vpred_rescale_cfg_multiplier.clamp(0.0, 1.0)
                }
            }),
        );
        result.model_source = (rescale_id, 0);
        result.next_id += 1;
    }

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

/// NAG (Normalized Attention Guidance) and APG (Adaptive Projected Guidance)
/// for SDXL-family models. Both are core ComfyUI model patchers that stack with
/// Smart Guidance and RescaleCFG: NAG patches attn1 so the negative prompt
/// stays effective (works even at CFG 1), APG projects the guidance vector to
/// its perpendicular component to prevent oversaturation at higher CFG.
fn inject_sdxl_guidance_extras(result: &mut WorkflowResult, params: &GenerationParams) {
    if !params.is_sdxl_like {
        return;
    }

    if params.nag_enabled {
        let node_id = result.next_id.to_string();
        result.workflow.insert(
            node_id.clone(),
            json!({
                "class_type": "NAGuidance",
                "inputs": {
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "nag_scale": params.nag_scale.clamp(0.0, 50.0),
                    "nag_alpha": 0.5,
                    "nag_tau": 1.5
                }
            }),
        );
        result.model_source = (node_id, 0);
        result.next_id += 1;
    }

    // APG intercepts pre-CFG and needs both cond and uncond batches, which only
    // exist at CFG > 1 — at CFG <= 1 ComfyUI skips the uncond pass entirely.
    if params.apg_enabled && params.cfg > 1.0 {
        let node_id = result.next_id.to_string();
        result.workflow.insert(
            node_id.clone(),
            json!({
                "class_type": "APG",
                "inputs": {
                    "model": [result.model_source.0.clone(), result.model_source.1],
                    "eta": params.apg_eta.clamp(-10.0, 10.0),
                    "norm_threshold": params.apg_norm_threshold.clamp(0.0, 50.0),
                    "momentum": params.apg_momentum.clamp(-5.0, 1.0)
                }
            }),
        );
        result.model_source = (node_id, 0);
        result.next_id += 1;
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Minimal params for a split-model INT8-Fast generation targeting a
    /// Flux2-Klein-9B file. `serde_json::from_value` gives us the `Default`
    /// semantics of all the `#[serde(default)]` fields we don't care about.
    fn klein_int8_params(family: &str, enabled: bool) -> GenerationParams {
        let mut value = json!({
            "mode": "txt2img",
            "positive_prompt": "a red fox",
            "negative_prompt": "",
            "checkpoint": "",
            "loras": [],
            "sampler_name": "euler",
            "scheduler": "normal",
            "steps": 20,
            "cfg": 1.0,
            "seed": "42",
            "width": 1024,
            "height": 1024,
            "batch_size": 1,
            "denoise": 1.0,
            "upscale_enabled": false,
            "upscale_method": "latent",
            "upscale_scale": 2.0,
            "upscale_denoise": 0.5,
            "upscale_steps": 10,
            "upscale_tile_size": 512,
            "upscale_tiling": false,
            "use_split_model": true,
            "diffusion_model": "flux2-klein-9b-int8-convrot.safetensors",
            "model_architecture": family,
            "int8_fast_enabled": enabled,
            "int8_fast_convrot": true
        });
        // Krea 2 split-model validation requires clip_type = "krea2".
        if family == "krea2" {
            value["clip_type"] = json!("krea2");
        }
        serde_json::from_value(value).expect("test params must deserialize")
    }

    #[test]
    fn int8_fast_emits_otunetloader_for_klein() {
        let params = klein_int8_params("flux2klein9b", true);
        let mut workflow = serde_json::Map::new();
        let result = load_model_nodes(&mut workflow, 1, &params);
        // The first node inserted must be OTUNetLoaderW8A8.
        let node = workflow
            .get(&result.model_source.0)
            .expect("model source node must be present");
        assert_eq!(
            node["class_type"],
            json!("OTUNetLoaderW8A8"),
            "INT8-Fast mode must emit OTUNetLoaderW8A8, not UNETLoader"
        );
        assert_eq!(
            node["inputs"]["model_type"],
            json!("flux2"),
            "Klein 9b family must map to 'flux2' model_type"
        );
        assert_eq!(
            node["inputs"]["enable_convrot"],
            json!(true),
            "enable_convrot must follow int8_fast_convrot param"
        );
        assert_eq!(
            node["inputs"]["on_the_fly_quantization"],
            json!(false),
            "on_the_fly_quantization must be false for pre-quantized files"
        );
    }

    #[test]
    fn int8_fast_disabled_emits_unetloader() {
        let params = klein_int8_params("flux2klein9b", false);
        let mut workflow = serde_json::Map::new();
        let result = load_model_nodes(&mut workflow, 1, &params);
        let node = workflow
            .get(&result.model_source.0)
            .expect("model source node must be present");
        assert_eq!(
            node["class_type"],
            json!("UNETLoader"),
            "When INT8-Fast is disabled, UNETLoader must be used"
        );
    }

    #[test]
    fn validate_params_rejects_unsupported_family_with_int8_fast() {
        let params = klein_int8_params("illustrious", true);
        let err = validate_generation_params(&params);
        assert!(err.is_err(), "Unsupported family must be rejected");
        let msg = err.unwrap_err();
        assert!(
            msg.contains("INT8-Fast loader does not support"),
            "Error must mention INT8-Fast: {msg}"
        );
        assert!(
            msg.contains("illustrious"),
            "Error must name the offending family: {msg}"
        );
    }

    #[test]
    fn validate_params_rejects_int8_fast_with_gguf_model() {
        // INT8-Fast + GGUF diffusion model must be a hard error: the GGUF loader
        // and the INT8-Fast loader are mutually exclusive code paths, so allowing
        // this combination would silently ignore the INT8-Fast toggle.
        let mut params = klein_int8_params("flux2d", true);
        params.diffusion_model = Some("flux1-dev-Q4_K_M.gguf".to_string());
        let err = validate_generation_params(&params);
        assert!(err.is_err(), "INT8-Fast + GGUF must be rejected");
        let msg = err.unwrap_err();
        assert!(
            msg.contains("INT8-Fast loader is not compatible with GGUF"),
            "Error must explain the GGUF incompatibility: {msg}"
        );
    }

    #[test]
    fn validate_params_accepts_supported_families() {
        // krea2 is excluded here because it has a separate CLIP-encoder guard in
        // validate_generation_params that rejects params without the Qwen3-VL 4B
        // encoder set; it would fail that guard, not the INT8-Fast guard.
        // The int8_fast_model_type mapping for krea2 is verified in
        // int8_fast_model_type_maps_correctly below.
        for family in &[
            "flux2klein9b",
            "flux2klein4b",
            "flux2klein9bbase",
            "flux2klein4bbase",
            "flux2d",
            "zit",
            "zib",
            "chroma",
            "qwen",
            "anima",
            "ideogram4",
        ] {
            let params = klein_int8_params(family, true);
            assert!(
                validate_generation_params(&params).is_ok(),
                "Family '{}' must be accepted by INT8-Fast validate_params",
                family
            );
        }
    }

    #[test]
    fn int8_fast_model_type_maps_correctly() {
        assert_eq!(int8_fast_model_type("flux2klein9b"), Some("flux2"));
        assert_eq!(int8_fast_model_type("flux2klein4bbase"), Some("flux2"));
        assert_eq!(int8_fast_model_type("flux2d"), Some("flux2"));
        assert_eq!(int8_fast_model_type("zit"), Some("z-image"));
        assert_eq!(int8_fast_model_type("zib"), Some("z-image"));
        assert_eq!(int8_fast_model_type("chroma"), Some("chroma"));
        assert_eq!(int8_fast_model_type("krea2"), Some("krea2"));
        assert_eq!(int8_fast_model_type("qwen_edit"), Some("qwen"));
        assert_eq!(int8_fast_model_type("anima"), Some("anima"));
        assert_eq!(int8_fast_model_type("ideogram4"), Some("ideogram4"));
        assert_eq!(int8_fast_model_type("illustrious"), None);
        assert_eq!(int8_fast_model_type("sd15"), None);
        assert_eq!(int8_fast_model_type("unknown"), None);
    }

    // ----- pause / resume -----

    /// Plain SDXL checkpoint txt2img params, the simplest graph that can pause.
    fn pausable_params() -> GenerationParams {
        serde_json::from_value(json!({
            "mode": "txt2img",
            "positive_prompt": "a red fox",
            "negative_prompt": "blurry",
            "checkpoint": "sdxl.safetensors",
            "loras": [],
            "sampler_name": "euler",
            "scheduler": "normal",
            "steps": 20,
            "cfg": 6.0,
            "seed": "42",
            "width": 1024,
            "height": 1024,
            "batch_size": 1,
            "denoise": 1.0,
            "upscale_enabled": false,
            "upscale_method": "latent",
            "upscale_scale": 2.0,
            "upscale_denoise": 0.5,
            "upscale_steps": 10,
            "upscale_tile_size": 512,
            "upscale_tiling": false,
            "use_split_model": false,
            "model_architecture": "sdxl",
            "is_sdxl_like": true
        }))
        .expect("test params must deserialize")
    }

    fn nodes_of_class<'a>(
        workflow: &'a serde_json::Map<String, Value>,
        class: &str,
    ) -> Vec<(&'a String, &'a Value)> {
        let mut nodes: Vec<_> = workflow
            .iter()
            .filter(|(_, node)| node["class_type"] == json!(class))
            .collect();
        nodes.sort_by_key(|(id, _)| id.parse::<u32>().unwrap_or(u32::MAX));
        nodes
    }

    fn resume_stage(params: &GenerationParams, seed: i64) -> crate::comfyui::types::ResumeStage {
        crate::comfyui::types::ResumeStage {
            params: Box::new(params.clone()),
            seed,
            worker_id: Some(0),
        }
    }

    #[test]
    fn pause_stops_early_and_keeps_leftover_noise() {
        let mut params = pausable_params();
        params.pause_at_step = Some(5);
        // Post-process chains must not run on a half-denoised preview.
        params.upscale_enabled = true;
        params.facefix_enabled = true;

        let workflow = build_workflow(&params, 42, false);
        let workflow = workflow.as_object().expect("workflow is an object");

        assert!(
            nodes_of_class(workflow, "KSampler").is_empty(),
            "a paused run must not use the plain KSampler"
        );
        let samplers = nodes_of_class(workflow, "KSamplerAdvanced");
        assert_eq!(samplers.len(), 1, "one sampler for the first stage");
        let inputs = &samplers[0].1["inputs"];
        assert_eq!(inputs["add_noise"], json!("enable"));
        assert_eq!(inputs["start_at_step"], json!(0));
        assert_eq!(inputs["end_at_step"], json!(5));
        assert_eq!(
            inputs["steps"],
            json!(20),
            "schedule is built from the full step count"
        );
        assert_eq!(inputs["return_with_leftover_noise"], json!("enable"));
        assert_eq!(inputs["noise_seed"], json!(42));

        assert_eq!(nodes_of_class(workflow, "VAEDecode").len(), 1);
        assert_eq!(nodes_of_class(workflow, "MooshieSaveImage").len(), 1);
        assert!(
            nodes_of_class(workflow, "FaceDetailer").is_empty()
                && nodes_of_class(workflow, "LatentUpscaleBy").is_empty()
                && nodes_of_class(workflow, "ImageScaleBy").is_empty(),
            "upscale and face fix wait for the stage that finishes the schedule"
        );
    }

    #[test]
    fn resume_rebuilds_the_paused_stage_verbatim_and_samples_the_rest() {
        let mut first = pausable_params();
        first.pause_at_step = Some(5);
        let paused = build_workflow(&first, 42, false);
        let paused = paused.as_object().unwrap();
        let paused_sampler_id = nodes_of_class(paused, "KSamplerAdvanced")[0].0.clone();
        let paused_decode_id = nodes_of_class(paused, "VAEDecode")[0].0.clone();
        let paused_save_id = nodes_of_class(paused, "MooshieSaveImage")[0].0.clone();

        // The user changes the prompt, CFG, sampler and adds a LoRA before continuing.
        let mut second = pausable_params();
        second.positive_prompt = "a red fox wearing a crown".to_string();
        second.cfg = 4.0;
        second.sampler_name = "dpmpp_2m".to_string();
        second.loras = vec![crate::comfyui::types::LoraParam {
            name: "crown.safetensors".to_string(),
            strength_model: 0.8,
            strength_clip: 0.8,
        }];
        second.seed = 7;
        second.resume_stages = vec![resume_stage(&first, 42)];
        validate_generation_params(&second).expect("resume with unlocked changes is valid");

        let resumed = build_workflow(&second, 7, false);
        let resumed = resumed.as_object().unwrap();

        // Every node of the paused stage except its decode and save comes back
        // byte-identical under the same ID, which is what lets ComfyUI's
        // execution cache serve the latent instead of sampling it again.
        for (id, node) in paused {
            if *id == paused_decode_id || *id == paused_save_id {
                continue;
            }
            assert_eq!(
                resumed.get(id),
                Some(node),
                "paused-stage node {id} must be rebuilt unchanged"
            );
        }
        assert!(
            resumed
                .get(&paused_decode_id)
                .is_none_or(|n| n["class_type"] != json!("VAEDecode")),
            "the intermediate is not decoded again on resume"
        );

        let samplers = nodes_of_class(resumed, "KSamplerAdvanced");
        assert_eq!(samplers.len(), 2, "paused stage plus the resumed stage");
        let (_, second_sampler) = samplers[1];
        let inputs = &second_sampler["inputs"];
        assert_eq!(
            inputs["add_noise"],
            json!("disable"),
            "noise was added by the first stage"
        );
        assert_eq!(inputs["start_at_step"], json!(5));
        assert_eq!(inputs["end_at_step"], json!(10000));
        assert_eq!(inputs["return_with_leftover_noise"], json!("disable"));
        assert_eq!(inputs["latent_image"], json!([paused_sampler_id, 0]));
        assert_eq!(inputs["cfg"], json!(4.0));
        assert_eq!(inputs["sampler_name"], json!("dpmpp_2m"));
        assert_eq!(inputs["noise_seed"], json!(7));

        // The new prompt feeds the resumed sampler only.
        let positive_id = inputs["positive"][0].as_str().unwrap();
        assert_eq!(
            resumed[positive_id]["inputs"]["text"],
            json!("a red fox wearing a crown")
        );

        // The checkpoint is loaded once; the resumed stage's LoRA chain starts
        // from the first stage's loader outputs.
        let loaders = nodes_of_class(resumed, "CheckpointLoaderSimple");
        assert_eq!(loaders.len(), 1, "one checkpoint load for the whole run");
        let loras = nodes_of_class(resumed, "LoraLoader");
        assert_eq!(loras.len(), 1);
        assert_eq!(loras[0].1["inputs"]["model"], json!([loaders[0].0, 0]));
        assert_eq!(
            loras[0].1["inputs"]["lora_name"],
            json!("crown.safetensors")
        );
        assert_eq!(inputs["model"], json!([loras[0].0, 0]));

        assert_eq!(nodes_of_class(resumed, "VAEDecode").len(), 1);
        assert_eq!(nodes_of_class(resumed, "MooshieSaveImage").len(), 1);
    }

    #[test]
    fn resume_can_pause_again_later_in_the_schedule() {
        let mut first = pausable_params();
        first.pause_at_step = Some(5);
        let mut second = pausable_params();
        second.pause_at_step = Some(12);
        second.resume_stages = vec![resume_stage(&first, 42)];
        let mut third = pausable_params();
        third.resume_stages = vec![resume_stage(&first, 42), resume_stage(&second, 42)];
        validate_generation_params(&third).expect("ascending pause steps are valid");

        let workflow = build_workflow(&third, 42, false);
        let workflow = workflow.as_object().unwrap();
        let ranges: Vec<(u64, u64)> = nodes_of_class(workflow, "KSamplerAdvanced")
            .iter()
            .map(|(_, n)| {
                (
                    n["inputs"]["start_at_step"].as_u64().unwrap(),
                    n["inputs"]["end_at_step"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(ranges, vec![(0, 5), (5, 12), (12, 10000)]);
        assert_eq!(nodes_of_class(workflow, "VAEDecode").len(), 1);
    }

    #[test]
    fn pause_validation_rejects_bad_steps_and_locked_changes() {
        let mut params = pausable_params();
        params.pause_at_step = Some(0);
        assert!(
            validate_generation_params(&params).is_err(),
            "pause at 0 is no pause"
        );
        params.pause_at_step = Some(20);
        assert!(
            validate_generation_params(&params).is_err(),
            "pause at the last step is no pause"
        );
        params.pause_at_step = Some(19);
        assert!(validate_generation_params(&params).is_ok());

        let mut first = pausable_params();
        first.pause_at_step = Some(5);

        let mut same_step = pausable_params();
        same_step.pause_at_step = Some(5);
        same_step.resume_stages = vec![resume_stage(&first, 42)];
        assert!(
            validate_generation_params(&same_step).is_err(),
            "a second pause must come after the first"
        );

        let mut changed_steps = pausable_params();
        changed_steps.steps = 30;
        changed_steps.resume_stages = vec![resume_stage(&first, 42)];
        let err = validate_generation_params(&changed_steps).unwrap_err();
        assert!(
            err.contains("steps"),
            "error names the locked setting: {err}"
        );

        let mut changed_size = pausable_params();
        changed_size.width = 768;
        changed_size.resume_stages = vec![resume_stage(&first, 42)];
        assert!(validate_generation_params(&changed_size).is_err());

        let mut wrong_mode = pausable_params();
        wrong_mode.mode = "img2img".to_string();
        wrong_mode.input_image = Some("in.png".to_string());
        wrong_mode.resume_stages = vec![resume_stage(&first, 42)];
        assert!(validate_generation_params(&wrong_mode).is_err());
    }

    #[test]
    fn pause_switches_off_teacache_and_style_transfer() {
        let mut params = pausable_params();
        params.model_architecture = "anima".to_string();
        params.is_sdxl_like = false;
        params.anima_teacache_enabled = true;
        params.style_transfer_enabled = true;
        params.style_reference_image = Some("ref.png".to_string());
        params.pause_at_step = Some(5);

        let workflow = build_workflow(&params, 42, false);
        let workflow = workflow.as_object().unwrap();
        assert!(nodes_of_class(workflow, "MooshieAnimaTeaCache").is_empty());
        assert!(
            nodes_of_class(workflow, "SamplerCustomAdvanced").is_empty(),
            "style transfer's own sampler graph must not be used"
        );
        assert_eq!(nodes_of_class(workflow, "KSamplerAdvanced").len(), 1);
    }

    #[test]
    fn full_run_without_pause_still_uses_plain_ksampler() {
        let params = pausable_params();
        let workflow = build_workflow(&params, 42, false);
        let workflow = workflow.as_object().unwrap();
        assert_eq!(nodes_of_class(workflow, "KSampler").len(), 1);
        assert!(nodes_of_class(workflow, "KSamplerAdvanced").is_empty());
    }
}
