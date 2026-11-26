//! Preparing a Precise Reference image onto a canvas NovelAI's encoder accepts.
//!
//! V4.5's character reference (`director_reference_*`) encoder takes exactly
//! three canvas sizes (`1024x1536`, `1536x1024`, `1472x1472`) and answers
//! anything else with a bare 400. V5 replaces that with a 97-entry aspect-ratio
//! bucket table (`256x4096` .. `4096x256`), using scale-to-cover + centre-crop,
//! preserving alpha and stripping NovelAI's stealth-PNG alpha LSB marker.
//!
//! This runs backend-side rather than in the picker so that references already
//! persisted in a user's settings, and those arriving through the NovelAI
//! import modal, are fixed on the way out instead of only newly-picked files.
//!
//! Vibe transfer does *not* come through here. Those images are uploaded to
//! `/ai/encode-vibe`, which accepts arbitrary sizes and returns a token.

use base64::Engine as _;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
use std::io::Cursor;

use super::models::ReferenceCanvasMode;

/// The only canvas sizes NovelAI's V4.5 character reference encoder accepts.
///
/// Taken from NovelAI's own client-side preprocessing, which picks whichever
/// of the three is closest in aspect ratio to the source image.
pub const ACCEPTED_CANVASES: [(u32, u32); 3] = [(1024, 1536), (1536, 1024), (1472, 1472)];

/// The 97 aspect-ratio buckets NovelAI's V5 Precise Reference encoder accepts.
pub const V5_REFERENCE_BUCKETS: [(u32, u32); 97] = [
    (256, 4096),
    (256, 4032),
    (256, 3968),
    (256, 3904),
    (256, 3840),
    (256, 3776),
    (256, 3712),
    (256, 3648),
    (256, 3584),
    (256, 3520),
    (256, 3456),
    (256, 3392),
    (256, 3328),
    (320, 3264),
    (320, 3200),
    (320, 3136),
    (320, 3072),
    (320, 3008),
    (320, 2944),
    (320, 2880),
    (320, 2816),
    (320, 2752),
    (384, 2688),
    (384, 2624),
    (384, 2560),
    (384, 2496),
    (384, 2432),
    (384, 2368),
    (448, 2304),
    (448, 2240),
    (448, 2176),
    (448, 2112),
    (512, 2048),
    (512, 1984),
    (512, 1920),
    (512, 1856),
    (576, 1792),
    (576, 1728),
    (576, 1664),
    (640, 1600),
    (640, 1536),
    (704, 1472),
    (704, 1408),
    (768, 1344),
    (768, 1280),
    (832, 1216),
    (896, 1152),
    (960, 1088),
    (1024, 1024),
    (1088, 960),
    (1152, 896),
    (1216, 832),
    (1280, 768),
    (1344, 768),
    (1408, 704),
    (1472, 704),
    (1536, 640),
    (1600, 640),
    (1664, 576),
    (1728, 576),
    (1792, 576),
    (1856, 512),
    (1920, 512),
    (1984, 512),
    (2048, 512),
    (2112, 448),
    (2176, 448),
    (2240, 448),
    (2304, 448),
    (2368, 384),
    (2432, 384),
    (2496, 384),
    (2560, 384),
    (2624, 384),
    (2688, 384),
    (2752, 320),
    (2816, 320),
    (2880, 320),
    (2944, 320),
    (3008, 320),
    (3072, 320),
    (3136, 320),
    (3200, 320),
    (3264, 320),
    (3328, 256),
    (3392, 256),
    (3456, 256),
    (3520, 256),
    (3584, 256),
    (3648, 256),
    (3712, 256),
    (3776, 256),
    (3840, 256),
    (3904, 256),
    (3968, 256),
    (4032, 256),
    (4096, 256),
];

/// Max pixel count NovelAI inspects for stealth-PNG alpha markers (`16 Mi px`).
const STEALTH_MAX_PIXELS: usize = 0x100_0000;
const STEALTH_MAGIC: &[u8] = b"stealth_pngcomp";

fn strip_data_url(s: &str) -> &str {
    s.split_once("base64,").map_or(s, |(_, rest)| rest)
}

/// Pick the accepted V4.5 canvas whose aspect ratio is closest to `width`x`height`.
pub fn choose_canvas(width: u32, height: u32) -> (u32, u32) {
    let aspect = width as f64 / height as f64;
    let mut best = ACCEPTED_CANVASES[0];
    let mut best_diff = f64::INFINITY;
    for (cw, ch) in ACCEPTED_CANVASES {
        let diff = ((cw as f64 / ch as f64) - aspect).abs();
        if diff < best_diff {
            best_diff = diff;
            best = (cw, ch);
        }
    }
    best
}

