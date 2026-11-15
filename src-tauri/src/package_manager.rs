//! Which Linux package manager owns this install, if any.
//!
//! The .deb and .rpm from a GitHub release update themselves through the
//! in-app updater (`pkexec dpkg -i` / `rpm -U`). Installed from the MooshieUI
//! apt or rpm repo, or from the AUR, the same files belong to the system
//! package manager instead, and the in-app updater would fight it. Those
//! installs say so: the repo setup drops one of the source files below, and
//! the AUR package writes [`MARKER`].

use std::path::Path;

/// apt source added by the MooshieUI repo setup instructions.
pub const APT_SOURCE: &str = "/etc/apt/sources.list.d/mooshieui.sources";
/// dnf repo file served at mooshieui.com/rpm/mooshieui.repo.
pub const DNF_REPO: &str = "/etc/yum.repos.d/mooshieui.repo";
/// The same repo file added through zypper on openSUSE.
pub const ZYPPER_REPO: &str = "/etc/zypp/repos.d/mooshieui.repo";
/// Written by distro packages (the AUR PKGBUILD); its first line names the
/// package manager.
pub const MARKER: &str = "/usr/share/mooshieui/package-manager";

/// Names [`MARKER`] may hold. Anything else is ignored.
const KNOWN: [&str; 4] = ["pacman", "apt", "dnf", "zypper"];

/// The package manager that owns this install, or `None` when the app updates
/// itself (AppImage, Windows, macOS, a hand-installed .deb or .rpm).
pub fn detect() -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    detect_with(
        &exe,
        |path| std::fs::read_to_string(path).ok(),
        |path| path.is_file(),
    )
}

fn detect_with(
    exe: &Path,
    read: impl Fn(&Path) -> Option<String>,
    exists: impl Fn(&Path) -> bool,
) -> Option<&'static str> {
    // Packages install under /usr. An AppImage (mounted under /tmp) or a dev
    // build keeps updating itself even on a machine that has the repo set up.
    if !exe.starts_with("/usr") {
        return None;
    }
    if let Some(text) = read(Path::new(MARKER)) {
        let name = text.lines().next().unwrap_or("").trim();
        if let Some(known) = KNOWN.iter().find(|k| **k == name) {
            return Some(known);
        }
    }
    [
        (APT_SOURCE, "apt"),
        (DNF_REPO, "dnf"),
        (ZYPPER_REPO, "zypper"),
    ]
    .into_iter()
    .find(|(file, _)| exists(Path::new(file)))
    .map(|(_, manager)| manager)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALLED: &str = "/usr/bin/mooshie-ui";

    fn detect_files(exe: &str, files: &[(&str, &str)]) -> Option<&'static str> {
        let lookup = |path: &Path| {
            files
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| text.to_string())
        };
        detect_with(Path::new(exe), lookup, |path| lookup(path).is_some())
    }

    #[test]
    fn hand_installed_package_updates_itself() {
        assert_eq!(detect_files(INSTALLED, &[]), None);
    }

    #[test]
    fn repo_files_name_their_manager() {
        assert_eq!(detect_files(INSTALLED, &[(APT_SOURCE, "")]), Some("apt"));
        assert_eq!(detect_files(INSTALLED, &[(DNF_REPO, "")]), Some("dnf"));
        assert_eq!(
            detect_files(INSTALLED, &[(ZYPPER_REPO, "")]),
            Some("zypper")
        );
    }

    #[test]
    fn marker_wins_and_only_known_names_count() {
        assert_eq!(
            detect_files(INSTALLED, &[(MARKER, "pacman\n"), (APT_SOURCE, "")]),
            Some("pacman")
        );
        assert_eq!(detect_files(INSTALLED, &[(MARKER, "rm -rf /\n")]), None);
        assert_eq!(
            detect_files(INSTALLED, &[(MARKER, "whatever"), (DNF_REPO, "")]),
            Some("dnf")
        );
    }

    #[test]
    fn appimage_and_dev_builds_ignore_repo_files() {
        let files = [(MARKER, "pacman"), (APT_SOURCE, "")];
        assert_eq!(
            detect_files("/tmp/.mount_MooshieXYZ/usr/bin/mooshie-ui", &files),
            None
        );
        assert_eq!(
            detect_files(
                "/home/me/MooshieUI/src-tauri/target/debug/mooshie-ui",
                &files
            ),
            None
        );
    }
}
