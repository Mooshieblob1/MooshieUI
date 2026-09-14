//! Git used by the managed runtime, including Windows installs without Git on PATH.

use std::path::PathBuf;
#[cfg(any(windows, feature = "desktop", test))]
use std::{path::Path, time::Duration};

#[cfg(windows)]
const MINGIT_URL: &str = "https://github.com/git-for-windows/git/releases/download/v2.55.0.windows.5/MinGit-2.55.0.5-64-bit.zip";
// Published by Git for Windows alongside the immutable release above.
#[cfg(windows)]
const MINGIT_SHA256: &str = "56d7b226b7693196cfc71fef26568f536c4a021ab6c37ff2db4287bed908e96e";

#[cfg(windows)]
static SELECTED_GIT: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

#[cfg(windows)]
fn managed_git_dir(base: &Path) -> PathBuf {
    base.join("bin").join("mingit")
}

#[cfg(windows)]
fn windows_candidates(base: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(base) = base {
        candidates.push(managed_git_dir(base).join("cmd/git.exe"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join("git.exe")));
    }
    // Explorer can retain the PATH from before Git was installed. Also support
    // installs whose user chose not to add Git to PATH.
    for name in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(name) {
            candidates.push(PathBuf::from(dir).join("Git/cmd/git.exe"));
        }
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(dir).join("Programs/Git/cmd/git.exe"));
    }
    candidates
}

#[cfg(windows)]
fn first_existing(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
fn installed_git() -> Option<PathBuf> {
    if let Some(git) = SELECTED_GIT
        .read()
        .ok()
        .and_then(|git| git.clone())
        .filter(|git| git.is_file())
    {
        return Some(git);
    }
    first_existing(windows_candidates(crate::config::app_data_dir().as_deref()))
}

/// Resolve Git explicitly: changing a child's PATH alone does not reliably
/// change Windows' lookup of the executable being spawned.
pub(crate) fn resolve_program(program: &std::ffi::OsStr) -> PathBuf {
    #[cfg(windows)]
    if program == "git" || program == "git.exe" {
        if let Some(git) = installed_git() {
            return git;
        }
    }
    PathBuf::from(program)
}

/// Let Python (including GitPython), pip and uv find the same Git after every
/// app restart. Never change the parent process or the user's system PATH.
pub(crate) fn apply_child_env(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    if let Some(git) = installed_git() {
        if let Some(dir) = git.parent() {
            let mut paths = vec![dir.to_path_buf()];
            if let Some(path) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&path));
            }
            if let Ok(path) = std::env::join_paths(paths) {
                cmd.env("PATH", path);
            }
            if std::env::var_os("GIT_PYTHON_GIT_EXECUTABLE").is_none() {
                cmd.env("GIT_PYTHON_GIT_EXECUTABLE", git);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(any(windows, feature = "desktop", test))]
async fn verify_git(git: &Path) -> Result<(), String> {
    run_git(git, &["--version"], None).await.map(|_| ())
}

/// Provision an app-local MinGit only when no working Git can be found.
/// The shared lock also covers simultaneous GPU-worker startup attempts.
#[cfg(any(windows, feature = "desktop"))]
pub(crate) async fn ensure_available(
    base: &Path,
    client: &reqwest::Client,
    network_proxy: Option<&str>,
) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        ensure_windows_git(base, client, network_proxy, windows_candidates(Some(base))).await
    }
    #[cfg(not(windows))]
    {
        let _ = (base, client, network_proxy);
        let git = PathBuf::from("git");
        verify_git(&git).await.map_err(|e| {
            format!("Git is required to update ComfyUI. Install Git and restart MooshieUI. {e}")
        })?;
        Ok(git)
    }
}

