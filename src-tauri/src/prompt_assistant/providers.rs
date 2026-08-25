//! Registry of external LLM providers for the prompt assistant.
//!
//! A provider id selects the wire format and supplies the API root plus a
//! prefilled model name. Credentials and the effective model still live in the
//! existing `llm_external_*` config fields, so every call site keeps reading
//! config exactly as it did before providers existed.
//!
//! The config mutations live here rather than in the Tauri command layer so the
//! desktop commands and the browser-mode dispatch arms share one implementation
//! of the rules that protect the user's key.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::error::AppError;
use crate::state::AppState;

/// Wire format a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// `POST {base}/chat/completions`, `Authorization: Bearer <key>`,
    /// answer at `choices[0].message.content`.
    OpenAiCompatible,
    /// `POST {base}/messages`, `x-api-key` + `anthropic-version` headers,
    /// `system` as a top-level field, answer at `content[0].text`.
    Anthropic,
}

/// A known external LLM provider.
pub struct LlmProvider {
    /// Stable id persisted in `AppConfig::llm_provider`.
    pub id: &'static str,
    /// API root written into `llm_external_base_url` when the provider is
    /// selected. Empty means the user supplies their own.
    pub base_url: &'static str,
    /// Model name prefilled on first selection. Always user-overridable, and
    /// `list_external_llm_models` fetches the live list from the provider.
    pub default_model: &'static str,
    pub wire: Wire,
    /// Whether this build implements an OAuth sign-in flow for the provider.
    pub oauth: bool,
}

/// Every provider the prompt assistant knows how to talk to.
///
/// `custom` is the escape hatch for self-hosted OpenAI-compatible servers
/// (Ollama, vLLM, LM Studio) and for any vendor not listed here. It is also the
/// default for installs that predate the provider field, so their existing
/// base URL and key keep working untouched.
const PROVIDERS: &[LlmProvider] = &[
    LlmProvider {
        id: "anthropic",
        base_url: "https://api.anthropic.com/v1",
        default_model: "claude-sonnet-5",
        wire: Wire::Anthropic,
        oauth: false,
    },
    LlmProvider {
        id: "openai",
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o-mini",
        wire: Wire::OpenAiCompatible,
        oauth: false,
    },
    LlmProvider {
        id: "xai",
        base_url: "https://api.x.ai/v1",
        default_model: "grok-4.5",
        wire: Wire::OpenAiCompatible,
        oauth: false,
    },
    // The same host and wire as `xai`, reached with a signed-in session instead
    // of a prepaid API key, which is what lets a SuperGrok subscription pay for
    // the requests. Split into its own id rather than made a mode of `xai`
    // because the two carry different credentials and switching between them
    // has to discard the old one.
    LlmProvider {
        id: "xai-oauth",
        base_url: "https://api.x.ai/v1",
        default_model: "grok-4.5",
        wire: Wire::OpenAiCompatible,
        oauth: true,
    },
    LlmProvider {
        id: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        default_model: "openai/gpt-4o-mini",
        wire: Wire::OpenAiCompatible,
        oauth: true,
    },
    // Nous Research's Portal is an aggregator like OpenRouter: one credential
    // reaches Hermes plus a few hundred third-party models, all on the
    // OpenAI-compatible wire. Its sign-in is a standards-track OAuth flow with
    // dynamic client registration, so unlike xAI, whose client ids are handed
    // out by hand, this install registers itself as a client on first use.
    LlmProvider {
        id: "nous",
        base_url: "https://inference-api.nousresearch.com/v1",
        default_model: "nousresearch/hermes-4-405b",
        wire: Wire::OpenAiCompatible,
        oauth: true,
    },
    LlmProvider {
        id: "custom",
        base_url: "",
        default_model: "",
        wire: Wire::OpenAiCompatible,
        oauth: false,
    },
];

/// The provider id used when config carries none (every pre-provider install).
pub const DEFAULT_PROVIDER: &str = "custom";

