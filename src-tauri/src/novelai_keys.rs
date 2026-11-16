//! The instance owner's saved NovelAI keys, for switching between accounts.
//!
//! The key in use stays where it always was, `config.novelai_api_key`, so
//! every request path is unchanged. This file only holds the named keys the
//! owner can switch to; switching copies one into the config. Which entry is
//! "in use" is worked out by comparing it with the config key, so nothing can
//! drift out of step when the key is changed through the ordinary field.
//!
//! Kept out of `config.json` on purpose: the config is sent to clients
//! (redacted), merged on save and backed up, and every one of those paths
//! would need its own rule for a list of secrets. This file is read and
//! written only here, owner-only, with the same private atomic write as the
//! config. An entry may hold `env:NAME` like the main field.
//!
//! Compiled in both builds.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "novelai_keys.json";
pub const MAX_KEYS: usize = 20;
const MAX_LABEL_LEN: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedKey {
    pub label: String,
    pub key: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct KeysFile {
    #[serde(default)]
    keys: Vec<SavedKey>,
}

/// What the UI sees: never the key itself.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KeySummary {
    pub label: String,
    /// `…abcd` for a key, or `env:NAME` for a variable reference.
    pub hint: String,
    pub active: bool,
}

fn file_path() -> Result<PathBuf, String> {
    crate::config::app_data_dir()
        .map(|dir| dir.join(FILE_NAME))
        .ok_or_else(|| "Failed to determine app data directory".to_string())
}

fn read_in(path: &Path) -> Vec<SavedKey> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<KeysFile>(&s).ok())
        .map(|f| f.keys)
        .unwrap_or_default()
}

fn write_in(path: &Path, keys: &[SavedKey]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create data dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&KeysFile {
        keys: keys.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    crate::config::write_private_file_atomic(path, json.as_bytes())
        .map_err(|e| format!("Failed to save NovelAI keys: {e}"))
}

fn hint(key: &str) -> String {
    if crate::key_source::is_env_ref(key) {
        return key.trim().to_string();
    }
    let tail: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

pub fn summaries(keys: &[SavedKey], current: Option<&str>) -> Vec<KeySummary> {
    let current = current.map(str::trim).filter(|k| !k.is_empty());
    keys.iter()
        .map(|k| KeySummary {
            label: k.label.clone(),
            hint: hint(&k.key),
            active: current == Some(k.key.as_str()),
        })
        .collect()
}

/// Add or replace the entry named `label`. Returns the new list.
pub fn upsert(mut keys: Vec<SavedKey>, label: &str, key: &str) -> Result<Vec<SavedKey>, String> {
    let label = label.trim();
    let key = key.trim();
    if label.is_empty() {
        return Err("Give the key a name".into());
    }
    if label.chars().count() > MAX_LABEL_LEN {
        return Err(format!("Key names are at most {MAX_LABEL_LEN} characters"));
    }
    if key.is_empty() {
        return Err("Paste the NovelAI key to save".into());
    }
    crate::key_source::check(key)?;
    if let Some(existing) = keys
        .iter_mut()
        .find(|k| k.label.eq_ignore_ascii_case(label))
    {
        existing.label = label.to_string();
        existing.key = key.to_string();
        return Ok(keys);
    }
    if keys.len() >= MAX_KEYS {
        return Err(format!("At most {MAX_KEYS} keys can be saved"));
    }
    keys.push(SavedKey {
        label: label.to_string(),
        key: key.to_string(),
    });
    Ok(keys)
}

fn find<'a>(keys: &'a [SavedKey], label: &str) -> Option<&'a SavedKey> {
    keys.iter()
        .find(|k| k.label.eq_ignore_ascii_case(label.trim()))
}

// --- file-backed operations ------------------------------------------------

pub fn list(current: Option<&str>) -> Result<Vec<KeySummary>, String> {
    Ok(summaries(&read_in(&file_path()?), current))
}

pub fn save(label: &str, key: &str, current: Option<&str>) -> Result<Vec<KeySummary>, String> {
    let path = file_path()?;
    let keys = upsert(read_in(&path), label, key)?;
    write_in(&path, &keys)?;
    Ok(summaries(&keys, current))
}

pub fn delete(label: &str, current: Option<&str>) -> Result<Vec<KeySummary>, String> {
    let path = file_path()?;
    let mut keys = read_in(&path);
    let before = keys.len();
    keys.retain(|k| !k.label.eq_ignore_ascii_case(label.trim()));
    if keys.len() != before {
        write_in(&path, &keys)?;
    }
    Ok(summaries(&keys, current))
}

/// The stored key behind `label`, for switching to it.
pub fn key_for(label: &str) -> Result<String, String> {
    find(&read_in(&file_path()?), label)
        .map(|k| k.key.clone())
        .ok_or_else(|| format!("No saved NovelAI key named \"{}\"", label.trim()))
}

/// Make the saved key `label` the one in use, under the config write lock.
pub async fn switch(
    state: &crate::state::AppState,
    label: &str,
) -> Result<Vec<KeySummary>, String> {
    let key = key_for(label)?;
    let mut config = state.config.write().await;
    config.novelai_api_key = Some(key);
    crate::config::save_config(&config)?;
    list(config.novelai_api_key.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(label: &str, key: &str) -> SavedKey {
        SavedKey {
            label: label.into(),
            key: key.into(),
        }
    }

    #[test]
    fn upsert_adds_replaces_and_validates() {
        let keys = upsert(vec![], " Main ", " pst-aaaa1111 ").unwrap();
        assert_eq!(keys, vec![key("Main", "pst-aaaa1111")]);
        let keys = upsert(keys, "main", "pst-bbbb2222").unwrap();
        assert_eq!(keys, vec![key("main", "pst-bbbb2222")]);
        assert!(upsert(keys.clone(), " ", "pst-x").is_err());
        assert!(upsert(keys.clone(), "Alt", "  ").is_err());
        assert!(upsert(keys.clone(), "Alt", "env:MOOSHIEUI_SECRET_KEY").is_err());
        let full: Vec<SavedKey> = (0..MAX_KEYS).map(|i| key(&format!("k{i}"), "x")).collect();
        assert!(upsert(full, "one more", "pst-y").is_err());
    }

    #[test]
    fn summaries_hide_keys_and_mark_the_one_in_use() {
        let keys = vec![key("Main", "pst-aaaa1111"), key("Env", "env:NAI_ALT")];
        let s = summaries(&keys, Some(" pst-aaaa1111 "));
        assert_eq!(s[0].hint, "…1111");
        assert!(s[0].active);
        assert_eq!(s[1].hint, "env:NAI_ALT");
        assert!(!s[1].active);
        assert!(summaries(&keys, None).iter().all(|k| !k.active));
    }

    #[test]
    fn file_round_trip_and_delete() {
        let dir = std::env::temp_dir().join(format!("nai-keys-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        write_in(&path, &[key("A", "pst-1"), key("B", "pst-2")]).unwrap();
        let read = read_in(&path);
        assert_eq!(read.len(), 2);
        assert_eq!(find(&read, " b ").unwrap().key, "pst-2");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read_in(&path).is_empty());
    }
}
