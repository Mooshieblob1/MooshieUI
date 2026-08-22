//! Pure construction of the NovelAI `/ai/generate-image` request body.
//!
//! This module deliberately takes a lean [`PayloadInput`] rather than the full
//! `GenerationParams`, so every branch here is testable without building a
//! hundred-field struct. `mod.rs` owns the mapping from `GenerationParams`.

use serde_json::{json, Map, Value};

use super::models::{self, NovelAiModel};
use super::params::NovelAiParams;

/// The generic half of a generation, extracted from `GenerationParams`.
#[derive(Debug, Clone, Default)]
pub struct PayloadInput {
    pub positive_prompt: String,
    pub negative_prompt: String,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub cfg: f64,
    pub seed: i64,
    pub sampler: String,
    /// Images per request. NovelAI bills each one.
    pub n_samples: u32,
    /// Base64 PNG for img2img and infill.
    pub input_image: Option<String>,
    /// Base64 PNG mask for infill.
    pub mask_image: Option<String>,
}

/// Build the full request body for a NovelAI image generation.
pub fn build(
    input: &PayloadInput,
    nai: &NovelAiParams,
    model: &NovelAiModel,
) -> Result<Value, String> {
    let action = normalise_action(&nai.action, input);
    let mut parameters = Map::new();

    parameters.insert("params_version".into(), json!(3));
    parameters.insert("width".into(), json!(input.width));
    parameters.insert("height".into(), json!(input.height));
    parameters.insert("scale".into(), json!(input.cfg));
    parameters.insert("sampler".into(), json!(input.sampler));
    parameters.insert("steps".into(), json!(input.steps));
    parameters.insert("n_samples".into(), json!(input.n_samples.clamp(1, 8)));
    parameters.insert("seed".into(), json!(input.seed));
    parameters.insert("ucPreset".into(), json!(nai.uc_preset));
    parameters.insert("qualityToggle".into(), json!(nai.quality_toggle));
    parameters.insert(
        "dynamic_thresholding".into(),
        json!(nai.dynamic_thresholding),
    );
    parameters.insert("cfg_rescale".into(), json!(nai.cfg_rescale));
    parameters.insert("noise_schedule".into(), json!(nai.noise_schedule));
    parameters.insert("uncond_scale".into(), json!(nai.uncond_scale));
    parameters.insert("controlnet_strength".into(), json!(1.0));
    parameters.insert("legacy".into(), json!(false));
    parameters.insert("legacy_v3_extend".into(), json!(false));
    parameters.insert("prefer_brownian".into(), json!(true));
    parameters.insert("deliberate_euler_ancestral_bug".into(), json!(false));
    parameters.insert("negative_prompt".into(), json!(input.negative_prompt));

    // "Variety+" disables CFG for the earliest, highest-noise steps. NovelAI
    // derives the cutoff from the pixel count rather than exposing a slider.
    parameters.insert(
        "skip_cfg_above_sigma".into(),
        if nai.variety_plus {
            json!(variety_plus_sigma(input.width, input.height))
        } else {
            Value::Null
        },
    );

    let characters = nai.active_characters();
    parameters.insert("use_coords".into(), json!(nai.use_coords));

    if model.v4_prompt {
        parameters.insert(
            "v4_prompt".into(),
            json!({
                "caption": {
                    "base_caption": input.positive_prompt,
                    "char_captions": characters
                        .iter()
                        .map(|c| json!({
                            "char_caption": c.prompt,
                            "centers": [{ "x": c.center.x, "y": c.center.y }],
                        }))
                        .collect::<Vec<_>>(),
                },
                "use_coords": nai.use_coords,
                "use_order": true,
            }),
        );
        parameters.insert(
            "v4_negative_prompt".into(),
            json!({
                "caption": {
                    "base_caption": input.negative_prompt,
                    "char_captions": characters
                        .iter()
                        .map(|c| json!({
                            "char_caption": c.negative_prompt,
                            "centers": [{ "x": c.center.x, "y": c.center.y }],
                        }))
                        .collect::<Vec<_>>(),
                },
                "legacy_uc": nai.legacy_uc,
            }),
        );
        // The flat mirror of the same data. NovelAI's own client sends both.
        parameters.insert(
            "characterPrompts".into(),
            json!(characters
                .iter()
                .map(|c| json!({
                    "prompt": c.prompt,
                    "uc": c.negative_prompt,
                    "center": { "x": c.center.x, "y": c.center.y },
                    "enabled": true,
                }))
                .collect::<Vec<_>>()),
        );
    }

    apply_vibes(&mut parameters, nai, model);
    apply_director_references(&mut parameters, nai, model);
    apply_image_action(&mut parameters, input, nai, &action)?;

    Ok(json!({
        "input": input.positive_prompt,
        "model": models::resolve_id(model, &action),
        "action": action,
        "parameters": Value::Object(parameters),
    }))
}

