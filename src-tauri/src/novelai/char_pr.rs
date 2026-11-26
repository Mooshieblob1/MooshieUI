//! Orchestrator for experimental multi-character Precise Reference isolation.
//!
//! Pure geometry, matching, prompt building and compositing live in
//! [`super::char_pr_pass`]. This module handles request partitioning, local YOLO
//! person detection + optional WD-tagger disambiguation, sequential NovelAI
//! `img2img` character passes, progress/preview events, and PNG chunk restoration.

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::comfyui::types::GenerationParams;
use crate::error::AppError;
use crate::novelai::char_pr_pass::{
    self, CharacterPrGroup, IsolationPlan, IsolationSkipReason, PlannedCharacterRegion, SheetEntry,
};
use crate::novelai::client::NovelAiClient;
use crate::novelai::face_detail::{
    decode_and_fit, decode_rgba, encode_png, resize_rgba, run_wd_tagger,
};
use crate::novelai::face_pass::{self, CropPlan};
use crate::novelai::models::{NovelAiModel, ReferenceCanvasMode};
use crate::novelai::params::{
    NovelAiCharacter, NovelAiDirectorReference, NovelAiMultiCharPr, NovelAiParams,
};
use crate::novelai::{build_request, detect, resolve_model_id, EventSink, StreamEvent};
use crate::state::AppState;
use crate::templates::face_detect::DetectedRegion;

/// Pause between per-character NovelAI requests to avoid burst rate limits.
const CHAR_PR_GAP: Duration = Duration::from_millis(400);
const CUTOUT_CACHE_CAP: usize = 32;
const DEFAULT_SHEET_DETECTOR: &str = "person_yolov8m-seg.pt";

/// Result of removing an opaque background from one Precise Reference image.
#[derive(Debug, Clone, Serialize)]
pub struct CutoutResult {
    pub image: String,
    pub already_transparent: bool,
    pub applied: bool,
}

fn cutout_cache() -> &'static Mutex<VecDeque<([u8; 32], String)>> {
    static CACHE: OnceLock<Mutex<VecDeque<([u8; 32], String)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::with_capacity(CUTOUT_CACHE_CAP)))
}

fn strip_data_url(s: &str) -> &str {
    s.split_once("base64,").map_or(s, |(_, rest)| rest).trim()
}

/// Segment the main person out of an opaque reference image using local YOLO
/// person segmentation, caching results by image + detector SHA-256.
pub async fn cutout_reference_bg(
    state: &Arc<AppState>,
    image_b64: &str,
    detector_model: &str,
) -> Result<CutoutResult, AppError> {
    let raw = strip_data_url(image_b64);
    let engine = base64::engine::general_purpose::STANDARD;
    let bytes = engine
        .decode(raw)
        .map_err(|err| AppError::Other(format!("Invalid base64 reference image: {err}")))?;
    let rgba = decode_rgba(bytes.clone()).await?;
    if char_pr_pass::has_meaningful_alpha(&rgba) {
        return Ok(CutoutResult {
            image: raw.to_string(),
            already_transparent: true,
            applied: false,
        });
    }

    let detector = if detector_model.trim().is_empty() {
        DEFAULT_SHEET_DETECTOR
    } else {
        detector_model.trim()
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher.update([0u8]);
    hasher.update(detector.as_bytes());
    let key: [u8; 32] = hasher.finalize().into();

    {
        let cache = cutout_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, cached)) = cache.iter().find(|(k, _)| *k == key) {
            return Ok(CutoutResult {
                image: cached.clone(),
                already_transparent: false,
                applied: true,
            });
        }
    }

    let regions = detect::detect_regions(state, bytes, detector, 0.25, 4, true).await?;
    let best_region = regions
        .into_iter()
        .filter(|r| r.mask_png.is_some())
        .max_by(|a, b| {
            let sa = (a.bbox.width().max(0) as f64)
                * (a.bbox.height().max(0) as f64)
                * a.bbox.confidence.max(0.0);
            let sb = (b.bbox.width().max(0) as f64)
                * (b.bbox.height().max(0) as f64)
                * b.bbox.confidence.max(0.0);
            sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
        });

    let Some(region) = best_region else {
        return Ok(CutoutResult {
            image: raw.to_string(),
            already_transparent: false,
            applied: false,
        });
    };

    let cutout_opt =
        tokio::task::spawn_blocking(move || char_pr_pass::apply_person_cutout(&rgba, &region))
            .await
            .map_err(|err| AppError::Other(format!("Cutout task failed: {err}")))?;

    let Some(cutout) = cutout_opt else {
        return Ok(CutoutResult {
            image: raw.to_string(),
            already_transparent: false,
            applied: false,
        });
    };

    let png_bytes = encode_png(&cutout).await?;
    let encoded = engine.encode(png_bytes);
    {
        let mut cache = cutout_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while cache.len() >= CUTOUT_CACHE_CAP {
            cache.pop_front();
        }
        cache.push_back((key, encoded.clone()));
    }

    Ok(CutoutResult {
        image: encoded,
        already_transparent: false,
        applied: true,
    })
}

