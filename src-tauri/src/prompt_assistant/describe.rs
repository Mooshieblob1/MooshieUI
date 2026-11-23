//! Describe an image in words, as a prompt that would recreate it.
//!
//! The natural-language half of Interrogate: the tagger answers in Danbooru
//! tags, which suit anime checkpoints but not the prose-prompted families
//! (Flux, Krea, H3, Qwen Image). This asks a model that can see the image for
//! the paragraph those families want instead.

use super::vision::VisionSupport;
use crate::error::AppError;

/// Where a `Describe image` request should run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescribeRoute {
    /// Use the active external provider first, keeping `fallback_local_vision`
    /// in reserve if the external model turns out to be text-only.
    External {
        fallback_local_vision: Option<String>,
    },
    /// Route directly to an installed bundled vision model (preferring JoyCaption).
    Local(String),
}

/// Choose the route for `Describe image` from the external-provider toggle, the
/// external model's vision capability, and any installed bundled vision model.
pub fn plan_route(
    ext_enabled: bool,
    vision_support: VisionSupport,
    local_vision: Option<String>,
) -> Result<DescribeRoute, AppError> {
    if !ext_enabled {
        return local_vision
            .map(DescribeRoute::Local)
            .ok_or_else(|| AppError::LlmError("prompt_assistant.no_vision_model".into()));
    }
    match vision_support {
        VisionSupport::Unsupported => match local_vision {
            Some(model) => Ok(DescribeRoute::Local(model)),
            None => Err(AppError::LlmError(
                "prompt_assistant.provider_no_vision".into(),
            )),
        },
        VisionSupport::Supported | VisionSupport::Unknown => Ok(DescribeRoute::External {
            fallback_local_vision: local_vision,
        }),
    }
}

/// How much the description should cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    /// Two or three sentences: subject, setting, style.
    Short,
    /// One full paragraph.
    Detailed,
}

impl Length {
    /// Anything but `"short"` is the detailed paragraph, the useful default.
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("short") => Length::Short,
            _ => Length::Detailed,
        }
    }

    pub fn max_tokens(self) -> u32 {
        match self {
            Length::Short => 160,
            Length::Detailed => 420,
        }
    }
}

pub const SYSTEM: &str = "You describe images as prompts for a text-to-image model. \
You write plain prose that names what is visible, never guesses at what is not, and \
never comments on the image or the request. If no image is attached to the message, \
reply with exactly NO_IMAGE and nothing else.";

/// What a model that was sent no image answers (see [`SYSTEM`]): an external
/// model without vision, whose endpoint dropped the picture and kept the text.
pub const NO_IMAGE: &str = "NO_IMAGE";

/// Whether the answer says the model never saw the image.
pub fn saw_no_image(answer: &str) -> bool {
    answer
        .trim()
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        == NO_IMAGE
}

/// The instruction that goes with the image.
pub fn instruction(length: Length) -> &'static str {
    match length {
        Length::Short => {
            "Write a short prompt, two or three sentences, that would recreate this image: \
the main subject, the setting, and the art style or medium. Output only the prompt."
        }
        Length::Detailed => {
            "Write one detailed paragraph that would recreate this image as a text-to-image \
prompt. Cover the subject and their appearance, clothing, pose and expression, the setting, \
the lighting, the camera framing and angle, and the art style or medium. If the image has \
legible text, quote it exactly. Output only the prompt, with no preamble, list or heading."
        }
    }
}

/// Strip the wrapping a model adds around an answer it was told to give bare:
/// a label such as "Prompt:", surrounding quotes, or a code fence.
pub fn clean(raw: &str) -> String {
    let mut text = raw.trim();
    if let Some(inner) = text.strip_prefix("```").and_then(|t| t.strip_suffix("```")) {
        // Drop the fence's language tag, if any, with its line.
        text = inner
            .split_once('\n')
            .map_or(inner, |(first, rest)| {
                if first.trim().chars().all(|c| c.is_ascii_alphanumeric()) {
                    rest
                } else {
                    inner
                }
            })
            .trim();
    }
    for label in ["Prompt:", "prompt:", "PROMPT:", "Description:", "Caption:"] {
        if let Some(rest) = text.strip_prefix(label) {
            text = rest.trim_start();
            break;
        }
    }
    let unquoted = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    unquoted.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_defaults_to_detailed() {
        assert_eq!(Length::parse(Some("short")), Length::Short);
        assert_eq!(Length::parse(Some("detailed")), Length::Detailed);
        assert_eq!(Length::parse(None), Length::Detailed);
        assert_eq!(Length::parse(Some("weird")), Length::Detailed);
        assert!(Length::Short.max_tokens() < Length::Detailed.max_tokens());
    }

    #[test]
    fn clean_removes_labels_quotes_and_fences() {
        assert_eq!(clean("  A cat on a mat.  "), "A cat on a mat.");
        assert_eq!(clean("Prompt: A cat on a mat."), "A cat on a mat.");
        assert_eq!(clean("\"A cat on a mat.\""), "A cat on a mat.");
        assert_eq!(clean("```\nA cat on a mat.\n```"), "A cat on a mat.");
        assert_eq!(clean("```text\nA cat on a mat.\n```"), "A cat on a mat.");
        assert_eq!(clean("Caption: \"A cat.\""), "A cat.");
    }

    #[test]
    fn a_blind_model_is_recognised() {
        assert!(saw_no_image("NO_IMAGE"));
        assert!(saw_no_image(" `NO_IMAGE`. "));
        assert!(!saw_no_image("A cat on a mat."));
        assert!(!saw_no_image("NO_IMAGE of a cat"));
    }

    #[test]
    fn clean_keeps_quotes_inside_the_text() {
        assert_eq!(
            clean("A sign that reads \"OPEN\" hangs on the door."),
            "A sign that reads \"OPEN\" hangs on the door."
        );
    }

    #[test]
    fn route_prefers_installed_joycaption_when_external_is_text_only() {
        assert_eq!(
            plan_route(
                true,
                VisionSupport::Unsupported,
                Some("joycaption-beta-one".into())
            )
            .unwrap(),
            DescribeRoute::Local("joycaption-beta-one".into())
        );
        assert!(plan_route(true, VisionSupport::Unsupported, None)
            .unwrap_err()
            .to_string()
            .contains("provider_no_vision"));
        assert_eq!(
            plan_route(
                true,
                VisionSupport::Supported,
                Some("joycaption-beta-one".into())
            )
            .unwrap(),
            DescribeRoute::External {
                fallback_local_vision: Some("joycaption-beta-one".into()),
            }
        );
        assert_eq!(
            plan_route(
                false,
                VisionSupport::Unknown,
                Some("joycaption-beta-one".into())
            )
            .unwrap(),
            DescribeRoute::Local("joycaption-beta-one".into())
        );
        assert!(plan_route(false, VisionSupport::Unknown, None)
            .unwrap_err()
            .to_string()
            .contains("no_vision_model"));
    }
}
