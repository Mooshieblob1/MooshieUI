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
//!
//! `src/lib/utils/novelaiModels.ts` mirrors which presets each model offers,
//! so the panel only lists the ones that exist; keep the two in step.

use super::models::NovelAiModel;

/// NovelAI's tag hint ids, shared by `tag_hint_qt` and `tag_hint_uc_preset`.
/// 6 to 8 are V1-era UC presets MooshieUI does not offer.
const HINT_NONE: u8 = 0;
const HINT_STANDARD: u8 = 1;
const HINT_HEAVY: u8 = 2;
const HINT_LIGHT: u8 = 3;
const HINT_HUMAN_FOCUS: u8 = 4;
const HINT_FURRY_FOCUS: u8 = 5;

/// A quality-tag preset as MooshieUI stores it in
/// `NovelAiParams::quality_preset` (0 Standard, 1 Light). Off is the separate
/// `quality_toggle`, which predates the presets and is kept so saved settings
/// stay valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    Standard,
    Light,
}

impl QualityPreset {
    /// MooshieUI's stored index. Anything unknown is Standard, the stack the
    /// toggle always meant.
    pub fn from_index(index: u8) -> Self {
        match index {
            1 => Self::Light,
            _ => Self::Standard,
        }
    }

    /// The inverse of [`Self::from_index`].
    pub fn index(self) -> u8 {
        match self {
            Self::Standard => 0,
            Self::Light => 1,
        }
    }

    /// NovelAI's `tag_hint_qt` value.
    pub fn tag_hint(self) -> u8 {
        match self {
            Self::Standard => HINT_STANDARD,
            Self::Light => HINT_LIGHT,
        }
    }

    /// The inverse of [`Self::tag_hint`], for reading a NovelAI image back.
    /// `None` here means the hint is not a quality preset at all; 0 (off) is
    /// handled by the caller, since it is the toggle rather than a preset.
    pub fn from_tag_hint(hint: u64) -> Option<Self> {
        match hint {
            1 => Some(Self::Standard),
            3 => Some(Self::Light),
            _ => None,
        }
    }
}

/// An undesired-content preset as MooshieUI stores it.
///
/// `NovelAiParams::uc_preset` keeps MooshieUI's own numbering (0 Heavy,
/// 1 Light, 2 Human Focus, 3 None, 4 Furry Focus) so saved settings and
/// gallery metadata stay valid; Furry Focus came later and took the next free
/// slot. It is not NovelAI's numbering: the wire value is [`Self::tag_hint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UcPreset {
    Heavy,
    Light,
    HumanFocus,
    FurryFocus,
    None,
}

impl UcPreset {
    /// MooshieUI's stored index. Anything unknown adds no hidden text.
    pub fn from_index(index: u8) -> Self {
        match index {
            0 => Self::Heavy,
            1 => Self::Light,
            2 => Self::HumanFocus,
            4 => Self::FurryFocus,
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
            Self::FurryFocus => 4,
        }
    }

    /// NovelAI's `tag_hint_uc_preset` value.
    pub fn tag_hint(self) -> u8 {
        match self {
            Self::None => HINT_NONE,
            Self::Heavy => HINT_HEAVY,
            Self::Light => HINT_LIGHT,
            Self::HumanFocus => HINT_HUMAN_FOCUS,
            Self::FurryFocus => HINT_FURRY_FOCUS,
        }
    }

    /// The inverse of [`Self::tag_hint`], for reading a NovelAI image back.
    /// The V1-era ids have no MooshieUI equivalent.
    pub fn from_tag_hint(hint: u64) -> Option<Self> {
        match hint {
            0 => Some(Self::None),
            2 => Some(Self::Heavy),
            3 => Some(Self::Light),
            4 => Some(Self::HumanFocus),
            5 => Some(Self::FurryFocus),
            _ => None,
        }
    }
}

/// `tag_hint_qt` when no quality tags were added.
pub const QUALITY_HINT_NONE: u8 = HINT_NONE;

const V5_QUALITY: &str = "very aesthetic, masterpiece, no text";
const V5_QUALITY_LIGHT: &str = "very aesthetic, amazing quality, no text";
const V4_QUALITY: &str = "no text, best quality, very aesthetic, absurdres";

