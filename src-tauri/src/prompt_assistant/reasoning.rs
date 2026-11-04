//! How hard a hosted model thinks before it answers the prompt assistant.
//!
//! Most hosted models now reason before they answer, and each vendor picks a
//! default for the general case: Grok 4.5 to 4.7 run at `high`, the GPT-5 and
//! GPT-6 families at `medium`, current Claude models think at `high` unless
//! told otherwise. A prompt rewrite gains next to nothing from that, but the
//! thinking costs time and tokens: Grok 4.6 and 4.7 at `high` regularly ran
//! past the request timeout, which the fallback chain then read as the model
//! being unavailable.
//!
//! The owner picks one [`Setting`] (`AppConfig::llm_reasoning_effort`), and
//! this module turns it into what each model accepts. The rules:
//!
//! * `Low` (the default) asks for `low`, unless the model's own default is
//!   already lighter (`minimal`, `none`), in which case the default stays.
//! * `Medium` and `High` ask for that level.
//! * `ModelDefault` sends nothing.
//! * A level the model does not offer snaps to the nearest one it does,
//!   preferring the lighter side, and never to `none`.
//!
//! Which levels a model offers comes from a table for xAI, OpenAI and
//! Anthropic, from OpenRouter's own model metadata for OpenRouter, and from
//! the companion's model list for ChatGPT and Gemini sign-in. A model this
//! module does not recognise is left exactly as it was.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

/// Every effort level any provider uses, lightest first.
const LEVELS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

fn rank(level: &str) -> Option<usize> {
    LEVELS.iter().position(|l| *l == level)
}

fn levels(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// The owner's choice, persisted as `AppConfig::llm_reasoning_effort`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    /// Send nothing and let each model use its own default.
    ModelDefault,
    Low,
    Medium,
    High,
}

impl Setting {
    /// Parse the stored value. Anything unrecognised, including an empty
    /// string from a config that predates the field, is the default `Low`.
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "default" => Self::ModelDefault,
            "medium" => Self::Medium,
            "high" => Self::High,
            _ => Self::Low,
        }
    }

    fn level(self) -> Option<&'static str> {
        match self {
            Self::ModelDefault => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
        }
    }
}

/// The effort levels one model accepts, and the one it uses when told nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelEfforts {
    pub supported: Vec<String>,
    pub default: Option<String>,
}

impl ModelEfforts {
    fn new(supported: &[&str], default: &str) -> Self {
        Self {
            supported: levels(supported),
            default: Some(default.to_string()),
        }
    }
}

/// The effort to send for `setting`, or `None` to leave the model's default.
pub fn pick(setting: Setting, model: &ModelEfforts) -> Option<String> {
    let target = setting.level()?;
    let wanted = rank(target)?;
    if setting == Setting::Low {
        // A model that already defaults below `low` is faster left alone.
        if let Some(default) = model.default.as_deref().and_then(rank) {
            if default < wanted {
                return None;
            }
        }
    }
    if model.supported.iter().any(|s| s == target) {
        return Some(target.to_string());
    }
    let usable: Vec<(usize, &String)> = model
        .supported
        .iter()
        .filter_map(|s| rank(s).filter(|r| *r > 0).map(|r| (r, s)))
        .collect();
    let below = usable
        .iter()
        .filter(|(r, _)| *r < wanted)
        .max_by_key(|(r, _)| *r);
    let above = usable
        .iter()
        .filter(|(r, _)| *r > wanted)
        .min_by_key(|(r, _)| *r);
    below.or(above).map(|(_, s)| s.to_string())
}

/// The level the model will run at: what was sent, else its own default.
pub fn effective<'a>(picked: Option<&'a str>, model: &'a ModelEfforts) -> Option<&'a str> {
    picked.or(model.default.as_deref())
}

