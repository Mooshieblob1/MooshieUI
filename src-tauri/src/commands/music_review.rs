//! Opt-in recording review. Only xAI credentials go to the fixed xAI STT endpoint;
//! neither recordings nor provider responses are written to diagnostic logs.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::{config::AppConfig, error::AppError, state::AppState};

const MAX_AUDIO: usize = 64 * 1024 * 1024;
const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
pub struct ReviewWord {
    text: String,
    start: f64,
    end: f64,
}

#[derive(Deserialize, Serialize)]
pub struct MusicTranscript {
    text: String,
    #[serde(default)]
    language: Option<String>,
    duration: f64,
    // xAI omits `words` when it returns no recognized words (observed with
    // sung source audio). Preserve that as inconclusive instead of a parse error.
    #[serde(default)]
    words: Vec<ReviewWord>,
    #[serde(default)]
    audio_sha256: String,
}

fn configured(config: &AppConfig) -> bool {
    config.llm_external_enabled
        && matches!(config.llm_provider.as_str(), "xai" | "xai-oauth")
        && !config.llm_external_api_key.trim().is_empty()
}

pub(crate) async fn capabilities(state: &AppState) -> bool {
    configured(&*state.config.read().await)
}

pub(crate) fn audio_hash(audio_base64: &str) -> Result<String, AppError> {
    let (data, _) = decode_audio(audio_base64)?;
    Ok(format!("{:x}", Sha256::digest(data)))
}

fn decode_audio(encoded: &str) -> Result<(Vec<u8>, &'static str), AppError> {
    if encoded.is_empty() || encoded.len() > MAX_AUDIO.div_ceil(3) * 4 {
        return Err("Review audio must be between 1 byte and 64 MiB".into());
    }
    let data = STANDARD
        .decode(encoded)
        .map_err(|_| AppError::from("Invalid review audio encoding"))?;
    if data.is_empty() || data.len() > MAX_AUDIO {
        return Err("Review audio must be between 1 byte and 64 MiB".into());
    }
    // Derive the upload name from the container, never from a user path.
    let extension = if data.starts_with(b"fLaC") {
        "flac"
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WAVE") {
        "wav"
    } else if data.starts_with(b"OggS") {
        "ogg"
    } else if data.starts_with(b"ID3")
        || (data.len() > 2 && data[0] == 0xff && data[1] & 0xe0 == 0xe0)
    {
        "mp3"
    } else if data.get(4..8) == Some(b"ftyp") {
        "m4a"
    } else if data.starts_with(b"FORM") && matches!(data.get(8..12), Some(b"AIFF" | b"AIFC")) {
        "aiff"
    } else {
        return Err("Use WAV, FLAC, MP3, OGG, M4A or AIFF audio for review".into());
    };
    Ok((data, extension))
}

fn validate_transcript(result: &MusicTranscript) -> Result<(), AppError> {
    if !result.duration.is_finite()
        || result.duration <= 0.0
        || result.duration > 361.0
        || result.text.len() > 65536
        || result.words.len() > 4000
        || result.language.as_ref().is_some_and(|s| s.len() > 64)
    {
        return Err(
            "Review requires a recording of at most six minutes and a bounded transcript".into(),
        );
    }
    let mut previous = 0.0;
    for word in &result.words {
        if word.text.len() > 512
            || word.text.trim().is_empty()
            || !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < previous
            || word.end < word.start
            || word.end > result.duration + 0.25
        {
            return Err("xAI returned invalid word timestamps; no timing review was saved".into());
        }
        previous = word.start;
    }
    Ok(())
}

