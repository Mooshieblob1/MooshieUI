//! Which prompt (and so which account) produced each ComfyUI output file.
//!
//! ComfyUI serves every file in its output directory to anyone who names it,
//! and names are guessable (`mooshie_video_00001_.mp4`). The browser-mode
//! `get_output_image` / `save_to_gallery` routes proxy that directory, so they
//! must only hand a LAN account the files its own prompts produced.
//!
//! The prompt cleanup reactor records every `type: "output"` file listed in a
//! ComfyUI `executed` event. The map is bounded two ways: entries expire after
//! [`OUTPUT_OWNER_TTL`], and at most [`MAX_OUTPUT_OWNERS`] are kept, oldest
//! evicted first. A file that is not (or no longer) recorded is refused to
//! every caller except the admin.
//!
//! Once [`OutputOwners::enable_persistence`] has run, the map is also kept in
//! [`OUTPUT_OWNERS_FILE`] under the app data dir, so an account can still
//! fetch its earlier outputs after the server restarts. Saves are debounced
//! (see [`OUTPUT_OWNER_SAVE_INTERVAL`]) and made again on graceful shutdown.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How many output files are remembered at once.
pub const MAX_OUTPUT_OWNERS: usize = 4096;

/// How long a recorded output stays readable by its owner through the proxy.
pub const OUTPUT_OWNER_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// File under the app data dir that keeps the map across restarts.
pub const OUTPUT_OWNERS_FILE: &str = "output_owners.json";

/// How often, at most, a changed map is written back to its file.
pub const OUTPUT_OWNER_SAVE_INTERVAL: Duration = Duration::from_secs(5);

/// Version of the on-disk format; a file with any other version is ignored.
const PERSIST_FORMAT_VERSION: u32 = 1;

/// Who produced one output file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRecord {
    /// The ComfyUI prompt id from the `executed` event.
    pub prompt_id: String,
    /// The owning account when it was known at record time. `None` is either
    /// the admin or a prompt whose ownership was not bound yet; the prompt id
    /// is checked again at read time in that case.
    pub owner: Option<String>,
    /// When the record stops being honoured. Monotonic, so a wall-clock jump
    /// while running neither extends nor cuts short a record's life.
    expires_at: Instant,
    /// Wall-clock record time in Unix seconds. An `Instant` means nothing to
    /// the next process, so this is what the saved file carries.
    recorded_unix: u64,
    seq: u64,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<String, OutputRecord>,
    /// Insertion order as (key, seq); a key re-recorded later leaves a stale
    /// pair behind, which eviction skips because its seq no longer matches.
    order: VecDeque<(String, u64)>,
    next_seq: u64,
}

impl Inner {
    /// Insert (or replace) one record, then evict expired entries and the
    /// oldest live ones until the map is back under `capacity`.
    #[allow(clippy::too_many_arguments)]
    fn insert(
        &mut self,
        key: String,
        prompt_id: String,
        owner: Option<String>,
        expires_at: Instant,
        recorded_unix: u64,
        now: Instant,
        capacity: usize,
    ) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.insert(
            key.clone(),
            OutputRecord {
                prompt_id,
                owner,
                expires_at,
                recorded_unix,
                seq,
            },
        );
        self.order.push_back((key, seq));

        // Drop expired entries from the front, then the oldest live ones
        // until the map is back under its cap.
        while let Some((front_key, front_seq)) = self.order.front().cloned() {
            let live = self
                .entries
                .get(&front_key)
                .filter(|record| record.seq == front_seq);
            let over_cap = self.entries.len() > capacity;
            match live {
                // A stale pair left behind by a re-record: nothing to evict.
                None => {
                    self.order.pop_front();
                }
                Some(record) if over_cap || now >= record.expires_at => {
                    self.entries.remove(&front_key);
                    self.order.pop_front();
                }
                Some(_) => break,
            }
        }
        // Stale pairs behind a live front are only reached once it goes, so a
        // file re-recorded over and over could still grow the queue. Compact
        // it whenever it holds far more pairs than there are entries.
        if self.order.len() > capacity.saturating_mul(2) {
            let Inner { entries, order, .. } = self;
            order.retain(|(key, seq)| entries.get(key).is_some_and(|r| r.seq == *seq));
        }
    }
}