const V5_HEAVY: &str = "lowres, artistic error, film grain, scan artifacts, worst quality, bad quality, jpeg artifacts, very displeasing, chromatic aberration, dithering, halftone, screentone, multiple views, logo, too many watermarks, negative space, blank page";
const V5_LIGHT: &str = "lowres, bad hands, bad anatomy, artistic error, sepia, white haze, worst quality, very displeasing, jpeg artifacts, 0::ai-generated::";
const V5_HUMAN_FOCUS: &str = "lowres, artistic error, film grain, scan artifacts, worst quality, bad quality, jpeg artifacts, very displeasing, chromatic aberration, dithering, halftone, screentone, multiple views, logo, too many watermarks, negative space, blank page, @_@, mismatched pupils, glowing eyes, bad anatomy";
/// Identical on V5 and V4.5 Full.
const FURRY_FOCUS: &str = "{worst quality}, distracting watermark, unfinished, bad quality, {widescreen}, upscale, {sequence}, {{grandfathered content}}, blurred foreground, chromatic aberration, sketch, everyone, [sketch background], simple, [flat colors], ych (character), outline, multiple scenes, [[horror (theme)]], comic";
const V45_LIGHT: &str = "lowres, artistic error, scan artifacts, worst quality, bad quality, jpeg artifacts, multiple views, very displeasing, too many watermarks, negative space, blank page";
const V4_HEAVY: &str = "blurry, lowres, error, film grain, scan artifacts, worst quality, bad quality, jpeg artifacts, very displeasing, chromatic aberration, multiple views, logo, too many watermarks, white blank page, blank page";
const V4_LIGHT: &str = "blurry, lowres, error, worst quality, bad quality, jpeg artifacts, very displeasing, white blank page, blank page";

fn is_v5(model: &NovelAiModel) -> bool {
    matches!(model.id, "nai-diffusion-5-full" | "nai-diffusion-5-curated")
}

/// The quality preset that actually applies on this model, and its suffix.
///
/// Light only exists from V5 on. Older models offer Standard alone, so Light
/// falls back to it there, which is also what NovelAI's client lands on when
/// the model is switched under it.
pub fn quality_tags(
    model: &NovelAiModel,
    preset: QualityPreset,
) -> Option<(QualityPreset, &'static str)> {
    match (model.id, preset) {
        (_, QualityPreset::Light) if is_v5(model) => Some((preset, V5_QUALITY_LIGHT)),
        // V4.5 Full shares V5's standard stack in the client; the docs page
        // still shows an older `location, ` variant for it.
        ("nai-diffusion-5-full" | "nai-diffusion-5-curated" | "nai-diffusion-4-5-full", _) => {
            Some((QualityPreset::Standard, V5_QUALITY))
        }
        ("nai-diffusion-4-full", _) => Some((QualityPreset::Standard, V4_QUALITY)),
        _ => None,
    }
}

