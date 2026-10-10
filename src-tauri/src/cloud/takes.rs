//! On-disk store for paid scene intermediates, starting with voice takes.
//!
//! A take is cached by a hash of everything that shapes it (voice, model,
//! text, settings, take number), so asking for the same take again never
//! pays twice. Paid work is never regenerated silently: a fresh take of the
//! same line is a new take number the user asked for (research doc 5.3).
//!
//! Layout: the owner's assets live in `{app_data}/scene_assets`, a named
//! account's in `{app_data}/users/{account dir}/scene_assets`, next to its
//! encrypted secrets, so accounts never read each other's takes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::elevenlabs::{SpeechSettings, TTS_MODEL};
use super::scene::mouth::{MouthMap, MOUTH_VERSION};

/// Bump to invalidate every cached take, for example if the hash inputs change.
const TAKE_KEY_VERSION: &str = "take-v1";

pub fn scene_assets_dir(username: Option<&str>) -> Option<PathBuf> {
    let root = crate::config::app_data_dir()?;
    scene_assets_dir_in(&root, username)
}

fn scene_assets_dir_in(root: &Path, username: Option<&str>) -> Option<PathBuf> {
    match username {
        None => Some(root.join("scene_assets")),
        Some(user) => Some(
            root.join("users")
                .join(crate::user_secrets::sanitize_username(user)?)
                .join("scene_assets"),
        ),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TakeMeta {
    pub voice_id: String,
    pub model: String,
    pub text: String,
    /// Credits ElevenLabs reported for this take, when it reported them.
    pub credits: Option<u64>,
    pub created_unix: i64,
}

pub fn take_id(voice_id: &str, text: &str, settings: &SpeechSettings, take_index: u32) -> String {
    let key = serde_json::json!({
        "v": TAKE_KEY_VERSION,
        "voice": voice_id.trim(),
        "model": TTS_MODEL,
        "text": text,
        // Rounded so 0.35 and 0.35000001 are the same take.
        "stability": (settings.stability * 1000.0).round() as i64,
        "similarity": (settings.similarity * 1000.0).round() as i64,
        "language": settings.language_code.trim(),
        "normalize": settings.normalize_text,
        "seed": settings.seed,
        "take": take_index,
    });
    hex::encode(Sha256::digest(key.to_string().as_bytes()))
}

/// A take id goes into a file name, so it must be exactly a SHA-256 in hex.
pub fn valid_take_id(id: &str) -> bool {
    id.len() == 64
        && id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

pub struct TakeStore {
    dir: PathBuf,
}

impl TakeStore {
    pub fn for_account(username: Option<&str>) -> Option<Self> {
        Some(Self::in_dir(scene_assets_dir(username)?.join("takes")))
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn audio_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.mp3"))
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub fn contains(&self, id: &str) -> bool {
        valid_take_id(id) && self.audio_path(id).is_file()
    }

    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        self.contains(id).then(|| self.audio_path(id))
    }

    pub fn get(&self, id: &str) -> Option<(TakeMeta, Vec<u8>)> {
        if !valid_take_id(id) {
            return None;
        }
        let bytes = std::fs::read(self.audio_path(id)).ok()?;
        let meta = std::fs::read(self.meta_path(id))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())?;
        Some((meta, bytes))
    }

    fn mouth_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.mouth.json"))
    }

    /// The take's cached mouth map, if it was measured with the current rules.
    pub fn mouth(&self, id: &str) -> Option<MouthMap> {
        if !valid_take_id(id) {
            return None;
        }
        std::fs::read(self.mouth_path(id))
            .ok()
            .and_then(|b| serde_json::from_slice::<MouthMap>(&b).ok())
            .filter(|m| m.version == MOUTH_VERSION)
    }

    pub fn put_mouth(&self, id: &str, map: &MouthMap) -> Result<(), String> {
        if !self.contains(id) {
            return Err("Invalid take id".into());
        }
        let json = serde_json::to_vec_pretty(map).map_err(|e| e.to_string())?;
        write_atomic(&self.mouth_path(id), &json)
    }

    /// Write audio first and metadata last, each through a temp file and a
    /// rename, so a crash never leaves metadata pointing at partial audio.
    pub fn put(&self, id: &str, bytes: &[u8], meta: &TakeMeta) -> Result<(), String> {
        if !valid_take_id(id) {
            return Err("Invalid take id".into());
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        write_atomic(&self.audio_path(id), bytes)?;
        let json = serde_json::to_vec_pretty(meta).map_err(|e| e.to_string())?;
        write_atomic(&self.meta_path(id), &json)
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> SpeechSettings {
        SpeechSettings {
            stability: 0.35,
            similarity: 0.75,
            language_code: "ja".into(),
            normalize_text: false,
            seed: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mooshie-takes-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_same_request_is_the_same_take() {
        let a = take_id("v1", "こんにちは", &settings(), 0);
        assert_eq!(a, take_id("v1", "こんにちは", &settings(), 0));
        assert!(valid_take_id(&a));
        let mut nudged = settings();
        nudged.stability = 0.350_000_1;
        assert_eq!(a, take_id("v1", "こんにちは", &nudged, 0));
    }

    #[test]
    fn anything_that_changes_the_audio_changes_the_take() {
        let base = take_id("v1", "line", &settings(), 0);
        assert_ne!(base, take_id("v2", "line", &settings(), 0));
        assert_ne!(base, take_id("v1", "line!", &settings(), 0));
        assert_ne!(base, take_id("v1", "line", &settings(), 1));
        let mut s = settings();
        s.language_code = "en".into();
        assert_ne!(base, take_id("v1", "line", &s, 0));
        let mut s = settings();
        s.seed = Some(1);
        assert_ne!(base, take_id("v1", "line", &s, 0));
    }

    #[test]
    fn take_ids_cannot_name_other_files() {
        for bad in [
            "",
            "../secrets",
            &"A".repeat(64),
            &"g".repeat(64),
            &"a".repeat(63),
        ] {
            assert!(!valid_take_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_take_round_trips() {
        let dir = scratch("round-trip");
        let store = TakeStore::in_dir(dir.join("takes"));
        let id = take_id("v1", "line", &settings(), 0);
        assert!(!store.contains(&id));
        let meta = TakeMeta {
            voice_id: "v1".into(),
            model: TTS_MODEL.into(),
            text: "line".into(),
            credits: Some(12),
            created_unix: 1,
        };
        store.put(&id, b"mp3-bytes", &meta).unwrap();
        assert!(store.contains(&id));
        let (back, bytes) = store.get(&id).unwrap();
        assert_eq!(back, meta);
        assert_eq!(bytes, b"mp3-bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accounts_get_separate_asset_directories() {
        let root = PathBuf::from("/data");
        let owner = scene_assets_dir_in(&root, None).unwrap();
        let alice = scene_assets_dir_in(&root, Some("alice")).unwrap();
        let bob = scene_assets_dir_in(&root, Some("bob")).unwrap();
        assert_ne!(owner, alice);
        assert_ne!(alice, bob);
        assert!(alice.starts_with(root.join("users")));
    }
}
