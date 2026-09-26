//! Animated AVIF / WebP / GIF and re-encoded MP4 export from gallery videos.
//!
//! The pure functions in this module are the canonical implementation of the
//! export math. `src/lib/utils/videoExport.ts` mirrors them so the popover can
//! compute values synchronously without an IPC round-trip per slider move; the
//! two are kept in sync by hand and by these tests.

/// Frame rates below this are not offered, however cleanly they divide.
pub const MIN_OFFERED_FPS: u32 = 6;

/// `auto` loop mode trims the duplicate frame below this measured seam delta.
pub const AUTO_SEAM_THRESHOLD: f32 = 2.0;

/// Default crossfade length for a clip of `f` frames.
///
/// Proportional to the clip (~8%) rather than a flat constant: a short clip
/// does not lose a disproportionate slice of itself to the crossfade, while a
/// long one still gets a perceptible blend. Clamped to the slider's 1-16
/// range, and never exceeds what `crossfade_available` allows for `f` (0 when
/// no crossfade fits at all).
///
/// Mirrored by `defaultCrossfadeFrames` in `src/lib/utils/videoExport.ts`.
pub fn default_crossfade_frames(f: u32) -> u32 {
    // Largest n for which crossfade_available(f, n) holds, i.e. f > 3n.
    let max_n = f.saturating_sub(1) / 3;
    if max_n == 0 {
        return 0;
    }
    let proportional = ((f as f64) * 0.08).round() as u32;
    proportional.clamp(1, max_n.min(16))
}

/// Every integer divisor of `n`, largest first.
pub fn divisors(n: u32) -> Vec<u32> {
    if n == 0 {
        return Vec::new();
    }
    (1..=n).rev().filter(|d| n.is_multiple_of(*d)).collect()
}

/// Frame rates the export picker offers for a clip at `source_fps`.
///
/// Only integer divisors resample cleanly - the `fps` filter dropping 24 to a
/// non-divisor rate discards frames on an uneven cadence, which visibly
/// judders. Rates above the source are absent, not clamped and not greyed out.
pub fn offered_fps(source_fps: u32) -> Vec<u32> {
    if source_fps == 0 {
        return vec![MIN_OFFERED_FPS];
    }
    let offered: Vec<u32> = divisors(source_fps)
        .into_iter()
        .filter(|d| *d >= MIN_OFFERED_FPS)
        .collect();
    if offered.is_empty() {
        // A source below the floor has no legal divisor; offer it as-is rather
        // than rendering an empty picker.
        vec![source_fps]
    } else {
        offered
    }
}

/// Resolve a preset's *target* fps against what this source can actually
/// deliver: the highest offered value at or below the target.
pub fn snap_fps(target: u32, source_fps: u32) -> u32 {
    let offered = offered_fps(source_fps);
    offered
        .iter()
        .copied()
        .find(|v| *v <= target)
        .unwrap_or_else(|| offered.last().copied().unwrap_or(MIN_OFFERED_FPS))
}

/// Output dimensions for a requested width: never upscaled past the source,
/// width snapped down to even first, then height derived from that snapped
/// width and rounded to the nearest even number (not down, so the aspect ratio
/// is preserved as faithfully as possible within the 2px grid).
///
/// The height rounds to NEAREST rather than DOWN because rounding down can
/// accumulate a visible aspect-ratio error; the TS mirror task must copy this
/// same rule.
pub fn output_dimensions(src_w: u32, src_h: u32, requested_w: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (0, 0);
    }
    // Snap width down to even and never upscale past the source.
    let w = (requested_w.min(src_w).max(2)) & !1u32;
    let h_exact = w as f64 * src_h as f64 / src_w as f64;
    // Round height to the nearest even number; .max(2) guards a degenerate
    // very-wide-panorama case.
    let h = (((h_exact / 2.0).round() as u32) * 2).max(2);
    (w, h)
}

/// Whether a clip of `f` frames can crossfade over `n` frames without folding
/// into itself.
pub fn crossfade_available(f: u32, n: u32) -> bool {
    n > 0 && f > 3 * n
}

/// Frames the encoder will actually write for a given loop mode.
///
/// Mirrored by `outputFrameCount` in `src/lib/utils/videoExport.ts`.
pub fn output_frame_count(mode: &str, f: u32, n: u32) -> u32 {
    match mode {
        "trim" => f.saturating_sub(1).max(1),
        "crossfade" => {
            if crossfade_available(f, n) {
                f - n
            } else {
                f
            }
        }
        "pingpong" => {
            if f < 3 {
                f
            } else {
                2 * f - 2
            }
        }
        // "none" and anything unrecognised encode the source verbatim.
        _ => f,
    }
}

/// What `auto` resolves to for a measured seam delta.
///
/// Under the threshold the ends already match, so the only defect left is the
/// duplicate frame - trim it. Above it, trimming would drop a frame that is
/// carrying real motion, so leave the clip alone.
pub fn resolve_auto(seam_delta: f32) -> &'static str {
    if seam_delta < AUTO_SEAM_THRESHOLD {
        "trim"
    } else {
        "none"
    }
}

/// On-disk extension for an export format.
///
/// AVIF is the default arm: it is the recommended format, so an unrecognised
/// string resolves to it rather than to the largest output.
pub fn ext_for(format: &str) -> &'static str {
    match format {
        "gif" => "gif",
        "webp" => "webp",
        "mp4" => "mp4",
        _ => "avif",
    }
}

