//! Refine a retained clean H3 draft at exactly twice its spatial dimensions.
use crate::comfyui::types::GenerationParams;
use serde_json::{json, Value};

pub fn build(
    params: &GenerationParams,
    draft_id: &str,
    source: &str,
    width: u32,
    height: u32,
    steps: u32,
    sigma: f64,
) -> Value {
    let seed = params.seed;
    let mut workflow = serde_json::Map::new();
    let mut next_id = 1u32;
    let mut add = |class: &str, inputs: Value| {
        let id = next_id.to_string();
        next_id += 1;
        workflow.insert(id.clone(), json!({"class_type": class, "inputs": inputs}));
        id
    };
    let filename = params.video_diffusion_model.as_deref().unwrap_or("");
    // Fresh, unaccelerated base model: a distilled draft can be refined without
    // carrying its Turbo/VDN patch into the partial-noise continuation schedule.
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
    let upscale = add(
        "MooshieH3UpscaleDraft",
        json!({"samples": [load.as_str(), 0], "positive": [load.as_str(), 1], "steps": steps, "sigma": sigma}),
    );
    let guider = add(
        "BasicGuider",
        json!({"model": [shifted, 0], "conditioning": [upscale.as_str(), 1]}),
    );
    let noise = add("RandomNoise", json!({"noise_seed": seed}));
    let sampler = add("KSamplerSelect", json!({"sampler_name": "euler"}));
    let sample = add(
        "SamplerCustomAdvanced",
        json!({"noise": [noise, 0], "guider": [guider, 0], "sampler": [sampler, 0], "sigmas": [upscale.as_str(), 2], "latent_image": [upscale, 0]}),
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
    metadata.insert("size".into(), format!("{}x{}", width * 2, height * 2));
    metadata.insert("steps".into(), steps.to_string());
    metadata.insert("sampler".into(), "euler".into());
    metadata.insert("scheduler".into(), "linear_sigma".into());
    metadata.insert("mooshie_video_fps".into(), fps.to_string());
    metadata.insert("mooshie_video_acceleration".into(), "standard".into());
    metadata.remove("mooshie_video_turbo");
    metadata.remove("mooshie_video_turbo_preset");
    metadata.remove("mooshie_video_turbo_lora");
    metadata.remove("mooshie_video_vdn_checkpoint");
    metadata.insert("mooshie_video_source_draft".into(), draft_id.into());
    metadata.insert("mooshie_video_source_filename".into(), source.into());
    metadata.insert("mooshie_video_refine_sigma".into(), sigma.to_string());
    metadata.insert(
        "mooshie_video_upscaler".into(),
        super::super::commands::video_drafts::MODEL_NAME.into(),
    );
    add(
        "MooshieSaveVideo",
        json!({"video": [video, 0], "filename_prefix": "mooshie_video_2x", "metadata_json": crate::metadata::format_swarmui_json(&metadata)}),
    );
    Value::Object(workflow)
}
