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
    // Transparency first, text second, so the `Text:` block ends up last: the
    // API wants it at the very end of the prompt, after every tag.
    let positive_prompt = with_transparency(&input.positive_prompt, nai, model);
    let positive_prompt = with_text_blocks(&positive_prompt, model);
    let mut parameters = Map::new();

    parameters.insert("params_version".into(), json!(model.params_version));
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

    // Capped at what the model seats (22 on V5, 6 before it): NovelAI's API
    // past the cap is untested, and a 400 there costs the user a paid request.
    let mut characters = nai.active_characters();
    characters.truncate(model.max_characters);
    parameters.insert("use_coords".into(), json!(nai.use_coords));

    if model.v4_prompt {
        parameters.insert(
            "v4_prompt".into(),
            json!({
                "caption": {
                    "base_caption": positive_prompt,
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
                // Lives in BOTH prompt blocks, not just the negative one; the
                // V5 reference implementations added it here in their V5
                // commits.
                "legacy_uc": nai.legacy_uc,
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
                // Placement belongs to the positive block alone; the
                // references pin both flags false here rather than omitting
                // them.
                "use_coords": false,
                "use_order": false,
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
        "input": positive_prompt,
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

/// The tag NovelAI's own "Transparent BG" button inserts, at the weight their
/// release notes recommend for a clean cut-out.
const TRANSPARENCY_TAG: &str = "2.1::transparent background::";

/// Append the transparency tag when the toggle is on and the model can honour
/// it.
///
/// NovelAI ships transparency as a prompt tag rather than a request field, and
/// only the V5 VAE emits an alpha channel, so an older model would just spend
/// Anlas on a picture of a checkerboard. The tag is kept out of the user's
/// prompt box on purpose: the toggle owns it, so turning the toggle off takes
/// it away again cleanly.
fn with_transparency(prompt: &str, nai: &NovelAiParams, model: &NovelAiModel) -> String {
    if !nai.transparent_background || !model.alpha {
        return prompt.to_string();
    }
    // A user who typed the tag themselves already has what the toggle adds,
    // and a second copy would only fight the first one's weight.
    if prompt.to_lowercase().contains("transparent background") {
        return prompt.to_string();
    }
    let trimmed = prompt.trim_end().trim_end_matches(',').trim_end();
    if trimmed.is_empty() {
        return TRANSPARENCY_TAG.to_string();
    }
    format!("{trimmed}, {TRANSPARENCY_TAG}")
}

/// Quote pairs that mark text to be rendered in the image. NovelAI's V5
/// announcement shows CJK corner brackets and typographic double quotes; the
/// straight ASCII quote is what most people type, so it pairs up too (first
/// one opens, the next one closes).
const TEXT_QUOTE_PAIRS: &[(char, char)] = &[('「', '」'), ('“', '”'), ('"', '"')];

/// Auto-format quoted prompt text into a trailing `Text:` block, the way
/// NovelAI's own V5 frontend does.
///
/// The quoted text stays where it was typed as well: NovelAI's docs recommend
/// repeating the text in natural language for reliability, and their client
/// keeps the quote in the prompt when it prepares the block. A `Text:` line the
/// user wrote themselves turns the transform off entirely, which is also
/// NovelAI's behaviour, so power users keep full control of the block.
fn with_text_blocks(prompt: &str, model: &NovelAiModel) -> String {
    if !model.auto_text {
        return prompt.to_string();
    }
    if prompt
        .lines()
        .any(|line| line.trim_start().starts_with("Text:"))
    {
        return prompt.to_string();
    }
    let pieces = extract_quoted(prompt);
    if pieces.is_empty() {
        return prompt.to_string();
    }
    // Multiple pieces are separated by an empty line, per NovelAI's text
    // rendering docs.
    format!("{}\nText: {}", prompt.trim_end(), pieces.join("\n\n"))
}

/// Every properly closed quote in the prompt, in order of appearance.
///
/// An unmatched quote is skipped rather than swallowing the rest of the
/// prompt, and an empty pair contributes nothing.
fn extract_quoted(prompt: &str) -> Vec<String> {
    let chars: Vec<char> = prompt.chars().collect();
    let mut pieces = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some(&(_, close)) = TEXT_QUOTE_PAIRS.iter().find(|(open, _)| *open == chars[i]) {
            if let Some(j) = (i + 1..chars.len()).find(|&j| chars[j] == close) {
                let piece: String = chars[i + 1..j].iter().collect();
                let piece = piece.trim();
                if !piece.is_empty() {
                    pieces.push(piece.to_string());
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    pieces
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
    let mut strengths: Vec<f64> = Vec::new();
    for vibe in &nai.vibes {
        // Every vibe-capable model in the table is V4 or later, and those take
        // an encoded `.naiv4vibe` token here, never a raw image. `mod.rs` runs
        // the encode pass before this, so a vibe with no encoding is one that
        // failed to encode and is dropped rather than sent as a raw image,
        // which NovelAI answers with an opaque 500.
        let Some(encoding) = vibe.encoding.as_ref().filter(|s| !s.is_empty()) else {
            continue;
        };
        refs.push(json!(encoding));
        strengths.push(vibe.strength);
    }
    if refs.is_empty() {
        return;
    }
    if nai.normalize_reference_strength {
        normalize_strengths(&mut strengths);
    }
    parameters.insert("reference_image_multiple".into(), json!(refs));
    parameters.insert("reference_strength_multiple".into(), json!(strengths));
    // `information_extracted` is baked into the token at encode time, so V4
    // takes this flag in place of the per-image list V3 used. NovelAI's team
    // confirmed the backend ignores this key entirely, but their own client
    // sends it, so it is kept for request parity rather than for effect.
    parameters.insert(
        "normalize_reference_strength_multiple".into(),
        json!(nai.normalize_reference_strength),
    );
}

/// Scale the strengths so they sum to 1.
///
/// This is client-side work. `normalize_reference_strength_multiple` reaches
/// NovelAI untouched but the backend does nothing with it (confirmed by their
/// team); the official client divides the strengths itself before sending and
/// keeps the checkbox as local state. Doing it anywhere later would be wrong,
/// because the sliders are meant to keep showing the raw values the user set.
///
/// A sum of zero is left alone rather than divided by, and so is a set that
/// already sums to 1, which keeps two vibes at 0.5 a genuine no-op instead of
/// a round-trip through floating point.
fn normalize_strengths(strengths: &mut [f64]) {
    let total: f64 = strengths.iter().sum();
    if total <= 0.0 || (total - 1.0).abs() < f64::EPSILON {
        return;
    }
    for s in strengths.iter_mut() {
        *s /= total;
    }
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
    parameters.remove("reference_strength_multiple");
    parameters.remove("normalize_reference_strength_multiple");

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
///
/// `pub(super)` because the standalone upscaler takes the same field from the
/// same frontend and has to strip it the same way, without building a payload.
pub(super) fn strip_data_url(s: &str) -> &str {
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
    fn params_version_follows_the_model() {
        // V5 endpoints declare params_version 4 in NovelAI's OpenAPI schema;
        // V4/V4.5 stay on 3.
        let v5 = models::find("nai-diffusion-5-full").unwrap();
        let body = build(&input(), &nai(), v5).unwrap();
        assert_eq!(body["parameters"]["params_version"], 4);

        let body = build(&input(), &nai(), v45()).unwrap();
        assert_eq!(body["parameters"]["params_version"], 3);
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
    fn prompt_blocks_carry_the_reference_flag_set() {
        // Mirrors the V5 reference implementations (raspie10032, caru-ini):
        // `legacy_uc` lives inside BOTH prompt blocks, and the negative block
        // pins `use_coords`/`use_order` to false.
        let mut n = nai();
        n.use_coords = true;
        n.characters = vec![NovelAiCharacter {
            prompt: "girl".into(),
            center: NovelAiCoord { x: 0.1, y: 0.5 },
            enabled: true,
            ..Default::default()
        }];
        let v5 = models::find("nai-diffusion-5-full").unwrap();
        let body = build(&input(), &n, v5).unwrap();
        let pos = &body["parameters"]["v4_prompt"];
        assert_eq!(pos["use_coords"], true);
        assert_eq!(pos["use_order"], true);
        assert_eq!(pos["legacy_uc"], false);
        let neg = &body["parameters"]["v4_negative_prompt"];
        assert_eq!(neg["use_coords"], false);
        assert_eq!(neg["use_order"], false);
        assert_eq!(neg["legacy_uc"], false);
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
    fn an_unencoded_vibe_is_dropped_rather_than_sent_raw() {
        // V4 answers a raw reference image with an opaque 500, so a vibe
        // whose encode pass did not run must not reach the wire at all.
        let mut n = nai();
        n.vibes = vec![
            NovelAiVibe {
                image: Some("rawpng".into()),
                strength: 0.5,
                ..Default::default()
            },
            NovelAiVibe {
                encoding: Some("token".into()),
                strength: 0.7,
                ..Default::default()
            },
        ];
        let body = build(&input(), &n, v45()).unwrap();
        let p = &body["parameters"];
        assert_eq!(p["reference_image_multiple"].as_array().unwrap().len(), 1);
        assert_eq!(p["reference_image_multiple"][0], "token");
        assert_eq!(p["reference_strength_multiple"][0], 0.7);
    }

    #[test]
    fn vibes_use_the_v4_key_set() {
        // `information_extracted` is baked into the token at encode time, so
        // V4 has no per-image list for it and takes a normalise flag instead.
        let mut n = nai();
        n.normalize_reference_strength = true;
        n.vibes = vec![NovelAiVibe {
            encoding: Some("token".into()),
            ..Default::default()
        }];
        let body = build(&input(), &n, v45()).unwrap();
        let p = &body["parameters"];
        assert_eq!(p["normalize_reference_strength_multiple"], true);
        assert!(p.get("reference_information_extracted_multiple").is_none());
    }

    #[test]
    fn normalize_scales_the_strengths_client_side() {
        // NovelAI's backend ignores the flag; the official client divides the
        // strengths itself. Two vibes at 1.0 have to leave here as 0.5 each.
        let mut n = nai();
        n.normalize_reference_strength = true;
        n.vibes = vec![
            NovelAiVibe {
                encoding: Some("a".into()),
                strength: 1.0,
                ..Default::default()
            },
            NovelAiVibe {
                encoding: Some("b".into()),
                strength: 1.0,
                ..Default::default()
            },
        ];
        let body = build(&input(), &n, v45()).unwrap();
        assert_eq!(
            body["parameters"]["reference_strength_multiple"],
            json!([0.5, 0.5])
        );
    }

    #[test]
    fn normalize_off_leaves_the_strengths_alone() {
        let mut n = nai();
        n.normalize_reference_strength = false;
        n.vibes = vec![
            NovelAiVibe {
                encoding: Some("a".into()),
                strength: 1.0,
                ..Default::default()
            },
            NovelAiVibe {
                encoding: Some("b".into()),
                strength: 1.0,
                ..Default::default()
            },
        ];
        let body = build(&input(), &n, v45()).unwrap();
        assert_eq!(
            body["parameters"]["reference_strength_multiple"],
            json!([1.0, 1.0])
        );
    }

    #[test]
    fn normalize_is_a_no_op_when_the_strengths_already_sum_to_one() {
        let mut n = nai();
        n.normalize_reference_strength = true;
        n.vibes = vec![
            NovelAiVibe {
                encoding: Some("a".into()),
                strength: 0.5,
                ..Default::default()
            },
            NovelAiVibe {
                encoding: Some("b".into()),
                strength: 0.5,
                ..Default::default()
            },
        ];
        let body = build(&input(), &n, v45()).unwrap();
        assert_eq!(
            body["parameters"]["reference_strength_multiple"],
            json!([0.5, 0.5])
        );
    }

    #[test]
    fn normalize_survives_an_all_zero_set() {
        // Dividing by the sum would produce NaN, which serialises to null and
        // would make NovelAI reject the whole request.
        let mut n = nai();
        n.normalize_reference_strength = true;
        n.vibes = vec![NovelAiVibe {
            encoding: Some("a".into()),
            strength: 0.0,
            ..Default::default()
        }];
        let body = build(&input(), &n, v45()).unwrap();
        assert_eq!(
            body["parameters"]["reference_strength_multiple"],
            json!([0.0])
        );
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

    fn v5() -> &'static NovelAiModel {
        models::find("nai-diffusion-5-full").unwrap()
    }

    #[test]
    fn transparency_appends_the_tag_to_both_prompt_copies() {
        let mut n = nai();
        n.transparent_background = true;
        let body = build(&input(), &n, v5()).unwrap();
        let expected = "1girl, solo, 2.1::transparent background::";
        assert_eq!(body["input"], expected);
        assert_eq!(
            body["parameters"]["v4_prompt"]["caption"]["base_caption"],
            expected
        );
        // The negative prompt is left alone.
        assert_eq!(body["parameters"]["negative_prompt"], "lowres");
    }

    #[test]
    fn transparency_is_off_by_default() {
        let body = build(&input(), &nai(), v5()).unwrap();
        assert_eq!(body["input"], "1girl, solo");
    }

    #[test]
    fn transparency_is_ignored_by_models_without_an_alpha_channel() {
        // V4.5's VAE has no alpha channel, so the tag would only cost Anlas
        // for a picture of a checkerboard.
        let mut n = nai();
        n.transparent_background = true;
        let body = build(&input(), &n, v45()).unwrap();
        assert_eq!(body["input"], "1girl, solo");
    }

    #[test]
    fn transparency_does_not_double_up_a_tag_the_user_typed() {
        let mut n = nai();
        n.transparent_background = true;
        let mut i = input();
        i.positive_prompt = "1girl, Transparent Background".into();
        let body = build(&i, &n, v5()).unwrap();
        assert_eq!(body["input"], "1girl, Transparent Background");
    }

    #[test]
    fn transparency_tidies_the_join_and_survives_an_empty_prompt() {
        let mut n = nai();
        n.transparent_background = true;
        let mut i = input();
        i.positive_prompt = "1girl,  ".into();
        assert_eq!(
            build(&i, &n, v5()).unwrap()["input"],
            "1girl, 2.1::transparent background::"
        );

        i.positive_prompt = String::new();
        assert_eq!(
            build(&i, &n, v5()).unwrap()["input"],
            "2.1::transparent background::"
        );
    }

    #[test]
    fn quoted_text_appends_a_text_block_on_v5() {
        let mut i = input();
        i.positive_prompt = "1girl, sign saying \"hello world\"".into();
        let body = build(&i, &nai(), v5()).unwrap();
        let expected = "1girl, sign saying \"hello world\"\nText: hello world";
        assert_eq!(body["input"], expected);
        // The structured caption carries the same transformed prompt.
        assert_eq!(
            body["parameters"]["v4_prompt"]["caption"]["base_caption"],
            expected
        );
    }

    #[test]
    fn text_blocks_are_not_prepared_for_pre_v5_models() {
        let mut i = input();
        i.positive_prompt = "sign saying \"hello\"".into();
        let body = build(&i, &nai(), v45()).unwrap();
        assert_eq!(body["input"], "sign saying \"hello\"");
    }

    #[test]
    fn a_manual_text_block_disables_the_auto_formatting() {
        // Matching NovelAI's client: writing `Text:` yourself takes over.
        let mut i = input();
        i.positive_prompt = "sign saying \"hello\"\nText: HELLO".into();
        let body = build(&i, &nai(), v5()).unwrap();
        assert_eq!(body["input"], "sign saying \"hello\"\nText: HELLO");
    }

    #[test]
    fn multiple_quotes_are_separated_by_an_empty_line() {
        let mut i = input();
        i.positive_prompt = "poster \"BIG SALE\" and a tag saying \"50% off\"".into();
        let body = build(&i, &nai(), v5()).unwrap();
        assert_eq!(
            body["input"],
            "poster \"BIG SALE\" and a tag saying \"50% off\"\nText: BIG SALE\n\n50% off"
        );
    }

    #[test]
    fn cjk_and_typographic_quotes_are_matched_too() {
        let mut i = input();
        i.positive_prompt = "看板「こんにちは」, banner “Welcome”".into();
        let body = build(&i, &nai(), v5()).unwrap();
        assert_eq!(
            body["input"],
            "看板「こんにちは」, banner “Welcome”\nText: こんにちは\n\nWelcome"
        );
    }

    #[test]
    fn unmatched_and_empty_quotes_add_no_block() {
        // Straight quotes toggle-scan, so an empty pair contributes nothing
        // and a final quote with no partner is skipped, not left swallowing
        // the rest of the prompt.
        let mut i = input();
        i.positive_prompt = "empty \"\" pair, one lone \" quote".into();
        let body = build(&i, &nai(), v5()).unwrap();
        assert_eq!(body["input"], "empty \"\" pair, one lone \" quote");
    }

    #[test]
    fn text_block_lands_after_the_transparency_tag() {
        let mut n = nai();
        n.transparent_background = true;
        let mut i = input();
        i.positive_prompt = "sticker saying \"nya\"".into();
        let body = build(&i, &n, v5()).unwrap();
        assert_eq!(
            body["input"],
            "sticker saying \"nya\", 2.1::transparent background::\nText: nya"
        );
    }

    #[test]
    fn characters_are_truncated_to_the_model_cap() {
        let mut n = nai();
        n.characters = (0..8)
            .map(|idx| NovelAiCharacter {
                prompt: format!("char {idx}"),
                enabled: true,
                ..Default::default()
            })
            .collect();
        // V4.5 seats 6; the two extra slots must not reach the wire.
        let body = build(&input(), &n, v45()).unwrap();
        let caps = &body["parameters"]["v4_prompt"]["caption"]["char_captions"];
        assert_eq!(caps.as_array().unwrap().len(), 6);
        assert_eq!(
            body["parameters"]["characterPrompts"]
                .as_array()
                .unwrap()
                .len(),
            6
        );
        // V5 seats 22, so all eight survive there.
        let body = build(&input(), &n, v5()).unwrap();
        let caps = &body["parameters"]["v4_prompt"]["caption"]["char_captions"];
        assert_eq!(caps.as_array().unwrap().len(), 8);
    }
}