async fn request_transcript(
    client: &reqwest::Client,
    endpoint: &str,
    key: &str,
    data: Vec<u8>,
    extension: &str,
) -> Result<MusicTranscript, AppError> {
    let hash = format!("{:x}", Sha256::digest(&data));
    // xAI requires file to be the LAST multipart field. No expected lyrics are
    // supplied as recognition hints: that would bias the omission check.
    let form = reqwest::multipart::Form::new()
        .text("format", "false")
        .text("filler_words", "true")
        .part(
            "file",
            reqwest::multipart::Part::bytes(data).file_name(format!("review.{extension}")),
        );
    let mut response = client.post(endpoint).bearer_auth(key).multipart(form)
        .timeout(Duration::from_secs(90)).send().await
        .map_err(|_| AppError::from("xAI transcription could not complete within 90 seconds. Retry when the connection is available."))?;
    let status = response.status();
    if !status.is_success() {
        // Do not expose upstream bodies, which can echo audio or credentials.
        return Err(AppError::ApiError { status: status.as_u16(), message: match status.as_u16() {
            401 | 403 => "xAI did not authorize speech-to-text. Sign in again in Assistant settings; if STT access remains unavailable, configure an xAI API key with speech-to-text access.",
            429 => "xAI speech-to-text is rate limited or has no remaining quota. Check your xAI account before retrying.",
            _ => "xAI could not transcribe this recording. Check the audio format and your speech-to-text access.",
        }.into() });
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AppError::from("xAI transcription response was interrupted"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err("xAI transcript exceeds the review limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    let mut result: MusicTranscript = serde_json::from_slice(&body)
        .map_err(|_| AppError::from("xAI returned an unreadable transcript"))?;
    validate_transcript(&result)?;
    result.audio_sha256 = hash;
    Ok(result)
}

pub(crate) async fn transcribe(
    state: &AppState,
    audio_base64: &str,
) -> Result<MusicTranscript, AppError> {
    let _permit = state.music_review_lock.try_lock().map_err(|_| {
        AppError::from("Another recording is being transcribed. Try again when it finishes.")
    })?;
    if !capabilities(state).await {
        return Err("Configure xAI in Assistant settings to use speech-to-text review".into());
    }
    let (data, extension) = decode_audio(audio_base64)?;
    crate::prompt_assistant::providers::ensure_fresh_token(&state.http_client, &state.config).await;
    let key = {
        let config = state.config.read().await;
        if !configured(&config) {
            return Err("Assistant provider changed before transcription".into());
        }
        config.llm_external_api_key.clone()
    };
    request_transcript(
        &state.http_client_no_redirect,
        "https://api.x.ai/v1/stt",
        &key,
        data,
        extension,
    )
    .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_music_review_capabilities(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<bool, AppError> {
    Ok(capabilities(&state).await)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub fn hash_music_review_audio(audio_base64: String) -> Result<String, AppError> {
    audio_hash(&audio_base64)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn transcribe_music_review(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    audio_base64: String,
) -> Result<MusicTranscript, AppError> {
    transcribe(&state, &audio_base64).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_container_and_provider_boundaries() {
        assert_eq!(
            decode_audio(&STANDARD.encode(b"RIFF1234WAVEpayload"))
                .unwrap()
                .1,
            "wav"
        );
        assert_eq!(
            decode_audio(&STANDARD.encode(b"fLaCpayload")).unwrap().1,
            "flac"
        );
        assert!(decode_audio(&STANDARD.encode(b"not audio")).is_err());
        assert!(decode_audio("data:audio/wav;base64,aaaa").is_err());
        let mut config = AppConfig::default();
        config.llm_external_enabled = true;
        config.llm_external_api_key = "test-only".into();
        config.llm_provider = "openai".into();
        assert!(!configured(&config));
        config.llm_provider = "xai-oauth".into();
        assert!(configured(&config));
        config.llm_external_enabled = false;
        assert!(!configured(&config));
    }

    #[test]
    fn timestamps_are_bounded_and_ordered_without_invented_confidence() {
        let silent: MusicTranscript =
            serde_json::from_str(r#"{"text":"","language":"","duration":256.022}"#).unwrap();
        assert!(silent.words.is_empty());
        assert!(validate_transcript(&silent).is_ok());
        let mut transcript: MusicTranscript = serde_json::from_str(
            r#"{"text":"hello","duration":2.5,"words":[{"text":"hello","start":0.2,"end":1.1}]}"#,
        )
        .unwrap();
        assert!(validate_transcript(&transcript).is_ok());
        transcript.words[0].start = -0.1;
        assert!(validate_transcript(&transcript).is_err());
        transcript.words[0].start = 0.2;
        transcript.words[0].end = 3.0;
        assert!(validate_transcript(&transcript).is_err());
        transcript.words.clear();
        assert!(
            validate_transcript(&transcript).is_ok(),
            "An empty transcript is an inconclusive review, not a fabricated match"
        );
        transcript.duration = f64::NAN;
        assert!(validate_transcript(&transcript).is_err());
    }

    #[tokio::test]
    async fn sends_real_multipart_file_last_and_preserves_word_timestamps() {
        use axum::{body::Bytes, http::HeaderMap, routing::post, Router};
        let app = Router::new().route("/stt", post(|headers: HeaderMap, body: Bytes| async move {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer synthetic");
            let body = String::from_utf8_lossy(&body);
            assert!(body.find("name=\"format\"").unwrap() < body.find("name=\"file\"").unwrap());
            assert!(body.find("name=\"filler_words\"").unwrap() < body.find("name=\"file\"").unwrap());
            assert!(body.contains("filename=\"review.wav\""));
            assert!(body.contains("RIFF1234WAVEsynthetic"));
            axum::Json(serde_json::json!({"text":"hello", "language":"en", "duration":2.0, "words":[{"text":"hello","start":0.25,"end":1.0}]}))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/stt", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = request_transcript(
            &reqwest::Client::new(),
            &endpoint,
            "synthetic",
            b"RIFF1234WAVEsynthetic".to_vec(),
            "wav",
        )
        .await
        .unwrap();
        assert_eq!(result.words[0].start, 0.25);
        assert_eq!(result.audio_sha256.len(), 64);
        task.abort();
    }

    #[tokio::test]
    async fn provider_errors_do_not_expose_upstream_bodies() {
        use axum::{http::StatusCode, routing::post, Router};
        let app = Router::new().route(
            "/stt",
            post(|| async {
                (
                    StatusCode::FORBIDDEN,
                    "private transcript and credential echoed upstream",
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/stt", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let error = request_transcript(
            &reqwest::Client::new(),
            &endpoint,
            "synthetic",
            b"RIFF1234WAVEsynthetic".to_vec(),
            "wav",
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("403") && error.contains("Sign in again"));
        assert!(!error.contains("private transcript") && !error.contains("credential echoed"));
        task.abort();
    }
}
