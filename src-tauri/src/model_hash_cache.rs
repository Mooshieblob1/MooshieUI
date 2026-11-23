//! Remembered SHA-256 hashes of model files.
//!
//! Hashing a multi-GB checkpoint reads the whole file, and the Model Hub's
//! renamed-file detection (`find_model_by_hash`) hashes every file in a model
//! folder, once per recommended file it looks for. Without a cache that ran on
//! every launch and re-read the same folders many times over, in parallel.
//!
//! A hash is reused while the file keeps the same size and modification time
//! (the same rule A1111 and Forge use), kept in memory and in
//! `model-hash-cache.json` under the app data directory so the next launch
//! skips it too. Two callers asking for the same file at once share one read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

const CACHE_FILE: &str = "model-hash-cache.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    size: u64,
    /// Modification time in nanoseconds since the Unix epoch (0 if unknown).
    mtime_ns: u128,
    sha256: String,
}

#[derive(Default)]
struct Cache {
    loaded: bool,
    entries: HashMap<PathBuf, Entry>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// One lock per file being hashed, so concurrent lookups of the same file wait
/// for the first read instead of reading it again.
fn in_flight() -> &'static Mutex<HashMap<PathBuf, Arc<Mutex<()>>>> {
    static IN_FLIGHT: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_path() -> Option<PathBuf> {
    crate::config::app_data_dir().map(|dir| dir.join(CACHE_FILE))
}

fn stamp(path: &Path) -> std::io::Result<(u64, u128)> {
    let meta = std::fs::metadata(path)?;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok((meta.len(), mtime_ns))
}

fn key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn ensure_loaded(cache: &mut Cache) {
    if cache.loaded {
        return;
    }
    cache.loaded = true;
    let Some(file) = cache_path() else { return };
    let Ok(bytes) = std::fs::read(&file) else {
        return;
    };
    match serde_json::from_slice::<HashMap<PathBuf, Entry>>(&bytes) {
        // Files deleted since the last run are dropped when the cache is next saved.
        Ok(entries) => cache.entries = entries,
        Err(e) => log::warn!("Ignoring unreadable {}: {}", file.display(), e),
    }
}

fn lookup(path: &Path, size: u64, mtime_ns: u128) -> Option<String> {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    ensure_loaded(&mut cache);
    cache
        .entries
        .get(path)
        .filter(|e| e.size == size && e.mtime_ns == mtime_ns && mtime_ns != 0)
        .map(|e| e.sha256.clone())
}

fn remember(path: PathBuf, entry: Entry) {
    let snapshot = {
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        ensure_loaded(&mut cache);
        cache.entries.insert(path, entry);
        cache.entries.retain(|p, _| p.is_file());
        cache.entries.clone()
    };
    let Some(file) = cache_path() else { return };
    let Ok(bytes) = serde_json::to_vec(&snapshot) else {
        return;
    };
    // Write then rename, so a crash mid-write never leaves a torn cache file.
    let tmp = file.with_extension("json.tmp");
    let saved = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &file));
    if let Err(e) = saved {
        log::warn!("Could not save {}: {}", file.display(), e);
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The file's SHA-256 (uppercase hex), read from the cache when the file is
/// unchanged since it was last hashed. Blocking: call from `spawn_blocking`.
pub fn sha256(path: &Path) -> Result<String, AppError> {
    let key = key(path);
    let (size, mtime_ns) = stamp(&key)?;
    if let Some(hash) = lookup(&key, size, mtime_ns) {
        return Ok(hash);
    }

    let file_lock = {
        let mut map = in_flight().lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let result = {
        let _guard = file_lock.lock().unwrap_or_else(|e| e.into_inner());
        // Another caller may have finished hashing this file while we waited.
        match lookup(&key, size, mtime_ns) {
            Some(hash) => Ok(hash),
            None => {
                let hash = crate::commands::api::sha256_uncached(&key)?;
                // Only remember the hash if the file did not change while it was read.
                if mtime_ns != 0 && stamp(&key).ok() == Some((size, mtime_ns)) {
                    remember(
                        key.clone(),
                        Entry {
                            size,
                            mtime_ns,
                            sha256: hash.clone(),
                        },
                    );
                }
                Ok(hash)
            }
        }
    };
    {
        let mut map = in_flight().lock().unwrap_or_else(|e| e.into_inner());
        if map
            .get(&key)
            .is_some_and(|lock| Arc::ptr_eq(lock, &file_lock) && Arc::strong_count(lock) <= 2)
        {
            map.remove(&key);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mooshie-hash-cache-{}-{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.safetensors");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        path
    }

    #[test]
    fn hashes_match_the_uncached_hash_and_are_reused() {
        let path = temp_file("reuse", b"hello world");
        let expected = crate::commands::api::sha256_uncached(&path).unwrap();
        assert_eq!(sha256(&path).unwrap(), expected);
        let (size, mtime) = stamp(&key(&path)).unwrap();
        assert_eq!(lookup(&key(&path), size, mtime), Some(expected.clone()));
        assert_eq!(sha256(&path).unwrap(), expected);
    }

    #[test]
    fn a_changed_file_is_hashed_again() {
        let path = temp_file("changed", b"first contents");
        let first = sha256(&path).unwrap();
        // A different size invalidates the entry even if the mtime does not move.
        std::fs::write(&path, b"second, longer contents").unwrap();
        let second = sha256(&path).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            second,
            crate::commands::api::sha256_uncached(&path).unwrap()
        );
    }

    #[test]
    fn concurrent_callers_get_the_same_hash() {
        let path = temp_file("concurrent", &vec![7u8; 1 << 20]);
        let expected = crate::commands::api::sha256_uncached(&path).unwrap();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let p = path.clone();
                std::thread::spawn(move || sha256(&p).unwrap())
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), expected);
        }
    }
}
