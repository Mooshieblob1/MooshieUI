//! Retake a frame range of a retained clean H3 draft, keeping its soundtrack.
//!
//! `MooshieH3RetakeMask` puts a per-token noise mask on the draft's joint latent:
//! the video frames in the range are regenerated, everything else (all of the
//! audio included) stays fixed, so the new frames are sampled against the
//! original sound. `MooshieH3RestoreAudio` then guarantees the decoded audio is
//! the original latent, bit for bit.
use crate::comfyui::types::GenerationParams;
use serde_json::{json, Value};

/// Sampler for the full-denoise retake: the H3 preset for the Standard method.
pub const RETAKE_SAMPLER: &str = "res_multistep";

pub fn build(
    params: &GenerationParams,
    draft_id: &str,
    source: &str,
    start_frame: u32,
    end_frame: u32,
    steps: u32,
    seed: i64,
) -> Value {
    let mut workflow = serde_json::Map::new();
    let mut next_id = 1u32;
    let mut add = |class: &str, inputs: Value| {
        let id = next_id.to_string();
        next_id += 1;
        workflow.insert(id.clone(), json!({"class_type": class, "inputs": inputs}));
        id
    };
    let filename = params.video_diffusion_model.as_deref().unwrap_or("");
    // Fresh, unaccelerated base model, as for refinement: the draft's Turbo or
    // PDD adapter fixes its own schedule and would not fit this one.
    let model = if filename.to_ascii_lowercase().ends_with(".gguf") {
        add("UnetLoaderGGUF", json!({"unet_name": filename}))
    } else {
        add(
            "UNETLoader",
            json!({"unet_name": filename, "weight_dtype": "default"}),
        )
    };
    let shifted = add(
        "MiniMaxH3SigmaShift",
        json!({"model": [model, 0], "shift_video": 12.0, "shift_audio": 3.0}),
    );
    let load = add("MooshieH3LoadDraft", json!({"draft_id": draft_id}));
    let masked = add(
        "MooshieH3RetakeMask",
        json!({"samples": [load.as_str(), 0], "start_frame": start_frame, "end_frame": end_frame}),
    );
    let guider = add(
        "BasicGuider",
        json!({"model": [shifted.as_str(), 0], "conditioning": [load.as_str(), 1]}),
    );
    let noise = add("RandomNoise", json!({"noise_seed": seed}));
    let sampler = add("KSamplerSelect", json!({"sampler_name": RETAKE_SAMPLER}));
    let sigmas = add(
        "BasicScheduler",
        json!({"model": [shifted, 0], "scheduler": "simple", "steps": steps, "denoise": 1.0}),
    );
    let sample = add(
        "SamplerCustomAdvanced",
        json!({"noise": [noise, 0], "guider": [guider, 0], "sampler": [sampler, 0], "sigmas": [sigmas, 0], "latent_image": [masked, 0]}),
    );
    let restored = add(
        "MooshieH3RestoreAudio",
        json!({"samples": [sample, 1], "original": [load.as_str(), 0]}),
    );
    let vae = add("VAELoader", json!({"vae_name": params.video_vae_model}));
    let decoded = add(
        "VAEDecode",
        json!({"samples": [restored.as_str(), 0], "vae": [vae, 0]}),
    );
    let audio = if params.video_timeline_custom_audio
        && params
            .video_timeline_data
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    {
        json!([load, 2])
    } else {
        let vae = add(
            "VAELoader",
            json!({"vae_name": params.video_audio_vae_model}),
        );
        let decoded = add(
            "VAEDecodeAudio",
            json!({"samples": [restored, 0], "vae": [vae, 0]}),
        );
        json!([decoded, 0])
    };
    let (images, fps) = if params.video_rife_enabled {
        let settings = super::rife::RifeSettings::from_params(params);
        let node = settings.node(json!([decoded, 0]));
        let id = add(
            node["class_type"].as_str().unwrap_or("RIFE VFI"),
            node["inputs"].clone(),
        );
        (id, settings.output_fps(24.0))
    } else {
        (decoded, 24.0)
    };
    let video = add(
        "CreateVideo",
        json!({"images": [images, 0], "audio": audio, "fps": fps}),
    );
    let mut metadata = super::video::video_metadata_params(params, seed);
    metadata.insert("steps".into(), steps.to_string());
    metadata.insert("sampler".into(), RETAKE_SAMPLER.into());
    metadata.insert("scheduler".into(), "simple".into());
    metadata.insert("mooshie_video_fps".into(), fps.to_string());
    metadata.insert("mooshie_video_acceleration".into(), "standard".into());
    metadata.remove("mooshie_video_turbo");
    metadata.remove("mooshie_video_turbo_preset");
    metadata.remove("mooshie_video_turbo_lora");
    metadata.remove("mooshie_video_vdn_checkpoint");
    metadata.insert("mooshie_video_source_draft".into(), draft_id.into());
    metadata.insert("mooshie_video_source_filename".into(), source.into());
    metadata.insert(
        "mooshie_video_retake_frames".into(),
        format!("{start_frame}-{end_frame}"),
    );
    add(
        "MooshieSaveVideo",
        json!({"video": [video, 0], "filename_prefix": "mooshie_video_retake", "metadata_json": crate::metadata::format_swarmui_json(&metadata)}),
    );
    Value::Object(workflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> GenerationParams {
        serde_json::from_value(json!({
            "mode": "video",
            "positive_prompt": "a cat",
            "negative_prompt": "",
            "checkpoint": "",
            "loras": [],
            "sampler_name": "euler",
            "scheduler": "normal",
            "steps": 28,
            "cfg": 1.0,
            "seed": "7",
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
            "video_variant": "fl2va",
            "video_duration_seconds": 5.0,
            "video_megapixels": 0.4,
            "video_aspect_ratio": "16:9",
            "video_diffusion_model": "minimax_h3_fl2va_pruned_int8_convrot.safetensors",
            "video_clip_model": "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
            "video_vae_model": "minimax_h3_video_vae_fp16.safetensors",
            "video_audio_vae_model": "minimax_h3_audio_vae_fp32.safetensors",
            "video_acceleration": "turbo",
            "video_turbo_preset": "pdd_fl2va_8"
        }))
        .expect("valid test params")
    }

    fn nodes<'a>(workflow: &'a Value, class: &str) -> Vec<(&'a String, &'a Value)> {
        workflow
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, node)| node["class_type"] == class)
            .collect()
    }

    #[test]
    fn retake_masks_the_draft_range_and_restores_its_audio() {
        let workflow = build(&params(), &"a".repeat(32), "clip.mp4", 24, 72, 20, 1234);
        let (load_id, _) = nodes(&workflow, "MooshieH3LoadDraft")[0];
        let (mask_id, mask) = nodes(&workflow, "MooshieH3RetakeMask")[0];
        assert_eq!(mask["inputs"]["samples"], json!([load_id, 0]));
        assert_eq!(mask["inputs"]["start_frame"], 24);
        assert_eq!(mask["inputs"]["end_frame"], 72);

        let (_, sample) = nodes(&workflow, "SamplerCustomAdvanced")[0];
        assert_eq!(sample["inputs"]["latent_image"], json!([mask_id, 0]));
        let (_, guider) = nodes(&workflow, "BasicGuider")[0];
        assert_eq!(guider["inputs"]["conditioning"], json!([load_id, 1]));
        let (_, noise) = nodes(&workflow, "RandomNoise")[0];
        assert_eq!(noise["inputs"]["noise_seed"], 1234);

        // The soundtrack is the draft's own latent, not whatever the sampler returned.
        let (restore_id, restore) = nodes(&workflow, "MooshieH3RestoreAudio")[0];
        assert_eq!(restore["inputs"]["original"], json!([load_id, 0]));
        let (_, audio) = nodes(&workflow, "VAEDecodeAudio")[0];
        assert_eq!(audio["inputs"]["samples"], json!([restore_id, 0]));
    }

    #[test]
    fn retake_samples_the_base_model_at_full_denoise() {
        let workflow = build(&params(), &"a".repeat(32), "clip.mp4", 0, 30, 16, 1);
        // The draft's PDD preset is not reapplied.
        assert!(nodes(&workflow, "LoraLoaderModelOnly").is_empty());
        assert!(nodes(&workflow, "MiniMaxH3TurboLoRA").is_empty());
        let (_, scheduler) = nodes(&workflow, "BasicScheduler")[0];
        assert_eq!(scheduler["inputs"]["steps"], 16);
        assert_eq!(scheduler["inputs"]["denoise"], 1.0);
        let (_, sampler) = nodes(&workflow, "KSamplerSelect")[0];
        assert_eq!(sampler["inputs"]["sampler_name"], RETAKE_SAMPLER);
        let (_, save) = nodes(&workflow, "MooshieSaveVideo")[0];
        let metadata = save["inputs"]["metadata_json"].as_str().unwrap();
        // format_swarmui_json keeps mooshie_ keys with the prefix stripped.
        let parsed: Value = serde_json::from_str(metadata).unwrap();
        assert!(parsed
            .to_string()
            .contains(r#""video_retake_frames":"0-30""#));
        assert!(!metadata.contains("pdd_fl2va_8"));
    }
}
