//! The NovelAI face detailer's pure logic: crop geometry, the free-window
//! rule, the face prompt, and the feathered composite.
//!
//! Everything here is deliberately free of I/O so it can be unit tested, which
//! matters more than usual: every mistake in this file is paid for in Anlas or
//! in a visibly wrong face, and neither shows up in a compile error.
//!
//! The geometry is a port of `MooshieFaceDetailer.process` in
//! `comfyui/mooshie_nodes.py`. The two must agree, because the panel's sliders
//! drive both engines and a user switching between them expects the same crop.

use image::RgbaImage;

use crate::novelai::params::{NovelAiCharacter, NovelAiFaceDetail};
use crate::templates::face_detect::FaceBox;

/// The Opus free window's pixel ceiling.
///
/// Mirrors `novelAiOpusCovers` in `src/lib/utils/novelaiCost.ts`, which is the
/// frontend's copy of the same rule. The runtime decision (downscale, or warn
/// and bill) happens here, so the constants have to exist on both sides; the
/// test below pins them so a change to one is a change to a named number
/// rather than a silent divergence.
pub const FREE_PIXELS: u64 = 1_048_576;

/// The free window's step ceiling. A crop asking for more is billed in full,
/// so the request clamps to this rather than honouring a higher panel value.
pub const FREE_STEPS: u32 = 28;

/// NovelAI's dimension grid.
const DIMENSION_STEP: u32 = 64;

/// Framing that is always appended and never derived.
///
/// The point of the whole prompt exercise is that a face crop must not be
/// conditioned on full-body composition, so the one thing the prompt is
/// allowed to assert unconditionally is that it *is* a face crop.
pub const FRAMING_ANCHOR: &[&str] = &["portrait", "close-up", "face focus"];

/// Tagger output that describes a defect rather than the face.
///
/// The crop is being re-rendered because it looks wrong, so any tag the tagger
/// reads off that wrongness would ask NovelAI to reproduce it. Rating and
/// copyright tags are dropped by the caller for the same reason: neither says
/// anything about a face, and a series name pulls style.
const DEFECT_TAGS: &[&str] = &[
    "blurry",
    "blurry_background",
    "blurry_foreground",
    "depth_of_field",
    "lowres",
    "jpeg_artifacts",
    "bad_anatomy",
    "bad_hands",
    "3d",
    "realistic",
    "photorealistic",
    "monochrome",
    "greyscale",
    "sketch",
    "artist_name",
    "watermark",
    "signature",
    "web_address",
    "censored",
    "mosaic_censoring",
];

/// Round down onto NovelAI's grid, never below one full step.
fn snap_down(px: u32) -> u32 {
    (px / DIMENSION_STEP).max(1) * DIMENSION_STEP
}

/// Round to the nearest grid step, never below one full step.
fn snap_nearest(px: u32) -> u32 {
    ((px + DIMENSION_STEP / 2) / DIMENSION_STEP).max(1) * DIMENSION_STEP
}

/// Where a face is cut from, and what size it is sent at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropPlan {
    /// Source rectangle in the base image.
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// The size the crop is sent to NovelAI at. Multiples of 64, because
    /// NovelAI rejects anything else.
    pub req_w: u32,
    pub req_h: u32,
}

impl CropPlan {
    pub fn request_pixels(&self) -> u64 {
        u64::from(self.req_w) * u64::from(self.req_h)
    }

    /// Does this crop fit the Opus free window at the given step count?
    pub fn is_free(&self, steps: u32) -> bool {
        fits_free_window(self.req_w, self.req_h, steps, 1)
    }
}

/// The free window: one sample, at most a megapixel, at most 28 steps.
///
/// All three have to hold. A crop that satisfies two of them is billed exactly
/// like one that satisfies none.
pub fn fits_free_window(width: u32, height: u32, steps: u32, n_samples: u32) -> bool {
    u64::from(width) * u64::from(height) <= FREE_PIXELS && steps <= FREE_STEPS && n_samples <= 1
}