/// The saved form of the map ([`OUTPUT_OWNERS_FILE`]).
#[derive(Debug, Serialize, Deserialize)]
struct PersistedOwners {
    version: u32,
    /// Oldest first, so a restore keeps the eviction order.
    entries: Vec<PersistedOutput>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedOutput {
    subfolder: String,
    filename: String,
    prompt_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    /// Wall-clock record time, Unix seconds.
    recorded_at: u64,
}

/// Seconds since the Unix epoch; 0 if the clock reads before it.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Where the map is saved: [`OUTPUT_OWNERS_FILE`] under the app data dir.
pub fn persist_path() -> Option<PathBuf> {
    crate::config::app_data_dir().map(|dir| dir.join(OUTPUT_OWNERS_FILE))
}

/// Bounded map of ComfyUI output file → producing prompt/owner.
pub struct OutputOwners {
    inner: Mutex<Inner>,
    capacity: usize,
    ttl: Duration,
    /// Where the map is saved, once [`Self::enable_persistence`] has run.
    persist_path: OnceLock<PathBuf>,
    /// Whether the map changed since it was last saved.
    dirty: AtomicBool,
    /// Serialises saves, so an older snapshot is never renamed over a newer one.
    save_lock: Mutex<()>,
}

impl Default for OutputOwners {
    fn default() -> Self {
        Self::new()
    }
}

/// Map key for an output file. ComfyUI reports nested subfolders with the
/// host's separator, so both separators and stray edge slashes fold together.
fn output_key(subfolder: &str, filename: &str) -> String {
    let subfolder = subfolder.replace('\\', "/");
    format!("{}\n{}", subfolder.trim_matches('/'), filename)
}

impl OutputOwners {
    pub fn new() -> Self {
        Self::with_limits(MAX_OUTPUT_OWNERS, OUTPUT_OWNER_TTL)
    }

    pub fn with_limits(capacity: usize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            capacity: capacity.max(1),
            ttl,
            persist_path: OnceLock::new(),
            dirty: AtomicBool::new(false),
            save_lock: Mutex::new(()),
        }
    }

    /// Remember that `prompt_id` (owned by `owner`, if known) produced a file.
    pub fn record(&self, subfolder: &str, filename: &str, prompt_id: &str, owner: Option<String>) {
        self.record_at(
            subfolder,
            filename,
            prompt_id,
            owner,
            Instant::now(),
            unix_now(),
        );
    }

    fn record_at(
        &self,
        subfolder: &str,
        filename: &str,
        prompt_id: &str,
        owner: Option<String>,
        now: Instant,
        now_unix: u64,
    ) {
        let key = output_key(subfolder, filename);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.insert(
            key,
            prompt_id.to_string(),
            owner,
            now + self.ttl,
            now_unix,
            now,
            self.capacity,
        );
        drop(inner);
        self.dirty.store(true, Ordering::Release);
    }

    /// The record for an output file, if one is known and has not expired.
    pub fn lookup(&self, subfolder: &str, filename: &str) -> Option<OutputRecord> {
        self.lookup_at(subfolder, filename, Instant::now())
    }