/// Extra output tokens to allow for thinking, on providers that count it
/// against the same `max_tokens` as the answer (OpenAI, OpenRouter, Anthropic).
///
/// The prompt assistant asks for 96 to 384 tokens of answer; a reasoning
/// model handed that budget alone can spend all of it thinking and return
/// nothing. A model that has not reasoned for long is not charged for room it
/// leaves unused.
pub fn headroom(level: Option<&str>) -> u32 {
    match level {
        None | Some("none") => 0,
        Some("minimal") | Some("low") => 2048,
        Some("medium") => 8192,
        Some(_) => 16384,
    }
}

// ---------------------------------------------------------------------------
// xAI
// ---------------------------------------------------------------------------

/// Whether an xAI model reasons at all (`grok-4.20-*-non-reasoning` does not).
pub fn xai_reasons(model: &str) -> bool {
    !model.to_ascii_lowercase().contains("non-reasoning")
}

/// The effort levels an xAI model accepts, from xAI's model pages.
///
/// `grok-4.3` defaults to `low`; 4.5 to 4.7 default to `high`, with `xhigh`
/// from 4.6 on. `grok-4.20` and `grok-build` reason without taking an effort,
/// so they, like any model this table does not know, get none.
pub fn xai(model: &str) -> Option<ModelEfforts> {
    let m = model.trim().to_ascii_lowercase();
    if !xai_reasons(&m) {
        return None;
    }
    let rest = m.strip_prefix("grok-")?;
    let (major, minor) = version(rest)?;
    match (major, minor) {
        (4, Some(3)) => Some(ModelEfforts::new(
            &["none", "low", "medium", "high", "xhigh"],
            "low",
        )),
        (4, Some(5)) => Some(ModelEfforts::new(&["low", "medium", "high"], "high")),
        // 4.20 is a different line that takes no effort.
        (4, Some(minor)) if (6..20).contains(&minor) => Some(ModelEfforts::new(
            &["low", "medium", "high", "xhigh"],
            "high",
        )),
        (major, _) if major >= 5 => Some(ModelEfforts::new(
            &["low", "medium", "high", "xhigh"],
            "high",
        )),
        _ => None,
    }
}

/// `major.minor` (or a bare `major`) at the start of a model id remainder.
fn version(rest: &str) -> Option<(u32, Option<u32>)> {
    let head = rest.split(['-', '_']).next()?;
    let mut parts = head.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = match parts.next() {
        Some(m) => Some(m.parse().ok()?),
        None => None,
    };
    Some((major, minor))
}

// ---------------------------------------------------------------------------
// OpenAI
// ---------------------------------------------------------------------------

/// An OpenAI reasoning model, or `None` for a model that does not reason.
///
/// Every reasoning model rejects `max_tokens` (it takes
/// `max_completion_tokens`, which also covers its reasoning) and any
/// `temperature` but the default, so the request shape changes even when the
/// effort is left alone. `-pro` models only run at their own level and get an
/// empty list, which [`pick`] turns into "send nothing".
pub fn openai(model: &str) -> Option<ModelEfforts> {
    let m = model.trim().to_ascii_lowercase();
    let m = m.strip_prefix("openai/").unwrap_or(&m);
    if [
        "-chat",
        "image",
        "audio",
        "realtime",
        "search",
        "transcribe",
        "tts",
    ]
    .iter()
    .any(|k| m.contains(k))
    {
        return None;
    }
    let o_series = m.starts_with('o') && m[1..].starts_with(|c: char| c.is_ascii_digit());
    let gpt_major = m.strip_prefix("gpt-").and_then(version);
    let reasoning = o_series || matches!(gpt_major, Some((major, _)) if major >= 5);
    if !reasoning {
        return None;
    }
    if m.contains("-pro") {
        return Some(ModelEfforts {
            supported: Vec::new(),
            default: None,
        });
    }
    Some(match gpt_major {
        // The original GPT-5 family has `minimal` and no `xhigh`.
        Some((5, None)) => ModelEfforts::new(&["minimal", "low", "medium", "high"], "medium"),
        Some(_) => ModelEfforts::new(&["low", "medium", "high", "xhigh"], "medium"),
        None => ModelEfforts::new(&["low", "medium", "high"], "medium"),
    })
}

