//! Installed-vs-target ComfyUI version reporting.
//!
//! This logic is shared by the desktop Tauri command
//! ([`crate::setup::get_comfyui_version`]) and the browser-mode webserver
//! dispatch, so it lives in its own module compiled for both the `desktop` and
//! `server` builds rather than inside the desktop-only `setup` module.

use std::path::Path;

/// Marker file written into the ComfyUI directory while an update is being
/// applied, and removed only once every step succeeded.
///
/// `update_comfyui` moves the source checkout onto the pinned tag *before*
/// reinstalling ComfyUI's Python dependencies, so a failure in the dependency
/// step leaves new source running against an old venv. The version report is
/// derived from the checkout alone, which would then read as "up to date" and
/// hide the very affordance needed to retry. This marker makes that
/// half-applied state visible so the update can be offered again.
pub const UPDATE_PENDING_MARKER: &str = ".mooshie-update-pending";

/// True when a previous update moved the checkout but did not finish.
pub fn update_is_incomplete(comfyui_dir: &Path) -> bool {
    comfyui_dir.join(UPDATE_PENDING_MARKER).exists()
}

/// Pinned ComfyUI release tag that the app installs and updates to.
///
/// Fresh installs and the in-app "Update ComfyUI" action both target this exact
/// tag, so every MooshieUI build runs against a known-good ComfyUI rather than
/// whatever `master` happened to be at install time. The scheduled
/// `comfyui-compat` workflow opens a bot PR bumping this constant once the
/// custom-node smoke test passes against a newer ComfyUI release.
pub const COMFYUI_REF: &str = "v0.34.0";

/// Read the installed ComfyUI version from its `comfyui_version.py` file
/// (`__version__ = "0.26.0"`). Returns `None` if the file is missing or
/// unparseable (e.g. a very old ComfyUI without the version module).
fn read_installed_comfyui_version(comfyui_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(comfyui_dir.join("comfyui_version.py")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("__version__") {
            if let Some(value) = rest.split('=').nth(1) {
                let v = value.trim().trim_matches(|c| c == '"' || c == '\'');
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Parse a ComfyUI version string (`v0.26.0` / `0.26.0`) into numeric parts,
/// tolerating trailing non-digits in any component.
fn parse_comfyui_version(v: &str) -> Vec<u32> {
    v.trim_start_matches('v')
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

/// True when `installed` is strictly older than `target` (so an update is worth
/// offering). A newer-or-equal install is not flagged, to avoid presenting a
/// downgrade as an "update".
fn comfyui_version_is_older(installed: &str, target: &str) -> bool {
    let a = parse_comfyui_version(installed);
    let b = parse_comfyui_version(target);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x < y;
        }
    }
    false
}

#[derive(Clone, serde::Serialize)]
pub struct ComfyUiVersionInfo {
    /// Version currently installed on disk, if detectable.
    pub installed: Option<String>,
    /// The pinned tag this MooshieUI build targets ([`COMFYUI_REF`]).
    pub target: String,
    /// True when the installed version is older than the pinned target.
    pub update_available: bool,
    /// True when a previous update left the install half-applied (new source,
    /// stale Python dependencies). The version numbers can look current while
    /// this is set.
    pub update_incomplete: bool,
}

/// Compute the installed-vs-target version report for a ComfyUI checkout.
/// Shared by the desktop command and the browser-mode webserver dispatch.
pub fn comfyui_version_info(comfyui_dir: &Path) -> ComfyUiVersionInfo {
    let installed = read_installed_comfyui_version(comfyui_dir);
    let update_incomplete = update_is_incomplete(comfyui_dir);
    let update_available = update_incomplete
        || match installed.as_deref() {
            Some(v) => comfyui_version_is_older(v, COMFYUI_REF),
            // An install with main.py but no comfyui_version.py predates the
            // version module entirely, so it is always older than the pinned
            // target. No main.py means ComfyUI isn't installed at all — that is
            // the setup wizard's job, not the updater's.
            None => comfyui_dir.join("main.py").exists(),
        };
    ComfyUiVersionInfo {
        installed,
        target: COMFYUI_REF.to_string(),
        update_available,
        update_incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checkout already on the pinned tag still reports an update when the
    /// pending marker is there. Without this, a failed dependency step leaves
    /// ComfyUI unable to start and hides the only in-app way to retry.
    #[test]
    fn pending_marker_forces_update_available() {
        let dir = std::env::temp_dir().join("mooshie_version_marker_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("comfyui_version.py"),
            format!(
                "__version__ = \"{}\"\n",
                COMFYUI_REF.trim_start_matches('v')
            ),
        )
        .unwrap();
        std::fs::write(dir.join("main.py"), "").unwrap();
        let _ = std::fs::remove_file(dir.join(UPDATE_PENDING_MARKER));

        let clean = comfyui_version_info(&dir);
        assert!(!clean.update_available);
        assert!(!clean.update_incomplete);

        std::fs::write(dir.join(UPDATE_PENDING_MARKER), COMFYUI_REF).unwrap();
        let pending = comfyui_version_info(&dir);
        assert!(pending.update_incomplete);
        assert!(pending.update_available);

        std::fs::remove_dir_all(&dir).ok();
    }
}