async fn encode_sheet_png(composite: RgbaImage, transparent_bg: bool) -> Result<Vec<u8>, AppError> {
    if transparent_bg {
        return encode_png(&composite).await;
    }
    tokio::task::spawn_blocking(move || {
        let rgb = DynamicImage::ImageRgba8(composite).to_rgb8();
        let mut buf = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(rgb)
            .write_to(&mut buf, ImageFormat::Png)
            .map_err(|err| AppError::Other(format!("Failed to encode RGB sheet PNG: {err}")))?;
        Ok(buf.into_inner())
    })
    .await
    .map_err(|err| AppError::Other(format!("Sheet PNG encode task failed: {err}")))?
}

async fn build_sheet_reference(
    state: &Arc<AppState>,
    params: &GenerationParams,
    nai: &NovelAiParams,
    model: &NovelAiModel,
    plan: &IsolationPlan,
) -> Option<NovelAiDirectorReference> {
    let (canvas_w, canvas_h) = char_pr_pass::sheet_canvas_size(
        params.width,
        params.height,
        model.precise_reference_canvas,
    );
    let placements = char_pr_pass::resolve_sheet_placements(plan, nai, canvas_w, canvas_h);
    if placements.is_empty() {
        return None;
    }

    let engine = base64::engine::general_purpose::STANDARD;
    let mut entries = Vec::with_capacity(placements.len());
    let mut member_refs: Vec<&NovelAiDirectorReference> = Vec::with_capacity(placements.len());
    let mut strength_sum = 0.0_f64;
    let mut fidelity_sum = 0.0_f64;

    for placement in placements {
        let reference = &plan.character_groups[placement.group_idx].references[placement.ref_idx];
        let mut img_b64 = strip_data_url(&reference.image).to_string();
        if nai.multi_char_pr.sheet_cutout {
            match cutout_reference_bg(state, &img_b64, &nai.multi_char_pr.detector_model).await {
                Ok(res) => {
                    img_b64 = res.image;
                }
                Err(err) => {
                    log::warn!(
                        "NovelAI character sheet cutout skipped ({err}); using original image"
                    );
                }
            }
        }
        let Ok(bytes) = engine.decode(strip_data_url(&img_b64)) else {
            continue;
        };
        let Ok(rgba) = decode_rgba(bytes).await else {
            continue;
        };
        entries.push(SheetEntry {
            image: rgba,
            x: placement.x,
            y: placement.y,
            scale: placement.scale,
        });
        member_refs.push(reference);
        strength_sum += reference.strength;
        fidelity_sum += reference.fidelity;
    }

    if entries.is_empty() {
        return None;
    }

    let transparent_bg = model.precise_reference_canvas == ReferenceCanvasMode::Bucket;
    let composite = tokio::task::spawn_blocking(move || {
        char_pr_pass::build_combined_sheet_image(&entries, canvas_w, canvas_h, transparent_bg)
    })
    .await
    .ok()?;
    let png_bytes = encode_sheet_png(composite, transparent_bg).await.ok()?;
    let count = member_refs.len() as f64;
    let description = char_pr_pass::combined_sheet_description(&member_refs, model);

    Some(NovelAiDirectorReference {
        image: engine.encode(png_bytes),
        description,
        strength: strength_sum / count,
        fidelity: fidelity_sum / count,
        enabled: true,
        ..Default::default()
    })
}