/// Look up a provider by id.
pub fn provider(id: &str) -> Option<&'static LlmProvider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Wire format for a provider id.
///
/// Unknown ids fall back to OpenAI-compatible, which is what every install
/// spoke before the registry existed.
pub fn wire_for(id: &str) -> Wire {
    provider(id)
        .map(|p| p.wire)
        .unwrap_or(Wire::OpenAiCompatible)
}

/// The API root to actually call: the configured base URL when the user set
/// one, otherwise the provider's own root. `custom` has no root of its own, so
/// an unset base stays empty and the caller reports it as a misconfiguration.
pub fn effective_base_url(id: &str, configured: &str) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    provider(id).map(|p| p.base_url).unwrap_or("").to_string()
}

/// What the settings UI needs to render the provider row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LlmProviderState {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    /// Whether a key is stored. The key itself never leaves Rust.
    pub api_key_configured: bool,
    /// Whether this build can sign in to the provider without an API key.
    pub oauth: bool,
    /// Whether the external path is the one the assistant will actually use.
    pub enabled: bool,
    /// The xAI OAuth client id the operator supplied, or empty. Public by
    /// design, so unlike the key it is safe to hand back to the settings UI,
    /// which needs to show what is configured.
    pub xai_client_id: String,
    /// The xAI scope override, or empty for the built-in default.
    pub xai_scope: String,
    /// The client id sign-in falls back to while the override above is empty,
    /// so the settings UI can show which one is actually in play.
    pub xai_client_id_default: String,
}

/// Project the provider-relevant slice of config, minus the secret.
pub fn state_of(cfg: &AppConfig) -> LlmProviderState {
    LlmProviderState {
        provider: cfg.llm_provider.clone(),
        base_url: cfg.llm_external_base_url.clone(),
        model: cfg.llm_external_model.clone(),
        api_key_configured: !cfg.llm_external_api_key.trim().is_empty(),
        oauth: provider(&cfg.llm_provider).is_some_and(|p| p.oauth),
        enabled: cfg.llm_external_enabled,
        xai_client_id: cfg.llm_xai_client_id.clone(),
        xai_scope: cfg.llm_xai_scope.clone(),
        xai_client_id_default: super::oauth::XAI_DEFAULT_CLIENT_ID.to_string(),
    }
}

/// Read the current provider settings without exposing the key.
pub async fn read_state(config: &RwLock<AppConfig>) -> LlmProviderState {
    state_of(&*config.read().await)
}

/// Mutate the live config under one write lock, persist it, and project the
/// result. Holding the lock across the save keeps a concurrent `update_config`
/// from writing config.json between our mutation and our save.
async fn mutate<F>(config: &RwLock<AppConfig>, edit: F) -> Result<LlmProviderState, AppError>
where
    F: FnOnce(&mut AppConfig),
{
    let mut cfg = config.write().await;
    edit(&mut cfg);
    crate::config::save_config(&cfg).map_err(AppError::Other)?;
    Ok(state_of(&cfg))
}

/// Switch provider, prefilling its API root and model.
///
/// Changing provider discards the stored API key: a key is issued by one
/// provider, and sending it to another would hand a third party the user's
/// credentials. Selecting the provider already in use is a no-op, so re-opening
/// the dropdown never costs the user their key.
pub async fn select(
    config: &RwLock<AppConfig>,
    provider_id: &str,
) -> Result<LlmProviderState, AppError> {
    let known = provider(provider_id)
        .ok_or_else(|| AppError::LlmError(format!("Unknown LLM provider: {provider_id}")))?;
    mutate(config, |cfg| {
        if cfg.llm_provider != provider_id {
            cfg.llm_provider = provider_id.to_string();
            cfg.llm_external_api_key = String::new();
            clear_oauth_session(cfg);
            cfg.llm_external_base_url = known.base_url.to_string();
            cfg.llm_external_model = known.default_model.to_string();
        }
    })
    .await
}

/// Forget an OAuth session. Called wherever the stored key is replaced or
/// cleared: a refresh token outlives the access token minted from it, so
/// leaving one behind would let the next refresh resurrect a credential the
/// user believes they got rid of.
fn clear_oauth_session(cfg: &mut AppConfig) {
    cfg.llm_oauth_refresh_token = String::new();
    cfg.llm_oauth_client_id = String::new();
    cfg.llm_oauth_expires_at = 0;
}

