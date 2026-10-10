//! The mouth map (rule S10): when the character's mouth moves, from
//! ElevenLabs forced alignment of each take. Each take is aligned once and
//! the result cached next to it, so the map costs nothing to rebuild.

use serde::{Deserialize, Serialize};

use crate::cloud::elevenlabs::AlignedUnit;

/// Bump when the merge rules below change, so old caches are measured again.
pub const MOUTH_VERSION: u32 = 1;

/// Pauses shorter than this are part of the same talking stretch: the
/// mouth does not visibly close between syllables.
const MERGE_GAP: f64 = 0.3;
/// Stretches shorter than this are dropped as alignment noise.
const MIN_SPAN: f64 = 0.15;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

/// What is cached per take.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MouthMap {
    pub version: u32,
    /// Talking stretches, in seconds from the start of the take.
    pub spans: Vec<Span>,
}

/// Units that make a sound: punctuation and spaces move no mouth.
fn voiced(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

/// Merge aligned units into talking stretches.
pub fn spans_from(units: &[AlignedUnit]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut voiced_units: Vec<&AlignedUnit> = units.iter().filter(|u| voiced(&u.text)).collect();
    voiced_units.sort_by(|a, b| a.start.total_cmp(&b.start));
    for unit in voiced_units {
        match spans.last_mut() {
            Some(last) if unit.start - last.end < MERGE_GAP => {
                last.end = last.end.max(unit.end);
            }
            _ => spans.push(Span {
                start: unit.start,
                end: unit.end,
            }),
        }
    }
    spans.retain(|s| s.end - s.start >= MIN_SPAN);
    spans
}

/// The take's stretches placed in the scene: shifted to where the line
/// starts and cut at the end of its window.
pub fn place(spans: &[Span], line_start: f64, line_end: f64) -> Vec<Span> {
    spans
        .iter()
        .map(|s| Span {
            start: line_start + s.start,
            end: (line_start + s.end).min(line_end),
        })
        .filter(|s| s.end - s.start >= MIN_SPAN)
        .collect()
}

/// The text ElevenLabs actually spoke: delivery notes are sent as bracketed
/// audio tags (rule E1), which are not spoken and must not be aligned.
pub fn spoken_only(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(text: &str, start: f64, end: f64) -> AlignedUnit {
        AlignedUnit {
            text: text.into(),
            start,
            end,
        }
    }

    #[test]
    fn syllables_merge_and_real_pauses_split() {
        let units = [
            unit("べ", 0.20, 0.30),
            unit("つ", 0.32, 0.45),
            unit("、", 0.45, 0.90),
            unit("に", 1.00, 1.10),
            unit("ち", 1.12, 1.30),
            unit(" ", 1.30, 1.35),
            unit("x", 3.00, 3.05),
        ];
        let spans = spans_from(&units);
        assert_eq!(
            spans,
            vec![
                Span {
                    start: 0.20,
                    end: 0.45
                },
                Span {
                    start: 1.00,
                    end: 1.30
                },
            ]
        );
    }

    #[test]
    fn placing_shifts_and_clips_to_the_line() {
        let spans = [
            Span {
                start: 0.2,
                end: 1.0,
            },
            Span {
                start: 1.5,
                end: 2.5,
            },
        ];
        let placed = place(&spans, 10.0, 12.0);
        assert_eq!(placed[0].start, 10.2);
        assert_eq!(placed[1].end, 12.0);
        assert!(place(&spans, 10.0, 10.1).is_empty());
    }

    #[test]
    fn delivery_tags_are_not_aligned() {
        assert_eq!(
            spoken_only("[annoyed] べつに、 [sigh] いいけど"),
            "べつに、 いいけど"
        );
        assert_eq!(spoken_only("plain"), "plain");
    }
}