    fn lookup_at(&self, subfolder: &str, filename: &str, now: Instant) -> Option<OutputRecord> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .entries
            .get(&output_key(subfolder, filename))
            .filter(|record| now < record.expires_at)
            .cloned()
    }

    /// Load the map saved at `path`, then save later changes back to it.
    ///
    /// A missing file starts the map empty. So does an unreadable or corrupt
    /// one, with a warning; the next save replaces it. Entries past their TTL
    /// are dropped, and at most the newest `capacity` are kept. Calls after
    /// the first are ignored.
    pub fn enable_persistence(&self, path: PathBuf) {
        if self.persist_path.get().is_some() {
            return;
        }
        match std::fs::read(&path) {
            Ok(bytes) => match self.restore_bytes_at(&bytes, Instant::now(), unix_now()) {
                Ok(restored) => log::info!(
                    "Restored ownership of {} output file(s) from {}",
                    restored,
                    path.display()
                ),
                Err(e) => log::warn!(
                    "Ignoring unreadable output ownership file {} ({}); starting empty",
                    path.display(),
                    e
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!(
                "Could not read output ownership file {} ({}); starting empty",
                path.display(),
                e
            ),
        }
        let _ = self.persist_path.set(path);
    }

    /// Save the map to its file if it changed since the last save. A no-op
    /// until [`Self::enable_persistence`] has run. Does blocking file I/O.
    pub fn flush(&self) {
        let Some(path) = self.persist_path.get() else {
            return;
        };
        let _guard = self.save_lock.lock().unwrap_or_else(|e| e.into_inner());
        // Cleared before the snapshot, so a record that lands mid-save marks
        // the map dirty again and is picked up by the next flush.
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Err(e) = self.save_to(path, Instant::now()) {
            self.dirty.store(true, Ordering::Release);
            log::warn!(
                "Failed to save output ownership to {}: {}",
                path.display(),
                e
            );
        }
    }

    fn save_to(&self, path: &Path, now: Instant) -> std::io::Result<()> {
        let bytes = self.persisted_bytes_at(now)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::config::write_private_file_atomic(path, &bytes)
    }

    /// The unexpired entries as the saved JSON, oldest first.
    fn persisted_bytes_at(&self, now: Instant) -> std::io::Result<Vec<u8>> {
        let saved = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let mut live: Vec<(&String, &OutputRecord)> = inner
                .entries
                .iter()
                .filter(|(_, record)| now < record.expires_at)
                .collect();
            live.sort_by_key(|(_, record)| record.seq);
            let entries = live
                .into_iter()
                .map(|(key, record)| {
                    let (subfolder, filename) = key.split_once('\n').unwrap_or(("", key));
                    PersistedOutput {
                        subfolder: subfolder.to_string(),
                        filename: filename.to_string(),
                        prompt_id: record.prompt_id.clone(),
                        owner: record.owner.clone(),
                        recorded_at: record.recorded_unix,
                    }
                })
                .collect();
            PersistedOwners {
                version: PERSIST_FORMAT_VERSION,
                entries,
            }
        };
        serde_json::to_vec(&saved).map_err(std::io::Error::other)
    }

    /// Add the entries of a saved file and return how many were kept. Their
    /// remaining life is measured on the wall clock (`now_unix`); a record
    /// time in the future counts as `now_unix`. A file already in the map is
    /// newer than anything saved, so it is left alone.
    fn restore_bytes_at(&self, bytes: &[u8], now: Instant, now_unix: u64) -> Result<usize, String> {
        let saved: PersistedOwners = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if saved.version != PERSIST_FORMAT_VERSION {
            return Err(format!("unsupported format version {}", saved.version));
        }
        let mut live: Vec<(PersistedOutput, u64, Duration)> = saved
            .entries
            .into_iter()
            .filter_map(|entry| {
                if entry.filename.is_empty() {
                    return None;
                }
                let recorded = entry.recorded_at.min(now_unix);
                let remaining = self
                    .ttl
                    .checked_sub(Duration::from_secs(now_unix - recorded))
                    .filter(|left| !left.is_zero())?;
                Some((entry, recorded, remaining))
            })
            .collect();
        // Oldest first (a stable sort, so equal times keep the file's order);
        // only the newest `capacity` survive.
        live.sort_by_key(|(_, recorded, _)| *recorded);
        let skip = live.len().saturating_sub(self.capacity);

        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut restored = 0;
        for (entry, recorded, remaining) in live.into_iter().skip(skip) {
            let key = output_key(&entry.subfolder, &entry.filename);
            if inner.entries.contains_key(&key) {
                continue;
            }
            inner.insert(
                key,
                entry.prompt_id,
                entry.owner,
                now + remaining,
                recorded,
                now,
                self.capacity,
            );
            restored += 1;
        }
        Ok(restored)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.lock().unwrap().entries.len()
    }

    #[cfg(test)]
    fn queued(&self) -> usize {
        self.inner.lock().unwrap().order.len()
    }
}

/// The `(subfolder, filename)` of every `type: "output"` file listed in a
/// ComfyUI `executed` event payload (`{"prompt_id", "node", "output": {...}}`).
/// Output nodes list their files under arbitrary keys (`images`, `gifs`,
/// `audio`, ...), so every array under `output` is scanned. Temp previews
/// (`type: "temp"`) are skipped: the proxy only ever reads `type=output`.
pub fn executed_output_files(payload: &serde_json::Value) -> Vec<(String, String)> {
    let Some(output) = payload.get("output").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    output
        .values()
        .filter_map(|v| v.as_array())
        .flatten()
        .filter_map(|item| {
            let filename = item.get("filename")?.as_str()?;
            let kind = item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("output");
            if filename.is_empty() || kind != "output" {
                return None;
            }
            let subfolder = item.get("subfolder").and_then(|v| v.as_str()).unwrap_or("");
            Some((subfolder.to_string(), filename.to_string()))
        })
        .collect()
}

