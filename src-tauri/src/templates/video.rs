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

/// MiniMax H3 emits 24 fps and only accepts frame counts on the 17n+5 grid
/// (widget: min 5, max 3600, step 17). Snaps the requested duration UP to
/// the next valid count, clamped to the largest on-grid value <= 3600.
pub fn compute_h3_frame_length(seconds: f64) -> u32 {
    let base = ((seconds * 24.0).round() as i64).max(5);
    let snapped = base + (5 - (base % 17)).rem_euclid(17);
    snapped.min(3592) as u32
}

/// Width/height from a target megapixel budget and aspect ratio, snapped to
/// multiples of 32 (the H3 width/height widget step). Replaces the official
/// workflow's ResolutionSelector custom node. Unknown ratios fall back to
/// 16:9.
pub fn compute_h3_dimensions(aspect_ratio: &str, megapixels: f64) -> (u32, u32) {
    let (rw, rh) = match aspect_ratio {
        "9:16" => (9.0, 16.0),
        "1:1" => (1.0, 1.0),
        "4:3" => (4.0, 3.0),
        "3:4" => (3.0, 4.0),
        _ => (16.0, 9.0),
    };
    let pixels = megapixels.max(0.05) * 1_000_000.0;
    let height = (pixels * rh / rw).sqrt();
    let width = height * rw / rh;
    let snap = |v: f64| ((v / 32.0).round() as u32).max(2) * 32;
    (snap(width), snap(height))
}

/// Build the complete MiniMax H3 video workflow for either variant.
///
/// Returns the final workflow JSON directly — never routed through
/// `finish_workflow`. The negative prompt is unused by design: `BasicGuider`
/// has no negative conditioning input.
pub fn build(params: &GenerationParams, seed: i64) -> Value {
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

    let h3_id = if params.video_variant == "ref2va" {
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
            inputs.insert(key.to_string(), json!([load_id.as_str(), 0]));
        }
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({ "class_type": "MiniMaxH3ImageToVideo", "inputs": inputs }),
        );
        next_id += 1;
        id
    };

    let noise_id = next_id.to_string();
    workflow.insert(
        noise_id.clone(),
        json!({ "class_type": "RandomNoise", "inputs": { "noise_seed": seed } }),
    );
    next_id += 1;

    let sampler_select_id = next_id.to_string();
    workflow.insert(
        sampler_select_id.clone(),
        json!({
            "class_type": "KSamplerSelect",
            "inputs": { "sampler_name": "res_multistep" }
        }),
    );
    next_id += 1;

    let scheduler_id = next_id.to_string();
    workflow.insert(
        scheduler_id.clone(),
        json!({
            "class_type": "BasicScheduler",
            "inputs": {
                "model": [unet_id.as_str(), 0],
                "scheduler": "simple",
                "steps": 20,
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
                "model": [unet_id.as_str(), 0],
                "conditioning": [h3_id.as_str(), 0]
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
                "latent_image": [h3_id.as_str(), 1]
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

    let create_video_id = next_id.to_string();
    workflow.insert(
        create_video_id.clone(),
        json!({
            "class_type": "CreateVideo",
            "inputs": {
                "images": [decode_id.as_str(), 0],
                "audio": [audio_decode_id.as_str(), 0],
                "fps": 24.0
            }
        }),
    );
    next_id += 1;

    let save_id = next_id.to_string();
    workflow.insert(
        save_id,
        json!({
            "class_type": "MooshieSaveVideo",
            "inputs": {
                "video": [create_video_id.as_str(), 0],
                "filename_prefix": "mooshie_video"
            }
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
        let workflow = build(&video_params("fl2va"), 42);
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
        let workflow = build(&params, 1);
        assert_eq!(nodes_of_class(&workflow, "LoadImage").len(), 2);
        let h3 = nodes_of_class(&workflow, "MiniMaxH3ImageToVideo")[0];
        assert!(h3["inputs"]["first_frame"].is_array());
        assert!(h3["inputs"]["last_frame"].is_array());
    }

    #[test]
    fn fl2va_ignores_blank_frame_entries() {
        let mut params = video_params("fl2va");
        params.video_first_frame = Some("  ".to_string());
        let workflow = build(&params, 1);
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
        let workflow = build(&params, 1);
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
        let workflow = crate::templates::build_workflow(&video_params("fl2va"), 42);
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
    fn validate_video_ignores_stale_image_mode_flags() {
        // Mode switches can leave image-only toggles set; they must not
        // block video generation (the video arm early-returns).
        let mut params = video_params("fl2va");
        params.style_transfer_enabled = true;
        assert!(crate::templates::validate_generation_params(&params).is_ok());
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
            let body = json!({ "prompt": build(&params, 42) });
            let path = std::env::temp_dir().join(format!("h3_{variant}_workflow.json"));
            std::fs::write(&path, serde_json::to_string_pretty(&body).unwrap()).unwrap();
            println!("wrote {}", path.display());
        }
    }
}
