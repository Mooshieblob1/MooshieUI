//! On-disk cache for gallery thumbnails.
//!
//! A thumbnail is a full decode of the original (a multi-megapixel PNG or JXL)
//! plus a WebP encode. Without a cache every tile that scrolls into view pays
//! that again, on every launch, which is what makes a gallery of thousands of
//! images slow to browse. Entries are keyed by the source path, its size and
//! modification time and the requested size, so an edited or replaced file
//! never serves a stale thumbnail; entries for deleted files are simply never
//! asked for again and age out when the cache is pruned.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;

/// Prune once the cache holds more than this.
const MAX_CACHE_BYTES: u64 = 1024 * 1024 * 1024;
/// Prune down to this, so one prune buys many writes.
const PRUNE_TARGET_BYTES: u64 = MAX_CACHE_BYTES / 4 * 3;
/// Check the cache size on the first write of a run and every this many after.
const WRITES_PER_PRUNE_CHECK: u32 = 512;

static WRITES: AtomicU32 = AtomicU32::new(0);

/// Where thumbnails are cached: `{app data}/thumbnail_cache`.
pub fn default_dir() -> Option<PathBuf> {
    crate::config::app_data_dir().map(|dir| dir.join("thumbnail_cache"))
}

/// `Cache-Control` for a thumbnail response. Gallery URLs carry a `v=` version
/// built from the file's size and modification time, so such a response can be
/// kept by the webview for good; anything else is revalidated as before.
pub fn cache_control_for_query(query: &str) -> &'static str {
    if query.split('&').any(|p| p.starts_with("v=") && p.len() > 2) {
        "private, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// Cache file name for `source` at `max_size`, or `None` when the file can't be read.
fn entry_name(source: &Path, max_size: u32) -> Option<String> {
    let meta = std::fs::metadata(source).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(source.to_string_lossy().as_bytes());
    hasher.update([0]);
    hasher.update(meta.len().to_le_bytes());
    hasher.update(modified.to_le_bytes());
    hasher.update(max_size.to_le_bytes());
    Some(format!("{}.webp", hex::encode(&hasher.finalize()[..16])))
}

/// The cached thumbnail of `source` at `max_size`, generating it with `generate`
/// on a miss. Cache failures are never errors: the thumbnail is still returned.
pub fn get_or_generate(
    cache_dir: Option<&Path>,
    source: &Path,
    max_size: u32,
    generate: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let entry = cache_dir.and_then(|dir| Some(dir.join(entry_name(source, max_size)?)));
    if let Some(entry) = &entry {
        if let Ok(bytes) = std::fs::read(entry) {
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
    }
    let bytes = generate()?;
    if let (Some(entry), Some(dir)) = (&entry, cache_dir) {
        store(dir, entry, &bytes);
    }
    Ok(bytes)
}

fn store(dir: &Path, entry: &Path, bytes: &[u8]) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // Write beside the entry and rename, so a concurrent reader never sees a
    // half-written thumbnail.
    let tmp = entry.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, entry).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    if WRITES.fetch_add(1, Ordering::Relaxed) % WRITES_PER_PRUNE_CHECK == 0 {
        prune(dir, MAX_CACHE_BYTES, PRUNE_TARGET_BYTES);
    }
}

/// When `dir` holds more than `max_bytes`, delete its oldest entries until it
/// holds at most `target_bytes`.
fn prune(dir: &Path, max_bytes: u64, target_bytes: u64) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(SystemTime, u64, PathBuf)> = read
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, meta.len(), entry.path()))
        })
        .collect();
    let mut total: u64 = entries.iter().map(|(_, len, _)| len).sum();
    if total <= max_bytes {
        return;
    }
    entries.sort_by_key(|(modified, _, _)| *modified);
    for (_, len, path) in entries {
        if total <= target_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mooshie_thumb_cache_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_versioned_urls_are_cached_by_the_webview() {
        assert_eq!(cache_control_for_query("size=384"), "no-cache");
        assert_eq!(cache_control_for_query("size=384&v="), "no-cache");
        assert_eq!(cache_control_for_query(""), "no-cache");
        assert!(cache_control_for_query("token=abc&v=17-42&size=384").contains("immutable"));
    }

    #[test]
    fn second_request_is_served_from_the_cache() {
        let dir = temp_dir("hit");
        let source = dir.join("a.png");
        std::fs::write(&source, b"original").unwrap();
        let cache = dir.join("cache");
        let calls = Cell::new(0);
        let generate = || {
            calls.set(calls.get() + 1);
            Ok(b"thumb".to_vec())
        };
        assert_eq!(
            get_or_generate(Some(&cache), &source, 384, generate).unwrap(),
            b"thumb"
        );
        assert_eq!(
            get_or_generate(Some(&cache), &source, 384, generate).unwrap(),
            b"thumb"
        );
        assert_eq!(calls.get(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn size_and_file_changes_miss_the_cache() {
        let dir = temp_dir("miss");
        let source = dir.join("a.png");
        std::fs::write(&source, b"original").unwrap();
        let cache = dir.join("cache");
        let calls = Cell::new(0);
        let generate = || {
            calls.set(calls.get() + 1);
            Ok(vec![calls.get() as u8])
        };
        get_or_generate(Some(&cache), &source, 384, generate).unwrap();
        get_or_generate(Some(&cache), &source, 480, generate).unwrap();
        assert_eq!(calls.get(), 2);
        // A replaced file (different length) must not reuse the old thumbnail.
        std::fs::write(&source, b"a different, longer original").unwrap();
        assert_eq!(
            get_or_generate(Some(&cache), &source, 384, generate).unwrap(),
            vec![3]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failures_are_not_cached_and_no_cache_dir_still_generates() {
        let dir = temp_dir("fail");
        let source = dir.join("a.png");
        std::fs::write(&source, b"original").unwrap();
        let cache = dir.join("cache");
        assert!(get_or_generate(Some(&cache), &source, 384, || Err("bad".into())).is_err());
        assert_eq!(
            get_or_generate(Some(&cache), &source, 384, || Ok(b"ok".to_vec())).unwrap(),
            b"ok"
        );
        assert_eq!(
            get_or_generate(None, &source, 384, || Ok(b"plain".to_vec())).unwrap(),
            b"plain"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_removes_oldest_entries_down_to_the_target() {
        let dir = temp_dir("prune");
        for i in 0..5u64 {
            let path = dir.join(format!("{i}.webp"));
            std::fs::write(&path, vec![0u8; 100]).unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i))
                .unwrap();
        }
        // Under the cap: nothing goes.
        prune(&dir, 500, 300);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 5);
        prune(&dir, 499, 300);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["2.webp", "3.webp", "4.webp"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
