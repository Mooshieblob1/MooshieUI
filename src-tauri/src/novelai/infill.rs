//! NovelAI inpainting (`action: "infill"`) preparation and compositing.
//!
//! NovelAI's web client differs from a raw pixel-space mask request in four
//! ways that materially affect output quality:
//!
//! 1. **8x8 latent mask quantization.** Diffusion happens at 1/8 resolution, so
//!    masks with curved/antialiased pixel edges produce grey partial cells and
//!    fuzzy halos. Quantizing the binary mask onto the 8x8 block grid matches
//!    novelai.net's mask geometry.
//! 2. **Focused Inpainting.** Small masked regions on a large canvas occupy too
//!    few latent cells to render fine details (eyes, clips, mouths). Cropping
//!    with generous context and upscaling to ~1 MP (within Opus's free window)
//!    lets NovelAI paint at full detail before blending back onto the base.
//! 3. **Client-side feathered compositing.** Sending `add_original_image: true`
//!    on V5 can leave a hard latent-grid seam (or be ignored by some routes).
//!    When "Keep the area outside the mask" is enabled, requesting with
//!    `add_original_image: false` and compositing with a dilated + finite-box-
//!    blurred feather mask keeps untouched pixels bit-identical without seams.
//! 4. **Stealth alpha scrubbing.** NovelAI PNGs embed metadata in the LSBs of
//!    the alpha channel (`254..=255`). Normalizing `>= 254` opaque images to
//!    `255` prevents V5's alpha VAE from reading metadata bits as transparency.

use image::imageops::{resize, FilterType};
use image::{DynamicImage, GrayImage, ImageFormat, Luma, Rgb, RgbImage, RgbaImage};

use crate::novelai::params::NovelAiCharacter;

/// Spatial downsampling factor of NovelAI's diffusion latent space.
pub const LATENT_BLOCK: u32 = 8;

/// NovelAI canvas dimension step (must be multiples of 64).
pub const DIMENSION_STEP: u32 = 64;

/// Target pixel budget for Focused Inpainting (matches the 1 MP Opus free window).
pub const FOCUSED_TARGET_PIXELS: u64 = 1_048_576;

/// Default outward dilation in 8x8 latent blocks for the client-side blend mask.
pub const DEFAULT_DILATE_BLOCKS: u32 = 1;

/// Default two-pass box-blur feather radius (in request-canvas pixels).
pub const DEFAULT_FEATHER_PX: u32 = 6;

/// Crop region on the source image and the 64-px-snapped request size sent to NovelAI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusedCropPlan {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub req_w: u32,
    pub req_h: u32,
}

impl FocusedCropPlan {
    /// Plan representing the entire canvas without cropping or rescaling.
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            w: width,
            h: height,
            req_w: width,
            req_h: height,
        }
    }

    pub fn is_full(&self, img_w: u32, img_h: u32) -> bool {
        self.x == 0
            && self.y == 0
            && self.w == img_w
            && self.h == img_h
            && self.req_w == img_w
            && self.req_h == img_h
    }
}

/// Captured inputs needed to blend NovelAI's response back over the original image.
#[derive(Debug, Clone)]
pub struct InfillCompositePlan {
    pub base_rgba: RgbaImage,
    pub crop: FocusedCropPlan,
    /// Binary (`0` or `255`) mask at latent block resolution (`ceil(req_w/8) x ceil(req_h/8)`).
    pub quantized_low_mask: GrayImage,
    pub dilate_blocks: u32,
    pub feather_px: u32,
}

/// Extract a single-channel mask luminance image from a decoded PNG/WebP/JPEG.
///
/// Masks can arrive either as white-on-black RGB/grayscale images (what the
/// canvas exporter writes) or as shapes drawn onto a transparent background
/// (RGBA with `alpha < 128` outside the stroke). Stealth-alpha LSB noise
/// (`254..=255` everywhere) is ignored so white-on-black RGBA masks still read
/// their RGB luminance.
pub fn extract_mask_luma(img: &DynamicImage) -> GrayImage {
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        let has_transparent = rgba.pixels().any(|p| p.0[3] < 128);
        if has_transparent {
            let (w, h) = rgba.dimensions();
            let mut out = GrayImage::new(w, h);
            for (x, y, p) in rgba.enumerate_pixels() {
                out.put_pixel(x, y, Luma([p.0[3]]));
            }
            return out;
        }
    }
    img.to_luma8()
}