/// Whether `caller` may read an output file with this record.
///
/// `caller` is `None` for the admin (localhost owner or an admin account),
/// who reads everything. A named account needs a record that names it, or a
/// record whose prompt `prompt_owned` confirms is theirs.
pub fn caller_may_read(
    record: Option<&OutputRecord>,
    caller: Option<&str>,
    prompt_owned: impl FnOnce(&str) -> bool,
) -> bool {
    let Some(caller) = caller else {
        return true;
    };
    let Some(record) = record else {
        return false;
    };
    if let Some(owner) = record.owner.as_deref() {
        return owner.eq_ignore_ascii_case(caller);
    }
    prompt_owned(&record.prompt_id)
}

#[cfg(test)]
mod output_owner_tests {
    use super::*;

    #[test]
    fn records_are_found_by_subfolder_and_filename() {
        let owners = OutputOwners::new();
        owners.record("", "a.png", "p1", Some("alice".into()));
        owners.record("audio", "b.flac", "p2", None);

        let a = owners.lookup("", "a.png").unwrap();
        assert_eq!(a.prompt_id, "p1");
        assert_eq!(a.owner.as_deref(), Some("alice"));
        assert!(owners.lookup("audio", "b.flac").is_some());
        assert!(owners.lookup("", "b.flac").is_none());
        assert!(owners.lookup("other", "a.png").is_none());
    }

    #[test]
    fn subfolder_separators_are_normalised() {
        let owners = OutputOwners::new();
        owners.record("videos\\clips", "v.mp4", "p1", Some("bob".into()));
        assert!(owners.lookup("videos/clips", "v.mp4").is_some());
        assert!(owners.lookup("/videos/clips/", "v.mp4").is_some());
    }

    #[test]
    fn oldest_entries_are_evicted_past_capacity() {
        let owners = OutputOwners::with_limits(3, OUTPUT_OWNER_TTL);
        for i in 0..5 {
            owners.record("", &format!("{i}.png"), "p", None);
        }
        assert_eq!(owners.len(), 3);
        assert!(owners.lookup("", "0.png").is_none());
        assert!(owners.lookup("", "1.png").is_none());
        assert!(owners.lookup("", "4.png").is_some());
    }

    #[test]
    fn re_recording_a_file_refreshes_it_without_growing_the_map() {
        let owners = OutputOwners::with_limits(2, OUTPUT_OWNER_TTL);
        owners.record("", "a.png", "p1", Some("alice".into()));
        owners.record("", "b.png", "p2", None);
        // Overwrite a.png: it becomes the newest entry, so b.png goes first.
        owners.record("", "a.png", "p3", Some("bob".into()));
        owners.record("", "c.png", "p4", None);
        assert_eq!(owners.len(), 2);
        assert!(owners.lookup("", "b.png").is_none());
        assert_eq!(owners.lookup("", "a.png").unwrap().prompt_id, "p3");
        assert!(owners.lookup("", "c.png").is_some());
    }

    #[test]
    fn repeated_re_records_keep_the_order_queue_bounded() {
        let owners = OutputOwners::with_limits(4, OUTPUT_OWNER_TTL);
        owners.record("", "first.png", "p0", None);
        for i in 0..100 {
            owners.record("", "same.png", &format!("p{i}"), None);
        }
        assert_eq!(owners.len(), 2);
        assert!(owners.queued() <= 8);
        assert!(owners.lookup("", "first.png").is_some());
        assert_eq!(owners.lookup("", "same.png").unwrap().prompt_id, "p99");
    }

    #[test]
    fn expired_entries_are_hidden_and_pruned() {
        let ttl = Duration::from_secs(60);
        let owners = OutputOwners::with_limits(100, ttl);
        let t0 = Instant::now();
        owners.record_at("", "old.png", "p1", None, t0, 1_000);
        assert!(owners.lookup_at("", "old.png", t0).is_some());
        assert!(owners.lookup_at("", "old.png", t0 + ttl).is_none());
        // The next record past the TTL drops the stale entry entirely.
        owners.record_at("", "new.png", "p2", None, t0 + ttl, 1_060);
        assert_eq!(owners.len(), 1);
    }

