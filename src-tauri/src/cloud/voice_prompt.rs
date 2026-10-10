//! Voice Design description builder, following ElevenLabs' Voice Design
//! prompting guide (`docs/research/anime-scene-pipeline.md`, rule E6):
//! language and region first; then gender, age and voice quality; a short
//! persona with two or three emotions; then one or two sentences on timbre,
//! pacing and delivery. Effect words (reverb, echo, phone, tape) are dropped.
//!
//! The builder only assembles what the user wrote. It never adds, changes or
//! removes an age, so a refusal from ElevenLabs is about what the user
//! actually described (rule E7: for a character flagged as a minor, that
//! refusal is final).

use serde::{Deserialize, Serialize};

use super::elevenlabs::{DESCRIPTION_MAX, DESCRIPTION_MIN};

/// Bump when the output format changes, so a saved draft can say which
/// builder wrote it.
pub const BUILDER_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VoiceBrief {
    /// For example "Japanese, Tokyo standard".
    pub language: String,
    pub gender: String,
    pub age: String,
    /// For example "clear, bright".
    pub quality: String,
    /// Two to five words.
    pub persona: String,
    /// Two or three emotions.
    pub emotions: String,
    /// One or two sentences on timbre, pacing and delivery.
    pub delivery: String,
}

/// Words that describe recording effects rather than a voice. Voice Design
/// tries to reproduce them, which degrades the voice.
const EFFECT_WORDS: [&str; 6] = ["reverb", "echo", "phone", "telephone", "tape", "radio"];

pub fn build_description(brief: &VoiceBrief) -> Result<String, String> {
    let clean = |s: &str| strip_effect_words(s.trim());
    let mut parts: Vec<String> = Vec::new();
    let language = clean(&brief.language);
    if !language.is_empty() {
        parts.push(format!("{language} speaker."));
    }
    let who: Vec<String> = [&brief.gender, &brief.age, &brief.quality]
        .iter()
        .map(|s| clean(s))
        .filter(|s| !s.is_empty())
        .collect();
    if !who.is_empty() {
        parts.push(format!("{}.", who.join(", ")));
    }
    let persona = clean(&brief.persona);
    let emotions = clean(&brief.emotions);
    match (persona.is_empty(), emotions.is_empty()) {
        (false, false) => parts.push(format!("{persona}, sounding {emotions}.")),
        (false, true) => parts.push(format!("{persona}.")),
        (true, false) => parts.push(format!("Sounds {emotions}.")),
        (true, true) => {}
    }
    let delivery = clean(&brief.delivery);
    if !delivery.is_empty() {
        let end = if delivery.ends_with(['.', '!', '?']) {
            ""
        } else {
            "."
        };
        parts.push(format!("{delivery}{end}"));
    }
    let description = parts
        .join(" ")
        .replace("..", ".")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let len = description.chars().count();
    if len < DESCRIPTION_MIN {
        return Err(format!(
            "The voice description needs at least {DESCRIPTION_MIN} characters."
        ));
    }
    if len > DESCRIPTION_MAX {
        return Err(format!(
            "The voice description can be at most {DESCRIPTION_MAX} characters."
        ));
    }
    Ok(description)
}

fn strip_effect_words(text: &str) -> String {
    text.split_whitespace()
        .filter(|word| {
            let bare: String = word
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase();
            !EFFECT_WORDS.contains(&bare.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief() -> VoiceBrief {
        VoiceBrief {
            language: "Japanese, Tokyo standard".into(),
            gender: "female".into(),
            age: "adult".into(),
            quality: "clear, bright".into(),
            persona: "proud tsundere student council president".into(),
            emotions: "haughty, flustered".into(),
            delivery: "Quick, clipped pacing that softens when embarrassed".into(),
        }
    }

    #[test]
    fn sections_come_in_the_guide_order() {
        let text = build_description(&brief()).unwrap();
        let language = text.find("Japanese").unwrap();
        let gender = text.find("female").unwrap();
        let persona = text.find("tsundere").unwrap();
        let delivery = text.find("Quick").unwrap();
        assert!(language < gender && gender < persona && persona < delivery);
        assert!(text.ends_with('.'));
    }

    #[test]
    fn effect_words_are_dropped() {
        let mut b = brief();
        b.delivery = "Warm, close voice with reverb and a phone filter, like old tape".into();
        let text = build_description(&b).unwrap();
        for word in ["reverb", "phone", "tape"] {
            assert!(!text.to_lowercase().contains(word), "{word} in {text}");
        }
    }

    #[test]
    fn the_age_is_passed_through_untouched() {
        // The builder never rewrites an age to get past a provider filter.
        let mut b = brief();
        b.age = "13 years old".into();
        assert!(build_description(&b).unwrap().contains("13 years old"));
    }

    #[test]
    fn length_limits_are_enforced() {
        assert!(build_description(&VoiceBrief {
            gender: "male".into(),
            ..Default::default()
        })
        .is_err());
        let mut b = brief();
        b.delivery = "very ".repeat(300);
        assert!(build_description(&b).is_err());
    }
}