/// Pick the V5 bucket closest in log-aspect space to `width`x`height`.
pub fn choose_v5_bucket(width: u32, height: u32) -> (u32, u32) {
    let target = (width.max(1) as f64 / height.max(1) as f64).ln();
    let mut best = V5_REFERENCE_BUCKETS[0];
    let mut best_diff = f64::INFINITY;
    for (bw, bh) in V5_REFERENCE_BUCKETS {
        let diff = (target - (bw as f64 / bh as f64).ln()).abs();
        if diff < best_diff {
            best_diff = diff;
            best = (bw, bh);
        }
    }
    best
}

/// Detect NovelAI's column-major stealth-PNG alpha LSB header and scrub the
/// 254/1 bit flips back to 255/0 without disturbing real `0` transparency.
fn strip_stealth_alpha(rgba: &mut RgbaImage) -> bool {
    let w = rgba.width() as usize;
    let h = rgba.height() as usize;
    let total = w.saturating_mul(h);
    if total > STEALTH_MAX_PIXELS || total < STEALTH_MAGIC.len() * 8 {
        return false;
    }
    for (byte_idx, &expected) in STEALTH_MAGIC.iter().enumerate() {
        let mut byte = 0u8;
        for bit in 0..8 {
            let l = byte_idx * 8 + bit;
            let y = (l % h) as u32;
            let x = (l / h) as u32;
            let alpha = rgba.get_pixel(x, y)[3];
            byte |= (alpha & 1) << (7 - bit);
        }
        if byte != expected {
            return false;
        }
    }
    for pixel in rgba.pixels_mut() {
        if pixel[3] == 254 {
            pixel[3] = 255;
        } else if pixel[3] == 1 {
            pixel[3] = 0;
        }
    }
    true
}

/// Prepare a bare-base64 Precise Reference image according to the model's canvas mode.
pub fn prepare_reference(base64_png: &str, mode: ReferenceCanvasMode) -> String {
    match mode {
        ReferenceCanvasMode::Letterbox => letterbox_reference(base64_png),
        ReferenceCanvasMode::Bucket => bucket_reference(base64_png),
    }
}

/// Letterbox a bare-base64 image onto the closest accepted V4.5 canvas.
///
/// Returns the re-encoded base64 PNG. An image that already sits on an accepted
/// canvas is returned untouched, so a reference that made the round trip once
/// is not re-encoded on every generation.
///
/// Anything that fails to decode is passed through unchanged rather than
/// dropped: the request then fails the same way it did before, which is a
/// clearer outcome than a generation that silently ignores the reference the
/// user attached.
pub fn letterbox_reference(base64_png: &str) -> String {
    let raw = strip_data_url(base64_png).trim();
    match letterbox_inner(raw) {
        Some(fixed) => fixed,
        None => base64_png.to_string(),
    }
}

fn letterbox_inner(base64_png: &str) -> Option<String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let bytes = engine.decode(base64_png).ok()?;
    let source = image::load_from_memory(&bytes).ok()?;
    let (w, h) = (source.width(), source.height());
    if w == 0 || h == 0 {
        return None;
    }
    let (cw, ch) = choose_canvas(w, h);
    if (w, h) == (cw, ch) {
        return None;
    }

    // Fit inside the canvas, then centre. Black rather than transparent: the
    // encoder reads an alpha channel as content, so padding has to be opaque.
    let scale = (cw as f64 / w as f64).min(ch as f64 / h as f64);
    let new_w = ((w as f64 * scale).round() as u32).clamp(1, cw);
    let new_h = ((h as f64 * scale).round() as u32).clamp(1, ch);
    let resized = source
        .resize_exact(new_w, new_h, FilterType::Lanczos3)
        .to_rgb8();

    let mut canvas = RgbImage::new(cw, ch);
    image::imageops::overlay(
        &mut canvas,
        &resized,
        ((cw - new_w) / 2) as i64,
        ((ch - new_h) / 2) as i64,
    );

    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(canvas)
        .write_to(&mut out, ImageFormat::Png)
        .ok()?;
    Some(engine.encode(out.into_inner()))
}

/// Scale-to-cover and centre-crop a reference image onto the closest V5 bucket,
/// preserving transparency and scrubbing stealth-PNG alpha noise if present.
pub fn bucket_reference(base64_png: &str) -> String {
    let raw = strip_data_url(base64_png).trim();
    match bucket_inner(raw) {
        Some(Some(fixed)) => fixed,
        Some(None) => raw.to_string(),
        None => base64_png.to_string(),
    }
}

