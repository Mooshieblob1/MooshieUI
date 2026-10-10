//! From the user's shot list and the measured takes to exact timings: the
//! voice track layout, each line's window and each shot's span. Pure, so the
//! timing rules are tested here rather than discovered in a paid render.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::audio::{self, PlannedLine, TrackPlan};
use super::prompt::{ShotLine, ShotSpec};

/// Longest pause the shot list accepts before a line or as a silent shot.
pub const MAX_LEAD_SECONDS: f64 = 10.0;

/// One line of the script with the take chosen for it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LineInput {
    pub line_id: String,
    pub take_id: String,
    pub text: String,
    #[serde(default)]
    pub delivery: String,
}

/// One shot as the user describes it. `lead` is the pause before the shot's
/// line, or the whole length of a shot without one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShotInput {
    #[serde(default)]
    pub framing: String,
    #[serde(default)]
    pub action: String,
    pub line_id: Option<String>,
    #[serde(default)]
    pub lead: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    pub track: TrackPlan,
    /// Lines in track order, as the track builder lays them out.
    pub planned: Vec<PlannedLine>,
    /// Silence after the last line, trailing silent shots included.
    pub tail: f64,
    /// Whole seconds of video, at least the track's length.
    pub seconds: u32,
    pub shots: Vec<ShotSpec>,
}