/// Shrink a request onto the free window, keeping the aspect ratio.
///
/// The uniform factor is `sqrt(free / pixels)`, and snapping *down* after it
/// can only remove pixels, so the result is inside the window by construction.
/// The loop is a backstop for the degenerate case where one side has already
/// bottomed out at a single grid step.
fn fit_into_free_window(width: u32, height: u32) -> (u32, u32) {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= FREE_PIXELS {
        return (width, height);
    }
    let factor = (FREE_PIXELS as f64 / pixels as f64).sqrt();
    let mut w = snap_down((f64::from(width) * factor) as u32);
    let mut h = snap_down((f64::from(height) * factor) as u32);
    while u64::from(w) * u64::from(h) > FREE_PIXELS && (w > DIMENSION_STEP || h > DIMENSION_STEP) {
        if w >= h {
            w = w.saturating_sub(DIMENSION_STEP).max(DIMENSION_STEP);
        } else {
            h = h.saturating_sub(DIMENSION_STEP).max(DIMENSION_STEP);
        }
    }
    (w, h)
}

/// Work out the crop rectangle and the request size for one face.
///
/// `None` means the face is not worth detailing: a crop under 8 px on a side
/// carries no detail to improve and would only cost a round trip.
///
/// `fit_free` is the `fit_free` Anlas policy. Under `allow_paid` the crop goes
/// out at its natural size and the caller warns about the cost.
pub fn plan_crop(
    face: &FaceBox,
    image_w: u32,
    image_h: u32,
    padding: f64,
    guide_size: u32,
    fit_free: bool,
) -> Option<CropPlan> {
    if image_w == 0 || image_h == 0 {
        return None;
    }
    let bw = face.width() as f64;
    let bh = face.height() as f64;
    if bw <= 0.0 || bh <= 0.0 {
        return None;
    }

    let cx = (face.x1 + face.x2) as f64 / 2.0;
    let cy = (face.y1 + face.y2) as f64 / 2.0;
    // A square window around the box's centre, like the Python node: a face
    // box is tight around the features, and img2img needs the jaw, hair line
    // and neck around them or it repaints a head that does not fit its body.
    let size = bw.max(bh) * padding.max(1.0);

    let x1 = (cx - size / 2.0).floor().max(0.0) as u32;
    let y1 = (cy - size / 2.0).floor().max(0.0) as u32;
    let x2 = (cx + size / 2.0).ceil().clamp(0.0, f64::from(image_w)) as u32;
    let y2 = (cy + size / 2.0).ceil().clamp(0.0, f64::from(image_h)) as u32;

    let w = x2.saturating_sub(x1);
    let h = y2.saturating_sub(y1);
    if w < 8 || h < 8 {
        return None;
    }

    // Scale the long side to `guide_size`. This deliberately runs in both
    // directions: a small face is enlarged so NovelAI has pixels to work with,
    // a large one is reduced so it stays inside the free window.
    let scale = f64::from(guide_size.max(DIMENSION_STEP)) / f64::from(w.max(h));
    let req_w = snap_nearest((f64::from(w) * scale).round() as u32);
    let req_h = snap_nearest((f64::from(h) * scale).round() as u32);
    let (req_w, req_h) = if fit_free {
        fit_into_free_window(req_w, req_h)
    } else {
        (req_w, req_h)
    };

    Some(CropPlan {
        x: x1,
        y: y1,
        w,
        h,
        req_w,
        req_h,
    })
}