fn apply_sheet_or_strip_base_params(
    params: &GenerationParams,
    plan: &IsolationPlan,
    sheet_ref: Option<NovelAiDirectorReference>,
) -> GenerationParams {
    let mut base_params = params.clone();
    if let Some(base_nai) = base_params.novelai.as_mut() {
        base_nai.director_references = plan.base_references.clone();
        if let Some(sheet) = sheet_ref {
            base_nai.director_references.push(sheet);
        }
    }
    base_params
}

/// Emit a non-fatal status notice for the multi-character PR pass.
pub fn notify(sink: &EventSink, prompt_id: &str, status: &str, reason: &str) {
    sink.emit(
        "novelai:char_pr_pass",
        serde_json::json!({
            "prompt_id": prompt_id,
            "status": status,
            "reason": reason,
        }),
    );
}

/// Partition references out of the base generation (or stitch a combined sheet)
/// when experimental multi-character isolation applies.
///
/// Returns `(base_override, followup_plan, skip_reason)`. When `strategy` is
/// `"sheet"`, only `base_override` is returned (`followup_plan` is `None`);
/// `"passes"` returns both stripped base + `followup_plan`; `"hybrid"` attaches
/// the combined sheet on base AND returns `followup_plan` for light passes.
pub async fn prepare_base_request(
    state: &Arc<AppState>,
    params: &GenerationParams,
    transparency: bool,
) -> (
    Option<GenerationParams>,
    Option<IsolationPlan>,
    Option<IsolationSkipReason>,
) {
    let Some(nai) = params.novelai.as_ref() else {
        return (None, None, None);
    };
    let model_id = resolve_model_id(params, nai);
    let Some(model) = crate::novelai::models::find(&model_id) else {
        return (None, None, None);
    };

    match char_pr_pass::partition_references(params, nai, model, transparency) {
        Ok(Some(plan)) => {
            let sheet_ref = if plan.strategy.uses_sheet() {
                build_sheet_reference(state, params, nai, model, &plan).await
            } else {
                None
            };
            if plan.strategy.uses_sheet() && sheet_ref.is_none() && !plan.strategy.uses_passes() {
                return (None, None, None);
            }
            let base_params = apply_sheet_or_strip_base_params(params, &plan, sheet_ref);
            let followup_plan = plan.strategy.uses_passes().then_some(plan);
            (Some(base_params), followup_plan, None)
        }
        Ok(None) => (None, None, None),
        Err(reason) => (None, None, Some(reason)),
    }
}

fn crop_steps(main_steps: u32, cfg: &NovelAiMultiCharPr) -> u32 {
    if cfg.fits_free_window() {
        main_steps.min(face_pass::FREE_STEPS).max(1)
    } else {
        main_steps.max(1)
    }
}

fn char_seed(seed: i64, index: usize) -> i64 {
    let modulus = crate::novelai::MAX_SEED + 1;
    let offset = (100 + index as i64).rem_euclid(modulus);
    (seed.rem_euclid(modulus) + offset) % modulus
}

