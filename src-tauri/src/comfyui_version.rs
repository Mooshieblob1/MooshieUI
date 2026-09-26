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

/// Baseline release for compatibility comparisons. Installers resolve it with
/// `comfyui_source_ref()`, which can select a tested immutable commit while a
/// feature awaits a tagged release. The compatibility bot advances this tag
/// only after bundled-node and native music checks pass.
pub const COMFYUI_REF: &str = "v0.37.0";

#[derive(serde::Deserialize)]
struct SourceOverride {
    release: String,
    revision: String,
    label: String,
}

static SOURCE_OVERRIDE: std::sync::LazyLock<SourceOverride> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../runtime/comfyui-source.json"))
        .expect("bundled ComfyUI source manifest must be valid")
});

/// Use the tested YuE2 commit until a newer release passes the compatibility
/// gate. Keeping the baseline tag separate lets the release bot compare tags.
pub fn comfyui_source_ref() -> &'static str {
    if SOURCE_OVERRIDE.release == COMFYUI_REF {
        &SOURCE_OVERRIDE.revision
    } else {
        COMFYUI_REF
    }
}

pub fn comfyui_target_label() -> &'static str {
    if SOURCE_OVERRIDE.release == COMFYUI_REF {
        &SOURCE_OVERRIDE.label
    } else {
        COMFYUI_REF
    }
}

pub fn comfyui_archive_url() -> String {
    format!(
        "https://github.com/Comfy-Org/ComfyUI/archive/{}.zip",
        comfyui_source_ref()
    )
}

pub fn comfyui_zip_dirname() -> String {
    format!("ComfyUI-{}", comfyui_source_ref().trim_start_matches('v'))
}

fn has_native_music(comfyui_dir: &Path) -> bool {
    [
        "comfy_extras/nodes_yue2.py",
        "comfy/text_encoders/yue2.py",
        "comfy/ldm/yue2/model.py",
    ]
    .iter()
    .all(|file| comfyui_dir.join(file).is_file())
}

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
    /// Display label of the managed source, including any feature override.
    pub target: String,
    /// True when the installation is older, incomplete, or lacks target features.
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
            Some(v) => {
                comfyui_version_is_older(v, COMFYUI_REF)
                    || (!comfyui_version_is_older(COMFYUI_REF, v) && !has_native_music(comfyui_dir))
            }
            // An install with main.py but no comfyui_version.py predates the
            // version module entirely, so it is always older than the pinned
            // target. No main.py means ComfyUI isn't installed at all — that is
            // the setup wizard's job, not the updater's.
            None => comfyui_dir.join("main.py").exists(),
        };
    ComfyUiVersionInfo {
        installed,
        target: comfyui_target_label().to_string(),
        update_available,
        update_incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_source_and_archive_use_the_same_immutable_revision() {
        let source = comfyui_source_ref();
        if SOURCE_OVERRIDE.release == COMFYUI_REF {
            assert_eq!(source.len(), 40);
            assert!(source.bytes().all(|b| b.is_ascii_hexdigit()));
            assert_ne!(source, COMFYUI_REF);
            assert!(comfyui_target_label().contains("YuE2"));
        }
        assert!(comfyui_archive_url().ends_with(&format!("/{source}.zip")));
        assert_eq!(
            comfyui_zip_dirname(),
            format!("ComfyUI-{}", source.trim_start_matches('v'))
        );
    }

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
        for file in [
            "comfy_extras/nodes_yue2.py",
            "comfy/text_encoders/yue2.py",
            "comfy/ldm/yue2/model.py",
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let _ = std::fs::remove_file(dir.join(UPDATE_PENDING_MARKER));

        let clean = comfyui_version_info(&dir);
        assert!(!clean.update_available);
        assert!(!clean.update_incomplete);

        std::fs::remove_file(dir.join("comfy_extras/nodes_yue2.py")).unwrap();
        assert!(
            comfyui_version_info(&dir).update_available,
            "plain v0.35.0 must upgrade to the YuE2 source"
        );
        std::fs::write(dir.join("comfy_extras/nodes_yue2.py"), "").unwrap();

        std::fs::write(dir.join(UPDATE_PENDING_MARKER), COMFYUI_REF).unwrap();
        let pending = comfyui_version_info(&dir);
        assert!(pending.update_incomplete);
        assert!(pending.update_available);

        std::fs::remove_dir_all(&dir).ok();
    }
}