/// An action is only as good as the images backing it. A client that asks for
/// img2img without an image would otherwise burn Anlas on a 400.
fn normalise_action(action: &str, input: &PayloadInput) -> String {
    match action {
        "infill" if input.input_image.is_some() && input.mask_image.is_some() => "infill".into(),
        "img2img" | "infill" if input.input_image.is_some() => "img2img".into(),
        _ => "generate".into(),
    }
}

/// NovelAI scales the Variety+ cutoff with the diagonal of the latent, so a
/// larger canvas keeps CFG suppressed for longer.
fn variety_plus_sigma(width: u32, height: u32) -> f64 {
    let w = (width.max(1) / 8) as f64;
    let h = (height.max(1) / 8) as f64;
    19.0 * ((w * h) / (128.0 * 128.0)).sqrt()
}

fn apply_vibes(parameters: &mut Map<String, Value>, nai: &NovelAiParams, model: &NovelAiModel) {
    if !model.vibe_transfer || nai.vibes.is_empty() {
        return;
    }
    let mut refs = Vec::new();
    let mut extracted = Vec::new();
    let mut strengths = Vec::new();
    for vibe in &nai.vibes {
        // A cached encoding costs nothing; a raw image is encoded (and billed)
        // by NovelAI on arrival.
        let payload = vibe
            .encoding
            .as_ref()
            .filter(|s| !s.is_empty())
            .or(vibe.image.as_ref().filter(|s| !s.is_empty()));
        let Some(payload) = payload else { continue };
        refs.push(json!(payload));
        extracted.push(json!(vibe.information_extracted));
        strengths.push(json!(vibe.strength));
    }
    if refs.is_empty() {
        return;
    }
    parameters.insert("reference_image_multiple".into(), json!(refs));
    parameters.insert(
        "reference_information_extracted_multiple".into(),
        json!(extracted),
    );
    parameters.insert("reference_strength_multiple".into(), json!(strengths));
}

fn apply_director_references(
    parameters: &mut Map<String, Value>,
    nai: &NovelAiParams,
    model: &NovelAiModel,
) {
    if !model.precise_reference || nai.director_references.is_empty() {
        return;
    }
    // NovelAI rejects a request carrying both systems, so the UI makes them
    // mutually exclusive and the payload enforces the same precedence.
    parameters.remove("reference_image_multiple");
    parameters.remove("reference_information_extracted_multiple");
    parameters.remove("reference_strength_multiple");

    let active: Vec<_> = nai
        .director_references
        .iter()
        .filter(|r| !r.image.is_empty())
        .collect();
    if active.is_empty() {
        return;
    }
    parameters.insert(
        "director_reference_images".into(),
        json!(active.iter().map(|r| json!(r.image)).collect::<Vec<_>>()),
    );
    parameters.insert(
        "director_reference_descriptions".into(),
        json!(active
            .iter()
            .map(|r| json!({
                "caption": {
                    "base_caption": r.description,
                    "char_captions": [],
                },
                "legacy_uc": false,
            }))
            .collect::<Vec<_>>()),
    );
    parameters.insert(
        "director_reference_information_extracted".into(),
        json!(active
            .iter()
            .map(|r| json!(r.information_extracted))
            .collect::<Vec<_>>()),
    );
    parameters.insert(
        "director_reference_strength_values".into(),
        json!(active.iter().map(|r| json!(r.strength)).collect::<Vec<_>>()),
    );
}

