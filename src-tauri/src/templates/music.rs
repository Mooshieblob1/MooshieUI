//! Native ComfyUI YuE2 graph, based on Comfy-Org/ComfyUI#16250.
//! Audio has its own terminal node and must not enter finish_workflow's image chain.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MusicTask {
    #[default]
    Audio,
    Plan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicSampling {
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub repetition_penalty: f64,
    pub cfg_scale: f64,
    pub max_abc_tokens: u32,
}

impl Default for MusicSampling {
    fn default() -> Self {
        Self {
            temperature: 1.0,
            top_p: 0.95,
            top_k: 100,
            repetition_penalty: 1.2,
            cfg_scale: -1.0,
            max_abc_tokens: 8192,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicParams {
    pub checkpoint: String,
    pub style: String,
    pub lyrics: String,
    pub planning: String,
    pub abc: String,
    #[serde(default)]
    pub cover: bool,
    pub max_duration: f64,
    pub steps: u32,
    #[serde(with = "crate::comfyui::types::seed_string")]
    pub seed: i64,
    #[serde(default)]
    pub task: MusicTask,
    #[serde(default)]
    pub sampling: MusicSampling,
}

pub const EXTENDED_NODES: &[&str] = &["MooshieYuE2Plan", "MooshieYuE2Music"];
pub const COVER_NODES: &[&str] = &[
    "AudioEncoderLoader",
    "SheetSage2AudioToABC",
    "MooshieMusicLoadAudio",
    "PreviewAny",
];

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
    if params.cover {
        if params.planning == "off" {
            return Err("Song covers require melody or full planning.".into());
        }
        validate_score_input(&params.abc, params.planning == "melody")?;
    }
    if params.task == MusicTask::Plan && (params.planning == "off" || params.cover) {
        return Err(
            "Score generation requires full or melody planning for an original song.".into(),
        );
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
    let s = &params.sampling;
    if !(0.0..=5.0).contains(&s.temperature)
        || !(0.01..=1.0).contains(&s.top_p)
        || !(1..=32768).contains(&s.top_k)
        || !(0.01..=10.0).contains(&s.repetition_penalty)
        || !(s.cfg_scale == -1.0 || (0.0..=20.0).contains(&s.cfg_scale))
        || !(1..=20000).contains(&s.max_abc_tokens)
    {
        return Err("Invalid YuE2 advanced sampling settings.".into());
    }
    Ok(())
}

pub fn validate_cover_score(abc: &str) -> Result<(), String> {
    validate_score_input(abc, true)
}

fn validate_score_input(abc: &str, melody_only: bool) -> Result<(), String> {
    use std::sync::LazyLock;
    static HEADER: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^\s*(?:[A-Za-z]:|%)").unwrap());
    static TOKENS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"\[[A-Za-z]:[^\]]*\]|%.*|"(?:\\.|[^"\\])*""#).unwrap()
    });
    let has_x = abc.lines().any(|line| {
        line.trim()
            .strip_prefix("X:")
            .is_some_and(|s| s.trim().starts_with(|c: char| c.is_ascii_digit()))
    });
    let has_key = abc.lines().any(|line| {
        line.trim()
            .strip_prefix("K:")
            .is_some_and(|s| !s.trim().is_empty())
    });
    let mut has_notes = false;
    let mut has_chords = false;
    for line in abc.lines().filter(|line| !HEADER.is_match(line)) {
        let body = TOKENS.replace_all(line, |caps: &regex::Captures| {
            let token = &caps[0];
            if token.starts_with('"') && !token.chars().nth(1).is_some_and(|c| "^_<>@".contains(c))
            {
                has_chords = true;
            }
            ""
        });
        has_notes |= body.chars().any(|c| matches!(c, 'A'..='G' | 'a'..='g'));
    }
    if abc.len() > 131_072 || !has_x || !has_key || !has_notes {
        return Err("Import or enter a melody ABC score with X: and K: headers and musical notes, then review it before generating a cover.".into());
    }
    if melody_only && has_chords {
        return Err("Remove chord symbols from the cover score so the accompaniment can adapt to the new style.".into());
    }
    Ok(())
}

pub fn build(params: &MusicParams, seed: i64) -> Value {
    build_with_capabilities(params, seed, false)
}

pub fn build_with_capabilities(params: &MusicParams, seed: i64, extended: bool) -> Value {
    let sampling = &params.sampling;
    let mut graph = json!({
        "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": params.checkpoint}},
        "3": {"class_type": "YuE2GenerateMusic", "inputs": {
            "clip": ["1", 1], "style": params.style, "lyrics": params.lyrics,
            "abc": "", "seed": seed,
            "mode": if params.planning == "melody" { "melody" } else { "full" },
            "max_duration": params.max_duration,
            "temperature": sampling.temperature, "top_p": sampling.top_p,
            "top_k": sampling.top_k, "repetition_penalty": sampling.repetition_penalty
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
    if extended {
        graph["3"]["class_type"] = json!("MooshieYuE2Music");
        graph["3"]["inputs"]["cfg_scale"] = json!(sampling.cfg_scale);
        graph["10"] = json!({"class_type": "PreviewAny", "inputs": {"source": ["3", 2]}});
    }
    // The native music node selects off mode when abc is empty. It does not
    // accept "off" as a mode widget value.
    if params.planning != "off" {
        let score = if params.abc.trim().is_empty() || params.task == MusicTask::Plan {
            graph["2"] = json!({"class_type": if extended { "MooshieYuE2Plan" } else { "YuE2GenerateABC" }, "inputs": {
                "clip": ["1", 1], "style": params.style, "lyrics": params.lyrics,
                "seed": seed, "mode": params.planning, "max_abc_tokens": sampling.max_abc_tokens
            }});
            if extended {
                graph["11"] = json!({"class_type": "PreviewAny", "inputs": {"source": ["2", 1]}});
            }
            json!(["2", 0])
        } else {
            json!(params.abc)
        };
        graph["3"]["inputs"]["abc"] = score.clone();
        graph["9"] = json!({"class_type": "PreviewAny", "inputs": {"source": score}});
    }
    graph["12"] = json!({"class_type": "PreviewAny", "inputs": {"source": json!({
        "kind": if params.task == MusicTask::Plan { "plan" } else { "audio" },
        "score_source": if params.planning == "off" { "none" } else if params.task == MusicTask::Plan || params.abc.trim().is_empty() { "generated" } else { "provided" },
    }).to_string()}});
    if params.task == MusicTask::Plan {
        for node in ["3", "4", "5", "6", "8", "10"] {
            graph.as_object_mut().unwrap().remove(node);
        }
    }
    graph
}

pub fn build_transcription(encoder: &str, audio: &str, mode: &str) -> Value {
    json!({
        "1": {"class_type": "AudioEncoderLoader", "inputs": {"audio_encoder_name": encoder}},
        "2": {"class_type": "MooshieMusicLoadAudio", "inputs": {"audio": audio}},
        "3": {"class_type": "SheetSage2AudioToABC", "inputs": {"audio_encoder": ["1", 0], "audio": ["2", 0], "mode": mode}},
        "9": {"class_type": "PreviewAny", "inputs": {"source": ["3", 0]}},
        "12": {"class_type": "PreviewAny", "inputs": {"source": "{\"kind\":\"transcription\",\"score_source\":\"transcribed\"}"}}
    })
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
            cover: false,
            max_duration: 120.0,
            steps: 32,
            seed: -1,
            task: MusicTask::Audio,
            sampling: MusicSampling::default(),
        }
    }
    #[test]
    fn reviewed_plans_have_no_audio_nodes_and_never_reuse_old_abc() {
        let mut p = params();
        p.task = MusicTask::Plan;
        p.abc = "old draft".into();
        for extended in [false, true] {
            let graph = build_with_capabilities(&p, 7, extended);
            assert!(graph.get("2").is_some());
            assert_eq!(graph["9"]["inputs"]["source"], json!(["2", 0]));
            for node in graph.as_object().unwrap().values() {
                assert!(![
                    "KSampler",
                    "SaveAudio",
                    "MooshieYuE2Music",
                    "YuE2GenerateMusic"
                ]
                .contains(&node["class_type"].as_str().unwrap()));
            }
            assert_eq!(graph.get("11").is_some(), extended);
        }
        p.cover = true;
        assert!(validate(&p).is_err());
        p.cover = false;
        p.planning = "off".into();
        p.abc.clear();
        assert!(validate(&p).is_err());
    }

    #[test]
    fn fidelity_sampling_and_receipts_use_correct_stages() {
        let mut p = params();
        p.cover = true;
        p.abc = "X:1\nK:C\n\"Cmaj7\"CDEF".into();
        assert!(validate(&p).is_ok());
        p.planning = "melody".into();
        assert!(validate(&p).is_err());
        p.planning = "full".into();
        p.sampling.cfg_scale = 1.3;
        p.sampling.temperature = 0.8;
        let graph = build_with_capabilities(&p, 42, true);
        assert!(graph.get("2").is_none());
        assert_eq!(graph["3"]["inputs"]["cfg_scale"], 1.3);
        assert_eq!(graph["3"]["inputs"]["temperature"], 0.8);
        assert_eq!(graph["5"]["inputs"]["cfg"], 1.0);
        assert_eq!(graph["10"]["inputs"]["source"], json!(["3", 2]));
        let fallback = build_with_capabilities(&p, 42, false);
        assert!(fallback["3"]["inputs"].get("cfg_scale").is_none());
        assert!(fallback.get("10").is_none());
        for invalid in [f64::NAN, f64::INFINITY, -0.1, 5.1] {
            p.sampling.temperature = invalid;
            assert!(validate(&p).is_err());
        }
        p.sampling = MusicSampling::default();
        p.sampling.max_abc_tokens = 20001;
        assert!(validate(&p).is_err());
    }

    #[test]
    fn transcription_uses_native_encoder_and_list_score_output() {
        let graph = build_transcription(
            "sheetsage2_bf16.safetensors",
            "mooshie_cover_test.wav",
            "full",
        );
        assert_eq!(
            graph["1"]["inputs"]["audio_encoder_name"],
            "sheetsage2_bf16.safetensors"
        );
        assert_eq!(graph["3"]["inputs"]["audio"], json!(["2", 0]));
        assert_eq!(graph["3"]["inputs"]["mode"], "full");
        assert_eq!(graph["9"]["inputs"]["source"], json!(["3", 0]));
        assert!(graph.get("8").is_none());
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
    fn covers_require_a_chord_free_melody_and_skip_score_generation() {
        let mut p = params();
        p.cover = true;
        assert!(validate(&p).is_err());
        p.planning = "melody".into();
        assert!(validate(&p).is_err());
        p.abc = "X:1\nV:Vocal name=\"Voice\"\nK:C\nC2 D2 [V:Ins name=\"Piano\"] \"^softly\"E2 % \"Am\" comment".into();
        assert!(validate(&p).is_ok());
        let graph = build(&p, 42);
        assert!(graph.get("2").is_none());
        assert_eq!(graph["3"]["inputs"]["mode"], "melody");
        assert_eq!(graph["3"]["inputs"]["abc"], p.abc);
        assert_eq!(graph["9"]["inputs"]["source"], p.abc);
        for bad in [
            "X:1\nK:C\n\"Am\"C2 D2",
            "X:1\nK:C\n% CDEF",
            "X:1\nK:C\n[V:Vocal] z8",
            "not a score",
        ] {
            p.abc = bad.into();
            assert!(validate(&p).is_err(), "{bad}");
        }
        let mut old = serde_json::to_value(params()).unwrap();
        old.as_object_mut().unwrap().remove("cover");
        assert!(!serde_json::from_value::<MusicParams>(old).unwrap().cover);
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