/// Normalize NovelAI stealth-metadata alpha LSBs (`254..=255`) to opaque `255`
/// while leaving images with real transparency untouched.
pub fn scrub_stealth_alpha_if_opaque(img: &mut RgbaImage) {
    if img.pixels().all(|p| p.0[3] >= 254) {
        for p in img.pixels_mut() {
            p.0[3] = 255;
        }
    }
}

/// Snap a mask to NovelAI's 8x8 latent grid as strict `{0, 255}` values.
///
/// Returns `(low_mask, full_mask)`:
/// - `low_mask` has dimensions `ceil(target_w / 8) x ceil(target_h / 8)`.
///   Any 8x8 block containing at least one pixel `>= 128` becomes `255`,
///   preserving thin strokes cleanly without shrinking them away.
/// - `full_mask` has dimensions `target_w x target_h`, nearest-expanded from
///   `low_mask`.
pub fn quantize_mask_to_latent_grid(
    mask_luma: &GrayImage,
    target_w: u32,
    target_h: u32,
) -> (GrayImage, GrayImage) {
    let target_w = target_w.max(1);
    let target_h = target_h.max(1);
    let resized_cow;
    let src: &GrayImage = if mask_luma.width() != target_w || mask_luma.height() != target_h {
        resized_cow = resize(mask_luma, target_w, target_h, FilterType::Nearest);
        &resized_cow
    } else {
        mask_luma
    };

    let low_w = target_w.div_ceil(LATENT_BLOCK).max(1);
    let low_h = target_h.div_ceil(LATENT_BLOCK).max(1);
    let mut low = GrayImage::new(low_w, low_h);

    for by in 0..low_h {
        let y0 = by * LATENT_BLOCK;
        let y1 = (y0 + LATENT_BLOCK).min(target_h);
        for bx in 0..low_w {
            let x0 = bx * LATENT_BLOCK;
            let x1 = (x0 + LATENT_BLOCK).min(target_w);
            let mut active = false;
            'block: for y in y0..y1 {
                for x in x0..x1 {
                    if src.get_pixel(x, y).0[0] >= 128 {
                        active = true;
                        break 'block;
                    }
                }
            }
            low.put_pixel(bx, by, Luma([if active { 255 } else { 0 }]));
        }
    }

    let mut full = GrayImage::new(target_w, target_h);
    for y in 0..target_h {
        let by = (y / LATENT_BLOCK).min(low_h - 1);
        for x in 0..target_w {
            let bx = (x / LATENT_BLOCK).min(low_w - 1);
            full.put_pixel(x, y, *low.get_pixel(bx, by));
        }
    }

    (low, full)
}

/// True when the mask selects at least one pixel (`>= 128`).
pub fn mask_has_selection(mask: &GrayImage) -> bool {
    mask.pixels().any(|p| p.0[0] >= 128)
}

/// Encode a binary (`0`/`255`) mask as an opaque RGB PNG.
pub fn encode_binary_mask_png(full_mask: &GrayImage) -> Result<Vec<u8>, String> {
    let (w, h) = full_mask.dimensions();
    let mut rgb = RgbImage::new(w, h);
    for (x, y, p) in full_mask.enumerate_pixels() {
        let v = if p.0[0] >= 128 { 255 } else { 0 };
        rgb.put_pixel(x, y, Rgb([v, v, v]));
    }
    let mut buf = Vec::new();
    rgb.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| format!("could not encode mask PNG: {e}"))?;
    Ok(buf)
}

/// Encode an infill base image as 3-channel RGB PNG when every pixel is opaque,
/// keeping genuine alpha channels as RGBA PNG.
pub fn encode_infill_image_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    if img.pixels().all(|p| p.0[3] == 255) {
        let (w, h) = img.dimensions();
        let rgb = RgbImage::from_fn(w, h, |x, y| {
            let p = img.get_pixel(x, y).0;
            Rgb([p[0], p[1], p[2]])
        });
        rgb.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)
            .map_err(|e| format!("could not encode infill image PNG: {e}"))?;
    } else {
        img.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)
            .map_err(|e| format!("could not encode infill image PNG: {e}"))?;
    }
    Ok(buf)
}

