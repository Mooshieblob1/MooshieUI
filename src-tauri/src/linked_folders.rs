//! Linked gallery folders: folders outside the gallery that the desktop
//! gallery shows in place, without copying them in (Settings > Gallery).
//!
//! Each linked file is named by an opaque token that passes the gallery's
//! single-file-name checks, so every per-image command and URI scheme can
//! take it where it takes a gallery file name:
//!
//! `lnk~{folder id}~{hex of the path inside the folder}.{extension}`
//!
//! The folder id is a hash of the folder path as configured, so a token keeps
//! naming the same file across restarts and when other folders are added or
//! removed, and favourites or ratings stored against it stay put. The path is
//! hex rather than base64 because base64's `_` would let a token contain the
//! `__` that gallery file names use as a separator. The extension is repeated
//! at the end because callers pick JXL and video handling from the name.
//!
//! Desktop only: the browser server never resolves these tokens, so a LAN
//! account cannot read the host's folders through them.

use std::path::{Component, Path, PathBuf};

pub const TOKEN_PREFIX: &str = "lnk~";

/// File types a linked folder contributes. Videos are left out: their
/// thumbnail is a poster sidecar that only the save pipeline writes.
const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "jxl"];

/// How deep below a linked folder the listing descends.
const MAX_DEPTH: usize = 32;

/// Most files all linked folders together contribute to one listing. Every
/// entry costs an IPC row, a thumbnail and a metadata read in the gallery, so
/// a linked drive root stops here instead of stalling the app.
pub const MAX_FILES: usize = 50_000;

/// One file found in a linked folder.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedEntry {
    pub token: String,
    /// The folder as configured.
    pub folder: String,
    /// The file's path inside the folder, `/`-separated.
    pub display_name: String,
    pub size_bytes: u64,
    pub modified_ms: u64,
}

/// Stable id for a configured folder: 64-bit FNV-1a of its trimmed path.
/// `DefaultHasher` is not used because its output may change between Rust
/// releases, which would orphan every stored favourite and rating.
pub fn folder_id(folder: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in folder.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn is_token(name: &str) -> bool {
    name.starts_with(TOKEN_PREFIX)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

fn allowed_extension(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// `rel` as `/`-joined parts, or `None` if it is not a plain relative path.
fn relative_key(rel: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_string()),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The token for the file at `rel` inside linked folder `folder`.
pub fn make_token(folder: &str, rel: &Path) -> Option<String> {
    let ext = allowed_extension(rel)?;
    let key = relative_key(rel)?;
    Some(format!(
        "{TOKEN_PREFIX}{}~{}.{ext}",
        folder_id(folder),
        hex_encode(key.as_bytes())
    ))
}

/// Split a token into its folder id and the relative path it names.
fn parse_token(token: &str) -> Option<(&str, String)> {
    let rest = token.strip_prefix(TOKEN_PREFIX)?;
    let (id, tail) = rest.split_once('~')?;
    let (hex, ext) = tail.rsplit_once('.')?;
    let rel = String::from_utf8(hex_decode(hex)?).ok()?;
    // The visible extension must be the real one, or a caller deciding
    // "is this JXL?" from the name would be lied to.
    if allowed_extension(Path::new(&rel))? != ext {
        return None;
    }
    Some((id, rel))
}

/// Resolve `token` against the configured `folders`. `None` unless it names an
/// existing regular file of an allowed type that is really inside its folder:
/// `..` parts are refused and the check is made on canonical paths, so neither
/// a crafted token nor a symlink inside the folder can reach anything else.
pub fn resolve_in(folders: &[String], token: &str) -> Option<PathBuf> {
    let (id, rel) = parse_token(token)?;
    let folder = folders.iter().find(|f| folder_id(f) == id)?;
    let root = PathBuf::from(folder.trim());
    let mut path = root.clone();
    for part in rel.split('/') {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(_)), None) if !part.contains(['\\', ':', '\0']) => {
                path.push(part)
            }
            _ => return None,
        }
    }
    let canonical_root = root.canonicalize().ok()?;
    let canonical = path.canonicalize().ok()?;
    (canonical.starts_with(&canonical_root) && canonical.is_file()).then_some(path)
}

/// Resolve a token against the folders configured in Settings.
pub fn resolve(token: &str) -> Option<PathBuf> {
    resolve_in(&crate::config::linked_gallery_folders(), token)
}

fn modified_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Walk `dir` (a folder inside linked folder `folder`, rooted at `root`).
/// Symlinks are skipped, as are dot-directories (`.git`, thumbnail caches)
/// and `exclude`, the gallery itself, whose files the gallery already lists.
fn walk(
    folder: &str,
    root: &Path,
    dir: &Path,
    depth: usize,
    exclude: Option<&Path>,
    out: &mut Vec<LinkedEntry>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            let excluded = exclude
                .is_some_and(|ex| path.canonicalize().is_ok_and(|canonical| canonical == ex));
            if !hidden && !excluded && depth < MAX_DEPTH {
                walk(folder, root, &path, depth + 1, exclude, out);
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let Some(token) = make_token(folder, rel) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        out.push(LinkedEntry {
            token,
            folder: folder.to_string(),
            display_name: relative_key(rel).unwrap_or_default(),
            size_bytes: meta.len(),
            modified_ms: modified_ms(&meta),
        });
    }
}

