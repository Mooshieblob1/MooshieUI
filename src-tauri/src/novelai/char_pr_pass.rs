//! Pure logic for experimental multi-character Precise Reference isolation.
//!
//! NovelAI's `director_reference_*` conditioning is global across the canvas,
//! so sending multiple character references in a single pass blends their traits.
//! This module partitions targeted references out of the base pass and plans
//! per-character `img2img` crop passes (from YOLO person segmentation masks or
//! coordinate Voronoi cells) plus mask-guided compositing and isolated prompts.
//!
//! Zero I/O and zero `tauri` dependencies so it compiles in both desktop and
//! server builds and is exhaustively unit-tested.

use std::collections::{BTreeMap, HashMap, HashSet};

use image::{imageops::FilterType, Rgba, RgbaImage};

use crate::comfyui::types::GenerationParams;
use crate::novelai::face_pass::{self, CropPlan};
use crate::novelai::models::{NovelAiModel, ReferenceCanvasMode};
use crate::novelai::params::{NovelAiCharacter, NovelAiDirectorReference, NovelAiParams};
use crate::novelai::reference_canvas;
use crate::templates::face_detect::{DetectedRegion, FaceBox};

/// Minimum crop side (in source pixels) worth sending as an isolated pass.
const MIN_CROP_SIDE: u32 = 32;
/// Minimum and maximum request sides on NovelAI's 64-px grid for character crops.
const MIN_REQUEST_SIDE: u32 = 256;
const MAX_REQUEST_SIDE: u32 = 1536;
const DIMENSION_STEP: u32 = 64;

/// Active references grouped for one character box.
#[derive(Debug, Clone)]
pub struct CharacterPrGroup {
    pub character_index: usize,
    pub character: NovelAiCharacter,
    pub references: Vec<NovelAiDirectorReference>,
}

/// Strategy for multi-character Precise Reference isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiCharPrStrategy {
    Sheet,
    Passes,
    Hybrid,
}

impl MultiCharPrStrategy {
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("passes") {
            Self::Passes
        } else if s.eq_ignore_ascii_case("hybrid") {
            Self::Hybrid
        } else {
            Self::Sheet
        }
    }

    pub fn uses_sheet(self) -> bool {
        matches!(self, Self::Sheet | Self::Hybrid)
    }

    pub fn uses_passes(self) -> bool {
        matches!(self, Self::Passes | Self::Hybrid)
    }
}

/// Partitioned base + per-character reference plan for a multi-character run.
#[derive(Debug, Clone)]
pub struct IsolationPlan {
    pub strategy: MultiCharPrStrategy,
    /// References kept on the initial base composition pass (e.g. Style, untargeted).
    pub base_references: Vec<NovelAiDirectorReference>,
    /// All active enabled non-empty character box indices in `nai.characters`.
    pub active_character_indices: Vec<usize>,
    /// Sorted by `character_index` ascending.
    pub character_groups: Vec<CharacterPrGroup>,
}

/// Non-fatal reasons an eligible isolation run skips the character passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationSkipReason {
    Batch,
    Transparency,
}

impl IsolationSkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Batch => "batch",
            Self::Transparency => "transparency",
        }
    }
}

/// Planned crop rectangle + alpha blend mask in crop-local row-major order.
#[derive(Debug, Clone)]
pub struct PlannedCharacterRegion {
    pub plan: CropPlan,
    pub mask_alpha: Vec<f32>,
}

/// Partition `nai.director_references` into base-pass references and isolated
/// per-character groups.
///
/// Returns `Ok(None)` silently when multi-character isolation is off or not
/// applicable (unsupported mode/model, fewer than 2 active characters, or no
/// valid targeted non-style reference). Returns `Err(reason)` when a pass-based
/// strategy (`Passes` or `Hybrid`) is blocked by batch > 1 or transparency.
pub fn partition_references(
    params: &GenerationParams,
    nai: &NovelAiParams,
    model: &NovelAiModel,
    transparency: bool,
) -> Result<Option<IsolationPlan>, IsolationSkipReason> {
    if !nai.multi_char_pr.enabled || !model.precise_reference {
        return Ok(None);
    }
    let strategy = MultiCharPrStrategy::parse(&nai.multi_char_pr.strategy);
    if strategy.uses_passes() {
        if params.mode != "txt2img" || !matches!(nai.action.as_str(), "" | "generate") {
            return Ok(None);
        }
    } else if !matches!(params.mode.as_str(), "txt2img" | "img2img")
        || !matches!(nai.action.as_str(), "" | "generate" | "img2img")
    {
        return Ok(None);
    }

    let mut active_indices: Vec<usize> = nai
        .characters
        .iter()
        .enumerate()
        .filter(|(_, c)| c.enabled && !c.prompt.trim().is_empty())
        .map(|(idx, _)| idx)
        .collect();
    active_indices.truncate(model.max_characters);
    if active_indices.len() < 2 {
        return Ok(None);
    }
    let active_set: HashSet<usize> = active_indices.iter().copied().collect();

    let mut base_references = Vec::new();
    let mut by_char: BTreeMap<usize, Vec<NovelAiDirectorReference>> = BTreeMap::new();

    for reference in &nai.director_references {
        let active = reference.enabled
            && !reference.image.is_empty()
            && (model.outfit_references || reference.description != "costume");
        if !active {
            base_references.push(reference.clone());
            continue;
        }
        if reference.description != "style" {
            if let Some(idx) = reference.target_character_index {
                if active_set.contains(&idx) {
                    by_char.entry(idx).or_default().push(reference.clone());
                    continue;
                }
            }
        }
        base_references.push(reference.clone());
    }

    if by_char.is_empty() {
        return Ok(None);
    }

    if strategy.uses_passes() {
        if params.batch_size > 1 {
            return Err(IsolationSkipReason::Batch);
        }
        if transparency {
            return Err(IsolationSkipReason::Transparency);
        }
    }

    let character_groups = by_char
        .into_iter()
        .map(|(character_index, references)| CharacterPrGroup {
            character_index,
            character: nai.characters[character_index].clone(),
            references,
        })
        .collect();

    Ok(Some(IsolationPlan {
        strategy,
        base_references,
        active_character_indices: active_indices,
        character_groups,
    }))
}

fn snap_nearest_side(px: u32) -> u32 {
    (((px + DIMENSION_STEP / 2) / DIMENSION_STEP) * DIMENSION_STEP)
        .clamp(MIN_REQUEST_SIDE, MAX_REQUEST_SIDE)
}

/// Compute a NovelAI-legal `(req_w, req_h)` on the 64-px grid preserving the
/// crop aspect ratio and staying inside the 1 MP Opus window when `fit_free`
/// is true.
pub fn compute_request_size(crop_w: u32, crop_h: u32, fit_free: bool) -> (u32, u32) {
    let w = crop_w.max(1);
    let h = crop_h.max(1);
    let target_pixels = if fit_free {
        face_pass::FREE_PIXELS as f64
    } else {
        1024.0 * 1536.0
    };
    let mut scale = (target_pixels / (f64::from(w) * f64::from(h))).sqrt();
    let max_side = f64::from(w.max(h));
    if max_side * scale > MAX_REQUEST_SIDE as f64 {
        scale = MAX_REQUEST_SIDE as f64 / max_side;
    }

    let mut req_w = snap_nearest_side((f64::from(w) * scale).round() as u32);
    let mut req_h = snap_nearest_side((f64::from(h) * scale).round() as u32);

    if fit_free {
        let src_aspect = f64::from(w) / f64::from(h);
        while u64::from(req_w) * u64::from(req_h) > face_pass::FREE_PIXELS
            && (req_w > MIN_REQUEST_SIDE || req_h > MIN_REQUEST_SIDE)
        {
            let can_shrink_w = req_w > MIN_REQUEST_SIDE;
            let can_shrink_h = req_h > MIN_REQUEST_SIDE;
            match (can_shrink_w, can_shrink_h) {
                (true, true) => {
                    let err_w =
                        ((f64::from(req_w - DIMENSION_STEP) / f64::from(req_h)) - src_aspect).abs();
                    let err_h =
                        ((f64::from(req_w) / f64::from(req_h - DIMENSION_STEP)) - src_aspect).abs();
                    if err_w <= err_h {
                        req_w -= DIMENSION_STEP;
                    } else {
                        req_h -= DIMENSION_STEP;
                    }
                }
                (true, false) => req_w -= DIMENSION_STEP,
                (false, true) => req_h -= DIMENSION_STEP,
                (false, false) => break,
            }
        }
    }

    (req_w, req_h)
}

