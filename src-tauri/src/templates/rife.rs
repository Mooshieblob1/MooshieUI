//! Shared builder for the `RIFE VFI` node.
//!
//! Two graphs need an identical widget block: the inline video graph
//! (interpolate while generating) and the post-hoc graph (interpolate a clip
//! that is already in the gallery). One builder means a pack update that adds
//! or renames a required widget is a one-line fix, not two that can drift.

use serde_json::{json, Value};

use crate::comfyui::types::GenerationParams;

/// The node declares `multiplier` as `INT, min 1` with no maximum, but RIFE
/// holds its entire output as one CPU float32 batch. A 5 second 720p clip at
/// 4x is already around 5 GB, so higher factors are a memory trap rather than
/// a feature.
pub const MIN_MULTIPLIER: u32 = 1;
pub const MAX_MULTIPLIER: u32 = 4;

/// `scale_factor` is a combo widget, not a free float. ComfyUI rejects a value
/// that is not in the list, so anything a client sends is snapped to the
/// nearest entry instead of being passed through.
pub const SCALE_FACTORS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RifeSettings {
    pub multiplier: u32,
    pub scale_factor: f64,
    pub fast_mode: bool,
    pub ensemble: bool,
}

impl RifeSettings {
    /// Clamp and snap untrusted input. The browser-mode REST endpoint takes
    /// whatever a LAN client sends, so validation lives here rather than in the
    /// UI that normally produces these values.
    pub fn sanitized(multiplier: u32, scale_factor: f64, fast_mode: bool, ensemble: bool) -> Self {
        let requested = if scale_factor.is_finite() {
            scale_factor
        } else {
            1.0
        };
        let nearest = SCALE_FACTORS
            .iter()
            .copied()
            .min_by(|a, b| {
                (a - requested)
                    .abs()
                    .partial_cmp(&(b - requested).abs())
                    .expect("scale factors and requested value are finite")
            })
            .unwrap_or(1.0);

        Self {
            multiplier: multiplier.clamp(MIN_MULTIPLIER, MAX_MULTIPLIER),
            scale_factor: nearest,
            fast_mode,
            ensemble,
        }
    }

    pub fn from_params(params: &GenerationParams) -> Self {
        Self::sanitized(
            params.video_rife_multiplier,
            params.video_rife_scale_factor,
            params.video_rife_fast_mode,
            params.video_rife_ensemble,
        )
    }

    /// Playback rate after interpolation. Duration never changes: the node
    /// emits `(N - 1) * multiplier + 1` frames, so the frame rate has to rise
    /// by the same factor for the clip to last as long as it did.
    pub fn output_fps(&self, source_fps: f64) -> f64 {
        source_fps * self.multiplier as f64
    }

    /// A complete `RIFE VFI` node. Every widget is sent explicitly: ComfyUI
    /// errors on a missing required input, and the pack's own defaults are not
    /// guaranteed to survive an upstream update.
    pub fn node(&self, frames: Value) -> Value {
        json!({
            "class_type": "RIFE VFI",
            "inputs": {
                "frames": frames,
                "ckpt_name": crate::comfyui::nodes::RIFE_CKPT_FILENAME,
                "clear_cache_after_n_frames": 10,
                "multiplier": self.multiplier,
                "fast_mode": self.fast_mode,
                "ensemble": self.ensemble,
                "scale_factor": self.scale_factor,
                "dtype": "float32",
                "torch_compile": false,
                "batch_size": 1
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sanitized_clamps_multiplier_to_supported_range() {
        assert_eq!(RifeSettings::sanitized(0, 1.0, true, true).multiplier, 1);
        assert_eq!(RifeSettings::sanitized(3, 1.0, true, true).multiplier, 3);
        assert_eq!(RifeSettings::sanitized(99, 1.0, true, true).multiplier, 4);
    }

    #[test]
    fn sanitized_snaps_scale_factor_to_the_node_combo() {
        assert_eq!(
            RifeSettings::sanitized(2, 0.6, true, true).scale_factor,
            0.5
        );
        assert_eq!(
            RifeSettings::sanitized(2, 3.5, true, true).scale_factor,
            4.0
        );
        assert_eq!(
            RifeSettings::sanitized(2, -8.0, true, true).scale_factor,
            0.25
        );
        assert_eq!(
            RifeSettings::sanitized(2, f64::NAN, true, true).scale_factor,
            1.0
        );
    }

    #[test]
    fn output_fps_scales_with_the_multiplier() {
        assert_eq!(
            RifeSettings::sanitized(3, 1.0, true, true).output_fps(24.0),
            72.0
        );
        assert_eq!(
            RifeSettings::sanitized(2, 1.0, true, true).output_fps(48.0),
            96.0
        );
    }

    #[test]
    fn node_sends_every_widget_the_pack_declares() {
        let node = RifeSettings::sanitized(4, 0.5, false, false).node(json!(["7", 0]));
        assert_eq!(node["class_type"], json!("RIFE VFI"));
        for key in [
            "frames",
            "ckpt_name",
            "clear_cache_after_n_frames",
            "multiplier",
            "fast_mode",
            "ensemble",
            "scale_factor",
            "dtype",
            "torch_compile",
            "batch_size",
        ] {
            assert!(node["inputs"].get(key).is_some(), "missing widget {key}");
        }
        assert_eq!(node["inputs"]["frames"], json!(["7", 0]));
        assert_eq!(node["inputs"]["multiplier"], json!(4));
        assert_eq!(node["inputs"]["scale_factor"], json!(0.5));
        assert_eq!(node["inputs"]["fast_mode"], json!(false));
        assert_eq!(node["inputs"]["ensemble"], json!(false));
        assert_eq!(node["inputs"]["dtype"], json!("float32"));
        assert_eq!(node["inputs"]["torch_compile"], json!(false));
        assert_eq!(node["inputs"]["batch_size"], json!(1));
    }
}
