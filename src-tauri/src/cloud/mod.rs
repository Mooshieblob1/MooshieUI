//! Cloud voice and video providers (ElevenLabs, fal.ai, Segmind).
//!
//! Each one is optional and runs on the user's own API key, billed by that
//! provider. Keys follow the NovelAI rules in `novelai::resolve_credential`:
//! the desktop owner (and a localhost or admin caller, which
//! `webserver::resolve_username` collapses to `None`) uses the keys in
//! `config`, and a named account uses only the keys in its own encrypted
//! `user_secrets` store, never the owner's.
//!
//! Compiles in both the desktop and the server build, so nothing here may
//! reference `tauri`.

pub mod elevenlabs;
pub mod scene;
pub mod takes;
pub mod video;
pub mod voice;
pub mod voice_prompt;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;
use crate::user_secrets::ProviderKey;

/// A cloud provider that bills the user's own key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloudProvider {
    ElevenLabs,
    Fal,
    Segmind,
}

impl CloudProvider {
    pub const ALL: [CloudProvider; 3] = [
        CloudProvider::ElevenLabs,
        CloudProvider::Fal,
        CloudProvider::Segmind,
    ];

    /// The id the frontend sends, matching the serde form.
    pub fn id(self) -> &'static str {
        match self {
            CloudProvider::ElevenLabs => "elevenlabs",
            CloudProvider::Fal => "fal",
            CloudProvider::Segmind => "segmind",
        }
    }

    pub fn parse(id: &str) -> Result<Self, AppError> {
        Self::ALL
            .into_iter()
            .find(|p| p.id() == id)
            .ok_or_else(|| AppError::Other(format!("Unknown cloud provider: {id}")))
    }

    fn display_name(self) -> &'static str {
        match self {
            CloudProvider::ElevenLabs => "ElevenLabs",
            CloudProvider::Fal => "fal.ai",
            CloudProvider::Segmind => "Segmind",
        }
    }

    fn secret_slot(self) -> ProviderKey {
        match self {
            CloudProvider::ElevenLabs => ProviderKey::ElevenLabs,
            CloudProvider::Fal => ProviderKey::Fal,
            CloudProvider::Segmind => ProviderKey::Segmind,
        }
    }

    fn owner_key(self, config: &crate::config::AppConfig) -> Option<String> {
        match self {
            CloudProvider::ElevenLabs => config.elevenlabs_api_key.clone(),
            CloudProvider::Fal => config.fal_api_key.clone(),
            CloudProvider::Segmind => config.segmind_api_key.clone(),
        }
    }

    fn owner_key_mut(self, config: &mut crate::config::AppConfig) -> &mut Option<String> {
        match self {
            CloudProvider::ElevenLabs => &mut config.elevenlabs_api_key,
            CloudProvider::Fal => &mut config.fal_api_key,
            CloudProvider::Segmind => &mut config.segmind_api_key,
        }
    }
}

/// A resolved provider key, tied to whoever pays for the request.
///
/// Redacted `Debug` for the same reason as `NaiCredential`: the ring-buffer
/// log ships in diagnostic exports. Never put this in anything serialised,
/// logged or written into media metadata.
#[derive(Clone)]
pub struct CloudCredential {
    provider: CloudProvider,
    key: String,
}

impl CloudCredential {
    pub fn provider(&self) -> CloudProvider {
        self.provider
    }

    pub fn as_str(&self) -> &str {
        &self.key
    }
}

impl std::fmt::Debug for CloudCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CloudCredential({}, ***)", self.provider.id())
    }
}

/// Resolve whose key pays for a request to `provider`. Call it at the request
/// edge, before any job id is minted, so a missing key fails the caller's own
/// call.
pub async fn resolve_credential(
    state: &Arc<AppState>,
    username: Option<&str>,
    provider: CloudProvider,
) -> Result<CloudCredential, AppError> {
    let owner_key = if username.is_none() {
        provider.owner_key(&*state.config.read().await)
    } else {
        None
    };
    select_credential(provider, owner_key, username, |user| {
        crate::user_secrets::load_key(user, provider.secret_slot())
    })
}

