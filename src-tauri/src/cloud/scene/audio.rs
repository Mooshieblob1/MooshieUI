//! The scene's single voice track: chosen takes laid end to end with
//! silences, and the exact window each line occupies (research doc 5.4).
//!
//! The planning is pure and tested; the ffmpeg calls only execute the plan.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Seedance 2.5 limits for one audio reference (fal API page, 2026-10-11).
pub const AUDIO_MIN_SECONDS: f64 = 1.8;
pub const AUDIO_MAX_SECONDS: f64 = 30.2;
const SAMPLE_RATE: u32 = 44_100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LineWindow {
    pub line_id: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TrackPlan {
    pub windows: Vec<LineWindow>,
    pub total: f64,
}

/// One line in the track: its id, the take's measured duration, and the
/// silence placed *before* it (the first one is the lead-in, for example an
/// establishing shot with no dialogue).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PlannedLine {
    pub line_id: String,
    pub duration: f64,
    pub silence_before: f64,
}

/// Lay the lines out end to end. `tail` is silence after the last line.
pub fn plan_track(lines: &[PlannedLine], tail: f64) -> Result<TrackPlan, String> {
    if lines.is_empty() {
        return Err("The scene needs at least one recorded line.".into());
    }
    let mut cursor = 0.0;
    let mut windows = Vec::with_capacity(lines.len());
    for line in lines {
        if !(line.duration.is_finite() && line.duration > 0.0) {
            return Err("A take has no measurable length.".into());
        }
        let gap = line.silence_before.max(0.0);
        cursor += gap;
        let start = round_ms(cursor);
        cursor += line.duration;
        windows.push(LineWindow {
            line_id: line.line_id.clone(),
            start,
            end: round_ms(cursor),
        });
    }
    let total = round_ms(cursor + tail.max(0.0));
    if total < AUDIO_MIN_SECONDS {
        return Err(format!(
            "The voice track must be at least {AUDIO_MIN_SECONDS} seconds."
        ));
    }
    if total > AUDIO_MAX_SECONDS {
        return Err(format!(
            "The voice track is {total:.1} seconds; the video provider accepts at most {AUDIO_MAX_SECONDS}. Shorten the pauses or drop a line."
        ));
    }
    Ok(TrackPlan { windows, total })
}

fn round_ms(seconds: f64) -> f64 {
    (seconds * 1000.0).round() / 1000.0
}

/// The ffmpeg arguments that build the track: every take resampled to mono
/// 44.1 kHz, silences generated in between, all concatenated into one
/// 16-bit WAV. WAV rather than MP3 so no optional encoder is needed; a full
/// 30 s track is about 2.6 MB, well under the provider's 15 MB limit.
pub fn track_args(takes: &[(PathBuf, f64)], tail: f64, out: &Path) -> Vec<String> {
    let mut b = TrackArgs::default();
    for (path, silence_before) in takes {
        b.silence(*silence_before);
        b.take(path);
    }
    b.silence(tail);
    b.finish(out)
}

#[derive(Default)]
struct TrackArgs {
    inputs: Vec<String>,
    filter: String,
    labels: String,
    count: usize,
}

impl TrackArgs {
    fn silence(&mut self, seconds: f64) {
        if seconds <= 0.0 {
            return;
        }
        self.inputs.extend([
            "-f".into(),
            "lavfi".into(),
            "-t".into(),
            format!("{seconds:.3}"),
            "-i".into(),
            format!("anullsrc=r={SAMPLE_RATE}:cl=mono"),
        ]);
        self.segment("anull".into());
    }

    fn take(&mut self, path: &Path) {
        self.inputs
            .extend(["-i".into(), path.to_string_lossy().into_owned()]);
        self.segment(format!(
            "aresample={SAMPLE_RATE},aformat=sample_fmts=s16:channel_layouts=mono"
        ));
    }

    fn segment(&mut self, chain: String) {
        let n = self.count;
        self.filter.push_str(&format!("[{n}:a]{chain}[s{n}];"));
        self.labels.push_str(&format!("[s{n}]"));
        self.count += 1;
    }

    fn finish(self, out: &Path) -> Vec<String> {
        let mut args: Vec<String> = vec!["-hide_banner".into(), "-y".into()];
        args.extend(self.inputs);
        let filter = format!(
            "{}{}concat=n={}:v=0:a=1[out]",
            self.filter, self.labels, self.count
        );
        args.extend([
            "-filter_complex".into(),
            filter,
            "-map".into(),
            "[out]".into(),
            "-ar".into(),
            SAMPLE_RATE.to_string(),
            "-ac".into(),
            "1".into(),
            "-c:a".into(),
            "pcm_s16le".into(),
            out.to_string_lossy().into_owned(),
        ]);
        args
    }
}