#[cfg(windows)]
async fn ensure_windows_git(
    base: &Path,
    client: &reqwest::Client,
    network_proxy: Option<&str>,
    candidates: Vec<PathBuf>,
) -> Result<PathBuf, String> {
    static INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _install = INSTALL_LOCK.lock().await;
    for candidate in candidates {
        if candidate.is_file() && verify_git(&candidate).await.is_ok() {
            if let Ok(mut selected) = SELECTED_GIT.write() {
                *selected = Some(candidate.clone());
            }
            return Ok(candidate);
        }
    }

    log::info!("Git is unavailable; installing managed MinGit for Windows");
    // Cache proxy clients, preserving the shared pool for direct downloads.
    // A reqwest proxy is a client option, not a per-request option.
    static PROXY_CLIENT: std::sync::Mutex<Option<(String, reqwest::Client)>> =
        std::sync::Mutex::new(None);
    let download_client =
        if let Some(proxy) = network_proxy.map(str::trim).filter(|p| !p.is_empty()) {
            let mut cached = PROXY_CLIENT.lock().map_err(|e| e.to_string())?;
            if cached.as_ref().is_none_or(|(url, _)| url != proxy) {
                let proxy_config = reqwest::Proxy::all(proxy)
                    .map_err(|_| "Invalid network proxy for the Git download".to_string())?;
                let client = reqwest::Client::builder()
                    .proxy(proxy_config)
                    .build()
                    .map_err(|e| format!("Cannot configure Git download: {e}"))?;
                *cached = Some((proxy.to_string(), client));
            }
            cached.as_ref().unwrap().1.clone()
        } else {
            client.clone()
        };
    let bytes = download_client
        .get(MINGIT_URL)
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| format!("Cannot download Git: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Cannot download Git: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Cannot download Git: {e}"))?;
    let destination = managed_git_dir(base);
    let staging = base
        .join("bin")
        .join(format!(".mingit-{}", uuid::Uuid::new_v4()));
    let extract_to = staging.clone();
    let result = tokio::task::spawn_blocking(move || extract_mingit(&bytes, &extract_to))
        .await
        .map_err(|e| format!("Git extraction failed: {e}"))?;
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    let result = verify_git(&staging.join("cmd/git.exe")).await;
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("Downloaded Git cannot run: {error}"));
    }
    let result = publish_mingit(&staging, &destination);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result?;
    log::info!("Managed Git installed successfully");
    let git = destination.join("cmd/git.exe");
    if let Ok(mut selected) = SELECTED_GIT.write() {
        *selected = Some(git.clone());
    }
    Ok(git)
}

