//! Interpolate a clip that is already in the gallery.
//!
//! The desktop command and the browser-mode dispatch arm share every step
//! except how they find the gallery directory, so the validation and submission
//! halves live here as free functions.

use std::path::{Path, PathBuf};
#[cfg(feature = "desktop")]
use std::sync::Arc;

#[cfg(feature = "desktop")]
use tauri::State;

use crate::error::AppError;
use crate::state::AppState;
use crate::templates::rife::RifeSettings;

#[derive(serde::Serialize)]
pub struct InterpolateVideoResponse {
    pub prompt_id: String,
}

/// Resolve a caller-supplied gallery filename to a path on disk.
///
/// In browser mode the filename comes from any authenticated LAN user, so
/// accepting a path would let them point the job at an arbitrary mp4 on the
/// host and have the result copied into their own gallery. Only a bare gallery
/// filename is accepted, and the resolved path is re-checked against the
/// gallery root in case a symlink points elsewhere.
///
/// Validation order (each check applies to the already-trimmed name):
/// 1. Empty after trim -- catches whitespace-only strings.
/// 2. Contains '/' or '\\' -- rejects any path separator, including relative
///    paths on both platforms.
/// 3. Contains ".." -- belt-and-suspenders against traversal sequences that
///    slip through step 2.
/// 4. `file_name()` round-trip -- catches drive-relative paths ("C:foo"),
///    ADS suffixes ("foo.mp4:stream"), and anything whose file_name does not
///    round-trip to the original string.
/// 5. Must end with ".mp4" -- only video clips can be interpolated.
/// 6. `is_listable_gallery_file` -- deliberately redundant today. Step 5
///    already requires ".mp4", which that function accepts, so this can never
///    reject anything. It is kept so the set of names this command will touch
///    stays tied to the same gallery-membership rule the listing API uses: if
///    that rule ever narrows (new sidecar convention, a format retired), this
///    follows automatically instead of silently drifting.
/// 7. Canonicalize both the joined path and the gallery root, then assert the
///    joined path is under the root -- symlinks and Windows drive-letter quirks
///    are resolved before comparison, so neither can escape the gallery.
pub(crate) fn resolve_gallery_video(dir: &Path, filename: &str) -> Result<PathBuf, AppError> {
    let name = filename.trim();

    if name.is_empty() {
        return Err(AppError::Other("Invalid filename".into()));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(AppError::Other("Invalid filename".into()));
    }
    if name.contains("..") {
        return Err(AppError::Other("Invalid filename".into()));
    }

    // Reject anything whose file_name does not round-trip exactly.  This
    // catches drive-relative Windows paths ("C:foo.mp4"), alternate data
    // stream suffixes ("foo.mp4:evil"), trailing dots/spaces that the NTFS
    // kernel strips but std::fs does not ("foo.mp4 "), and similar quirks.
    let looks_bare = Path::new(name)
        .file_name()
        .map(|f| f.to_string_lossy() == name)
        .unwrap_or(false);
    if !looks_bare {
        return Err(AppError::Other("Invalid filename".into()));
    }

    if !name.to_ascii_lowercase().ends_with(".mp4") {
        return Err(AppError::Other("Only mp4 clips can be interpolated".into()));
    }
    // Unreachable given the .mp4 check above; kept so this command and the
    // gallery listing agree on what counts as a gallery entry.  See step 6.
    if !crate::commands::api::is_listable_gallery_file(name) {
        return Err(AppError::Other("Not a gallery file".into()));
    }

    let resolved = dir
        .join(name)
        .canonicalize()
        .map_err(|_| AppError::Other(format!("Video not found: {name}")))?;
    let root = dir
        .canonicalize()
        .map_err(|e| AppError::Other(format!("Gallery unavailable: {e}")))?;
    if !resolved.starts_with(&root) {
        return Err(AppError::Other("Not a gallery file".into()));
    }
    Ok(resolved)
}

/// Build and queue the interpolation prompt. Mirrors `commands::workflow::generate`
/// so a post-hoc job shows up in the shared queue exactly like a generation.
pub(crate) async fn submit_interpolation(
    state: &AppState,
    source: &Path,
    settings: RifeSettings,
    owner: Option<String>,
) -> Result<String, AppError> {
    let comfyui_path = state.config.read().await.comfyui_path.clone();
    if !crate::comfyui::nodes::is_rife_installed(&comfyui_path) {
        return Err(AppError::Other(
            "Frame interpolation is not installed yet".into(),
        ));
    }

    let workflow = crate::templates::video_interpolate::build(&source.to_string_lossy(), settings);
    log::info!(
        "interpolate_video: {}x scale={} source={}",
        settings.multiplier,
        settings.scale_factor,
        source.display()
    );

    crate::comfyui::process::mark_legacy_worker_idle(state).await;
    // RIFE's float32 batch competes with the assistant LLM for memory, so free
    // it first exactly as a normal generation does.
    state.free_llm_vram_for_generation().await;

    let timeout = std::time::Duration::from_secs(300);
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt(workflow, &state.client_id, timeout)
        .await?;

    state.prompt_queue.insert(&response.prompt_id, owner);
    state
        .prompt_queue
        .set_worker(&response.prompt_id, worker_id);
    state.broadcast_queue_positions();

    Ok(response.prompt_id)
}

/// Queue a RIFE pass over a finished gallery clip. The result arrives through
/// the normal video output path as a new gallery entry.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn interpolate_video(
    state: State<'_, Arc<AppState>>,
    filename: String,
    multiplier: u32,
    scale_factor: f64,
    fast_mode: bool,
    ensemble: bool,
) -> Result<InterpolateVideoResponse, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let source = resolve_gallery_video(&dir, &filename)?;
    let settings = RifeSettings::sanitized(multiplier, scale_factor, fast_mode, ensemble);
    let prompt_id = submit_interpolation(state.inner(), &source, settings, None).await?;
    Ok(InterpolateVideoResponse { prompt_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_gallery(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mooshie-rife-test-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp gallery");
        dir
    }

    #[test]
    fn rejects_paths_and_traversal() {
        let dir = temp_gallery("traversal");
        std::fs::write(dir.join("clip.mp4"), b"x").expect("write clip");
        for bad in [
            "../clip.mp4",
            "sub/clip.mp4",
            "sub\\clip.mp4",
            "..",
            "",
            "   ",
        ] {
            assert!(
                resolve_gallery_video(&dir, bad).is_err(),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_non_mp4_and_missing_files() {
        let dir = temp_gallery("kinds");
        std::fs::write(dir.join("still.png"), b"x").expect("write still");
        assert!(resolve_gallery_video(&dir, "still.png").is_err());
        assert!(resolve_gallery_video(&dir, "gone.mp4").is_err());
    }

    #[test]
    fn accepts_a_plain_gallery_clip() {
        let dir = temp_gallery("ok");
        std::fs::write(dir.join("mooshie_video_00001_.mp4"), b"x").expect("write clip");
        let resolved =
            resolve_gallery_video(&dir, "mooshie_video_00001_.mp4").expect("clip resolves");
        assert!(resolved.ends_with("mooshie_video_00001_.mp4"));
    }
}