/// Returns `None` on decode failure, `Some(None)` when already matching and clean,
/// or `Some(Some(encoded))` when resized/cropped or stealth-cleaned.
fn bucket_inner(base64_png: &str) -> Option<Option<String>> {
    let engine = base64::engine::general_purpose::STANDARD;
    let bytes = engine.decode(base64_png).ok()?;
    let source = image::load_from_memory(&bytes).ok()?;
    let (w, h) = (source.width(), source.height());
    if w == 0 || h == 0 {
        return None;
    }

    let has_alpha = source.color().has_alpha();
    let (working, stealth_cleaned) = if has_alpha {
        let mut rgba = source.to_rgba8();
        let cleaned = strip_stealth_alpha(&mut rgba);
        (DynamicImage::ImageRgba8(rgba), cleaned)
    } else {
        (source, false)
    };

    let (bw, bh) = choose_v5_bucket(w, h);
    if (w, h) == (bw, bh) && !stealth_cleaned {
        return Some(None);
    }

    let final_img = if (w, h) == (bw, bh) {
        working
    } else {
        let scale = (bw as f64 / w as f64).max(bh as f64 / h as f64);
        let resize_w = ((w as f64 * scale + 0.5).floor() as u32).max(bw);
        let resize_h = ((h as f64 * scale + 0.5).floor() as u32).max(bh);
        let left = (resize_w - bw) / 2;
        let top = (resize_h - bh) / 2;
        let resized = if (resize_w, resize_h) == (w, h) {
            working
        } else {
            working.resize_exact(resize_w, resize_h, FilterType::Lanczos3)
        };
        if left == 0 && top == 0 && resized.width() == bw && resized.height() == bh {
            resized
        } else {
            resized.crop_imm(left, top, bw, bh)
        }
    };

    let encoded_img = if has_alpha {
        DynamicImage::ImageRgba8(final_img.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(final_img.to_rgb8())
    };
    let mut out = Cursor::new(Vec::new());
    encoded_img.write_to(&mut out, ImageFormat::Png).ok()?;
    Some(Some(engine.encode(out.into_inner())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_base64(width: u32, height: u32) -> String {
        let img = RgbImage::from_pixel(width, height, image::Rgb([200, 40, 90]));
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    }

    fn dimensions_of(base64_png: &str) -> (u32, u32) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64_png)
            .unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn portrait_lands_on_the_portrait_canvas() {
        assert_eq!(choose_canvas(683, 1024), (1024, 1536));
    }

    #[test]
    fn landscape_lands_on_the_landscape_canvas() {
        assert_eq!(choose_canvas(1024, 683), (1536, 1024));
    }

    #[test]
    fn square_lands_on_the_square_canvas() {
        assert_eq!(choose_canvas(900, 900), (1472, 1472));
    }

    #[test]
    fn an_arbitrary_upload_is_resized_to_an_accepted_canvas() {
        // The shape the picker used to produce: longest side 1024, source ratio
        // kept. Every one of these earned a 400 from the encoder.
        let out = letterbox_reference(&png_base64(1024, 683));
        assert_eq!(dimensions_of(&out), (1536, 1024));
    }

    #[test]
    fn a_tall_upload_is_padded_rather_than_stretched() {
        // 512x1536 fits the portrait canvas by height, so it is centred with
        // black bars either side instead of being squashed to 1024 wide.
        let out = letterbox_reference(&png_base64(512, 1536));
        assert_eq!(dimensions_of(&out), (1024, 1536));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&out)
            .unwrap();
        let img = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(img.get_pixel(2, 768), &image::Rgb([0, 0, 0]));
        assert_eq!(img.get_pixel(512, 768), &image::Rgb([200, 40, 90]));
    }

    #[test]
    fn an_image_already_on_an_accepted_canvas_is_left_alone() {
        let already = png_base64(1472, 1472);
        assert_eq!(letterbox_reference(&already), already);
    }

    #[test]
    fn undecodable_input_is_passed_through_untouched() {
        assert_eq!(letterbox_reference("not-an-image"), "not-an-image");
        assert_eq!(letterbox_reference(""), "");
        assert_eq!(bucket_reference("not-an-image"), "not-an-image");
        assert_eq!(bucket_reference(""), "");
    }

    fn rgba_png_base64(img: RgbaImage) -> String {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    }

    fn decode_rgba(base64_png: &str) -> RgbaImage {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64_png)
            .unwrap();
        image::load_from_memory(&bytes).unwrap().to_rgba8()
    }

    #[test]
    fn v5_bucket_selection_covers_standard_and_extreme_ratios() {
        assert_eq!(choose_v5_bucket(832, 1216), (832, 1216));
        assert_eq!(choose_v5_bucket(1216, 832), (1216, 832));
        assert_eq!(choose_v5_bucket(1024, 1024), (1024, 1024));
        assert_eq!(choose_v5_bucket(900, 900), (1024, 1024));
        assert_eq!(choose_v5_bucket(100, 3000), (256, 4096));
        assert_eq!(choose_v5_bucket(3000, 100), (4096, 256));
    }

    #[test]
    fn v5_bucket_cover_crops_without_black_bars() {
        // 512x1536 is taller than 2:3; closest V5 bucket is 384x1152? Check actual closest bucket
        // and verify border pixels match source color instead of black padding.
        let (bw, bh) = choose_v5_bucket(683, 1024);
        let out = bucket_reference(&png_base64(683, 1024));
        let img = decode_rgba(&out);
        assert_eq!((img.width(), img.height()), (bw, bh));
        assert_eq!(img.get_pixel(0, 0), &image::Rgba([200, 40, 90, 255]));
        assert_eq!(
            img.get_pixel(bw - 1, bh - 1),
            &image::Rgba([200, 40, 90, 255])
        );
    }

    #[test]
    fn v5_bucket_returns_matching_clean_image_untouched() {
        let already = png_base64(832, 1216);
        assert_eq!(bucket_reference(&already), already);
    }

    #[test]
    fn v5_bucket_preserves_alpha_transparency() {
        let mut src = RgbaImage::from_pixel(900, 900, image::Rgba([10, 20, 30, 0]));
        for y in 300..600 {
            for x in 300..600 {
                src.put_pixel(x, y, image::Rgba([200, 50, 60, 255]));
            }
        }
        let out = bucket_reference(&rgba_png_base64(src));
        let img = decode_rgba(&out);
        assert_eq!((img.width(), img.height()), (1024, 1024));
        assert_eq!(img.get_pixel(10, 10)[3], 0);
        assert_eq!(img.get_pixel(512, 512)[3], 255);
    }

    #[test]
    fn v5_bucket_cleans_stealth_png_alpha_lsbs_and_keeps_transparency() {
        let (w, h) = (832u32, 1216u32);
        let mut src = RgbaImage::from_pixel(w, h, image::Rgba([40, 50, 60, 255]));
        // Set one transparent pixel that should remain alpha=0, and one pixel at alpha=1.
        src.put_pixel(w - 1, h - 1, image::Rgba([40, 50, 60, 0]));
        src.put_pixel(w - 2, h - 1, image::Rgba([40, 50, 60, 1]));
        // Encode "stealth_pngcomp" header into column-major alpha LSBs (254 for 0, 255 for 1).
        for (byte_idx, &byte) in STEALTH_MAGIC.iter().enumerate() {
            for bit in 0..8 {
                let l = byte_idx * 8 + bit;
                let y = (l % h as usize) as u32;
                let x = (l / h as usize) as u32;
                let bit_val = (byte >> (7 - bit)) & 1;
                let alpha = if bit_val == 1 { 255 } else { 254 };
                src.put_pixel(x, y, image::Rgba([40, 50, 60, alpha]));
            }
        }
        let encoded = rgba_png_base64(src);
        let out = bucket_reference(&encoded);
        assert_ne!(out, encoded);
        let img = decode_rgba(&out);
        assert_eq!((img.width(), img.height()), (w, h));
        // Every header pixel that was 254 must be restored to 255.
        assert_eq!(img.get_pixel(0, 0)[3], 255);
        assert_eq!(img.get_pixel(0, 1)[3], 255);
        // Real alpha=0 stays 0; stealth-encoded alpha=1 flips to 0.
        assert_eq!(img.get_pixel(w - 1, h - 1)[3], 0);
        assert_eq!(img.get_pixel(w - 2, h - 1)[3], 0);
    }

    #[test]
    fn v5_bucket_leaves_ordinary_semi_transparent_images_alone() {
        let mut src = RgbaImage::from_pixel(832, 1216, image::Rgba([40, 50, 60, 254]));
        src.put_pixel(10, 10, image::Rgba([40, 50, 60, 1]));
        let encoded = rgba_png_base64(src);
        assert_eq!(bucket_reference(&encoded), encoded);
    }
}