/// Store the API key for the current provider. An empty key clears it.
pub async fn store_key(
    config: &RwLock<AppConfig>,
    api_key: &str,
) -> Result<LlmProviderState, AppError> {
    mutate(config, |cfg| {
        cfg.llm_external_api_key = api_key.trim().to_string();
        // A pasted key supersedes any signed-in session, and an empty one is
        // the sign-out path, so either way the OAuth session goes with it.
        clear_oauth_session(cfg);
        // A key is only worth storing if the external path is on, so pasting one
        // turns it on. Clearing the key turns it back off rather than leaving the
        // assistant pointed at a provider it can no longer authenticate to.
        cfg.llm_external_enabled = !cfg.llm_external_api_key.is_empty();
    })
    .await
}

/// Store a key an OAuth flow issued, switching provider if needed.
pub async fn store_oauth_key(
    config: &RwLock<AppConfig>,
    provider_id: &str,
    key: String,
) -> Result<LlmProviderState, AppError> {
    let known = provider(provider_id)
        .ok_or_else(|| AppError::LlmError(format!("Unknown LLM provider: {provider_id}")))?;
    mutate(config, move |cfg| {
        if cfg.llm_provider != provider_id {
            cfg.llm_external_model = known.default_model.to_string();
        }
        cfg.llm_provider = provider_id.to_string();
        cfg.llm_external_base_url = known.base_url.to_string();
        cfg.llm_external_api_key = key;
        // OpenRouter's flow hands back a durable key rather than a token pair,
        // so there is nothing to refresh and any session from a previous
        // provider must not linger.
        clear_oauth_session(cfg);
        cfg.llm_external_enabled = true;
    })
    .await
}

/// Store the result of a sign-in whose access token expires, switching provider
/// if needed.
///
/// The access token goes into `llm_external_api_key` so every existing call
/// site keeps reading credentials from exactly one field; the refresh token,
/// registered client id and expiry ride alongside it so `ensure_fresh_token`
/// can mint a replacement when it runs out.
pub async fn store_oauth_session(
    config: &RwLock<AppConfig>,
    provider_id: &str,
    session: super::oauth::OauthSession,
) -> Result<LlmProviderState, AppError> {
    let known = provider(provider_id)
        .ok_or_else(|| AppError::LlmError(format!("Unknown LLM provider: {provider_id}")))?;
    mutate(config, move |cfg| {
        if cfg.llm_provider != provider_id {
            cfg.llm_external_model = known.default_model.to_string();
        }
        cfg.llm_provider = provider_id.to_string();
        cfg.llm_external_base_url = known.base_url.to_string();
        cfg.llm_external_api_key = session.access_token;
        cfg.llm_oauth_refresh_token = session.refresh_token;
        cfg.llm_oauth_client_id = session.client_id;
        cfg.llm_oauth_expires_at = session.expires_at;
        cfg.llm_external_enabled = true;
    })
    .await
}

/// Seconds before expiry at which a token is already treated as spent. Covers
/// the round trip plus any clock skew between us and the provider, so we never
/// send a token that dies in flight.
const REFRESH_SKEW_SECS: i64 = 120;

/// Whether a stored credential needs replacing before it is used.
///
/// `expires_at == 0` marks a credential that never expires (every API key, and
/// OpenRouter's issued key), so those are always fresh. Split out from
/// `ensure_fresh_token` because it is the part worth testing directly.
fn needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at != 0 && now >= expires_at - REFRESH_SKEW_SECS
}