fn snap_down_64(px: u32) -> u32 {
    (px / DIMENSION_STEP).max(1) * DIMENSION_STEP
}

/// Compute a padded crop rectangle around the mask bounding box and a 64-px-snapped
/// request resolution inside the 1 MP Opus free window.
///
/// Returns `None` if the image has zero area or the mask is empty.
pub fn plan_focused_crop(mask_luma: &GrayImage, img_w: u32, img_h: u32) -> Option<FocusedCropPlan> {
    if img_w == 0 || img_h == 0 {
        return None;
    }

    let resized_cow;
    let src: &GrayImage = if mask_luma.width() != img_w || mask_luma.height() != img_h {
        resized_cow = resize(mask_luma, img_w, img_h, FilterType::Nearest);
        &resized_cow
    } else {
        mask_luma
    };

    let mut min_x = img_w;
    let mut min_y = img_h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for (x, y, p) in src.enumerate_pixels() {
        if p.0[0] >= 128 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + 1);
            max_y = max_y.max(y + 1);
        }
    }
    if max_x <= min_x || max_y <= min_y {
        return None;
    }

    let bw = max_x - min_x;
    let bh = max_y - min_y;
    let cx = min_x + bw / 2;
    let cy = min_y + bh / 2;

    let pad_x = bw.max(192);
    let pad_y = bh.max(192);
    let mut desired_w = (bw + 2 * pad_x).max(512).min(img_w);
    let mut desired_h = (bh + 2 * pad_y).max(512).min(img_h);

    // Keep the crop aspect ratio at most 2:1 (when the image allows) so thin
    // brush strokes do not create ultra-skinny requests.
    desired_w = desired_w.max(desired_h / 2).min(img_w);
    desired_h = desired_h.max(desired_w / 2).min(img_h);

    let crop_w = desired_w.clamp(1, img_w);
    let crop_h = desired_h.clamp(1, img_h);
    let crop_x = cx.saturating_sub(crop_w / 2).min(img_w - crop_w);
    let crop_y = cy.saturating_sub(crop_h / 2).min(img_h - crop_h);

    if crop_w * 4 >= img_w * 3
        && crop_h * 4 >= img_h * 3
        && img_w.is_multiple_of(DIMENSION_STEP)
        && img_h.is_multiple_of(DIMENSION_STEP)
        && u64::from(img_w) * u64::from(img_h) <= FOCUSED_TARGET_PIXELS
    {
        return Some(FocusedCropPlan::full(img_w, img_h));
    }

    let crop_area = u64::from(crop_w) * u64::from(crop_h);
    let (req_w, req_h) = if crop_x == 0
        && crop_y == 0
        && crop_w == img_w
        && crop_h == img_h
        && crop_w.is_multiple_of(DIMENSION_STEP)
        && crop_h.is_multiple_of(DIMENSION_STEP)
        && ((FOCUSED_TARGET_PIXELS * 85) / 100..=FOCUSED_TARGET_PIXELS).contains(&crop_area)
    {
        (crop_w, crop_h)
    } else {
        let scale = (FOCUSED_TARGET_PIXELS as f64 / crop_area.max(1) as f64).sqrt();
        let mut rw = snap_down_64((f64::from(crop_w) * scale).round() as u32);
        let mut rh = snap_down_64((f64::from(crop_h) * scale).round() as u32);
        while u64::from(rw) * u64::from(rh) > FOCUSED_TARGET_PIXELS
            && (rw > DIMENSION_STEP || rh > DIMENSION_STEP)
        {
            if rw >= rh && rw > DIMENSION_STEP {
                rw -= DIMENSION_STEP;
            } else if rh > DIMENSION_STEP {
                rh -= DIMENSION_STEP;
            } else {
                break;
            }
        }
        (rw, rh)
    };

    Some(FocusedCropPlan {
        x: crop_x,
        y: crop_y,
        w: crop_w,
        h: crop_h,
        req_w,
        req_h,
    })
}

