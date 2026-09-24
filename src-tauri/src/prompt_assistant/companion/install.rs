//! Unmodified official releases, installed privately on first sign-in.
use super::{error, root, Scratch};
use crate::{error::AppError, state::AppState};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncWriteExt;

const LIMIT: u64 = 768 * 1024 * 1024;
const EXPANDED_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Deserialize)]
pub(super) struct Package {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub binary: String,
}

fn package(provider: &str) -> Result<Package, AppError> {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("packages.json"))?;
    let key = if provider == "gemini-cli" {
        format!(
            "antigravity-{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    } else {
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    };
    serde_json::from_value(manifest[&key].clone()).map_err(|_| {
        error(if provider == "gemini-cli" {
            "Google's Antigravity companion is not published for this platform. On macOS, Google currently provides an Apple Silicon build only."
        } else {
            "Companion-tool setup is not available for this platform."
        })
    })
}

fn directory(provider: &str, package: &Package) -> Result<PathBuf, AppError> {
    Ok(root(provider)?.join(format!("tool-{}", package.version)))
}

pub(super) fn executable(provider: &str) -> Result<PathBuf, AppError> {
    let spec = package(provider)?;
    let dir = directory(provider, &spec)?;
    if std::fs::read_to_string(dir.join(".verified"))
        .ok()
        .as_deref()
        != Some(&spec.sha256)
    {
        return Err(error(
            "Sign in from Prompt Assistant settings to install the companion tool.",
        ));
    }
    let path = dir.join(spec.binary);
    if !path.is_file() {
        return Err(error(
            "Companion tool is missing. Sign in again to repair its installation.",
        ));
    }
    Ok(path)
}

pub(super) fn node_path() -> Result<PathBuf, AppError> {
    let path: String =
        serde_json::from_slice(&std::fs::read(root("gemini-cli")?.join("node.json"))?)?;
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_file() {
        return Err(error(
            "Node.js is missing. Wait for startup setup, then sign in again.",
        ));
    }
    Ok(path)
}

pub(super) async fn ensure(state: &AppState, provider: &str) -> Result<(), AppError> {
    if provider == "gemini-cli" {
        let node = state.media_tools.path("node").ok_or_else(|| {
            error("Node.js is being prepared. Wait for startup setup, then sign in again.")
        })?;
        tokio::fs::write(
            root(provider)?.join("node.json"),
            serde_json::to_vec(&node)?,
        )
        .await?;
    }
    if executable(provider).is_ok() {
        return Ok(());
    }
    let spec = package(provider)?;
    let staging = Scratch::new(&root(provider)?, "install")?;
    let archive = staging.0.join("download");
    let mut response = state
        .http_client
        .get(&spec.url)
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .map_err(|_| {
            error(
                "Could not download the official companion tool. Check your connection and retry.",
            )
        })?
        .error_for_status()
        .map_err(|_| error("The companion download server is unavailable. Retry shortly."))?;
    if response.content_length().is_some_and(|n| n > LIMIT) {
        return Err(error("Companion download exceeds the size limit."));
    }
    let mut file = tokio::fs::File::create(&archive).await?;
    let mut size = 0;
    let mut hash = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| error("Companion download interrupted. Please retry."))?
    {
        size += chunk.len() as u64;
        if size > LIMIT {
            return Err(error("Companion download exceeds the size limit."));
        }
        hash.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if hex::encode(hash.finalize()) != spec.sha256 {
        return Err(error(
            "Companion download failed checksum verification. Please retry.",
        ));
    }
    // Extraction is bounded and the archive is already authenticated. Keep the
    // staging owner inside the blocking task so cancellation cannot race cleanup.
    let destination = directory(provider, &spec)?;
    tokio::task::spawn_blocking(move || {
        let extracted = staging.0.join("extracted");
        extract(&archive, &extracted, &spec)?;
        std::fs::write(extracted.join(".verified"), &spec.sha256)?;
        if destination.exists() {
            std::fs::remove_dir_all(&destination)?;
        }
        std::fs::rename(extracted, destination)?;
        Ok::<_, AppError>(())
    })
    .await
    .map_err(|_| error("Companion installation stopped."))??;
    Ok(())
}

fn copy(mut source: impl Read, destination: &Path, remaining: &mut u64) -> Result<(), AppError> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(destination)?;
    let count = std::io::copy(&mut source.by_ref().take(*remaining + 1), &mut file)?;
    if count > *remaining {
        return Err(error("Companion archive exceeds the size limit."));
    }
    *remaining -= count;
    file.flush()?;
    Ok(())
}