/// Plan a rectangular crop around a detected person bounding box, padding X and Y
/// independently so tall full-body boxes stay portrait instead of pulling in
/// adjacent characters.
pub fn plan_character_crop(
    bbox: &FaceBox,
    image_w: u32,
    image_h: u32,
    padding: f64,
    fit_free: bool,
) -> Option<CropPlan> {
    if image_w == 0 || image_h == 0 {
        return None;
    }
    let bw = bbox.width() as f64;
    let bh = bbox.height() as f64;
    if bw <= 0.0 || bh <= 0.0 {
        return None;
    }

    let cx = (bbox.x1 + bbox.x2) as f64 / 2.0;
    let cy = (bbox.y1 + bbox.y2) as f64 / 2.0;
    let pad = padding.clamp(1.0, 2.0);
    let crop_w = (bw * pad).round();
    let crop_h = (bh * pad).round();

    let x1 = (cx - crop_w / 2.0).floor().max(0.0) as u32;
    let y1 = (cy - crop_h / 2.0).floor().max(0.0) as u32;
    let x2 = (cx + crop_w / 2.0).ceil().clamp(0.0, f64::from(image_w)) as u32;
    let y2 = (cy + crop_h / 2.0).ceil().clamp(0.0, f64::from(image_h)) as u32;

    let w = x2.saturating_sub(x1);
    let h = y2.saturating_sub(y1);
    if w < MIN_CROP_SIDE || h < MIN_CROP_SIDE {
        return None;
    }

    let (req_w, req_h) = compute_request_size(w, h, fit_free);
    Some(CropPlan {
        x: x1,
        y: y1,
        w,
        h,
        req_w,
        req_h,
    })
}

/// Partition the image canvas into smooth Voronoi regions around each active
/// character's placed coordinates (`center.x`, `center.y`).
pub fn plan_coord_regions(
    active_characters: &[(usize, NovelAiCharacter)],
    image_w: u32,
    image_h: u32,
    feather: u32,
    fit_free: bool,
) -> HashMap<usize, PlannedCharacterRegion> {
    let mut out = HashMap::new();
    if active_characters.is_empty() || image_w == 0 || image_h == 0 {
        return out;
    }

    let centers: Vec<(f32, f32)> = active_characters
        .iter()
        .map(|(_, c)| {
            (
                (c.center.x.clamp(0.0, 1.0) as f32) * image_w as f32,
                (c.center.y.clamp(0.0, 1.0) as f32) * image_h as f32,
            )
        })
        .collect();

    let n = active_characters.len();
    let mut bounds = vec![(image_w, image_h, 0u32, 0u32); n];

    // Coarse stride for fast bounding-box discovery on large canvases, snapped out by stride.
    let stride = if image_w.max(image_h) > 1024 { 2 } else { 1 };
    let mut y = 0u32;
    while y < image_h {
        let yf = y as f32 + 0.5;
        let mut x = 0u32;
        while x < image_w {
            let xf = x as f32 + 0.5;
            let mut best_i = 0usize;
            let mut best_d2 = f32::INFINITY;
            for (i, &(cx, cy)) in centers.iter().enumerate() {
                let dx = xf - cx;
                let dy = yf - cy;
                let d2 = dx * dx + dy * dy;
                if d2 < best_d2 {
                    best_d2 = d2;
                    best_i = i;
                }
            }
            let b = &mut bounds[best_i];
            b.0 = b.0.min(x);
            b.1 = b.1.min(y);
            b.2 = b.2.max((x + stride).min(image_w));
            b.3 = b.3.max((y + stride).min(image_h));
            x += stride;
        }
        y += stride;
    }

    let margin = (feather.saturating_mul(2)).max(image_w.min(image_h) / 10);
    let transition = (feather.saturating_mul(2))
        .max(image_w.min(image_h) / 12)
        .max(1) as f32;

    for (i, (char_idx, _)) in active_characters.iter().enumerate() {
        let (bx1, by1, bx2, by2) = bounds[i];
        let (x1, y1, x2, y2) = if bx1 >= bx2 || by1 >= by2 {
            // Degenerate identical coordinate tie: use a centered window around `centers[i]`.
            let half_w = (image_w / 3).max(MIN_CROP_SIDE);
            let half_h = (image_h / 2).max(MIN_CROP_SIDE);
            let cx = centers[i].0.round() as u32;
            let cy = centers[i].1.round() as u32;
            (
                cx.saturating_sub(half_w),
                cy.saturating_sub(half_h),
                (cx + half_w).min(image_w),
                (cy + half_h).min(image_h),
            )
        } else {
            (
                bx1.saturating_sub(margin),
                by1.saturating_sub(margin),
                bx2.saturating_add(margin).min(image_w),
                by2.saturating_add(margin).min(image_h),
            )
        };
        let w = x2.saturating_sub(x1);
        let h = y2.saturating_sub(y1);
        if w < MIN_CROP_SIDE || h < MIN_CROP_SIDE {
            continue;
        }
        let (req_w, req_h) = compute_request_size(w, h, fit_free);
        let plan = CropPlan {
            x: x1,
            y: y1,
            w,
            h,
            req_w,
            req_h,
        };

        let mut mask_alpha = vec![1.0f32; (w as usize) * (h as usize)];
        let (self_cx, self_cy) = centers[i];
        for row in 0..h {
            let yf = (y1 + row) as f32 + 0.5;
            for col in 0..w {
                let xf = (x1 + col) as f32 + 0.5;
                let d_self = ((xf - self_cx).powi(2) + (yf - self_cy).powi(2)).sqrt();
                let mut d_other = f32::INFINITY;
                for (j, &(ox, oy)) in centers.iter().enumerate() {
                    if j == i {
                        continue;
                    }
                    let d = ((xf - ox).powi(2) + (yf - oy).powi(2)).sqrt();
                    if d < d_other {
                        d_other = d;
                    }
                }
                let alpha = if d_other.is_infinite() {
                    1.0
                } else {
                    let signed_dist = (d_other - d_self) * 0.5;
                    let t = ((signed_dist / transition) + 0.5).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                mask_alpha[(row as usize) * (w as usize) + (col as usize)] = alpha;
            }
        }

        out.insert(*char_idx, PlannedCharacterRegion { plan, mask_alpha });
    }

    out
}

fn clean_tag_token(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = if let Some((_, rest)) = trimmed.split_once("::") {
        rest.trim_end_matches("::")
    } else {
        trimmed
    };
    stripped
        .trim_matches(|c: char| matches!(c, '{' | '}' | '[' | ']' | '(' | ')' | ' ' | '\t'))
        .to_lowercase()
        .replace('_', " ")
}

fn prompt_tokens_for_match(prompt: &str) -> (HashSet<String>, HashSet<String>) {
    let stop_words: HashSet<&str> = [
        "a", "an", "the", "in", "on", "at", "with", "and", "or", "of", "to", "is", "are", "was",
        "by", "for", "from", "her", "his", "she", "he", "standing", "sitting", "looking", "solo",
        "portrait", "upper", "body", "full",
    ]
    .into_iter()
    .collect();

    let mut exact_tags = HashSet::new();
    let mut words = HashSet::new();
    for piece in prompt.split([',', '\n']) {
        let clean = clean_tag_token(piece);
        if clean.is_empty() {
            continue;
        }
        exact_tags.insert(clean.clone());
        for word in clean.split_whitespace() {
            let w = word.trim_matches(|c: char| !c.is_alphanumeric());
            if w.len() >= 3 && !stop_words.contains(w) {
                words.insert(w.to_string());
            }
        }
    }
    (exact_tags, words)
}

fn gender_signal(tags: &HashSet<String>, words: &HashSet<String>) -> Option<i8> {
    let has_female = tags.contains("1girl")
        || tags.contains("girl")
        || tags.contains("woman")
        || words.contains("girl")
        || words.contains("woman")
        || words.contains("female");
    let has_male = tags.contains("1boy")
        || tags.contains("boy")
        || tags.contains("man")
        || words.contains("boy")
        || words.contains("man")
        || words.contains("male");
    match (has_female, has_male) {
        (true, false) => Some(1),
        (false, true) => Some(-1),
        _ => None,
    }
}

fn tag_match_score(char_prompt: &str, det_tags: &[String]) -> f64 {
    let (char_tags, char_words) = prompt_tokens_for_match(char_prompt);
    let mut det_exact = HashSet::new();
    let mut det_words = HashSet::new();
    for raw in det_tags {
        let clean = clean_tag_token(raw);
        if clean.is_empty() {
            continue;
        }
        det_exact.insert(clean.clone());
        for w in clean.split_whitespace() {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric());
            if w.len() >= 3 {
                det_words.insert(w.to_string());
            }
        }
    }

    let mut score = 0.0;
    for tag in &char_tags {
        if det_exact.contains(tag) {
            score += 3.0;
        }
    }
    for word in &char_words {
        if det_words.contains(word) {
            score += 1.0;
        }
    }
    if let (Some(cg), Some(dg)) = (
        gender_signal(&char_tags, &char_words),
        gender_signal(&det_exact, &det_words),
    ) {
        if cg == dg {
            score += 2.0;
        } else {
            score -= 2.0;
        }
    }
    score
}