/// Remap character coordinates from full-image `[0, 1]` space into a focused crop's `[0, 1]` space.
pub fn remap_characters_to_crop(
    characters: &mut [NovelAiCharacter],
    crop: &FocusedCropPlan,
    img_w: u32,
    img_h: u32,
) {
    if img_w == 0 || img_h == 0 || crop.w == 0 || crop.h == 0 {
        return;
    }
    for c in characters {
        c.center.x = ((c.center.x * f64::from(img_w) - f64::from(crop.x)) / f64::from(crop.w))
            .clamp(0.0, 1.0);
        c.center.y = ((c.center.y * f64::from(img_h) - f64::from(crop.y)) / f64::from(crop.h))
            .clamp(0.0, 1.0);
    }
}

/// Dilate a binary (`0`/`255`) mask using a Chebyshev square radius.
fn dilate_binary_mask(mask: &GrayImage, radius: u32) -> GrayImage {
    if radius == 0 {
        return mask.clone();
    }
    let (w, h) = mask.dimensions();
    let r = radius as i32;
    let mut out = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut active = false;
            'neigh: for dy in -r..=r {
                let ny = y as i32 + dy;
                if ny < 0 || ny >= h as i32 {
                    continue;
                }
                for dx in -r..=r {
                    let nx = x as i32 + dx;
                    if nx < 0 || nx >= w as i32 {
                        continue;
                    }
                    if mask.get_pixel(nx as u32, ny as u32).0[0] >= 128 {
                        active = true;
                        break 'neigh;
                    }
                }
            }
            out.put_pixel(x, y, Luma([if active { 255 } else { 0 }]));
        }
    }
    out
}

/// Finite separable box blur over a `[0, 1]` float grid, preserving exact zeros
/// outside `(radius)` distance from non-zero cells.
fn box_blur_f32(input: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    if radius == 0 || width == 0 || height == 0 {
        return input.to_vec();
    }
    let window = (2 * radius + 1) as f32;
    let mut horiz = vec![0.0_f32; input.len()];

    for y in 0..height {
        let row = &input[y * width..(y + 1) * width];
        let mut sum = 0.0_f32;
        for d in -(radius as isize)..=(radius as isize) {
            let idx = d.clamp(0, width as isize - 1) as usize;
            sum += row[idx];
        }
        for x in 0..width {
            horiz[y * width + x] = sum / window;
            let add_idx = (x + radius + 1).min(width - 1);
            let sub_idx = x.saturating_sub(radius);
            sum += row[add_idx] - row[sub_idx];
        }
    }

    let mut out = vec![0.0_f32; input.len()];
    for x in 0..width {
        let mut sum = 0.0_f32;
        for d in -(radius as isize)..=(radius as isize) {
            let idx = d.clamp(0, height as isize - 1) as usize;
            sum += horiz[idx * width + x];
        }
        for y in 0..height {
            out[y * width + x] = (sum / window).clamp(0.0, 1.0);
            let add_idx = (y + radius + 1).min(height - 1);
            let sub_idx = y.saturating_sub(radius);
            sum += horiz[add_idx * width + x] - horiz[sub_idx * width + x];
        }
    }
    out
}

fn bilinear_resize_f32(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<f32> {
    if src_w == dst_w && src_h == dst_h {
        return src.to_vec();
    }
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return vec![0.0; dst_w * dst_h];
    }
    let mut out = vec![0.0_f32; dst_w * dst_h];
    let scale_x = src_w as f32 / dst_w as f32;
    let scale_y = src_h as f32 / dst_h as f32;

    for y in 0..dst_h {
        let sy = ((y as f32 + 0.5) * scale_y - 0.5).clamp(0.0, (src_h - 1) as f32);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(src_h - 1);
        let fy = sy - y0 as f32;

        for x in 0..dst_w {
            let sx = ((x as f32 + 0.5) * scale_x - 0.5).clamp(0.0, (src_w - 1) as f32);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(src_w - 1);
            let fx = sx - x0 as f32;

            let v00 = src[y0 * src_w + x0];
            let v10 = src[y0 * src_w + x1];
            let v01 = src[y1 * src_w + x0];
            let v11 = src[y1 * src_w + x1];

            let top = v00 * (1.0 - fx) + v10 * fx;
            let bot = v01 * (1.0 - fx) + v11 * fx;
            out[y * dst_w + x] = (top * (1.0 - fy) + bot * fy).clamp(0.0, 1.0);
        }
    }
    out
}

