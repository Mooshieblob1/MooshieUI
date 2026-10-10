//! Shot keyframes: a NovelAI V4.5 Full image of the character in one shot's
//! framing, using the character image as a Precise Reference (research doc
//! 5.5 and rules N1 to N4). The result is an ordinary NovelAI generation that
//! lands in the gallery; the Scenes page then uses it as that shot's
//! reference image.

use std::sync::Arc;

use base64::Engine as _;
use serde::Deserialize;

use crate::comfyui::types::GenerationParams;
use crate::error::AppError;
use crate::novelai::params::NovelAiParams;
use crate::novelai::EventSink;
use crate::state::AppState;

/// Precise Reference is V4.5 only (rule N1).
pub const KEYFRAME_MODEL: &str = "nai-diffusion-4-5-full";
/// The prototype's settings: inside the Opus free size and step limits, so
/// on Opus only the Precise Reference surcharge is billed.
const KEYFRAME_STEPS: u32 = 28;
const KEYFRAME_CFG: f64 = 5.0;

#[derive(Debug, Clone, Deserialize)]
pub struct KeyframeRequest {
    /// Gallery filename of the character image.
    pub reference: String,
    /// The character's appearance tags, `1girl` first (rule N4).
    pub character_tags: String,
    /// Framing, pose, expression and setting for this shot.
    pub shot_tags: String,
    pub aspect: String,
    #[serde(default)]
    pub minor: bool,
}

/// A canvas of at most one megapixel on NovelAI's 64-pixel grid for each
/// aspect ratio the video models offer.
pub fn size_for(aspect: &str) -> (u32, u32) {
    match aspect {
        "9:16" => (832, 1216),
        "4:3" => (1152, 896),
        "3:4" => (896, 1152),
        "1:1" => (1024, 1024),
        "21:9" => (1472, 640),
        _ => (1216, 832),
    }
}

/// Words that describe a body's shape or exposure. Dropped for a character
/// flagged as a minor (research doc, section 3 lesson 3), matched as whole
/// words so `glasses` never matches `ass`.
const MINOR_BLOCKED_WORDS: &[&str] = &[
    "breast",
    "breasts",
    "cleavage",
    "boob",
    "boobs",
    "underboob",
    "sideboob",
    "chest",
    "thigh",
    "thighs",
    "hip",
    "hips",
    "waist",
    "curvy",
    "plump",
    "voluptuous",
    "busty",
    "thick",
    "ass",
    "butt",
    "navel",
    "midriff",
    "nipple",
    "nipples",
    "skindentation",
    "nsfw",
    "nude",
    "naked",
    "sexy",
    "seductive",
    "lingerie",
    "underwear",
    "panties",
    "bra",
    "swimsuit",
    "bikini",
    "cameltoe",
    "bulge",
];

fn blocked_for_minor(tag: &str) -> bool {
    tag.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|w| MINOR_BLOCKED_WORDS.contains(&w.to_ascii_lowercase().as_str()))
}

/// Split, trim and dedupe comma-separated tags. V4.5 prompts must be ASCII
/// (rule N4), so anything else is refused rather than silently dropped.
pub fn clean_tags(tags: &str, minor: bool) -> Result<Vec<String>, String> {
    if !tags.is_ascii() {
        return Err(
            "NovelAI V4.5 tags must be plain ASCII. Write them in English tags, not Japanese."
                .into(),
        );
    }
    let mut out: Vec<String> = Vec::new();
    for tag in tags.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if minor && blocked_for_minor(tag) {
            continue;
        }
        if !out.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            out.push(tag.to_string());
        }
    }
    Ok(out)
}

/// The generation request for one keyframe. `reference_png` is the
/// character image as base64 PNG with its metadata already stripped.
pub fn build_params(
    request: &KeyframeRequest,
    reference_png: String,
    seed: i64,
) -> Result<GenerationParams, String> {
    let mut tags = clean_tags(&request.character_tags, request.minor)?;
    if tags.is_empty() {
        return Err("Add the character's appearance tags first.".into());
    }
    for tag in clean_tags(&request.shot_tags, request.minor)? {
        if !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            tags.push(tag);
        }
    }
    let (width, height) = size_for(&request.aspect);
    // Built through serde so every field NovelAI needs gets the same
    // defaults a saved request would (sampler, schedule, quality tags).
    let novelai: NovelAiParams = serde_json::from_value(serde_json::json!({
        "model": KEYFRAME_MODEL,
        "action": "generate",
        "quality_toggle": true,
        "uc_preset": 0,
        "director_references": [{
            "image": reference_png,
            "description": "character&style",
            "strength": 1.0,
            "fidelity": 1.0,
        }],
    }))
    .map_err(|e| format!("Could not build the keyframe request: {e}"))?;
    Ok(GenerationParams {
        positive_prompt: tags.join(", "),
        // Minors always carry nsfw in Undesired Content (lesson 3).
        negative_prompt: if request.minor {
            "nsfw".into()
        } else {
            String::new()
        },
        checkpoint: KEYFRAME_MODEL.into(),
        width,
        height,
        steps: KEYFRAME_STEPS,
        cfg: KEYFRAME_CFG,
        seed,
        batch_size: 1,
        novelai: Some(novelai),
        ..Default::default()
    })
}

