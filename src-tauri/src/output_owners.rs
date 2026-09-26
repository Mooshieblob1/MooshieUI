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

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How many output files are remembered at once.
pub const MAX_OUTPUT_OWNERS: usize = 4096;

/// How long a recorded output stays readable by its owner through the proxy.
pub const OUTPUT_OWNER_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Who produced one output file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRecord {
    /// The ComfyUI prompt id from the `executed` event.
    pub prompt_id: String,
    /// The owning account when it was known at record time. `None` is either
    /// the admin or a prompt whose ownership was not bound yet; the prompt id
    /// is checked again at read time in that case.
    pub owner: Option<String>,
    recorded_at: Instant,
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

/// Bounded map of ComfyUI output file → producing prompt/owner.
pub struct OutputOwners {
    inner: Mutex<Inner>,
    capacity: usize,
    ttl: Duration,
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
        }
    }

    /// Remember that `prompt_id` (owned by `owner`, if known) produced a file.
    pub fn record(&self, subfolder: &str, filename: &str, prompt_id: &str, owner: Option<String>) {
        self.record_at(subfolder, filename, prompt_id, owner, Instant::now());
    }

    fn record_at(
        &self,
        subfolder: &str,
        filename: &str,
        prompt_id: &str,
        owner: Option<String>,
        now: Instant,
    ) {
        let key = output_key(subfolder, filename);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.entries.insert(
            key.clone(),
            OutputRecord {
                prompt_id: prompt_id.to_string(),
                owner,
                recorded_at: now,
                seq,
            },
        );
        inner.order.push_back((key, seq));

        // Drop expired entries from the front, then the oldest live ones
        // until the map is back under its cap.
        let ttl = self.ttl;
        while let Some((front_key, front_seq)) = inner.order.front().cloned() {
            let live = inner
                .entries
                .get(&front_key)
                .filter(|record| record.seq == front_seq);
            let over_cap = inner.entries.len() > self.capacity;
            match live {
                // A stale pair left behind by a re-record: nothing to evict.
                None => {
                    inner.order.pop_front();
                }
                Some(record) if over_cap || now.duration_since(record.recorded_at) >= ttl => {
                    inner.entries.remove(&front_key);
                    inner.order.pop_front();
                }
                Some(_) => break,
            }
        }
        // Stale pairs behind a live front are only reached once it goes, so a
        // file re-recorded over and over could still grow the queue. Compact
        // it whenever it holds far more pairs than there are entries.
        if inner.order.len() > self.capacity.saturating_mul(2) {
            let Inner { entries, order, .. } = &mut *inner;
            order.retain(|(key, seq)| entries.get(key).is_some_and(|r| r.seq == *seq));
        }
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
            .filter(|record| now.duration_since(record.recorded_at) < self.ttl)
            .cloned()
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
        owners.record_at("", "old.png", "p1", None, t0);
        assert!(owners.lookup_at("", "old.png", t0).is_some());
        assert!(owners.lookup_at("", "old.png", t0 + ttl).is_none());
        // The next record past the TTL drops the stale entry entirely.
        owners.record_at("", "new.png", "p2", None, t0 + ttl);
        assert_eq!(owners.len(), 1);
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
