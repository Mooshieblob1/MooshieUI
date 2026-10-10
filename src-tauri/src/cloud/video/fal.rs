//! fal.ai queue API and the Seedance 2.5 reference-to-video request.
//!
//! Submit returns a request id and the URLs to poll, fetch and cancel it;
//! video jobs take minutes, so nothing holds one request open. Shapes were
//! read from fal's queue docs, error docs and the Seedance 2.5 model page on
//! 2026-10-11. The client deliberately has no `Debug`: it holds the API key.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::cloud::elevenlabs::{Moderated, USER_AGENT};
use crate::cloud::CloudCredential;
use crate::error::AppError;

const QUEUE_BASE: &str = "https://queue.fal.run";
pub const SEEDANCE_25_REFERENCE: &str = "bytedance/seedance-2.5/reference-to-video";

const SUBMIT_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
/// A 30 s 1080p clip is tens of megabytes; this only stops a runaway body.
pub const MAX_VIDEO_BYTES: u64 = 1024 * 1024 * 1024;

pub struct FalClient<'a> {
    http: &'a reqwest::Client,
    credential: &'a CloudCredential,
}

/// Where a submitted job lives. Persisted with the scene job so a restart can
/// keep polling instead of paying again.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueueHandle {
    pub request_id: String,
    pub status_url: String,
    pub response_url: String,
    pub cancel_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueueStatus {
    Queued {
        position: Option<u64>,
    },
    Running,
    Completed,
    /// fal reported the request as failed. `blocked` marks a content-policy
    /// refusal, which is final.
    Failed {
        message: String,
        blocked: bool,
    },
}

/// What a finished Seedance request returned.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoResult {
    pub url: String,
    pub seed: Option<i64>,
    pub draft_id: Option<String>,
}

impl<'a> FalClient<'a> {
    pub fn new(http: &'a reqwest::Client, credential: &'a CloudCredential) -> Self {
        Self { http, credential }
    }

    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Key {}", self.credential.as_str()),
            )
            .header(reqwest::header::USER_AGENT, USER_AGENT)
    }

    /// Submit a job. One retry after a pause on 429, as with the face
    /// detailer; a refusal comes back as `Moderated::Blocked` and is final.
    pub async fn submit(
        &self,
        endpoint: &str,
        body: &Value,
    ) -> Result<Moderated<QueueHandle>, AppError> {
        let url = format!("{QUEUE_BASE}/{endpoint}");
        let mut attempt = 0;
        let res = loop {
            let res = self
                .request(reqwest::Method::POST, &url)
                .timeout(SUBMIT_TIMEOUT)
                .json(body)
                .send()
                .await?;
            if res.status().as_u16() == 429 && attempt == 0 {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            break res;
        };
        let res = match moderated_status(res).await? {
            Moderated::Blocked { message } => return Ok(Moderated::Blocked { message }),
            Moderated::Ok { value } => value,
        };
        let value = read_json(res).await?;
        Ok(Moderated::Ok {
            value: parse_handle(&value)?,
        })
    }

    pub async fn status(&self, handle: &QueueHandle) -> Result<QueueStatus, AppError> {
        let url = queue_url(&handle.status_url)?;
        let res = self
            .request(reqwest::Method::GET, url)
            .timeout(POLL_TIMEOUT)
            .send()
            .await?;
        let value = read_json(check_status(res).await?).await?;
        Ok(parse_status(&value))
    }

    pub async fn result(&self, handle: &QueueHandle) -> Result<Moderated<VideoResult>, AppError> {
        let url = queue_url(&handle.response_url)?;
        let res = self
            .request(reqwest::Method::GET, url)
            .timeout(POLL_TIMEOUT)
            .send()
            .await?;
        let res = match moderated_status(res).await? {
            Moderated::Blocked { message } => return Ok(Moderated::Blocked { message }),
            Moderated::Ok { value } => value,
        };
        let value = read_json(res).await?;
        Ok(Moderated::Ok {
            value: parse_video_result(&value)?,
        })
    }

    /// Ask fal to drop the job. Only a job still in the queue is guaranteed to
    /// stop; one already running usually finishes and bills anyway.
    pub async fn cancel(&self, handle: &QueueHandle) {
        let Ok(url) = queue_url(&handle.cancel_url) else {
            return;
        };
        if let Err(e) = self
            .request(reqwest::Method::PUT, url)
            .timeout(POLL_TIMEOUT)
            .send()
            .await
        {
            log::warn!("[scene] fal cancel request failed: {e}");
        }
    }

    /// Stream the finished video to `dest`. The media URL is a signed CDN
    /// link, so the API key is never sent with it.
    pub async fn download(&self, url: &str, dest: &Path) -> Result<u64, AppError> {
        if !url.starts_with("https://") {
            return Err(AppError::Other(
                "fal.ai returned an unexpected video link.".into(),
            ));
        }
        let mut res = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(AppError::Other(format!(
                "Downloading the video from fal.ai failed (HTTP {}).",
                res.status().as_u16()
            )));
        }
        let mut file = tokio::fs::File::create(dest).await?;
        let mut written: u64 = 0;
        while let Some(chunk) = res.chunk().await? {
            written += chunk.len() as u64;
            if written > MAX_VIDEO_BYTES {
                return Err(AppError::Other(
                    "The video from fal.ai is too large.".into(),
                ));
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(written)
    }
}

/// Only fal's own queue host may receive the key. The URLs come from fal's
/// submit response, but they are also read back from a job file on disk.
fn queue_url(url: &str) -> Result<&str, AppError> {
    if url.starts_with(&format!("{QUEUE_BASE}/")) && !url.contains('@') {
        Ok(url)
    } else {
        Err(AppError::Other(
            "The saved fal.ai job has an unexpected address.".into(),
        ))
    }
}

/// The Seedance 2.5 reference-to-video body. Files go inline as data URIs,
/// which fal accepts in place of URLs, so nothing is stored at fal beyond the
/// job itself.
#[allow(clippy::too_many_arguments)]
pub fn seedance_25_body(
    prompt: &str,
    image_data_uris: &[String],
    audio_data_uri: Option<&str>,
    resolution: &str,
    draft: bool,
    seconds: u32,
    aspect_ratio: &str,
    seed: Option<u32>,
) -> Value {
    let mut body = serde_json::json!({
        "prompt": prompt,
        "task": "reference",
        "image_urls": image_data_uris,
        "resolution": resolution,
        "draft": draft,
        // An enum of strings: "auto" or "4" to "30".
        "duration": seconds.to_string(),
        "aspect_ratio": aspect_ratio,
        "generate_audio": true,
        // H.265 does not play in every webview the gallery runs in.
        "codec": "H264",
    });
    if let Some(audio) = audio_data_uri {
        body["audio_urls"] = serde_json::json!([audio]);
    }
    if let Some(seed) = seed {
        body["seed"] = serde_json::json!(seed);
    }
    body
}

fn parse_handle(value: &Value) -> Result<QueueHandle, AppError> {
    let field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| AppError::Other(format!("fal.ai did not return a {name}.")))
    };
    let handle = QueueHandle {
        request_id: field("request_id")?,
        status_url: field("status_url")?,
        response_url: field("response_url")?,
        cancel_url: field("cancel_url")?,
    };
    queue_url(&handle.status_url)?;
    queue_url(&handle.response_url)?;
    Ok(handle)
}

