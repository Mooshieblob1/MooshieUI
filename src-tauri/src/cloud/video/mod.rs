//! Cloud video models. What a model can do is data ([`VideoCapabilities`]),
//! so the scene builder and the UI read limits from here and never branch on
//! a provider's name (research doc 5.2).
//!
//! Only fal's Seedance 2.5 is wired today. Adding a model means a new
//! [`VideoModel`] variant, its capabilities and price, and a request builder
//! in its provider module.

pub mod fal;

use serde::{Deserialize, Serialize};

use super::scene::price::{self, VideoEstimate};
use super::CloudProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoModel {
    FalSeedance25,
}

impl VideoModel {
    pub const ALL: [VideoModel; 1] = [VideoModel::FalSeedance25];

    pub fn capabilities(self) -> VideoCapabilities {
        match self {
            VideoModel::FalSeedance25 => VideoCapabilities {
                model: self,
                provider: CloudProvider::Fal,
                label: "Seedance 2.5 (fal.ai)",
                min_seconds: 4,
                max_seconds: 30,
                max_images: 30,
                max_image_bytes: 30 * 1024 * 1024,
                max_audio: 10,
                audio_min_seconds: 1.8,
                audio_max_seconds: 30.2,
                max_audio_bytes: 15 * 1024 * 1024,
                resolutions: &["480p", "720p", "1080p"],
                aspect_ratios: &["16:9", "9:16", "4:3", "3:4", "1:1", "21:9"],
                draft: true,
                audio_reference: true,
                price_checked: price::FAL_SEEDANCE_25_CHECKED,
            },
        }
    }

    pub fn estimate(self, resolution: &str, seconds: u32, draft: bool) -> Option<VideoEstimate> {
        match self {
            VideoModel::FalSeedance25 => price::fal_seedance_25(resolution, seconds, draft, 0.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VideoCapabilities {
    pub model: VideoModel,
    pub provider: CloudProvider,
    pub label: &'static str,
    pub min_seconds: u32,
    pub max_seconds: u32,
    pub max_images: usize,
    pub max_image_bytes: usize,
    pub max_audio: usize,
    /// Per audio reference, and for all audio together.
    pub audio_min_seconds: f64,
    pub audio_max_seconds: f64,
    pub max_audio_bytes: usize,
    pub resolutions: &'static [&'static str],
    pub aspect_ratios: &'static [&'static str],
    /// A cheap 480p draft that can later be completed at full resolution.
    pub draft: bool,
    /// Accepts a voice track as a reference.
    pub audio_reference: bool,
    pub price_checked: &'static str,
}

pub fn all_capabilities() -> Vec<VideoCapabilities> {
    VideoModel::ALL.iter().map(|m| m.capabilities()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_resolution_has_a_price() {
        for caps in all_capabilities() {
            for resolution in caps.resolutions {
                assert!(
                    caps.model
                        .estimate(resolution, caps.max_seconds, false)
                        .is_some(),
                    "{resolution}"
                );
            }
        }
    }

    #[test]
    fn audio_limits_match_the_track_builder() {
        let caps = VideoModel::FalSeedance25.capabilities();
        assert_eq!(
            caps.audio_min_seconds,
            crate::cloud::scene::audio::AUDIO_MIN_SECONDS
        );
        assert_eq!(
            caps.audio_max_seconds,
            crate::cloud::scene::audio::AUDIO_MAX_SECONDS
        );
    }
}