/// Build `[0.0, 1.0]` composite weights in crop pixel coordinates `(crop.w x crop.h)`.
///
/// Any side of `crop` that sits inside `base` (does not touch the outer image
/// border) receives a raised-cosine taper to `0.0` so a dilated/feathered mask
/// never creates a rectangular seam along a crop boundary.
pub fn build_feather_weights(
    low_mask: &GrayImage,
    req_size: (u32, u32),
    dilate_blocks: u32,
    feather_px: u32,
    crop: &FocusedCropPlan,
    base_size: (u32, u32),
) -> Vec<f32> {
    let req_w = req_size.0.max(1);
    let req_h = req_size.1.max(1);
    let (base_w, base_h) = base_size;
    let dilated = dilate_binary_mask(low_mask, dilate_blocks);
    let (low_w, low_h) = dilated.dimensions();

    let mut req_weights = vec![0.0_f32; (req_w as usize) * (req_h as usize)];
    for y in 0..req_h {
        let by = (y / LATENT_BLOCK).min(low_h.saturating_sub(1));
        for x in 0..req_w {
            let bx = (x / LATENT_BLOCK).min(low_w.saturating_sub(1));
            if dilated.get_pixel(bx, by).0[0] >= 128 {
                req_weights[(y as usize) * (req_w as usize) + (x as usize)] = 1.0;
            }
        }
    }

    let blurred = if feather_px > 0 {
        let pass1 = box_blur_f32(
            &req_weights,
            req_w as usize,
            req_h as usize,
            feather_px as usize,
        );
        box_blur_f32(&pass1, req_w as usize, req_h as usize, feather_px as usize)
    } else {
        req_weights
    };

    let mut weights = bilinear_resize_f32(
        &blurred,
        req_w as usize,
        req_h as usize,
        crop.w as usize,
        crop.h as usize,
    );

    // Guarantee zero weight right at any interior crop border.
    let cw = crop.w as usize;
    let ch = crop.h as usize;
    if cw > 0 && ch > 0 {
        let ramp_x = 24usize.min((cw / 6).max(1));
        let ramp_y = 24usize.min((ch / 6).max(1));
        let clamp_left = crop.x > 0;
        let clamp_right = crop.x.saturating_add(crop.w) < base_w;
        let clamp_top = crop.y > 0;
        let clamp_bottom = crop.y.saturating_add(crop.h) < base_h;

        let cosine_ramp = |dist: usize, ramp: usize| -> f32 {
            if dist >= ramp {
                1.0
            } else if ramp <= 1 {
                0.0
            } else {
                let t = std::f32::consts::PI * dist as f32 / (ramp - 1) as f32;
                0.5 * (1.0 - t.cos())
            }
        };

        for y in 0..ch {
            let mut ry = 1.0_f32;
            if clamp_top {
                ry = ry.min(cosine_ramp(y, ramp_y));
            }
            if clamp_bottom {
                ry = ry.min(cosine_ramp(ch - 1 - y, ramp_y));
            }
            for x in 0..cw {
                let mut rx = 1.0_f32;
                if clamp_left {
                    rx = rx.min(cosine_ramp(x, ramp_x));
                }
                if clamp_right {
                    rx = rx.min(cosine_ramp(cw - 1 - x, ramp_x));
                }
                let idx = y * cw + x;
                weights[idx] *= rx * ry;
            }
        }
    }

    weights
}