fn extract(archive: &Path, dest: &Path, spec: &Package) -> Result<(), AppError> {
    std::fs::create_dir(dest)?;
    let source = std::fs::File::open(archive)?;
    let mut remaining = EXPANDED_LIMIT;
    if spec.url.ends_with(".zip") {
        let mut zip =
            zip::ZipArchive::new(source).map_err(|_| error("Invalid companion ZIP archive."))?;
        if zip.len() > 10_000 {
            return Err(error("Too many companion archive entries."));
        }
        for i in 0..zip.len() {
            let entry = zip
                .by_index(i)
                .map_err(|_| error("Invalid companion ZIP entry."))?;
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| error("Unsafe companion archive path."))?;
            if entry.is_symlink() {
                return Err(error("Unexpected companion archive symlink."));
            }
            if entry.is_dir() {
                continue;
            }
            let output = dest.join(relative);
            let mode = entry.unix_mode();
            copy(entry, &output, &mut remaining)?;
            #[cfg(unix)]
            if mode.is_some_and(|m| m & 0o111 != 0) {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700))?;
            }
            #[cfg(not(unix))]
            let _ = mode;
        }
    } else {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let mut tar =
                tar::Archive::new(flate2::read::GzDecoder::new(source).take(EXPANDED_LIMIT + 1));
            for entry in tar.entries()? {
                let entry = entry?;
                if entry.path()?.as_ref() == Path::new(&spec.binary) {
                    if !entry.header().entry_type().is_file() {
                        return Err(error("Invalid companion executable."));
                    }
                    copy(entry, &dest.join(&spec.binary), &mut remaining)?;
                }
            }
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                dest.join(&spec.binary),
                std::fs::Permissions::from_mode(0o700),
            )?;
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        return Err(error("Unsupported companion archive format."));
    }
    if !dest.join(&spec.binary).is_file() {
        return Err(error("Companion archive is missing its executable."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Google's ZIP archives also contain the runtime used by the server.
        // Some ZIP producers omit Unix modes, so mark known binaries explicitly.
        for name in [&spec.binary, "localharness_external"] {
            let path = dest.join(name);
            if path.is_file() {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_package() -> Package {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("packages.json")).unwrap();
        serde_json::from_value(manifest["antigravity-windows-x86_64"].clone()).unwrap()
    }
    #[test]
    fn archive_cannot_write_outside_installation_or_overrun_budget() {
        let scratch = Scratch::new(&std::env::temp_dir(), "companion-archive-test").unwrap();
        let archive = scratch.0.join("bad.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"not allowed").unwrap();
        zip.finish().unwrap();
        let spec = test_package();
        assert!(extract(&archive, &scratch.0.join("install"), &spec).is_err());
        assert!(!scratch.0.join("escape").exists());
        assert!(copy(&b"oversized"[..], &scratch.0.join("bounded"), &mut 2).is_err());
    }

    #[test]
    fn complete_bundle_and_nested_modules_are_preserved() {
        let scratch = Scratch::new(&std::env::temp_dir(), "companion-bundle-test").unwrap();
        let archive = scratch.0.join("bundle.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let spec = test_package();
        for name in [spec.binary.as_str(), "chunks/module.js", "LICENSE"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(name.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let target = scratch.0.join("install");
        extract(&archive, &target, &spec).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("chunks/module.js")).unwrap(),
            "chunks/module.js"
        );
        assert!(target.join("LICENSE").is_file());
    }

    #[test]
    fn releases_are_pinned_for_desktop_platforms() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("packages.json")).unwrap();
        for key in [
            "windows-x86_64",
            "windows-aarch64",
            "macos-x86_64",
            "macos-aarch64",
            "linux-x86_64",
            "linux-aarch64",
            "antigravity-windows-x86_64",
            "antigravity-windows-aarch64",
            "antigravity-macos-aarch64",
            "antigravity-linux-x86_64",
            "antigravity-linux-aarch64",
        ] {
            let p: Package = serde_json::from_value(manifest[key].clone()).unwrap();
            assert_eq!(p.sha256.len(), 64);
            assert!(
                p.url.starts_with("https://github.com/")
                    || p.url
                        .starts_with("https://dl.google.com/agy-extensions/releases/")
            );
            assert!(!p.url.contains("latest"));
            assert!(p.binary.find(['/', '\\']).is_none());
        }
    }
}
