//! Per-user preferences storage.
//!
//! Stores client-side preferences server-side so they persist across devices
//! and operating systems when multiple clients connect to the same MooshieUI
//! server instance.
//!
//! Data stored at `{app_data_dir}/users/{username}/prefs.json`.
//!
//! The special username [`OWNER_KEY`] (`"_admin"`) is the machine owner: the
//! desktop app, and browser mode on localhost or with an admin account. All of
//! them read and write the same file, so the owner's characters, presets and
//! settings are one copy whichever mode they open.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config;

/// Prefs key for the machine owner (see the module docs).
pub const OWNER_KEY: &str = "_admin";

/// Serialises read-modify-write updates. The desktop app and a browser tab can
/// save at the same moment, and both land on the owner's file.
static UPDATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// All user-specific preferences that the frontend persists.
///
/// Every field is `Option` so clients can do partial updates (unknown fields
/// are ignored during deserialization). A `None` field in an update leaves the
/// stored value for that category unchanged, see [`update`].
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserPrefs {
    /// Generation parameters (checkpoint, sampler, dimensions, LoRAs, etc.)
    pub generation: Option<serde_json::Value>,
    /// Prompt history (last 100 entries).
    pub prompt_history: Option<serde_json::Value>,
    /// Prompt presets (named snippets) and their active state.
    pub prompt_presets: Option<serde_json::Value>,
    /// Artist styles and their active state.
    pub styles: Option<serde_json::Value>,
    /// LoRA stack presets.
    pub lora_presets: Option<serde_json::Value>,
    /// Favourited artist slugs and user-defined categories.
    pub artist_favourites: Option<serde_json::Value>,
    /// Gallery board assignments and custom board names.
    pub gallery_boards: Option<serde_json::Value>,
    /// Autocomplete source/settings.
    pub autocomplete: Option<serde_json::Value>,
    /// Accessibility settings (vision simulator, info tips).
    pub accessibility: Option<serde_json::Value>,
    /// Freeform user notes (bottom panel Notes tab).
    pub notes: Option<serde_json::Value>,
    /// Characters saved from prompts (bottom panel Characters tab), each
    /// tagged with the model architecture it belongs to.
    pub characters: Option<serde_json::Value>,
    /// H3 Director timeline (bottom panel Timeline tab, video mode).
    pub video_timeline: Option<serde_json::Value>,
    /// UI locale string (e.g. `"en"`, `"ja"`).
    pub locale: Option<serde_json::Value>,
    /// Gallery images generated as artist previews, by artist slug.
    pub artist_local_previews: Option<serde_json::Value>,
    /// Prompt enhancer conversations, one per enhancer flow.
    pub enhancer_sessions: Option<serde_json::Value>,
    /// Style Creator settings, anchor and drawn-combination history.
    pub style_creator: Option<serde_json::Value>,
    /// Anime scene draft (character, voice, lines, shots, clips).
    pub anime_scene: Option<serde_json::Value>,
    /// True once the desktop app has merged its own saved data into this file
    /// (the one-time move from per-mode storage to this shared file).
    pub desktop_merged: Option<bool>,
    /// ISO 8601 timestamp of the last update (set by the server, not the client).
    pub updated_at: Option<String>,
    /// Set by a client sending its one-time merge (`"desktop"` or
    /// `"browser"`): the file as it was before is kept next to it first.
    /// Never stored.
    #[serde(default, skip_serializing)]
    pub merge_source: Option<String>,
}