/// Renew the stored access token if it is at or near expiry.
///
/// Call this before reading credentials out of config for a request. It is a
/// no-op for every provider that issues non-expiring credentials, which is all
/// of them except Nous Portal, so the common path costs one integer compare
/// under a read lock.
///
/// A refresh failure is deliberately *not* fatal: the stored token may still
/// have a few seconds on it, and letting the actual API call report the real
/// error beats masking it with a refresh error. The token is simply left alone
/// for the next attempt to retry.
pub async fn ensure_fresh_token(client: &reqwest::Client, config: &RwLock<AppConfig>) {
    let (provider_id, refresh_token, client_id, expires_at) = {
        let cfg = config.read().await;
        (
            cfg.llm_provider.clone(),
            cfg.llm_oauth_refresh_token.clone(),
            cfg.llm_oauth_client_id.clone(),
            cfg.llm_oauth_expires_at,
        )
    };
    if refresh_token.is_empty() || !needs_refresh(expires_at, chrono::Utc::now().timestamp()) {
        return;
    }

    // Which issuer to go back to is a property of the provider, not of the
    // stored session: both write the same three fields, and redeeming a Portal
    // refresh token at xAI (or the reverse) would just burn it.
    let attempt = match provider_id.as_str() {
        "nous" => super::oauth::refresh_nous(client, &client_id, &refresh_token).await,
        "xai-oauth" => super::oauth::refresh_xai(client, &client_id, &refresh_token).await,
        // Every other provider holds a key that does not expire, so there is
        // nothing to refresh even if a stale session is still on disk.
        _ => return,
    };
    let refreshed = match attempt {
        Ok(s) => s,
        Err(e) => {
            log::warn!("{provider_id} token refresh failed, using the stored token: {e}");
            return;
        }
    };

    // Re-check under the write lock: a concurrent request may have refreshed
    // while we were on the wire, and clobbering its newer token with ours would
    // waste a rotation. Providers that rotate the refresh token invalidate the
    // old one, so the loser of that race must not write.
    let mut cfg = config.write().await;
    if cfg.llm_oauth_refresh_token != refresh_token {
        return;
    }
    cfg.llm_external_api_key = refreshed.access_token;
    cfg.llm_oauth_refresh_token = refreshed.refresh_token;
    cfg.llm_oauth_expires_at = refreshed.expires_at;
    if let Err(e) = crate::config::save_config(&cfg) {
        log::warn!("Could not persist the refreshed {provider_id} token: {e}");
    }
}

/// Run the xAI device sign-in and store the session it produces.
///
/// Lives here rather than in the command layer because the device grant has no
/// redirect, so unlike the OpenRouter and Portal flows it is reachable from
/// browser mode too and both entry points need the same implementation.
///
/// The user code is emitted the moment it exists, while this call is still
/// blocked polling: it is the whole point of the flow, and nothing else will
/// show it. Both transports fire because a desktop instance can have LAN
/// browser clients attached at the same time.
pub async fn connect_xai_session(state: &Arc<AppState>) -> Result<LlmProviderState, AppError> {
    let (client_id, scope) = {
        let cfg = state.config.read().await;
        (cfg.llm_xai_client_id.clone(), cfg.llm_xai_scope.clone())
    };
    // Resolved before the flow starts: the callback below is synchronous and
    // cannot take the async lock the handle sits behind.
    #[cfg(feature = "desktop")]
    let app = state.app_handle.lock().await.clone();
    let emitter = Arc::clone(state);

    let session = super::oauth::connect_xai(&state.http_client, &client_id, &scope, move |auth| {
        let payload = serde_json::json!({
            "provider": "xai-oauth",
            "user_code": auth.user_code,
            "verification_uri": auth.verification_uri,
            "verification_uri_complete": auth.best_uri(),
        });
        emitter.broadcast("llm:device_code", payload.clone());
        #[cfg(feature = "desktop")]
        if let Some(app) = app.as_ref() {
            use tauri::Emitter;
            let _ = app.emit("llm:device_code", payload);
        }
    })
    .await?;

    store_oauth_session(&state.config, "xai-oauth", session).await
}

/// Store the operator-supplied xAI OAuth client id and scope.
///
/// Kept apart from [`clear_oauth_session`]: this is configuration for how to
/// sign in, so signing out must not erase it. An empty scope means the built-in
/// default, which is why it is stored blank rather than expanded here.
pub async fn set_xai_client(
    config: &RwLock<AppConfig>,
    client_id: &str,
    scope: &str,
) -> Result<LlmProviderState, AppError> {
    mutate(config, |cfg| {
        cfg.llm_xai_client_id = client_id.trim().to_string();
        cfg.llm_xai_scope = scope.trim().to_string();
    })
    .await
}

