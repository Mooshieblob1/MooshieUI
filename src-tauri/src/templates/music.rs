//! Native ComfyUI YuE2 graph, based on Comfy-Org/ComfyUI#16250.
//! Audio has its own terminal node and must not enter finish_workflow's image chain.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicParams {
    pub checkpoint: String,
    pub style: String,
    pub lyrics: String,
    pub planning: String,
    pub abc: String,
    pub max_duration: f64,
    pub steps: u32,
    #[serde(with = "crate::comfyui::types::seed_string")]
    pub seed: i64,
}

pub const REQUIRED_NODES: &[&str] = &[
    "CheckpointLoaderSimple",
    "YuE2GenerateABC",
    "YuE2GenerateMusic",
    "EmptyYuE2LatentAudio",
    "KSampler",
    "VAEDecodeAudioTiled",
    "SaveAudio",
    "PreviewAny",
];

pub fn validate(params: &MusicParams) -> Result<(), String> {
    if params.checkpoint.trim().is_empty() || !params.checkpoint.to_lowercase().contains("yue2") {
        return Err("Select a YuE2 checkpoint from ComfyUI's checkpoints folder.".into());
    }
    if params.style.trim().is_empty() || params.lyrics.trim().is_empty() {
        return Err("Enter a style and sectioned lyrics for YuE2.".into());
    }
    if !matches!(params.planning.as_str(), "full" | "melody" | "off") {
        return Err("YuE2 planning must be full, melody, or off.".into());
    }
    if params.planning == "off" && !params.abc.trim().is_empty() {
        return Err("Choose full or melody planning to use an ABC score.".into());
    }
    if !(1.0..=360.0).contains(&params.max_duration) {
        return Err("YuE2 maximum duration must be between 1 and 360 seconds.".into());
    }
    if !(1..=100).contains(&params.steps) || params.seed < -1 {
        return Err("Use 1-100 steps and a seed of -1 (random) or a non-negative integer.".into());
    }
    if params.style.len() > 16_384 || params.lyrics.len() > 65_536 || params.abc.len() > 131_072 {
        return Err("YuE2 style, lyrics, or score exceeds the input size limit.".into());
    }
    Ok(())
}

pub fn build(params: &MusicParams, seed: i64) -> Value {
    let mut graph = json!({
        "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": params.checkpoint}},
        "3": {"class_type": "YuE2GenerateMusic", "inputs": {
            "clip": ["1", 1], "style": params.style, "lyrics": params.lyrics,
            "abc": "", "seed": seed,
            "mode": if params.planning == "melody" { "melody" } else { "full" },
            "max_duration": params.max_duration,
            "temperature": 1.0, "top_p": 0.95, "top_k": 100, "repetition_penalty": 1.2
        }},
        "4": {"class_type": "EmptyYuE2LatentAudio", "inputs": {"seconds": ["3", 1], "batch_size": 1}},
        "5": {"class_type": "KSampler", "inputs": {
            "model": ["1", 0], "positive": ["3", 0], "negative": ["3", 0],
            "latent_image": ["4", 0], "seed": seed, "steps": params.steps,
            "cfg": 1.0, "sampler_name": "dpm_2", "scheduler": "sgm_uniform", "denoise": 1.0
        }},
        "6": {"class_type": "VAEDecodeAudioTiled", "inputs": {
            "samples": ["5", 0], "vae": ["1", 2], "tile_size": 1920, "overlap": 128
        }},
        "8": {"class_type": "SaveAudio", "inputs": {
            "audio": ["6", 0], "filename_prefix": "audio/mooshie_yue2"
        }}
    });
    // The native music node selects off mode when abc is empty. It does not
    // accept "off" as a mode widget value.
    if params.planning != "off" {
        let score = if params.abc.trim().is_empty() {
            graph["2"] = json!({"class_type": "YuE2GenerateABC", "inputs": {
                "clip": ["1", 1], "style": params.style, "lyrics": params.lyrics,
                "seed": seed, "mode": params.planning, "max_abc_tokens": 8192
            }});
            json!(["2", 0])
        } else {
            json!(params.abc)
        };
        graph["3"]["inputs"]["abc"] = score.clone();
        graph["9"] = json!({"class_type": "PreviewAny", "inputs": {"source": score}});
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    fn params() -> MusicParams {
        MusicParams {
            checkpoint: "yue2_3b_bf16.safetensors".into(),
            style: "folk".into(),
            lyrics: "[verse]\nMorning light".into(),
            planning: "full".into(),
            abc: "".into(),
            max_duration: 120.0,
            steps: 32,
            seed: -1,
        }
    }
    #[test]
    fn native_workflow_uses_actual_duration_and_audio_ports() {
        let graph = build(&params(), i64::MAX);
        assert_eq!(graph["3"]["inputs"]["abc"], json!(["2", 0]));
        assert_eq!(graph["4"]["inputs"]["seconds"], json!(["3", 1]));
        assert_eq!(graph["6"]["inputs"]["vae"], json!(["1", 2]));
        assert_eq!(graph["5"]["inputs"]["seed"], json!(i64::MAX));
        assert_eq!(graph["8"]["class_type"], "SaveAudio");
        for node in graph.as_object().unwrap().values() {
            assert!(!node["class_type"].as_str().unwrap().contains("Image"));
            for input in node["inputs"].as_object().unwrap().values() {
                if let Some(link) = input.as_array() {
                    assert!(
                        graph.get(link[0].as_str().unwrap()).is_some(),
                        "dangling link"
                    );
                }
            }
        }
    }
    #[test]
    fn custom_score_and_off_mode_do_not_generate_a_plan() {
        let mut p = params();
        p.abc = "X:1\nK:C\nCDEF".into();
        let graph = build(&p, 1);
        assert!(graph.get("2").is_none());
        assert_eq!(graph["3"]["inputs"]["abc"], p.abc);
        p.planning = "off".into();
        assert!(validate(&p).is_err());
        p.abc.clear();
        let graph = build(&p, 1);
        assert!(validate(&p).is_ok());
        assert!(graph.get("2").is_none() && graph.get("9").is_none());
        assert_eq!(graph["3"]["inputs"]["abc"], "");
        assert_eq!(graph["3"]["inputs"]["mode"], "full");
    }
    #[test]
    fn rejects_invalid_limits_and_preserves_seed_precision() {
        let mut p = params();
        for duration in [0.0, 361.0, f64::NAN, f64::INFINITY] {
            p.max_duration = duration;
            assert!(validate(&p).is_err());
        }
        p = params();
        p.seed = i64::MAX;
        let encoded = serde_json::to_value(&p).unwrap();
        assert_eq!(encoded["seed"], i64::MAX.to_string());
        assert_eq!(
            serde_json::from_value::<MusicParams>(encoded).unwrap().seed,
            i64::MAX
        );
    }
}