/// Map the UI's 0-100 higher-is-better quality onto libx264's CRF, where lower
/// is better.
///
/// The UI never shows CRF. `quality` already means an AV1 quality for AVIF and a
/// libwebp quality for WebP, both 0-100 higher-is-better; giving MP4 a fourth,
/// inverted meaning would be worse than mapping it here. 0 lands on CRF 34
/// (visibly soft but tiny), 100 on CRF 14 (effectively transparent).
pub fn quality_to_crf(quality: u32) -> u32 {
    let q = quality.min(100) as f32;
    (34.0 - q * 0.2).round() as u32
}

/// Whether an export keeps the source clip's audio track.
///
/// MP4 is the only format with an audio track at all. Ping-pong is the only loop
/// mode that cannot keep one: it doubles the frame list and plays the second half
/// in reverse, which no audio stream can follow. Every other mode either keeps
/// the frames verbatim or truncates them, and the audio truncates to match.
///
/// `auto` resolves to `trim` or `none` at encode time - both keep audio - so it
/// counts as audio-capable here.
pub fn supports_audio(format: &str, loop_mode: &str) -> bool {
    format == "mp4" && loop_mode != "pingpong"
}

/// Whether a produced file exceeds the selected platform's attachment limit.
/// An unrecognised or absent target never produces a hint.
pub fn over_size_limit(bytes: u64, target: &str) -> bool {
    match target {
        "discord" => bytes > 20 * 1024 * 1024,
        "nitro" => bytes > 500 * 1024 * 1024,
        _ => false,
    }
}

use crate::error::AppError;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(feature = "desktop")]
use std::sync::Arc;
#[cfg(feature = "desktop")]
use tauri::{Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Piped to the venv's python on stdin. Nothing to install, nothing to version,
/// nothing to go stale.
const EXPORT_SCRIPT: &str = include_str!("video_export.py");

#[derive(Debug, Serialize, Deserialize)]
pub struct VideoExportResult {
    pub path: String,
    pub size_bytes: u64,
    pub frame_count: u32,
    /// 0-100, measured on the source frames after fps resampling.
    pub seam_delta: f32,
    /// What `auto` resolved to; echoes the request for the other modes.
    pub applied_loop_mode: String,
    /// Whether an audio track actually landed in the file. Asking for audio is
    /// not the same as getting it - a source with no audio track, or one the mp4
    /// container will not hold, degrades to a silent export rather than failing,
    /// and the popover says so instead of leaving the user to find out on
    /// playback.
    #[serde(default)]
    pub has_audio: bool,
}

#[derive(Debug, Serialize)]
pub struct ExportCapability {
    pub available: bool,
    pub reason: Option<String>,
    /// Whether this venv's PyAV was built with an H.264 encoder. Everything the
    /// animated formats need ships with PyAV itself, but libx264 is a separate
    /// ffmpeg build option, so the MP4 tab has to be gated on it independently.
    pub mp4: bool,
}

/// Probed with `python -c`. Written as a real multi-line program rather than a
/// semicolon one-liner because the libx264 check needs a `try` block: PyAV
/// raises rather than returning `None` when a codec is absent.
const PROBE_SCRIPT: &str = concat!(
    "import av, numpy, PIL\n",
    "try:\n",
    "    av.codec.Codec('libx264', 'w')\n",
    "    mp4 = 1\n",
    "except Exception:\n",
    "    mp4 = 0\n",
    "print('ok', mp4)\n",
);

/// Where exports land before Save as / Copy act on them. Swept at app start.
pub fn export_temp_dir() -> PathBuf {
    std::env::temp_dir().join("mooshie-export")
}

/// The export directory for one caller. Desktop and the browser-mode admin
/// (`None`) use the root export dir; a LAN user gets `users/{name}/` under it,
/// so export names (which are deterministic) cannot be fetched across users.
/// `None` when the name cannot safely become one path component.
pub fn export_dir_for(username: Option<&str>) -> Option<PathBuf> {
    let base = export_temp_dir();
    match username {
        None => Some(base),
        Some(name) => {
            let safe = name.to_ascii_lowercase();
            crate::commands::api::is_single_safe_filename(&safe)
                .then(|| base.join("users").join(safe))
        }
    }
}

/// Create `dir` (the export root or a directory under it) and every missing
/// level between, each private to the current user, and refuse to use any
/// level that is not.
///
/// The export root lives in the shared system temp dir under a fixed name,
/// so on a multi-user machine someone else can create it first: as a symlink
/// steering our writes elsewhere, or as a directory they own and can read
/// every export from. Each level must be a real directory (never a symlink)
/// owned by us, and on Unix it is created 0700 and tightened to 0700 if an
/// earlier version left it wider.
pub fn ensure_private_export_dir(root: &Path, dir: &Path) -> std::io::Result<()> {
    let rel = dir.strip_prefix(root).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "export directory is outside the export root",
        )
    })?;
    // Vet the whole subpath before creating anything.
    let parts = rel
        .components()
        .map(|component| match component {
            std::path::Component::Normal(part) => Ok(part),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "export directory is not a plain subpath",
            )),
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut current = root.to_path_buf();
    ensure_private_dir(&current)?;
    for part in parts {
        current.push(part);
        ensure_private_dir(&current)?;
    }
    Ok(())
}