/// `durations` maps a line id to its take's measured length in seconds.
pub fn build(
    shots: &[ShotInput],
    lines: &[LineInput],
    durations: &HashMap<String, f64>,
    tail: f64,
    min_seconds: u32,
    max_seconds: u32,
) -> Result<Timeline, String> {
    if shots.is_empty() {
        return Err("Add at least one shot.".into());
    }
    let by_id: HashMap<&str, &LineInput> = lines.iter().map(|l| (l.line_id.as_str(), l)).collect();
    let mut used = HashSet::new();
    let mut planned = Vec::new();
    let mut pending = 0.0;
    for (i, shot) in shots.iter().enumerate() {
        let n = i + 1;
        if !(shot.lead.is_finite() && (0.0..=MAX_LEAD_SECONDS).contains(&shot.lead)) {
            return Err(format!(
                "The pause in shot {n} must be 0 to {MAX_LEAD_SECONDS} seconds."
            ));
        }
        match &shot.line_id {
            Some(id) => {
                if !by_id.contains_key(id.as_str()) {
                    return Err(format!("Shot {n} uses a line that has no chosen take."));
                }
                if !used.insert(id.as_str()) {
                    return Err(format!(
                        "Shot {n} repeats a line another shot already uses."
                    ));
                }
                let duration = *durations
                    .get(id)
                    .ok_or_else(|| format!("The take for shot {n} could not be measured."))?;
                planned.push(PlannedLine {
                    line_id: id.clone(),
                    duration,
                    silence_before: pending + shot.lead,
                });
                pending = 0.0;
            }
            None => {
                if shot.lead <= 0.0 {
                    return Err(format!("Give shot {n} a length, since it has no line."));
                }
                pending += shot.lead;
            }
        }
    }
    let tail = pending + tail.clamp(0.0, MAX_LEAD_SECONDS);
    let track = audio::plan_track(&planned, tail)?;
    let seconds = (track.total - 1e-6).ceil().max(f64::from(min_seconds)) as u32;
    if seconds > max_seconds {
        return Err(format!(
            "The scene would be {seconds} seconds; the video model makes at most {max_seconds}. Shorten the pauses or drop a line."
        ));
    }

    let windows: HashMap<&str, (f64, f64)> = track
        .windows
        .iter()
        .map(|w| (w.line_id.as_str(), (w.start, w.end)))
        .collect();
    let mut cursor = 0.0;
    let mut specs = Vec::with_capacity(shots.len());
    for (i, shot) in shots.iter().enumerate() {
        let last = i + 1 == shots.len();
        let start = cursor;
        let (end, line) = match &shot.line_id {
            Some(id) => {
                let (line_start, line_end) = windows[id.as_str()];
                let input = by_id[id.as_str()];
                (
                    line_end,
                    Some(ShotLine {
                        text: input.text.clone(),
                        delivery: input.delivery.clone(),
                        start: line_start,
                        end: line_end,
                    }),
                )
            }
            None => (start + shot.lead, None),
        };
        let end = if last { f64::from(seconds) } else { end };
        specs.push(ShotSpec {
            start,
            end,
            framing: shot.framing.clone(),
            action: shot.action.clone(),
            line,
        });
        cursor = end;
    }
    Ok(Timeline {
        track,
        planned,
        tail,
        seconds,
        shots: specs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str) -> LineInput {
        LineInput {
            line_id: id.into(),
            take_id: format!("take-{id}"),
            text: format!("line {id}"),
            delivery: String::new(),
        }
    }

    fn shot(line_id: Option<&str>, lead: f64) -> ShotInput {
        ShotInput {
            framing: "Medium shot".into(),
            action: "She looks up".into(),
            line_id: line_id.map(str::to_string),
            lead,
        }
    }

    fn durations() -> HashMap<String, f64> {
        [("a", 7.8), ("b", 7.3), ("c", 3.9), ("d", 3.9)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn the_prototype_scene_lays_out() {
        let lines = ["a", "b", "c", "d"].map(line);
        let shots = [
            shot(None, 1.5),
            shot(Some("a"), 0.0),
            shot(Some("b"), 0.5),
            shot(Some("c"), 0.5),
            shot(Some("d"), 0.6),
        ];
        let t = build(&shots, &lines, &durations(), 1.98, 4, 30).unwrap();
        assert_eq!(t.seconds, 28);
        assert_eq!((t.shots[0].start, t.shots[0].end), (0.0, 1.5));
        assert_eq!((t.shots[1].start, t.shots[1].end), (1.5, 9.3));
        let first = t.shots[1].line.as_ref().unwrap();
        assert_eq!((first.start, first.end), (1.5, 9.3));
        // The next shot starts where the previous line ended and holds the pause.
        assert_eq!(t.shots[2].start, 9.3);
        assert_eq!(t.shots[2].line.as_ref().unwrap().start, 9.8);
        // The last shot runs to the end of the clip.
        assert_eq!(t.shots[4].end, 28.0);
        assert!(
            super::super::prompt::validate(&crate::cloud::scene::prompt::SceneSpec {
                seconds: t.seconds,
                aspect: "16:9".into(),
                continuous: false,
                character_name: "Aoi".into(),
                character_traits: String::new(),
                images: vec![],
                has_voice_track: true,
                language: String::new(),
                starting_state: String::new(),
                shots: t.shots.clone(),
                ambience: String::new(),
                ending_state: String::new(),
                minor: false,
            })
            .is_ok()
        );
    }

    #[test]
    fn silent_shots_push_the_next_line_back() {
        let lines = [line("a")];
        let shots = [shot(None, 1.0), shot(None, 1.0), shot(Some("a"), 0.5)];
        let t = build(&shots, &lines, &durations(), 0.0, 4, 30).unwrap();
        assert_eq!(t.planned[0].silence_before, 2.5);
        assert_eq!(t.shots[1].start, 1.0);
        assert_eq!(t.shots[1].end, 2.0);
        assert_eq!(t.shots[2].start, 2.0);
    }

    #[test]
    fn short_scenes_round_up_to_the_minimum() {
        let mut d = durations();
        d.insert("a".into(), 2.0);
        let t = build(&[shot(Some("a"), 0.0)], &[line("a")], &d, 0.0, 4, 30).unwrap();
        assert_eq!(t.seconds, 4);
        assert_eq!(t.shots[0].end, 4.0);
    }

    #[test]
    fn bad_shot_lists_are_refused() {
        let lines = [line("a"), line("b")];
        let d = durations();
        assert!(build(&[], &lines, &d, 0.0, 4, 30).is_err());
        assert!(build(
            &[shot(None, 0.0), shot(Some("a"), 0.0)],
            &lines,
            &d,
            0.0,
            4,
            30
        )
        .is_err());
        assert!(build(
            &[shot(Some("a"), 0.0), shot(Some("a"), 0.0)],
            &lines,
            &d,
            0.0,
            4,
            30
        )
        .is_err());
        assert!(build(&[shot(Some("zz"), 0.0)], &lines, &d, 0.0, 4, 30).is_err());
        assert!(build(&[shot(Some("a"), -1.0)], &lines, &d, 0.0, 4, 30).is_err());
        // No line at all means no voice track.
        assert!(build(&[shot(None, 5.0)], &lines, &d, 0.0, 4, 30).is_err());
        // 7.8 + 7.3 + 10 s of pauses + tail is over the model's 15 s.
        assert!(build(
            &[shot(Some("a"), 0.0), shot(Some("b"), 0.0)],
            &lines,
            &d,
            0.0,
            4,
            15
        )
        .is_err());
    }
}