fn parse_status(value: &Value) -> QueueStatus {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match status {
        "IN_QUEUE" => QueueStatus::Queued {
            position: value.get("queue_position").and_then(Value::as_u64),
        },
        "COMPLETED" => {
            let error = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let error_type = value
                .get("error_type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if error.is_empty() && error_type.is_empty() {
                QueueStatus::Completed
            } else {
                failure(error_type, error)
            }
        }
        // IN_PROGRESS, and anything new, keeps polling.
        _ => QueueStatus::Running,
    }
}

fn failure(error_type: &str, message: &str) -> QueueStatus {
    let blocked = error_type == "content_policy_violation";
    let message = if blocked {
        blocked_message(message)
    } else if message.is_empty() {
        format!("fal.ai could not make this video ({error_type}).")
    } else {
        format!("fal.ai: {message}")
    };
    QueueStatus::Failed { message, blocked }
}

fn blocked_message(detail: &str) -> String {
    if detail.trim().is_empty() {
        "fal.ai declined this scene under its content policy.".to_string()
    } else {
        format!(
            "fal.ai declined this scene under its content policy: {}",
            detail.trim()
        )
    }
}

fn parse_video_result(value: &Value) -> Result<VideoResult, AppError> {
    let url = value
        .pointer("/video/url")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Other("fal.ai finished without a video.".into()))?;
    Ok(VideoResult {
        url: url.to_string(),
        seed: value.get("seed").and_then(Value::as_i64),
        draft_id: value
            .get("draft_id")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

async fn read_bounded(mut res: reqwest::Response, limit: usize) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = res.chunk().await? {
        if bytes.len() + chunk.len() > limit {
            return Err(AppError::Other(
                "fal.ai response exceeded the size limit.".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn read_json(res: reqwest::Response) -> Result<Value, AppError> {
    let bytes = read_bounded(res, MAX_JSON_BYTES).await?;
    serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Other("fal.ai returned invalid JSON.".into()))
}

async fn moderated_status(
    res: reqwest::Response,
) -> Result<Moderated<reqwest::Response>, AppError> {
    let status = res.status();
    if status.is_success() {
        return Ok(Moderated::Ok { value: res });
    }
    let body = read_bounded(res, 64 * 1024).await.unwrap_or_default();
    match classify_error(status.as_u16(), &String::from_utf8_lossy(&body)) {
        ApiFailure::Blocked(message) => Ok(Moderated::Blocked { message }),
        ApiFailure::Error(err) => Err(err),
    }
}

async fn check_status(res: reqwest::Response) -> Result<reqwest::Response, AppError> {
    match moderated_status(res).await? {
        Moderated::Ok { value } => Ok(value),
        Moderated::Blocked { message } => Err(AppError::Other(message)),
    }
}

enum ApiFailure {
    Blocked(String),
    Error(AppError),
}

/// fal errors are `{"detail": [{"loc", "msg", "type", ...}]}`, or a plain
/// `{"detail": "..."}` from the queue itself.
fn classify_error(status: u16, body: &str) -> ApiFailure {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let detail = parsed.as_ref().and_then(|v| v.get("detail"));
    let items: Vec<&Value> = match detail {
        Some(Value::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    };
    let message = match detail {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(_)) => items
            .iter()
            .filter_map(|i| i.get("msg").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("; "),
        _ => body.chars().take(200).collect(),
    };
    if items
        .iter()
        .any(|i| i.get("type").and_then(Value::as_str) == Some("content_policy_violation"))
    {
        return ApiFailure::Blocked(blocked_message(&message));
    }
    let text = match status {
        401 | 403 => "fal.ai rejected the API key. Check it in Settings.".to_string(),
        402 => "Not enough fal.ai balance for this video.".to_string(),
        429 => "fal.ai is rate limiting this account. Try again shortly.".to_string(),
        _ if message.is_empty() => format!("fal.ai request failed (HTTP {status})."),
        _ => format!("fal.ai: {message}"),
    };
    ApiFailure::Error(AppError::ApiError {
        status,
        message: text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_follows_the_model_page() {
        let body = seedance_25_body(
            "Format: ...",
            &["data:image/jpeg;base64,AAA".into()],
            Some("data:audio/wav;base64,BBB"),
            "720p",
            true,
            28,
            "16:9",
            Some(7),
        );
        assert_eq!(body["duration"], "28");
        assert_eq!(body["draft"], true);
        assert_eq!(body["generate_audio"], true);
        assert_eq!(body["codec"], "H264");
        assert_eq!(body["image_urls"][0], "data:image/jpeg;base64,AAA");
        assert_eq!(body["audio_urls"][0], "data:audio/wav;base64,BBB");
        assert_eq!(body["seed"], 7);
        let silent = seedance_25_body("p", &[], None, "480p", false, 5, "1:1", None);
        assert!(silent.get("audio_urls").is_none());
        assert!(silent.get("seed").is_none());
    }

    #[test]
    fn statuses_are_read() {
        let s = |v: Value| parse_status(&v);
        assert_eq!(
            s(serde_json::json!({"status": "IN_QUEUE", "queue_position": 3})),
            QueueStatus::Queued { position: Some(3) }
        );
        assert_eq!(
            s(serde_json::json!({"status": "IN_PROGRESS"})),
            QueueStatus::Running
        );
        assert_eq!(
            s(serde_json::json!({"status": "COMPLETED"})),
            QueueStatus::Completed
        );
        match s(
            serde_json::json!({"status": "COMPLETED", "error": "nope", "error_type": "content_policy_violation"}),
        ) {
            QueueStatus::Failed { blocked, message } => {
                assert!(blocked);
                assert!(message.contains("content policy"));
            }
            other => panic!("{other:?}"),
        }
        match s(serde_json::json!({"status": "COMPLETED", "error_type": "generation_timeout"})) {
            QueueStatus::Failed { blocked, .. } => assert!(!blocked),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_content_policy_error_is_a_refusal() {
        let body = r#"{"detail":[{"loc":["body","prompt"],"msg":"flagged","type":"content_policy_violation","url":"x"}]}"#;
        assert!(
            matches!(classify_error(422, body), ApiFailure::Blocked(m) if m.contains("flagged"))
        );
        let body = r#"{"detail":[{"loc":["body","duration"],"msg":"bad","type":"value_error","url":"x"}]}"#;
        assert!(matches!(classify_error(422, body), ApiFailure::Error(_)));
        assert!(matches!(
            classify_error(401, "{}"),
            ApiFailure::Error(AppError::ApiError { status: 401, .. })
        ));
    }

    #[test]
    fn the_key_only_goes_to_the_queue_host() {
        assert!(
            queue_url("https://queue.fal.run/bytedance/seedance-2.5/requests/abc/status").is_ok()
        );
        for bad in [
            "https://evil.example/queue.fal.run/x",
            "http://queue.fal.run/x",
            "https://queue.fal.run.evil.example/x",
            "https://queue.fal.run@evil.example/x",
        ] {
            assert!(queue_url(bad).is_err(), "{bad}");
        }
        let handle = serde_json::json!({
            "request_id": "abc",
            "status_url": "https://queue.fal.run/a/requests/abc/status",
            "response_url": "https://queue.fal.run/a/requests/abc",
            "cancel_url": "https://queue.fal.run/a/requests/abc/cancel",
        });
        assert_eq!(parse_handle(&handle).unwrap().request_id, "abc");
        let mut bad = handle.clone();
        bad["status_url"] = "https://elsewhere.example/x".into();
        assert!(parse_handle(&bad).is_err());
    }

    #[test]
    fn results_need_a_video() {
        let r = parse_video_result(&serde_json::json!({"video": {"url": "https://v3.fal.media/x.mp4"}, "seed": 5, "draft_id": "d1"})).unwrap();
        assert_eq!(r.draft_id.as_deref(), Some("d1"));
        assert!(parse_video_result(&serde_json::json!({"seed": 5})).is_err());
    }
}
