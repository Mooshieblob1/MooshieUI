//! Animated GIF / WebP export from gallery videos.
//!
//! The pure functions in this module are the canonical implementation of the
//! export math. `src/lib/utils/videoExport.ts` mirrors them so the popover can
//! compute values synchronously without an IPC round-trip per slider move; the
//! two are kept in sync by hand and by these tests.

/// Frame rates below this are not offered, however cleanly they divide.
pub const MIN_OFFERED_FPS: u32 = 6;

/// `auto` loop mode trims the duplicate frame below this measured seam delta.
pub const AUTO_SEAM_THRESHOLD: f32 = 2.0;

/// Default crossfade length, in frames.
pub const DEFAULT_CROSSFADE_FRAMES: u32 = 4;

/// Every integer divisor of `n`, largest first.
pub fn divisors(n: u32) -> Vec<u32> {
    if n == 0 {
        return Vec::new();
    }
    (1..=n).rev().filter(|d| n.is_multiple_of(*d)).collect()
}

/// Frame rates the export picker offers for a clip at `source_fps`.
///
/// Only integer divisors resample cleanly - the `fps` filter dropping 24 to a
/// non-divisor rate discards frames on an uneven cadence, which visibly
/// judders. Rates above the source are absent, not clamped and not greyed out.
pub fn offered_fps(source_fps: u32) -> Vec<u32> {
    if source_fps == 0 {
        return vec![MIN_OFFERED_FPS];
    }
    let offered: Vec<u32> = divisors(source_fps)
        .into_iter()
        .filter(|d| *d >= MIN_OFFERED_FPS)
        .collect();
    if offered.is_empty() {
        // A source below the floor has no legal divisor; offer it as-is rather
        // than rendering an empty picker.
        vec![source_fps]
    } else {
        offered
    }
}

/// Resolve a preset's *target* fps against what this source can actually
/// deliver: the highest offered value at or below the target.
pub fn snap_fps(target: u32, source_fps: u32) -> u32 {
    let offered = offered_fps(source_fps);
    offered
        .iter()
        .copied()
        .find(|v| *v <= target)
        .unwrap_or_else(|| offered.last().copied().unwrap_or(MIN_OFFERED_FPS))
}

/// Output dimensions for a requested width: never upscaled past the source,
/// width snapped down to even first, then height derived from that snapped
/// width and rounded to the nearest even number (not down, so the aspect ratio
/// is preserved as faithfully as possible within the 2px grid).
///
/// The height rounds to NEAREST rather than DOWN because rounding down can
/// accumulate a visible aspect-ratio error; the TS mirror task must copy this
/// same rule.
pub fn output_dimensions(src_w: u32, src_h: u32, requested_w: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (0, 0);
    }
    // Snap width down to even and never upscale past the source.
    let w = (requested_w.min(src_w).max(2)) & !1u32;
    let h_exact = w as f64 * src_h as f64 / src_w as f64;
    // Round height to the nearest even number; .max(2) guards a degenerate
    // very-wide-panorama case.
    let h = (((h_exact / 2.0).round() as u32) * 2).max(2);
    (w, h)
}

/// Whether a clip of `f` frames can crossfade over `n` frames without folding
/// into itself.
pub fn crossfade_available(f: u32, n: u32) -> bool {
    n > 0 && f > 3 * n
}

/// Frames the encoder will actually write for a given loop mode.
///
/// Mirrored by `outputFrameCount` in `src/lib/utils/videoExport.ts`.
pub fn output_frame_count(mode: &str, f: u32, n: u32) -> u32 {
    match mode {
        "trim" => f.saturating_sub(1).max(1),
        "crossfade" => {
            if crossfade_available(f, n) {
                f - n
            } else {
                f
            }
        }
        "pingpong" => {
            if f < 3 {
                f
            } else {
                2 * f - 2
            }
        }
        // "none" and anything unrecognised encode the source verbatim.
        _ => f,
    }
}

/// What `auto` resolves to for a measured seam delta.
///
/// Under the threshold the ends already match, so the only defect left is the
/// duplicate frame - trim it. Above it, trimming would drop a frame that is
/// carrying real motion, so leave the clip alone.
pub fn resolve_auto(seam_delta: f32) -> &'static str {
    if seam_delta < AUTO_SEAM_THRESHOLD {
        "trim"
    } else {
        "none"
    }
}

