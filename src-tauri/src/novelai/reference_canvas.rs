//! Letterboxing a character reference onto a canvas NovelAI's encoder accepts.
//!
//! NovelAI's character reference (`director_reference_*`) encoder is not a
//! general image endpoint: it takes exactly three canvas sizes and answers
//! anything else with a bare 400, surfaced to the user as "Error encoding v4
//! director references: non-200 response: 400". Their own web client never
//! sends a raw upload, it letterboxes the picked file onto the closest of the
//! three first, and every working third-party client mirrors that step.
//!
//! This runs backend-side rather than in the picker so that references already
//! persisted in a user's settings, and those arriving through the NovelAI
//! import modal, are fixed on the way out instead of only newly-picked files.
//!
//! Vibe transfer does *not* come through here. Those images are uploaded to
//! `/ai/encode-vibe`, which accepts arbitrary sizes and returns a token.

use base64::Engine as _;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbImage};
use std::io::Cursor;

/// The only canvas sizes NovelAI's character reference encoder accepts.
///
/// Taken from NovelAI's own client-side preprocessing, which picks whichever
/// of the three is closest in aspect ratio to the source image.
pub const ACCEPTED_CANVASES: [(u32, u32); 3] = [(1024, 1536), (1536, 1024), (1472, 1472)];

/// Pick the accepted canvas whose aspect ratio is closest to `width`x`height`.
fn choose_canvas(width: u32, height: u32) -> (u32, u32) {
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

/// Letterbox a bare-base64 image onto the closest accepted canvas.
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
    match letterbox_inner(base64_png) {
        Some(fixed) => fixed,
        None => base64_png.to_string(),
    }
}

fn letterbox_inner(base64_png: &str) -> Option<String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let bytes = engine.decode(base64_png.trim()).ok()?;
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
    }
}