/// Create one directory level 0700 if it is missing, then vet it.
fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    if let Err(e) = std::fs::symlink_metadata(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e);
        }
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        // Not `create_dir_all`: each level is created, and vetted, on its own.
        match builder.create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    check_private_dir(path)
}

/// A directory is private when it is a real directory, not a symlink, owned by
/// the current user. On Unix any group or other permission bits are removed.
fn check_private_dir(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is not a plain directory", path.display()),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.uid() != current_uid()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{} is owned by another user", path.display()),
            ));
        }
        if meta.mode() & 0o077 != 0 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// The effective uid of this process, without a libc dependency: the owner
/// of a file this process has just created exclusively (`O_EXCL`, random
/// name), read back through its own handle so nothing can swap it underneath.
#[cfg(unix)]
fn current_uid() -> std::io::Result<u32> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    static UID: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    if let Some(uid) = UID.get() {
        return Ok(*uid);
    }
    let probe = std::env::temp_dir().join(format!(".mooshie-uid-{}", uuid::Uuid::new_v4()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&probe)?;
    let meta = file.metadata();
    drop(file);
    let _ = std::fs::remove_file(&probe);
    let uid = meta?.uid();
    Ok(*UID.get_or_init(|| uid))
}

/// The loop modes the encoder implements (see `apply_loop_mode` in
/// `video_export.py`). The mode also lands in the output filename, so an
/// unrecognised value falls back to the default instead of reaching the path.
pub fn normalize_loop_mode(mode: &str) -> &'static str {
    match mode {
        "none" => "none",
        "trim" => "trim",
        "crossfade" => "crossfade",
        "pingpong" => "pingpong",
        _ => "auto",
    }
}

/// The output filename for an export. Every piece is either a number, a
/// whitelisted loop mode, a fixed extension, or the source file's own stem.
fn export_file_name(stem: &str, out_w: u32, fps: u32, loop_mode: &str, ext: &str) -> String {
    format!("{stem}_{out_w}w_{fps}fps_{loop_mode}.{ext}")
}

/// Delete last session's exports. Called once from setup; failures are logged
/// and ignored, because a stale temp file is not worth blocking startup over.
pub fn sweep_export_temp_dir() {
    let dir = export_temp_dir();
    if !dir.exists() {
        return;
    }
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        log::warn!("[export] could not sweep {}: {e}", dir.display());
    }
}

/// Resolve the venv's python, or explain exactly what is missing.
async fn resolve_python(state: &AppState) -> Result<PathBuf, AppError> {
    let venv_path = {
        let cfg = state.config.read().await;
        cfg.venv_path.clone()
    };
    if venv_path.trim().is_empty() {
        return Err(AppError::Other(
            "No Python environment is configured. Set one up in Settings to enable export.".into(),
        ));
    }
    let python = crate::commands::api::resolve_venv_python_bin(&venv_path);
    if !python.exists() {
        return Err(AppError::Other(format!(
            "Python not found at {}. Reinstall or repoint the environment in Settings.",
            python.display()
        )));
    }
    Ok(python)
}

/// One-time capability check: does the venv's python have what the script
/// imports? Cheap enough to run on popover open, so the buttons can disable
/// before the click rather than failing after it.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn probe_video_export(
    state: State<'_, Arc<AppState>>,
) -> Result<ExportCapability, AppError> {
    Ok(probe_export_inner(&state).await)
}

pub(crate) async fn probe_export_inner(state: &AppState) -> ExportCapability {
    let python = match resolve_python(state).await {
        Ok(p) => p,
        Err(e) => {
            return ExportCapability {
                available: false,
                reason: Some(e.to_string()),
                mp4: false,
            }
        }
    };
    let mut cmd = crate::comfyui::process::tokio_command_no_window(&python);
    cmd.arg("-c")
        .arg(PROBE_SCRIPT)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    match cmd.output().await {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // "ok 1" / "ok 0". A missing or unreadable flag means no MP4 tab,
            // never a broken export offered as if it worked.
            let mp4 = stdout
                .split_whitespace()
                .last()
                .map(|t| t == "1")
                .unwrap_or(false);
            ExportCapability {
                available: true,
                reason: None,
                mp4,
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let last = stderr.lines().last().unwrap_or("unknown import failure");
            ExportCapability {
                available: false,
                reason: Some(last.to_string()),
                mp4: false,
            }
        }
        Err(e) => ExportCapability {
            available: false,
            reason: Some(e.to_string()),
            mp4: false,
        },
    }
}

/// The source clip's generation parameters, re-serialised as SwarmUI JSON, or
/// an empty string when it has none.
///
/// Read from the file rather than the gallery SQLite row on purpose: one source
/// of truth, it works on videos generated before metadata shipped, and the
/// reader has to exist for drag-and-drop anyway. A column would be faster and
/// would create two places that can disagree.
///
/// The source is a gallery video of up to a few gigabytes, so this goes
/// through the streaming reader (box headers plus the small metadata boxes)
/// rather than loading the file. Blocking I/O: `run_export` calls it on a
/// blocking thread.
fn source_metadata_json(source: &Path) -> String {
    crate::metadata::read_file_metadata(source)
        .map(|params| crate::metadata::format_swarmui_json(&params))
        .unwrap_or_default()
}

