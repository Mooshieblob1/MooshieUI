//! The quality-tag and undesired-content preset text NovelAI's own client
//! writes into a request.
//!
//! NovelAI's backend does not expand either setting. The official web client
//! composes the text itself: the quality suffix is appended to the prompt and
//! the preset prefix is prepended to the UC before the request leaves the
//! browser, and the only trace of the two settings in the body is a pair of
//! numeric `tag_hint_*` fields. The strings and tables below are copied from
//! that client (novelai.net `_app` bundle) and agree with
//! docs.novelai.net/en/image/qualitytags and /undesiredcontent.

use super::models::NovelAiModel;

/// An undesired-content preset as MooshieUI stores it.
///
/// `NovelAiParams::uc_preset` keeps MooshieUI's own numbering (0 Heavy,
/// 1 Light, 2 Human Focus, 3 None) so saved settings and gallery metadata stay
/// valid. It is not NovelAI's numbering: the wire value is [`Self::tag_hint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UcPreset {
    Heavy,
    Light,
    HumanFocus,
    None,
}

impl UcPreset {
    /// MooshieUI's stored index. Anything unknown adds no hidden text.
    pub fn from_index(index: u8) -> Self {
        match index {
            0 => Self::Heavy,
            1 => Self::Light,
            2 => Self::HumanFocus,
            _ => Self::None,
        }
    }

    /// The inverse of [`Self::from_index`].
    pub fn index(self) -> u8 {
        match self {
            Self::Heavy => 0,
            Self::Light => 1,
            Self::HumanFocus => 2,
            Self::None => 3,
        }
    }

    /// NovelAI's `tag_hint_uc_preset` value. The client's table is shared with
    /// the quality hint: 0 none, 1 standard, 2 heavy, 3 light, 4 human focus,
    /// 5 furry focus.
    pub fn tag_hint(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Heavy => 2,
            Self::Light => 3,
            Self::HumanFocus => 4,
        }
    }

    /// The inverse of [`Self::tag_hint`], for reading a NovelAI image back.
    /// Furry Focus and the V1-era ids have no MooshieUI equivalent.
    pub fn from_tag_hint(hint: u64) -> Option<Self> {
        match hint {
            0 => Some(Self::None),
            2 => Some(Self::Heavy),
            3 => Some(Self::Light),
            4 => Some(Self::HumanFocus),
            _ => None,
        }
    }
}

/// `tag_hint_qt` for the standard quality preset, and for none.
pub const QUALITY_HINT_STANDARD: u8 = 1;
pub const QUALITY_HINT_NONE: u8 = 0;

const V5_QUALITY: &str = "very aesthetic, masterpiece, no text";
const V4_QUALITY: &str = "no text, best quality, very aesthetic, absurdres";

const V5_HEAVY: &str = "lowres, artistic error, film grain, scan artifacts, worst quality, bad quality, jpeg artifacts, very displeasing, chromatic aberration, dithering, halftone, screentone, multiple views, logo, too many watermarks, negative space, blank page";
const V5_LIGHT: &str = "lowres, bad hands, bad anatomy, artistic error, sepia, white haze, worst quality, very displeasing, jpeg artifacts, 0::ai-generated::";
const V5_HUMAN_FOCUS: &str = "lowres, artistic error, film grain, scan artifacts, worst quality, bad quality, jpeg artifacts, very displeasing, chromatic aberration, dithering, halftone, screentone, multiple views, logo, too many watermarks, negative space, blank page, @_@, mismatched pupils, glowing eyes, bad anatomy";
const V45_LIGHT: &str = "lowres, artistic error, scan artifacts, worst quality, bad quality, jpeg artifacts, multiple views, very displeasing, too many watermarks, negative space, blank page";
const V4_HEAVY: &str = "blurry, lowres, error, film grain, scan artifacts, worst quality, bad quality, jpeg artifacts, very displeasing, chromatic aberration, multiple views, logo, too many watermarks, white blank page, blank page";
const V4_LIGHT: &str = "blurry, lowres, error, worst quality, bad quality, jpeg artifacts, very displeasing, white blank page, blank page";

/// The standard quality suffix for a model, appended to the prompt when the
/// quality toggle is on.
pub fn quality_tags(model: &NovelAiModel) -> Option<&'static str> {
    match model.id {
        // V4.5 Full shares V5's standard stack in the client; the docs page
        // still shows an older `location, ` variant for it.
        "nai-diffusion-5-full" | "nai-diffusion-5-curated" | "nai-diffusion-4-5-full" => {
            Some(V5_QUALITY)
        }
        "nai-diffusion-4-full" => Some(V4_QUALITY),
        _ => None,
    }
}