/// Which character box, if any, this face belongs to.
///
/// Only meaningful with `use_coords`: without it NovelAI places characters
/// itself and the stored centres say nothing about where they landed. Returns
/// the character prompt, which is already an isolated per-character identity
/// description and so is exactly the right shape to condition a face on.
pub fn nearest_character<'a>(
    characters: &'a [NovelAiCharacter],
    use_coords: bool,
    plan: &CropPlan,
    image_w: u32,
    image_h: u32,
) -> Option<&'a NovelAiCharacter> {
    if !use_coords || image_w == 0 || image_h == 0 {
        return None;
    }
    let fx = (f64::from(plan.x) + f64::from(plan.w) / 2.0) / f64::from(image_w);
    let fy = (f64::from(plan.y) + f64::from(plan.h) / 2.0) / f64::from(image_h);

    characters
        .iter()
        .filter(|c| c.enabled && !c.prompt.trim().is_empty())
        .min_by(|a, b| {
            let d = |c: &NovelAiCharacter| (c.center.x - fx).powi(2) + (c.center.y - fy).powi(2);
            d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Which sources actually fed the prompt. Logged, and asserted on in tests:
/// the one outcome that must never happen is the base prompt reaching a face
/// crop whole, which is the live hazard in the local `facefix` chain today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacePromptTier {
    /// The user's own text, verbatim.
    Custom,
    /// The framing anchor alone.
    Generic,
    /// Prompt identity plus tagger attributes.
    Merged,
    /// The tagger only: the prompt had nothing face-relevant in it.
    TaggerOnly,
    /// Prompt identity only: no tagger, or it found nothing usable.
    IdentityOnly,
    /// Nothing was available. The anchor still holds the framing.
    AnchorOnly,
}

/// Canonical form used for de-duplication: NovelAI and the tagger disagree on
/// spaces versus underscores for the same tag.
fn canonical(tag: &str) -> String {
    tag.trim().to_lowercase().replace(' ', "_")
}

/// Pull `artist:` items back out of the prompt.
///
/// The tagger has an artist head but its recall is poor, and on NovelAI the
/// artist tag is most of the style. Taking it from the prompt keeps the crop
/// in the same style as the render it is being pasted into.
fn artist_items(prompt: &str) -> Vec<String> {
    prompt
        .split(',')
        .map(str::trim)
        .filter(|item| {
            item.trim_start_matches(['(', '[', '{'])
                .to_lowercase()
                .starts_with("artist:")
        })
        .map(str::to_string)
        .collect()
}

/// Keep only tagger output that belongs on a face.
///
/// A tagger run on a face crop structurally cannot emit `full body` or
/// `cowboy shot`, which is the whole reason it is preferred to parsing the
/// prompt. What it can emit is the damage it is being asked to repair, which
/// is what [`DEFECT_TAGS`] removes.
fn usable_tagger_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .filter(|tag| !tag.trim().is_empty())
        .filter(|tag| !DEFECT_TAGS.contains(&canonical(tag).as_str()))
        .map(|tag| tag.trim().replace('_', " "))
        .collect()
}

/// Build the prompt one face crop is rendered with.
///
/// `character_prompt` is the matching character box when there is one;
/// `tagger_tags` are the names the WD tagger read off this crop, still in
/// their underscore form. Neither is required: the fallback chain degrades to
/// the framing anchor alone rather than ever reaching for the full prompt.
pub fn build_face_prompt(
    detail: &NovelAiFaceDetail,
    base_prompt: &str,
    character_prompt: Option<&str>,
    tagger_tags: &[String],
) -> (String, FacePromptTier) {
    let mode = detail.prompt_mode.trim().to_lowercase();

    if mode == "custom" {
        let custom = detail.custom_prompt.trim();
        if !custom.is_empty() {
            return (custom.to_string(), FacePromptTier::Custom);
        }
        // An empty custom prompt is a half-configured panel, not a request to
        // fall back to the user's scene prompt.
        return (join_unique(&[], &[]), FacePromptTier::Generic);
    }
    if mode == "generic" {
        return (join_unique(&[], &[]), FacePromptTier::Generic);
    }

    // `auto`. Identity comes from the character box when the face maps to one,
    // because that text describes one person rather than the whole scene.
    let identity_source = character_prompt
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(base_prompt);
    let identity = crate::prompt_assistant::grounding::extract_face_tags(identity_source);
    let mut identity_items: Vec<String> = artist_items(base_prompt);
    identity_items.extend(
        identity
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    );
    let tagger_items = usable_tagger_tags(tagger_tags);

    let tier = match (identity_items.is_empty(), tagger_items.is_empty()) {
        (false, false) => FacePromptTier::Merged,
        (true, false) => FacePromptTier::TaggerOnly,
        (false, true) => FacePromptTier::IdentityOnly,
        (true, true) => FacePromptTier::AnchorOnly,
    };
    (join_unique(&identity_items, &tagger_items), tier)
}

