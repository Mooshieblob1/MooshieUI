//! The NovelAI face detailer: detect locally, repaint on NovelAI, composite in
//! Rust.
//!
//! This module is the orchestration half of the feature. Everything it decides
//! -- crop geometry, the free-window fit, the face prompt, the feathered blend
//! -- lives in [`face_pass`], which has no I/O and is unit tested on its own.
//! What is here is the ordering, the network calls and the degradations.
//!
//! Why img2img on a Rust-side crop rather than NovelAI infill: infill switches
//! to the model's inpainting variant, and V5 Curated has none of its own, so a
//! masked face pass would be painted by a different model than the image it is
//! fixing. Cropping locally and blending locally keeps every pixel in the frame
//! from the same model.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use image::{DynamicImage, RgbaImage};

use crate::comfyui::types::GenerationParams;
use crate::error::AppError;
use crate::novelai::client::NovelAiClient;
use crate::novelai::face_pass::{self, CropPlan};
use crate::novelai::{build_request, detect, EventSink};
use crate::state::AppState;

/// Pause between face requests.
///
/// NovelAI rate limits per account, and a face pass fires N requests back to
/// back immediately after the base render, which is the worst possible moment
/// to sprint.
const FACE_GAP: Duration = Duration::from_millis(400);

/// How long to wait before the single retry after a 429.
///
/// A fixed value rather than the response's `Retry-After`: `client::check_status`
/// turns the response into an `AppError` without reading its headers, and
/// threading the header through would touch every `ApiError` construction site
/// in the crate for one call path.
const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(6);

/// Repaint every detected face on `png` through NovelAI.
///
/// `Ok(None)` means nothing changed and the caller should deliver the original
/// bytes: the pass is off, the detector found no faces, or every face was
/// skipped. `Err` means the pass could not run at all; the caller has already
/// paid for the base image, so it treats that as a warning and delivers the
/// image untouched rather than failing the generation.
pub async fn run_face_pass(
    state: &Arc<AppState>,
    sink: &EventSink,
    prompt_id: &str,
    params: &GenerationParams,
    client: &NovelAiClient<'_>,
    png: &[u8],
) -> Result<Option<Vec<u8>>, AppError> {
    let Some(nai) = params.novelai.as_ref() else {
        return Ok(None);
    };
    let detail = nai.face_detail.clone();
    if !detail.enabled || !detail.uses_novelai_engine() {
        return Ok(None);
    }

    let faces = detect::detect_faces(
        state,
        png.to_vec(),
        &detail.detector_model,
        detail.threshold,
        detail.max_faces,
    )
    .await?;
    if faces.is_empty() {
        log::info!("NovelAI {prompt_id}: face pass found no faces");
        return Ok(None);
    }

    let mut base = decode_rgba(png.to_vec()).await?;
    let (image_w, image_h) = (base.width(), base.height());

    // Strongest first, and each crop is taken from the running composite, so
    // overlapping detections see the already-repainted neighbour rather than
    // the original artefact.
    let mut faces = faces;
    faces.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total = faces.len();
    let mut painted = 0usize;

    for (index, face) in faces.iter().enumerate() {
        if state.prompt_queue.is_cancelled(prompt_id) {
            break;
        }

        let Some(plan) = face_pass::plan_crop(
            face,
            image_w,
            image_h,
            detail.padding,
            detail.guide_size,
            detail.fits_free_window(),
        ) else {
            log::debug!(
                "NovelAI {prompt_id}: face {} is too small to repaint",
                index + 1
            );
            continue;
        };

        sink.emit(
            "comfyui:progress",
            serde_json::json!({
                "prompt_id": prompt_id,
                "value": index,
                "max": total,
                "node": format!("NovelAI face {}/{}", index + 1, total),
            }),
        );

        let crop = image::imageops::crop_imm(&base, plan.x, plan.y, plan.w, plan.h).to_image();
        let sent = match resize_rgba(crop, plan.req_w, plan.req_h).await {
            Ok(img) => img,
            Err(err) => {
                log::warn!(
                    "NovelAI {prompt_id}: face {} could not be cropped ({err})",
                    index + 1
                );
                continue;
            }
        };

        let tags = tagger_tags(state, DynamicImage::ImageRgba8(sent.clone()), &detail).await;
        let character =
            face_pass::nearest_character(&nai.characters, nai.use_coords, &plan, image_w, image_h);
        let (prompt, tier) = face_pass::build_face_prompt(
            &detail,
            &params.positive_prompt,
            character.map(|c| c.prompt.as_str()),
            &tags,
        );
        log::info!(
            "NovelAI {prompt_id}: face {}/{total} {}x{} ({:?}) {}",
            index + 1,
            plan.req_w,
            plan.req_h,
            tier,
            if plan.is_free(detail.steps) {
                "free"
            } else {
                "costs Anlas"
            }
        );

        let encoded = match encode_png(&sent).await {
            Ok(bytes) => base64::engine::general_purpose::STANDARD.encode(&bytes),
            Err(err) => {
                log::warn!(
                    "NovelAI {prompt_id}: face {} could not be encoded ({err})",
                    index + 1
                );
                continue;
            }
        };

        let crop_params = crop_request(params, &plan, &prompt, encoded, index);
        let body = match build_request(&crop_params) {
            Ok(body) => body,
            Err(err) => {
                log::warn!("NovelAI {prompt_id}: face pass request rejected ({err})");
                break;
            }
        };

        if index > 0 {
            tokio::time::sleep(FACE_GAP).await;
        }

        let images = match generate_with_retry(client, &body).await {
            Ok(images) => images,
            Err(err) => {
                // Stop rather than continue: whatever refused this face (a rate
                // limit, an empty allowance, a network drop) will refuse the
                // next one too, and the faces already done are worth keeping.
                log::warn!(
                    "NovelAI {prompt_id}: face {} of {total} failed ({err}); delivering what is done",
                    index + 1
                );
                break;
            }
        };
        let Some(result) = images.into_iter().next() else {
            log::warn!("NovelAI {prompt_id}: face {} came back empty", index + 1);
            break;
        };

        let patch = match decode_and_fit(result, plan.w, plan.h).await {
            Ok(patch) => patch,
            Err(err) => {
                log::warn!(
                    "NovelAI {prompt_id}: face {} could not be read back ({err})",
                    index + 1
                );
                break;
            }
        };
        face_pass::composite_face(&mut base, &patch, &plan, detail.feather);
        painted += 1;
    }

    if painted == 0 {
        return Ok(None);
    }

    sink.emit(
        "comfyui:progress",
        serde_json::json!({
            "prompt_id": prompt_id,
            "value": total,
            "max": total,
            "node": "NovelAI faces",
        }),
    );

    encode_png(&base).await.map(Some)
}