/// Account selection without the live secret store, for tests.
fn select_credential(
    provider: CloudProvider,
    owner_key: Option<String>,
    username: Option<&str>,
    load_user_key: impl FnOnce(&str) -> Option<String>,
) -> Result<CloudCredential, AppError> {
    let key = match username {
        None => owner_key,
        Some(user) => load_user_key(user),
    };
    key.map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .map(|key| CloudCredential { provider, key })
        .ok_or_else(|| {
            AppError::Other(match username {
                None => format!(
                    "No {} API key configured. Add one in Settings.",
                    provider.display_name()
                ),
                Some(_) => format!(
                    "No {} API key on this account. Add your own key in Settings.",
                    provider.display_name()
                ),
            })
        })
}

/// Store or clear a provider key for the caller. An empty string clears.
/// Returns whether a key is now configured.
///
/// A named account writes only its own encrypted store and can never reach
/// `config`, so no account can read, replace or clear the owner's key.
pub async fn set_api_key(
    state: &Arc<AppState>,
    username: Option<&str>,
    provider: CloudProvider,
    api_key: &str,
) -> Result<bool, AppError> {
    let trimmed = api_key.trim();
    let configured = !trimmed.is_empty();
    let value = configured.then_some(trimmed);
    match username {
        Some(user) => {
            crate::user_secrets::save_key(user, provider.secret_slot(), value)
                .map_err(AppError::Other)?;
        }
        None => {
            // Saved under the write lock so a concurrent `update_config`
            // cannot land in between and be overwritten by an older copy.
            let mut config = state.config.write().await;
            *provider.owner_key_mut(&mut config) = value.map(str::to_string);
            crate::config::save_config(&config).map_err(AppError::Other)?;
        }
    }
    Ok(configured)
}

/// Which providers the caller has a key for. Never carries a key.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudKeyStatus {
    pub elevenlabs: bool,
    pub fal: bool,
    pub segmind: bool,
}

pub async fn key_status(state: &Arc<AppState>, username: Option<&str>) -> CloudKeyStatus {
    let mut status = CloudKeyStatus::default();
    for provider in CloudProvider::ALL {
        let has = resolve_credential(state, username, provider).await.is_ok();
        match provider {
            CloudProvider::ElevenLabs => status.elevenlabs = has,
            CloudProvider::Fal => status.fal = has,
            CloudProvider::Segmind => status.segmind = has,
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_round_trip() {
        for provider in CloudProvider::ALL {
            assert_eq!(CloudProvider::parse(provider.id()).unwrap(), provider);
            assert_eq!(
                serde_json::to_value(provider).unwrap(),
                serde_json::json!(provider.id())
            );
        }
        assert!(CloudProvider::parse("novelai").is_err());
    }

    #[test]
    fn the_owner_uses_the_config_key() {
        let cred = select_credential(CloudProvider::Fal, Some(" fal-owner ".into()), None, |_| {
            panic!("the owner never reads an account store")
        })
        .unwrap();
        assert_eq!(cred.as_str(), "fal-owner");
    }

    #[test]
    fn a_named_account_never_falls_back_to_the_owner_key() {
        // The owner key is not even passed for a named account, but make the
        // rule explicit: an account without its own key gets an error.
        let err = select_credential(
            CloudProvider::ElevenLabs,
            Some("owner-key".into()),
            Some("alice"),
            |_| None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("this account"));

        let cred = select_credential(CloudProvider::ElevenLabs, None, Some("alice"), |user| {
            (user == "alice").then(|| "alice-key".to_string())
        })
        .unwrap();
        assert_eq!(cred.as_str(), "alice-key");
    }

    #[test]
    fn a_blank_key_counts_as_missing() {
        assert!(
            select_credential(CloudProvider::Segmind, Some("   ".into()), None, |_| None).is_err()
        );
    }

    #[test]
    fn debug_never_prints_the_key() {
        let cred = select_credential(CloudProvider::Fal, Some("fal-secret".into()), None, |_| {
            None
        })
        .unwrap();
        let text = format!("{cred:?}");
        assert!(!text.contains("fal-secret"));
        assert!(text.contains("fal"));
    }
}
