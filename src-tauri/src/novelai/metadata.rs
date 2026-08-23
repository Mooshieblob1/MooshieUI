//! Read NovelAI's own image metadata back into the app's flat parameter map.
//!
//! Every PNG NovelAI returns already describes how it was made, in a format
//! nothing else uses: a set of named PNG text chunks (`Title`, `Description`,
//! `Software`, `Source`, `Comment`) rather than the single `parameters` chunk
//! A1111 and this app write. `Comment` holds the real payload as JSON.
//!
//! That is what makes copy-paste interop work in one direction. A user can copy
//! an image straight off novelai.net, press Ctrl+V in the app, and have the
//! prompt, seed and sampler land in the panel, because the image carries them.
//! The reverse direction needs no code at all: a pure NovelAI generation is
//! written to disk byte for byte (see `save_to_gallery_inner`), so copying it
//! back out hands NovelAI the exact file it produced, signature intact.
//!
//! Only genuine NovelAI images are read here: the entry points bail unless the
//! `Software` or `Source` chunk says so, and every other reader is tried first.
//!
//! # What is deliberately approximate
//!
//! - **The model.** `Source` names a version and a checksum ("NovelAI Diffusion
//!   V4.5 4BDE2A90"), not the API model id, and it does not distinguish Full
//!   from Curated. The version maps to the Full id, and the raw string is kept
//!   in `mooshie_novelai_source` so nothing is lost.
//! - **Quality tags and the UC preset.** NovelAI bakes both into the prompt and
//!   `uc` text before saving, so the captured prompt already contains them.
//!   Both toggles are therefore restored as off, or the app would append a
//!   second copy on the next generation.

use std::collections::HashMap;

use super::prompt_syntax;

/// The chunk NovelAI stamps its name into, and the value it writes.
const SOFTWARE_CHUNK: &str = "Software";
const SOFTWARE_VALUE: &str = "novelai";

/// Does this set of PNG text chunks come from NovelAI?
///
/// `Software` is the documented marker. `Source` is accepted as a fallback
/// because a few older exports carry the version string without `Software`.
pub fn is_novelai_chunks(chunks: &HashMap<String, String>) -> bool {
    let says_novelai = |key: &str| {
        chunks
            .get(key)
            .is_some_and(|v| v.to_ascii_lowercase().contains(SOFTWARE_VALUE))
    };
    says_novelai(SOFTWARE_CHUNK) || says_novelai("Source")
}

