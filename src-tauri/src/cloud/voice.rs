//! Voice steps of a scene: the ElevenLabs voice library, Voice Design and
//! cached line takes. Shared by the desktop commands and the browser server,
//! so the account rules live in one place.

use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

use super::elevenlabs::{
    DesignResult, ElevenLabsClient, Moderated, SpeechSettings, Subscription, VoiceSummary,
    PREVIEW_TEXT_MAX, PREVIEW_TEXT_MIN, TTS_MODEL, TTS_TEXT_MAX,
};
use super::takes::{self, TakeMeta, TakeStore};
use super::voice_prompt::{self, VoiceBrief};
use super::{resolve_credential, CloudProvider};

#[derive(Debug, Clone, Deserialize)]
pub struct DesignRequest {
    pub brief: VoiceBrief,
    pub preview_text: String,
    pub seed: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesignResponse {
    /// The description actually sent, so the UI can show and save it.
    pub description: String,
    pub result: Moderated<DesignResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TakeRequest {
    pub voice_id: String,
    pub text: String,
    pub settings: SpeechSettings,
    /// 0 for the first take of a line; the user asks for 1, 2, ... explicitly.
    pub take_index: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TakeInfo {
    pub take_id: String,
    pub audio_base64: String,
    pub media_type: &'static str,
    /// Credits charged when this take was rendered, if ElevenLabs said.
    pub credits: Option<u64>,
    /// True when this came from the cache and cost nothing now.
    pub cached: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SpeechEstimate {
    /// Requests that would be sent (cached takes excluded).
    pub requests: usize,
    /// Characters those requests would bill.
    pub characters: usize,
    /// Takes already on disk, which cost nothing.
    pub cached: usize,
}

/// Preview text for Voice Design is billed once for all three previews.
pub fn design_characters(preview_text: &str) -> usize {
    preview_text.trim().chars().count()
}

async fn client_credential(
    state: &Arc<AppState>,
    username: Option<&str>,
) -> Result<super::CloudCredential, AppError> {
    resolve_credential(state, username, CloudProvider::ElevenLabs).await
}

pub async fn subscription(
    state: &Arc<AppState>,
    username: Option<&str>,
) -> Result<Subscription, AppError> {
    let credential = client_credential(state, username).await?;
    ElevenLabsClient::new(&state.http_client, &credential)
        .subscription()
        .await
}

pub async fn list_voices(
    state: &Arc<AppState>,
    username: Option<&str>,
) -> Result<Vec<VoiceSummary>, AppError> {
    let credential = client_credential(state, username).await?;
    ElevenLabsClient::new(&state.http_client, &credential)
        .list_voices()
        .await
}

/// Build the description from the brief and request three previews.
///
/// Validation happens before the key is even resolved, so a bad request never
/// reaches the provider. A refusal comes back as `Moderated::Blocked` and is
/// never retried here.
pub async fn design(
    state: &Arc<AppState>,
    username: Option<&str>,
    request: DesignRequest,
) -> Result<DesignResponse, AppError> {
    let description = voice_prompt::build_description(&request.brief).map_err(AppError::Other)?;
    let preview_text = request.preview_text.trim();
    let len = preview_text.chars().count();
    if !(PREVIEW_TEXT_MIN..=PREVIEW_TEXT_MAX).contains(&len) {
        return Err(AppError::Other(format!(
            "Preview text must be {PREVIEW_TEXT_MIN} to {PREVIEW_TEXT_MAX} characters."
        )));
    }
    let credential = client_credential(state, username).await?;
    let result = ElevenLabsClient::new(&state.http_client, &credential)
        .design_voice(&description, preview_text, request.seed)
        .await?;
    Ok(DesignResponse {
        description,
        result,
    })
}

pub async fn save_voice(
    state: &Arc<AppState>,
    username: Option<&str>,
    name: &str,
    description: &str,
    generated_voice_id: &str,
) -> Result<VoiceSummary, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::Other(
            "Give the voice a name of up to 100 characters.".into(),
        ));
    }
    let credential = client_credential(state, username).await?;
    ElevenLabsClient::new(&state.http_client, &credential)
        .save_voice(name, description.trim(), generated_voice_id.trim())
        .await
}

/// Delete a voice from the ElevenLabs account. The UI asks for explicit
/// confirmation first; this cannot be undone.
pub async fn delete_voice(
    state: &Arc<AppState>,
    username: Option<&str>,
    voice_id: &str,
) -> Result<(), AppError> {
    let credential = client_credential(state, username).await?;
    ElevenLabsClient::new(&state.http_client, &credential)
        .delete_voice(voice_id)
        .await
}

fn validate_take(request: &TakeRequest) -> Result<(), AppError> {
    let len = request.text.trim().chars().count();
    if len == 0 {
        return Err(AppError::Other("A line needs some text.".into()));
    }
    if len > TTS_TEXT_MAX {
        return Err(AppError::Other(format!(
            "A line can be at most {TTS_TEXT_MAX} characters."
        )));
    }
    if request.voice_id.trim().is_empty() {
        return Err(AppError::Other("Pick a voice first.".into()));
    }
    Ok(())
}

fn store_for(username: Option<&str>) -> Result<TakeStore, AppError> {
    TakeStore::for_account(username)
        .ok_or_else(|| AppError::Other("Cannot locate the scene asset folder.".into()))
}

fn take_id_for(request: &TakeRequest) -> String {
    takes::take_id(
        &request.voice_id,
        request.text.trim(),
        &request.settings,
        request.take_index,
    )
}

/// What a batch of takes would cost, counting only takes not already on disk.
/// Pure apart from the cache lookup.
pub fn estimate_takes(
    username: Option<&str>,
    requests: &[TakeRequest],
) -> Result<SpeechEstimate, AppError> {
    let store = store_for(username)?;
    Ok(estimate_with(requests, |id| store.contains(id)))
}

fn estimate_with(requests: &[TakeRequest], cached: impl Fn(&str) -> bool) -> SpeechEstimate {
    let mut estimate = SpeechEstimate::default();
    for request in requests {
        if cached(&take_id_for(request)) {
            estimate.cached += 1;
        } else {
            estimate.requests += 1;
            estimate.characters += request.text.trim().chars().count();
        }
    }
    estimate
}

fn take_info(take_id: String, meta: &TakeMeta, bytes: &[u8], cached: bool) -> TakeInfo {
    TakeInfo {
        take_id,
        audio_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        media_type: "audio/mpeg",
        credits: meta.credits,
        cached,
    }
}

/// Render one take, or return it from the cache without paying again.
pub async fn render_take(
    state: &Arc<AppState>,
    username: Option<&str>,
    request: TakeRequest,
) -> Result<Moderated<TakeInfo>, AppError> {
    validate_take(&request)?;
    let store = store_for(username)?;
    let id = take_id_for(&request);
    if let Some((meta, bytes)) = store.get(&id) {
        return Ok(Moderated::Ok {
            value: take_info(id, &meta, &bytes, true),
        });
    }
    let credential = client_credential(state, username).await?;
    let text = request.text.trim();
    let audio = match ElevenLabsClient::new(&state.http_client, &credential)
        .text_to_speech(&request.voice_id, text, &request.settings)
        .await?
    {
        Moderated::Blocked { message } => return Ok(Moderated::Blocked { message }),
        Moderated::Ok { value } => value,
    };
    let meta = TakeMeta {
        voice_id: request.voice_id.trim().to_string(),
        model: TTS_MODEL.to_string(),
        text: text.to_string(),
        credits: audio.character_cost,
        created_unix: chrono::Utc::now().timestamp(),
    };
    // Degrade, never discard: if the cache write fails the user still gets
    // the paid audio back, and the failure is logged.
    if let Err(e) = store.put(&id, &audio.bytes, &meta) {
        log::warn!("Could not cache a voice take: {e}");
    }
    Ok(Moderated::Ok {
        value: take_info(id, &meta, &audio.bytes, false),
    })
}

/// Load a take the account already has, by id. Never calls ElevenLabs.
pub fn load_take(username: Option<&str>, take_id: &str) -> Result<TakeInfo, AppError> {
    let store = store_for(username)?;
    let (meta, bytes) = store
        .get(take_id)
        .ok_or_else(|| AppError::Other("That take is no longer on disk.".into()))?;
    Ok(take_info(take_id.to_string(), &meta, &bytes, true))
}

/// What measuring mouth timing for a set of takes would cost.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AlignEstimate {
    /// Takes that would be sent to ElevenLabs.
    pub takes: usize,
    /// Their total length.
    pub seconds: f64,
    pub usd: f64,
    /// Takes already measured, which cost nothing.
    pub cached: usize,
    pub checked: &'static str,
}

