//! MiniMax H3 video workflow builder (fl2va and ref2va variants).
//!
//! Unlike the image builders, `build` returns the final workflow JSON
//! directly and never goes through `finish_workflow` — upscale, face fix,
//! segment refinement, and `MooshieSaveImage` are all image-only concepts.
//! The terminal node is `MooshieSaveVideo` (deployed by PR 2).

use serde_json::{json, Value};

use crate::comfyui::types::GenerationParams;

/// Filename markers identifying MiniMax H3 model files (matched as lowercase
/// substrings, any-match). Consumed by the video arm of
/// `validate_generation_params`; PR 5's model onboarding reuses them to
/// filter dropdown lists.
pub const H3_DIFFUSION_MARKERS: [&str; 2] = ["minimax", "h3"];

/// Sampling steps for the undistilled base model.
pub const H3_DEFAULT_STEPS: u32 = 20;

/// Step bounds for the Turbo LoRA. Below 4 the distilled model has no schedule
/// left to work with; above 8 it stops improving and starts over-sharpening.
pub const H3_TURBO_MIN_STEPS: u32 = 4;
pub const H3_TURBO_MAX_STEPS: u32 = 8;

/// MiniMax H3 emits 24 fps and only accepts frame counts on the 17n+5 grid
/// (widget: min 5, max 3600, step 17). Snaps the requested duration UP to
/// the next valid count, clamped to the largest on-grid value <= 3600.
pub fn compute_h3_frame_length(seconds: f64) -> u32 {
    let base = ((seconds * 24.0).round() as i64).max(5);
    let snapped = base + (5 - (base % 17)).rem_euclid(17);
    snapped.min(3592) as u32
}

/// `W:H` -> numerator/denominator. Accepts any positive pair, not just the five
/// UI presets: when the user picks "match image" the frontend resolves the
/// selection to the uploaded frame's own pixel dimensions ("1920:1080") before
/// sending it. Anything unparseable falls back to 16:9.
fn parse_aspect_ratio(aspect_ratio: &str) -> (f64, f64) {
    let mut parts = aspect_ratio.split(':');
    let parsed = match (parts.next(), parts.next(), parts.next()) {
        (Some(w), Some(h), None) => w
            .trim()
            .parse::<f64>()
            .ok()
            .zip(h.trim().parse::<f64>().ok()),
        _ => None,
    };
    match parsed {
        Some((rw, rh)) if rw.is_finite() && rh.is_finite() && rw > 0.0 && rh > 0.0 => (rw, rh),
        _ => (16.0, 9.0),
    }
}

/// Width/height from a target megapixel budget and aspect ratio, snapped to
/// multiples of 32 (the H3 width/height widget step). Replaces the official
/// workflow's ResolutionSelector custom node. Unknown ratios fall back to
/// 16:9.
pub fn compute_h3_dimensions(aspect_ratio: &str, megapixels: f64) -> (u32, u32) {
    let (rw, rh) = parse_aspect_ratio(aspect_ratio);
    let pixels = megapixels.max(0.05) * 1_000_000.0;
    let height = (pixels * rh / rw).sqrt();
    let width = height * rw / rh;
    let snap = |v: f64| ((v / 32.0).round() as u32).max(2) * 32;
    (snap(width), snap(height))
}

/// Flat parameter map for the metadata embedded in a generated video.
///
/// Keys are the same internal names the image path uses, so
/// `format_swarmui_json` maps them onto SwarmUI's field names and a video read
/// comes back through `parse_swarmui_json` looking exactly like an image read.
/// Everything video-specific goes under a `mooshie_` prefix, which the
/// formatter strips and files under `mooshie_extra`.
pub(crate) fn video_metadata_params(
    params: &GenerationParams,
    seed: i64,
) -> std::collections::HashMap<String, String> {
    let (width, height) =
        compute_h3_dimensions(&params.video_aspect_ratio, params.video_megapixels);
    let steps = if params.video_turbo_enabled {
        params
            .video_turbo_steps
            .clamp(H3_TURBO_MIN_STEPS, H3_TURBO_MAX_STEPS)
    } else {
        H3_DEFAULT_STEPS
    };
    // RIFE doubles H3's native 24 fps without changing the clip's duration.
    let fps = if params.video_rife_enabled {
        48.0
    } else {
        24.0
    };

    let mut out = std::collections::HashMap::new();
    let mut put = |k: &str, v: String| {
        if !v.is_empty() {
            out.insert(k.to_string(), v);
        }
    };
    put("positive_prompt", params.positive_prompt.clone());
    put("negative_prompt", params.negative_prompt.clone());
    put(
        "model",
        params.video_diffusion_model.clone().unwrap_or_default(),
    );
    put("seed", seed.to_string());
    put("steps", steps.to_string());
    put("mode", "video".to_string());
    put("size", format!("{width}x{height}"));
    put(
        "sampler",
        if params.video_turbo_enabled {
            "minimax_h3_turbo".to_string()
        } else {
            "res_multistep".to_string()
        },
    );
    put("scheduler", "simple".to_string());
    put("mooshie_video_variant", params.video_variant.clone());
    put("mooshie_video_fps", format!("{fps}"));
    put(
        "mooshie_video_duration_seconds",
        params.video_duration_seconds.to_string(),
    );
    if params.video_turbo_enabled {
        put("mooshie_video_turbo", "true".to_string());
    }
    if params.video_rife_enabled {
        put("mooshie_video_rife", "true".to_string());
    }
    out
}

/// The SwarmUI JSON string sent to `MooshieSaveVideo`'s `metadata_json` input.
pub(crate) fn video_metadata_json(params: &GenerationParams, seed: i64) -> String {
    crate::metadata::format_swarmui_json(&video_metadata_params(params, seed))
}