/// Read `Duration: HH:MM:SS.ss` from ffmpeg's banner output.
pub fn parse_duration(stderr: &str) -> Option<f64> {
    let rest = stderr.split("Duration: ").nth(1)?;
    let stamp = rest.split(',').next()?.trim();
    let mut parts = stamp.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
}

/// Read the first video stream's size and frame rate from ffmpeg's banner.
pub fn parse_video_info(stderr: &str) -> Option<VideoInfo> {
    let duration = parse_duration(stderr).unwrap_or(0.0);
    let line = stderr
        .lines()
        .find(|l| l.contains("Stream #") && l.contains("Video:"))?;
    let (width, height) = line
        .split([',', ' '])
        .filter_map(|tok| {
            let (w, h) = tok.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .find(|(w, h)| *w > 0 && *h > 0)?;
    let fps = line
        .split(',')
        .map(str::trim)
        .find_map(|part| part.strip_suffix(" fps")?.trim().parse::<f64>().ok())
        .unwrap_or(24.0);
    Some(VideoInfo {
        width,
        height,
        fps,
        duration,
    })
}

/// Run ffmpeg and return its stderr. A non-zero exit is an error unless
/// `probe` is set: `ffmpeg -i file` with no output always exits 1, and its
/// banner is what the probe wants.
pub async fn run_ffmpeg(ffmpeg: &Path, args: &[String], probe: bool) -> Result<String, String> {
    let mut command = tokio::process::Command::new(ffmpeg);
    command.args(args).kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW, so no console flashes up on desktop.
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .await
        .map_err(|e| format!("Could not run FFmpeg: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !probe && !output.status.success() {
        let tail: String = stderr
            .lines()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(format!("FFmpeg failed: {tail}"));
    }
    Ok(stderr)
}

pub async fn probe_duration(ffmpeg: &Path, file: &Path) -> Result<f64, String> {
    let stderr = run_ffmpeg(
        ffmpeg,
        &[
            "-hide_banner".into(),
            "-i".into(),
            file.to_string_lossy().into_owned(),
        ],
        true,
    )
    .await?;
    parse_duration(&stderr).ok_or_else(|| "Could not read a take's length.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, duration: f64, silence_before: f64) -> PlannedLine {
        PlannedLine {
            line_id: id.into(),
            duration,
            silence_before,
        }
    }

    #[test]
    fn the_prototype_layout_reproduces() {
        // Four lines with a 1.5 s lead-in, as in the prototype run.
        let plan = plan_track(
            &[
                line("a", 7.8, 1.5),
                line("b", 7.3, 0.5),
                line("c", 3.9, 0.5),
                line("d", 3.9, 0.6),
            ],
            1.98,
        )
        .unwrap();
        assert_eq!(plan.windows[0].start, 1.5);
        assert_eq!(plan.windows[0].end, 9.3);
        assert_eq!(plan.windows[1].start, 9.8);
        assert_eq!(plan.windows[3].end, 26.0);
        assert!((plan.total - 27.98).abs() < 1e-9);
    }

    #[test]
    fn provider_limits_are_enforced() {
        assert!(plan_track(&[], 0.0).is_err());
        assert!(plan_track(&[line("a", 1.0, 0.0)], 0.0).is_err());
        assert!(plan_track(&[line("a", 20.0, 0.0), line("b", 11.0, 0.0)], 0.0).is_err());
        assert!(plan_track(&[line("a", 0.0, 0.0)], 2.0).is_err());
    }

    #[test]
    fn the_track_command_concatenates_silences_and_takes() {
        let args = track_args(
            &[
                (PathBuf::from("/t/a.mp3"), 1.5),
                (PathBuf::from("/t/b.mp3"), 0.5),
            ],
            1.0,
            Path::new("/t/out.wav"),
        );
        let filter = &args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1];
        // silence, take, silence, take, tail
        assert!(filter.ends_with("concat=n=5:v=0:a=1[out]"), "{filter}");
        assert_eq!(args.iter().filter(|a| *a == "lavfi").count(), 3);
        assert_eq!(args.last().unwrap(), "/t/out.wav");
    }

    #[test]
    fn ffmpeg_banners_are_read() {
        let banner = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'x.mp4':\n  Duration: 00:00:28.04, start: 0.000000, bitrate: 1200 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709), 864x496 [SAR 1:1 DAR 54:31], 1100 kb/s, 24 fps, 24 tbr, 12288 tbn (default)\n  Stream #0:1[0x2](und): Audio: aac (LC), 44100 Hz, mono, fltp, 69 kb/s";
        assert_eq!(parse_duration(banner), Some(28.04));
        let info = parse_video_info(banner).unwrap();
        assert_eq!((info.width, info.height), (864, 496));
        assert_eq!(info.fps, 24.0);
        assert!(parse_video_info("Duration: 00:00:01.00, start").is_none());
    }
}
