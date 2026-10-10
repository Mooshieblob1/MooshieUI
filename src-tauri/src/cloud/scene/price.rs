//! Provider price formulas, from each provider's own published pricing.
//! Never from aggregator tables: the prototype's first quote came from one
//! and was off by two thirds (research doc, section 3 lesson 1).
//!
//! Every figure is an estimate. The provider's own billing is authoritative.

use serde::Serialize;

/// When the constants below were last checked against the provider's page.
pub const FAL_SEEDANCE_25_CHECKED: &str = "2026-10-11";

/// fal's published per-second examples for Seedance 2.5 reference-to-video
/// (fal model page, checked on `FAL_SEEDANCE_25_CHECKED`). fal derives them
/// from its token formula, `height x width x (input_seconds + output_seconds)
/// x 24 / 1024` tokens at $0.0214 per 1,000 (480p and 720p) or about $0.0234
/// (1080p). Image and audio references are not billed.
const FAL_SEEDANCE_25_USD_PER_SECOND: [(&str, f64); 3] =
    [("480p", 0.2205), ("720p", 0.4730), ("1080p", 1.164)];

/// fal bills video inputs at 0.6x. Scenes send no video inputs today.
const FAL_VIDEO_INPUT_MULTIPLIER: f64 = 0.6;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VideoEstimate {
    pub usd: f64,
    pub resolution: String,
    pub seconds: u32,
    pub checked: &'static str,
}

/// Estimated fal charge for a Seedance 2.5 clip. A draft renders at 480p
/// whatever resolution was asked for.
pub fn fal_seedance_25(
    resolution: &str,
    seconds: u32,
    draft: bool,
    video_input_seconds: f64,
) -> Option<VideoEstimate> {
    let billed = if draft { "480p" } else { resolution };
    let per_second = FAL_SEEDANCE_25_USD_PER_SECOND
        .iter()
        .find(|(r, _)| *r == billed)?
        .1;
    let usd = per_second * f64::from(seconds)
        + per_second * video_input_seconds.max(0.0) * FAL_VIDEO_INPUT_MULTIPLIER;
    Some(VideoEstimate {
        usd: (usd * 100.0).round() / 100.0,
        resolution: billed.to_string(),
        seconds,
        checked: FAL_SEEDANCE_25_CHECKED,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prototype_draft_matches_its_real_charge() {
        // 28 s at 480p cost about $6.17 in the prototype run.
        let e = fal_seedance_25("1080p", 28, true, 0.0).unwrap();
        assert_eq!(e.resolution, "480p");
        assert!((e.usd - 6.17).abs() < 0.01, "{}", e.usd);
    }

    #[test]
    fn resolutions_follow_the_published_examples() {
        assert!((fal_seedance_25("720p", 10, false, 0.0).unwrap().usd - 4.73).abs() < 0.01);
        assert!((fal_seedance_25("1080p", 10, false, 0.0).unwrap().usd - 11.64).abs() < 0.01);
        assert!(fal_seedance_25("4k", 10, false, 0.0).is_none());
    }

    #[test]
    fn video_inputs_cost_extra() {
        let plain = fal_seedance_25("720p", 10, false, 0.0).unwrap().usd;
        let with_video = fal_seedance_25("720p", 10, false, 5.0).unwrap().usd;
        assert!(with_video > plain);
    }
}