/// `incoming` laid over `existing`: every category `incoming` carries replaces
/// the stored one, and every category it leaves out (`None`) is kept.
fn merge_over(existing: UserPrefs, incoming: UserPrefs) -> Result<UserPrefs, String> {
    let mut base = match serde_json::to_value(existing).map_err(|e| e.to_string())? {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    if let serde_json::Value::Object(map) =
        serde_json::to_value(incoming).map_err(|e| e.to_string())?
    {
        for (key, value) in map {
            if !value.is_null() {
                base.insert(key, value);
            }
        }
    }
    serde_json::from_value(serde_json::Value::Object(base)).map_err(|e| e.to_string())
}

/// File name for the copy kept before a client's one-time merge. Only
/// alphanumerics survive, so a client cannot steer the path.
fn pre_merge_backup_name(source: &str) -> Option<String> {
    let safe: String = source
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(16)
        .collect();
    (!safe.is_empty()).then(|| format!("prefs.before-merge-{safe}.json"))
}

/// Compute the path to a user's prefs file.
///
/// Sanitises the username to prevent path traversal: only alphanumerics,
/// hyphens, and underscores are allowed (which also covers the reserved
/// `"_admin"` key for localhost/admin sessions).
fn prefs_path(username: &str) -> Option<PathBuf> {
    let safe_name: String = username
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if safe_name.is_empty() {
        return None;
    }
    config::app_data_dir().map(|d| d.join("users").join(safe_name).join("prefs.json"))
}

/// Load a user's prefs from disk.  Returns `None` if the file doesn't exist
/// or cannot be parsed (treated as "no prefs yet").
pub async fn load(username: &str) -> Option<UserPrefs> {
    let path = prefs_path(username)?;
    let bytes = tokio::fs::read(&path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Apply a client's update to a user's stored prefs and return the new
/// `updated_at`. Categories the update leaves out are kept (see
/// [`merge_over`]), so a client that only sends what changed cannot wipe what
/// another client saved meanwhile.
pub async fn update(username: &str, incoming: UserPrefs) -> Result<String, String> {
    let _guard = UPDATE_LOCK.lock().await;
    let path = prefs_path(username).ok_or_else(|| "Cannot resolve prefs path".to_string())?;
    if let Some(name) = incoming
        .merge_source
        .as_deref()
        .and_then(pre_merge_backup_name)
    {
        let backup = path.with_file_name(name);
        // The first merge from each side keeps what was there before; a
        // retried merge must not replace that copy with merged data.
        if tokio::fs::try_exists(&path).await.unwrap_or(false)
            && !tokio::fs::try_exists(&backup).await.unwrap_or(false)
        {
            tokio::fs::copy(&path, &backup)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    let existing = load(username).await.unwrap_or_default();
    let mut merged = merge_over(existing, incoming)?;
    let updated_at = chrono::Utc::now().to_rfc3339();
    merged.updated_at = Some(updated_at.clone());
    save(username, &merged).await?;
    Ok(updated_at)
}

/// Save a user's prefs to disk, creating parent directories as needed.
pub async fn save(username: &str, prefs: &UserPrefs) -> Result<(), String> {
    let path = prefs_path(username).ok_or_else(|| "Cannot resolve prefs path".to_string())?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(prefs).map_err(|e| e.to_string())?;
    // Atomic, because `load` treats a file it cannot parse as "no prefs yet":
    // a write cut short would silently wipe the user's history and presets.
    tokio::task::spawn_blocking(move || config::write_private_file_atomic(&path, &bytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_over_keeps_categories_the_update_leaves_out() {
        let existing = UserPrefs {
            characters: Some(json!({ "characters": [{ "id": "a" }] })),
            notes: Some(json!({ "text": "keep me" })),
            desktop_merged: Some(true),
            ..Default::default()
        };
        let incoming = UserPrefs {
            notes: Some(json!({ "text": "new" })),
            ..Default::default()
        };
        let merged = merge_over(existing, incoming).unwrap();
        assert_eq!(merged.notes, Some(json!({ "text": "new" })));
        assert_eq!(
            merged.characters,
            Some(json!({ "characters": [{ "id": "a" }] }))
        );
        assert_eq!(merged.desktop_merged, Some(true));
    }

    #[test]
    fn merge_over_replaces_a_category_wholesale() {
        let existing = UserPrefs {
            styles: Some(json!({ "styles": [{ "id": "a" }, { "id": "b" }] })),
            ..Default::default()
        };
        let incoming = UserPrefs {
            styles: Some(json!({ "styles": [{ "id": "a" }] })),
            ..Default::default()
        };
        let merged = merge_over(existing, incoming).unwrap();
        assert_eq!(merged.styles, Some(json!({ "styles": [{ "id": "a" }] })));
    }

    #[test]
    fn merge_source_is_never_stored() {
        let prefs = UserPrefs {
            merge_source: Some("desktop".into()),
            ..Default::default()
        };
        let text = serde_json::to_string(&prefs).unwrap();
        assert!(!text.contains("merge_source"));
        let back: UserPrefs =
            serde_json::from_str(r#"{"merge_source":"browser","locale":"en"}"#).unwrap();
        assert_eq!(back.merge_source.as_deref(), Some("browser"));
    }

    #[test]
    fn pre_merge_backup_name_strips_path_characters() {
        assert_eq!(
            pre_merge_backup_name("desktop").as_deref(),
            Some("prefs.before-merge-desktop.json")
        );
        assert_eq!(
            pre_merge_backup_name("../../etc").as_deref(),
            Some("prefs.before-merge-etc.json")
        );
        assert_eq!(pre_merge_backup_name("../.."), None);
    }
}