/// The reference as NovelAI gets it: RGB PNG at most 1536 px on a side, so
/// neither text chunks nor NovelAI's alpha-channel metadata leave the machine.
fn clean_reference(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|e| format!("Could not read the character image: {e}"))?;
    let image = if image.width().max(image.height()) > 1536 {
        image.resize(1536, 1536, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(image.to_rgb8())
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| format!("Could not encode the character image: {e}"))?;
    Ok(out)
}

/// Check, build and start a keyframe generation; returns its prompt id. It
/// runs as an ordinary NovelAI generation on the caller's own NovelAI key.
pub async fn start(
    state: &Arc<AppState>,
    username: Option<&str>,
    request: &KeyframeRequest,
    sink: EventSink,
) -> Result<String, AppError> {
    crate::temp_images::cleanup(300);
    let credential = crate::novelai::resolve_credential(state, username).await?;
    let gallery = crate::webserver::user_gallery_dir(username)
        .ok_or_else(|| AppError::Other("Cannot find the gallery folder.".into()))?;
    let bytes =
        crate::commands::api::load_gallery_image_png_from_dir(&gallery, &request.reference).await?;
    let png = tokio::task::spawn_blocking(move || clean_reference(&bytes))
        .await
        .map_err(|e| AppError::Other(format!("Task failed: {e}")))?
        .map_err(AppError::Other)?;
    let reference = base64::engine::general_purpose::STANDARD.encode(png);
    let params = build_params(request, reference, crate::novelai::resolve_seed(-1))
        .map_err(AppError::Other)?;
    crate::novelai::preflight(&params)?;

    let prompt_id = crate::novelai::new_prompt_id();
    state
        .prompt_queue
        .insert(&prompt_id, username.map(str::to_string));
    state.broadcast_queue_positions();
    let bg_state = Arc::clone(state);
    let bg_id = prompt_id.clone();
    tokio::spawn(async move {
        let result = crate::novelai::run(
            Arc::clone(&bg_state),
            sink,
            bg_id.clone(),
            params,
            credential,
        )
        .await;
        if let Err(e) = &result {
            log::error!("[scene] keyframe {bg_id} failed: {e}");
        }
        if !matches!(result, Ok(crate::novelai::RunOutcome::HandedOff)) {
            bg_state.prompt_queue.cancel_and_remove(&bg_id);
            bg_state.broadcast_queue_positions();
        }
    });
    Ok(prompt_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(minor: bool) -> KeyframeRequest {
        KeyframeRequest {
            reference: "a.jxl".into(),
            character_tags: "1girl, short black hair, red ribbon, large breasts, glasses".into(),
            shot_tags: "medium shot, arms crossed, classroom, short black hair".into(),
            aspect: "16:9".into(),
            minor,
        }
    }

    #[test]
    fn a_minor_loses_body_shape_tags_and_gets_nsfw_in_uc() {
        let p = build_params(&request(true), "AAAA".into(), 1).unwrap();
        assert!(!p.positive_prompt.contains("breasts"));
        assert!(p.positive_prompt.contains("glasses"));
        assert_eq!(p.negative_prompt, "nsfw");
        let adult = build_params(&request(false), "AAAA".into(), 1).unwrap();
        assert!(adult.positive_prompt.contains("large breasts"));
    }

    #[test]
    fn tags_keep_their_order_and_drop_repeats() {
        let p = build_params(&request(false), "AAAA".into(), 1).unwrap();
        assert!(p.positive_prompt.starts_with("1girl, short black hair"));
        assert_eq!(p.positive_prompt.matches("short black hair").count(), 1);
        assert!(p.positive_prompt.ends_with("classroom"));
    }

    #[test]
    fn the_request_uses_precise_reference_on_v45() {
        let p = build_params(&request(false), "AAAA".into(), 7).unwrap();
        let nai = p.novelai.as_ref().unwrap();
        assert_eq!(nai.model, KEYFRAME_MODEL);
        assert_eq!(nai.director_references.len(), 1);
        assert_eq!(nai.director_references[0].description, "character&style");
        assert!(!nai.sampler.is_empty());
        assert_eq!((p.width, p.height), (1216, 832));
        assert!(crate::novelai::preflight(&p).is_ok());
    }

    #[test]
    fn keyframes_stay_inside_the_opus_free_size() {
        for aspect in ["16:9", "9:16", "4:3", "3:4", "1:1", "21:9"] {
            let (w, h) = size_for(aspect);
            assert!(w * h <= 1024 * 1024, "{aspect}");
            assert_eq!((w % 64, h % 64), (0, 0), "{aspect}");
        }
    }

    #[test]
    fn the_reference_loses_alpha_and_shrinks() {
        let img = image::RgbaImage::from_pixel(2000, 3000, image::Rgba([1, 2, 3, 128]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let back = image::load_from_memory(&clean_reference(&png).unwrap()).unwrap();
        assert_eq!(back.height(), 1536);
        assert!(!back.color().has_alpha());
    }

    #[test]
    fn non_ascii_tags_are_refused() {
        let mut r = request(false);
        r.shot_tags = "教室".into();
        assert!(build_params(&r, "AAAA".into(), 1).is_err());
        r.shot_tags = String::new();
        r.character_tags = " , ".into();
        assert!(build_params(&r, "AAAA".into(), 1).is_err());
    }
}
