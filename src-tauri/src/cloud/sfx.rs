//! On-disk per-account store and generation flow for ElevenLabs v3 sound effects.
//!
//! Layout matches voice takes (`takes.rs`):
//! - Desktop / admin owner: `{app_data}/scene_assets/sfx/{id}.{mp3,json}`
//! - Named web account: `{app_data}/users/{account}/scene_assets/sfx/{id}.{mp3,json}`
//!
//! Compiles in both desktop and server builds; keep `tauri` imports out.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

use super::elevenlabs::{
    validate_sfx_request, ElevenLabsClient, Moderated, SfxSettings, SFX_DURATION_MAX,
    SFX_DURATION_MIN,
};
use super::takes;
use super::{resolve_credential, CloudProvider};

const AUDIO_MEDIA_TYPE: &str = "audio/mpeg";
const DEFAULT_PROMPT_INFLUENCE: f32 = 0.3;

#[derive(Debug, Clone, Deserialize)]
pub struct SfxRequest {
    pub prompt: String,
    pub duration_seconds: Option<f32>,
    pub prompt_influence: Option<f32>,
    #[serde(default)]
    pub loop_audio: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SfxClipSummary {
    pub id: String,
    pub prompt: String,
    pub created_unix: i64,
    pub duration_seconds: Option<f32>,
    pub prompt_influence: f32,
    pub loop_audio: bool,
    pub credits: Option<u64>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SfxClipData {
    pub summary: SfxClipSummary,
    pub audio_base64: String,
    pub media_type: &'static str,
}

pub fn valid_sfx_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub struct SfxStore {
    dir: PathBuf,
}

impl SfxStore {
    pub fn for_account(username: Option<&str>) -> Option<Self> {
        Some(Self::in_dir(takes::scene_assets_dir(username)?.join("sfx")))
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn resolve_existing(&self, id: &str, ext: &str) -> Result<PathBuf, AppError> {
        if !valid_sfx_id(id) {
            return Err(AppError::Other("Invalid sound effect identifier.".into()));
        }
        let candidate = self.dir.join(format!("{id}.{ext}"));
        let meta = std::fs::symlink_metadata(&candidate)
            .map_err(|_| AppError::Other("Sound effect clip was not found.".into()))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(AppError::Other("Invalid sound effect entry.".into()));
        }
        let dir_canon = self
            .dir
            .canonicalize()
            .map_err(|e| AppError::Other(format!("Sound effects folder unavailable: {e}")))?;
        let file_canon = candidate
            .canonicalize()
            .map_err(|e| AppError::Other(format!("Sound effect clip unavailable: {e}")))?;
        if !file_canon.starts_with(&dir_canon) || file_canon.parent() != Some(dir_canon.as_path()) {
            return Err(AppError::Other("Invalid sound effect path.".into()));
        }
        Ok(file_canon)
    }

    pub fn put(&self, summary: &SfxClipSummary, audio_bytes: &[u8]) -> Result<(), AppError> {
        if !valid_sfx_id(&summary.id) {
            return Err(AppError::Other("Invalid sound effect identifier.".into()));
        }
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| AppError::Other(format!("Cannot create SFX folder: {e}")))?;
        let audio_path = self.dir.join(format!("{}.mp3", summary.id));
        let meta_path = self.dir.join(format!("{}.json", summary.id));
        write_atomic(&audio_path, audio_bytes)?;
        let json = serde_json::to_vec_pretty(summary)
            .map_err(|e| AppError::Other(format!("Failed to serialize SFX metadata: {e}")))?;
        write_atomic(&meta_path, &json)
    }

    pub fn list(&self) -> Result<Vec<SfxClipSummary>, AppError> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Ok(Vec::new());
        };
        let mut items = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !valid_sfx_id(stem) {
                continue;
            }
            let Ok(meta_path) = self.resolve_existing(stem, "json") else {
                continue;
            };
            if self.resolve_existing(stem, "mp3").is_err() {
                continue;
            }
            let Ok(bytes) = std::fs::read(&meta_path) else {
                continue;
            };
            if let Ok(summary) = serde_json::from_slice::<SfxClipSummary>(&bytes) {
                if valid_sfx_id(&summary.id) && summary.id == stem {
                    items.push(summary);
                }
            }
        }
        items.sort_by(|a, b| {
            b.created_unix
                .cmp(&a.created_unix)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(items)
    }

    pub fn load(&self, clip_id: &str) -> Result<SfxClipData, AppError> {
        let meta_path = self.resolve_existing(clip_id, "json")?;
        let audio_path = self.resolve_existing(clip_id, "mp3")?;
        let meta_bytes = std::fs::read(&meta_path)
            .map_err(|e| AppError::Other(format!("Could not read SFX metadata: {e}")))?;
        let summary: SfxClipSummary = serde_json::from_slice(&meta_bytes)
            .map_err(|e| AppError::Other(format!("Corrupt SFX metadata: {e}")))?;
        let audio_bytes = std::fs::read(&audio_path)
            .map_err(|e| AppError::Other(format!("Could not read SFX audio: {e}")))?;
        Ok(SfxClipData {
            summary,
            audio_base64: base64::engine::general_purpose::STANDARD.encode(&audio_bytes),
            media_type: AUDIO_MEDIA_TYPE,
        })
    }

    pub fn delete(&self, clip_id: &str) -> Result<(), AppError> {
        let meta_path = self.resolve_existing(clip_id, "json").ok();
        let audio_path = self.resolve_existing(clip_id, "mp3").ok();
        if meta_path.is_none() && audio_path.is_none() {
            return Err(AppError::Other("Sound effect clip was not found.".into()));
        }
        if let Some(p) = audio_path {
            std::fs::remove_file(&p)
                .map_err(|e| AppError::Other(format!("Failed to delete SFX audio: {e}")))?;
        }
        if let Some(p) = meta_path {
            std::fs::remove_file(&p)
                .map_err(|e| AppError::Other(format!("Failed to delete SFX metadata: {e}")))?;
        }
        Ok(())
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, bytes)
        .map_err(|e| AppError::Other(format!("Failed to write SFX file: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::Other(format!("Failed to save SFX file: {e}"))
    })
}

fn store_for(username: Option<&str>) -> Result<SfxStore, AppError> {
    SfxStore::for_account(username)
        .ok_or_else(|| AppError::Other("Could not locate scene assets directory.".into()))
}

pub async fn generate(
    state: &Arc<AppState>,
    username: Option<&str>,
    request: SfxRequest,
) -> Result<Moderated<SfxClipData>, AppError> {
    let settings = SfxSettings {
        duration_seconds: request.duration_seconds.map(|d| {
            if d.is_finite() {
                d.clamp(SFX_DURATION_MIN, SFX_DURATION_MAX)
            } else {
                d
            }
        }),
        prompt_influence: request
            .prompt_influence
            .unwrap_or(DEFAULT_PROMPT_INFLUENCE)
            .clamp(0.0, 1.0),
        loop_audio: request.loop_audio,
    };
    let clean_prompt = validate_sfx_request(&request.prompt, &settings)?.to_string();

    let credential = resolve_credential(state, username, CloudProvider::ElevenLabs).await?;
    let outcome = ElevenLabsClient::new(&state.http_client, &credential)
        .generate_sound_effect(&clean_prompt, &settings)
        .await?;

    match outcome {
        Moderated::Blocked { message } => Ok(Moderated::Blocked { message }),
        Moderated::Ok { value: audio } => {
            let created_unix = chrono::Utc::now().timestamp();
            let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
            let id = format!("{created_unix}-{suffix}");
            let summary = SfxClipSummary {
                id,
                prompt: clean_prompt,
                created_unix,
                duration_seconds: settings.duration_seconds,
                prompt_influence: settings.prompt_influence,
                loop_audio: settings.loop_audio,
                credits: audio.character_cost,
                size_bytes: audio.bytes.len() as u64,
            };
            let store = store_for(username)?;
            store.put(&summary, &audio.bytes)?;
            Ok(Moderated::Ok {
                value: SfxClipData {
                    summary,
                    audio_base64: base64::engine::general_purpose::STANDARD.encode(&audio.bytes),
                    media_type: AUDIO_MEDIA_TYPE,
                },
            })
        }
    }
}

pub fn list(username: Option<&str>) -> Result<Vec<SfxClipSummary>, AppError> {
    store_for(username)?.list()
}

pub fn load(username: Option<&str>, clip_id: &str) -> Result<SfxClipData, AppError> {
    store_for(username)?.load(clip_id)
}

pub fn delete(username: Option<&str>, clip_id: &str) -> Result<(), AppError> {
    store_for(username)?.delete(clip_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "mooshie-sfx-{name}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_summary(id: &str, ts: i64) -> SfxClipSummary {
        SfxClipSummary {
            id: id.into(),
            prompt: "rain on window".into(),
            created_unix: ts,
            duration_seconds: Some(3.0),
            prompt_influence: 0.3,
            loop_audio: false,
            credits: Some(120),
            size_bytes: 4,
        }
    }

    #[test]
    fn put_list_load_and_delete_round_trip() {
        let dir = Scratch::new("roundtrip");
        let store = SfxStore::in_dir(dir.0.clone());
        store
            .put(&sample_summary("100-aaaa", 100), b"MP3A")
            .unwrap();
        store
            .put(&sample_summary("200-bbbb", 200), b"MP3B")
            .unwrap();

        let listed = store.list().unwrap();
        let ids: Vec<_> = listed.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["200-bbbb", "100-aaaa"]);

        let loaded = store.load("100-aaaa").unwrap();
        assert_eq!(loaded.media_type, "audio/mpeg");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&loaded.audio_base64)
                .unwrap(),
            b"MP3A"
        );

        store.delete("100-aaaa").unwrap();
        assert!(store.load("100-aaaa").is_err());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn rejects_invalid_and_traversal_clip_ids() {
        let dir = Scratch::new("traversal");
        let store = SfxStore::in_dir(dir.0.clone());
        for bad in ["", "../escape", "a/b", "a\\b", "clip.mp3", "a b"] {
            assert!(!valid_sfx_id(bad), "{bad}");
            assert!(store.load(bad).is_err(), "{bad}");
            assert!(store.delete(bad).is_err(), "{bad}");
        }
    }
}
