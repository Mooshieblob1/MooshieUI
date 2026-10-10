//! HTTP access to ElevenLabs: voice library, Voice Design and text to speech.
//!
//! Borrows the shared `reqwest::Client` from `AppState` and sets a deadline on
//! every call, because the shared client has none. The client deliberately has
//! no `Debug`: it holds the API key. Endpoint shapes were read from
//! ElevenLabs' API reference on 2026-10-11.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;

use super::CloudCredential;

const API_BASE: &str = "https://api.elevenlabs.io";

/// Every new provider client sends an explicit User-Agent (research doc,
/// section 3 lesson 7).
pub const USER_AGENT: &str = concat!("MooshieUI/", env!("CARGO_PKG_VERSION"));

/// Voice Design model. v3 is the one that follows detailed prompts.
pub const DESIGN_MODEL: &str = "eleven_ttv_v3";
/// Speech model. The prototype's listener strongly preferred v4 over v3.
pub const TTS_MODEL: &str = "eleven_v4";
/// MP3 at 44.1 kHz, which Seedance accepts as an audio reference as is.
const OUTPUT_FORMAT: &str = "mp3_44100_128";

const ACCOUNT_TIMEOUT: Duration = Duration::from_secs(30);
const GENERATION_TIMEOUT: Duration = Duration::from_secs(180);

/// Largest body read from any ElevenLabs response. Three Voice Design previews
/// arrive base64-encoded in one JSON document.
const MAX_JSON_BYTES: usize = 32 * 1024 * 1024;
const MAX_AUDIO_BYTES: usize = 32 * 1024 * 1024;

/// Voice Design limits from the API reference.
pub const DESCRIPTION_MIN: usize = 20;
pub const DESCRIPTION_MAX: usize = 1000;
pub const PREVIEW_TEXT_MIN: usize = 100;
pub const PREVIEW_TEXT_MAX: usize = 1000;
/// Eleven v4's per-request limit (best practices page, rule E5).
pub const TTS_TEXT_MAX: usize = 10_000;

pub struct ElevenLabsClient<'a> {
    http: &'a reqwest::Client,
    credential: &'a CloudCredential,
}