fn ffmpeg_for(state: &AppState) -> Result<std::path::PathBuf, AppError> {
    state.media_tools.path("ffmpeg").ok_or_else(|| {
        AppError::Other(
            "Scenes need FFmpeg. Set up the music tools on the Music page first.".into(),
        )
    })
}

/// Distinct, valid take ids that are on disk and not measured yet.
fn unmeasured<'a>(store: &TakeStore, take_ids: &'a [String]) -> (Vec<&'a str>, usize) {
    let mut seen = std::collections::HashSet::new();
    let mut todo = Vec::new();
    let mut cached = 0;
    for id in take_ids.iter().map(String::as_str) {
        if !seen.insert(id) || !store.contains(id) {
            continue;
        }
        if store.mouth(id).is_some() {
            cached += 1;
        } else {
            todo.push(id);
        }
    }
    (todo, cached)
}

/// Price of measuring these takes. Reads the takes' lengths; calls no
/// provider.
pub async fn estimate_alignment(
    state: &Arc<AppState>,
    username: Option<&str>,
    take_ids: &[String],
) -> Result<AlignEstimate, AppError> {
    let store = store_for(username)?;
    let (todo, cached) = unmeasured(&store, take_ids);
    let ffmpeg = ffmpeg_for(state)?;
    let mut seconds = 0.0;
    for id in &todo {
        if let Some(path) = store.path_of(id) {
            seconds += super::scene::audio::probe_duration(&ffmpeg, &path)
                .await
                .map_err(AppError::Other)?;
        }
    }
    Ok(AlignEstimate {
        takes: todo.len(),
        seconds,
        usd: super::scene::price::elevenlabs_alignment(seconds),
        cached,
        checked: super::scene::price::ELEVENLABS_ALIGNMENT_CHECKED,
    })
}

