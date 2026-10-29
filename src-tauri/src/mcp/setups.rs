//! Generation setups the MCP server reuses.
//!
//! The frontend works out a model's family, loaders, text encoder and tuned
//! defaults before it sends `generate`, and none of that can be rebuilt from a
//! filename on the Rust side. So the MCP server never builds a request from
//! scratch: it remembers the last txt2img request the owner ran with each
//! model and lets an agent change only what is safe to change (prompt, size,
//! steps, CFG, seed, sampler, LoRAs).
//!
//! Stored in `mcp_setups.json` in the app data directory, without the prompt
//! and without any input image.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::comfyui::types::{GenerationParams, LoraParam};

/// Most models remembered; the least recently used one is dropped beyond it.
const MAX_SETUPS: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setup {
    /// Checkpoint, or diffusion model for split loaders. The lookup key.
    pub model: String,
    pub last_used_ms: u64,
    pub params: GenerationParams,
}

static SETUPS: LazyLock<Mutex<Vec<Setup>>> = LazyLock::new(|| Mutex::new(load()));

fn path() -> Option<std::path::PathBuf> {
    crate::config::app_data_dir().map(|d| d.join("mcp_setups.json"))
}

fn load() -> Vec<Setup> {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn persist(setups: &[Setup]) {
    let Some(path) = path() else { return };
    let Ok(text) = serde_json::to_string(setups) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)) {
        log::warn!("Could not save MCP setups: {e}");
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The model a request runs, as the setup key.
pub fn model_key(params: &GenerationParams) -> String {
    if params.use_split_model {
        params.diffusion_model.clone().unwrap_or_default()
    } else {
        params.checkpoint.clone()
    }
}

/// A request reduced to a reusable setup, or `None` when it is not one: only
/// plain local txt2img qualifies. NovelAI, paused/resumed runs, and modes that
/// need an input image are skipped.
pub fn snapshot(params: &GenerationParams) -> Option<GenerationParams> {
    if params.mode != "txt2img"
        || params.novelai.is_some()
        || params.pause_at_step.is_some()
        || !params.resume_stages.is_empty()
        || model_key(params).trim().is_empty()
    {
        return None;
    }
    let mut p = params.clone();
    // The prompt and everything tied to it belongs to that one image.
    p.positive_prompt.clear();
    p.positive_segments.clear();
    p.negative_segments.clear();
    p.detail_segments.clear();
    p.positive_regions.clear();
    p.facefix_prompt_override = None;
    // Inputs that point at one particular image (ComfyUI input files).
    p.input_image = None;
    p.mask_image = None;
    p.controlnet = None;
    p.style_transfer_enabled = false;
    p.style_reference_image = None;
    p.style_ref_enabled = false;
    p.style_ref_image = None;
    p.style_ref_image_2 = None;
    p.edit_reference_images.clear();
    p.resume_edit_image = None;
    p.resume_edit_mask = None;
    p.seed = -1;
    Some(p)
}

/// Remember a request the owner just ran. Cheap and best-effort.
pub fn remember(params: &GenerationParams) {
    let Some(snap) = snapshot(params) else {
        return;
    };
    let model = model_key(&snap);
    let mut setups = SETUPS.lock().unwrap();
    setups.retain(|s| s.model != model);
    setups.insert(
        0,
        Setup {
            model,
            last_used_ms: now_ms(),
            params: snap,
        },
    );
    setups.truncate(MAX_SETUPS);
    persist(&setups);
}

pub fn count() -> usize {
    SETUPS.lock().unwrap().len()
}

/// All setups, most recently used first.
pub fn list() -> Vec<Setup> {
    SETUPS.lock().unwrap().clone()
}

fn file_stem(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = match base.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => base,
    };
    stem.to_ascii_lowercase()
}