#[cfg(windows)]
fn extract_mingit(bytes: &[u8], staging: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    if hex::encode(Sha256::digest(bytes)) != MINGIT_SHA256 {
        return Err("Git download checksum mismatch. Please retry the update.".to_string());
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("Cannot read Git archive: {e}"))?;
    archive
        .extract(staging)
        .map_err(|e| format!("Cannot extract Git: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn publish_mingit(staging: &Path, destination: &Path) -> Result<(), String> {
    let backup = staging.with_extension("old");
    let had_previous = destination.exists();
    if had_previous {
        std::fs::rename(destination, &backup)
            .map_err(|e| format!("Cannot replace damaged managed Git: {e}"))?;
    }
    if let Err(error) = std::fs::rename(staging, destination) {
        if had_previous {
            let _ = std::fs::rename(&backup, destination);
        }
        return Err(format!("Cannot install managed Git: {error}"));
    }
    if had_previous {
        let _ = std::fs::remove_dir_all(backup);
    }
    Ok(())
}

#[cfg(any(windows, feature = "desktop", test))]
async fn run_git(git: &Path, args: &[&str], network_proxy: Option<&str>) -> Result<String, String> {
    let mut command = super::process::tokio_command_no_window(git);
    command
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    super::nodes::apply_network_proxy(&mut command, network_proxy);
    let timeout = if args == ["--version"] { 10 } else { 300 };
    let output = tokio::time::timeout(Duration::from_secs(timeout), command.output())
        .await
        .map_err(|_| {
            "Git timed out. Check the connection and network proxy, then retry.".to_string()
        })?
        .map_err(|e| format!("Cannot run Git at '{}': {e}", git.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(format!(
            "Git exited with {}: {}",
            output.status,
            detail.chars().take(4000).collect::<String>()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Fetch first, while the old ComfyUI process and source are still usable.
/// The returned commit is fixed even if another tool changes FETCH_HEAD later.
#[cfg(any(feature = "desktop", test))]
pub(crate) struct PreparedCheckout {
    git: PathBuf,
    directory: PathBuf,
    revision: String,
    pub method: &'static str,
}

#[cfg(any(feature = "desktop", test))]
pub(crate) async fn prepare_checkout(
    git: &Path,
    directory: &Path,
    url: &str,
    source_ref: &str,
    network_proxy: Option<&str>,
) -> Result<PreparedCheckout, String> {
    let dir = directory.to_str().ok_or("Invalid ComfyUI path")?;
    let method = if directory.join(".git").exists() {
        "git-fetch"
    } else {
        run_git(git, &["-C", dir, "init"], None)
            .await
            .map_err(|e| format!("Failed to initialise Git in the ComfyUI directory: {e}"))?;
        "git-init"
    };
    // Fetch the canonical URL directly. A missing/broken origin left by an
    // interrupted ZIP conversion must not make every subsequent retry fail.
    run_git(
        git,
        &["-C", dir, "fetch", "--depth=1", url, source_ref],
        network_proxy,
    )
    .await
    .map_err(|e| format!("Failed to fetch ComfyUI {source_ref}: {e}"))?;
    let revision = run_git(
        git,
        &["-C", dir, "rev-parse", "--verify", "FETCH_HEAD^{commit}"],
        None,
    )
    .await?;
    Ok(PreparedCheckout {
        git: git.to_path_buf(),
        directory: directory.to_path_buf(),
        revision,
        method,
    })
}

#[cfg(any(feature = "desktop", test))]
impl PreparedCheckout {
    pub(crate) async fn apply(&self) -> Result<(), String> {
        let dir = self.directory.to_str().ok_or("Invalid ComfyUI path")?;
        // Never git clean: models, inputs, outputs and custom nodes are user data.
        run_git(
            &self.git,
            &["-C", dir, "reset", "--hard", &self.revision],
            None,
        )
        .await
        .map_err(|e| format!("Failed to check out ComfyUI: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("mooshie-git-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    async fn source_repo(root: &Path) -> (PathBuf, PathBuf, String) {
        let git = resolve_program("git".as_ref());
        verify_git(&git)
            .await
            .expect("Git must be installed to run checkout integration tests");
        let source = root.join("source repo");
        fs::create_dir_all(&source).unwrap();
        run_git(&git, &["-C", source.to_str().unwrap(), "init"], None)
            .await
            .unwrap();
        fs::write(source.join("main.py"), "new source\n").unwrap();
        fs::write(source.join("obsolete.py"), "old module\n").unwrap();
        let revision = commit(&git, &source).await;
        (git, source, revision)
    }

    async fn commit(git: &Path, source: &Path) -> String {
        let dir = source.to_str().unwrap();
        run_git(git, &["-C", dir, "add", "."], None).await.unwrap();
        run_git(
            git,
            &[
                "-C",
                dir,
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-m",
                "Test source",
            ],
            None,
        )
        .await
        .unwrap();
        run_git(git, &["-C", dir, "rev-parse", "HEAD"], None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn missing_git_preserves_the_install_and_reports_the_spawn_error() {
        let fixture = Fixture::new();
        let dir = fixture.0.join("comfyui");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.py"), "working source").unwrap();
        let error = prepare_checkout(
            &fixture.0.join("missing-git.exe"),
            &dir,
            "unused",
            "HEAD",
            None,
        )
        .await
        .err()
        .unwrap();
        assert!(error.contains("initialise Git"), "{error}");
        assert!(error.contains("missing-git.exe"), "{error}");
        assert_eq!(
            fs::read_to_string(dir.join("main.py")).unwrap(),
            "working source"
        );
        assert!(!dir.join(".git").exists());
    }

    #[tokio::test]
    async fn zip_install_retries_failed_fetch_without_origin_and_preserves_user_data() {
        let fixture = Fixture::new();
        let (git, source, revision) = source_repo(&fixture.0).await;
        let dir = fixture.0.join("ComfyUI with spaces");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.py"), "working source").unwrap();
        let user_files = [
            "models/model.bin",
            "input/photo.png",
            "output/result.png",
            "custom_nodes/user_node/__init__.py",
            "user/settings.json",
            "extra_model_paths.yaml",
        ];
        for file in user_files {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "keep this data").unwrap();
        }
        let url = source.to_str().unwrap();
        let error = prepare_checkout(&git, &dir, url, "refs/heads/missing-test-ref", None)
            .await
            .err()
            .unwrap();
        assert!(error.contains("missing-test-ref"), "{error}");
        assert!(error.contains("Git exited"), "{error}");
        assert_eq!(
            fs::read_to_string(dir.join("main.py")).unwrap(),
            "working source"
        );
        assert!(!dir
            .join(crate::comfyui_version::UPDATE_PENDING_MARKER)
            .exists());
        // A failed fetch has already created .git, but no origin exists.
        let checkout = prepare_checkout(&git, &dir, url, &revision, None)
            .await
            .unwrap();
        assert_eq!(checkout.method, "git-fetch");
        assert_eq!(
            fs::read_to_string(dir.join("main.py")).unwrap(),
            "working source"
        );
        checkout.apply().await.unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("main.py")).unwrap().trim(),
            "new source"
        );
        for file in user_files {
            assert_eq!(
                fs::read_to_string(dir.join(file)).unwrap(),
                "keep this data",
                "{file}"
            );
        }
    }

    #[tokio::test]
    async fn existing_checkout_uses_prepared_commit_and_removes_obsolete_source() {
        let fixture = Fixture::new();
        let (git, source, first_revision) = source_repo(&fixture.0).await;
        let dir = fixture.0.join("comfyui");
        fs::create_dir_all(&dir).unwrap();
        let checkout =
            prepare_checkout(&git, &dir, source.to_str().unwrap(), &first_revision, None)
                .await
                .unwrap();
        assert_eq!(checkout.method, "git-init");
        checkout.apply().await.unwrap();
        run_git(
            &git,
            &[
                "-C",
                dir.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                "invalid-origin",
            ],
            None,
        )
        .await
        .unwrap();
        fs::remove_file(source.join("obsolete.py")).unwrap();
        fs::write(source.join("main.py"), "updated source\n").unwrap();
        let revision = commit(&git, &source).await;
        let checkout = prepare_checkout(&git, &dir, source.to_str().unwrap(), &revision, None)
            .await
            .unwrap();
        assert!(dir.join("obsolete.py").exists());
        fs::write(dir.join(".git/FETCH_HEAD"), format!("{first_revision}\n")).unwrap();
        checkout.apply().await.unwrap();
        assert!(!dir.join("obsolete.py").exists());
        assert_eq!(
            fs::read_to_string(dir.join("main.py")).unwrap().trim(),
            "updated source"
        );
        assert_eq!(
            run_git(
                &git,
                &["-C", dir.to_str().unwrap(), "remote", "get-url", "origin"],
                None
            )
            .await
            .unwrap(),
            "invalid-origin"
        );
    }

    #[cfg(windows)]
    #[test]
    fn git_discovery_handles_spaces_and_missing_path_entries() {
        let fixture = Fixture::new();
        let git = fixture.0.join("Program Files/Git/cmd/git.exe");
        fs::create_dir_all(git.parent().unwrap()).unwrap();
        fs::write(&git, "fixture").unwrap();
        assert_eq!(
            first_existing(vec![fixture.0.join("missing/git.exe"), git.clone()]),
            Some(git)
        );
        assert_eq!(first_existing(Vec::new()), None);
    }

    #[cfg(windows)]
    #[test]
    fn invalid_git_download_is_rejected_before_extraction() {
        let fixture = Fixture::new();
        let staging = fixture.0.join("staging");
        assert!(extract_mingit(b"truncated or tampered download", &staging)
            .unwrap_err()
            .contains("checksum"));
        assert!(!staging.exists());
    }

    #[cfg(windows)]
    #[test]
    fn failed_git_publish_restores_the_previous_install() {
        let fixture = Fixture::new();
        let destination = managed_git_dir(&fixture.0);
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("sentinel"), "previous Git").unwrap();
        assert!(publish_mingit(&fixture.0.join("missing-staging"), &destination).is_err());
        assert_eq!(
            fs::read_to_string(destination.join("sentinel")).unwrap(),
            "previous Git"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "downloads the official MinGit archive and tests its HTTPS transport"]
    async fn managed_git_download_smoke() {
        let fixture = Fixture::new();
        let git = ensure_windows_git(&fixture.0, &reqwest::Client::new(), None, Vec::new())
            .await
            .unwrap();
        assert!(git.starts_with(&fixture.0));
        verify_git(&git).await.unwrap();
        // Exercise git-remote-https and its DLLs, not only git.exe --version.
        let result = run_git(
            &git,
            &[
                "ls-remote",
                "https://github.com/Comfy-Org/ComfyUI.git",
                "HEAD",
            ],
            None,
        )
        .await
        .unwrap();
        assert!(result.contains("HEAD"), "{result}");
        let reused =
            ensure_windows_git(&fixture.0, &reqwest::Client::new(), None, vec![git.clone()])
                .await
                .unwrap();
        assert_eq!(reused, git);
    }
}