/// Set the model to use with the current provider.
pub async fn set_model(
    config: &RwLock<AppConfig>,
    model: &str,
) -> Result<LlmProviderState, AppError> {
    mutate(config, |cfg| {
        cfg.llm_external_model = model.trim().to_string();
    })
    .await
}

/// Point a self-hosted (`custom`) provider at its server.
pub async fn set_base_url(
    config: &RwLock<AppConfig>,
    base_url: &str,
) -> Result<LlmProviderState, AppError> {
    mutate(config, |cfg| {
        cfg.llm_external_base_url = base_url.trim().trim_end_matches('/').to_string();
    })
    .await
}

/// Ask the current provider which models the stored key can actually use.
pub async fn list_available_models(
    client: &reqwest::Client,
    config: &RwLock<AppConfig>,
) -> Result<Vec<String>, AppError> {
    ensure_fresh_token(client, config).await;
    let (provider_id, base_url, api_key) = {
        let cfg = config.read().await;
        (
            cfg.llm_provider.clone(),
            cfg.llm_external_base_url.clone(),
            cfg.llm_external_api_key.clone(),
        )
    };
    super::server::list_models(client, &provider_id, &base_url, &api_key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_exists_and_is_openai_compatible() {
        let p = provider(DEFAULT_PROVIDER).expect("default provider must be in the registry");
        assert_eq!(p.wire, Wire::OpenAiCompatible);
        assert!(
            p.base_url.is_empty(),
            "custom must not pin a base URL; the user supplies it"
        );
    }

    #[test]
    fn config_default_matches_registry_default() {
        // `config::default_llm_provider` hardcodes the id because `config` is
        // compiled in builds that gate this module out.
        assert_eq!(
            crate::config::AppConfig::default().llm_provider,
            DEFAULT_PROVIDER
        );
    }

    #[test]
    fn unknown_provider_falls_back_to_openai_compatible() {
        assert!(provider("not-a-provider").is_none());
        assert_eq!(wire_for("not-a-provider"), Wire::OpenAiCompatible);
    }

    #[test]
    fn only_anthropic_uses_the_anthropic_wire() {
        for p in PROVIDERS {
            let expected = if p.id == "anthropic" {
                Wire::Anthropic
            } else {
                Wire::OpenAiCompatible
            };
            assert_eq!(p.wire, expected, "wrong wire for {}", p.id);
        }
    }

    #[test]
    fn hosted_providers_pin_an_https_base_url_and_a_model() {
        for p in PROVIDERS.iter().filter(|p| p.id != "custom") {
            assert!(
                p.base_url.starts_with("https://"),
                "{} must pin an https base URL",
                p.id
            );
            assert!(
                !p.default_model.is_empty(),
                "{} needs a prefill model",
                p.id
            );
        }
    }

    #[test]
    fn nous_is_registered_with_sign_in() {
        let p = provider("nous").expect("the Nous provider must be in the registry");
        assert!(
            p.oauth,
            "the settings UI only shows sign-in when this is set"
        );
        assert_eq!(p.wire, Wire::OpenAiCompatible);
        // Sign-in happens on portal.nousresearch.com; inference does not.
        assert_eq!(p.base_url, "https://inference-api.nousresearch.com/v1");
    }

    #[test]
    fn a_credential_with_no_expiry_never_refreshes() {
        // API keys and OpenRouter's issued key store `0`. Treating that as a
        // deadline in 1970 would fire a refresh on every single request.
        assert!(!needs_refresh(0, 1_000_000));
    }

    #[test]
    fn refresh_fires_inside_the_skew_window_and_not_before() {
        let expires_at = 1_000_000;
        assert!(!needs_refresh(
            expires_at,
            expires_at - REFRESH_SKEW_SECS - 1
        ));
        // Exactly at the edge counts: a token about to die mid-flight is no
        // more usable than one already dead.
        assert!(needs_refresh(expires_at, expires_at - REFRESH_SKEW_SECS));
        assert!(needs_refresh(expires_at, expires_at + 1));
    }
}