/// Shared by the Tauri command and the browser-mode dispatch arm. `app` is
/// `None` in browser mode, where progress goes out over SSE only, and absent
/// entirely from the server build, which has no Tauri to emit to.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_export(
    #[cfg(feature = "desktop")] app: Option<&tauri::AppHandle>,
    state: &AppState,
    username: Option<&str>,
    source: &Path,
    format: &str,
    fps: u32,
    width: u32,
    quality: u32,
    loop_count: u32,
    loop_mode: &str,
    crossfade_frames: u32,
    keep_audio: bool,
) -> Result<VideoExportResult, AppError> {
    if !source.exists() {
        return Err(AppError::Other(
            "That video is no longer in the gallery.".into(),
        ));
    }
    // Whitelisted up front: the mode is echoed into the output filename and
    // the job, and REST callers send it unvalidated.
    let loop_mode = normalize_loop_mode(loop_mode);
    let python = resolve_python(state).await?;

    // Source dimensions come from the gallery index; if the row is missing we
    // let the requested width through unchanged and let the decoder scale.
    let (src_w, src_h) = source_dimensions(source);
    let (out_w, out_h) = if src_w > 0 {
        output_dimensions(src_w, src_h, width)
    } else {
        (width & !1, 0)
    };

    // Every other mirrored function is re-derived from the raw request below,
    // so a drifted `videoExport.ts` only mislabels the popover. `fps` was the
    // exception: the picker chooses it and it reached the encoder verbatim, and
    // a rate that does not divide the source resamples on an uneven cadence and
    // visibly judders. Snapping it here makes Rust authoritative for the fps
    // math too. A correct request passes through untouched.
    let fps = match source_fps(source) {
        Some(src_fps) => {
            let snapped = snap_fps(fps, src_fps);
            if snapped != fps {
                log::warn!(
                    "[export] {fps} fps does not divide the {src_fps} fps source; encoding at {snapped}"
                );
            }
            snapped
        }
        // Nothing to check against - no indexed rate for this clip. Let the
        // request through rather than guessing at a source rate.
        None => fps,
    };

    let dir = export_dir_for(username)
        .ok_or_else(|| AppError::Other("Invalid export directory".into()))?;
    ensure_private_export_dir(&export_temp_dir(), &dir)
        .map_err(|e| AppError::Other(format!("Export directory is unusable: {e}")))?;
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".into());
    let ext = ext_for(format);
    // Deterministic: re-exporting the same settings overwrites rather than
    // piling temp files up. The export temp dir is never the gallery dir, so an
    // MP4 export cannot collide with the mp4 it is reading.
    let out_name = export_file_name(&stem, out_w, fps, loop_mode, ext);
    let out_path = dir.join(&out_name);
    // Belt and braces: the name must land as a direct child of `dir`, never
    // somewhere a separator or drive prefix in it would steer the write.
    if out_path.file_name() != Some(std::ffi::OsStr::new(&out_name))
        || out_path.parent() != Some(dir.as_path())
    {
        return Err(AppError::Other("Invalid export filename".into()));
    }

    // Audio only survives on MP4, and not under ping-pong; asking for it anywhere
    // else is silently ignored rather than treated as an error, so the popover
    // can leave the toggle's state alone when the user switches formats.
    let keep_audio = keep_audio && supports_audio(ext, loop_mode);

    let metadata_json = {
        let source = source.to_path_buf();
        tokio::task::spawn_blocking(move || source_metadata_json(&source))
            .await
            .unwrap_or_default()
    };
    let job = serde_json::json!({
        "source": source.to_string_lossy(),
        "out": out_path.to_string_lossy(),
        "format": ext,
        "fps": fps,
        "width": out_w,
        "height": out_h,
        "quality": quality,
        "crf": quality_to_crf(quality),
        "keep_audio": keep_audio,
        "loop_count": loop_count,
        "loop_mode": loop_mode,
        "crossfade_frames": crossfade_frames,
        "auto_threshold": AUTO_SEAM_THRESHOLD,
        "metadata_json": metadata_json,
    });

    let mut cmd = crate::comfyui::process::tokio_command_no_window(&python);
    cmd.arg("-")
        .arg(job.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Other(format!("Could not start Python: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(EXPORT_SCRIPT.as_bytes()).await?;
        stdin.shutdown().await?;
        drop(stdin);
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Other("Python produced no stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Other("Python produced no stderr".into()))?;

    // Drain stderr concurrently so a chatty failure cannot deadlock the pipe.
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut collected = Vec::new();
        while let Ok(Some(line)) = lines.next_line().await {
            collected.push(line);
        }
        collected
    });

    let mut result: Option<VideoExportResult> = None;
    let mut script_error: Option<String> = None;
    let mut seam_delta_val = 0.0_f32;

    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(err) = msg.get("error").and_then(|v| v.as_str()) {
            script_error = Some(err.to_string());
            continue;
        }
        if let Some(res) = msg.get("result") {
            result = serde_json::from_value(res.clone()).ok();
            continue;
        }
        if let Some(d) = msg.get("seam_delta").and_then(|v| v.as_f64()) {
            seam_delta_val = d as f32;
        }
        emit_progress(
            #[cfg(feature = "desktop")]
            app,
            state,
            &msg,
        );
    }

    let status = child.wait().await?;
    let stderr_lines = stderr_task.await.unwrap_or_default();

    if !status.success() || result.is_none() {
        let reason = script_error
            .or_else(|| stderr_lines.last().cloned())
            .unwrap_or_else(|| format!("Python exited with {status}"));
        return Err(AppError::Other(reason));
    }

    let result = result.expect("checked is_none above");

    // Cross-check the mirror: Python resolves `auto` locally so it can do it in
    // the same pass it measures the seam. If the two halves ever disagree, that
    // is a drift bug and we want it in the log rather than silent.
    if loop_mode == "auto" {
        let expected = resolve_auto(seam_delta_val);
        if expected != result.applied_loop_mode {
            log::warn!(
                "[export] auto resolution drift: rust={expected} python={} (seam {seam_delta_val})",
                result.applied_loop_mode
            );
        }
    }

    // mp4 and avif are both ISOBMFF, so both get the sidecar that survives a
    // chat-client upload. WebP and GIF have no equivalent box and keep only the
    // container-native carrier Python just wrote.
    if matches!(ext, "mp4" | "avif") {
        let out_path = out_path.clone();
        let _ =
            tokio::task::spawn_blocking(move || crate::metadata::mirror_uuid_sidecar(&out_path))
                .await;
    }

    Ok(result)
}

fn emit_progress(
    #[cfg(feature = "desktop")] app: Option<&tauri::AppHandle>,
    state: &AppState,
    payload: &serde_json::Value,
) {
    #[cfg(feature = "desktop")]
    if let Some(app) = app {
        let _ = app.emit("export:progress", payload.clone());
    }
    state.broadcast("export:progress", payload.clone());
}

/// Source dimensions from the gallery index, keyed by full path. Returns
/// `(0, 0)` when the row is unknown.
fn source_dimensions(source: &Path) -> (u32, u32) {
    let path_str = source.to_string_lossy().to_string();
    crate::gallery_index::video_dimensions(&path_str).unwrap_or((0, 0))
}

/// Source frame rate from the gallery index, rounded to whole frames. `None`
/// when the row is unknown or carries no rate, in which case the requested fps
/// is not second-guessed.
fn source_fps(source: &Path) -> Option<u32> {
    let path_str = source.to_string_lossy().to_string();
    crate::gallery_index::video_fps(&path_str).map(|f| f.round() as u32)
}

#[cfg(feature = "desktop")]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn export_video_animation(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    filename: String,
    format: String,
    fps: u32,
    width: u32,
    quality: u32,
    loop_count: u32,
    loop_mode: String,
    crossfade_frames: u32,
    keep_audio: bool,
) -> Result<VideoExportResult, AppError> {
    // Basename only; never let a caller walk out of the gallery.
    let name = Path::new(&filename)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| AppError::Other("Invalid filename".into()))?;
    if !crate::commands::api::is_listable_gallery_file(&name) {
        return Err(AppError::Other("Not a gallery file".into()));
    }
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let source = dir.join(&name);

    run_export(
        #[cfg(feature = "desktop")]
        Some(&app),
        state.inner(),
        None,
        &source,
        &format,
        fps,
        width,
        quality,
        loop_count,
        &loop_mode,
        crossfade_frames,
        keep_audio,
    )
    .await
}

