//! Seedance prompt builder. Follows `docs/SEEDANCE_PROMPTING_AGENTS.md`
//! (rules S1 to S15); the tests below assert those rules, since the frontend
//! has no test framework.
//!
//! Output shape (rule S1): Format, Reference roles, Starting state, timed
//! beats, Continuity, Audio, Ending state, Constraints. Sections that would be
//! empty are dropped.

use serde::{Deserialize, Serialize};

/// Bump when the emitted text changes, so a saved prompt says which builder
/// wrote it.
pub const BUILDER_VERSION: u32 = 1;

/// What one reference image controls (rule S5: one narrow job each).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageRole {
    /// The character's identity. Background and pose must not transfer.
    Character,
    /// The location. Contains no people.
    Location,
    /// Framing and expression for one shot (0-based index into `shots`).
    Shot { shot: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShotLine {
    pub text: String,
    pub delivery: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShotSpec {
    pub start: f64,
    pub end: f64,
    pub framing: String,
    pub action: String,
    pub line: Option<ShotLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneSpec {
    pub seconds: u32,
    pub aspect: String,
    /// One continuous take instead of several shots (rule S4).
    pub continuous: bool,
    pub character_name: String,
    /// Traits that must stay fixed: face, hair, outfit, accessories.
    pub character_traits: String,
    /// In upload order: the first is @Image1 (rule S5).
    pub images: Vec<ImageRole>,
    pub has_voice_track: bool,
    pub language: String,
    pub starting_state: String,
    pub shots: Vec<ShotSpec>,
    pub ambience: String,
    pub ending_state: String,
    /// Adds age-appropriate constraints (research doc, section 10).
    pub minor: bool,
}

pub const MIN_SECONDS: u32 = 4;
pub const MAX_SECONDS: u32 = 30;

pub fn validate(spec: &SceneSpec) -> Result<(), String> {
    if !(MIN_SECONDS..=MAX_SECONDS).contains(&spec.seconds) {
        return Err(format!(
            "A scene is {MIN_SECONDS} to {MAX_SECONDS} seconds long."
        ));
    }
    if spec.shots.is_empty() {
        return Err("Add at least one shot.".into());
    }
    let total = f64::from(spec.seconds);
    let mut previous_end = 0.0;
    for (i, shot) in spec.shots.iter().enumerate() {
        let n = i + 1;
        if shot.start < previous_end - 1e-6 || shot.end <= shot.start || shot.end > total + 1e-6 {
            return Err(format!(
                "Shot {n} must start after the previous shot and end within the scene."
            ));
        }
        if shot.framing.trim().is_empty() && shot.action.trim().is_empty() {
            return Err(format!("Describe what happens in shot {n}."));
        }
        if let Some(line) = &shot.line {
            if line.text.trim().is_empty() {
                return Err(format!("Shot {n} has an empty line."));
            }
            if line.end <= line.start {
                return Err(format!("The line in shot {n} has no length."));
            }
        }
        previous_end = shot.end;
    }
    for role in &spec.images {
        if let ImageRole::Shot { shot } = role {
            if *shot >= spec.shots.len() {
                return Err("A reference image points at a shot that does not exist.".into());
            }
        }
    }
    Ok(())
}

/// Seconds as the guide writes them: "1.5", "9", never "9.000".
fn secs(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{rounded:.1}")
    }
}

/// Dialogue goes in straight double quotes (rule S8), so the line itself may
/// not contain one.
fn quote_safe(text: &str) -> String {
    text.trim()
        .replace(['"', '\u{201C}', '\u{201D}'], "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn sentence(text: &str) -> String {
    let t = text.trim().trim_end_matches(['.', ' ']);
    if t.is_empty() {
        String::new()
    } else {
        format!("{t}.")
    }
}

pub fn build(spec: &SceneSpec) -> Result<String, String> {
    validate(spec)?;
    let name = {
        let n = spec.character_name.trim();
        if n.is_empty() {
            "the character".to_string()
        } else {
            n.to_string()
        }
    };
    let mut sections: Vec<String> = Vec::new();

    // Format (S1, S4, S15).
    let cuts = if spec.continuous {
        "Continuous single take."
    } else {
        "Several shots as described below, changing shot at each new time block."
    };
    sections.push(format!(
        "Format: {} seconds, {}, 2D Japanese TV anime look, cel shading, clean line art, hand-drawn character animation at normal speed. {cuts}",
        spec.seconds,
        spec.aspect.trim()
    ));

    // Reference roles (S5, S6).
    let mut roles: Vec<String> = Vec::new();
    for (i, role) in spec.images.iter().enumerate() {
        let tag = format!("@Image{}", i + 1);
        roles.push(match role {
            ImageRole::Character => {
                let traits = spec.character_traits.trim();
                if traits.is_empty() {
                    format!("{tag} controls only {name}'s identity; do not copy its background or pose.")
                } else {
                    format!("{tag} controls only {name}'s identity: {traits}; do not copy its background or pose.")
                }
            }
            ImageRole::Location => {
                format!("{tag} controls only the location; it contains no people.")
            }
            ImageRole::Shot { shot } => format!(
                "{tag} controls only the framing and expression of shot {}.",
                shot + 1
            ),
        });
    }
    if spec.has_voice_track {
        roles.push(format!(
            "@Audio1 is {name}'s voice and contains all of {name}'s lines in order."
        ));
    }
    if !roles.is_empty() {
        sections.push(format!("Reference roles: {}", roles.join(" ")));
    }

    // Starting state (S1).
    let start = sentence(&spec.starting_state);
    sections.push(format!(
        "Starting state: {}{}Mouth closed.",
        start,
        if start.is_empty() { "" } else { " " }
    ));

    // Timed beats (S2, S3, S8, S9).
    for (i, shot) in spec.shots.iter().enumerate() {
        let mut beat = format!("{}-{} seconds:", secs(shot.start), secs(shot.end));
        let framing = sentence(&shot.framing);
        let framing = match spec
            .images
            .iter()
            .position(|r| *r == ImageRole::Shot { shot: i })
        {
            Some(p) if !framing.is_empty() => {
                format!(
                    "{} framed like @Image{}.",
                    framing.trim_end_matches('.'),
                    p + 1
                )
            }
            Some(p) => format!("Framed like @Image{}.", p + 1),
            None => framing,
        };
        if !framing.is_empty() {
            beat.push(' ');
            beat.push_str(&framing);
        }
        let action = sentence(&shot.action);
        if !action.is_empty() {
            beat.push(' ');
            beat.push_str(&action);
        }
        match &shot.line {
            Some(line) => {
                let delivery = line.delivery.trim();
                beat.push_str(&format!(
                    " From {} to {} seconds {name} says: \"{}\"{}{} {name}'s mouth moves only during this line.",
                    secs(line.start),
                    secs(line.end),
                    quote_safe(&line.text),
                    if delivery.is_empty() { "" } else { " " },
                    sentence(delivery),
                ));
            }
            None => beat.push_str(&format!(" {name} does not speak; mouth closed.")),
        }
        sections.push(beat);
    }

    // Continuity.
    sections.push(format!(
        "Continuity: only {name} is on screen. Same face, hair, outfit and accessories in every shot. {name}'s mouth stays closed in every pause."
    ));

    // Audio (S9).
    let language = spec.language.trim();
    let ambience = spec.ambience.trim();
    let mut audio = if spec.has_voice_track {
        format!(
            "Audio: clean lip-synced {} speech in the voice of @Audio1",
            if language.is_empty() { "" } else { language }
        )
        .replace("  ", " ")
    } else {
        "Audio: no speech".to_string()
    };
    if !ambience.is_empty() {
        audio.push_str(&format!("; {}", ambience.trim_end_matches('.')));
    }
    audio.push_str("; no music.");
    sections.push(audio);

    // Ending state.
    let end = sentence(&spec.ending_state);
    if !end.is_empty() {
        sections.push(format!("Ending state: {end}"));
    }

    // Constraints last (S12).
    let mut constraints = vec![
        "no other characters",
        "no extra hands",
        "no subtitles or on-screen text",
        "no music",
        "no slow motion",
        "no repeated actions",
        "no morphing of face or outfit",
        "no 3D rendering",
    ];
    if spec.continuous {
        constraints.push("no cuts");
    }
    if spec.minor {
        constraints.extend([
            "age-appropriate everyday scene",
            "fully clothed",
            "nothing suggestive",
        ]);
    }
    sections.push(format!("Constraints: {}.", constraints.join(", ")));

    Ok(sections.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> SceneSpec {
        SceneSpec {
            seconds: 28,
            aspect: "16:9".into(),
            continuous: false,
            character_name: "Aoi".into(),
            character_traits: "short black hair, red ribbon, navy blazer".into(),
            images: vec![
                ImageRole::Character,
                ImageRole::Location,
                ImageRole::Shot { shot: 1 },
            ],
            has_voice_track: true,
            language: "Japanese".into(),
            starting_state: "Aoi stands by the classroom window, arms folded".into(),
            shots: vec![
                ShotSpec {
                    start: 0.0,
                    end: 1.5,
                    framing: "Wide establishing shot of the empty classroom".into(),
                    action: "Afternoon light through the window".into(),
                    line: None,
                },
                ShotSpec {
                    start: 1.5,
                    end: 9.5,
                    framing: "Medium shot".into(),
                    action: "Aoi turns toward the camera".into(),
                    line: Some(ShotLine {
                        text: "べ、別に…\"あなた\"のためじゃないから".into(),
                        delivery: "haughty, scoffing".into(),
                        start: 1.5,
                        end: 9.3,
                    }),
                },
            ],
            ambience: "quiet classroom ambience".into(),
            ending_state: "Aoi looks away, cheeks puffed".into(),
            minor: false,
        }
    }

    fn index(text: &str, needle: &str) -> usize {
        text.find(needle)
            .unwrap_or_else(|| panic!("missing {needle:?} in:\n{text}"))
    }

    #[test]
    fn sections_follow_the_guide_order() {
        let text = build(&spec()).unwrap();
        let order = [
            "Format:",
            "Reference roles:",
            "Starting state:",
            "0-1.5 seconds:",
            "1.5-9.5 seconds:",
            "Continuity:",
            "Audio:",
            "Ending state:",
            "Constraints:",
        ];
        let positions: Vec<usize> = order.iter().map(|n| index(&text, n)).collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{text}");
        assert!(text.trim_end().ends_with('.'));
        assert!(text.rfind("Constraints:").unwrap() > text.rfind("seconds:").unwrap());
    }

    #[test]
    fn every_reference_has_one_role() {
        let text = build(&spec()).unwrap();
        for tag in [
            "@Image1 controls only Aoi's identity",
            "@Image2 controls only the location",
            "@Image3 controls only the framing",
        ] {
            index(&text, tag);
        }
        index(&text, "@Audio1 is Aoi's voice");
        index(&text, "framed like @Image3");
        assert!(!text.contains("@Image4"));
    }

    #[test]
    fn lines_are_quoted_with_their_window() {
        let text = build(&spec()).unwrap();
        index(&text, "From 1.5 to 9.3 seconds Aoi says: \"");
        index(&text, "mouth moves only during this line");
        index(&text, "Aoi does not speak; mouth closed");
        // A double quote inside the line cannot end the quotation early.
        index(&text, "'あなた'");
        index(&text, "clean lip-synced Japanese speech");
    }

    #[test]
    fn a_minor_gets_age_appropriate_constraints() {
        let mut s = spec();
        assert!(!build(&s).unwrap().contains("nothing suggestive"));
        s.minor = true;
        let text = build(&s).unwrap();
        let constraints = &text[index(&text, "Constraints:")..];
        assert!(constraints.contains("fully clothed"));
        assert!(constraints.contains("nothing suggestive"));
    }

    #[test]
    fn a_continuous_take_says_so_twice() {
        let mut s = spec();
        s.continuous = true;
        let text = build(&s).unwrap();
        index(&text, "Continuous single take.");
        assert!(text[index(&text, "Constraints:")..].contains("no cuts"));
    }

    #[test]
    fn invalid_timelines_are_refused() {
        let mut s = spec();
        s.seconds = 31;
        assert!(build(&s).is_err());

        let mut s = spec();
        s.shots[1].start = 1.0; // overlaps shot 1
        assert!(build(&s).is_err());

        let mut s = spec();
        s.shots[1].end = 29.0; // past the end
        assert!(build(&s).is_err());

        let mut s = spec();
        s.images.push(ImageRole::Shot { shot: 9 });
        assert!(build(&s).is_err());

        let mut s = spec();
        s.shots.clear();
        assert!(build(&s).is_err());
    }

    #[test]
    fn seconds_print_like_the_guide() {
        assert_eq!(secs(1.5), "1.5");
        assert_eq!(secs(9.0), "9");
        assert_eq!(secs(9.04), "9");
        assert_eq!(secs(26.05), "26.1");
    }
}