/// Composite `generated` (at `crop.req_w x crop.req_h`) onto `base` using the
/// feathered latent mask weights, leaving zero-weight pixels bit-identical.
pub fn composite_infill_patch(
    base: &RgbaImage,
    generated: &RgbaImage,
    crop: &FocusedCropPlan,
    low_mask: &GrayImage,
    dilate_blocks: u32,
    feather_px: u32,
) -> RgbaImage {
    let mut out = base.clone();
    if crop.w == 0 || crop.h == 0 || base.width() == 0 || base.height() == 0 {
        return out;
    }

    let (gen_w, gen_h) = generated.dimensions();
    let patch_cow;
    let patch: &RgbaImage = if gen_w != crop.w || gen_h != crop.h {
        patch_cow = resize(generated, crop.w, crop.h, FilterType::Lanczos3);
        &patch_cow
    } else {
        generated
    };

    let weights = build_feather_weights(
        low_mask,
        (gen_w, gen_h),
        dilate_blocks,
        feather_px,
        crop,
        (base.width(), base.height()),
    );

    let max_w = crop.w.min(base.width().saturating_sub(crop.x));
    let max_h = crop.h.min(base.height().saturating_sub(crop.y));
    let cw = crop.w as usize;
    let base_opaque = base.pixels().all(|p| p.0[3] == 255);

    for y in 0..max_h {
        for x in 0..max_w {
            let alpha = weights[(y as usize) * cw + (x as usize)];
            if alpha <= 0.0 {
                continue;
            }
            let src = patch.get_pixel(x, y).0;
            let dst = out.get_pixel_mut(crop.x + x, crop.y + y);
            if alpha >= 1.0 - 1e-6 {
                dst.0 = if base_opaque {
                    [src[0], src[1], src[2], 255]
                } else {
                    src
                };
            } else {
                let channels = if base_opaque { 3 } else { 4 };
                for (dst_c, src_c) in dst.0[..channels].iter_mut().zip(&src[..channels]) {
                    let blended = f32::from(*src_c) * alpha + f32::from(*dst_c) * (1.0 - alpha);
                    *dst_c = blended.round().clamp(0.0, 255.0) as u8;
                }
                if base_opaque {
                    dst.0[3] = 255;
                }
            }
        }
    }

    out
}

