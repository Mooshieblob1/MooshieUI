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

/// Which VFI model from the Frame-Interpolation pack does the work. Both
/// register from the same pack install, so `is_rife_installed` gates both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterpEngine {
    #[default]
    Rife,
    /// GMFSS Fortuna: slower than RIFE but markedly better on anime line art.
    /// Its checkpoints are auto-downloaded by the pack on first use into
    /// `ComfyUI-Frame-Interpolation/ckpts/gmfss_fortuna/`.
    Gmfss,
}

impl InterpEngine {
    /// Untrusted strings (REST args, saved settings) fall back to RIFE rather
    /// than erroring: an unknown engine name should degrade, not brick the job.
    pub fn parse(value: &str) -> Self {
        match value {
            "gmfss" => Self::Gmfss,
            _ => Self::Rife,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RifeSettings {
    pub engine: InterpEngine,
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
            engine: InterpEngine::Rife,
            multiplier: multiplier.clamp(MIN_MULTIPLIER, MAX_MULTIPLIER),
            scale_factor: nearest,
            fast_mode,
            ensemble,
        }
    }

    pub fn with_engine(mut self, engine: InterpEngine) -> Self {
        self.engine = engine;
        self
    }

    pub fn from_params(params: &GenerationParams) -> Self {
        Self::sanitized(
            params.video_rife_multiplier,
            params.video_rife_scale_factor,
            params.video_rife_fast_mode,
            params.video_rife_ensemble,
        )
        .with_engine(InterpEngine::parse(&params.video_interp_engine))
    }

    /// Playback rate after interpolation. Duration never changes: the node
    /// emits `(N - 1) * multiplier + 1` frames, so the frame rate has to rise
    /// by the same factor for the clip to last as long as it did.
    pub fn output_fps(&self, source_fps: f64) -> f64 {
        source_fps * self.multiplier as f64
    }

    /// A complete VFI node for the selected engine. Every widget is sent
    /// explicitly: ComfyUI errors on a missing required input, and the pack's
    /// own defaults are not guaranteed to survive an upstream update.
    ///
    /// GMFSS Fortuna takes no `fast_mode`/`ensemble`/`scale_factor` — those are
    /// RIFE-arch knobs — so those settings are simply ignored under GMFSS.
    pub fn node(&self, frames: Value) -> Value {
        match self.engine {
            InterpEngine::Rife => json!({
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
            }),
            InterpEngine::Gmfss => json!({
                "class_type": "GMFSS Fortuna VFI",
                "inputs": {
                    "frames": frames,
                    // The union checkpoint runs RIFE 4.6 for the flow half, which
                    // handles fast motion better than the pure-GMFSS weights.
                    "ckpt_name": "GMFSS_fortuna_union",
                    "clear_cache_after_n_frames": 10,
                    "multiplier": self.multiplier
                }
            }),
        }
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

    #[test]
    fn gmfss_engine_emits_the_fortuna_node_without_rife_knobs() {
        let node = RifeSettings::sanitized(3, 1.0, true, true)
            .with_engine(InterpEngine::Gmfss)
            .node(json!(["7", 0]));
        assert_eq!(node["class_type"], json!("GMFSS Fortuna VFI"));
        assert_eq!(node["inputs"]["frames"], json!(["7", 0]));
        assert_eq!(node["inputs"]["ckpt_name"], json!("GMFSS_fortuna_union"));
        assert_eq!(node["inputs"]["clear_cache_after_n_frames"], json!(10));
        assert_eq!(node["inputs"]["multiplier"], json!(3));
        for rife_only in ["fast_mode", "ensemble", "scale_factor", "dtype"] {
            assert!(
                node["inputs"].get(rife_only).is_none(),
                "GMFSS node must not send RIFE widget {rife_only}"
            );
        }
    }

    #[test]
    fn engine_parse_defaults_unknown_strings_to_rife() {
        assert_eq!(InterpEngine::parse("gmfss"), InterpEngine::Gmfss);
        assert_eq!(InterpEngine::parse("rife"), InterpEngine::Rife);
        assert_eq!(InterpEngine::parse("sepconv"), InterpEngine::Rife);
        assert_eq!(InterpEngine::parse(""), InterpEngine::Rife);
    }
}