/// The request for one face crop.
///
/// Split out and pure so the things that must *not* survive into a face
/// request can be asserted on: the character array, `use_coords`, the batch
/// and the scene prompt. A character box that reached a face crop would place
/// a second character inside it, and `use_coords` would move the one face
/// there is off-centre.
fn crop_request(
    params: &GenerationParams,
    plan: &CropPlan,
    prompt: &str,
    image: String,
    index: usize,
) -> GenerationParams {
    let mut out = params.clone();
    let detail = out
        .novelai
        .as_ref()
        .map(|nai| nai.face_detail.clone())
        .unwrap_or_default();

    out.positive_prompt = prompt.to_string();
    out.width = plan.req_w;
    out.height = plan.req_h;
    out.batch_size = 1;
    // Above 28 steps the crop leaves the Opus free window, so a pass the panel
    // called free would quietly start billing. Under `allow_paid` the user has
    // said they are spending anyway.
    out.steps = if detail.fits_free_window() {
        detail.steps.min(face_pass::FREE_STEPS).max(1)
    } else {
        detail.steps.max(1)
    };
    // A negative seed means "randomise", and every face should stay random in
    // that case rather than collapsing onto a fixed low number.
    out.seed = if params.seed < 0 {
        params.seed
    } else {
        params.seed.saturating_add(2 + index as i64)
    };
    out.input_image = Some(image);
    out.mask_image = None;

    if let Some(nai) = out.novelai.as_mut() {
        nai.action = "img2img".to_string();
        nai.strength = detail.strength;
        // The crop is a real image, not a blank canvas: added noise only fights
        // the structure the pass is meant to preserve.
        nai.noise = 0.0;
        nai.characters = Vec::new();
        nai.use_coords = false;
        // Nothing downstream of the face pass runs a second time.
        nai.local_post_process = false;
        nai.face_detail = Default::default();
    }
    out
}