// ---------------------------------------------------------------------------
// Anthropic
// ---------------------------------------------------------------------------

/// How one Claude model treats sampling and thinking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claude {
    /// Whether the model still accepts `temperature`. Opus 4.7 and every
    /// model from the 5 generation on reject any value but the default.
    pub sampling: bool,
    /// Whether the model thinks when the request does not mention thinking.
    /// From the 5 generation on it does, and the thinking counts against
    /// `max_tokens`.
    pub thinks: bool,
    /// The `output_config.effort` levels the model takes, when it thinks.
    pub efforts: ModelEfforts,
}

/// The behaviour of a Claude model id, from Anthropic's model notes.
pub fn anthropic(model: &str) -> Claude {
    let legacy = Claude {
        sampling: true,
        thinks: false,
        efforts: ModelEfforts::default(),
    };
    let m = model.trim().to_ascii_lowercase();
    let mut parts = m.split('-').skip_while(|p| *p == "claude");
    let Some(family) = parts.next() else {
        return legacy;
    };
    let all = ["low", "medium", "high", "xhigh", "max"];
    if family == "fable" || family == "mythos" {
        return Claude {
            sampling: false,
            thinks: true,
            efforts: ModelEfforts::new(&all, "high"),
        };
    }
    if !matches!(family, "opus" | "sonnet" | "haiku") {
        // `claude-3-5-sonnet-...` and anything else unrecognised.
        return legacy;
    }
    let number = |p: Option<&str>| {
        p.filter(|s| s.len() <= 2)
            .and_then(|s| s.parse::<u32>().ok())
    };
    let Some(major) = number(parts.next()) else {
        return legacy;
    };
    let minor = number(parts.next()).unwrap_or(0);
    if major >= 5 {
        // Opus 5.5 and Haiku 5.5 moved the default down to `medium`.
        let default = if minor >= 5 && matches!(family, "opus" | "haiku") {
            "medium"
        } else {
            "high"
        };
        return Claude {
            sampling: false,
            thinks: true,
            efforts: ModelEfforts::new(&all, default),
        };
    }
    if major == 4 && minor >= 7 {
        // Opus 4.7 and 4.8: no sampling parameters, no thinking unless asked.
        return Claude {
            sampling: false,
            thinks: false,
            efforts: ModelEfforts::default(),
        };
    }
    legacy
}

// ---------------------------------------------------------------------------
// OpenRouter
// ---------------------------------------------------------------------------

/// A reasoning model as OpenRouter describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouterModel {
    pub efforts: ModelEfforts,
    /// Whether it reasons when the request says nothing. A model that only
    /// reasons on request is left that way: turning it on would only slow it.
    pub on_by_default: bool,
}

/// One entry of `GET /models`, or `None` for a model without a `reasoning`
/// object, which OpenRouter's docs say is a model that does not reason.
fn router_model(reasoning: &Value) -> Option<RouterModel> {
    if !reasoning.is_object() {
        return None;
    }
    let supported = match reasoning["supported_efforts"].as_array() {
        Some(list) => list
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        // `null` means every gateway level is accepted.
        None if reasoning.get("supported_efforts").is_some() => levels(&LEVELS),
        // Absent means the model takes no effort at all.
        None => Vec::new(),
    };
    Some(RouterModel {
        efforts: ModelEfforts {
            supported,
            default: reasoning["default_effort"].as_str().map(str::to_string),
        },
        on_by_default: reasoning["mandatory"] == true || reasoning["default_enabled"] == true,
    })
}

/// How long a fetched model list stays good, and how long a failed fetch is
/// remembered before the next request tries again.
const ROUTER_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const ROUTER_RETRY: Duration = Duration::from_secs(5 * 60);

type RouterCache = (Instant, Duration, HashMap<String, RouterModel>);
static ROUTER_CACHE: Mutex<Option<RouterCache>> = Mutex::new(None);