/// Parse NovelAI PNG text chunks into the app's flat metadata map.
///
/// Returns `None` for anything that is not a NovelAI image, so this can be
/// wired in as a fallback after every other reader has declined.
pub fn parse_chunks(chunks: &HashMap<String, String>) -> Option<HashMap<String, String>> {
    if !is_novelai_chunks(chunks) {
        return None;
    }

    let comment: serde_json::Value = chunks
        .get("Comment")
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(serde_json::Value::Null);
    let comment = comment.as_object();

    let mut params: HashMap<String, String> = HashMap::new();

    // Every image read through here came from NovelAI, whatever happens to the
    // fields below. This is the flag the frontend switches on, and the one a
    // re-imported post-processed image is stamped with on the way out.
    params.insert("mooshie_backend".into(), "novelai".into());

    // The V4 structured prompt block is authoritative when present: `prompt`
    // is a flattened rendering of it that folds the character prompts in.
    let positive = comment
        .and_then(|c| base_caption(c.get("v4_prompt")))
        .or_else(|| comment.and_then(|c| string_of(c.get("prompt")?)))
        .or_else(|| chunks.get("Description").cloned());
    if let Some(positive) = positive {
        params.insert(
            "positive_prompt".into(),
            prompt_syntax::from_novelai(&positive),
        );
    }

    let negative = comment
        .and_then(|c| base_caption(c.get("v4_negative_prompt")))
        .or_else(|| comment.and_then(|c| string_of(c.get("uc")?)));
    if let Some(negative) = negative {
        params.insert(
            "negative_prompt".into(),
            prompt_syntax::from_novelai(&negative),
        );
    }

    if let Some(comment) = comment {
        let direct: &[(&str, &str)] = &[
            ("steps", "steps"),
            ("scale", "cfg"),
            ("seed", "seed"),
            ("sampler", "sampler"),
            // NovelAI's noise schedule occupies the same slot as a local
            // scheduler: one dropdown, one value written per image.
            ("noise_schedule", "scheduler"),
        ];
        for &(nai_key, internal) in direct {
            if let Some(value) = comment.get(nai_key).and_then(string_of) {
                params.insert(internal.into(), value);
            }
        }

        if let (Some(w), Some(h)) = (
            comment.get("width").and_then(string_of),
            comment.get("height").and_then(string_of),
        ) {
            params.insert("size".into(), format!("{w}x{h}"));
        }

        let extras: &[(&str, &str)] = &[
            ("cfg_rescale", "mooshie_novelai_cfg_rescale"),
            ("uncond_scale", "mooshie_novelai_uncond_scale"),
            (
                "dynamic_thresholding",
                "mooshie_novelai_dynamic_thresholding",
            ),
        ];
        for &(nai_key, internal) in extras {
            if let Some(value) = comment.get(nai_key).and_then(string_of) {
                params.insert(internal.into(), value);
            }
        }

        // Variety+ is not stored as a boolean: NovelAI records the sigma it
        // starts skipping CFG above, and the feature is off when that is null.
        if comment
            .get("skip_cfg_above_sigma")
            .is_some_and(|v| !v.is_null())
        {
            params.insert("mooshie_novelai_variety_plus".into(), "true".into());
        }

        if let Some(use_coords) = comment
            .get("v4_prompt")
            .and_then(|v| v.get("use_coords"))
            .and_then(|v| v.as_bool())
        {
            params.insert("mooshie_novelai_use_coords".into(), use_coords.to_string());
        }

        if let Some(characters) = parse_characters(comment) {
            params.insert("mooshie_novelai_characters".into(), characters);
        }
    }

    // The captured prompt and UC already have the quality tags and the preset
    // text folded in, so re-enabling either toggle would append a second copy.
    params.insert("mooshie_novelai_quality_toggle".into(), "false".into());
    params.insert("mooshie_novelai_uc_preset".into(), "0".into());

    if let Some(source) = chunks.get("Source") {
        params.insert("mooshie_novelai_source".into(), source.clone());
        if let Some(model) = model_id_from_source(source) {
            params.insert("model".into(), model.into());
        }
    }
    if let Some(generation_time) = chunks.get("Generation time") {
        params.insert(
            "mooshie_novelai_generation_time".into(),
            generation_time.clone(),
        );
    }

    Some(params)
}

/// Parse a NovelAI stealth-alpha payload.
///
/// NovelAI hides the same chunk set in the alpha LSBs as well as writing it in
/// the clear, so an image that lost its text chunks to a re-encode that kept
/// the pixels intact can still be read. The payload is a flat JSON object with
/// exactly the chunk names as keys, which is why it reuses [`parse_chunks`].
pub fn parse_stealth_json(text: &str) -> Option<HashMap<String, String>> {
    let root: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = root.as_object()?;
    let chunks: HashMap<String, String> = obj
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), string_of(value)?)))
        .collect();
    parse_chunks(&chunks)
}

/// Pull `caption.base_caption` out of a V4 structured prompt block.
fn base_caption(block: Option<&serde_json::Value>) -> Option<String> {
    block?
        .get("caption")?
        .get("base_caption")?
        .as_str()
        .map(str::to_string)
}

