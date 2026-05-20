//! Per-user preferences storage.
//!
//! Stores client-side preferences server-side so they persist across devices
//! and operating systems when multiple clients connect to the same MooshieUI
//! server instance.
//!
//! Data stored at `{app_data_dir}/users/{username}/prefs.json`.
//!
//! The special username `"_admin"` is used for localhost / single-user
//! desktop sessions where no named account is active.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config;

/// All user-specific preferences that the frontend persists.
///
/// Every field is `Option` so clients can do partial PUTs (unknown fields are
/// ignored during deserialization).  A `None` field in a PUT body leaves the
/// server-side value for that category unchanged.
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
    /// Favourited artist slugs and user-defined categories.
    pub artist_favourites: Option<serde_json::Value>,
    /// Gallery board assignments and custom board names.
    pub gallery_boards: Option<serde_json::Value>,
    /// Autocomplete source/settings.
    pub autocomplete: Option<serde_json::Value>,
    /// Accessibility settings (vision simulator, info tips).
    pub accessibility: Option<serde_json::Value>,
    /// UI locale string (e.g. `"en"`, `"ja"`).
    pub locale: Option<serde_json::Value>,
    /// ISO 8601 timestamp of the last update (set by the server, not the client).
    pub updated_at: Option<String>,
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

/// Save a user's prefs to disk, creating parent directories as needed.
pub async fn save(username: &str, prefs: &UserPrefs) -> Result<(), String> {
    let path = prefs_path(username).ok_or_else(|| "Cannot resolve prefs path".to_string())?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(prefs).map_err(|e| e.to_string())?;
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