/// What the subscription endpoint says about the account, trimmed to what the
/// UI shows. Every field is optional so a renamed field degrades to "unknown"
/// rather than failing the whole readout.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Subscription {
    pub tier: Option<String>,
    pub character_count: Option<u64>,
    pub character_limit: Option<u64>,
    pub voice_slots_used: Option<u64>,
    pub voice_limit: Option<u64>,
    pub next_character_count_reset_unix: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceSummary {
    pub voice_id: String,
    pub name: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub preview_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoicePreview {
    pub generated_voice_id: String,
    /// Base64 audio, as ElevenLabs returns it.
    pub audio_base_64: String,
    pub media_type: Option<String>,
    pub duration_secs: Option<f64>,
}

/// Result of a call that ElevenLabs may refuse under its safety rules.
///
/// A refusal is a normal outcome, not an error, so the UI can show it as
/// final. Nothing in MooshieUI retries a blocked request with a reworded one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Moderated<T> {
    Ok { value: T },
    Blocked { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignResult {
    pub previews: Vec<VoicePreview>,
    pub text: Option<String>,
}

/// One rendered line.
pub struct SpeechAudio {
    pub bytes: Vec<u8>,
    /// Credits ElevenLabs charged, from its `character-cost` header.
    pub character_cost: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpeechSettings {
    pub stability: f32,
    pub similarity: f32,
    /// ISO 639-1, for example "ja". Empty lets the model decide.
    pub language_code: String,
    /// Better Japanese pronunciation at a large latency cost (rule E4).
    pub normalize_text: bool,
    pub seed: Option<u32>,
}

impl<'a> ElevenLabsClient<'a> {
    pub fn new(http: &'a reqwest::Client, credential: &'a CloudCredential) -> Self {
        Self { http, credential }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{API_BASE}{path}"))
            .header("xi-api-key", self.credential.as_str())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
    }

    pub async fn subscription(&self) -> Result<Subscription, AppError> {
        let res = self
            .request(reqwest::Method::GET, "/v1/user/subscription")
            .timeout(ACCOUNT_TIMEOUT)
            .send()
            .await?;
        let value = read_json(check_status(res).await?).await?;
        Ok(parse_subscription(&value))
    }

    pub async fn list_voices(&self) -> Result<Vec<VoiceSummary>, AppError> {
        let res = self
            .request(reqwest::Method::GET, "/v2/voices?page_size=100")
            .timeout(ACCOUNT_TIMEOUT)
            .send()
            .await?;
        let value = read_json(check_status(res).await?).await?;
        Ok(parse_voices(&value))
    }

    pub async fn design_voice(
        &self,
        description: &str,
        preview_text: &str,
        seed: Option<u32>,
    ) -> Result<Moderated<DesignResult>, AppError> {
        let mut body = serde_json::json!({
            "voice_description": description,
            "model_id": DESIGN_MODEL,
            "text": preview_text,
        });
        if let Some(seed) = seed {
            body["seed"] = serde_json::json!(seed);
        }
        let res = self
            .request(
                reqwest::Method::POST,
                &format!("/v1/text-to-voice/design?output_format={OUTPUT_FORMAT}"),
            )
            .timeout(GENERATION_TIMEOUT)
            .json(&body)
            .send()
            .await?;
        match moderated_status(res).await? {
            Moderated::Blocked { message } => Ok(Moderated::Blocked { message }),
            Moderated::Ok { value: res } => {
                let value = read_json(res).await?;
                Ok(Moderated::Ok {
                    value: parse_design(&value)?,
                })
            }
        }
    }

    /// Save a designed preview as a voice on the account. Uses a voice slot.
    pub async fn save_voice(
        &self,
        name: &str,
        description: &str,
        generated_voice_id: &str,
    ) -> Result<VoiceSummary, AppError> {
        let body = serde_json::json!({
            "voice_name": name,
            "voice_description": description,
            "generated_voice_id": generated_voice_id,
        });
        let res = self
            .request(reqwest::Method::POST, "/v1/text-to-voice")
            .timeout(ACCOUNT_TIMEOUT)
            .json(&body)
            .send()
            .await?;
        let value = read_json(check_status(res).await?).await?;
        parse_voice(&value).ok_or_else(|| {
            AppError::Other("ElevenLabs saved the voice but returned no voice id.".into())
        })
    }

    pub async fn delete_voice(&self, voice_id: &str) -> Result<(), AppError> {
        let res = self
            .request(
                reqwest::Method::DELETE,
                &format!("/v1/voices/{}", path_segment(voice_id)?),
            )
            .timeout(ACCOUNT_TIMEOUT)
            .send()
            .await?;
        check_status(res).await?;
        Ok(())
    }

    pub async fn text_to_speech(
        &self,
        voice_id: &str,
        text: &str,
        settings: &SpeechSettings,
    ) -> Result<Moderated<SpeechAudio>, AppError> {
        let body = speech_body(text, settings);
        let res = self
            .request(
                reqwest::Method::POST,
                &format!(
                    "/v1/text-to-speech/{}?output_format={OUTPUT_FORMAT}",
                    path_segment(voice_id)?
                ),
            )
            .timeout(GENERATION_TIMEOUT)
            .json(&body)
            .send()
            .await?;
        match moderated_status(res).await? {
            Moderated::Blocked { message } => Ok(Moderated::Blocked { message }),
            Moderated::Ok { value: res } => {
                let character_cost = res
                    .headers()
                    .get("character-cost")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok());
                let bytes = read_bounded(res, MAX_AUDIO_BYTES).await?;
                if bytes.is_empty() {
                    return Err(AppError::Other("ElevenLabs returned no audio.".into()));
                }
                Ok(Moderated::Ok {
                    value: SpeechAudio {
                        bytes,
                        character_cost,
                    },
                })
            }
        }
    }
}

fn speech_body(text: &str, settings: &SpeechSettings) -> Value {
    let mut body = serde_json::json!({
        "text": text,
        "model_id": TTS_MODEL,
        // v4 takes stability and similarity only (rule E3).
        "voice_settings": {
            "stability": settings.stability.clamp(0.0, 1.0),
            "similarity_boost": settings.similarity.clamp(0.0, 1.0),
        },
    });
    let language = settings.language_code.trim();
    if !language.is_empty() {
        body["language_code"] = serde_json::json!(language);
    }
    if settings.normalize_text {
        body["apply_language_text_normalization"] = serde_json::json!(true);
    }
    if let Some(seed) = settings.seed {
        body["seed"] = serde_json::json!(seed);
    }
    body
}

/// A voice id goes into a URL path, so accept only the characters ElevenLabs
/// ids are made of.
fn path_segment(id: &str) -> Result<&str, AppError> {
    let id = id.trim();
    if !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
        Ok(id)
    } else {
        Err(AppError::Other("Invalid ElevenLabs voice id.".into()))
    }
}

async fn read_bounded(mut res: reqwest::Response, limit: usize) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = res.chunk().await? {
        if bytes.len() + chunk.len() > limit {
            return Err(AppError::Other(
                "ElevenLabs response exceeded the size limit.".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn read_json(res: reqwest::Response) -> Result<Value, AppError> {
    let bytes = read_bounded(res, MAX_JSON_BYTES).await?;
    serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Other("ElevenLabs returned invalid JSON.".into()))
}

/// Like [`check_status`], but a safety refusal comes back as
/// [`Moderated::Blocked`] instead of an error.
async fn moderated_status(
    res: reqwest::Response,
) -> Result<Moderated<reqwest::Response>, AppError> {
    let status = res.status();
    if status.is_success() {
        return Ok(Moderated::Ok { value: res });
    }
    let body = read_bounded(res, 64 * 1024).await.unwrap_or_default();
    let body = String::from_utf8_lossy(&body);
    match classify_error(status.as_u16(), &body) {
        ApiFailure::Blocked(message) => Ok(Moderated::Blocked { message }),
        ApiFailure::Error(err) => Err(err),
    }
}

async fn check_status(res: reqwest::Response) -> Result<reqwest::Response, AppError> {
    match moderated_status(res).await? {
        Moderated::Ok { value } => Ok(value),
        // A refusal on an endpoint that does not generate is still a refusal.
        Moderated::Blocked { message } => Err(AppError::Other(message)),
    }
}

enum ApiFailure {
    Blocked(String),
    Error(AppError),
}

/// Turn an ElevenLabs error body into something the user can act on.
///
/// Errors arrive as `{"detail": {"status": "...", "message": "..."}}`, or as
/// a validation list `{"detail": [{"msg": "..."}]}` on 422.
fn classify_error(status: u16, body: &str) -> ApiFailure {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let detail = parsed.as_ref().and_then(|v| v.get("detail"));
    let code = detail
        .and_then(|d| d.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let detail_message = match detail {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.get("msg").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("; "),
        Some(d) => d
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        None => body.chars().take(200).collect(),
    };

    if code == "blocked_generation" {
        let message = if detail_message.is_empty() {
            "ElevenLabs declined this request under its safety rules.".to_string()
        } else {
            format!("ElevenLabs declined this request under its safety rules: {detail_message}")
        };
        return ApiFailure::Blocked(message);
    }

    let message = match (status, code) {
        (401, _) | (_, "invalid_api_key") => {
            "ElevenLabs rejected the API key. Check it in Settings.".to_string()
        }
        (_, "quota_exceeded") => {
            "Not enough ElevenLabs credits for this request.".to_string()
        }
        (_, "voice_limit_reached") => {
            "All your ElevenLabs voice slots are in use. Delete a voice you no longer need, or pick an existing one.".to_string()
        }
        (429, _) => "ElevenLabs is rate limiting this account. Try again shortly.".to_string(),
        _ if detail_message.is_empty() => format!("ElevenLabs request failed (HTTP {status})."),
        _ => format!("ElevenLabs: {detail_message}"),
    };
    ApiFailure::Error(AppError::ApiError { status, message })
}

fn parse_subscription(value: &Value) -> Subscription {
    Subscription {
        tier: value
            .get("tier")
            .and_then(Value::as_str)
            .map(str::to_string),
        character_count: value.get("character_count").and_then(Value::as_u64),
        character_limit: value.get("character_limit").and_then(Value::as_u64),
        voice_slots_used: value.get("voice_slots_used").and_then(Value::as_u64),
        voice_limit: value.get("voice_limit").and_then(Value::as_u64),
        next_character_count_reset_unix: value
            .get("next_character_count_reset_unix")
            .and_then(Value::as_i64),
    }
}

fn parse_voice(value: &Value) -> Option<VoiceSummary> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(VoiceSummary {
        voice_id: text("voice_id")?,
        name: text("name").unwrap_or_default(),
        category: text("category"),
        description: text("description"),
        preview_url: text("preview_url"),
    })
}

fn parse_voices(value: &Value) -> Vec<VoiceSummary> {
    value
        .get("voices")
        .and_then(Value::as_array)
        .map(|voices| voices.iter().filter_map(parse_voice).collect())
        .unwrap_or_default()
}

fn parse_design(value: &Value) -> Result<DesignResult, AppError> {
    let previews: Vec<VoicePreview> = value
        .get("previews")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|p| {
                    Some(VoicePreview {
                        generated_voice_id: p.get("generated_voice_id")?.as_str()?.to_string(),
                        audio_base_64: p.get("audio_base_64")?.as_str()?.to_string(),
                        media_type: p
                            .get("media_type")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        duration_secs: p.get("duration_secs").and_then(Value::as_f64),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if previews.is_empty() {
        return Err(AppError::Other(
            "ElevenLabs returned no voice previews.".into(),
        ));
    }
    Ok(DesignResult {
        previews,
        text: value
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blocked_generation_is_a_refusal_not_an_error() {
        let body = r#"{"detail":{"status":"blocked_generation","message":"This description is not allowed."}}"#;
        match classify_error(403, body) {
            ApiFailure::Blocked(message) => {
                assert!(message.contains("safety rules"));
                assert!(message.contains("not allowed"));
            }
            ApiFailure::Error(_) => panic!("a refusal must not be an ordinary error"),
        }
    }

    #[test]
    fn errors_say_what_to_do() {
        let cases = [
            (401, "{}", "rejected the API key"),
            (
                400,
                r#"{"detail":{"status":"quota_exceeded","message":"x"}}"#,
                "credits",
            ),
            (
                400,
                r#"{"detail":{"status":"voice_limit_reached","message":"x"}}"#,
                "voice slots",
            ),
            (429, "{}", "rate limiting"),
            (
                422,
                r#"{"detail":[{"loc":["body","text"],"msg":"too short","type":"value_error"}]}"#,
                "too short",
            ),
        ];
        for (status, body, needle) in cases {
            match classify_error(status, body) {
                ApiFailure::Error(err) => {
                    let text = err.to_string();
                    assert!(text.contains(needle), "{status}: {text}");
                }
                ApiFailure::Blocked(_) => panic!("{status} is not a refusal"),
            }
        }
    }

    #[test]
    fn subscription_fields_are_optional() {
        let sub = parse_subscription(&serde_json::json!({
            "tier": "creator",
            "character_count": 1200,
            "character_limit": 100000,
            "voice_slots_used": 3,
            "voice_limit": 30,
        }));
        assert_eq!(sub.character_limit, Some(100_000));
        assert_eq!(sub.voice_slots_used, Some(3));
        assert_eq!(
            parse_subscription(&serde_json::json!({})),
            Subscription::default()
        );
    }

    #[test]
    fn voices_without_an_id_are_skipped() {
        let voices = parse_voices(&serde_json::json!({
            "voices": [
                {"voice_id": "abc123", "name": "Aoi", "category": "generated"},
                {"name": "no id"},
            ]
        }));
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].voice_id, "abc123");
        assert_eq!(voices[0].category.as_deref(), Some("generated"));
    }

    #[test]
    fn design_needs_at_least_one_preview() {
        assert!(parse_design(&serde_json::json!({"previews": []})).is_err());
        let result = parse_design(&serde_json::json!({
            "previews": [{"generated_voice_id": "g1", "audio_base_64": "AAAA", "duration_secs": 4.2}],
            "text": "hello",
        }))
        .unwrap();
        assert_eq!(result.previews[0].generated_voice_id, "g1");
    }

    #[test]
    fn voice_ids_cannot_escape_the_path() {
        assert!(path_segment("abcDEF123").is_ok());
        for bad in ["", "../x", "a/b", "a?b", "a b"] {
            assert!(path_segment(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_speech_body_follows_v4_rules() {
        let body = speech_body(
            "こんにちは",
            &SpeechSettings {
                stability: 1.5,
                similarity: 0.75,
                language_code: "ja".into(),
                normalize_text: true,
                seed: Some(7),
            },
        );
        assert_eq!(body["model_id"], TTS_MODEL);
        assert_eq!(body["voice_settings"]["stability"], 1.0);
        assert!(body["voice_settings"].get("style").is_none());
        assert_eq!(body["language_code"], "ja");
        assert_eq!(body["apply_language_text_normalization"], true);
        assert_eq!(body["seed"], 7);

        let plain = speech_body(
            "hi",
            &SpeechSettings {
                stability: 0.35,
                similarity: 0.75,
                language_code: String::new(),
                normalize_text: false,
                seed: None,
            },
        );
        assert!(plain.get("language_code").is_none());
        assert!(plain.get("apply_language_text_normalization").is_none());
    }
}
