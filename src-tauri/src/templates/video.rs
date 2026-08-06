//! MiniMax H3 video workflow builder (fl2va and ref2va variants).
//!
//! Unlike the image builders, `build` returns the final workflow JSON
//! directly and never goes through `finish_workflow` — upscale, face fix,
//! segment refinement, and `MooshieSaveImage` are all image-only concepts.
//! The terminal node is `MooshieSaveVideo` (deployed by PR 2).

use serde_json::{json, Value};

use crate::comfyui::types::GenerationParams;

/// Filename markers identifying MiniMax H3 model files (matched as lowercase
/// substrings, any-match). Consumed by the video arm of
/// `validate_generation_params`; PR 5's model onboarding reuses them to
/// filter dropdown lists.
pub const H3_DIFFUSION_MARKERS: [&str; 2] = ["minimax", "h3"];

/// MiniMax H3 emits 24 fps and only accepts frame counts on the 17n+5 grid
/// (widget: min 5, max 3600, step 17). Snaps the requested duration UP to
/// the next valid count, clamped to the largest on-grid value <= 3600.
pub fn compute_h3_frame_length(seconds: f64) -> u32 {
    let base = ((seconds * 24.0).round() as i64).max(5);
    let snapped = base + (5 - (base % 17)).rem_euclid(17);
    snapped.min(3592) as u32
}

/// Width/height from a target megapixel budget and aspect ratio, snapped to
/// multiples of 32 (the H3 width/height widget step). Replaces the official
/// workflow's ResolutionSelector custom node. Unknown ratios fall back to
/// 16:9.
pub fn compute_h3_dimensions(aspect_ratio: &str, megapixels: f64) -> (u32, u32) {
    let (rw, rh) = match aspect_ratio {
        "9:16" => (9.0, 16.0),
        "1:1" => (1.0, 1.0),
        "4:3" => (4.0, 3.0),
        "3:4" => (3.0, 4.0),
        _ => (16.0, 9.0),
    };
    let pixels = megapixels.max(0.05) * 1_000_000.0;
    let height = (pixels * rh / rw).sqrt();
    let width = height * rw / rh;
    let snap = |v: f64| ((v / 32.0).round() as u32).max(2) * 32;
    (snap(width), snap(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_length_snaps_up_to_17n_plus_5() {
        assert_eq!(compute_h3_frame_length(5.0), 124);
        assert_eq!(compute_h3_frame_length(1.0), 39);
        assert_eq!(compute_h3_frame_length(15.0), 362);
        // Robustness at the edges: never below the widget minimum,
        // never above the largest on-grid value <= 3600.
        assert_eq!(compute_h3_frame_length(0.0), 5);
        assert_eq!(compute_h3_frame_length(500.0), 3592);
    }

    #[test]
    fn frame_length_is_always_on_grid() {
        for tenths in 10..=150 {
            let length = compute_h3_frame_length(tenths as f64 / 10.0);
            assert_eq!(
                length % 17,
                5,
                "off-grid length {} for {} s",
                length,
                tenths as f64 / 10.0
            );
        }
    }

    #[test]
    fn dimensions_hit_known_targets() {
        assert_eq!(compute_h3_dimensions("16:9", 0.4), (832, 480));
        assert_eq!(compute_h3_dimensions("9:16", 0.4), (480, 832));
        assert_eq!(compute_h3_dimensions("1:1", 0.4), (640, 640));
        assert_eq!(compute_h3_dimensions("4:3", 0.4), (736, 544));
        assert_eq!(compute_h3_dimensions("3:4", 0.4), (544, 736));
        assert_eq!(compute_h3_dimensions("16:9", 0.6), (1024, 576));
        // Unknown ratios fall back to 16:9.
        assert_eq!(compute_h3_dimensions("bogus", 0.4), (832, 480));
    }

    #[test]
    fn dimensions_are_multiples_of_32() {
        for ratio in ["16:9", "9:16", "1:1", "4:3", "3:4"] {
            for mp in [0.2, 0.4, 0.6, 1.0] {
                let (w, h) = compute_h3_dimensions(ratio, mp);
                assert_eq!(w % 32, 0);
                assert_eq!(h % 32, 0);
                assert!(w >= 64 && h >= 64);
            }
        }
    }
}