/// Every image in `folders`, up to MAX_FILES in all. A folder that is missing
/// or unreadable contributes nothing rather than failing the listing.
pub fn list_in(folders: &[String], exclude: Option<&Path>) -> Vec<LinkedEntry> {
    let exclude = exclude.and_then(|p| p.canonicalize().ok());
    let mut out = Vec::new();
    for folder in folders {
        let root = PathBuf::from(folder.trim());
        if !root.is_dir() {
            continue;
        }
        walk(folder, &root, &root, 0, exclude.as_deref(), &mut out);
        if out.len() >= MAX_FILES {
            log::warn!(
                "linked folders: stopped listing at {MAX_FILES} files; link narrower folders to see the rest"
            );
            break;
        }
    }
    out
}

/// Every image in the folders configured in Settings, minus the gallery itself.
pub fn list() -> Vec<LinkedEntry> {
    let gallery = crate::config::gallery_dir();
    list_in(&crate::config::linked_gallery_folders(), gallery.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mooshieui-linked-{}-{}-{}",
            name,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn folder_string(dir: &Path) -> String {
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn token_round_trips_and_passes_gallery_name_checks() {
        let token = make_token("/pics", Path::new("sub dir/Image 1.PNG")).unwrap();
        assert!(is_token(&token));
        assert!(token.ends_with(".png"));
        assert!(!token.contains("__"));
        assert!(crate::commands::api::validate_gallery_filename(&token).is_ok());
        let (id, rel) = parse_token(&token).unwrap();
        assert_eq!(id, folder_id("/pics"));
        assert_eq!(rel, "sub dir/Image 1.PNG");
    }

    #[test]
    fn folder_id_is_stable() {
        // Stored favourites and ratings depend on this never changing.
        assert_eq!(folder_id("/pics"), folder_id("  /pics "));
        assert_eq!(folder_id(""), "cbf29ce484222325");
        assert_ne!(folder_id("/pics"), folder_id("/pics2"));
    }

    #[test]
    fn rejects_unsupported_or_mismatched_names() {
        assert!(make_token("/pics", Path::new("clip.mp4")).is_none());
        assert!(make_token("/pics", Path::new("../a.png")).is_none());
        let token = make_token("/pics", Path::new("a.png")).unwrap();
        let lying = token.replace(".png", ".jxl");
        assert!(parse_token(&lying).is_none());
        assert!(parse_token("lnk~abc~zz.png").is_none());
        assert!(parse_token("a.png").is_none());
    }

    #[test]
    fn lists_and_resolves_nested_images() {
        let root = scratch("list");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join(".cache")).unwrap();
        std::fs::write(root.join("top.png"), b"x").unwrap();
        std::fs::write(root.join("a/b/deep.webp"), b"x").unwrap();
        std::fs::write(root.join("a/notes.txt"), b"x").unwrap();
        std::fs::write(root.join(".cache/hidden.png"), b"x").unwrap();
        let folders = vec![folder_string(&root)];

        let mut names: Vec<_> = list_in(&folders, None)
            .into_iter()
            .map(|e| e.display_name)
            .collect();
        names.sort();
        assert_eq!(names, ["a/b/deep.webp", "top.png"]);

        for entry in list_in(&folders, None) {
            let path = resolve_in(&folders, &entry.token).unwrap();
            assert!(path.is_file());
            assert_eq!(entry.folder, folders[0]);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skips_the_gallery_directory() {
        let root = scratch("exclude");
        std::fs::create_dir_all(root.join("gallery")).unwrap();
        std::fs::write(root.join("gallery/own.png"), b"x").unwrap();
        std::fs::write(root.join("mine.png"), b"x").unwrap();
        let folders = vec![folder_string(&root)];
        let names: Vec<_> = list_in(&folders, Some(&root.join("gallery")))
            .into_iter()
            .map(|e| e.display_name)
            .collect();
        assert_eq!(names, ["mine.png"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn does_not_resolve_outside_its_folder() {
        let base = scratch("escape");
        let linked = base.join("linked");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(base.join("secret.png"), b"x").unwrap();
        let folders = vec![folder_string(&linked)];

        // A crafted token whose path climbs out of the folder.
        let escape = format!(
            "{TOKEN_PREFIX}{}~{}.png",
            folder_id(&folders[0]),
            hex_encode(b"../secret.png")
        );
        assert!(resolve_in(&folders, &escape).is_none());

        // A token for a folder that is no longer linked.
        let other = make_token("/not/linked", Path::new("secret.png")).unwrap();
        assert!(resolve_in(&folders, &other).is_none());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("secret.png"), linked.join("link.png")).unwrap();
            let token = make_token(&folders[0], Path::new("link.png")).unwrap();
            assert!(resolve_in(&folders, &token).is_none());
            assert!(list_in(&folders, None).is_empty());
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