/// OpenRouter's description of `model`, fetched once and cached.
///
/// `GET {base}/models` is public and lists every model's `reasoning` object
/// (supported levels, default level, whether reasoning is always on). A
/// failed fetch means "unknown", which leaves the request untouched.
pub async fn openrouter(client: &reqwest::Client, base: &str, model: &str) -> Option<RouterModel> {
    {
        let cache = ROUTER_CACHE.lock().ok()?;
        if let Some((at, ttl, models)) = cache.as_ref() {
            if at.elapsed() < *ttl {
                return models.get(model.trim()).cloned();
            }
        }
    }
    let fetched = fetch_router_models(client, base).await;
    let (ttl, models) = match fetched {
        Ok(models) => (ROUTER_TTL, models),
        Err(e) => {
            log::warn!("[prompt-assistant] could not read OpenRouter's model list: {e}");
            (ROUTER_RETRY, HashMap::new())
        }
    };
    let found = models.get(model.trim()).cloned();
    if let Ok(mut cache) = ROUTER_CACHE.lock() {
        *cache = Some((Instant::now(), ttl, models));
    }
    found
}

async fn fetch_router_models(
    client: &reqwest::Client,
    base: &str,
) -> Result<HashMap<String, RouterModel>, crate::error::AppError> {
    let url = format!("{}/models", base.trim().trim_end_matches('/'));
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| crate::error::AppError::LlmError(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(crate::error::AppError::LlmError(format!(
            "HTTP {}",
            resp.status()
        )));
    }
    let v =
        super::server::bounded_json(resp, super::server::RESPONSE_LIMIT, "OpenRouter model list")
            .await?;
    Ok(v["data"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|m| {
                    Some((
                        m["id"].as_str()?.to_string(),
                        router_model(&m["reasoning"])?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Companions
// ---------------------------------------------------------------------------

/// The `effort` for a ChatGPT (Codex app-server) turn, from `model/list`.
///
/// `model` empty means the account's default model, the entry marked
/// `isDefault`.
pub fn codex(setting: Setting, models: &Value, model: &str) -> Option<String> {
    let list = models.as_array()?;
    let model = model.trim();
    let entry = list.iter().find(|m| {
        if model.is_empty() {
            m["isDefault"] == true
        } else {
            m["model"] == model || m["id"] == model
        }
    })?;
    let efforts = ModelEfforts {
        supported: entry["supportedReasoningEfforts"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|e| e["reasoningEffort"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        default: entry["defaultReasoningEffort"].as_str().map(str::to_string),
    };
    pick(setting, &efforts)
}

/// The ACP config option and value that set the thinking level for a Gemini
/// (Antigravity) session, when the session offers one.
///
/// ACP marks such an option with the `thought_level` category; its values can
/// be listed flat or in groups. Values are matched case-insensitively and the
/// option's own spelling is what goes back.
pub fn acp_thought_level(setting: Setting, options: &Value) -> Option<(String, String)> {
    let option = options
        .as_array()?
        .iter()
        .find(|o| o["category"] == "thought_level")?;
    let id = option["id"].as_str()?.to_string();
    let mut values = Vec::new();
    for item in option["options"].as_array()? {
        match item["options"].as_array() {
            Some(group) => values.extend(group.iter().filter_map(|v| v["value"].as_str())),
            None => values.extend(item["value"].as_str()),
        }
    }
    let lower = |s: &str| s.trim().to_ascii_lowercase();
    let efforts = ModelEfforts {
        supported: values.iter().map(|v| lower(v)).collect(),
        default: option["currentValue"].as_str().map(lower),
    };
    let chosen = pick(setting, &efforts)?;
    let value = values.into_iter().find(|v| lower(v) == chosen)?;
    if option["currentValue"].as_str() == Some(value) {
        return None;
    }
    Some((id, value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn efforts(supported: &[&str], default: &str) -> ModelEfforts {
        ModelEfforts::new(supported, default)
    }

    #[test]
    fn settings_parse_with_low_as_the_fallback() {
        assert_eq!(Setting::parse("default"), Setting::ModelDefault);
        assert_eq!(Setting::parse("medium"), Setting::Medium);
        assert_eq!(Setting::parse(" high "), Setting::High);
        assert_eq!(Setting::parse("low"), Setting::Low);
        assert_eq!(Setting::parse(""), Setting::Low);
        assert_eq!(Setting::parse("xhigh"), Setting::Low);
    }

    #[test]
    fn low_is_sent_when_offered() {
        let grok = efforts(&["low", "medium", "high", "xhigh"], "high");
        assert_eq!(pick(Setting::Low, &grok).as_deref(), Some("low"));
        assert_eq!(pick(Setting::High, &grok).as_deref(), Some("high"));
        assert_eq!(pick(Setting::ModelDefault, &grok), None);
    }

    #[test]
    fn a_lighter_default_is_kept_for_low() {
        let lite = efforts(&["high", "medium", "low", "minimal"], "minimal");
        assert_eq!(pick(Setting::Low, &lite), None);
        // An explicit Medium still raises it, since that is what was asked.
        assert_eq!(pick(Setting::Medium, &lite).as_deref(), Some("medium"));
    }

    #[test]
    fn a_missing_level_snaps_lighter_first_and_never_to_none() {
        let image = efforts(&["high", "minimal", "none"], "minimal");
        assert_eq!(pick(Setting::Medium, &image).as_deref(), Some("minimal"));
        let pro = efforts(&["xhigh", "high", "medium"], "medium");
        assert_eq!(pick(Setting::Low, &pro).as_deref(), Some("medium"));
        let only_none = efforts(&["none"], "none");
        assert_eq!(pick(Setting::High, &only_none), None);
        assert_eq!(pick(Setting::Low, &ModelEfforts::default()), None);
    }

    #[test]
    fn xai_table_matches_the_model_pages() {
        assert_eq!(xai("grok-4.5").unwrap().default.as_deref(), Some("high"));
        assert!(!xai("grok-4.5")
            .unwrap()
            .supported
            .contains(&"xhigh".to_string()));
        assert!(xai("grok-4.7")
            .unwrap()
            .supported
            .contains(&"xhigh".to_string()));
        assert!(xai("grok-4.6-latest").is_some());
        assert_eq!(xai("grok-4.3").unwrap().default.as_deref(), Some("low"));
        assert_eq!(
            pick(Setting::Low, &xai("grok-4.3").unwrap()).as_deref(),
            Some("low")
        );
        assert!(xai("grok-4.20-0309-reasoning").is_none());
        assert!(xai("grok-4.20-0309-non-reasoning").is_none());
        assert!(!xai_reasons("grok-4.20-non-reasoning"));
        assert!(xai("grok-build-0.1").is_none());
        assert!(xai("grok-5").is_some());
        assert!(xai("grok-3").is_none());
    }

    #[test]
    fn openai_table_spots_reasoning_models() {
        assert!(openai("gpt-4o-mini").is_none());
        assert!(openai("gpt-4.1").is_none());
        assert!(openai("gpt-5-chat-latest").is_none());
        assert!(openai("gpt-5-image").is_none());
        let gpt5 = openai("gpt-5-mini").unwrap();
        assert!(gpt5.supported.contains(&"minimal".to_string()));
        assert_eq!(
            pick(Setting::Low, &openai("gpt-5.5").unwrap()).as_deref(),
            Some("low")
        );
        assert_eq!(
            pick(Setting::Low, &openai("gpt-6.1-sol").unwrap()).as_deref(),
            Some("low")
        );
        assert_eq!(
            pick(Setting::Low, &openai("o4-mini").unwrap()).as_deref(),
            Some("low")
        );
        // Pro models reason but take no effort from us.
        let pro = openai("gpt-5.5-pro").unwrap();
        assert_eq!(pick(Setting::Low, &pro), None);
    }

    #[test]
    fn claude_generations_are_told_apart() {
        let sonnet5 = anthropic("claude-sonnet-5");
        assert!(sonnet5.thinks && !sonnet5.sampling);
        assert_eq!(sonnet5.efforts.default.as_deref(), Some("high"));
        assert_eq!(
            anthropic("claude-opus-5-5").efforts.default.as_deref(),
            Some("medium")
        );
        assert_eq!(
            anthropic("claude-haiku-5-5").efforts.default.as_deref(),
            Some("medium")
        );
        assert!(anthropic("claude-fable-5-1").thinks);
        let opus47 = anthropic("claude-opus-4-7");
        assert!(!opus47.thinks && !opus47.sampling);
        let sonnet46 = anthropic("claude-sonnet-4-6");
        assert!(!sonnet46.thinks && sonnet46.sampling);
        // A dated id is not mistaken for a minor version.
        let haiku45 = anthropic("claude-haiku-4-5-20251001");
        assert!(!haiku45.thinks && haiku45.sampling);
        assert!(anthropic("claude-3-5-sonnet-20241022").sampling);
        assert!(anthropic("something-else").sampling);
    }

    #[test]
    fn openrouter_metadata_is_read() {
        let grok = router_model(&json!({
            "mandatory": true, "default_enabled": true,
            "supported_efforts": ["xhigh", "high", "medium", "low"], "default_effort": "high"
        }))
        .unwrap();
        assert!(grok.on_by_default);
        assert_eq!(pick(Setting::Low, &grok.efforts).as_deref(), Some("low"));
        let hermes = router_model(&json!({ "mandatory": false })).unwrap();
        assert!(!hermes.on_by_default);
        assert_eq!(pick(Setting::Low, &hermes.efforts), None);
        let open =
            router_model(&json!({ "default_enabled": true, "supported_efforts": null })).unwrap();
        assert_eq!(pick(Setting::Low, &open.efforts).as_deref(), Some("low"));
        assert!(router_model(&Value::Null).is_none());
    }

    #[test]
    fn codex_effort_comes_from_the_model_list() {
        let list = json!([
            { "model": "gpt-6", "isDefault": true, "defaultReasoningEffort": "medium",
              "supportedReasoningEfforts": [
                { "reasoningEffort": "low" }, { "reasoningEffort": "medium" }, { "reasoningEffort": "high" }
              ] },
            { "model": "gpt-6-pro", "isDefault": false, "defaultReasoningEffort": "high",
              "supportedReasoningEfforts": [{ "reasoningEffort": "high" }] }
        ]);
        assert_eq!(codex(Setting::Low, &list, "").as_deref(), Some("low"));
        assert_eq!(
            codex(Setting::Low, &list, "gpt-6-pro").as_deref(),
            Some("high")
        );
        assert_eq!(codex(Setting::Low, &list, "unknown"), None);
        assert_eq!(codex(Setting::ModelDefault, &list, ""), None);
    }

    #[test]
    fn acp_thought_level_is_found_flat_or_grouped() {
        let flat = json!([
            { "id": "model", "category": "model", "currentValue": "x", "options": [] },
            { "id": "thinking", "category": "thought_level", "currentValue": "HIGH",
              "options": [{ "value": "LOW" }, { "value": "HIGH" }] }
        ]);
        assert_eq!(
            acp_thought_level(Setting::Low, &flat),
            Some(("thinking".into(), "LOW".into()))
        );
        // Already there: nothing to send.
        assert_eq!(acp_thought_level(Setting::High, &flat), None);
        let grouped = json!([
            { "id": "t", "category": "thought_level", "currentValue": "medium",
              "options": [{ "group": "g", "options": [{ "value": "low" }, { "value": "medium" }] }] }
        ]);
        assert_eq!(
            acp_thought_level(Setting::Low, &grouped),
            Some(("t".into(), "low".into()))
        );
        assert_eq!(acp_thought_level(Setting::Low, &json!([])), None);
        assert_eq!(acp_thought_level(Setting::Low, &Value::Null), None);
    }
}