fn apply_image_action(
    parameters: &mut Map<String, Value>,
    input: &PayloadInput,
    nai: &NovelAiParams,
    action: &str,
) -> Result<(), String> {
    if action == "generate" {
        return Ok(());
    }
    let image = input
        .input_image
        .as_ref()
        .ok_or("NovelAI img2img requires an input image")?;
    parameters.insert("image".into(), json!(strip_data_url(image)));
    parameters.insert("strength".into(), json!(nai.strength));
    parameters.insert("noise".into(), json!(nai.noise));
    parameters.insert("extra_noise_seed".into(), json!(input.seed));

    if action == "infill" {
        let mask = input
            .mask_image
            .as_ref()
            .ok_or("NovelAI infill requires a mask")?;
        parameters.insert("mask".into(), json!(strip_data_url(mask)));
        parameters.insert("add_original_image".into(), json!(nai.add_original_image));
    }
    Ok(())
}

/// The frontend may send either a bare base64 blob or a full data URL.
fn strip_data_url(s: &str) -> &str {
    s.split_once("base64,").map_or(s, |(_, rest)| rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::params::{
        NovelAiCharacter, NovelAiCoord, NovelAiDirectorReference, NovelAiVibe,
    };

    fn input() -> PayloadInput {
        PayloadInput {
            positive_prompt: "1girl, solo".into(),
            negative_prompt: "lowres".into(),
            width: 832,
            height: 1216,
            steps: 23,
            cfg: 7.0,
            seed: 42,
            sampler: "k_euler_ancestral".into(),
            n_samples: 1,
            ..Default::default()
        }
    }

    fn nai() -> NovelAiParams {
        NovelAiParams {
            model: "nai-diffusion-4-5-full".into(),
            action: "generate".into(),
            noise_schedule: "karras".into(),
            uncond_scale: 1.0,
            quality_toggle: true,
            add_original_image: true,
            strength: 0.7,
            ..Default::default()
        }
    }

    fn v45() -> &'static NovelAiModel {
        models::find("nai-diffusion-4-5-full").unwrap()
    }

    #[test]
    fn builds_the_documented_top_level_shape() {
        let body = build(&input(), &nai(), v45()).unwrap();
        assert_eq!(body["input"], "1girl, solo");
        assert_eq!(body["model"], "nai-diffusion-4-5-full");
        assert_eq!(body["action"], "generate");
        let p = &body["parameters"];
        assert_eq!(p["steps"], 23);
        assert_eq!(p["scale"], 7.0);
        assert_eq!(p["sampler"], "k_euler_ancestral");
        assert_eq!(p["noise_schedule"], "karras");
        assert_eq!(p["seed"], 42);
        assert_eq!(p["n_samples"], 1);
    }

    #[test]
    fn character_captions_carry_centres_and_negatives() {
        let mut n = nai();
        n.use_coords = true;
        n.characters = vec![
            NovelAiCharacter {
                prompt: "girl, red hair".into(),
                negative_prompt: "hat".into(),
                center: NovelAiCoord::from_grid(0, 2),
                enabled: true,
            },
            NovelAiCharacter {
                prompt: "boy".into(),
                negative_prompt: String::new(),
                center: NovelAiCoord::from_grid(4, 2),
                enabled: true,
            },
        ];
        let body = build(&input(), &n, v45()).unwrap();
        let caps = &body["parameters"]["v4_prompt"]["caption"]["char_captions"];
        assert_eq!(caps.as_array().unwrap().len(), 2);
        assert_eq!(caps[0]["char_caption"], "girl, red hair");
        assert_eq!(caps[0]["centers"][0]["x"], 0.1);
        assert_eq!(caps[0]["centers"][0]["y"], 0.5);
        assert_eq!(caps[1]["centers"][0]["x"], 0.9);

        let negs = &body["parameters"]["v4_negative_prompt"]["caption"]["char_captions"];
        assert_eq!(negs[0]["char_caption"], "hat");
        assert_eq!(body["parameters"]["use_coords"], true);
    }

    #[test]
    fn disabled_characters_never_reach_the_payload() {
        let mut n = nai();
        n.characters = vec![NovelAiCharacter {
            prompt: "ghost".into(),
            enabled: false,
            ..Default::default()
        }];
        let body = build(&input(), &n, v45()).unwrap();
        let caps = &body["parameters"]["v4_prompt"]["caption"]["char_captions"];
        assert!(caps.as_array().unwrap().is_empty());
    }

    #[test]
    fn vibes_are_dropped_on_models_that_lack_support() {
        let mut n = nai();
        n.vibes = vec![NovelAiVibe {
            encoding: Some("cached".into()),
            strength: 0.6,
            information_extracted: 1.0,
            ..Default::default()
        }];
        let v5 = models::find("nai-diffusion-5-full").unwrap();
        let body = build(&input(), &n, v5).unwrap();
        assert!(body["parameters"].get("reference_image_multiple").is_none());

        let body = build(&input(), &n, v45()).unwrap();
        assert_eq!(body["parameters"]["reference_image_multiple"][0], "cached");
        assert_eq!(body["parameters"]["reference_strength_multiple"][0], 0.6);
    }

    #[test]
    fn precise_reference_wins_over_vibes() {
        let mut n = nai();
        n.vibes = vec![NovelAiVibe {
            encoding: Some("cached".into()),
            ..Default::default()
        }];
        n.director_references = vec![NovelAiDirectorReference {
            image: "refpng".into(),
            description: "character&style".into(),
            information_extracted: 1.0,
            strength: 0.8,
        }];
        let body = build(&input(), &n, v45()).unwrap();
        let p = &body["parameters"];
        assert!(p.get("reference_image_multiple").is_none());
        assert_eq!(p["director_reference_images"][0], "refpng");
        assert_eq!(
            p["director_reference_descriptions"][0]["caption"]["base_caption"],
            "character&style"
        );
        assert_eq!(p["director_reference_strength_values"][0], 0.8);
    }

    #[test]
    fn img2img_without_an_image_degrades_to_generate() {
        let mut n = nai();
        n.action = "img2img".into();
        let body = build(&input(), &n, v45()).unwrap();
        assert_eq!(body["action"], "generate");
        assert!(body["parameters"].get("image").is_none());
    }

    #[test]
    fn img2img_carries_strength_and_extra_noise_seed() {
        let mut n = nai();
        n.action = "img2img".into();
        n.strength = 0.55;
        let mut i = input();
        i.input_image = Some("data:image/png;base64,AAAA".into());
        let body = build(&i, &n, v45()).unwrap();
        assert_eq!(body["action"], "img2img");
        assert_eq!(body["parameters"]["image"], "AAAA");
        assert_eq!(body["parameters"]["strength"], 0.55);
        assert_eq!(body["parameters"]["extra_noise_seed"], 42);
    }

    #[test]
    fn infill_swaps_the_model_and_sends_the_mask() {
        let mut n = nai();
        n.action = "infill".into();
        let mut i = input();
        i.input_image = Some("AAAA".into());
        i.mask_image = Some("BBBB".into());
        let body = build(&i, &n, v45()).unwrap();
        assert_eq!(body["action"], "infill");
        assert_eq!(body["model"], "nai-diffusion-4-5-full-inpainting");
        assert_eq!(body["parameters"]["mask"], "BBBB");
        assert_eq!(body["parameters"]["add_original_image"], true);
    }

    #[test]
    fn infill_without_a_mask_falls_back_to_img2img() {
        let mut n = nai();
        n.action = "infill".into();
        let mut i = input();
        i.input_image = Some("AAAA".into());
        let body = build(&i, &n, v45()).unwrap();
        assert_eq!(body["action"], "img2img");
        assert_eq!(body["model"], "nai-diffusion-4-5-full");
    }

    #[test]
    fn variety_plus_sets_a_sigma_only_when_enabled() {
        let body = build(&input(), &nai(), v45()).unwrap();
        assert!(body["parameters"]["skip_cfg_above_sigma"].is_null());

        let mut n = nai();
        n.variety_plus = true;
        let body = build(&input(), &n, v45()).unwrap();
        assert!(body["parameters"]["skip_cfg_above_sigma"].as_f64().unwrap() > 0.0);
    }
}
