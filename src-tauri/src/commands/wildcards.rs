//! Wildcard files: `<app data>/wildcards/**/*.{txt,yaml,yml,json}`.
//!
//! The backend only lists, saves and deletes the files; parsing and `__name__`
//! expansion happen in the frontend (`src/lib/utils/wildcards.ts`) because the
//! prompt is built there. Compiled in both builds: the Tauri wrappers are gated
//! on `desktop`, and `webserver.rs` calls the plain functions.

use crate::error::AppError;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

/// File types the frontend knows how to read.
const EXTENSIONS: &[&str] = &["txt", "yaml", "yml", "json"];
/// Big community packs run to a few hundred files; this keeps a stray huge
/// folder from freezing the app.
const MAX_FILES: usize = 2000;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DEPTH: usize = 6;

#[derive(Debug, Serialize, PartialEq)]
pub struct WildcardFile {
    /// Path under the wildcards folder, `/`-separated, with its extension.
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct WildcardListing {
    /// Absolute folder path, for "Open folder".
    pub dir: String,
    pub files: Vec<WildcardFile>,
    /// True when files were skipped for count or size limits.
    pub truncated: bool,
}

pub fn wildcards_dir() -> Result<PathBuf, AppError> {
    let base = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("App data folder is not available".into()))?;
    Ok(base.join("wildcards"))
}

fn has_allowed_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Turn a user-supplied relative path into one inside `root`, or refuse it.
fn resolve_relative(root: &Path, rel: &str) -> Result<PathBuf, AppError> {
    let rel = rel.trim().replace('\\', "/");
    if rel.is_empty() || rel.len() > 200 {
        return Err(AppError::Other("Invalid wildcard file name".into()));
    }
    let candidate = Path::new(&rel);
    let mut out = root.to_path_buf();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.starts_with('.')
                    || part.chars().any(|c| {
                        c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|')
                    })
                {
                    return Err(AppError::Other("Invalid wildcard file name".into()));
                }
                out.push(part.as_ref());
            }
            _ => return Err(AppError::Other("Invalid wildcard file name".into())),
        }
    }
    if !has_allowed_extension(&out) {
        return Err(AppError::Other(
            "Wildcard files must be .txt, .yaml, .yml or .json".into(),
        ));
    }
    Ok(out)
}

fn collect(
    root: &Path,
    dir: &Path,
    depth: usize,
    files: &mut Vec<WildcardFile>,
    truncated: &mut bool,
) -> Result<(), AppError> {
    let mut entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(read) => read.filter_map(Result::ok).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if files.len() >= MAX_FILES {
            *truncated = true;
            return Ok(());
        }
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        // symlink_metadata: a link out of the folder is not followed.
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let path = entry.path();
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                collect(root, &path, depth + 1, files, truncated)?;
            } else {
                *truncated = true;
            }
        } else if meta.is_file() && has_allowed_extension(&path) {
            if meta.len() > MAX_FILE_BYTES {
                *truncated = true;
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let content = String::from_utf8_lossy(&bytes)
                .trim_start_matches('\u{feff}')
                .to_string();
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push(WildcardFile { path: rel, content });
        }
    }
    Ok(())
}

pub fn list_in(root: &Path) -> Result<WildcardListing, AppError> {
    let mut files = Vec::new();
    let mut truncated = false;
    collect(root, root, 0, &mut files, &mut truncated)?;
    Ok(WildcardListing {
        dir: root.to_string_lossy().into_owned(),
        files,
        truncated,
    })
}

pub fn save_in(root: &Path, rel: &str, content: &str) -> Result<(), AppError> {
    if content.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::Other("Wildcard file is too large".into()));
    }
    let path = resolve_relative(root, rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

pub fn delete_in(root: &Path, rel: &str) -> Result<(), AppError> {
    let path = resolve_relative(root, rel)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn list() -> Result<WildcardListing, AppError> {
    let root = wildcards_dir()?;
    std::fs::create_dir_all(&root)?;
    list_in(&root)
}

pub fn save(rel: &str, content: &str) -> Result<(), AppError> {
    save_in(&wildcards_dir()?, rel, content)
}

pub fn delete(rel: &str) -> Result<(), AppError> {
    delete_in(&wildcards_dir()?, rel)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn list_wildcards() -> Result<WildcardListing, AppError> {
    tokio::task::spawn_blocking(list)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn save_wildcard(path: String, content: String) -> Result<(), AppError> {
    save(&path, &content)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn delete_wildcard(path: String) -> Result<(), AppError> {
    delete(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mooshie-wildcards-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_supported_files_recursively_with_slash_paths() {
        let root = temp_root("list");
        std::fs::create_dir_all(root.join("hair")).unwrap();
        std::fs::write(root.join("colors.txt"), "\u{feff}red\nblue").unwrap();
        std::fs::write(root.join("hair/styles.yaml"), "a:\n  - b").unwrap();
        std::fs::write(root.join("notes.md"), "skip").unwrap();
        std::fs::write(root.join(".hidden.txt"), "skip").unwrap();
        let listing = list_in(&root).unwrap();
        let paths: Vec<_> = listing.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["colors.txt", "hair/styles.yaml"]);
        assert_eq!(listing.files[0].content, "red\nblue");
        assert!(!listing.truncated);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn save_creates_folders_and_delete_removes() {
        let root = temp_root("save");
        save_in(&root, "pack/poses.txt", "standing").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("pack/poses.txt")).unwrap(),
            "standing"
        );
        delete_in(&root, "pack/poses.txt").unwrap();
        assert!(!root.join("pack/poses.txt").exists());
        // Deleting a missing file is not an error.
        delete_in(&root, "pack/poses.txt").unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_paths_that_leave_the_folder_or_have_other_types() {
        let root = temp_root("refuse");
        for bad in [
            "../x.txt",
            "/etc/passwd.txt",
            "a/../../x.txt",
            "x.exe",
            ".git/x.txt",
            "C:/x.txt",
            "",
        ] {
            assert!(save_in(&root, bad, "x").is_err(), "{bad} should be refused");
        }
        assert!(save_in(&root, "a\\b.txt", "x").is_ok());
        assert!(root.join("a/b.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