/// Build the complete MiniMax H3 video workflow for either variant.
///
/// Returns the final workflow JSON directly — never routed through
/// `finish_workflow`. The negative prompt is unused by design: `BasicGuider`
/// has no negative conditioning input.
///
/// `include_metadata` is false when the ComfyUI server's `MooshieSaveVideo` is
/// too old to declare the input. Setting an undeclared input fails ComfyUI's
/// prompt validation outright, so an old node has to mean a bare video rather
/// than a failed generation.
pub fn build(params: &GenerationParams, seed: i64, include_metadata: bool) -> Value {
    let (width, height) =
        compute_h3_dimensions(&params.video_aspect_ratio, params.video_megapixels);
    let length = compute_h3_frame_length(params.video_duration_seconds);

    let mut workflow = serde_json::Map::new();
    let mut next_id: u32 = 1;

    let unet_id = next_id.to_string();
    workflow.insert(
        unet_id.clone(),
        json!({
            "class_type": "UNETLoader",
            "inputs": {
                "unet_name": params.video_diffusion_model.as_deref().unwrap_or(""),
                "weight_dtype": "default"
            }
        }),
    );
    next_id += 1;

    // MiniMax-H3 Turbo LoRA (optional): a distilled adapter that collapses
    // sampling to 4-8 steps. It sits between the DiT and both MODEL consumers
    // (`BasicScheduler` and `BasicGuider`) — re-pointing only one of them would
    // schedule sigmas for a model the guider never sees.
    let model_source_id = if params.video_turbo_enabled {
        let lora_id = next_id.to_string();
        workflow.insert(
            lora_id.clone(),
            json!({
                "class_type": "MiniMaxH3TurboLoRA",
                "inputs": {
                    "model": [unet_id.as_str(), 0],
                    "lora_name": params
                        .video_turbo_lora
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(crate::comfyui::nodes::H3_TURBO_LORA_FILENAME),
                    "strength": 1.0,
                    // false = bypass: the adapter is applied at run time instead of
                    // being merged into the base weights. Sharper, at a higher peak
                    // VRAM cost; merging only pays off on cards that cannot hold both.
                    "low_vram": false
                }
            }),
        );
        next_id += 1;
        lora_id
    } else {
        unet_id.clone()
    };

    let clip_id = next_id.to_string();
    workflow.insert(
        clip_id.clone(),
        json!({
            "class_type": "CLIPLoader",
            "inputs": {
                "clip_name": params.video_clip_model.as_deref().unwrap_or(""),
                "type": "minimax"
            }
        }),
    );
    next_id += 1;

    let vae_id = next_id.to_string();
    workflow.insert(
        vae_id.clone(),
        json!({
            "class_type": "VAELoader",
            "inputs": { "vae_name": params.video_vae_model.as_deref().unwrap_or("") }
        }),
    );
    next_id += 1;

    let audio_vae_id = next_id.to_string();
    workflow.insert(
        audio_vae_id.clone(),
        json!({
            "class_type": "VAELoader",
            "inputs": { "vae_name": params.video_audio_vae_model.as_deref().unwrap_or("") }
        }),
    );
    next_id += 1;

    // A non-empty `video_timeline_data` routes the graph through the vendored
    // `MooshieH3Director` instead of the plain native H3 node: same model, same
    // sampler, but the conditioning and latent come from a compiled timeline.
    let timeline_data = params
        .video_timeline_data
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let is_ref2va = params.video_variant == "ref2va";

    let h3_id = if let Some(timeline) = timeline_data {
        let mut inputs = serde_json::Map::new();
        inputs.insert("clip".to_string(), json!([clip_id.as_str(), 0]));
        inputs.insert("vae".to_string(), json!([vae_id.as_str(), 0]));
        inputs.insert("audio_vae".to_string(), json!([audio_vae_id.as_str(), 0]));
        // `model` and `model_ref2va` are lazy inputs and the node evaluates only
        // the one matching `reference_mode`. Wiring exactly the matching socket
        // keeps ComfyUI from loading ~42 GB of DiT twice; the other stays
        // `_UNCONNECTED`, which `check_lazy_status` accepts without a warning.
        inputs.insert(
            if is_ref2va { "model_ref2va" } else { "model" }.to_string(),
            json!([model_source_id.as_str(), 0]),
        );
        inputs.insert("timeline_data".to_string(), json!(timeline));
        // `global_prompt` is `force_input`, so it cannot carry a widget value.
        // The node falls back to `timeline_data["global_prompt"]`, which is where
        // the frontend puts the composed positive prompt.
        //
        // The seconds widgets are inert: `resolve_window` reads `start_frame` and
        // `duration_frames` (or the unconnected `start`/`end`/`duration` sockets)
        // and nothing else. They are emitted consistently anyway so the numbers
        // agree for anyone who opens the graph in ComfyUI.
        let duration_frames = ((params.video_duration_seconds * 24.0).round() as i64).max(5);
        inputs.insert("start_second".to_string(), json!(0.0));
        inputs.insert(
            "end_second".to_string(),
            json!(params.video_duration_seconds),
        );
        inputs.insert(
            "duration_seconds".to_string(),
            json!(params.video_duration_seconds),
        );
        inputs.insert("start_frame".to_string(), json!(0));
        inputs.insert("end_frame".to_string(), json!(duration_frames));
        inputs.insert("duration_frames".to_string(), json!(duration_frames));
        inputs.insert("frame_rate".to_string(), json!(24.0));
        inputs.insert("display_mode".to_string(), json!("seconds"));
        // Declared but never read by `execute` — they mirror editor state for the
        // node's own JS panel, which MooshieUI does not vendor. Still required
        // inputs, so ComfyUI rejects the prompt outright when they are missing.
        inputs.insert("local_prompts".to_string(), json!(""));
        inputs.insert("segment_lengths".to_string(), json!(""));
        inputs.insert("guide_strength".to_string(), json!(""));
        // Canvas: hand the node the same dimensions the native path computes so
        // the aspect/megapixel picker still decides the output size. Without
        // these it would size the canvas off the first timeline image.
        inputs.insert("custom_width".to_string(), json!(width));
        inputs.insert("custom_height".to_string(), json!(height));
        inputs.insert("divisible_by".to_string(), json!(32));
        inputs.insert("resize_method".to_string(), json!("crop"));
        inputs.insert("img_compression".to_string(), json!(0));
        inputs.insert("ref_image_size".to_string(), json!("match"));
        inputs.insert("shift_video".to_string(), json!(12.0));
        inputs.insert("shift_audio".to_string(), json!(3.0));
        inputs.insert("inpaint_audio".to_string(), json!(true));
        inputs.insert("override_audio".to_string(), json!(false));
        inputs.insert(
            "use_custom_motion".to_string(),
            json!(params.video_timeline_custom_motion),
        );
        inputs.insert(
            "use_custom_audio".to_string(),
            json!(params.video_timeline_custom_audio),
        );
        // Unlike the native ref2va node, `ref_images` is one batched IMAGE rather
        // than nine Autogrow slots, so the reference uploads are chained through
        // `ImageBatch`. The node counts `ref_images.shape[0]` and only reads them
        // at all when `reference_mode` is on.
        if is_ref2va {
            let mut batch_id: Option<String> = None;
            for filename in params
                .video_ref_images
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .take(9)
            {
                let load_id = next_id.to_string();
                workflow.insert(
                    load_id.clone(),
                    json!({ "class_type": "LoadImage", "inputs": { "image": filename } }),
                );
                next_id += 1;
                batch_id = Some(match batch_id {
                    None => load_id,
                    Some(previous) => {
                        let id = next_id.to_string();
                        workflow.insert(
                            id.clone(),
                            json!({
                                "class_type": "ImageBatch",
                                "inputs": {
                                    "image1": [previous.as_str(), 0],
                                    "image2": [load_id.as_str(), 0]
                                }
                            }),
                        );
                        next_id += 1;
                        id
                    }
                });
            }
            if let Some(id) = batch_id {
                inputs.insert("ref_images".to_string(), json!([id.as_str(), 0]));
            }
        }
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({ "class_type": "MooshieH3Director", "inputs": inputs }),
        );
        next_id += 1;
        id
    } else if is_ref2va {
        let mut inputs = serde_json::Map::new();
        inputs.insert("clip".to_string(), json!([clip_id.as_str(), 0]));
        inputs.insert("vae".to_string(), json!([vae_id.as_str(), 0]));
        inputs.insert("audio_vae".to_string(), json!([audio_vae_id.as_str(), 0]));
        inputs.insert("prompt".to_string(), json!(params.positive_prompt));
        inputs.insert("width".to_string(), json!(width));
        inputs.insert("height".to_string(), json!(height));
        inputs.insert("length".to_string(), json!(length));
        inputs.insert("ref_image_size".to_string(), json!("match"));
        for (i, filename) in params
            .video_ref_images
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .take(9)
            .enumerate()
        {
            let load_id = next_id.to_string();
            workflow.insert(
                load_id.clone(),
                json!({ "class_type": "LoadImage", "inputs": { "image": filename } }),
            );
            next_id += 1;
            // ComfyUI's Autogrow (COMFY_AUTOGROW_V3) expands the `ref_images` slot into
            // dotted paths: the template names are 0-based (`[f"{prefix}{i}" for i in
            // range(max)]`) and each is prefixed with the Autogrow input's own id, so the
            // first reference image is `ref_images.ref_image_0`. `build_nested_inputs`
            // then splits on `.` to hand the node a single `ref_images` dict. A bare
            // `ref_image_0` (or any 1-based name) is rejected with
            // "execute() got an unexpected keyword argument".
            inputs.insert(
                format!("ref_images.ref_image_{}", i),
                json!([load_id.as_str(), 0]),
            );
        }
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({ "class_type": "MiniMaxH3ReferenceToVideo", "inputs": inputs }),
        );
        next_id += 1;
        id
    } else {
        let mut inputs = serde_json::Map::new();
        inputs.insert("clip".to_string(), json!([clip_id.as_str(), 0]));
        inputs.insert("vae".to_string(), json!([vae_id.as_str(), 0]));
        inputs.insert("prompt".to_string(), json!(params.positive_prompt));
        inputs.insert("width".to_string(), json!(width));
        inputs.insert("height".to_string(), json!(height));
        inputs.insert("length".to_string(), json!(length));
        for (key, frame) in [
            ("first_frame", params.video_first_frame.as_deref()),
            ("last_frame", params.video_last_frame.as_deref()),
        ] {
            let filename = frame.map(str::trim).unwrap_or("");
            if filename.is_empty() {
                continue;
            }
            let load_id = next_id.to_string();
            workflow.insert(
                load_id.clone(),
                json!({ "class_type": "LoadImage", "inputs": { "image": filename } }),
            );
            next_id += 1;
            // The H3 node resizes the frames itself, but asymmetrically: the
            // first frame is the geometry anchor and gets a plain stretch onto
            // the canvas (crop "disabled"), the last frame a cover-crop. The
            // canvas aspect is the requested one rounded to a multiple of 32
            // per axis, so it never matches the source exactly — even the "auto"
            // ratio lands a few percent off — and the anchor comes out visibly
            // squashed. Worse, one image in both slots then enters as two
            // different geometries and the clip morphs between them.
            //
            // Cropping to the canvas here makes the node's own resize a no-op
            // for both slots: same framing in, same framing out.
            let crop_id = next_id.to_string();
            workflow.insert(
                crop_id.clone(),
                json!({
                    "class_type": "ImageScale",
                    "inputs": {
                        "image": [load_id.as_str(), 0],
                        "width": width,
                        "height": height,
                        "upscale_method": "lanczos",
                        "crop": "center"
                    }
                }),
            );
            next_id += 1;
            inputs.insert(key.to_string(), json!([crop_id.as_str(), 0]));
        }
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({ "class_type": "MiniMaxH3ImageToVideo", "inputs": inputs }),
        );
        next_id += 1;
        id
    };

    // The Director re-emits the MODEL it selected on output 0 and shifts
    // conditioning/latent one slot down the list, so every downstream consumer
    // hangs off it. Taking MODEL from the loader instead would bypass the lazy
    // `model`/`model_ref2va` evaluation entirely.
    let uses_director = timeline_data.is_some();
    let model_link = if uses_director {
        json!([h3_id.as_str(), 0])
    } else {
        json!([model_source_id.as_str(), 0])
    };
    let (conditioning_slot, latent_slot) = if uses_director { (1, 2) } else { (0, 1) };

    let noise_id = next_id.to_string();
    workflow.insert(
        noise_id.clone(),
        json!({ "class_type": "RandomNoise", "inputs": { "noise_seed": seed } }),
    );
    next_id += 1;

    // The Turbo pack ships its own SAMPLER: it applies the distilled model's
    // sigma shift (12.0 video / 3.0 audio) internally, so it replaces
    // `KSamplerSelect` outright rather than wrapping it.
    let sampler_select_id = next_id.to_string();
    workflow.insert(
        sampler_select_id.clone(),
        if params.video_turbo_enabled {
            json!({ "class_type": "MiniMaxH3TurboSampler", "inputs": {} })
        } else {
            json!({
                "class_type": "KSamplerSelect",
                "inputs": { "sampler_name": "res_multistep" }
            })
        },
    );
    next_id += 1;

    let steps = if params.video_turbo_enabled {
        params
            .video_turbo_steps
            .clamp(H3_TURBO_MIN_STEPS, H3_TURBO_MAX_STEPS)
    } else {
        H3_DEFAULT_STEPS
    };

    // TeaCache wraps the model in a cached-forward-pass function: it reuses the
    // previous step's output while the accumulated input delta stays under
    // threshold. Inserted after `steps` is known (needed for the "last N
    // steps" guard) and before the scheduler/guider fan `model_link` out to
    // both consumers, so both pick up the wrapped model transparently.
    let model_link = if params.video_teacache_enabled {
        let teacache_id = next_id.to_string();
        workflow.insert(
            teacache_id.clone(),
            json!({
                "class_type": "MiniMaxH3TeaCache",
                "inputs": {
                    "model": model_link,
                    "rel_l1_thresh": 0.15,
                    "start_step": 2,
                    "end_step": -2,
                    "total_steps": steps
                }
            }),
        );
        next_id += 1;
        json!([teacache_id.as_str(), 0])
    } else {
        model_link
    };

    let scheduler_id = next_id.to_string();
    workflow.insert(
        scheduler_id.clone(),
        json!({
            "class_type": "BasicScheduler",
            "inputs": {
                "model": model_link.clone(),
                "scheduler": "simple",
                "steps": steps,
                "denoise": 1.0
            }
        }),
    );
    next_id += 1;

    let guider_id = next_id.to_string();
    workflow.insert(
        guider_id.clone(),
        json!({
            "class_type": "BasicGuider",
            "inputs": {
                "model": model_link,
                "conditioning": [h3_id.as_str(), conditioning_slot]
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
                "noise": [noise_id.as_str(), 0],
                "guider": [guider_id.as_str(), 0],
                "sampler": [sampler_select_id.as_str(), 0],
                "sigmas": [scheduler_id.as_str(), 0],
                "latent_image": [h3_id.as_str(), latent_slot]
            }
        }),
    );
    next_id += 1;

    let decode_id = next_id.to_string();
    workflow.insert(
        decode_id.clone(),
        json!({
            "class_type": "VAEDecode",
            "inputs": {
                "samples": [sampler_id.as_str(), 0],
                "vae": [vae_id.as_str(), 0]
            }
        }),
    );
    next_id += 1;

    // The Director's `combined_audio` (output 3) is the timeline's own audio bed.
    // With no audio segments it is digital silence, not a passthrough, so it may
    // only replace H3's jointly-generated stereo when the timeline actually
    // carries audio. Otherwise the native decode stays in the graph.
    let audio_link = if uses_director && params.video_timeline_custom_audio {
        json!([h3_id.as_str(), 3])
    } else {
        let audio_decode_id = next_id.to_string();
        workflow.insert(
            audio_decode_id.clone(),
            json!({
                "class_type": "VAEDecodeAudio",
                "inputs": {
                    "samples": [sampler_id.as_str(), 0],
                    "vae": [audio_vae_id.as_str(), 0]
                }
            }),
        );
        next_id += 1;
        json!([audio_decode_id.as_str(), 0])
    };

    // Frame interpolation emits `(N - 1) * multiplier + 1` frames, so
    // `CreateVideo.fps` has to rise by the same factor or the clip would play
    // back in slow motion instead of just looking smoother.
    let (frames_source_id, fps) = if params.video_rife_enabled {
        let settings = crate::templates::rife::RifeSettings::from_params(params);
        let rife_id = next_id.to_string();
        workflow.insert(
            rife_id.clone(),
            settings.node(json!([decode_id.as_str(), 0])),
        );
        next_id += 1;
        (rife_id, settings.output_fps(24.0))
    } else {
        (decode_id.clone(), 24.0)
    };

    let create_video_id = next_id.to_string();
    workflow.insert(
        create_video_id.clone(),
        json!({
            "class_type": "CreateVideo",
            "inputs": {
                "images": [frames_source_id.as_str(), 0],
                "audio": audio_link,
                "fps": fps
            }
        }),
    );
    next_id += 1;

    let save_id = next_id.to_string();
    let mut save_inputs = serde_json::Map::new();
    save_inputs.insert("video".to_string(), json!([create_video_id.as_str(), 0]));
    save_inputs.insert("filename_prefix".to_string(), json!("mooshie_video"));
    if include_metadata {
        save_inputs.insert(
            "metadata_json".to_string(),
            json!(video_metadata_json(params, seed)),
        );
    }
    workflow.insert(
        save_id,
        json!({
            "class_type": "MooshieSaveVideo",
            "inputs": Value::Object(save_inputs)
        }),
    );

    Value::Object(workflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::comfyui::types::GenerationParams;

    /// Minimal deserialization-based constructor — `GenerationParams` has
    /// required fields and no `Default`, so tests build it from JSON. The
    /// diffusion filename is derived from the variant so validation's
    /// cross-variant guard (Task 3) accepts it.
    fn video_params(variant: &str) -> GenerationParams {
        serde_json::from_value(json!({
            "mode": "video",
            "positive_prompt": "a red fox running through snow",
            "negative_prompt": "",
            "checkpoint": "",
            "loras": [],
            "sampler_name": "euler",
            "scheduler": "normal",
            "steps": 28,
            "cfg": 1.0,
            "seed": "42",
            "width": 0,
            "height": 0,
            "batch_size": 1,
            "denoise": 1.0,
            "upscale_enabled": false,
            "upscale_method": "latent",
            "upscale_scale": 1.0,
            "upscale_denoise": 0.5,
            "upscale_steps": 10,
            "upscale_tile_size": 1024,
            "upscale_tiling": false,
            "video_variant": variant,
            "video_duration_seconds": 5.0,
            "video_megapixels": 0.4,
            "video_aspect_ratio": "16:9",
            "video_diffusion_model": format!("minimax_h3_{variant}_nvfp4.safetensors"),
            "video_clip_model": "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
            "video_vae_model": "minimax_h3_video_vae_fp16.safetensors",
            "video_audio_vae_model": "minimax_h3_audio_vae_fp32.safetensors"
        }))
        .expect("valid test params")
    }

    fn nodes_of_class<'a>(workflow: &'a Value, class_type: &str) -> Vec<&'a Value> {
        workflow
            .as_object()
            .expect("workflow is an object")
            .values()
            .filter(|node| node["class_type"] == class_type)
            .collect()
    }

    #[test]
    fn fl2va_graph_has_expected_shape() {
        let workflow = build(&video_params("fl2va"), 42, false);
        assert_eq!(nodes_of_class(&workflow, "MiniMaxH3ImageToVideo").len(), 1);
        assert!(nodes_of_class(&workflow, "MiniMaxH3ReferenceToVideo").is_empty());
        assert!(nodes_of_class(&workflow, "LoadImage").is_empty());
        assert_eq!(nodes_of_class(&workflow, "UNETLoader").len(), 1);
        assert_eq!(nodes_of_class(&workflow, "VAELoader").len(), 2);
        assert_eq!(nodes_of_class(&workflow, "VAEDecode").len(), 1);
        assert_eq!(nodes_of_class(&workflow, "VAEDecodeAudio").len(), 1);
        assert_eq!(nodes_of_class(&workflow, "MooshieSaveVideo").len(), 1);
        assert!(nodes_of_class(&workflow, "MooshieSaveImage").is_empty());

        let h3 = nodes_of_class(&workflow, "MiniMaxH3ImageToVideo")[0];
        assert_eq!(
            h3["inputs"]["prompt"],
            json!("a red fox running through snow")
        );
        assert_eq!(h3["inputs"]["width"], json!(832));
        assert_eq!(h3["inputs"]["height"], json!(480));
        assert_eq!(h3["inputs"]["length"], json!(124));
        assert!(h3["inputs"].get("first_frame").is_none());
        assert!(h3["inputs"].get("last_frame").is_none());

        let clip = nodes_of_class(&workflow, "CLIPLoader")[0];
        assert_eq!(clip["inputs"]["type"], json!("minimax"));

        let noise = nodes_of_class(&workflow, "RandomNoise")[0];
        assert_eq!(noise["inputs"]["noise_seed"], json!(42));

        let sampler_select = nodes_of_class(&workflow, "KSamplerSelect")[0];
        assert_eq!(
            sampler_select["inputs"]["sampler_name"],
            json!("res_multistep")
        );

        let scheduler = nodes_of_class(&workflow, "BasicScheduler")[0];
        assert_eq!(scheduler["inputs"]["scheduler"], json!("simple"));
        assert_eq!(scheduler["inputs"]["steps"], json!(20));
        assert_eq!(scheduler["inputs"]["denoise"], json!(1.0));

        let sampler = nodes_of_class(&workflow, "SamplerCustomAdvanced")[0];
        assert_eq!(sampler["inputs"]["latent_image"][1], json!(1));

        let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
        assert_eq!(create_video["inputs"]["fps"], json!(24.0));
        assert!(create_video["inputs"]["audio"].is_array());

        let save = nodes_of_class(&workflow, "MooshieSaveVideo")[0];
        assert_eq!(save["inputs"]["filename_prefix"], json!("mooshie_video"));
    }

    #[test]
    fn fl2va_wires_first_and_last_frames() {
        let mut params = video_params("fl2va");
        params.video_first_frame = Some("first.png".to_string());
        params.video_last_frame = Some("last.png".to_string());
        let workflow = build(&params, 1, false);
        assert_eq!(nodes_of_class(&workflow, "LoadImage").len(), 2);
        let h3 = nodes_of_class(&workflow, "MiniMaxH3ImageToVideo")[0];
        assert!(h3["inputs"]["first_frame"].is_array());
        assert!(h3["inputs"]["last_frame"].is_array());
    }

    /// `MiniMaxH3ImageToVideo` resizes `first_frame` with crop "disabled" — a
    /// plain stretch onto a canvas whose aspect is only ever the requested one
    /// rounded to a multiple of 32 — while `last_frame` gets a cover-crop. Both
    /// frames have to arrive already at the canvas size so neither is distorted
    /// and the same image in both slots yields the same geometry.
    #[test]
    fn fl2va_crops_frames_to_the_canvas_before_the_h3_node() {
        let mut params = video_params("fl2va");
        params.video_first_frame = Some("first.png".to_string());
        params.video_last_frame = Some("last.png".to_string());
        let workflow = build(&params, 1, false);

        let scales = nodes_of_class(&workflow, "ImageScale");
        assert_eq!(scales.len(), 2, "one crop per supplied frame");
        for scale in &scales {
            assert_eq!(scale["inputs"]["width"], json!(832));
            assert_eq!(scale["inputs"]["height"], json!(480));
            assert_eq!(scale["inputs"]["crop"], json!("center"));
            assert!(scale["inputs"]["image"].is_array());
        }

        // The H3 node must read the crops, not the raw LoadImage outputs.
        let load_ids: Vec<String> = workflow
            .as_object()
            .expect("workflow is an object")
            .iter()
            .filter(|(_, node)| node["class_type"] == "LoadImage")
            .map(|(id, _)| id.clone())
            .collect();
        let h3 = nodes_of_class(&workflow, "MiniMaxH3ImageToVideo")[0];
        for key in ["first_frame", "last_frame"] {
            let source = h3["inputs"][key][0]
                .as_str()
                .expect("frame input is a link")
                .to_string();
            assert!(
                !load_ids.contains(&source),
                "{key} is wired straight to LoadImage"
            );
            assert_eq!(workflow[&source]["class_type"], json!("ImageScale"));
        }
    }

    #[test]
    fn fl2va_ignores_blank_frame_entries() {
        let mut params = video_params("fl2va");
        params.video_first_frame = Some("  ".to_string());
        let workflow = build(&params, 1, false);
        assert!(nodes_of_class(&workflow, "LoadImage").is_empty());
        let h3 = nodes_of_class(&workflow, "MiniMaxH3ImageToVideo")[0];
        assert!(h3["inputs"].get("first_frame").is_none());
    }

    #[test]
    fn ref2va_wires_reference_images_individually() {
        let mut params = video_params("ref2va");
        params.video_ref_images = vec![
            "a.png".to_string(),
            "b.png".to_string(),
            "".to_string(),
            "c.png".to_string(),
        ];
        let workflow = build(&params, 1, false);
        assert!(nodes_of_class(&workflow, "MiniMaxH3ImageToVideo").is_empty());
        let h3 = nodes_of_class(&workflow, "MiniMaxH3ReferenceToVideo")[0];
        assert!(h3["inputs"]["audio_vae"].is_array());
        assert_eq!(h3["inputs"]["ref_image_size"], json!("match"));
        assert!(h3["inputs"]["ref_images.ref_image_0"].is_array());
        assert!(h3["inputs"]["ref_images.ref_image_1"].is_array());
        assert!(h3["inputs"]["ref_images.ref_image_2"].is_array());
        assert!(h3["inputs"].get("ref_images.ref_image_3").is_none());
        assert_eq!(nodes_of_class(&workflow, "LoadImage").len(), 3);
    }

    /// Workflow key of the (single) node with `class_type` — needed to assert
    /// on wiring, since ids are positional and shift when a node is spliced in.
    fn node_id_of_class(workflow: &Value, class_type: &str) -> String {
        workflow
            .as_object()
            .expect("workflow is an object")
            .iter()
            .find(|(_, node)| node["class_type"] == class_type)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| panic!("no {class_type} node in workflow"))
    }

    #[test]
    fn rife_disabled_wires_decode_straight_to_create_video() {
        let workflow = build(&video_params("fl2va"), 1, false);
        assert!(nodes_of_class(&workflow, "RIFE VFI").is_empty());

        let decode_id = node_id_of_class(&workflow, "VAEDecode");
        let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
        assert_eq!(create_video["inputs"]["images"], json!([decode_id, 0]));
        assert_eq!(create_video["inputs"]["fps"], json!(24.0));
    }

    #[test]
    fn rife_enabled_splices_interpolation_and_doubles_fps() {
        let mut params = video_params("fl2va");
        params.video_rife_enabled = true;
        let workflow = build(&params, 1, false);

        assert_eq!(nodes_of_class(&workflow, "RIFE VFI").len(), 1);
        let rife = nodes_of_class(&workflow, "RIFE VFI")[0];
        let decode_id = node_id_of_class(&workflow, "VAEDecode");
        assert_eq!(rife["inputs"]["frames"], json!([decode_id, 0]));
        assert_eq!(
            rife["inputs"]["ckpt_name"],
            json!(crate::comfyui::nodes::RIFE_CKPT_FILENAME)
        );
        assert_eq!(rife["inputs"]["multiplier"], json!(2));

        // ComfyUI rejects a prompt outright when a required widget is absent,
        // so every input the node declares has to be sent.
        for key in [
            "frames",
            "ckpt_name",
            "clear_cache_after_n_frames",
            "multiplier",
            "fast_mode",
            "ensemble",
            "scale_factor",
            "dtype",
            "torch_compile",
            "batch_size",
        ] {
            assert!(rife["inputs"].get(key).is_some(), "missing widget {key}");
        }

        let rife_id = node_id_of_class(&workflow, "RIFE VFI");
        let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
        assert_eq!(create_video["inputs"]["images"], json!([rife_id, 0]));
        assert_eq!(create_video["inputs"]["fps"], json!(48.0));
        // Interpolation touches frames only; audio still comes from the
        // audio VAE at its own rate.
        assert!(create_video["inputs"]["audio"].is_array());
        assert_eq!(nodes_of_class(&workflow, "MooshieSaveVideo").len(), 1);
    }

    #[test]
    fn rife_multiplier_drives_both_the_node_and_the_output_fps() {
        for (multiplier, expected_fps) in [(3u32, 72.0), (4u32, 96.0)] {
            let mut params = video_params("fl2va");
            params.video_rife_enabled = true;
            params.video_rife_multiplier = multiplier;
            let workflow = build(&params, 1, false);

            let rife = nodes_of_class(&workflow, "RIFE VFI")[0];
            assert_eq!(rife["inputs"]["multiplier"], json!(multiplier));
            let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
            assert_eq!(create_video["inputs"]["fps"], json!(expected_fps));
        }
    }

    #[test]
    fn rife_advanced_widgets_reach_the_node() {
        let mut params = video_params("fl2va");
        params.video_rife_enabled = true;
        params.video_rife_scale_factor = 0.5;
        params.video_rife_fast_mode = false;
        params.video_rife_ensemble = false;
        let workflow = build(&params, 1, false);

        let rife = nodes_of_class(&workflow, "RIFE VFI")[0];
        assert_eq!(rife["inputs"]["scale_factor"], json!(0.5));
        assert_eq!(rife["inputs"]["fast_mode"], json!(false));
        assert_eq!(rife["inputs"]["ensemble"], json!(false));
    }

    #[test]
    fn rife_multiplier_from_an_untrusted_client_is_clamped() {
        let mut params = video_params("fl2va");
        params.video_rife_enabled = true;
        params.video_rife_multiplier = 32;
        let workflow = build(&params, 1, false);

        let rife = nodes_of_class(&workflow, "RIFE VFI")[0];
        assert_eq!(rife["inputs"]["multiplier"], json!(4));
        let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
        assert_eq!(create_video["inputs"]["fps"], json!(96.0));
    }

    #[test]
    fn turbo_disabled_leaves_the_base_sampling_path_alone() {
        let workflow = build(&video_params("fl2va"), 1, false);
        assert!(nodes_of_class(&workflow, "MiniMaxH3TurboLoRA").is_empty());
        assert!(nodes_of_class(&workflow, "MiniMaxH3TurboSampler").is_empty());

        let unet_id = node_id_of_class(&workflow, "UNETLoader");
        let scheduler = nodes_of_class(&workflow, "BasicScheduler")[0];
        let guider = nodes_of_class(&workflow, "BasicGuider")[0];
        assert_eq!(scheduler["inputs"]["model"], json!([unet_id, 0]));
        assert_eq!(guider["inputs"]["model"], json!([unet_id, 0]));
        assert_eq!(scheduler["inputs"]["steps"], json!(H3_DEFAULT_STEPS));
    }

    #[test]
    fn turbo_enabled_inserts_the_lora_and_swaps_the_sampler() {
        let mut params = video_params("fl2va");
        params.video_turbo_enabled = true;
        params.video_turbo_steps = 6;
        let workflow = build(&params, 1, false);

        assert_eq!(nodes_of_class(&workflow, "MiniMaxH3TurboLoRA").len(), 1);
        assert_eq!(nodes_of_class(&workflow, "MiniMaxH3TurboSampler").len(), 1);
        // The turbo sampler applies the distilled sigma shift itself, so the
        // stock sampler node is replaced rather than wrapped.
        assert!(nodes_of_class(&workflow, "KSamplerSelect").is_empty());

        let unet_id = node_id_of_class(&workflow, "UNETLoader");
        let lora = nodes_of_class(&workflow, "MiniMaxH3TurboLoRA")[0];
        assert_eq!(lora["inputs"]["model"], json!([unet_id, 0]));
        assert_eq!(
            lora["inputs"]["lora_name"],
            json!(crate::comfyui::nodes::H3_TURBO_LORA_FILENAME)
        );
        assert_eq!(lora["inputs"]["strength"], json!(1.0));
        assert_eq!(lora["inputs"]["low_vram"], json!(false));

        // Both MODEL consumers must move to the adapter: scheduling sigmas for
        // the base model while guiding the distilled one produces noise.
        let lora_id = node_id_of_class(&workflow, "MiniMaxH3TurboLoRA");
        let scheduler = nodes_of_class(&workflow, "BasicScheduler")[0];
        let guider = nodes_of_class(&workflow, "BasicGuider")[0];
        assert_eq!(scheduler["inputs"]["model"], json!([lora_id, 0]));
        assert_eq!(guider["inputs"]["model"], json!([lora_id, 0]));
        assert_eq!(scheduler["inputs"]["steps"], json!(6));

        let sampler_id = node_id_of_class(&workflow, "MiniMaxH3TurboSampler");
        let sampler = nodes_of_class(&workflow, "SamplerCustomAdvanced")[0];
        assert_eq!(sampler["inputs"]["sampler"], json!([sampler_id, 0]));
    }

    #[test]
    fn turbo_steps_are_clamped_to_the_distilled_range() {
        for (requested, expected) in [(0, H3_TURBO_MIN_STEPS), (99, H3_TURBO_MAX_STEPS), (4, 4)] {
            let mut params = video_params("fl2va");
            params.video_turbo_enabled = true;
            params.video_turbo_steps = requested;
            let workflow = build(&params, 1, false);
            let scheduler = nodes_of_class(&workflow, "BasicScheduler")[0];
            assert_eq!(
                scheduler["inputs"]["steps"],
                json!(expected),
                "steps {requested} should clamp to {expected}"
            );
        }
    }

    #[test]
    fn turbo_honours_an_explicit_adapter_filename() {
        let mut params = video_params("fl2va");
        params.video_turbo_enabled = true;
        params.video_turbo_lora = Some("  minimax_h3_turbo_4step_ema.safetensors  ".to_string());
        let workflow = build(&params, 1, false);
        let lora = nodes_of_class(&workflow, "MiniMaxH3TurboLoRA")[0];
        assert_eq!(
            lora["inputs"]["lora_name"],
            json!("minimax_h3_turbo_4step_ema.safetensors")
        );

        // A blank override is a cleared dropdown, not a request for a nameless
        // adapter — it falls back to the recommended checkpoint.
        params.video_turbo_lora = Some("   ".to_string());
        let workflow = build(&params, 1, false);
        let lora = nodes_of_class(&workflow, "MiniMaxH3TurboLoRA")[0];
        assert_eq!(
            lora["inputs"]["lora_name"],
            json!(crate::comfyui::nodes::H3_TURBO_LORA_FILENAME)
        );
    }

    #[test]
    fn turbo_composes_with_rife_interpolation() {
        let mut params = video_params("fl2va");
        params.video_turbo_enabled = true;
        params.video_rife_enabled = true;
        let workflow = build(&params, 1, false);

        assert_eq!(nodes_of_class(&workflow, "MiniMaxH3TurboLoRA").len(), 1);
        let rife_id = node_id_of_class(&workflow, "RIFE VFI");
        let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
        assert_eq!(create_video["inputs"]["images"], json!([rife_id, 0]));
        assert_eq!(create_video["inputs"]["fps"], json!(48.0));
    }

    /// Minimal compiled timeline. `build` only checks that the string is
    /// non-blank and passes it through verbatim — the node parses it, not us.
    const TIMELINE_JSON: &str =
        r#"{"global_prompt":"a red fox","segments":[],"reference_mode":"OFF"}"#;

    fn director_params(variant: &str) -> GenerationParams {
        let mut params = video_params(variant);
        params.video_timeline_data = Some(TIMELINE_JSON.to_string());
        params
    }

    #[test]
    fn timeline_routes_the_graph_through_the_director() {
        let workflow = build(&director_params("fl2va"), 1, false);

        assert_eq!(nodes_of_class(&workflow, "MooshieH3Director").len(), 1);
        assert!(nodes_of_class(&workflow, "MiniMaxH3ImageToVideo").is_empty());
        assert!(nodes_of_class(&workflow, "MiniMaxH3ReferenceToVideo").is_empty());

        let director_id = node_id_of_class(&workflow, "MooshieH3Director");
        let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
        assert_eq!(director["inputs"]["timeline_data"], json!(TIMELINE_JSON));
        // The canvas still comes from the aspect/megapixel picker, not from the
        // first timeline image.
        assert_eq!(director["inputs"]["custom_width"], json!(832));
        assert_eq!(director["inputs"]["custom_height"], json!(480));
        assert_eq!(director["inputs"]["divisible_by"], json!(32));

        // Every non-optional input has to be present or ComfyUI rejects the
        // prompt during validation, including the three the node declares but
        // never reads.
        for key in [
            "clip",
            "vae",
            "audio_vae",
            "start_second",
            "end_second",
            "duration_seconds",
            "start_frame",
            "end_frame",
            "duration_frames",
            "frame_rate",
            "display_mode",
            "local_prompts",
            "segment_lengths",
            "guide_strength",
            "resize_method",
            "img_compression",
            "ref_image_size",
            "shift_video",
            "shift_audio",
            "inpaint_audio",
            "override_audio",
            "use_custom_motion",
            "use_custom_audio",
        ] {
            assert!(director["inputs"].get(key).is_some(), "missing input {key}");
        }
        // `global_prompt` is `force_input`, so a widget value would be rejected;
        // the node reads it out of `timeline_data` instead.
        assert!(director["inputs"].get("global_prompt").is_none());

        // MODEL comes off the Director's own output 0, and conditioning/latent
        // sit one slot further down than on the native nodes.
        let scheduler = nodes_of_class(&workflow, "BasicScheduler")[0];
        let guider = nodes_of_class(&workflow, "BasicGuider")[0];
        let sampler = nodes_of_class(&workflow, "SamplerCustomAdvanced")[0];
        assert_eq!(scheduler["inputs"]["model"], json!([director_id, 0]));
        assert_eq!(guider["inputs"]["model"], json!([director_id, 0]));
        assert_eq!(guider["inputs"]["conditioning"], json!([director_id, 1]));
        assert_eq!(sampler["inputs"]["latent_image"], json!([director_id, 2]));
    }

    #[test]
    fn a_blank_timeline_keeps_the_native_path() {
        for timeline in [None, Some(String::new()), Some("   ".to_string())] {
            let mut params = video_params("fl2va");
            params.video_timeline_data = timeline.clone();
            let workflow = build(&params, 1, false);
            assert!(
                nodes_of_class(&workflow, "MooshieH3Director").is_empty(),
                "director spliced in for {timeline:?}"
            );
            assert_eq!(nodes_of_class(&workflow, "MiniMaxH3ImageToVideo").len(), 1);
        }
    }

    #[test]
    fn director_wires_exactly_one_model_socket_per_variant() {
        // Both MODEL inputs are lazy and only the one matching `reference_mode`
        // is evaluated. Wiring both would load ~42 GB of DiT twice.
        let workflow = build(&director_params("fl2va"), 1, false);
        let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
        assert!(director["inputs"]["model"].is_array());
        assert!(director["inputs"].get("model_ref2va").is_none());

        let mut params = director_params("ref2va");
        params.video_ref_images = vec!["a.png".to_string()];
        let workflow = build(&params, 1, false);
        let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
        assert!(director["inputs"]["model_ref2va"].is_array());
        assert!(director["inputs"].get("model").is_none());
    }

    #[test]
    fn director_batches_reference_images_into_one_input() {
        // `ref_images` on the Director is a single batched IMAGE, not the native
        // node's nine Autogrow slots, so the uploads chain through `ImageBatch`.
        let mut params = director_params("ref2va");
        params.video_ref_images = vec![
            "a.png".to_string(),
            "b.png".to_string(),
            "  ".to_string(),
            "c.png".to_string(),
        ];
        let workflow = build(&params, 1, false);

        assert_eq!(nodes_of_class(&workflow, "LoadImage").len(), 3);
        assert_eq!(nodes_of_class(&workflow, "ImageBatch").len(), 2);
        let batch_id = {
            let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
            director["inputs"]["ref_images"][0]
                .as_str()
                .expect("ref_images points at a node")
                .to_string()
        };
        // The Director reads the LAST batch in the chain, which is the only one
        // nothing else consumes.
        let last_batch = &workflow[&batch_id];
        assert_eq!(last_batch["class_type"], json!("ImageBatch"));

        // One image needs no batching at all.
        let mut params = director_params("ref2va");
        params.video_ref_images = vec!["only.png".to_string()];
        let workflow = build(&params, 1, false);
        assert!(nodes_of_class(&workflow, "ImageBatch").is_empty());
        let load_id = node_id_of_class(&workflow, "LoadImage");
        let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
        assert_eq!(director["inputs"]["ref_images"], json!([load_id, 0]));

        // fl2va never reads references, so it never uploads them either.
        let workflow = build(&director_params("fl2va"), 1, false);
        assert!(nodes_of_class(&workflow, "LoadImage").is_empty());
        let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
        assert!(director["inputs"].get("ref_images").is_none());
    }

    #[test]
    fn director_audio_replaces_the_decode_only_when_the_timeline_has_audio() {
        // With no audio segments the Director's `combined_audio` is digital
        // silence rather than a passthrough, so the native decode has to stay.
        let workflow = build(&director_params("fl2va"), 1, false);
        let decode_id = node_id_of_class(&workflow, "VAEDecodeAudio");
        let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
        assert_eq!(create_video["inputs"]["audio"], json!([decode_id, 0]));

        let mut params = director_params("fl2va");
        params.video_timeline_custom_audio = true;
        let workflow = build(&params, 1, false);
        assert!(nodes_of_class(&workflow, "VAEDecodeAudio").is_empty());
        let director_id = node_id_of_class(&workflow, "MooshieH3Director");
        let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
        assert_eq!(create_video["inputs"]["audio"], json!([director_id, 3]));
    }

    #[test]
    fn director_motion_and_audio_widgets_track_the_compiled_timeline() {
        let mut params = director_params("fl2va");
        params.video_timeline_custom_motion = true;
        params.video_timeline_custom_audio = true;
        let workflow = build(&params, 1, false);
        let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
        assert_eq!(director["inputs"]["use_custom_motion"], json!(true));
        assert_eq!(director["inputs"]["use_custom_audio"], json!(true));

        let workflow = build(&director_params("fl2va"), 1, false);
        let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
        assert_eq!(director["inputs"]["use_custom_motion"], json!(false));
        assert_eq!(director["inputs"]["use_custom_audio"], json!(false));
    }

    #[test]
    fn director_window_resolves_to_the_native_frame_length() {
        // `resolve_window` reads `start_frame`/`duration_frames` only, then the
        // planner snaps the count onto the 17n+5 grid. The result has to match
        // what the native path would have asked for.
        fn align(mut frames: i64) -> i64 {
            while frames % 17 != 5 {
                frames += 1;
            }
            frames
        }
        for tenths in 10..=150 {
            let seconds = tenths as f64 / 10.0;
            let mut params = director_params("fl2va");
            params.video_duration_seconds = seconds;
            let workflow = build(&params, 1, false);
            let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
            assert_eq!(director["inputs"]["start_frame"], json!(0));
            assert_eq!(director["inputs"]["frame_rate"], json!(24.0));
            let frames = director["inputs"]["duration_frames"]
                .as_i64()
                .expect("duration_frames is an integer");
            assert_eq!(
                align(frames),
                i64::from(compute_h3_frame_length(seconds)),
                "director window disagrees with the native length at {seconds} s"
            );
        }
    }

    #[test]
    fn director_still_composes_with_turbo_and_rife() {
        let mut params = director_params("fl2va");
        params.video_turbo_enabled = true;
        params.video_rife_enabled = true;
        let workflow = build(&params, 1, false);

        // The adapter feeds the Director, and the Director still owns MODEL
        // downstream — taking it from the LoRA would skip the lazy evaluation.
        let lora_id = node_id_of_class(&workflow, "MiniMaxH3TurboLoRA");
        let director_id = node_id_of_class(&workflow, "MooshieH3Director");
        let director = nodes_of_class(&workflow, "MooshieH3Director")[0];
        assert_eq!(director["inputs"]["model"], json!([lora_id, 0]));
        let scheduler = nodes_of_class(&workflow, "BasicScheduler")[0];
        assert_eq!(scheduler["inputs"]["model"], json!([director_id, 0]));

        let rife_id = node_id_of_class(&workflow, "RIFE VFI");
        let create_video = nodes_of_class(&workflow, "CreateVideo")[0];
        assert_eq!(create_video["inputs"]["images"], json!([rife_id, 0]));
        assert_eq!(create_video["inputs"]["fps"], json!(48.0));
    }

    #[test]
    fn frame_length_snaps_up_to_17n_plus_5() {
        assert_eq!(compute_h3_frame_length(5.0), 124);
        assert_eq!(compute_h3_frame_length(1.0), 39);
        assert_eq!(compute_h3_frame_length(15.0), 362);
        // Robustness at the edges: never below the widget minimum,
        // never above the largest on-grid value <= 3600.
        assert_eq!(compute_h3_frame_length(0.0), 5);
        assert_eq!(compute_h3_frame_length(500.0), 3592);
    }

    #[test]
    fn frame_length_is_always_on_grid() {
        for tenths in 10..=150 {
            let length = compute_h3_frame_length(tenths as f64 / 10.0);
            assert_eq!(
                length % 17,
                5,
                "off-grid length {} for {} s",
                length,
                tenths as f64 / 10.0
            );
        }
    }

    #[test]
    fn dimensions_hit_known_targets() {
        assert_eq!(compute_h3_dimensions("16:9", 0.4), (832, 480));
        assert_eq!(compute_h3_dimensions("9:16", 0.4), (480, 832));
        assert_eq!(compute_h3_dimensions("1:1", 0.4), (640, 640));
        assert_eq!(compute_h3_dimensions("4:3", 0.4), (736, 544));
        assert_eq!(compute_h3_dimensions("3:4", 0.4), (544, 736));
        assert_eq!(compute_h3_dimensions("16:9", 0.6), (1024, 576));
        // Unknown ratios fall back to 16:9.
        assert_eq!(compute_h3_dimensions("bogus", 0.4), (832, 480));
    }

    #[test]
    fn dimensions_accept_literal_pixel_ratios() {
        // "Match image" sends the frame's own dimensions instead of a preset;
        // an exactly-16:9 image must land on the same result as the preset.
        assert_eq!(
            compute_h3_dimensions("1920:1080", 0.4),
            compute_h3_dimensions("16:9", 0.4)
        );
        assert_eq!(compute_h3_dimensions("1000:1000", 0.4), (640, 640));
        // Degenerate pairs fall back rather than dividing by zero.
        assert_eq!(compute_h3_dimensions("1920:0", 0.4), (832, 480));
        assert_eq!(compute_h3_dimensions("-16:9", 0.4), (832, 480));
        assert_eq!(compute_h3_dimensions("16:9:1", 0.4), (832, 480));
    }

    #[test]
    fn dimensions_are_multiples_of_32() {
        for ratio in ["16:9", "9:16", "1:1", "4:3", "3:4"] {
            for mp in [0.2, 0.4, 0.6, 1.0] {
                let (w, h) = compute_h3_dimensions(ratio, mp);
                assert_eq!(w % 32, 0);
                assert_eq!(h % 32, 0);
                assert!(w >= 64 && h >= 64);
            }
        }
    }

    #[test]
    fn build_workflow_dispatches_video_without_finish_workflow() {
        let workflow = crate::templates::build_workflow(&video_params("fl2va"), 42, true);
        assert_eq!(nodes_of_class(&workflow, "MooshieSaveVideo").len(), 1);
        assert!(nodes_of_class(&workflow, "MooshieSaveImage").is_empty());
    }

    #[test]
    fn validate_accepts_valid_video_params() {
        assert!(crate::templates::validate_generation_params(&video_params("fl2va")).is_ok());
    }

    #[test]
    fn validate_rejects_unknown_variant() {
        let mut params = video_params("fl2va");
        params.video_variant = "t2v".to_string();
        assert!(crate::templates::validate_generation_params(&params).is_err());
    }

    #[test]
    fn validate_rejects_missing_model_files() {
        let mut params = video_params("fl2va");
        params.video_clip_model = None;
        let err = crate::templates::validate_generation_params(&params).unwrap_err();
        assert!(err.contains("text encoder"), "unexpected error: {err}");
    }

    #[test]
    fn validate_rejects_non_h3_diffusion_model() {
        let mut params = video_params("fl2va");
        params.video_diffusion_model = Some("wan2.2_t2v_fp8.safetensors".to_string());
        assert!(crate::templates::validate_generation_params(&params).is_err());
    }

    #[test]
    fn validate_rejects_cross_variant_diffusion_model() {
        let mut params = video_params("fl2va");
        params.video_diffusion_model = Some("minimax_h3_ref2va_nvfp4.safetensors".to_string());
        assert!(crate::templates::validate_generation_params(&params).is_err());
    }

    #[test]
    fn validate_rejects_out_of_range_duration() {
        let mut params = video_params("fl2va");
        params.video_duration_seconds = 0.5;
        assert!(crate::templates::validate_generation_params(&params).is_err());
        params.video_duration_seconds = 16.0;
        assert!(crate::templates::validate_generation_params(&params).is_err());
    }

    #[test]
    fn validate_rejects_ref2va_without_references() {
        let params = video_params("ref2va");
        let err = crate::templates::validate_generation_params(&params).unwrap_err();
        assert!(err.contains("reference image"), "unexpected error: {err}");
    }

    #[test]
    fn validate_accepts_ref2va_without_references_when_timeline_drives() {
        // The Director derives its reference set from the timeline's shot
        // stills and cast photos, so the settings panel's slots are optional.
        let params = director_params("ref2va");
        assert!(crate::templates::validate_generation_params(&params).is_ok());
    }

    #[test]
    fn validate_still_caps_ref2va_references_with_a_timeline() {
        let mut params = director_params("ref2va");
        params.video_ref_images = (0..10).map(|i| format!("ref{i}.png")).collect();
        let err = crate::templates::validate_generation_params(&params).unwrap_err();
        assert!(err.contains("at most 9"), "unexpected error: {err}");
    }

    #[test]
    fn validate_video_ignores_stale_image_mode_flags() {
        // Mode switches can leave image-only toggles set; they must not
        // block video generation (the video arm early-returns).
        let mut params = video_params("fl2va");
        params.style_transfer_enabled = true;
        assert!(crate::templates::validate_generation_params(&params).is_ok());
    }

    #[test]
    fn metadata_json_round_trips_through_the_swarmui_parser() {
        let params = video_params("fl2va");
        let json = video_metadata_json(&params, 42);
        let read = crate::metadata::parse_swarmui_json(&json).expect("parses");

        assert_eq!(
            read.get("positive_prompt").map(String::as_str),
            Some("a red fox running through snow")
        );
        assert_eq!(read.get("seed").map(String::as_str), Some("42"));
        assert_eq!(read.get("mode").map(String::as_str), Some("video"));
    }

    #[test]
    fn save_node_carries_metadata_when_the_node_supports_it() {
        let workflow = build(&video_params("fl2va"), 42, true);
        let save = nodes_of_class(&workflow, "MooshieSaveVideo");
        let sent = save[0]["inputs"]["metadata_json"]
            .as_str()
            .expect("metadata_json is a string");
        assert!(sent.contains("sui_image_params"));
    }

    #[test]
    fn save_node_omits_metadata_when_the_node_is_too_old() {
        let workflow = build(&video_params("fl2va"), 42, false);
        let save = nodes_of_class(&workflow, "MooshieSaveVideo");
        assert!(save[0]["inputs"].get("metadata_json").is_none());
    }

    #[test]
    fn metadata_records_the_resolved_dimensions_and_steps() {
        let params = video_params("fl2va");
        let flat = video_metadata_params(&params, 7);
        let (w, h) = compute_h3_dimensions(&params.video_aspect_ratio, params.video_megapixels);

        assert_eq!(
            flat.get("size").map(String::as_str),
            Some(&format!("{w}x{h}")[..])
        );
        assert_eq!(
            flat.get("steps").map(String::as_str),
            Some(&H3_DEFAULT_STEPS.to_string()[..])
        );
    }

    /// Dev utility for the manual structural probe (not part of the suite).
    /// Writes ready-to-POST /prompt bodies for both variants to the system
    /// temp dir. Run:
    /// `cargo test --manifest-path src-tauri/Cargo.toml print_h3_workflow_json -- --ignored --nocapture`
    #[test]
    #[ignore = "dev utility for the manual ComfyUI structural probe"]
    fn print_h3_workflow_json() {
        for variant in ["fl2va", "ref2va"] {
            let mut params = video_params(variant);
            if variant == "ref2va" {
                params.video_ref_images = vec!["ref_probe.png".to_string()];
            }
            let body = json!({ "prompt": build(&params, 42, true) });
            let path = std::env::temp_dir().join(format!("h3_{variant}_workflow.json"));
            std::fs::write(&path, serde_json::to_string_pretty(&body).unwrap()).unwrap();
            println!("wrote {}", path.display());
        }
    }
}