/// Put a file on the clipboard as a *file*, not as pixels, so pasting into
/// Discord or a file manager produces the file itself.
///
/// Linux has no portable file-clipboard mechanism outside a desktop toolkit,
/// so it copies the path as text. That is a degraded result and the button's
/// tooltip says so before the click rather than after.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn copy_file_to_clipboard(path: String) -> Result<(), AppError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::Other(format!("File not found: {path}")));
    }
    let p = p
        .canonicalize()
        .map_err(|e| AppError::Other(e.to_string()))?;

    #[cfg(target_os = "windows")]
    {
        crate::commands::api::clipboard_set_file_drop_win(&p)
    }

    #[cfg(target_os = "macos")]
    {
        // osascript, not objc2: this is the mechanism `native_clipboard_write` already
        // uses for macOS, and it adds no dependency. `POSIX file` puts a file reference
        // on the pasteboard, so Finder and chat clients paste the file itself.
        //
        // The path goes in as an argument to the script's run handler, never
        // spliced into its source: a `"` or `\` in a filename would otherwise
        // end the string literal and run the rest as AppleScript.
        let status = std::process::Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "set the clipboard to POSIX file (item 1 of argv)",
                "-e",
                "end run",
            ])
            .arg(&p)
            .status()
            .map_err(|e| AppError::Other(format!("osascript failed: {e}")))?;
        if !status.success() {
            return Err(AppError::Other(
                "The clipboard rejected the file reference.".into(),
            ));
        }
        return Ok(());
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        // Path as text. Honest degraded path; the tooltip warned about it.
        // stdout and stderr must be null -- xclip forks a background daemon that
        // inherits piped fds and causes wait() to hang forever.
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::Other(format!("Could not run xclip to copy the path: {e}")))?;
        if let Some(ref mut stdin) = child.stdin {
            stdin
                .write_all(p.to_string_lossy().as_bytes())
                .map_err(|e| AppError::Other(format!("xclip stdin write failed: {e}")))?;
        }
        drop(child.stdin.take());
        child
            .wait()
            .map_err(|e| AppError::Other(format!("xclip wait failed: {e}")))?;
        Ok(())
    }
}