    /// A unique scratch directory, removed by the caller.
    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mooshie-output-owners-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn persisted_map_round_trips_through_its_file() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join(OUTPUT_OWNERS_FILE);

        let owners = OutputOwners::new();
        owners.enable_persistence(path.clone());
        owners.record("", "a.png", "p1", Some("alice".into()));
        owners.record("videos\\clips", "v.mp4", "p2", Some("bob".into()));
        owners.record("", "admin.png", "p3", None);
        owners.flush();
        assert!(path.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // A fresh map (the next process) reads the same ownership back.
        let restarted = OutputOwners::new();
        restarted.enable_persistence(path.clone());
        let a = restarted.lookup("", "a.png").unwrap();
        assert_eq!(a.prompt_id, "p1");
        assert_eq!(a.owner.as_deref(), Some("alice"));
        let v = restarted.lookup("videos/clips", "v.mp4").unwrap();
        assert_eq!(v.owner.as_deref(), Some("bob"));
        let admin = restarted.lookup("", "admin.png").unwrap();
        assert_eq!(admin.owner, None);
        assert_eq!(restarted.len(), 3);
        assert!(caller_may_read(Some(&a), Some("alice"), |_| false));
        assert!(!caller_may_read(Some(&a), Some("bob"), |_| false));

        // Nothing changed since the load, so a flush leaves the file alone;
        // a new record is written on the next flush.
        let before = std::fs::read(&path).unwrap();
        restarted.flush();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        restarted.record("", "c.png", "p4", Some("carol".into()));
        restarted.flush();
        let third = OutputOwners::new();
        third.enable_persistence(path.clone());
        assert_eq!(third.len(), 4);
        assert!(third.lookup("", "c.png").is_some());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flush_is_a_no_op_until_persistence_is_enabled() {
        let owners = OutputOwners::new();
        owners.record("", "a.png", "p1", None);
        owners.flush();
        assert!(owners.persist_path.get().is_none());
        assert!(owners.dirty.load(Ordering::Acquire));
    }

    #[test]
    fn restore_drops_expired_entries_and_keeps_the_remaining_ttl() {
        let ttl = Duration::from_secs(600);
        let owners = OutputOwners::with_limits(100, ttl);
        let now_unix = 1_000_000;
        let saved = serde_json::json!({
            "version": 1,
            "entries": [
                {"subfolder": "", "filename": "expired.png", "prompt_id": "p1",
                 "owner": "alice", "recorded_at": now_unix - 600},
                {"subfolder": "", "filename": "fresh.png", "prompt_id": "p2",
                 "owner": "alice", "recorded_at": now_unix - 500},
                {"subfolder": "", "filename": "future.png", "prompt_id": "p3",
                 "owner": "bob", "recorded_at": now_unix + 5_000},
                {"subfolder": "", "filename": "", "prompt_id": "p4",
                 "recorded_at": now_unix}
            ]
        });
        let t0 = Instant::now();
        let restored = owners
            .restore_bytes_at(saved.to_string().as_bytes(), t0, now_unix)
            .unwrap();
        assert_eq!(restored, 2);
        assert!(owners.lookup_at("", "expired.png", t0).is_none());

        // 500 of its 600 seconds were used before the restart.
        let fresh = Duration::from_secs(100);
        assert!(owners.lookup_at("", "fresh.png", t0).is_some());
        assert!(owners
            .lookup_at("", "fresh.png", t0 + fresh - Duration::from_secs(1))
            .is_some());
        assert!(owners.lookup_at("", "fresh.png", t0 + fresh).is_none());

        // A record time ahead of the clock counts as now, not as extra life.
        let future = owners.lookup_at("", "future.png", t0).unwrap();
        assert_eq!(future.recorded_unix, now_unix);
        assert!(owners.lookup_at("", "future.png", t0 + ttl).is_none());
    }

    #[test]
    fn restore_keeps_only_the_newest_entries_up_to_capacity() {
        let owners = OutputOwners::with_limits(2, OUTPUT_OWNER_TTL);
        let now_unix = 1_000_000;
        let saved = serde_json::json!({
            "version": 1,
            "entries": [
                {"subfolder": "", "filename": "newest.png", "prompt_id": "p3", "recorded_at": now_unix - 1},
                {"subfolder": "", "filename": "oldest.png", "prompt_id": "p1", "recorded_at": now_unix - 30},
                {"subfolder": "", "filename": "middle.png", "prompt_id": "p2", "recorded_at": now_unix - 20}
            ]
        });
        let t0 = Instant::now();
        let restored = owners
            .restore_bytes_at(saved.to_string().as_bytes(), t0, now_unix)
            .unwrap();
        assert_eq!(restored, 2);
        assert!(owners.lookup_at("", "oldest.png", t0).is_none());
        assert!(owners.lookup_at("", "middle.png", t0).is_some());
        assert!(owners.lookup_at("", "newest.png", t0).is_some());
        // Eviction order follows record time: the middle entry goes first.
        owners.record_at("", "next.png", "p4", None, t0, now_unix);
        assert!(owners.lookup_at("", "middle.png", t0).is_none());
        assert!(owners.lookup_at("", "newest.png", t0).is_some());
    }

    #[test]
    fn corrupt_or_foreign_files_start_the_map_empty() {
        let owners = OutputOwners::new();
        let t0 = Instant::now();
        for bad in [
            &b"not json"[..],
            b"{\"version\":1,\"entries\":[{\"filename\":1}]}",
            b"{\"version\":99,\"entries\":[]}",
            b"[]",
            b"",
        ] {
            assert!(owners.restore_bytes_at(bad, t0, 1_000).is_err());
        }
        assert_eq!(owners.len(), 0);

        let dir = scratch_dir("corrupt");
        let path = dir.join(OUTPUT_OWNERS_FILE);
        std::fs::write(&path, b"{\"version\":1,\"entries\":[trunc").unwrap();
        let loaded = OutputOwners::new();
        loaded.enable_persistence(path.clone());
        assert_eq!(loaded.len(), 0);
        // The corrupt file is replaced by the next save.
        loaded.record("", "a.png", "p1", Some("alice".into()));
        loaded.flush();
        let reloaded = OutputOwners::new();
        reloaded.enable_persistence(path.clone());
        assert_eq!(
            reloaded.lookup("", "a.png").unwrap().owner.as_deref(),
            Some("alice")
        );

        // A missing file is simply an empty map.
        let missing = OutputOwners::new();
        missing.enable_persistence(dir.join("absent.json"));
        assert_eq!(missing.len(), 0);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn executed_payload_lists_only_output_files() {
        let payload = serde_json::json!({
            "prompt_id": "p1",
            "node": "9",
            "output": {
                "images": [
                    {"filename": "img_00001_.png", "subfolder": "", "type": "output"},
                    {"filename": "preview.png", "subfolder": "", "type": "temp"}
                ],
                "gifs": [
                    {"filename": "mooshie_video_00001_.mp4", "subfolder": "video", "type": "output"}
                ],
                "text": ["not a file"],
                "audio": [{"filename": "", "type": "output"}]
            }
        });
        let mut files = executed_output_files(&payload);
        files.sort();
        assert_eq!(
            files,
            vec![
                ("".to_string(), "img_00001_.png".to_string()),
                ("video".to_string(), "mooshie_video_00001_.mp4".to_string()),
            ]
        );
        assert!(executed_output_files(&serde_json::json!({"output": null})).is_empty());
        assert!(executed_output_files(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn only_the_owner_or_admin_may_read_an_output() {
        let owners = OutputOwners::new();
        owners.record("", "alice.png", "p1", Some("alice".into()));
        owners.record("", "late.png", "p2", None);
        let alice = owners.lookup("", "alice.png");
        let late = owners.lookup("", "late.png");
        let unknown = owners.lookup("", "unknown.png");

        // Admin reads everything, including unrecorded files.
        assert!(caller_may_read(alice.as_ref(), None, |_| false));
        assert!(caller_may_read(unknown.as_ref(), None, |_| false));

        assert!(caller_may_read(alice.as_ref(), Some("alice"), |_| false));
        assert!(caller_may_read(alice.as_ref(), Some("Alice"), |_| false));
        // A named owner is not overridden by the prompt check.
        assert!(!caller_may_read(alice.as_ref(), Some("bob"), |_| true));

        // Unrecorded files are refused to every named account.
        assert!(!caller_may_read(unknown.as_ref(), Some("alice"), |_| true));

        // Owner unknown at record time: decided by the prompt's ownership.
        assert!(caller_may_read(late.as_ref(), Some("bob"), |pid| pid == "p2"));
        assert!(!caller_may_read(late.as_ref(), Some("bob"), |_| false));
    }
}
