//! HTTP access to NovelAI.
//!
//! The client borrows the shared `reqwest::Client` from `AppState` rather than
//! building its own, and never logs the bearer token.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::AppError;

use super::response::{self, StreamDecoder, StreamEvent, Subscription};

// Every endpoint MooshieUI calls lives on the image host, the subscription
// record included. api.novelai.net answers a subscription request with
// 400 "Please refresh NovelAI.net. If using a third-party tool, update to
// the image URL."
const IMAGE_BASE: &str = "https://image.novelai.net";

pub struct NovelAiClient<'a> {
    http: &'a reqwest::Client,
    api_key: &'a str,
}

impl<'a> NovelAiClient<'a> {
    pub fn new(http: &'a reqwest::Client, api_key: &'a str) -> Result<Self, AppError> {
        if api_key.trim().is_empty() {
            return Err(AppError::Other(
                "No NovelAI API key configured. Add one in Settings.".into(),
            ));
        }
        Ok(Self { http, api_key })
    }

    /// Generate images and return the decoded PNG bytes, one entry per sample.
    pub async fn generate(&self, body: &Value) -> Result<Vec<Vec<u8>>, AppError> {
        let res = self
            .http
            .post(format!("{IMAGE_BASE}/ai/generate-image"))
            .bearer_auth(self.api_key)
            .json(body)
            .send()
            .await?;

        let res = check_status(res).await?;
        let bytes = res.bytes().await?;
        response::unpack_images(&bytes).map_err(AppError::Other)
    }

    /// Generate with previews, invoking `on_event` for each decoded SSE frame.
    ///
    /// The callback is synchronous so callers can broadcast without holding a
    /// lock across an await.
    pub async fn generate_stream<F>(
        &self,
        body: &Value,
        mut on_event: F,
    ) -> Result<Vec<Vec<u8>>, AppError>
    where
        F: FnMut(StreamEvent),
    {
        // `stream` is set here rather than in `payload::build_request` on
        // purpose: the batch endpoint must not receive it.
        let mut streaming_body = body.clone();
        if let Some(parameters) = streaming_body
            .get_mut("parameters")
            .and_then(Value::as_object_mut)
        {
            parameters.insert("stream".into(), Value::String("msgpack".into()));
        }

        let res = self
            .http
            .post(format!("{IMAGE_BASE}/ai/generate-image-stream"))
            .bearer_auth(self.api_key)
            .json(&streaming_body)
            .send()
            .await?;

        let mut res = check_status(res).await?;
        let mut decoder = StreamDecoder::new();
        // Keyed by sample index so an interleaved batch comes back in order.
        let mut finals: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
        let mut previews: BTreeMap<u32, Vec<u8>> = BTreeMap::new();

        while let Some(chunk) = res.chunk().await? {
            for event in decoder.push(&chunk) {
                match &event {
                    StreamEvent::Final { image, sample } => {
                        finals.insert(*sample, image.clone());
                    }
                    StreamEvent::Intermediate { image, sample, .. } => {
                        previews.insert(*sample, image.clone());
                    }
                    StreamEvent::Error { message } => {
                        return Err(AppError::ApiError {
                            status: 500,
                            message: message.clone(),
                        });
                    }
                }
                on_event(event);
            }
        }

        // A stream that ends after previews but before its final frames still
        // billed the user, so those previews are delivered rather than lost.
        for (sample, image) in previews {
            finals.entry(sample).or_insert(image);
        }
        if finals.is_empty() {
            return Err(AppError::Other(
                "NovelAI closed the stream without returning an image".into(),
            ));
        }
        Ok(finals.into_values().collect())
    }

    /// Fetch the subscription record backing the Anlas and Opus readouts.
    pub async fn subscription(&self) -> Result<Subscription, AppError> {
        let res = self
            .http
            .get(format!("{IMAGE_BASE}/user/subscription"))
            .bearer_auth(self.api_key)
            .send()
            .await?;

        let res = check_status(res).await?;
        let sub: Subscription = res.json().await?;
        Ok(sub)
    }
}

/// Turn a non-2xx response into an `AppError` carrying NovelAI's own message.
///
/// NovelAI's status codes map onto user-fixable problems, so each gets wording
/// that says what to do rather than repeating the number.
async fn check_status(res: reqwest::Response) -> Result<reqwest::Response, AppError> {
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }

    let body = res.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("error"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(200).collect());

    let message = match status.as_u16() {
        401 => "NovelAI rejected the API key. Check it in Settings.".to_string(),
        402 => "Not enough Anlas for this generation.".to_string(),
        429 => "NovelAI is rate limiting this account. Try again shortly.".to_string(),
        _ if detail.is_empty() => "NovelAI request failed.".to_string(),
        _ => detail,
    };

    Err(AppError::ApiError {
        status: status.as_u16(),
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_key_fails_before_any_request() {
        let http = reqwest::Client::new();
        // Deliberately no `unwrap_err`: that needs `Debug` on the client, and
        // the client holds the API key, so a `Debug` impl is one `{:?}` away
        // from leaking the secret into a log line.
        let Err(err) = NovelAiClient::new(&http, "   ") else {
            panic!("a blank key must be rejected");
        };
        assert!(err.to_string().contains("No NovelAI API key"));
    }
}