/// Join identity, tagger and anchor into one prompt, first occurrence winning.
///
/// Order is deliberate: identity first because NovelAI weights early tokens
/// more heavily, the anchor last because it only has to be present.
fn join_unique(identity: &[String], tagger: &[String]) -> String {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let anchor = FRAMING_ANCHOR
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    for item in identity.iter().chain(tagger.iter()).chain(anchor.iter()) {
        let key = canonical(item);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(item.trim().to_string());
    }
    out.join(", ")
}

/// How wide the feather ramp is for a crop of this size.
///
/// The floor of a sixth of the short side is what hides NovelAI's slight
/// colour shift at low strength: a 20 px feather on a 1024 px crop is a hard
/// edge by comparison. Ported from the Python node, which applies the same
/// floor at its call site.
pub fn feather_extent(w: u32, h: u32, feather: u32) -> u32 {
    let requested = feather.max(w.min(h) / 6);
    requested.min(w.min(h) / 3)
}

/// One axis of the separable blend mask: 1.0 in the middle, a raised cosine
/// falling to 0 at both ends.
fn ramp_vector(len: u32, feather: u32) -> Vec<f32> {
    let len = len as usize;
    let mut v = vec![1.0f32; len];
    let f = (feather as usize).min(len / 2);
    if f == 0 {
        return v;
    }
    for i in 0..f {
        // `linspace(0, pi, f)` in the Python node: both endpoints included, so
        // the outermost pixel is fully transparent.
        let t = if f == 1 {
            0.0
        } else {
            std::f32::consts::PI * i as f32 / (f - 1) as f32
        };
        let value = 0.5 * (1.0 - t.cos());
        v[i] = value;
        v[len - 1 - i] = value;
    }
    v
}