/// Whether a produced file exceeds the selected platform's attachment limit.
/// An unrecognised or absent target never produces a hint.
pub fn over_size_limit(bytes: u64, target: &str) -> bool {
    match target {
        "discord" => bytes > 10 * 1024 * 1024,
        "nitro" => bytes > 500 * 1024 * 1024,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisors_descend_from_n() {
        assert_eq!(divisors(24), vec![24, 12, 8, 6, 4, 3, 2, 1]);
        assert_eq!(divisors(48), vec![48, 24, 16, 12, 8, 6, 4, 3, 2, 1]);
        assert_eq!(divisors(1), vec![1]);
        assert_eq!(divisors(0), Vec::<u32>::new());
    }

    #[test]
    fn offered_fps_floors_at_six() {
        assert_eq!(offered_fps(24), vec![24, 12, 8, 6]);
        assert_eq!(offered_fps(48), vec![48, 24, 16, 12, 8, 6]);
    }

    #[test]
    fn offered_fps_never_exceeds_the_source() {
        // A 16 fps clip cannot deliver 24, so 24 is absent - not clamped,
        // not greyed out, absent.
        assert!(!offered_fps(16).contains(&24));
        assert_eq!(offered_fps(16), vec![16, 8]);
    }

    #[test]
    fn offered_fps_degenerate_sources_still_offer_something() {
        // Below the floor there is no legal divisor; offer the source itself
        // rather than an empty picker.
        assert_eq!(offered_fps(5), vec![5]);
        assert_eq!(offered_fps(0), vec![MIN_OFFERED_FPS]);
    }

    #[test]
    fn snap_fps_picks_the_highest_offered_at_or_below_target() {
        // GIF Balanced targets 16: exact on a 48 fps clip, snaps down to 12
        // on a 24 fps clip because 16 does not divide 24.
        assert_eq!(snap_fps(16, 48), 16);
        assert_eq!(snap_fps(16, 24), 12);
        assert_eq!(snap_fps(12, 24), 12);
        assert_eq!(snap_fps(24, 24), 24);
        // A target above everything offered lands on the source rate.
        assert_eq!(snap_fps(60, 24), 24);
        // A target below everything offered lands on the lowest offered.
        assert_eq!(snap_fps(2, 24), 6);
    }

    #[test]
    fn output_dimensions_snap_even_and_never_upscale() {
        // 832x480 requested at 640 wide -> 640x370 -> snapped to 640x370.
        assert_eq!(output_dimensions(832, 480, 640), (640, 370));
        // Requesting wider than the source lands at the source width.
        assert_eq!(output_dimensions(480, 270, 640), (480, 270));
        // Odd inputs snap down to even on both axes.
        assert_eq!(output_dimensions(833, 481, 833), (832, 480));
    }

    #[test]
    fn loop_mode_frame_math() {
        assert_eq!(output_frame_count("none", 124, 4), 124);
        assert_eq!(output_frame_count("trim", 124, 4), 123);
        assert_eq!(output_frame_count("crossfade", 124, 4), 120);
        assert_eq!(output_frame_count("pingpong", 124, 4), 246);
    }

    #[test]
    fn loop_mode_frame_math_degenerates_safely() {
        // A one-frame clip cannot be trimmed to nothing.
        assert_eq!(output_frame_count("trim", 1, 4), 1);
        // Ping-pong on a two-frame clip is the clip itself.
        assert_eq!(output_frame_count("pingpong", 2, 4), 2);
        // Crossfade that is not available falls back to the source count.
        assert_eq!(output_frame_count("crossfade", 10, 4), 10);
        // An unknown mode is treated as "none" rather than panicking.
        assert_eq!(output_frame_count("nonsense", 124, 4), 124);
    }

    #[test]
    fn crossfade_offered_only_when_the_clip_is_long_enough() {
        assert!(crossfade_available(124, 4));
        assert!(crossfade_available(13, 4));
        assert!(!crossfade_available(12, 4));
        assert!(!crossfade_available(4, 4));
        assert!(!crossfade_available(124, 0));
    }

    #[test]
    fn auto_trims_a_matching_seam_and_leaves_a_mismatched_one() {
        assert_eq!(resolve_auto(1.2), "trim");
        assert_eq!(resolve_auto(0.0), "trim");
        assert_eq!(resolve_auto(40.0), "none");
        // Exactly at the threshold is not "under" it.
        assert_eq!(resolve_auto(AUTO_SEAM_THRESHOLD), "none");
    }

    #[test]
    fn size_limits_follow_the_platform_table() {
        let ten_mb = 10 * 1024 * 1024;
        assert!(!over_size_limit(ten_mb, "discord"));
        assert!(over_size_limit(ten_mb + 1, "discord"));
        assert!(!over_size_limit(ten_mb + 1, "nitro"));
        assert!(over_size_limit(500 * 1024 * 1024 + 1, "nitro"));
        // No target selected means no hint, ever.
        assert!(!over_size_limit(u64::MAX, "none"));
        assert!(!over_size_limit(u64::MAX, "anything-else"));
    }
}