/// Exact 1-to-1 minimum-cost assignment for a rectangular matrix using the
/// Hungarian (Kuhn-Munkres) algorithm.
pub fn hungarian_assign(costs: &[Vec<f64>]) -> Vec<Option<usize>> {
    let rows = costs.len();
    let cols = costs.first().map_or(0, Vec::len);
    if rows == 0 || cols == 0 {
        return vec![None; rows];
    }

    let n = rows.max(cols);
    let mut max_abs = 0.0_f64;
    for row in costs {
        for &val in row {
            if val.is_finite() {
                max_abs = max_abs.max(val.abs());
            }
        }
    }
    let dummy_cost = max_abs + 1_000_000.0;

    // 1-indexed potentials and matching arrays.
    let mut a = vec![vec![dummy_cost; n + 1]; n + 1];
    for r in 0..rows {
        for c in 0..cols {
            let v = costs[r][c];
            a[r + 1][c + 1] = if v.is_finite() { v } else { dummy_cost };
        }
    }

    let mut u = vec![0.0_f64; n + 1];
    let mut v = vec![0.0_f64; n + 1];
    let mut p = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];

    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![f64::INFINITY; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0usize;
            for j in 1..=n {
                if !used[j] {
                    let cur = a[i0][j] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }

    let mut result = vec![None; rows];
    for (col_1, &row_1) in p.iter().enumerate().skip(1) {
        if row_1 >= 1 && row_1 <= rows && col_1 >= 1 && col_1 <= cols {
            result[row_1 - 1] = Some(col_1 - 1);
        }
    }
    result
}

/// Match all active characters 1-to-1 against detected person regions.
///
/// Running assignment across *all* active characters prevents a targeted
/// character from claiming an untargeted character's detection. Returns a map
/// from `character_index` to index in `regions`.
pub fn assign_regions_to_characters(
    characters: &[(usize, &NovelAiCharacter)],
    regions: &[DetectedRegion],
    det_tags: &[Vec<String>],
    use_coords: bool,
    image_w: u32,
    image_h: u32,
) -> HashMap<usize, usize> {
    let mut map = HashMap::new();
    if characters.is_empty() || regions.is_empty() || image_w == 0 || image_h == 0 {
        return map;
    }

    let norm_centers: Vec<(f64, f64)> = regions
        .iter()
        .map(|r| {
            (
                ((r.bbox.x1 + r.bbox.x2) as f64 / 2.0) / f64::from(image_w),
                ((r.bbox.y1 + r.bbox.y2) as f64 / 2.0) / f64::from(image_h),
            )
        })
        .collect();

    let has_tags = !use_coords && det_tags.iter().any(|t| !t.is_empty());
    if !use_coords && !has_tags {
        // Without custom coordinates or tagger cues, pair the strongest N
        // detections sorted left-to-right with characters in slot order.
        let count = characters.len().min(regions.len());
        let mut strongest: Vec<usize> = (0..count).collect();
        strongest.sort_by(|&a, &b| {
            norm_centers[a]
                .0
                .partial_cmp(&norm_centers[b].0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for ((char_idx, _), region_idx) in characters.iter().zip(strongest.into_iter()) {
            map.insert(*char_idx, region_idx);
        }
        return map;
    }

    let m = characters.len();
    let mut costs = Vec::with_capacity(m);
    for (slot, (_, ch)) in characters.iter().enumerate() {
        let mut row = Vec::with_capacity(regions.len());
        let expected_x = if m > 1 {
            slot as f64 / (m - 1) as f64
        } else {
            0.5
        };
        for (k, &(px, py)) in norm_centers.iter().enumerate() {
            let cost = if use_coords {
                let dx = ch.center.x - px;
                let dy = ch.center.y - py;
                dx * dx + dy * dy
            } else {
                let tags = det_tags.get(k).map(Vec::as_slice).unwrap_or(&[]);
                let score = tag_match_score(&ch.prompt, tags);
                -(score * 100.0) + 0.01 * (expected_x - px).abs() + 0.001 * (k as f64)
            };
            row.push(cost);
        }
        costs.push(row);
    }

    let assignment = hungarian_assign(&costs);
    for ((char_idx, _), maybe_region) in characters.iter().zip(assignment.into_iter()) {
        if let Some(region_idx) = maybe_region {
            map.insert(*char_idx, region_idx);
        }
    }
    map
}

fn decode_mask_png_in_crop(region: &DetectedRegion, crop: &CropPlan) -> Option<Vec<f32>> {
    let bytes = region.mask_png.as_deref()?;
    let decoded = image::load_from_memory(bytes).ok()?;
    let cw = crop.w as usize;
    let ch = crop.h as usize;
    let mut out = vec![0.0_f32; cw * ch];
    if cw == 0 || ch == 0 {
        return Some(out);
    }

    let rx1 = region.bbox.x1.max(0) as u32;
    let ry1 = region.bbox.y1.max(0) as u32;
    let rx2 = region.bbox.x2.max(0) as u32;
    let ry2 = region.bbox.y2.max(0) as u32;
    let rw = rx2.saturating_sub(rx1);
    let rh = ry2.saturating_sub(ry1);
    if rw == 0 || rh == 0 {
        return Some(out);
    }

    let ox1 = rx1.max(crop.x);
    let oy1 = ry1.max(crop.y);
    let ox2 = rx2.min(crop.x + crop.w);
    let oy2 = ry2.min(crop.y + crop.h);
    if ox2 <= ox1 || oy2 <= oy1 {
        return Some(out);
    }

    let gray = if decoded.width() == rw && decoded.height() == rh {
        decoded.to_luma8()
    } else {
        decoded
            .resize_exact(rw, rh, FilterType::Triangle)
            .to_luma8()
    };
    for y in oy1..oy2 {
        let ly = y - ry1;
        let cy = (y - crop.y) as usize;
        for x in ox1..ox2 {
            let lx = x - rx1;
            let cx = (x - crop.x) as usize;
            out[cy * cw + cx] = f32::from(gray.get_pixel(lx, ly).0[0]) / 255.0;
        }
    }
    Some(out)
}

fn rasterize_region_in_crop(region: &DetectedRegion, crop: &CropPlan) -> Vec<f32> {
    if let Some(decoded) = decode_mask_png_in_crop(region, crop) {
        return decoded;
    }

    let cw = crop.w as usize;
    let ch = crop.h as usize;
    let mut out = vec![0.0_f32; cw * ch];
    if cw == 0 || ch == 0 {
        return out;
    }

    let rx1 = region.bbox.x1.max(0) as u32;
    let ry1 = region.bbox.y1.max(0) as u32;
    let rx2 = region.bbox.x2.max(0) as u32;
    let ry2 = region.bbox.y2.max(0) as u32;
    let rw = rx2.saturating_sub(rx1);
    let rh = ry2.saturating_sub(ry1);
    if rw == 0 || rh == 0 {
        return out;
    }

    let ox1 = rx1.max(crop.x);
    let oy1 = ry1.max(crop.y);
    let ox2 = rx2.min(crop.x + crop.w);
    let oy2 = ry2.min(crop.y + crop.h);
    if ox2 <= ox1 || oy2 <= oy1 {
        return out;
    }

    // Fallback when detector returned boxes without segmentation masks: soft ellipse.
    let half_w = (rw as f32 / 2.0).max(1.0);
    let half_h = (rh as f32 / 2.0).max(1.0);
    let rcx = rx1 as f32 + half_w;
    let rcy = ry1 as f32 + half_h;
    for y in oy1..oy2 {
        let cy = (y - crop.y) as usize;
        let dy = ((y as f32 + 0.5) - rcy) / half_h;
        for x in ox1..ox2 {
            let cx = (x - crop.x) as usize;
            let dx = ((x as f32 + 0.5) - rcx) / half_w;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = ((1.0 - dist) / 0.2).clamp(0.0, 1.0);
            out[cy * cw + cx] = alpha;
        }
    }
    out
}

fn dilate_1d(
    src: &[f32],
    dst: &mut [f32],
    len: usize,
    stride: usize,
    offset: usize,
    radius: usize,
) {
    if len == 0 {
        return;
    }
    if radius == 0 {
        for i in 0..len {
            dst[offset + i * stride] = src[offset + i * stride];
        }
        return;
    }
    let mut deque: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for i in 0..len.min(radius) {
        let val = src[offset + i * stride];
        while let Some(&back) = deque.back() {
            if src[offset + back * stride] <= val {
                deque.pop_back();
            } else {
                break;
            }
        }
        deque.push_back(i);
    }
    for i in 0..len {
        let enter = i + radius;
        if enter < len {
            let val = src[offset + enter * stride];
            while let Some(&back) = deque.back() {
                if src[offset + back * stride] <= val {
                    deque.pop_back();
                } else {
                    break;
                }
            }
            deque.push_back(enter);
        }
        if let Some(&front) = deque.front() {
            if front + radius < i {
                deque.pop_front();
            }
        }
        if let Some(&front) = deque.front() {
            dst[offset + i * stride] = src[offset + front * stride];
        }
    }
}

fn dilate_separable(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 || w == 0 || h == 0 {
        return src.to_vec();
    }
    let mut tmp = vec![0.0_f32; w * h];
    let mut out = vec![0.0_f32; w * h];
    for y in 0..h {
        dilate_1d(src, &mut tmp, w, 1, y * w, radius);
    }
    for x in 0..w {
        dilate_1d(&tmp, &mut out, h, w, x, radius);
    }
    out
}

fn box_blur_1d(
    src: &[f32],
    dst: &mut [f32],
    len: usize,
    stride: usize,
    offset: usize,
    radius: usize,
) {
    if len == 0 {
        return;
    }
    if radius == 0 {
        for i in 0..len {
            dst[offset + i * stride] = src[offset + i * stride];
        }
        return;
    }
    let mut prefix = Vec::with_capacity(len + 1);
    prefix.push(0.0_f64);
    let mut acc = 0.0_f64;
    for i in 0..len {
        acc += f64::from(src[offset + i * stride]);
        prefix.push(acc);
    }
    for i in 0..len {
        let lo = i.saturating_sub(radius);
        let hi = (i + radius + 1).min(len);
        let sum = prefix[hi] - prefix[lo];
        dst[offset + i * stride] = (sum / (hi - lo) as f64) as f32;
    }
}

fn blur_separable_twice(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 || w == 0 || h == 0 {
        return src.to_vec();
    }
    let mut cur = src.to_vec();
    let mut tmp = vec![0.0_f32; w * h];
    let mut out = vec![0.0_f32; w * h];
    for _ in 0..2 {
        for y in 0..h {
            box_blur_1d(&cur, &mut tmp, w, 1, y * w, radius);
        }
        for x in 0..w {
            box_blur_1d(&tmp, &mut out, h, w, x, radius);
        }
        cur.copy_from_slice(&out);
    }
    cur
}

/// Build a crop-local feathered alpha mask from `self_region`'s segmentation
/// mask while excluding any neighboring characters' cores so they are never
/// overwritten.
pub fn build_yolo_mask_alpha(
    plan: &CropPlan,
    self_region: &DetectedRegion,
    other_regions: &[&DetectedRegion],
    feather: u32,
) -> Vec<f32> {
    let w = plan.w as usize;
    let h = plan.h as usize;
    if w == 0 || h == 0 {
        return Vec::new();
    }

    let self_core = rasterize_region_in_crop(self_region, plan);
    let grow = (feather / 2).clamp(8, 28) as usize;
    let mut work = dilate_separable(&self_core, w, h, grow);

    for other in other_regions {
        let other_core = rasterize_region_in_crop(other, plan);
        for i in 0..work.len() {
            if other_core[i] > 0.5 && self_core[i] <= 0.5 {
                work[i] = 0.0;
            }
        }
    }

    let blur_radius = (feather.max(8) as usize).min(w.min(h) / 3).max(1);
    let blurred = blur_separable_twice(&work, w, h, blur_radius);
    blurred.into_iter().map(|v| v.clamp(0.0, 1.0)).collect()
}

/// Raised-cosine ramp along one axis that keeps canvas-boundary edges opaque
/// (`1.0`) while smoothly fading interior crop seams to `0.0`.
fn boundary_ramp(len: u32, feather: u32, fade_start: bool, fade_end: bool) -> Vec<f32> {
    let len_usize = len as usize;
    let mut v = vec![1.0_f32; len_usize];
    let f = (feather as usize).min(len_usize / 2);
    if f == 0 || (!fade_start && !fade_end) {
        return v;
    }
    for i in 0..f {
        let t = if f == 1 {
            0.0
        } else {
            std::f32::consts::PI * i as f32 / (f - 1) as f32
        };
        let value = 0.5 * (1.0 - t.cos());
        if fade_start {
            v[i] = value;
        }
        if fade_end {
            v[len_usize - 1 - i] = v[len_usize - 1 - i].min(value);
        }
    }
    v
}

/// Composite `patch` onto `base` at `plan`, combining optional `mask_alpha`
/// (in `plan.w * plan.h`) with canvas-boundary-aware interior feather ramps.
pub fn composite_character(
    base: &mut RgbaImage,
    patch: &RgbaImage,
    plan: &CropPlan,
    mask_alpha: Option<&[f32]>,
    feather: u32,
) {
    let w = plan
        .w
        .min(patch.width())
        .min(base.width().saturating_sub(plan.x));
    let h = plan
        .h
        .min(patch.height())
        .min(base.height().saturating_sub(plan.y));
    if w == 0 || h == 0 {
        return;
    }

    let extent = feather.max(12).min(w.min(h) / 3);
    let fade_left = plan.x > 0;
    let fade_right = plan.x.saturating_add(w) < base.width();
    let fade_top = plan.y > 0;
    let fade_bottom = plan.y.saturating_add(h) < base.height();

    let horizontal = boundary_ramp(w, extent, fade_left, fade_right);
    let vertical = boundary_ramp(h, extent, fade_top, fade_bottom);

    for y in 0..h {
        let vy = vertical[y as usize];
        let row_offset = (y as usize) * (plan.w as usize);
        for x in 0..w {
            let border_alpha = vy * horizontal[x as usize];
            let mask = mask_alpha
                .and_then(|m| m.get(row_offset + x as usize).copied())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            let alpha = border_alpha * mask;
            if alpha <= 0.0 {
                continue;
            }
            let src = patch.get_pixel(x, y).0;
            let dst = base.get_pixel_mut(plan.x + x, plan.y + y);
            for c in 0..3 {
                let blended = f32::from(src[c]) * alpha + f32::from(dst.0[c]) * (1.0 - alpha);
                dst.0[c] = blended.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

fn strip_text_block_for_pass(prompt: &str) -> &str {
    let mut offset = 0usize;
    for line in prompt.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.len() >= 5
            && trimmed.is_char_boundary(5)
            && trimmed[..5].eq_ignore_ascii_case("text:")
        {
            return &prompt[..offset];
        }
        offset += line.len();
    }
    prompt
}

fn is_unsafe_multi_char_item(clean_lower: &str) -> bool {
    if clean_lower.is_empty() {
        return true;
    }
    // Never carry long natural-language scene sentences that describe two characters.
    if clean_lower.len() > 60 {
        return true;
    }
    let multi_exact = [
        "multiple girls",
        "multiple boys",
        "multiple others",
        "couple",
        "duo",
        "trio",
        "group",
        "crowd",
        "holding hands",
        "hugging",
        "hug",
        "kissing",
        "kiss",
        "cuddling",
        "side-by-side",
        "side by side",
        "back-to-back",
        "back to back",
        "looking at each other",
        "eye contact",
        "facing each other",
        "1girl 1boy",
        "1boy 1girl",
        "hetero",
        "yuri",
        "yaoi",
    ];
    if multi_exact.iter().any(|p| clean_lower == *p) {
        return true;
    }

    // Count tags like `2girls`, `3boys`, `4others`.
    for suffix in ["girls", "boys", "others"] {
        if let Some(prefix) = clean_lower.strip_suffix(suffix) {
            if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit() || c == '+') {
                return true;
            }
        }
    }

    let multi_substrings = [
        "two ",
        "both ",
        "together",
        "each other",
        "between them",
        "standing next to",
        "on the left",
        "on the right",
    ];
    multi_substrings.iter().any(|p| clean_lower.contains(p))
}

fn push_unique_tag(seen: &mut HashSet<String>, out: &mut Vec<String>, raw: &str) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let key = clean_tag_token(trimmed);
    if key.is_empty() || !seen.insert(key) {
        return;
    }
    out.push(trimmed.to_string());
}

/// Build an isolated positive/negative prompt pair for one character's `img2img`
/// crop pass, prioritizing `character.prompt` first, asserting `solo` when
/// absent, and stripping multi-character count/interaction tags from the base
/// prompt while keeping safe artist/style/lighting context.
pub fn build_character_pass_prompt(
    base_positive: &str,
    base_negative: &str,
    character: &NovelAiCharacter,
) -> (String, String) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();

    for piece in character.prompt.split([',', '\n']) {
        push_unique_tag(&mut seen, &mut out, piece);
    }

    if !seen.contains("solo") {
        seen.insert("solo".to_string());
        out.push("solo".to_string());
    }

    let base_head = strip_text_block_for_pass(base_positive);
    for piece in base_head.split([',', '\n']) {
        let trimmed = piece.trim();
        if trimmed.is_empty() {
            continue;
        }
        let clean = clean_tag_token(trimmed);
        if clean.starts_with("artist:") {
            push_unique_tag(&mut seen, &mut out, trimmed);
            continue;
        }
        if is_unsafe_multi_char_item(&clean) {
            continue;
        }
        push_unique_tag(&mut seen, &mut out, trimmed);
    }

    let pos = out.join(", ");
    let base_neg = base_negative.trim();
    let char_neg = character.negative_prompt.trim();
    let neg = match (base_neg.is_empty(), char_neg.is_empty()) {
        (true, true) => String::new(),
        (false, true) => base_neg.to_string(),
        (true, false) => char_neg.to_string(),
        (false, false) => format!("{base_neg}, {char_neg}"),
    };
    (pos, neg)
}

/// Exact NovelAI Precise Reference canvas bucket for the target generation size.
pub fn sheet_canvas_size(gen_w: u32, gen_h: u32, mode: ReferenceCanvasMode) -> (u32, u32) {
    match mode {
        ReferenceCanvasMode::Bucket => {
            reference_canvas::choose_v5_bucket(gen_w.max(1), gen_h.max(1))
        }
        ReferenceCanvasMode::Letterbox => {
            reference_canvas::choose_canvas(gen_w.max(1), gen_h.max(1))
        }
    }
}

/// Automatic relative item size on the combined multi-character sheet.
pub fn auto_sheet_scale(canvas_w: u32, canvas_h: u32, item_count: usize) -> f64 {
    ((canvas_w as f64 / canvas_h.max(1) as f64) / (item_count.max(1) as f64 * 0.55))
        .clamp(0.45, 0.85)
}

/// Compute `(left, top, draw_w, draw_h)` on `canvas_w` x `canvas_h` for one item.
pub fn compute_sheet_item_rect(
    canvas_w: u32,
    canvas_h: u32,
    img_w: u32,
    img_h: u32,
    x: f64,
    y: f64,
    scale: f64,
) -> (i64, i64, u32, u32) {
    let cw = canvas_w.max(1) as f64;
    let ch = canvas_h.max(1) as f64;
    let iw = img_w.max(1) as f64;
    let ih = img_h.max(1) as f64;
    let s = if scale.is_finite() { scale } else { 0.65 }.clamp(0.20, 1.35);
    let cx = if x.is_finite() { x } else { 0.5 }.clamp(0.05, 0.95);
    let cy = if y.is_finite() { y } else { 0.5 }.clamp(0.05, 0.95);

    let factor = ((cw * s) / iw).min((ch * s) / ih);
    let draw_w = (iw * factor).round().max(1.0) as u32;
    let draw_h = (ih * factor).round().max(1.0) as u32;
    let left = (cx * cw - draw_w as f64 / 2.0).round() as i64;
    let top = (cy * ch - draw_h as f64 / 2.0).round() as i64;
    (left, top, draw_w, draw_h)
}

/// True when at least 2% of pixels carry real alpha transparency (`< 240`).
pub fn has_meaningful_alpha(img: &RgbaImage) -> bool {
    let total = (img.width() as usize).saturating_mul(img.height() as usize);
    if total == 0 {
        return false;
    }
    let transparent = img.pixels().filter(|p| p.0[3] < 240).count();
    transparent.saturating_mul(50) >= total
}

/// Apply a YOLO person segmentation mask to cut out the subject with clean
/// transparent pixels and tight bounding crop.
pub fn apply_person_cutout(img: &RgbaImage, region: &DetectedRegion) -> Option<RgbaImage> {
    let (iw, ih) = (img.width(), img.height());
    if iw < 16 || ih < 16 {
        return None;
    }
    let full_plan = CropPlan {
        x: 0,
        y: 0,
        w: iw,
        h: ih,
        req_w: iw,
        req_h: ih,
    };
    let raw_mask = decode_mask_png_in_crop(region, &full_plan)?;
    let w = iw as usize;
    let h = ih as usize;
    let dilate_radius = (w.min(h) / 200).clamp(2, 6);
    let dilated = dilate_separable(&raw_mask, w, h, dilate_radius);
    let blurred = blur_separable_twice(&dilated, w, h, 2);

    let mut masked = RgbaImage::new(iw, ih);
    let mut min_x = iw;
    let mut min_y = ih;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;

    for y in 0..ih {
        let row = (y as usize) * w;
        for x in 0..iw {
            let m = blurred[row + (x as usize)].clamp(0.0, 1.0);
            let orig = img.get_pixel(x, y).0;
            let a = (f32::from(orig[3]) * m).round().clamp(0.0, 255.0) as u8;
            let px = if a == 0 {
                Rgba([0, 0, 0, 0])
            } else {
                Rgba([orig[0], orig[1], orig[2], a])
            };
            masked.put_pixel(x, y, px);
            if a > 12 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if !any {
        return None;
    }

    let x1 = min_x.saturating_sub(4);
    let y1 = min_y.saturating_sub(4);
    let x2 = max_x.saturating_add(5).min(iw);
    let y2 = max_y.saturating_add(5).min(ih);
    let cw = x2.saturating_sub(x1);
    let ch = y2.saturating_sub(y1);
    if cw < 16 || ch < 16 {
        return None;
    }
    Some(image::imageops::crop_imm(&masked, x1, y1, cw, ch).to_image())
}

/// Choose the wire `description` value for a combined sheet from its members.
pub fn combined_sheet_description(
    refs: &[&NovelAiDirectorReference],
    model: &NovelAiModel,
) -> String {
    let any_char_style = refs.iter().any(|r| {
        r.description == "character&style" || r.description == "character&style_anyoutfit"
    });
    if any_char_style {
        if model.outfit_references
            && refs
                .iter()
                .any(|r| r.description == "character&style_anyoutfit")
        {
            return "character&style_anyoutfit".to_string();
        }
        return "character&style".to_string();
    }
    if model.outfit_references && refs.iter().any(|r| r.description == "character_mainoutfit") {
        return "character_mainoutfit".to_string();
    }
    if !refs.is_empty()
        && model.outfit_references
        && refs.iter().all(|r| r.description == "costume")
    {
        return "costume".to_string();
    }
    "character".to_string()
}

/// Resolved normalized placement and scale for one targeted reference on the
/// combined sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetPlacement {
    pub group_idx: usize,
    pub ref_idx: usize,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

/// Resolve automatic and user-overridden sheet coordinates/scales for every
/// targeted reference in `plan`.
pub fn resolve_sheet_placements(
    plan: &IsolationPlan,
    nai: &NovelAiParams,
    canvas_w: u32,
    canvas_h: u32,
) -> Vec<SheetPlacement> {
    let group_count = plan.character_groups.len().max(1);
    let total_items: usize = plan
        .character_groups
        .iter()
        .map(|g| g.references.len())
        .sum();
    let default_scale = auto_sheet_scale(canvas_w, canvas_h, total_items);
    let mut out = Vec::with_capacity(total_items);

    for (group_idx, group) in plan.character_groups.iter().enumerate() {
        let (base_x, base_y) = if nai.use_coords {
            (
                group.character.center.x.clamp(0.05, 0.95),
                group.character.center.y.clamp(0.05, 0.95),
            )
        } else {
            (((group_idx as f64) + 0.5) / group_count as f64, 0.5)
        };
        let n_in_group = group.references.len();
        for (ref_idx, reference) in group.references.iter().enumerate() {
            let spread = if n_in_group > 1 {
                (ref_idx as f64 - (n_in_group - 1) as f64 / 2.0) * 0.06
            } else {
                0.0
            };
            let auto_x = (base_x + spread).clamp(0.05, 0.95);
            let auto_y = base_y;
            let x = reference
                .sheet_x
                .filter(|v| v.is_finite())
                .unwrap_or(auto_x)
                .clamp(0.05, 0.95);
            let y = reference
                .sheet_y
                .filter(|v| v.is_finite())
                .unwrap_or(auto_y)
                .clamp(0.05, 0.95);
            let scale = reference
                .sheet_scale
                .filter(|v| v.is_finite())
                .unwrap_or(default_scale)
                .clamp(0.20, 1.35);
            out.push(SheetPlacement {
                group_idx,
                ref_idx,
                x,
                y,
                scale,
            });
        }
    }
    out
}

/// Decoded RGBA sprite and layout coordinates to composite onto the sheet.
#[derive(Debug, Clone)]
pub struct SheetEntry {
    pub image: RgbaImage,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

/// Render every entry onto a transparent (V5) or neutral white (V4.5) canvas.
pub fn build_combined_sheet_image(
    entries: &[SheetEntry],
    canvas_w: u32,
    canvas_h: u32,
    transparent_bg: bool,
) -> RgbaImage {
    let cw = canvas_w.max(1);
    let ch = canvas_h.max(1);
    let fill = if transparent_bg {
        Rgba([0, 0, 0, 0])
    } else {
        Rgba([255, 255, 255, 255])
    };
    let mut canvas = RgbaImage::from_pixel(cw, ch, fill);
    for entry in entries {
        let (iw, ih) = (entry.image.width(), entry.image.height());
        if iw == 0 || ih == 0 {
            continue;
        }
        let (left, top, draw_w, draw_h) =
            compute_sheet_item_rect(cw, ch, iw, ih, entry.x, entry.y, entry.scale);
        let resized = if (iw, ih) == (draw_w, draw_h) {
            entry.image.clone()
        } else {
            image::imageops::resize(&entry.image, draw_w, draw_h, FilterType::Lanczos3)
        };
        image::imageops::overlay(&mut canvas, &resized, left, top);
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::models;
    use crate::novelai::params::NovelAiCoord;
    use image::{DynamicImage, GrayImage, ImageFormat, Luma};
    use std::io::Cursor;

    fn char_entry(prompt: &str, x: f64, y: f64) -> NovelAiCharacter {
        NovelAiCharacter {
            prompt: prompt.into(),
            negative_prompt: String::new(),
            center: NovelAiCoord { x, y },
            enabled: true,
        }
    }

    fn ref_entry(
        image: &str,
        desc: &str,
        target: Option<usize>,
        enabled: bool,
    ) -> NovelAiDirectorReference {
        NovelAiDirectorReference {
            image: image.into(),
            description: desc.into(),
            strength: 1.0,
            fidelity: 1.0,
            enabled,
            target_character_index: target,
            ..Default::default()
        }
    }

    fn region(
        x1: i64,
        y1: i64,
        x2: i64,
        y2: i64,
        conf: f64,
        mask: Option<Vec<u8>>,
    ) -> DetectedRegion {
        DetectedRegion {
            bbox: FaceBox {
                x1,
                y1,
                x2,
                y2,
                confidence: conf,
            },
            mask_png: mask,
        }
    }

    fn mask_png_bytes(w: u32, h: u32, fill: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let mut img = GrayImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(x, y, Luma([fill(x, y)]));
            }
        }
        let mut buf = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(img)
            .write_to(&mut buf, ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn partition_keeps_style_global_separates_targets_and_checks_guards() {
        let v5 = models::find("nai-diffusion-5-full").unwrap();
        let mut params = GenerationParams {
            mode: "txt2img".into(),
            batch_size: 1,
            ..Default::default()
        };
        let mut nai = NovelAiParams {
            model: "nai-diffusion-5-full".into(),
            action: "generate".into(),
            characters: vec![
                char_entry("1girl, red hair", 0.25, 0.5),
                NovelAiCharacter {
                    enabled: false,
                    prompt: "ignored".into(),
                    ..Default::default()
                },
                char_entry("1girl, blue hair", 0.75, 0.5),
            ],
            director_references: vec![
                ref_entry("style_img", "style", Some(0), true),
                ref_entry("alice_img", "character", Some(0), true),
                ref_entry("bob_img", "character_mainoutfit", Some(2), true),
                ref_entry("disabled_target_img", "character", Some(1), true),
                ref_entry("muted_img", "character", Some(0), false),
            ],
            ..Default::default()
        };
        nai.multi_char_pr.enabled = true;

        // Back to passes checks.
        nai.multi_char_pr.strategy = "passes".into();
        let plan = partition_references(&params, &nai, v5, false)
            .unwrap()
            .expect("isolation plan");
        assert_eq!(plan.strategy, MultiCharPrStrategy::Passes);
        assert_eq!(plan.active_character_indices, vec![0, 2]);
        assert_eq!(plan.character_groups.len(), 2);
        assert_eq!(plan.character_groups[0].character_index, 0);
        assert_eq!(plan.character_groups[0].references.len(), 1);
        assert_eq!(plan.character_groups[0].references[0].image, "alice_img");
        assert_eq!(plan.character_groups[1].character_index, 2);
        assert_eq!(plan.character_groups[1].references[0].image, "bob_img");
        // Style (even if tagged), disabled slot target, and disabled ref stay out of groups.
        let base_imgs: Vec<&str> = plan
            .base_references
            .iter()
            .map(|r| r.image.as_str())
            .collect();
        assert_eq!(
            base_imgs,
            vec!["style_img", "disabled_target_img", "muted_img"]
        );

        // Sheet strategy allows batch > 1, transparency, and img2img.
        nai.multi_char_pr.strategy = "sheet".into();
        params.batch_size = 3;
        params.mode = "img2img".into();
        let sheet_plan = partition_references(&params, &nai, v5, true)
            .unwrap()
            .expect("sheet allows batch + transparency + img2img");
        assert_eq!(sheet_plan.strategy, MultiCharPrStrategy::Sheet);
        params.mode = "txt2img".into();

        // Hybrid strategy enforces txt2img, single batch, and opaque output.
        nai.multi_char_pr.strategy = "hybrid".into();
        assert_eq!(
            partition_references(&params, &nai, v5, false).unwrap_err(),
            IsolationSkipReason::Batch
        );
        params.batch_size = 1;
        assert_eq!(
            partition_references(&params, &nai, v5, true).unwrap_err(),
            IsolationSkipReason::Transparency
        );
        nai.multi_char_pr.strategy = "passes".into();

        // Fewer than 2 active characters -> silent None.
        let mut one_char = nai.clone();
        one_char.characters[2].enabled = false;
        assert!(partition_references(&params, &one_char, v5, false)
            .unwrap()
            .is_none());

        // Batch > 1 -> Err(Batch).
        params.batch_size = 2;
        assert_eq!(
            partition_references(&params, &nai, v5, false).unwrap_err(),
            IsolationSkipReason::Batch
        );

        // Transparency -> Err(Transparency).
        params.batch_size = 1;
        assert_eq!(
            partition_references(&params, &nai, v5, true).unwrap_err(),
            IsolationSkipReason::Transparency
        );

        // Off -> silent None even with batch > 1.
        nai.multi_char_pr.enabled = false;
        params.batch_size = 4;
        assert!(partition_references(&params, &nai, v5, false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn rectangular_person_crop_preserves_portrait_ratio_and_fits_free_window() {
        let tall = FaceBox {
            x1: 100,
            y1: 200,
            x2: 400,
            y2: 1100,
            confidence: 0.92,
        };
        let plan = plan_character_crop(&tall, 1216, 1216, 1.2, true).expect("valid crop");
        assert!(
            plan.h > plan.w,
            "expected portrait crop, got {}x{}",
            plan.w,
            plan.h
        );
        assert!(plan.req_h > plan.req_w);
        assert_eq!(plan.req_w % 64, 0);
        assert_eq!(plan.req_h % 64, 0);
        assert!(plan.is_free(28));

        let tiny = FaceBox {
            x1: 10,
            y1: 10,
            x2: 25,
            y2: 35,
            confidence: 0.9,
        };
        assert!(plan_character_crop(&tiny, 1024, 1024, 1.2, true).is_none());
    }

    #[test]
    fn hungarian_assign_and_region_matching_handle_swapped_coords_and_tags() {
        // Swapped custom coordinates: char 0 is on right, char 1 is on left.
        let c0 = char_entry("1girl, blonde hair", 0.78, 0.5);
        let c1 = char_entry("1girl, black hair", 0.22, 0.5);
        let chars = vec![(0, &c0), (1, &c1)];
        let regions = vec![
            region(60, 100, 360, 1100, 0.95, None),  // left (x~0.21)
            region(620, 100, 920, 1100, 0.90, None), // right (x~0.77)
            region(430, 300, 520, 700, 0.40, None),  // center extra bystander
        ];
        let matched = assign_regions_to_characters(&chars, &regions, &[], true, 1000, 1200);
        assert_eq!(matched.get(&0), Some(&1));
        assert_eq!(matched.get(&1), Some(&0));

        // Without coords: tagger disambiguates traits even when order is swapped.
        let tags = vec![
            vec!["1girl".into(), "black_hair".into(), "red_dress".into()],
            vec!["1girl".into(), "blonde_hair".into(), "blue_eyes".into()],
        ];
        let matched_tags =
            assign_regions_to_characters(&chars, &regions[..2], &tags, false, 1000, 1200);
        assert_eq!(matched_tags.get(&0), Some(&1));
        assert_eq!(matched_tags.get(&1), Some(&0));

        // Without coords and without tags: strongest N detections sorted left-to-right.
        let matched_lr = assign_regions_to_characters(&chars, &regions, &[], false, 1000, 1200);
        assert_eq!(matched_lr.get(&0), Some(&0));
        assert_eq!(matched_lr.get(&1), Some(&1));
    }

    #[test]
    fn coord_voronoi_regions_partition_canvas_and_blend_smoothly() {
        let chars = vec![
            (0, char_entry("1girl, left", 0.25, 0.5)),
            (1, char_entry("1girl, right", 0.75, 0.5)),
        ];
        let planned = plan_coord_regions(&chars, 800, 600, 24, true);
        assert_eq!(planned.len(), 2);
        let left = &planned[&0];
        let right = &planned[&1];
        assert!(left.plan.is_free(28));
        assert!(right.plan.is_free(28));
        assert_eq!(left.plan.x, 0);
        assert_eq!(right.plan.x + right.plan.w, 800);
        // Deep inside left cell -> alpha == 1.0.
        assert!((left.mask_alpha[10 * left.plan.w as usize + 50] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn mask_compositing_protects_neighbor_and_keeps_canvas_edges_opaque() {
        // Crop sits against left & bottom edges of a 200x200 image.
        let mut base = RgbaImage::from_pixel(200, 200, image::Rgba([0, 0, 0, 255]));
        let patch = RgbaImage::from_pixel(120, 120, image::Rgba([200, 200, 200, 255]));
        let plan = CropPlan {
            x: 0,
            y: 80,
            w: 120,
            h: 120,
            req_w: 512,
            req_h: 512,
        };
        let self_mask = mask_png_bytes(70, 100, |_, _| 255);
        let other_mask = mask_png_bytes(50, 100, |_, _| 255);
        let self_reg = region(10, 90, 80, 190, 0.95, Some(self_mask));
        let other_reg = region(70, 90, 120, 190, 0.90, Some(other_mask));

        let mask = build_yolo_mask_alpha(&plan, &self_reg, &[&other_reg], 12);
        composite_character(&mut base, &patch, &plan, Some(&mask), 12);

        // Interior of self near left/bottom canvas edge receives strong paint (left/bottom don't fade to 0).
        let self_px = base.get_pixel(20, 185).0[0];
        assert!(
            self_px > 180,
            "expected strong self paint near canvas edge, got {self_px}"
        );
        // Neighbor core outside self_core stays largely protected.
        let neighbor_px = base.get_pixel(105, 140).0[0];
        assert!(
            neighbor_px < 40,
            "expected neighbor core protected, got {neighbor_px}"
        );
        // Interior top/right crop edge (x=119, y=80) is 0.
        assert_eq!(base.get_pixel(119, 140).0[0], 0);
        assert_eq!(base.get_pixel(40, 80).0[0], 0);
    }

    #[test]
    fn character_prompt_strips_multi_char_and_text_blocks_while_keeping_artist_and_scene() {
        let character = NovelAiCharacter {
            prompt: "1girl, silver hair, blue eyes, red coat".into(),
            negative_prompt: "hat".into(),
            ..Default::default()
        };
        let (pos, neg) = build_character_pass_prompt(
            "2girls, multiple girls, artist:wlop, standing next to each other, cherry blossoms, golden hour, holding hands\nText: Hello",
            "lowres",
            &character,
        );
        assert!(pos.starts_with("1girl, silver hair, blue eyes, red coat, solo"));
        assert!(pos.contains("artist:wlop"), "got: {pos}");
        assert!(pos.contains("cherry blossoms"), "got: {pos}");
        assert!(pos.contains("golden hour"), "got: {pos}");
        for blocked in [
            "2girls",
            "multiple girls",
            "holding hands",
            "Hello",
            "Text:",
        ] {
            assert!(!pos.contains(blocked), "{blocked} leaked into: {pos}");
        }
        assert_eq!(neg, "lowres, hat");
    }

    #[test]
    fn sheet_canvas_and_item_rect_preserve_buckets_and_aspect() {
        assert_eq!(
            sheet_canvas_size(1216, 832, ReferenceCanvasMode::Bucket),
            (1216, 832)
        );
        assert_eq!(
            sheet_canvas_size(1216, 832, ReferenceCanvasMode::Letterbox),
            (1536, 1024)
        );
        let (left, top, dw, dh) = compute_sheet_item_rect(1200, 800, 500, 1000, 0.25, 0.5, 0.7);
        assert_eq!(dh, 560);
        assert_eq!(dw, 280);
        assert_eq!(left, 160);
        assert_eq!(top, 120);
    }

    #[test]
    fn cutout_segments_subject_zeros_bg_rgb_and_crops_tightly() {
        let mut src = RgbaImage::from_pixel(100, 100, Rgba([240, 30, 30, 255]));
        for y in 25..75 {
            for x in 35..65 {
                src.put_pixel(x, y, Rgba([20, 180, 220, 255]));
            }
        }
        assert!(!has_meaningful_alpha(&src));

        let mask = mask_png_bytes(30, 50, |_, _| 255);
        let reg = region(35, 25, 65, 75, 0.95, Some(mask));
        let cut = apply_person_cutout(&src, &reg).expect("cutout");
        assert!(has_meaningful_alpha(&cut));
        assert!(
            cut.width() < 60 && cut.height() < 75,
            "expected tight crop, got {}x{}",
            cut.width(),
            cut.height()
        );
        let corner = cut.get_pixel(0, 0).0;
        assert_eq!(corner, [0, 0, 0, 0]);
        let center = cut.get_pixel(cut.width() / 2, cut.height() / 2).0;
        assert_eq!(center[3], 255);
        assert_eq!(center[1], 180);

        let no_mask_reg = region(35, 25, 65, 75, 0.95, None);
        assert!(apply_person_cutout(&src, &no_mask_reg).is_none());
    }

    #[test]
    fn combined_sheet_places_chars_with_transparent_or_white_backdrop() {
        let left_sprite = RgbaImage::from_pixel(40, 80, Rgba([255, 0, 0, 255]));
        let right_sprite = RgbaImage::from_pixel(40, 80, Rgba([0, 0, 255, 255]));
        let entries = vec![
            SheetEntry {
                image: left_sprite,
                x: 0.25,
                y: 0.5,
                scale: 0.7,
            },
            SheetEntry {
                image: right_sprite,
                x: 0.75,
                y: 0.5,
                scale: 0.7,
            },
        ];

        let v5_sheet = build_combined_sheet_image(&entries, 400, 300, true);
        assert_eq!(v5_sheet.get_pixel(100, 150).0, [255, 0, 0, 255]);
        assert_eq!(v5_sheet.get_pixel(300, 150).0, [0, 0, 255, 255]);
        assert_eq!(v5_sheet.get_pixel(200, 10).0[3], 0);

        let v45_sheet = build_combined_sheet_image(&entries, 400, 300, false);
        assert_eq!(v45_sheet.get_pixel(200, 10).0, [255, 255, 255, 255]);
    }

    #[test]
    fn resolve_placements_and_sheet_description_respect_overrides_and_model_caps() {
        let v5 = models::find("nai-diffusion-5-full").unwrap();
        let v45 = models::find("nai-diffusion-4-5-full").unwrap();
        let params = GenerationParams {
            mode: "txt2img".into(),
            batch_size: 1,
            ..Default::default()
        };
        let mut r0 = ref_entry("a", "character&style", Some(0), true);
        r0.sheet_x = Some(0.18);
        r0.sheet_scale = Some(0.92);
        let r1 = ref_entry("b", "character_mainoutfit", Some(1), true);
        let mut nai = NovelAiParams {
            model: "nai-diffusion-5-full".into(),
            use_coords: false,
            characters: vec![
                char_entry("1girl, red", 0.2, 0.4),
                char_entry("1girl, blue", 0.8, 0.6),
            ],
            director_references: vec![r0.clone(), r1.clone()],
            ..Default::default()
        };
        nai.multi_char_pr.enabled = true;
        nai.multi_char_pr.strategy = "sheet".into();

        let plan = partition_references(&params, &nai, v5, false)
            .unwrap()
            .unwrap();
        let placements = resolve_sheet_placements(&plan, &nai, 1216, 832);
        assert_eq!(placements.len(), 2);
        assert!((placements[0].x - 0.18).abs() < 1e-6);
        assert!((placements[0].y - 0.5).abs() < 1e-6);
        assert!((placements[0].scale - 0.92).abs() < 1e-6);
        assert!((placements[1].x - 0.75).abs() < 1e-6);

        nai.use_coords = true;
        let coord_placements = resolve_sheet_placements(&plan, &nai, 1216, 832);
        assert!((coord_placements[1].x - 0.8).abs() < 1e-6);
        assert!((coord_placements[1].y - 0.6).abs() < 1e-6);

        let refs_slice = [&r0, &r1];
        assert_eq!(
            combined_sheet_description(&refs_slice, v5),
            "character&style"
        );
        let refs_outfit = [&r1];
        assert_eq!(
            combined_sheet_description(&refs_outfit, v5),
            "character_mainoutfit"
        );
        assert_eq!(combined_sheet_description(&refs_outfit, v45), "character");
    }
}