/// Composite a returned NovelAI PNG byte stream onto `plan.base_rgba`,
/// preserving the returned NovelAI PNG text metadata chunks.
pub fn apply_composite_to_png(
    plan: &InfillCompositePlan,
    returned_png: &[u8],
) -> Result<Vec<u8>, String> {
    let generated = image::load_from_memory(returned_png)
        .map_err(|e| format!("could not decode NovelAI infill result: {e}"))?
        .to_rgba8();
    let composited = composite_infill_patch(
        &plan.base_rgba,
        &generated,
        &plan.crop,
        &plan.quantized_low_mask,
        plan.dilate_blocks,
        plan.feather_px,
    );
    let mut buf = Vec::new();
    composited
        .write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| format!("could not encode composited PNG: {e}"))?;
    Ok(crate::metadata::copy_png_text_chunks(returned_png, &buf).unwrap_or(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::params::NovelAiCoord;

    #[test]
    fn extract_mask_luma_handles_rgb_transparent_alpha_and_stealth_lsb() {
        // White-on-black RGBA with opaque alpha everywhere reads RGB luma.
        let mut opaque = RgbaImage::from_pixel(16, 16, image::Rgba([0, 0, 0, 255]));
        opaque.put_pixel(4, 4, image::Rgba([255, 255, 255, 254]));
        let luma = extract_mask_luma(&DynamicImage::ImageRgba8(opaque));
        assert_eq!(luma.get_pixel(0, 0).0[0], 0);
        assert_eq!(luma.get_pixel(4, 4).0[0], 255);

        // Stroke on transparent background (`alpha = 0` outside) reads alpha.
        let mut transparent = RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 0]));
        transparent.put_pixel(8, 8, image::Rgba([255, 0, 0, 255]));
        let luma = extract_mask_luma(&DynamicImage::ImageRgba8(transparent));
        assert_eq!(luma.get_pixel(0, 0).0[0], 0);
        assert_eq!(luma.get_pixel(8, 8).0[0], 255);
    }

    #[test]
    fn scrub_stealth_alpha_only_touches_opaque_lsb_noise() {
        let mut stealth = RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 254]));
        stealth.put_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        scrub_stealth_alpha_if_opaque(&mut stealth);
        assert!(stealth.pixels().all(|p| p.0[3] == 255));

        let mut transparent = RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 254]));
        transparent.put_pixel(0, 0, image::Rgba([10, 20, 30, 0]));
        scrub_stealth_alpha_if_opaque(&mut transparent);
        assert_eq!(transparent.get_pixel(0, 0).0[3], 0);
        assert_eq!(transparent.get_pixel(1, 1).0[3], 254);
    }

    #[test]
    fn quantize_mask_snaps_to_8x8_blocks_and_keeps_thin_strokes() {
        let mut mask = GrayImage::new(64, 64);
        // Single 1px pixel inside block (2, 3) -> x=19, y=27.
        mask.put_pixel(19, 27, Luma([200]));
        let (low, full) = quantize_mask_to_latent_grid(&mask, 64, 64);
        assert_eq!(low.dimensions(), (8, 8));
        assert_eq!(low.get_pixel(2, 3).0[0], 255);
        assert_eq!(low.get_pixel(0, 0).0[0], 0);

        // Every pixel inside block (2, 3) is 255, and outside is 0.
        for y in 0..64 {
            for x in 0..64 {
                let expected = if (16..24).contains(&x) && (24..32).contains(&y) {
                    255
                } else {
                    0
                };
                assert_eq!(full.get_pixel(x, y).0[0], expected, "at {x},{y}");
            }
        }
    }

    #[test]
    fn plan_focused_crop_zooms_small_mask_to_1mp_and_keeps_full_standard_canvas() {
        let mut small_mask = GrayImage::new(1024, 1024);
        for y in 450..550 {
            for x in 650..750 {
                small_mask.put_pixel(x, y, Luma([255]));
            }
        }
        let crop = plan_focused_crop(&small_mask, 1024, 1024).expect("non-empty mask");
        assert!(crop.w >= 512 && crop.h >= 512);
        assert!(crop.w < 1024 && crop.h < 1024, "should zoom in: {crop:?}");
        assert!(crop.x <= 650 && crop.x + crop.w >= 750);
        assert!(crop.y <= 450 && crop.y + crop.h >= 550);
        assert_eq!(crop.req_w % 64, 0);
        assert_eq!(crop.req_h % 64, 0);
        assert!(u64::from(crop.req_w) * u64::from(crop.req_h) <= FOCUSED_TARGET_PIXELS);

        // A 300x300 mask whose 3x context >= 75% falls back to full 1024x1024 canvas.
        let mut wide_mask = GrayImage::new(1024, 1024);
        for y in 362..662 {
            for x in 362..662 {
                wide_mask.put_pixel(x, y, Luma([255]));
            }
        }
        let wide_crop = plan_focused_crop(&wide_mask, 1024, 1024).expect("wide mask");
        assert!(wide_crop.is_full(1024, 1024));

        // Full-canvas mask on 1024x1024 stays 1024x1024 without rescaling.
        let full_mask = GrayImage::from_pixel(1024, 1024, Luma([255]));
        let full_crop = plan_focused_crop(&full_mask, 1024, 1024).expect("full mask");
        assert!(full_crop.is_full(1024, 1024));

        // Empty mask returns None.
        let empty = GrayImage::new(1024, 1024);
        assert!(plan_focused_crop(&empty, 1024, 1024).is_none());
    }

    #[test]
    fn encode_infill_image_png_writes_rgb_for_opaque_and_rgba_for_transparent() {
        let opaque = RgbaImage::from_pixel(8, 8, image::Rgba([40, 80, 120, 255]));
        let opaque_bytes = encode_infill_image_png(&opaque).unwrap();
        let opaque_dec = png::Decoder::new(std::io::Cursor::new(&opaque_bytes))
            .read_info()
            .unwrap();
        assert_eq!(opaque_dec.info().color_type, png::ColorType::Rgb);

        let mut transparent = opaque;
        transparent.put_pixel(0, 0, image::Rgba([40, 80, 120, 128]));
        let trans_bytes = encode_infill_image_png(&transparent).unwrap();
        let trans_dec = png::Decoder::new(std::io::Cursor::new(&trans_bytes))
            .read_info()
            .unwrap();
        assert_eq!(trans_dec.info().color_type, png::ColorType::Rgba);
    }

    #[test]
    fn composite_infill_patch_keeps_opaque_base_alpha_255() {
        let base = RgbaImage::from_pixel(64, 64, image::Rgba([10, 20, 30, 255]));
        let generated = RgbaImage::from_pixel(64, 64, image::Rgba([200, 150, 100, 240]));
        let low = GrayImage::from_pixel(8, 8, Luma([255]));
        let out = composite_infill_patch(
            &base,
            &generated,
            &FocusedCropPlan::full(64, 64),
            &low,
            1,
            6,
        );
        assert!(out.pixels().all(|p| p.0[3] == 255));
        assert_eq!(out.get_pixel(32, 32).0, [200, 150, 100, 255]);
    }

    #[test]
    fn remap_characters_to_crop_maps_and_clamps() {
        let crop = FocusedCropPlan {
            x: 200,
            y: 100,
            w: 400,
            h: 400,
            req_w: 1024,
            req_h: 1024,
        };
        let mut chars = vec![
            NovelAiCharacter {
                prompt: "inside".into(),
                negative_prompt: String::new(),
                center: NovelAiCoord { x: 0.4, y: 0.3 }, // (400, 300) -> center (0.5, 0.5)
                enabled: true,
            },
            NovelAiCharacter {
                prompt: "outside".into(),
                negative_prompt: String::new(),
                center: NovelAiCoord { x: 0.0, y: 0.9 },
                enabled: true,
            },
        ];
        remap_characters_to_crop(&mut chars, &crop, 1000, 1000);
        assert!((chars[0].center.x - 0.5).abs() < 1e-6);
        assert!((chars[0].center.y - 0.5).abs() < 1e-6);
        assert_eq!(chars[1].center.x, 0.0);
        assert_eq!(chars[1].center.y, 1.0);
    }

    #[test]
    fn composite_preserves_outside_pixels_and_zeroes_interior_crop_edges() {
        let base = RgbaImage::from_pixel(512, 512, image::Rgba([20, 40, 60, 255]));
        let generated = RgbaImage::from_pixel(512, 512, image::Rgba([220, 180, 100, 255]));
        let crop = FocusedCropPlan {
            x: 128,
            y: 128,
            w: 256,
            h: 256,
            req_w: 512,
            req_h: 512,
        };
        let mut low = GrayImage::new(64, 64);
        for by in 24..40 {
            for bx in 24..40 {
                low.put_pixel(bx, by, Luma([255]));
            }
        }
        let out = composite_infill_patch(&base, &generated, &crop, &low, 2, 12);

        // Outside crop is untouched.
        assert_eq!(out.get_pixel(10, 10).0, [20, 40, 60, 255]);
        // Interior crop border itself is zero weight.
        assert_eq!(out.get_pixel(128, 128).0, [20, 40, 60, 255]);
        assert_eq!(out.get_pixel(383, 383).0, [20, 40, 60, 255]);
        // Center of mask gets full generated pixels.
        assert_eq!(out.get_pixel(256, 256).0, [220, 180, 100, 255]);
    }

    #[test]
    fn apply_composite_to_png_preserves_novelai_text_chunks() {
        let base = RgbaImage::from_pixel(128, 128, image::Rgba([10, 20, 30, 255]));
        let mut returned_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut returned_bytes, 128, 128);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            writer
                .write_text_chunk(&png::text_metadata::TEXtChunk {
                    keyword: "Software".into(),
                    text: "NovelAI".into(),
                })
                .unwrap();
            let pixels = vec![200u8; 128 * 128 * 4];
            writer.write_image_data(&pixels).unwrap();
        }
        let low = GrayImage::from_pixel(16, 16, Luma([255]));
        let plan = InfillCompositePlan {
            base_rgba: base,
            crop: FocusedCropPlan::full(128, 128),
            quantized_low_mask: low,
            dilate_blocks: 0,
            feather_px: 0,
        };
        let out = apply_composite_to_png(&plan, &returned_bytes).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(&out));
        let reader = decoder.read_info().unwrap();
        assert!(reader
            .info()
            .uncompressed_latin1_text
            .iter()
            .any(|t| t.keyword == "Software" && t.text == "NovelAI"));
    }
}