/// Build an isolated NovelAI `img2img` request for one character crop carrying
/// only that character's Precise Reference entries.
fn crop_request(
    params: &GenerationParams,
    plan: &CropPlan,
    pos_prompt: &str,
    neg_prompt: &str,
    char_refs: &[NovelAiDirectorReference],
    image: String,
    index: usize,
) -> GenerationParams {
    let mut out = params.clone();
    let cfg = out
        .novelai
        .as_ref()
        .map(|nai| nai.multi_char_pr.clone())
        .unwrap_or_default();

    out.positive_prompt = pos_prompt.to_string();
    out.negative_prompt = neg_prompt.to_string();
    out.width = plan.req_w;
    out.height = plan.req_h;
    out.batch_size = 1;
    out.steps = crop_steps(params.steps, &cfg);
    out.seed = if params.seed < 0 {
        params.seed
    } else {
        char_seed(params.seed, index)
    };
    out.input_image = Some(image);
    out.mask_image = None;

    if let Some(nai) = out.novelai.as_mut() {
        nai.action = "img2img".to_string();
        nai.effort = crate::novelai::models::EFFORT_HIGH.to_string();
        nai.strength = cfg.strength.clamp(0.05, 1.0);
        nai.noise = 0.0;
        nai.characters = Vec::new();
        nai.use_coords = false;
        nai.vibes.clear();
        nai.director_references = char_refs
            .iter()
            .cloned()
            .map(|mut r| {
                r.target_character_index = None;
                r.sheet_x = None;
                r.sheet_y = None;
                r.sheet_scale = None;
                r
            })
            .collect();
        nai.local_post_process = false;
        nai.face_detail = Default::default();
        nai.multi_char_pr = Default::default();
    }
    out
}