/// Render a JSON scalar as the plain string the flat map holds.
///
/// Objects and arrays are refused rather than stringified: the flat map is
/// consumed as text by the frontend, and a stray `{...}` there would be applied
/// verbatim to a field that expects a number.
fn string_of(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Rebuild the app's character list from the V4 prompt blocks.
///
/// The positive and negative blocks are two parallel arrays in the same order,
/// so they are zipped back together by index. Emitted as JSON in the app's own
/// `NovelAiCharacter` shape so the frontend can apply it without a translation
/// step of its own.
fn parse_characters(comment: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let positives = char_captions(comment.get("v4_prompt"))?;
    let negatives = char_captions(comment.get("v4_negative_prompt")).unwrap_or_default();

    let characters: Vec<serde_json::Value> = positives
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let center = entry
                .get("centers")
                .and_then(|v| v.as_array())
                .and_then(|centers| centers.first())
                .cloned()
                .unwrap_or_else(|| serde_json::json!({ "x": 0.5, "y": 0.5 }));
            serde_json::json!({
                "prompt": prompt_syntax::from_novelai(
                    entry.get("char_caption").and_then(|v| v.as_str()).unwrap_or_default(),
                ),
                "negative_prompt": prompt_syntax::from_novelai(
                    negatives
                        .get(index)
                        .and_then(|n| n.get("char_caption"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default(),
                ),
                "center": center,
                "enabled": true,
            })
        })
        .collect();

    if characters.is_empty() {
        return None;
    }
    serde_json::to_string(&characters).ok()
}

/// Pull the per-character caption array out of a V4 structured prompt block.
fn char_captions(block: Option<&serde_json::Value>) -> Option<Vec<serde_json::Value>> {
    block?
        .get("caption")?
        .get("char_captions")?
        .as_array()
        .cloned()
}

/// Map NovelAI's `Source` string onto the API model id the app selects with.
///
/// `Source` carries a version and a weights checksum, not the model id, and
/// says nothing about Full versus Curated, so every version resolves to its
/// Full id. Longest version prefix wins, or "V4.5" would match the "V4" arm.
fn model_id_from_source(source: &str) -> Option<&'static str> {
    let upper = source.to_ascii_uppercase();
    for (marker, model) in [
        ("V4.5", "nai-diffusion-4-5-full"),
        ("V5", "nai-diffusion-5-full"),
        ("V4", "nai-diffusion-4-full"),
    ] {
        if upper.contains(marker) {
            return Some(model);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn v4_comment() -> String {
        serde_json::json!({
            "prompt": "1girl, {blue hair}",
            "steps": 23,
            "height": 1216,
            "width": 832,
            "scale": 6.0,
            "uncond_scale": 1.0,
            "cfg_rescale": 0.4,
            "seed": 1234567890u64,
            "sampler": "k_euler_ancestral",
            "noise_schedule": "karras",
            "dynamic_thresholding": false,
            "skip_cfg_above_sigma": serde_json::Value::Null,
            "uc": "lowres, {bad anatomy}",
            "v4_prompt": {
                "caption": { "base_caption": "1girl, {blue hair}", "char_captions": [] },
                "use_coords": true,
                "use_order": true
            },
            "v4_negative_prompt": {
                "caption": { "base_caption": "lowres, {bad anatomy}", "char_captions": [] }
            }
        })
        .to_string()
    }

    #[test]
    fn a_non_novelai_chunk_set_is_declined() {
        let map = chunks(&[("parameters", "a photo"), ("Software", "AUTOMATIC1111")]);
        assert!(!is_novelai_chunks(&map));
        assert!(parse_chunks(&map).is_none());
    }

    #[test]
    fn the_source_chunk_alone_identifies_a_novelai_image() {
        let map = chunks(&[("Source", "NovelAI Diffusion V4.5 4BDE2A90")]);
        assert!(is_novelai_chunks(&map));
    }

    #[test]
    fn a_novelai_image_yields_the_generation_settings() {
        let comment = v4_comment();
        let map = chunks(&[
            ("Title", "AI generated image"),
            ("Software", "NovelAI"),
            ("Source", "NovelAI Diffusion V4.5 4BDE2A90"),
            ("Description", "1girl, {blue hair}"),
            ("Comment", &comment),
        ]);
        let params = parse_chunks(&map).expect("novelai chunks");

        assert_eq!(params.get("mooshie_backend").unwrap(), "novelai");
        assert_eq!(params.get("steps").unwrap(), "23");
        assert_eq!(params.get("cfg").unwrap(), "6.0");
        assert_eq!(params.get("seed").unwrap(), "1234567890");
        assert_eq!(params.get("sampler").unwrap(), "k_euler_ancestral");
        assert_eq!(params.get("scheduler").unwrap(), "karras");
        assert_eq!(params.get("size").unwrap(), "832x1216");
        assert_eq!(params.get("model").unwrap(), "nai-diffusion-4-5-full");
        assert_eq!(
            params.get("mooshie_novelai_source").unwrap(),
            "NovelAI Diffusion V4.5 4BDE2A90"
        );
        assert_eq!(params.get("mooshie_novelai_cfg_rescale").unwrap(), "0.4");
        assert_eq!(params.get("mooshie_novelai_use_coords").unwrap(), "true");
        // Variety+ was off, so no flag is written at all.
        assert!(!params.contains_key("mooshie_novelai_variety_plus"));
    }

    #[test]
    fn prompts_come_back_in_comfyui_weight_syntax() {
        let comment = v4_comment();
        let map = chunks(&[("Software", "NovelAI"), ("Comment", &comment)]);
        let params = parse_chunks(&map).expect("novelai chunks");
        assert_eq!(
            params.get("positive_prompt").unwrap(),
            "1girl, (blue hair:1.05)"
        );
        assert_eq!(
            params.get("negative_prompt").unwrap(),
            "lowres, (bad anatomy:1.05)"
        );
    }

    #[test]
    fn the_baked_in_quality_tags_are_not_re_enabled() {
        let map = chunks(&[("Software", "NovelAI"), ("Comment", &v4_comment())]);
        let params = parse_chunks(&map).expect("novelai chunks");
        assert_eq!(
            params.get("mooshie_novelai_quality_toggle").unwrap(),
            "false"
        );
        assert_eq!(params.get("mooshie_novelai_uc_preset").unwrap(), "0");
    }

    #[test]
    fn variety_plus_is_read_off_the_sigma_it_records() {
        let comment = serde_json::json!({ "skip_cfg_above_sigma": 19.34 }).to_string();
        let map = chunks(&[("Software", "NovelAI"), ("Comment", &comment)]);
        let params = parse_chunks(&map).expect("novelai chunks");
        assert_eq!(params.get("mooshie_novelai_variety_plus").unwrap(), "true");
    }

    #[test]
    fn character_prompts_are_zipped_back_into_the_app_shape() {
        let comment = serde_json::json!({
            "v4_prompt": {
                "caption": {
                    "base_caption": "2girls",
                    "char_captions": [
                        { "char_caption": "{red hair}", "centers": [{ "x": 0.25, "y": 0.5 }] },
                        { "char_caption": "blue hair", "centers": [{ "x": 0.75, "y": 0.5 }] }
                    ]
                }
            },
            "v4_negative_prompt": {
                "caption": {
                    "base_caption": "lowres",
                    "char_captions": [{ "char_caption": "hat", "centers": [] }]
                }
            }
        })
        .to_string();
        let map = chunks(&[("Software", "NovelAI"), ("Comment", &comment)]);
        let params = parse_chunks(&map).expect("novelai chunks");

        let raw = params
            .get("mooshie_novelai_characters")
            .expect("characters");
        let characters: serde_json::Value = serde_json::from_str(raw).unwrap();
        let characters = characters.as_array().unwrap();
        assert_eq!(characters.len(), 2);
        assert_eq!(characters[0]["prompt"], "(red hair:1.05)");
        assert_eq!(characters[0]["negative_prompt"], "hat");
        assert_eq!(characters[0]["center"]["x"], 0.25);
        assert_eq!(characters[0]["enabled"], true);
        // The second character has no matching negative, and gets an empty one
        // rather than dropping out of the list and shifting the coordinates.
        assert_eq!(characters[1]["prompt"], "blue hair");
        assert_eq!(characters[1]["negative_prompt"], "");
        assert_eq!(characters[1]["center"]["x"], 0.75);
    }

    #[test]
    fn the_stealth_payload_reads_the_same_as_the_chunks() {
        let payload = serde_json::json!({
            "Title": "AI generated image",
            "Software": "NovelAI",
            "Source": "NovelAI Diffusion V4 4F49EC75",
            "Description": "1girl",
            "Comment": v4_comment(),
        })
        .to_string();
        let params = parse_stealth_json(&payload).expect("novelai stealth payload");
        assert_eq!(params.get("model").unwrap(), "nai-diffusion-4-full");
        assert_eq!(params.get("steps").unwrap(), "23");
    }

    #[test]
    fn a_stealth_payload_from_another_tool_is_declined() {
        let payload =
            serde_json::json!({ "sui_image_params": { "prompt": "a photo" } }).to_string();
        assert!(parse_stealth_json(&payload).is_none());
    }

    #[test]
    fn the_version_in_source_picks_the_model_id() {
        assert_eq!(
            model_id_from_source("NovelAI Diffusion V4.5 4BDE2A90"),
            Some("nai-diffusion-4-5-full")
        );
        assert_eq!(
            model_id_from_source("NovelAI Diffusion V4 4F49EC75"),
            Some("nai-diffusion-4-full")
        );
        assert_eq!(
            model_id_from_source("NovelAI Diffusion V5 00000000"),
            Some("nai-diffusion-5-full")
        );
        // A V3 image predates every model the app offers, so the panel keeps
        // whatever checkpoint is already selected rather than being cleared.
        assert_eq!(model_id_from_source("Stable Diffusion F1022D28"), None);
    }
}
