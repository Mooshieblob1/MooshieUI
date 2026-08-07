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

use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::error::AppError;

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
        default_model: "grok-4",
        wire: Wire::OpenAiCompatible,
        oauth: false,
    },
    LlmProvider {
        id: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        default_model: "openai/gpt-4o-mini",
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
            cfg.llm_external_base_url = known.base_url.to_string();
            cfg.llm_external_model = known.default_model.to_string();
        }
    })
    .await
}

/// Store the API key for the current provider. An empty key clears it.
pub async fn store_key(
    config: &RwLock<AppConfig>,
    api_key: &str,
) -> Result<LlmProviderState, AppError> {
    mutate(config, |cfg| {
        cfg.llm_external_api_key = api_key.trim().to_string();
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
        cfg.llm_external_enabled = true;
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
}