/// Copy a file produced by the export pipeline to a caller-chosen destination path.
///
/// Desktop only: browser mode downloads straight from the export endpoint URL
/// instead of calling this command, exactly as `copy_gallery_file_to` does for
/// gallery files. There is deliberately no webserver dispatch arm.
///
/// The source is restricted to the export temp directory so the frontend cannot
/// use this command to read arbitrary paths off the filesystem. The source path
/// is canonicalized before the containment check so that `..` sequences and
/// symlinks are resolved first.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn copy_file_to(src_path: String, dest_path: String) -> Result<(), AppError> {
    // Canonicalize the source - this also confirms the file exists.
    let src = PathBuf::from(&src_path)
        .canonicalize()
        .map_err(|e| AppError::Other(format!("Source file not found: {e}")))?;

    let export_dir = export_temp_dir();
    // The containment check below is only meaningful if the root is ours: a
    // root that is a symlink (to `/`, say) canonicalizes to wherever it points
    // and would let any source pass.
    check_private_dir(&export_dir)
        .map_err(|e| AppError::Other(format!("Export directory is unusable: {e}")))?;
    // Canonicalize the temp dir when it exists. If it does not, fall back to
    // the raw path - any source that actually exists under a non-existent dir
    // cannot pass the canonicalize step above, so this branch is unreachable
    // in practice.
    let temp_dir = export_dir.canonicalize().unwrap_or(export_dir);

    if !src.starts_with(&temp_dir) {
        return Err(AppError::Other(
            "Source path is outside the export directory.".into(),
        ));
    }

    tokio::fs::copy(&src, &dest_path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisors_descend_from_n() {
        assert_eq!(divisors(24), vec![24, 12, 8, 6, 4, 3, 2, 1]);
        assert_eq!(divisors(48), vec![48, 24, 16, 12, 8, 6, 4, 3, 2, 1]);
        assert_eq!(divisors(1), vec![1]);
        assert_eq!(divisors(0), Vec::<u32>::new());
    }

    #[test]
    fn offered_fps_floors_at_six() {
        assert_eq!(offered_fps(24), vec![24, 12, 8, 6]);
        assert_eq!(offered_fps(48), vec![48, 24, 16, 12, 8, 6]);
    }

    #[test]
    fn offered_fps_never_exceeds_the_source() {
        // A 16 fps clip cannot deliver 24, so 24 is absent - not clamped,
        // not greyed out, absent.
        assert!(!offered_fps(16).contains(&24));
        assert_eq!(offered_fps(16), vec![16, 8]);
    }

    #[test]
    fn offered_fps_degenerate_sources_still_offer_something() {
        // Below the floor there is no legal divisor; offer the source itself
        // rather than an empty picker.
        assert_eq!(offered_fps(5), vec![5]);
        assert_eq!(offered_fps(0), vec![MIN_OFFERED_FPS]);
    }

    #[test]
    fn snap_fps_picks_the_highest_offered_at_or_below_target() {
        // GIF Balanced targets 16: exact on a 48 fps clip, snaps down to 12
        // on a 24 fps clip because 16 does not divide 24.
        assert_eq!(snap_fps(16, 48), 16);
        assert_eq!(snap_fps(16, 24), 12);
        assert_eq!(snap_fps(12, 24), 12);
        assert_eq!(snap_fps(24, 24), 24);
        // A target above everything offered lands on the source rate.
        assert_eq!(snap_fps(60, 24), 24);
        // A target below everything offered lands on the lowest offered.
        assert_eq!(snap_fps(2, 24), 6);
    }

    #[test]
    fn snapping_an_already_offered_rate_is_a_no_op() {
        // `run_export` runs every incoming rate through `snap_fps` so a drifted
        // `videoExport.ts` cannot hand the encoder a non-divisor. That guard is
        // only safe if it leaves a correct request completely alone.
        for src in [8u32, 12, 16, 24, 30, 48, 60] {
            for offered in offered_fps(src) {
                assert_eq!(snap_fps(offered, src), offered, "src={src} rate={offered}");
            }
        }
        // A drifted mirror gets corrected downward, never upward.
        assert_eq!(snap_fps(16, 24), 12);
        assert_eq!(snap_fps(30, 24), 24);
        // A missing or zero rate lands on the floor rather than reaching the
        // encoder as 0.
        assert_eq!(snap_fps(0, 24), MIN_OFFERED_FPS);
    }

    #[test]
    fn output_dimensions_snap_even_and_never_upscale() {
        // 832x480 requested at 640 wide -> 640x370 -> snapped to 640x370.
        assert_eq!(output_dimensions(832, 480, 640), (640, 370));
        // Requesting wider than the source lands at the source width.
        assert_eq!(output_dimensions(480, 270, 640), (480, 270));
        // Odd inputs snap down to even on both axes.
        assert_eq!(output_dimensions(833, 481, 833), (832, 480));
    }

    #[test]
    fn loop_mode_frame_math() {
        assert_eq!(output_frame_count("none", 124, 4), 124);
        assert_eq!(output_frame_count("trim", 124, 4), 123);
        assert_eq!(output_frame_count("crossfade", 124, 4), 120);
        assert_eq!(output_frame_count("pingpong", 124, 4), 246);
    }

    #[test]
    fn loop_mode_frame_math_degenerates_safely() {
        // A one-frame clip cannot be trimmed to nothing.
        assert_eq!(output_frame_count("trim", 1, 4), 1);
        // Ping-pong on a two-frame clip is the clip itself.
        assert_eq!(output_frame_count("pingpong", 2, 4), 2);
        // Crossfade that is not available falls back to the source count.
        assert_eq!(output_frame_count("crossfade", 10, 4), 10);
        // An unknown mode is treated as "none" rather than panicking.
        assert_eq!(output_frame_count("nonsense", 124, 4), 124);
    }

    #[test]
    fn crossfade_offered_only_when_the_clip_is_long_enough() {
        assert!(crossfade_available(124, 4));
        assert!(crossfade_available(13, 4));
        assert!(!crossfade_available(12, 4));
        assert!(!crossfade_available(4, 4));
        assert!(!crossfade_available(124, 0));
    }

    #[test]
    fn default_crossfade_scales_with_clip_length() {
        // A short clip (e.g. a 1s@24fps generation) keeps the crossfade small
        // rather than losing a sixth of itself to a flat 4-frame default.
        assert_eq!(default_crossfade_frames(24), 2);
        // Long clips get a proportionally bigger, still-clamped blend.
        assert_eq!(default_crossfade_frames(124), 10);
        assert_eq!(default_crossfade_frames(1000), 16);
        // Too short to crossfade at all: no n keeps crossfade_available true.
        assert_eq!(default_crossfade_frames(3), 0);
        assert_eq!(default_crossfade_frames(0), 0);
    }

    #[test]
    fn default_crossfade_is_always_available_when_nonzero() {
        for f in [4, 5, 6, 7, 12, 13, 24, 48, 124, 500, 5000] {
            let n = default_crossfade_frames(f);
            if n > 0 {
                assert!(
                    crossfade_available(f, n),
                    "default_crossfade_frames({f}) = {n} is not itself available"
                );
            }
        }
    }

    #[test]
    fn auto_trims_a_matching_seam_and_leaves_a_mismatched_one() {
        assert_eq!(resolve_auto(1.2), "trim");
        assert_eq!(resolve_auto(0.0), "trim");
        assert_eq!(resolve_auto(40.0), "none");
        // Exactly at the threshold is not "under" it.
        assert_eq!(resolve_auto(AUTO_SEAM_THRESHOLD), "none");
    }

    #[test]
    fn size_limits_follow_the_platform_table() {
        let twenty_mb = 20 * 1024 * 1024;
        assert!(!over_size_limit(twenty_mb, "discord"));
        assert!(over_size_limit(twenty_mb + 1, "discord"));
        assert!(!over_size_limit(twenty_mb + 1, "nitro"));
        assert!(over_size_limit(500 * 1024 * 1024 + 1, "nitro"));
        // No target selected means no hint, ever.
        assert!(!over_size_limit(u64::MAX, "none"));
        assert!(!over_size_limit(u64::MAX, "anything-else"));
    }

    #[test]
    fn extensions_cover_every_format_and_default_to_avif() {
        assert_eq!(ext_for("avif"), "avif");
        assert_eq!(ext_for("webp"), "webp");
        assert_eq!(ext_for("gif"), "gif");
        assert_eq!(ext_for("mp4"), "mp4");
        // An unrecognised format resolves to the recommended one, not the largest.
        assert_eq!(ext_for(""), "avif");
        assert_eq!(ext_for("mkv"), "avif");
    }

    #[test]
    fn quality_maps_onto_the_crf_scale_inverted() {
        // Higher UI quality must always mean a lower (better) CRF.
        assert_eq!(quality_to_crf(0), 34);
        assert_eq!(quality_to_crf(100), 14);
        assert_eq!(quality_to_crf(70), 20);
        assert_eq!(quality_to_crf(78), 18);
        assert_eq!(quality_to_crf(90), 16);
        // Out-of-range input clamps rather than producing a nonsense CRF.
        assert_eq!(quality_to_crf(500), 14);
        // Monotonically non-increasing across the whole scale.
        for q in 1..=100u32 {
            assert!(quality_to_crf(q) <= quality_to_crf(q - 1));
        }
    }

    #[test]
    fn audio_survives_only_on_mp4_and_never_under_pingpong() {
        assert!(supports_audio("mp4", "auto"));
        assert!(supports_audio("mp4", "none"));
        assert!(supports_audio("mp4", "trim"));
        assert!(supports_audio("mp4", "crossfade"));
        // Ping-pong doubles and reverses the frames; no audio track can follow.
        assert!(!supports_audio("mp4", "pingpong"));
        // The animated-image formats have no audio track at all.
        assert!(!supports_audio("avif", "none"));
        assert!(!supports_audio("webp", "none"));
        assert!(!supports_audio("gif", "none"));
    }

    /// A minimal walkable mp4: `ftyp` plus a stub `moov`, no metadata anywhere.
    /// `append_uuid_xmp` requires the first box to be `ftyp` and the last box to
    /// end at EOF, and this satisfies both.
    fn bare_mp4() -> Vec<u8> {
        let mut buf = 20u32.to_be_bytes().to_vec();
        buf.extend_from_slice(b"ftypisom");
        buf.extend_from_slice(&0u32.to_be_bytes()); // minor version
        buf.extend_from_slice(b"isom");
        buf.extend_from_slice(&13u32.to_be_bytes());
        buf.extend_from_slice(b"moovindex");
        buf
    }

    #[test]
    fn source_metadata_is_read_back_as_swarmui_json() {
        let mut params = std::collections::HashMap::new();
        params.insert(
            "positive_prompt".to_string(),
            "export round trip".to_string(),
        );
        params.insert("seed".to_string(), "31337".to_string());
        let json = crate::metadata::format_swarmui_json(&params);

        let dir = std::env::temp_dir().join("mooshie_export_meta");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("source.mp4");

        // A clip with no metadata yields an empty string, not an error.
        std::fs::write(&path, bare_mp4()).unwrap();
        assert_eq!(source_metadata_json(&path), "");

        // With the `uuid` sidecar `mirror_uuid_sidecar` leaves behind, it reads back.
        std::fs::write(
            &path,
            crate::metadata::embed_uuid_for_test(&bare_mp4(), &json),
        )
        .unwrap();
        let read = source_metadata_json(&path);
        assert!(read.contains("export round trip"), "got: {read}");
        assert!(read.contains("31337"), "got: {read}");

        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod export_path_tests {
    use super::{export_dir_for, export_file_name, export_temp_dir, normalize_loop_mode};

    #[test]
    fn loop_mode_is_whitelisted_to_what_the_encoder_implements() {
        for mode in ["auto", "none", "trim", "crossfade", "pingpong"] {
            assert_eq!(normalize_loop_mode(mode), mode);
        }
        for bad in ["", "PINGPONG", "..\\..\\x", "a/b", "C:evil", "trim\0"] {
            assert_eq!(normalize_loop_mode(bad), "auto", "{bad:?}");
        }
    }

    #[test]
    fn export_file_name_is_one_plain_component() {
        let name = export_file_name("clip", 640, 24, normalize_loop_mode("..\\..\\x"), "avif");
        assert_eq!(name, "clip_640w_24fps_auto.avif");
        assert!(crate::commands::api::is_single_safe_filename(&name));
    }

    #[test]
    fn exports_are_namespaced_per_lan_user() {
        let base = export_temp_dir();
        assert_eq!(export_dir_for(None), Some(base.clone()));
        assert_eq!(
            export_dir_for(Some("Bob")),
            Some(base.join("users").join("bob"))
        );
        assert_eq!(export_dir_for(Some("..")), None);
        assert_eq!(export_dir_for(Some("D:x")), None);
        assert_eq!(export_dir_for(Some("a/b")), None);
    }
}

#[cfg(test)]
mod private_dir_tests {
    use super::ensure_private_export_dir;
    use std::path::PathBuf;

    /// A fresh parent for a stand-in export root; the root itself is left
    /// for the code under test to create.
    fn scratch() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mooshie-export-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    fn mode(path: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    fn creates_every_level_private() {
        let parent = scratch();
        let root = parent.join("mooshie-export");
        let dir = root.join("users").join("bob");
        ensure_private_export_dir(&root, &dir).unwrap();
        assert!(dir.is_dir());
        #[cfg(unix)]
        for level in [&root, &root.join("users"), &dir] {
            assert_eq!(mode(level), 0o700, "{}", level.display());
        }
        // Idempotent.
        ensure_private_export_dir(&root, &dir).unwrap();
        std::fs::remove_dir_all(&parent).ok();
    }

    #[cfg(unix)]
    #[test]
    fn tightens_a_root_left_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let parent = scratch();
        let root = parent.join("mooshie-export");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        ensure_private_export_dir(&root, &root).unwrap();
        assert_eq!(mode(&root), 0o700);
        std::fs::remove_dir_all(&parent).ok();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_root_or_level() {
        let parent = scratch();
        let elsewhere = parent.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();

        let root = parent.join("mooshie-export");
        std::os::unix::fs::symlink(&elsewhere, &root).unwrap();
        assert!(ensure_private_export_dir(&root, &root.join("users")).is_err());
        assert!(std::fs::read_dir(&elsewhere).unwrap().next().is_none());

        let root2 = parent.join("mooshie-export-2");
        std::fs::create_dir(&root2).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root2.join("users")).unwrap();
        assert!(ensure_private_export_dir(&root2, &root2.join("users").join("bob")).is_err());
        assert!(std::fs::read_dir(&elsewhere).unwrap().next().is_none());
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn refuses_a_dir_outside_the_root_or_a_file_in_the_way() {
        let parent = scratch();
        let root = parent.join("mooshie-export");
        assert!(ensure_private_export_dir(&root, &parent.join("other")).is_err());
        assert!(ensure_private_export_dir(&root, &root.join("..").join("x")).is_err());
        // Refused before anything was created.
        assert!(!root.exists());

        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("users"), b"not a dir").unwrap();
        assert!(ensure_private_export_dir(&root, &root.join("users").join("bob")).is_err());
        std::fs::remove_dir_all(&parent).ok();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_root_owned_by_another_user() {
        // Only root can hand a directory to someone else; skip otherwise.
        if super::current_uid().unwrap() != 0 {
            return;
        }
        let parent = scratch();
        let root = parent.join("mooshie-export");
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::chown(&root, Some(54_321), None).unwrap();
        assert!(ensure_private_export_dir(&root, &root).is_err());
        std::fs::remove_dir_all(&parent).ok();
    }
}