/// Paste a rendered face back over its source rectangle.
///
/// `patch` must already be the crop's pixel size. The blend is done in pixel
/// space rather than with a NovelAI mask on purpose: NovelAI's masks are hard
/// and effectively latent-resolution, and infill would switch the request to
/// the model's inpainting variant, which on some V5 models is a different
/// model entirely.
pub fn composite_face(base: &mut RgbaImage, patch: &RgbaImage, plan: &CropPlan, feather: u32) {
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

    let extent = feather_extent(w, h, feather);
    let vertical = ramp_vector(h, extent);
    let horizontal = ramp_vector(w, extent);

    for y in 0..h {
        for x in 0..w {
            let alpha = vertical[y as usize] * horizontal[x as usize];
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::params::NovelAiCoord;

    fn face(x1: i64, y1: i64, x2: i64, y2: i64) -> FaceBox {
        FaceBox {
            x1,
            y1,
            x2,
            y2,
            confidence: 0.9,
        }
    }

    fn detail(mode: &str) -> NovelAiFaceDetail {
        NovelAiFaceDetail {
            prompt_mode: mode.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn the_free_window_constants_match_the_frontend_rule() {
        // Mirrors `novelAiOpusCovers` in src/lib/utils/novelaiCost.ts.
        assert_eq!(FREE_PIXELS, 1024 * 1024);
        assert_eq!(FREE_STEPS, 28);
        assert!(fits_free_window(1024, 1024, 28, 1));
        assert!(!fits_free_window(1088, 1024, 28, 1));
        assert!(!fits_free_window(1024, 1024, 29, 1));
        assert!(!fits_free_window(1024, 1024, 28, 2));
    }

    #[test]
    fn a_centred_face_is_padded_and_scaled_to_the_guide_size() {
        // A 200 px box at 1.5 padding is a 300 px square window.
        let plan = plan_crop(&face(400, 400, 600, 600), 1024, 1024, 1.5, 1024, true).unwrap();
        assert_eq!((plan.x, plan.y), (350, 350));
        assert_eq!((plan.w, plan.h), (300, 300));
        assert_eq!((plan.req_w, plan.req_h), (1024, 1024));
        assert!(plan.is_free(28));
    }

    #[test]
    fn an_edge_face_is_clamped_to_the_image_and_stays_rectangular() {
        let plan = plan_crop(&face(0, 0, 200, 200), 1024, 1024, 1.5, 1024, true).unwrap();
        assert_eq!((plan.x, plan.y), (0, 0));
        // The window would start at -50, so the crop is short on one side.
        assert_eq!((plan.w, plan.h), (250, 250));
        assert!(plan.is_free(28));
    }

    #[test]
    fn a_tiny_face_is_upscaled_rather_than_sent_at_its_own_size() {
        let plan = plan_crop(&face(100, 100, 140, 140), 1024, 1024, 1.5, 1024, true).unwrap();
        assert_eq!((plan.w, plan.h), (60, 60));
        assert_eq!((plan.req_w, plan.req_h), (1024, 1024));
    }

    #[test]
    fn a_face_under_eight_pixels_is_skipped() {
        assert!(plan_crop(&face(10, 10, 14, 14), 1024, 1024, 1.5, 1024, true).is_none());
    }

    #[test]
    fn an_upscaled_image_stays_inside_the_free_window_under_fit_free() {
        // 1664x2432 (a 2x upscale of 832x1216) with a 730 px face: the natural
        // crop is 1095 px, which would be billed.
        let plan = plan_crop(&face(400, 400, 1130, 1130), 1664, 2432, 1.5, 1536, true).unwrap();
        assert!(
            plan.request_pixels() <= FREE_PIXELS,
            "requested {}x{}",
            plan.req_w,
            plan.req_h
        );
        assert_eq!(plan.req_w % 64, 0);
        assert_eq!(plan.req_h % 64, 0);
        assert!(plan.is_free(28));
    }

    #[test]
    fn allow_paid_sends_the_crop_at_its_natural_size() {
        let plan = plan_crop(&face(400, 400, 1130, 1130), 1664, 2432, 1.5, 1536, false).unwrap();
        assert_eq!((plan.req_w, plan.req_h), (1536, 1536));
        assert!(!plan.is_free(28));
    }

    #[test]
    fn a_face_maps_to_the_nearest_enabled_character() {
        let characters = vec![
            NovelAiCharacter {
                prompt: "left girl".into(),
                center: NovelAiCoord { x: 0.25, y: 0.3 },
                enabled: true,
                ..Default::default()
            },
            NovelAiCharacter {
                prompt: "right girl".into(),
                center: NovelAiCoord { x: 0.75, y: 0.3 },
                enabled: true,
                ..Default::default()
            },
            NovelAiCharacter {
                prompt: "disabled girl".into(),
                center: NovelAiCoord { x: 0.76, y: 0.3 },
                enabled: false,
                ..Default::default()
            },
        ];
        let plan = plan_crop(&face(700, 200, 900, 400), 1024, 1024, 1.5, 1024, true).unwrap();
        let matched = nearest_character(&characters, true, &plan, 1024, 1024).unwrap();
        assert_eq!(matched.prompt, "right girl");

        // Without `use_coords` the stored centres say nothing about the render.
        assert!(nearest_character(&characters, false, &plan, 1024, 1024).is_none());
    }

    #[test]
    fn auto_merges_prompt_identity_with_tagger_tags_and_the_anchor() {
        let (prompt, tier) = build_face_prompt(
            &detail("auto"),
            "artist:wlop, 1girl, blue hair, red eyes, full body, standing, city street, night",
            None,
            &["smile".into(), "blush".into(), "blurry".into()],
        );
        assert_eq!(tier, FacePromptTier::Merged);
        assert!(prompt.starts_with("artist:wlop"), "got: {prompt}");
        for kept in ["1girl", "blue hair", "red eyes", "smile", "blush"] {
            assert!(prompt.contains(kept), "{kept} missing from: {prompt}");
        }
        for dropped in ["full body", "standing", "city street", "blurry"] {
            assert!(!prompt.contains(dropped), "{dropped} leaked into: {prompt}");
        }
        assert!(
            prompt.ends_with("portrait, close-up, face focus"),
            "got: {prompt}"
        );
    }

    #[test]
    fn auto_never_falls_back_to_the_whole_prompt() {
        // Nothing face-relevant, no tagger: the anchor is all that is left.
        let (prompt, tier) = build_face_prompt(
            &detail("auto"),
            "cowboy shot, standing, city street, night, rain",
            None,
            &[],
        );
        assert_eq!(tier, FacePromptTier::AnchorOnly);
        assert_eq!(prompt, "portrait, close-up, face focus");
    }

    #[test]
    fn the_character_box_wins_over_the_scene_prompt_for_identity() {
        let (prompt, _) = build_face_prompt(
            &detail("auto"),
            "2girls, blue hair, city street",
            Some("1girl, green eyes, twintails"),
            &[],
        );
        assert!(prompt.contains("green eyes"), "got: {prompt}");
        assert!(!prompt.contains("blue hair"), "got: {prompt}");
    }

    #[test]
    fn the_tagger_alone_still_produces_a_face_prompt() {
        let (prompt, tier) = build_face_prompt(
            &detail("auto"),
            "city street",
            None,
            &["closed_eyes".into()],
        );
        assert_eq!(tier, FacePromptTier::TaggerOnly);
        assert!(prompt.starts_with("closed eyes"), "got: {prompt}");
    }

    #[test]
    fn generic_and_empty_custom_both_reduce_to_the_anchor() {
        let (prompt, tier) = build_face_prompt(&detail("generic"), "1girl, blue hair", None, &[]);
        assert_eq!(tier, FacePromptTier::Generic);
        assert_eq!(prompt, "portrait, close-up, face focus");

        let (prompt, tier) = build_face_prompt(&detail("custom"), "1girl, blue hair", None, &[]);
        assert_eq!(tier, FacePromptTier::Generic);
        assert_eq!(prompt, "portrait, close-up, face focus");
    }

    #[test]
    fn custom_text_is_used_verbatim() {
        let mut d = detail("custom");
        d.custom_prompt = "  perfect face, detailed eyes  ".into();
        let (prompt, tier) = build_face_prompt(&d, "1girl, blue hair", None, &["smile".into()]);
        assert_eq!(tier, FacePromptTier::Custom);
        assert_eq!(prompt, "perfect face, detailed eyes");
    }

    #[test]
    fn the_feather_ramp_runs_from_zero_to_one_and_back() {
        let v = ramp_vector(100, 10);
        assert_eq!(v[0], 0.0);
        assert_eq!(v[99], 0.0);
        assert_eq!(v[50], 1.0);
        assert!(v[5] > 0.0 && v[5] < 1.0);
        // Symmetric, and monotonic through the ramp.
        assert!((v[3] - v[96]).abs() < 1e-6);
        assert!(v[3] < v[7]);
    }

    #[test]
    fn the_feather_floor_scales_with_the_crop() {
        // A 20 px feather on a 600 px crop is a hard edge, so the floor wins.
        assert_eq!(feather_extent(600, 600, 20), 100);
        // ...but it never eats more than a third of the crop.
        assert_eq!(feather_extent(30, 30, 20), 10);
    }

    #[test]
    fn compositing_replaces_the_centre_and_leaves_the_border_alone() {
        let mut base = RgbaImage::from_pixel(200, 200, image::Rgba([0, 0, 0, 255]));
        let patch = RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]));
        let plan = CropPlan {
            x: 50,
            y: 50,
            w: 100,
            h: 100,
            req_w: 1024,
            req_h: 1024,
        };
        composite_face(&mut base, &patch, &plan, 10);

        assert_eq!(base.get_pixel(100, 100).0[0], 255, "centre is the new face");
        assert_eq!(
            base.get_pixel(50, 50).0[0],
            0,
            "the crop's corner is unchanged"
        );
        assert_eq!(
            base.get_pixel(10, 10).0[0],
            0,
            "outside the crop is unchanged"
        );
        let edge = base.get_pixel(53, 100).0[0];
        assert!(edge > 0 && edge < 255, "the seam is blended, got {edge}");
    }

    #[test]
    fn compositing_stays_inside_the_image() {
        let mut base = RgbaImage::from_pixel(120, 120, image::Rgba([0, 0, 0, 255]));
        let patch = RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]));
        let plan = CropPlan {
            x: 80,
            y: 80,
            w: 100,
            h: 100,
            req_w: 1024,
            req_h: 1024,
        };
        // Would run 60 px past the edge; must clip rather than panic.
        composite_face(&mut base, &patch, &plan, 10);
    }
}