/// The preset that actually applies on this model, and its UC prefix.
///
/// V4 Full has no Human Focus preset. NovelAI's client resolves a missing
/// preset through its category fallback (human, then heavy, then light), so
/// Human Focus becomes Heavy there.
pub fn uc_preset(model: &NovelAiModel, preset: UcPreset) -> (UcPreset, Option<&'static str>) {
    let text = match (model.id, preset) {
        (_, UcPreset::None) => None,
        ("nai-diffusion-5-full" | "nai-diffusion-5-curated", UcPreset::Heavy) => Some(V5_HEAVY),
        ("nai-diffusion-5-full" | "nai-diffusion-5-curated", UcPreset::Light) => Some(V5_LIGHT),
        ("nai-diffusion-5-full" | "nai-diffusion-5-curated", UcPreset::HumanFocus) => {
            Some(V5_HUMAN_FOCUS)
        }
        ("nai-diffusion-4-5-full", UcPreset::Heavy) => Some(V5_HEAVY),
        ("nai-diffusion-4-5-full", UcPreset::Light) => Some(V45_LIGHT),
        ("nai-diffusion-4-5-full", UcPreset::HumanFocus) => Some(V5_HUMAN_FOCUS),
        ("nai-diffusion-4-full", UcPreset::Heavy | UcPreset::HumanFocus) => {
            return (UcPreset::Heavy, Some(V4_HEAVY));
        }
        ("nai-diffusion-4-full", UcPreset::Light) => Some(V4_LIGHT),
        _ => return (UcPreset::None, None),
    };
    (preset, text)
}

/// Curated models are SFW by construction, so the client only adds the `nsfw`
/// guard to the UC of the Full models.
pub fn guards_nsfw(model: &NovelAiModel) -> bool {
    model.id != "nai-diffusion-5-curated"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::models::{self, MODELS};

    #[test]
    fn stored_indices_round_trip() {
        for preset in [
            UcPreset::Heavy,
            UcPreset::Light,
            UcPreset::HumanFocus,
            UcPreset::None,
        ] {
            assert_eq!(UcPreset::from_index(preset.index()), preset);
            assert_eq!(
                UcPreset::from_tag_hint(preset.tag_hint().into()),
                Some(preset)
            );
        }
        assert_eq!(UcPreset::from_index(9), UcPreset::None);
        // Furry Focus has no MooshieUI slot.
        assert_eq!(UcPreset::from_tag_hint(5), None);
    }

    #[test]
    fn tag_hints_follow_the_official_table() {
        assert_eq!(UcPreset::None.tag_hint(), 0);
        assert_eq!(UcPreset::Heavy.tag_hint(), 2);
        assert_eq!(UcPreset::Light.tag_hint(), 3);
        assert_eq!(UcPreset::HumanFocus.tag_hint(), 4);
        assert_eq!(QUALITY_HINT_STANDARD, 1);
    }

    #[test]
    fn every_model_has_quality_tags_and_presets() {
        for m in MODELS {
            assert!(quality_tags(m).is_some(), "{}", m.id);
            for p in [UcPreset::Heavy, UcPreset::Light, UcPreset::HumanFocus] {
                assert!(uc_preset(m, p).1.is_some(), "{} {p:?}", m.id);
            }
            assert_eq!(uc_preset(m, UcPreset::None), (UcPreset::None, None));
        }
    }

    #[test]
    fn v5_text_matches_the_docs() {
        let v5 = models::find("nai-diffusion-5-full").unwrap();
        assert_eq!(
            quality_tags(v5),
            Some("very aesthetic, masterpiece, no text")
        );
        assert!(uc_preset(v5, UcPreset::Heavy)
            .1
            .unwrap()
            .ends_with("too many watermarks, negative space, blank page"));
        assert!(uc_preset(v5, UcPreset::Light)
            .1
            .unwrap()
            .ends_with("0::ai-generated::"));
        assert!(uc_preset(v5, UcPreset::HumanFocus)
            .1
            .unwrap()
            .ends_with("@_@, mismatched pupils, glowing eyes, bad anatomy"));
    }

    #[test]
    fn v4_human_focus_falls_back_to_heavy() {
        let v4 = models::find("nai-diffusion-4-full").unwrap();
        let (effective, text) = uc_preset(v4, UcPreset::HumanFocus);
        assert_eq!(effective, UcPreset::Heavy);
        assert_eq!(text, Some(V4_HEAVY));
    }

    #[test]
    fn only_curated_skips_the_nsfw_guard() {
        for m in MODELS {
            assert_eq!(guards_nsfw(m), !m.id.ends_with("-curated"), "{}", m.id);
        }
    }
}