/// Find a setup by model name. `None` picks the most recently used one. The
/// name may be the exact filename, or its file name without folder or
/// extension, case-insensitively; a unique substring also matches.
pub fn find(setups: &[Setup], model: Option<&str>) -> Result<Setup, String> {
    let Some(wanted) = model.map(str::trim).filter(|m| !m.is_empty()) else {
        return setups.first().cloned().ok_or_else(|| {
            "No saved generation setup yet. Generate one image in MooshieUI (txt2img) with the model you want, then try again.".to_string()
        });
    };
    let lower = wanted.to_ascii_lowercase();
    let stem = file_stem(wanted);
    let exact = setups
        .iter()
        .find(|s| s.model == wanted)
        .or_else(|| {
            setups
                .iter()
                .find(|s| s.model.to_ascii_lowercase() == lower)
        })
        .or_else(|| setups.iter().find(|s| file_stem(&s.model) == stem));
    if let Some(s) = exact {
        return Ok(s.clone());
    }
    let partial: Vec<&Setup> = setups
        .iter()
        .filter(|s| s.model.to_ascii_lowercase().contains(&lower))
        .collect();
    match partial.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!(
            "No saved setup for '{wanted}'. Models with a setup: {}. Generate once with '{wanted}' in MooshieUI to add it.",
            names(setups)
        )),
        many => Err(format!(
            "'{wanted}' matches several models: {}. Use the full name.",
            many.iter().map(|s| s.model.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

fn names(setups: &[Setup]) -> String {
    if setups.is_empty() {
        return "none".into();
    }
    setups
        .iter()
        .map(|s| s.model.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LoraOverride {
    pub name: String,
    pub strength: Option<f64>,
    pub strength_clip: Option<f64>,
}

/// What an agent may change on top of a setup.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Overrides {
    #[serde(default)]
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub steps: Option<u32>,
    pub cfg: Option<f64>,
    pub seed: Option<i64>,
    pub batch_size: Option<u32>,
    pub sampler: Option<String>,
    pub scheduler: Option<String>,
    /// Replaces the setup's LoRAs when present; `[]` removes them all.
    pub loras: Option<Vec<LoraOverride>>,
}

pub const MIN_SIDE: u32 = 256;
pub const MAX_SIDE: u32 = 4096;

/// Snap a side length to a multiple of 16, which every supported family
/// accepts (SD and SDXL need 8, Flux and the DiT families 16).
fn snap_side(v: u32) -> u32 {
    (v.clamp(MIN_SIDE, MAX_SIDE) + 8) / 16 * 16
}

/// The request to run: `base` with the agent's changes applied.
pub fn apply(base: &GenerationParams, o: &Overrides) -> Result<GenerationParams, String> {
    let prompt = o.prompt.trim();
    if prompt.is_empty() {
        return Err("prompt is required".into());
    }
    if prompt.chars().count() > 10_000 {
        return Err("prompt is too long (10000 characters at most)".into());
    }
    let mut p = base.clone();
    p.mode = "txt2img".into();
    p.denoise = 1.0;
    p.positive_prompt = prompt.to_string();
    if let Some(neg) = &o.negative_prompt {
        p.negative_prompt = neg.trim().to_string();
    }
    if let Some(w) = o.width {
        p.width = snap_side(w);
    }
    if let Some(h) = o.height {
        p.height = snap_side(h);
    }
    if let Some(steps) = o.steps {
        if !(1..=150).contains(&steps) {
            return Err("steps must be between 1 and 150".into());
        }
        p.steps = steps;
    }
    if let Some(cfg) = o.cfg {
        if !(0.0..=30.0).contains(&cfg) {
            return Err("cfg must be between 0 and 30".into());
        }
        p.cfg = cfg;
    }
    p.seed = o.seed.filter(|s| *s >= 0).unwrap_or(-1);
    if let Some(n) = o.batch_size {
        if !(1..=8).contains(&n) {
            return Err("batch_size must be between 1 and 8".into());
        }
        p.batch_size = n;
    }
    if let Some(s) = o
        .sampler
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        p.sampler_name = s.to_string();
    }
    if let Some(s) = o
        .scheduler
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        p.scheduler = s.to_string();
    }
    if let Some(loras) = &o.loras {
        if loras.len() > 10 {
            return Err("10 LoRAs at most".into());
        }
        let mut out = Vec::with_capacity(loras.len());
        for l in loras {
            let name = l.name.trim();
            if name.is_empty() {
                return Err("every LoRA needs a name".into());
            }
            let strength = l.strength.unwrap_or(1.0);
            let clip = l.strength_clip.unwrap_or(strength);
            if !(-5.0..=5.0).contains(&strength) || !(-5.0..=5.0).contains(&clip) {
                return Err(format!("LoRA '{name}' strength must be between -5 and 5"));
            }
            out.push(LoraParam {
                name: name.to_string(),
                strength_model: strength,
                strength_clip: clip,
            });
        }
        p.loras = out;
    }
    Ok(p)
}

/// The metadata map embedded in a saved image, the same keys the frontend's
/// `buildPngMetadata` writes, so the gallery, "Use these settings" and the
/// gallery index read MCP images like any other.
pub fn image_metadata(params: &GenerationParams, seed: i64) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("positive_prompt".into(), params.positive_prompt.clone());
    m.insert("negative_prompt".into(), params.negative_prompt.clone());
    m.insert("steps".into(), params.steps.to_string());
    m.insert("sampler".into(), params.sampler_name.clone());
    m.insert("scheduler".into(), params.scheduler.clone());
    m.insert("cfg".into(), params.cfg.to_string());
    m.insert("seed".into(), seed.to_string());
    m.insert("size".into(), format!("{}x{}", params.width, params.height));
    m.insert("model".into(), model_key(params));
    m.insert("vae".into(), params.vae.clone().unwrap_or_default());
    m.insert("mode".into(), params.mode.clone());
    m.insert(
        "date".into(),
        chrono::Local::now().format("%Y-%m-%d").to_string(),
    );
    if !params.loras.is_empty() {
        m.insert(
            "loras".into(),
            params
                .loras
                .iter()
                .map(|l| format!("{}:{:.2}:{:.2}", l.name, l.strength_model, l.strength_clip))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if params.output_bit_depth != "8bit" {
        m.insert("bit_depth".into(), params.output_bit_depth.clone());
    }
    if params.upscale_enabled {
        m.insert(
            "upscale_model".into(),
            params.upscale_model.clone().unwrap_or_default(),
        );
        m.insert("upscale_scale".into(), params.upscale_scale.to_string());
        m.insert("upscale_denoise".into(), params.upscale_denoise.to_string());
    }
    m.insert(
        "mooshie_model_architecture".into(),
        params.model_architecture.clone(),
    );
    m.insert("mooshie_source".into(), "mcp".into());
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> GenerationParams {
        GenerationParams {
            mode: "txt2img".into(),
            positive_prompt: "a cat".into(),
            negative_prompt: "blurry".into(),
            checkpoint: "sdxl/illustrious_v2.safetensors".into(),
            sampler_name: "euler".into(),
            scheduler: "normal".into(),
            steps: 28,
            cfg: 5.0,
            seed: 1234,
            width: 832,
            height: 1216,
            batch_size: 1,
            denoise: 1.0,
            output_bit_depth: "8bit".into(),
            ..Default::default()
        }
    }

    fn setup(model: &str) -> Setup {
        Setup {
            model: model.into(),
            last_used_ms: 0,
            params: base(),
        }
    }

    #[test]
    fn snapshot_drops_the_prompt_and_inputs() {
        let mut p = base();
        p.input_image = Some("x.png".into());
        p.style_ref_enabled = true;
        p.style_ref_image_2 = Some("second.png".into());
        let snap = snapshot(&p).unwrap();
        assert!(snap.positive_prompt.is_empty());
        assert_eq!(snap.negative_prompt, "blurry");
        assert_eq!(snap.input_image, None);
        assert!(!snap.style_ref_enabled);
        assert_eq!(snap.style_ref_image_2, None);
        assert_eq!(snap.seed, -1);
    }

    #[test]
    fn only_plain_txt2img_is_remembered() {
        let mut p = base();
        p.mode = "img2img".into();
        assert!(snapshot(&p).is_none());
        let mut p = base();
        p.pause_at_step = Some(10);
        assert!(snapshot(&p).is_none());
        let mut p = base();
        p.checkpoint.clear();
        assert!(snapshot(&p).is_none());
    }

    #[test]
    fn split_models_are_keyed_by_the_diffusion_model() {
        let mut p = base();
        p.use_split_model = true;
        p.diffusion_model = Some("flux2_klein.safetensors".into());
        assert_eq!(model_key(&p), "flux2_klein.safetensors");
    }

    #[test]
    fn find_matches_names_loosely_but_not_ambiguously() {
        let setups = vec![
            setup("sdxl/illustrious_v2.safetensors"),
            setup("anima_preview.safetensors"),
            setup("anima_final.safetensors"),
        ];
        assert_eq!(find(&setups, None).unwrap().model, setups[0].model);
        assert_eq!(
            find(&setups, Some("illustrious_v2")).unwrap().model,
            setups[0].model
        );
        assert_eq!(
            find(&setups, Some("ANIMA_FINAL.safetensors"))
                .unwrap()
                .model,
            setups[2].model
        );
        assert_eq!(find(&setups, Some("illus")).unwrap().model, setups[0].model);
        assert!(find(&setups, Some("anima"))
            .unwrap_err()
            .contains("several"));
        assert!(find(&setups, Some("pony")).is_err());
        assert!(find(&[], None).is_err());
    }

    #[test]
    fn apply_changes_only_what_was_asked() {
        let o = Overrides {
            prompt: "  a dog  ".into(),
            width: Some(1000),
            seed: Some(-5),
            ..Default::default()
        };
        let p = apply(&base(), &o).unwrap();
        assert_eq!(p.positive_prompt, "a dog");
        assert_eq!(p.negative_prompt, "blurry");
        assert_eq!(p.width, 1008);
        assert_eq!(p.height, 1216);
        assert_eq!(p.steps, 28);
        assert_eq!(p.seed, -1);
    }

    #[test]
    fn apply_rejects_bad_values() {
        let bad = |o: Overrides| apply(&base(), &o).is_err();
        assert!(bad(Overrides::default()));
        let p = || Overrides {
            prompt: "x".into(),
            ..Default::default()
        };
        assert!(bad(Overrides {
            steps: Some(0),
            ..p()
        }));
        assert!(bad(Overrides {
            cfg: Some(31.0),
            ..p()
        }));
        assert!(bad(Overrides {
            batch_size: Some(9),
            ..p()
        }));
        assert!(bad(Overrides {
            loras: Some(vec![LoraOverride::default()]),
            ..p()
        }));
    }

    #[test]
    fn sides_snap_to_sixteen_within_bounds() {
        assert_eq!(snap_side(1), MIN_SIDE);
        assert_eq!(snap_side(100_000), MAX_SIDE);
        assert_eq!(snap_side(1024), 1024);
        assert_eq!(snap_side(1031), 1024);
        assert_eq!(snap_side(1033), 1040);
    }

    #[test]
    fn lora_clip_strength_defaults_to_model_strength() {
        let o = Overrides {
            prompt: "x".into(),
            loras: Some(vec![LoraOverride {
                name: "style.safetensors".into(),
                strength: Some(0.7),
                strength_clip: None,
            }]),
            ..Default::default()
        };
        let p = apply(&base(), &o).unwrap();
        assert_eq!(p.loras.len(), 1);
        assert_eq!(p.loras[0].strength_clip, 0.7);
    }

    #[test]
    fn metadata_uses_the_frontend_keys() {
        let m = image_metadata(&base(), 42);
        assert_eq!(m["positive_prompt"], "a cat");
        assert_eq!(m["seed"], "42");
        assert_eq!(m["size"], "832x1216");
        assert_eq!(m["model"], "sdxl/illustrious_v2.safetensors");
        assert_eq!(m["mooshie_source"], "mcp");
    }
}