/// The preset that actually applies on this model, and its UC prefix.
///
/// V4 Full has neither focus preset. NovelAI's client resolves a missing
/// preset through its category fallback (human or furry, then heavy, then
/// light), so both become Heavy there.
pub fn uc_preset(model: &NovelAiModel, preset: UcPreset) -> (UcPreset, Option<&'static str>) {
    let text = match (model.id, preset) {
        (_, UcPreset::None) => None,
        (_, UcPreset::Heavy) if is_v5(model) => Some(V5_HEAVY),
        (_, UcPreset::Light) if is_v5(model) => Some(V5_LIGHT),
        (_, UcPreset::HumanFocus) if is_v5(model) => Some(V5_HUMAN_FOCUS),
        (_, UcPreset::FurryFocus) if is_v5(model) => Some(FURRY_FOCUS),
        ("nai-diffusion-4-5-full", UcPreset::Heavy) => Some(V5_HEAVY),
        ("nai-diffusion-4-5-full", UcPreset::Light) => Some(V45_LIGHT),
        ("nai-diffusion-4-5-full", UcPreset::HumanFocus) => Some(V5_HUMAN_FOCUS),
        ("nai-diffusion-4-5-full", UcPreset::FurryFocus) => Some(FURRY_FOCUS),
        ("nai-diffusion-4-full", UcPreset::Heavy | UcPreset::HumanFocus | UcPreset::FurryFocus) => {
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

    const ALL_UC: [UcPreset; 5] = [
        UcPreset::Heavy,
        UcPreset::Light,
        UcPreset::HumanFocus,
        UcPreset::FurryFocus,
        UcPreset::None,
    ];

    fn model(id: &str) -> &'static NovelAiModel {
        models::find(id).unwrap()
    }

    #[test]
    fn stored_indices_round_trip() {
        for preset in ALL_UC {
            assert_eq!(UcPreset::from_index(preset.index()), preset);
            assert_eq!(
                UcPreset::from_tag_hint(preset.tag_hint().into()),
                Some(preset)
            );
        }
        for preset in [QualityPreset::Standard, QualityPreset::Light] {
            assert_eq!(QualityPreset::from_index(preset.index()), preset);
            assert_eq!(
                QualityPreset::from_tag_hint(preset.tag_hint().into()),
                Some(preset)
            );
        }
        // Saved settings from before Furry Focus keep their meaning.
        assert_eq!(UcPreset::from_index(3), UcPreset::None);
        assert_eq!(UcPreset::from_index(9), UcPreset::None);
        assert_eq!(QualityPreset::from_index(9), QualityPreset::Standard);
        // V1-era presets have no MooshieUI slot.
        assert_eq!(UcPreset::from_tag_hint(6), None);
    }

    #[test]
    fn tag_hints_follow_the_official_table() {
        assert_eq!(UcPreset::None.tag_hint(), 0);
        assert_eq!(QualityPreset::Standard.tag_hint(), 1);
        assert_eq!(UcPreset::Heavy.tag_hint(), 2);
        assert_eq!(UcPreset::Light.tag_hint(), 3);
        assert_eq!(QualityPreset::Light.tag_hint(), 3);
        assert_eq!(UcPreset::HumanFocus.tag_hint(), 4);
        assert_eq!(UcPreset::FurryFocus.tag_hint(), 5);
        assert_eq!(QUALITY_HINT_NONE, 0);
    }

    #[test]
    fn every_model_has_quality_tags_and_presets() {
        for m in MODELS {
            assert!(
                quality_tags(m, QualityPreset::Standard).is_some(),
                "{}",
                m.id
            );
            for p in ALL_UC {
                if p != UcPreset::None {
                    assert!(uc_preset(m, p).1.is_some(), "{} {p:?}", m.id);
                }
            }
            assert_eq!(uc_preset(m, UcPreset::None), (UcPreset::None, None));
        }
    }

    #[test]
    fn v5_text_matches_the_docs() {
        for id in ["nai-diffusion-5-full", "nai-diffusion-5-curated"] {
            let v5 = model(id);
            assert_eq!(
                quality_tags(v5, QualityPreset::Standard),
                Some((
                    QualityPreset::Standard,
                    "very aesthetic, masterpiece, no text"
                ))
            );
            assert_eq!(
                quality_tags(v5, QualityPreset::Light),
                Some((
                    QualityPreset::Light,
                    "very aesthetic, amazing quality, no text"
                ))
            );
            for (preset, ends) in [
                (
                    UcPreset::Heavy,
                    "too many watermarks, negative space, blank page",
                ),
                (UcPreset::Light, "0::ai-generated::"),
                (
                    UcPreset::HumanFocus,
                    "@_@, mismatched pupils, glowing eyes, bad anatomy",
                ),
                (
                    UcPreset::FurryFocus,
                    "multiple scenes, [[horror (theme)]], comic",
                ),
            ] {
                let (effective, text) = uc_preset(v5, preset);
                assert_eq!(effective, preset, "{id} {preset:?}");
                assert!(text.unwrap().ends_with(ends), "{id} {preset:?}");
            }
        }
    }

    #[test]
    fn light_quality_falls_back_to_standard_before_v5() {
        for id in ["nai-diffusion-4-5-full", "nai-diffusion-4-full"] {
            let m = model(id);
            assert_eq!(
                quality_tags(m, QualityPreset::Light),
                quality_tags(m, QualityPreset::Standard),
                "{id}"
            );
            assert_eq!(
                quality_tags(m, QualityPreset::Light).unwrap().0,
                QualityPreset::Standard
            );
        }
    }

    #[test]
    fn v45_full_offers_furry_focus() {
        let (effective, text) = uc_preset(model("nai-diffusion-4-5-full"), UcPreset::FurryFocus);
        assert_eq!(effective, UcPreset::FurryFocus);
        assert_eq!(text, Some(FURRY_FOCUS));
    }

    #[test]
    fn v4_focus_presets_fall_back_to_heavy() {
        let v4 = model("nai-diffusion-4-full");
        for preset in [UcPreset::HumanFocus, UcPreset::FurryFocus] {
            assert_eq!(
                uc_preset(v4, preset),
                (UcPreset::Heavy, Some(V4_HEAVY)),
                "{preset:?}"
            );
        }
    }

    #[test]
    fn only_curated_skips_the_nsfw_guard() {
        for m in MODELS {
            assert_eq!(guards_nsfw(m), !m.id.ends_with("-curated"), "{}", m.id);
        }
    }
}