/// One retry, once, on a rate limit.
async fn generate_with_retry(
    client: &NovelAiClient<'_>,
    body: &serde_json::Value,
) -> Result<Vec<Vec<u8>>, AppError> {
    // The non-streaming endpoint costs exactly the same and there is no
    // per-step bar to feed for a crop, so the stream buys nothing here.
    match client.generate(body).await {
        Err(AppError::ApiError { status: 429, .. }) => {
            log::warn!("NovelAI face pass rate limited; retrying once");
            tokio::time::sleep(RATE_LIMIT_BACKOFF).await;
            client.generate(body).await
        }
        other => other,
    }
}

/// Run the WD tagger on a crop, or return nothing if it cannot run.
///
/// Nothing here can download: the interrogator's fetch helpers need an
/// `AppHandle` and this module compiles into the server build too. A missing
/// model or ORT runtime is therefore a degradation, not an error -- the prompt
/// falls back down [`face_pass::build_face_prompt`]'s chain, and the panel is
/// where the download is offered.
async fn tagger_tags(
    state: &Arc<AppState>,
    img: DynamicImage,
    detail: &crate::novelai::params::NovelAiFaceDetail,
) -> Vec<String> {
    if !detail.prompt_mode.trim().eq_ignore_ascii_case("auto") {
        return Vec::new();
    }

    let (model_id, custom_models, character_threshold) = {
        let config = state.config.read().await;
        (
            config.interrogator_model.clone(),
            config.interrogator_custom_models.clone(),
            config.interrogator_character_threshold,
        )
    };
    let root_dir = {
        let guard = state.interrogator.read().await;
        guard.root_dir()
    };

    let Ok(model_dir) =
        crate::interrogator::resolve_model_dir(&model_id, &root_dir, &custom_models)
    else {
        return Vec::new();
    };
    if !crate::interrogator::is_model_downloaded_at(&model_dir)
        || !crate::interrogator::is_ort_library_present_at(&root_dir)
    {
        log::info!("NovelAI face pass: tagger unavailable, using prompt identity only");
        return Vec::new();
    }

    let interrogator = Arc::clone(&state.interrogator);
    let general_threshold = detail.tagger_threshold as f32;
    let result = tokio::task::spawn_blocking(move || {
        let mut guard = interrogator.blocking_write();
        guard.set_model(&model_id, model_dir);
        guard.load_session()?;
        guard.run_inference_from_image(img, general_threshold, character_threshold)
    })
    .await;

    match result {
        Ok(Ok(tags)) => tags
            .character_tags
            .into_iter()
            .chain(tags.general_tags)
            .map(|t| t.name)
            .collect(),
        Ok(Err(err)) => {
            log::warn!("NovelAI face pass: tagger failed ({err})");
            Vec::new()
        }
        Err(err) => {
            log::warn!("NovelAI face pass: tagger panicked ({err})");
            Vec::new()
        }
    }
}

async fn decode_rgba(bytes: Vec<u8>) -> Result<RgbaImage, AppError> {
    tokio::task::spawn_blocking(move || {
        image::load_from_memory(&bytes)
            .map(|img| img.to_rgba8())
            .map_err(|e| AppError::Other(format!("could not decode the image: {e}")))
    })
    .await
    .map_err(|e| AppError::Other(format!("image decode task failed: {e}")))?
}

async fn resize_rgba(img: RgbaImage, width: u32, height: u32) -> Result<RgbaImage, AppError> {
    tokio::task::spawn_blocking(move || {
        if img.width() == width && img.height() == height {
            return img;
        }
        image::imageops::resize(&img, width, height, image::imageops::FilterType::Lanczos3)
    })
    .await
    .map_err(|e| AppError::Other(format!("image resize task failed: {e}")))
}

async fn decode_and_fit(bytes: Vec<u8>, width: u32, height: u32) -> Result<RgbaImage, AppError> {
    let decoded = decode_rgba(bytes).await?;
    resize_rgba(decoded, width, height).await
}

async fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, AppError> {
    let img = img.clone();
    tokio::task::spawn_blocking(move || {
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .map_err(|e| AppError::Other(format!("could not encode the image as PNG: {e}")))?;
        Ok(buf)
    })
    .await
    .map_err(|e| AppError::Other(format!("image encode task failed: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::params::{NovelAiCharacter, NovelAiParams};

    fn plan() -> CropPlan {
        CropPlan {
            x: 100,
            y: 120,
            w: 300,
            h: 300,
            req_w: 1024,
            req_h: 1024,
        }
    }

    fn params() -> GenerationParams {
        let mut nai = NovelAiParams {
            model: "nai-diffusion-4-5-full".into(),
            action: "generate".into(),
            ..Default::default()
        };
        nai.strength = 0.7;
        nai.noise = 0.2;
        nai.use_coords = true;
        nai.local_post_process = true;
        nai.characters = vec![NovelAiCharacter {
            prompt: "1girl, blue hair".into(),
            enabled: true,
            ..Default::default()
        }];
        nai.face_detail.enabled = true;
        nai.face_detail.strength = 0.35;
        nai.face_detail.steps = 40;

        GenerationParams {
            positive_prompt: "1girl, full body, standing, cowboy shot".into(),
            checkpoint: "nai-diffusion-4-5-full".into(),
            width: 832,
            height: 1216,
            steps: 23,
            batch_size: 4,
            seed: 1000,
            novelai: Some(nai),
            ..Default::default()
        }
    }

    #[test]
    fn the_crop_request_carries_no_characters_and_no_coords() {
        let out = crop_request(&params(), &plan(), "portrait, close-up", "AAAA".into(), 0);
        let nai = out.novelai.expect("nai block");
        assert!(nai.characters.is_empty());
        assert!(!nai.use_coords);
        assert_eq!(nai.action, "img2img");
    }

    #[test]
    fn the_crop_request_never_carries_the_scene_prompt() {
        let out = crop_request(&params(), &plan(), "portrait, close-up", "AAAA".into(), 0);
        assert_eq!(out.positive_prompt, "portrait, close-up");
        assert!(!out.positive_prompt.contains("full body"));
    }

    #[test]
    fn the_crop_request_is_a_single_image_at_the_planned_size() {
        let out = crop_request(&params(), &plan(), "portrait", "AAAA".into(), 0);
        assert_eq!(out.batch_size, 1);
        assert_eq!((out.width, out.height), (1024, 1024));
        assert_eq!(out.input_image.as_deref(), Some("AAAA"));
        assert!(out.mask_image.is_none());
    }

    #[test]
    fn steps_are_clamped_to_the_free_window_under_fit_free() {
        let out = crop_request(&params(), &plan(), "portrait", "AAAA".into(), 0);
        assert_eq!(out.steps, face_pass::FREE_STEPS);
    }

    #[test]
    fn allow_paid_keeps_the_panels_step_count() {
        let mut base = params();
        base.novelai.as_mut().unwrap().face_detail.anlas_policy = "allow_paid".into();
        let out = crop_request(&base, &plan(), "portrait", "AAAA".into(), 0);
        assert_eq!(out.steps, 40);
    }

    #[test]
    fn each_face_gets_its_own_seed_but_a_random_seed_stays_random() {
        let out = crop_request(&params(), &plan(), "portrait", "AAAA".into(), 2);
        assert_eq!(out.seed, 1004);

        let mut random = params();
        random.seed = -1;
        let out = crop_request(&random, &plan(), "portrait", "AAAA".into(), 2);
        assert_eq!(out.seed, -1);
    }

    #[test]
    fn the_crop_request_takes_the_panels_strength_and_no_noise() {
        let out = crop_request(&params(), &plan(), "portrait", "AAAA".into(), 0);
        let nai = out.novelai.expect("nai block");
        assert!((nai.strength - 0.35).abs() < 1e-9);
        assert!(nai.noise.abs() < 1e-9);
    }

    #[test]
    fn the_crop_request_cannot_recurse_into_another_pass() {
        let out = crop_request(&params(), &plan(), "portrait", "AAAA".into(), 0);
        let nai = out.novelai.expect("nai block");
        assert!(!nai.face_detail.enabled);
        assert!(!nai.local_post_process);
    }

    #[test]
    fn the_crop_request_builds_a_payload_with_no_character_captions() {
        let out = crop_request(&params(), &plan(), "portrait, close-up", "AAAA".into(), 0);
        let body = build_request(&out).expect("valid request");
        let text = body.to_string();
        assert!(
            !text.contains("blue hair"),
            "character prompt leaked: {text}"
        );
        assert_eq!(body["parameters"]["use_coords"], false);
        assert_eq!(body["action"], "img2img");
    }
}