/// Measure the mouth timing of each take not measured yet, one request at a
/// time, caching each result as it arrives so a failure part way through
/// loses nothing already paid for. Returns how many takes were measured.
pub async fn align_takes(
    state: &Arc<AppState>,
    username: Option<&str>,
    take_ids: &[String],
) -> Result<usize, AppError> {
    let store = store_for(username)?;
    let (todo, _) = unmeasured(&store, take_ids);
    if todo.is_empty() {
        return Ok(0);
    }
    let credential = client_credential(state, username).await?;
    let client = ElevenLabsClient::new(&state.http_client, &credential);
    let mut measured = 0;
    for id in todo {
        let Some((meta, bytes)) = store.get(id) else {
            continue;
        };
        let text = super::scene::mouth::spoken_only(&meta.text);
        if text.is_empty() {
            continue;
        }
        let units = client.align(bytes, &format!("{id}.mp3"), &text).await?;
        let map = super::scene::mouth::MouthMap {
            version: super::scene::mouth::MOUTH_VERSION,
            spans: super::scene::mouth::spans_from(&units),
        };
        if let Err(e) = store.put_mouth(id, &map) {
            log::warn!("Could not cache a mouth map: {e}");
        }
        measured += 1;
    }
    Ok(measured)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str, take_index: u32) -> TakeRequest {
        TakeRequest {
            voice_id: "voice1".into(),
            text: text.into(),
            settings: SpeechSettings {
                stability: 0.35,
                similarity: 0.75,
                language_code: "ja".into(),
                normalize_text: false,
                seed: None,
            },
            take_index,
        }
    }

    #[test]
    fn the_estimate_skips_cached_takes() {
        let requests = [request("abcde", 0), request("abcde", 1), request("xyz", 0)];
        let cached_id = take_id_for(&requests[0]);
        let estimate = estimate_with(&requests, |id| id == cached_id);
        assert_eq!(
            estimate,
            SpeechEstimate {
                requests: 2,
                characters: 8,
                cached: 1
            }
        );
    }

    #[test]
    fn characters_are_counted_not_bytes() {
        let estimate = estimate_with(&[request(" こんにちは ", 0)], |_| false);
        assert_eq!(estimate.characters, 5);
        assert_eq!(design_characters("  ありがとう "), 5);
    }

    #[test]
    fn invalid_takes_fail_before_any_request() {
        assert!(validate_take(&request("   ", 0)).is_err());
        assert!(validate_take(&request(&"a".repeat(TTS_TEXT_MAX + 1), 0)).is_err());
        let mut r = request("hello", 0);
        r.voice_id = " ".into();
        assert!(validate_take(&r).is_err());
        assert!(validate_take(&request("hello", 0)).is_ok());
    }
}