async fn resolve_planned_regions(
    state: &Arc<AppState>,
    sink: &EventSink,
    prompt_id: &str,
    params: &GenerationParams,
    plan: &IsolationPlan,
    base: &RgbaImage,
    png: &[u8],
) -> Result<Option<HashMap<usize, PlannedCharacterRegion>>, AppError> {
    let Some(nai) = params.novelai.as_ref() else {
        return Ok(None);
    };
    let cfg = &nai.multi_char_pr;
    let (image_w, image_h) = (base.width(), base.height());

    let active_chars_owned: Vec<(usize, NovelAiCharacter)> = plan
        .active_character_indices
        .iter()
        .filter_map(|&idx| nai.characters.get(idx).cloned().map(|c| (idx, c)))
        .collect();

    if cfg.locator.eq_ignore_ascii_case("coords") {
        if !nai.use_coords {
            log::info!("NovelAI {prompt_id}: character PR isolation skipped, coords mode requires custom positions");
            notify(sink, prompt_id, "skipped", "needs_coords");
            return Ok(None);
        }
        return Ok(Some(char_pr_pass::plan_coord_regions(
            &active_chars_owned,
            image_w,
            image_h,
            cfg.feather,
            cfg.fits_free_window(),
        )));
    }

    match detect::detect_regions(
        state,
        png.to_vec(),
        &cfg.detector_model,
        cfg.threshold,
        10,
        true,
    )
    .await
    {
        Ok(regions) if !regions.is_empty() => {
            let mut det_tags: Vec<Vec<String>> = Vec::new();
            if !nai.use_coords {
                for reg in &regions {
                    let rx1 = reg.bbox.x1.max(0).min(i64::from(image_w)) as u32;
                    let ry1 = reg.bbox.y1.max(0).min(i64::from(image_h)) as u32;
                    let rx2 = reg.bbox.x2.max(0).min(i64::from(image_w)) as u32;
                    let ry2 = reg.bbox.y2.max(0).min(i64::from(image_h)) as u32;
                    let rw = rx2.saturating_sub(rx1);
                    let rh = ry2.saturating_sub(ry1);
                    if rw >= 16 && rh >= 16 {
                        let crop = image::imageops::crop_imm(base, rx1, ry1, rw, rh).to_image();
                        let tags = run_wd_tagger(state, DynamicImage::ImageRgba8(crop), 0.35).await;
                        det_tags.push(tags);
                    } else {
                        det_tags.push(Vec::new());
                    }
                }
            }

            let active_chars_ref: Vec<(usize, &NovelAiCharacter)> =
                active_chars_owned.iter().map(|(i, c)| (*i, c)).collect();
            let assigned = char_pr_pass::assign_regions_to_characters(
                &active_chars_ref,
                &regions,
                &det_tags,
                nai.use_coords,
                image_w,
                image_h,
            );
            let coord_fallback = nai.use_coords.then(|| {
                char_pr_pass::plan_coord_regions(
                    &active_chars_owned,
                    image_w,
                    image_h,
                    cfg.feather,
                    cfg.fits_free_window(),
                )
            });

            let mut resolved = HashMap::new();
            for group in &plan.character_groups {
                if let Some(&reg_idx) = assigned.get(&group.character_index) {
                    let self_region = &regions[reg_idx];
                    if let Some(crop_plan) = char_pr_pass::plan_character_crop(
                        &self_region.bbox,
                        image_w,
                        image_h,
                        cfg.padding,
                        cfg.fits_free_window(),
                    ) {
                        let others: Vec<&DetectedRegion> = regions
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i != reg_idx)
                            .map(|(_, r)| r)
                            .collect();
                        let mask_alpha = char_pr_pass::build_yolo_mask_alpha(
                            &crop_plan,
                            self_region,
                            &others,
                            cfg.feather,
                        );
                        resolved.insert(
                            group.character_index,
                            PlannedCharacterRegion {
                                plan: crop_plan,
                                mask_alpha,
                            },
                        );
                        continue;
                    }
                }
                if let Some(fallback) = coord_fallback
                    .as_ref()
                    .and_then(|m| m.get(&group.character_index))
                {
                    resolved.insert(group.character_index, fallback.clone());
                }
            }
            Ok(Some(resolved))
        }
        Ok(_) => {
            if nai.use_coords {
                log::info!(
                    "NovelAI {prompt_id}: person detector found no regions, falling back to coordinate regions"
                );
                Ok(Some(char_pr_pass::plan_coord_regions(
                    &active_chars_owned,
                    image_w,
                    image_h,
                    cfg.feather,
                    cfg.fits_free_window(),
                )))
            } else {
                log::info!("NovelAI {prompt_id}: person detector found no characters");
                notify(sink, prompt_id, "skipped", "no_characters");
                Ok(None)
            }
        }
        Err(err) => {
            if nai.use_coords {
                log::warn!(
                    "NovelAI {prompt_id}: person detector failed ({err}); falling back to coordinate regions"
                );
                Ok(Some(char_pr_pass::plan_coord_regions(
                    &active_chars_owned,
                    image_w,
                    image_h,
                    cfg.feather,
                    cfg.fits_free_window(),
                )))
            } else {
                log::warn!("NovelAI {prompt_id}: person detector failed ({err})");
                notify(sink, prompt_id, "failed", "detector_failed");
                Ok(None)
            }
        }
    }
}

/// Run isolated per-character Precise Reference `img2img` passes on `png` and
/// composite the results back over the base image.
pub async fn run_character_pr_pass(
    state: &Arc<AppState>,
    sink: &EventSink,
    prompt_id: &str,
    params: &GenerationParams,
    plan: &IsolationPlan,
    client: &NovelAiClient<'_>,
    png: &[u8],
) -> Result<Option<Vec<u8>>, AppError> {
    let Some(nai) = params.novelai.as_ref() else {
        return Ok(None);
    };
    let cfg = nai.multi_char_pr.clone();
    let mut base = decode_rgba(png.to_vec()).await?;

    let Some(mut planned_by_char) =
        resolve_planned_regions(state, sink, prompt_id, params, plan, &base, png).await?
    else {
        return Ok(None);
    };

    let work_items: Vec<(&CharacterPrGroup, PlannedCharacterRegion)> = plan
        .character_groups
        .iter()
        .filter_map(|group| {
            planned_by_char
                .remove(&group.character_index)
                .map(|region| (group, region))
        })
        .collect();

    if work_items.is_empty() {
        notify(sink, prompt_id, "skipped", "nothing_painted");
        return Ok(None);
    }

    let total = work_items.len();
    let steps = crop_steps(params.steps, &cfg);
    let max_progress = (total as u32).saturating_mul(steps);
    let mut painted = 0usize;
    let mut failure: Option<&'static str> = None;

    for (pass_idx, (group, region_plan)) in work_items.iter().enumerate() {
        if state.prompt_queue.is_cancelled(prompt_id) {
            break;
        }
        let crop = image::imageops::crop_imm(
            &base,
            region_plan.plan.x,
            region_plan.plan.y,
            region_plan.plan.w,
            region_plan.plan.h,
        )
        .to_image();
        let sent = match resize_rgba(crop, region_plan.plan.req_w, region_plan.plan.req_h).await {
            Ok(img) => img,
            Err(err) => {
                log::warn!("NovelAI {prompt_id}: character crop failed ({err})");
                failure = Some("readback_failed");
                break;
            }
        };
        let encoded = match encode_png(&sent).await {
            Ok(bytes) => base64::engine::general_purpose::STANDARD.encode(&bytes),
            Err(err) => {
                log::warn!("NovelAI {prompt_id}: character crop encode failed ({err})");
                failure = Some("request_rejected");
                break;
            }
        };

        let (char_pos, char_neg) = char_pr_pass::build_character_pass_prompt(
            &params.positive_prompt,
            &params.negative_prompt,
            &group.character,
        );
        log::info!(
            "NovelAI {prompt_id}: character pass {}/{total} (slot {}, refs={}) {}x{} -> {}x{}",
            pass_idx + 1,
            group.character_index + 1,
            group.references.len(),
            region_plan.plan.w,
            region_plan.plan.h,
            region_plan.plan.req_w,
            region_plan.plan.req_h,
        );

        let node = format!("NovelAI character {}/{total}", pass_idx + 1);
        let done_before = pass_idx as u32 * steps;
        sink.emit(
            "comfyui:progress",
            serde_json::json!({
                "prompt_id": prompt_id,
                "value": done_before,
                "max": max_progress,
                "node": node,
            }),
        );

        let crop_params = crop_request(
            params,
            &region_plan.plan,
            &char_pos,
            &char_neg,
            &group.references,
            encoded,
            pass_idx,
        );
        let body = match build_request(&crop_params) {
            Ok(body) => body,
            Err(err) => {
                log::warn!("NovelAI {prompt_id}: character request rejected ({err})");
                failure = Some("request_rejected");
                break;
            }
        };

        if pass_idx > 0 {
            tokio::time::sleep(CHAR_PR_GAP).await;
        }
        if state.prompt_queue.is_cancelled(prompt_id) {
            break;
        }

        let images = match client
            .generate_stream(&body, |event| {
                if state.prompt_queue.is_cancelled(prompt_id) {
                    return;
                }
                let StreamEvent::Intermediate { image, step, .. } = event else {
                    return;
                };
                if let Some(temp) = crate::temp_images::save(&image, "png") {
                    sink.emit(
                        "comfyui:preview",
                        serde_json::json!({
                            "temp_filename": temp, "format": "png", "prompt_id": prompt_id,
                        }),
                    );
                }
                sink.emit(
                    "comfyui:progress",
                    serde_json::json!({
                        "prompt_id": prompt_id,
                        "value": done_before + (step + 1).min(steps),
                        "max": max_progress,
                        "node": node,
                    }),
                );
            })
            .await
        {
            Ok(images) => images,
            Err(err) => {
                log::warn!("NovelAI {prompt_id}: {node} failed ({err}); keeping completed work");
                failure = Some("generate_failed");
                break;
            }
        };

        if state.prompt_queue.is_cancelled(prompt_id) {
            break;
        }
        let Some(result) = images.into_iter().next() else {
            failure = Some("empty_result");
            break;
        };
        let patch = match decode_and_fit(result, region_plan.plan.w, region_plan.plan.h).await {
            Ok(patch) => patch,
            Err(err) => {
                log::warn!("NovelAI {prompt_id}: {node} readback failed ({err})");
                failure = Some("readback_failed");
                break;
            }
        };
        char_pr_pass::composite_character(
            &mut base,
            &patch,
            &region_plan.plan,
            Some(&region_plan.mask_alpha),
            cfg.feather,
        );
        painted += 1;
    }

    if state.prompt_queue.is_cancelled(prompt_id) {
        return Ok(None);
    }
    if let Some(reason) = failure {
        let status = if painted > 0 { "partial" } else { "failed" };
        notify(sink, prompt_id, status, reason);
    } else if painted == 0 {
        notify(sink, prompt_id, "skipped", "nothing_painted");
    } else if painted < plan.character_groups.len() {
        notify(sink, prompt_id, "partial", "few_characters");
    }

    if painted == 0 {
        return Ok(None);
    }

    sink.emit(
        "comfyui:progress",
        serde_json::json!({
            "prompt_id": prompt_id,
            "value": max_progress,
            "max": max_progress,
            "node": "NovelAI characters",
        }),
    );

    let composite = encode_png(&base).await?;
    let restored = crate::metadata::copy_png_text_chunks(png, &composite);
    Ok(Some(restored.unwrap_or(composite)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::params::{NovelAiCoord, NovelAiParams, NovelAiVibe};

    fn sample_params() -> GenerationParams {
        let mut nai = NovelAiParams {
            model: "nai-diffusion-5-full".into(),
            action: "generate".into(),
            effort: crate::novelai::models::EFFORT_MEDIUM.into(),
            use_coords: true,
            local_post_process: true,
            characters: vec![
                NovelAiCharacter {
                    prompt: "1girl, blonde hair".into(),
                    center: NovelAiCoord { x: 0.25, y: 0.5 },
                    enabled: true,
                    ..Default::default()
                },
                NovelAiCharacter {
                    prompt: "1girl, black hair".into(),
                    center: NovelAiCoord { x: 0.75, y: 0.5 },
                    enabled: true,
                    ..Default::default()
                },
            ],
            director_references: vec![
                NovelAiDirectorReference {
                    image: "stylepng".into(),
                    description: "style".into(),
                    strength: 0.8,
                    fidelity: 1.0,
                    enabled: true,
                    target_character_index: None,
                    ..Default::default()
                },
                NovelAiDirectorReference {
                    image: "alicepng".into(),
                    description: "character".into(),
                    strength: 1.0,
                    fidelity: 1.0,
                    enabled: true,
                    target_character_index: Some(0),
                    sheet_x: Some(0.22),
                    sheet_y: Some(0.5),
                    sheet_scale: Some(0.8),
                },
                NovelAiDirectorReference {
                    image: "bobpng".into(),
                    description: "character_mainoutfit".into(),
                    strength: 1.2,
                    fidelity: 1.0,
                    enabled: true,
                    target_character_index: Some(1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        nai.multi_char_pr.enabled = true;
        nai.multi_char_pr.strategy = "passes".into();
        nai.multi_char_pr.strength = 0.65;
        nai.face_detail.enabled = true;

        GenerationParams {
            mode: "txt2img".into(),
            positive_prompt: "2girls, park".into(),
            negative_prompt: "lowres".into(),
            checkpoint: "nai-diffusion-5-full".into(),
            width: 1216,
            height: 832,
            steps: 36,
            batch_size: 1,
            seed: 500,
            novelai: Some(nai),
            ..Default::default()
        }
    }

    #[test]
    fn base_params_prep_supports_passes_sheet_and_hybrid() {
        let params = sample_params();
        let nai = params.novelai.as_ref().unwrap();
        let model = crate::novelai::models::find("nai-diffusion-5-full").unwrap();

        let pass_plan = char_pr_pass::partition_references(&params, nai, model, false)
            .unwrap()
            .unwrap();
        let stripped = apply_sheet_or_strip_base_params(&params, &pass_plan, None);
        let stripped_refs = &stripped.novelai.unwrap().director_references;
        assert_eq!(stripped_refs.len(), 1);
        assert_eq!(stripped_refs[0].image, "stylepng");

        let sheet_ref = NovelAiDirectorReference {
            image: "sheetpng".into(),
            description: "character_mainoutfit".into(),
            strength: 1.1,
            fidelity: 1.0,
            enabled: true,
            ..Default::default()
        };
        let with_sheet = apply_sheet_or_strip_base_params(&params, &pass_plan, Some(sheet_ref));
        let sheet_refs = &with_sheet.novelai.unwrap().director_references;
        assert_eq!(sheet_refs.len(), 2);
        assert_eq!(sheet_refs[0].image, "stylepng");
        assert_eq!(sheet_refs[1].image, "sheetpng");
        assert_eq!(sheet_refs[1].description, "character_mainoutfit");

        let mut hybrid_params = params.clone();
        hybrid_params
            .novelai
            .as_mut()
            .unwrap()
            .multi_char_pr
            .strategy = "hybrid".into();
        let hybrid_plan = char_pr_pass::partition_references(
            &hybrid_params,
            hybrid_params.novelai.as_ref().unwrap(),
            model,
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            hybrid_plan.strategy,
            char_pr_pass::MultiCharPrStrategy::Hybrid
        );
        assert!(hybrid_plan.strategy.uses_sheet());
        assert!(hybrid_plan.strategy.uses_passes());
    }

    #[test]
    fn crop_request_isolates_character_pr_clears_coords_and_clamps_steps() {
        let mut params = sample_params();
        params.novelai.as_mut().unwrap().vibes = vec![NovelAiVibe {
            encoding: Some("enc".into()),
            ..Default::default()
        }];
        let plan = CropPlan {
            x: 50,
            y: 40,
            w: 400,
            h: 700,
            req_w: 768,
            req_h: 1344,
        };
        let refs = vec![NovelAiDirectorReference {
            image: "alicepng".into(),
            description: "character".into(),
            strength: 1.0,
            fidelity: 1.0,
            enabled: true,
            target_character_index: Some(0),
            sheet_x: Some(0.22),
            sheet_y: Some(0.5),
            sheet_scale: Some(0.8),
        }];

        let req = crop_request(
            &params,
            &plan,
            "1girl, blonde hair, solo",
            "lowres",
            &refs,
            "AAAA".into(),
            1,
        );
        assert_eq!(req.width, 768);
        assert_eq!(req.height, 1344);
        assert_eq!(req.batch_size, 1);
        assert_eq!(req.steps, face_pass::FREE_STEPS);
        assert_eq!(req.seed, 601);
        assert_eq!(req.input_image.as_deref(), Some("AAAA"));

        let nai = req.novelai.as_ref().unwrap();
        assert_eq!(nai.action, "img2img");
        assert_eq!(nai.effort, crate::novelai::models::EFFORT_HIGH);
        assert!((nai.strength - 0.65).abs() < 1e-9);
        assert!(nai.characters.is_empty());
        assert!(!nai.use_coords);
        assert!(nai.vibes.is_empty());
        assert!(!nai.local_post_process);
        assert!(!nai.face_detail.enabled);
        assert!(!nai.multi_char_pr.enabled);
        assert_eq!(nai.director_references.len(), 1);
        assert_eq!(nai.director_references[0].image, "alicepng");
        assert!(nai.director_references[0].target_character_index.is_none());

        let body = build_request(&req).expect("valid payload");
        assert_eq!(body["action"], "img2img");
        assert_eq!(body["parameters"]["use_coords"], false);
        assert_eq!(
            body["parameters"]["director_reference_images"],
            serde_json::json!(["alicepng"])
        );
    }
}
