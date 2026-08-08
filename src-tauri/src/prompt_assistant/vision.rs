//! Sending an image to an external LLM.
//!
//! Two jobs live here because they are the same concern from opposite ends:
//! turning an uploaded frame into something a vision model can read, and
//! working out which models can read one at all.

use base64::Engine;
use std::time::Duration;

use crate::error::AppError;
use crate::state::AppState;

/// Fetch an image already uploaded to ComfyUI's input folder and encode it for a
/// vision request, or `None` with a log line if any step fails.
///
/// Callers pass the upload filename rather than bytes, because the filename is
/// all the client keeps: an upload leaves only a name behind, and routing
/// megabytes of base64 back through IPC to send it straight out again would be
/// pointless. Every failure degrades to a text-only turn, since a rewrite
/// written from the prompt alone beats an error dialog.
pub async fn load_input_frame(state: &AppState, filename: Option<String>) -> Option<VisionImage> {
    let filename = filename?;
    let filename = filename.trim();
    if filename.is_empty() {
        return None;
    }
    let bytes = match state.get_input_image_bytes(filename, "").await {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[prompt-assistant] could not read input image {filename}: {e}");
            return None;
        }
    };
    // Decode plus resize plus JPEG encode is real CPU work on a full-resolution
    // frame, and this runs on the same runtime that serves browser mode.
    let encoded =
        tokio::task::spawn_blocking(move || encode_downscaled(&bytes, VISION_MAX_PIXELS)).await;
    match encoded {
        Ok(Ok(img)) => Some(img),
        Ok(Err(e)) => {
            log::warn!("[prompt-assistant] could not encode input image {filename}: {e}");
            None
        }
        Err(e) => {
            log::warn!("[prompt-assistant] image encode task failed: {e}");
            None
        }
    }
}

/// An image ready to inline in a chat request.
///
/// Both wire formats want the same two things spelled differently, so the
/// per-wire code in `server.rs` formats these fields rather than re-deriving
/// them.
pub struct VisionImage {
    /// MIME type, always `image/jpeg` as produced here.
    pub media_type: String,
    /// Base64 of the encoded bytes, no data-URI prefix.
    pub base64: String,
}

/// Pixel budget for an inlined frame.
///
/// Roughly 1 MP. Every provider downsamples above its own ceiling anyway, and
/// the frame only has to answer "what is in this shot" - detail beyond this
/// buys nothing and costs tokens on every rewrite.
pub const VISION_MAX_PIXELS: u32 = 1_050_000;

/// JPEG quality for the inlined frame. High enough that text and faces survive,
/// low enough that a 1 MP frame stays well under a megabyte of base64.
const JPEG_QUALITY: u8 = 85;

/// Decode, downscale to `max_pixels`, and re-encode as base64 JPEG.
///
/// CPU-bound: call it from `spawn_blocking`. Images already inside the budget
/// are still re-encoded, because the source may be a PNG several times the size
/// of the JPEG a model needs.
pub fn encode_downscaled(bytes: &[u8], max_pixels: u32) -> Result<VisionImage, AppError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| AppError::LlmError(format!("Could not read the frame image: {e}")))?;
    let (w, h) = (img.width().max(1), img.height().max(1));
    let pixels = u64::from(w) * u64::from(h);
    let img = if pixels > u64::from(max_pixels) {
        let scale = (f64::from(max_pixels) / pixels as f64).sqrt();
        let nw = ((f64::from(w) * scale).round() as u32).max(1);
        let nh = ((f64::from(h) * scale).round() as u32).max(1);
        img.resize(nw, nh, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    // Flatten to RGB: JPEG has no alpha channel, and a frame handed to a video
    // model is opaque by definition.
    let rgb = img.to_rgb8();
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY)
        .encode(
            &rgb,
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| AppError::LlmError(format!("Could not encode the frame image: {e}")))?;
    Ok(VisionImage {
        media_type: "image/jpeg".to_string(),
        base64: base64::engine::general_purpose::STANDARD.encode(&buf),
    })
}

/// Ollama's native API root, derived from whatever OpenAI-compatible base URL
/// the user configured (`http://host:11434/v1`, `http://host:11434`, or the
/// full chat-completions URL all reduce to `http://host:11434`).
fn ollama_root(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let base = base.strip_suffix("/chat/completions").unwrap_or(base);
    let base = base.strip_suffix("/models").unwrap_or(base);
    let base = base.trim_end_matches('/');
    base.strip_suffix("/v1").unwrap_or(base).to_string()
}

/// How many installed models to probe. A capability check is one request each,
/// and nobody picks a model out of a list longer than this anyway.
const MAX_PROBED_MODELS: usize = 60;

/// The names of the vision-capable models installed on the Ollama server behind
/// `base_url`, or `None` when that endpoint is not Ollama.
///
/// The OpenAI-compatible `/models` list carries no modality information, so a
/// text-only model is indistinguishable from a VLM there. Ollama's own API does
/// know: `/api/show` reports a `capabilities` array, and vision models list
/// `"vision"` in it. Probing is best-effort throughout - any failure returns
/// `None` so the caller falls back to the unfiltered list rather than showing
/// an empty picker.
pub async fn ollama_vision_models(client: &reqwest::Client, base_url: &str) -> Option<Vec<String>> {
    let root = ollama_root(base_url);
    if root.is_empty() {
        return None;
    }
    let resp = client
        .get(format!("{root}/api/tags"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    let names: Vec<String> = v["models"]
        .as_array()?
        .iter()
        .filter_map(|m| m["model"].as_str().or_else(|| m["name"].as_str()))
        .map(|s| s.to_string())
        .take(MAX_PROBED_MODELS)
        .collect();
    if names.is_empty() {
        return None;
    }

    let mut vision = Vec::new();
    for name in names {
        if model_has_vision(client, &root, &name).await {
            vision.push(name);
        }
    }
    Some(vision)
}

/// Whether `/api/show` reports the `vision` capability for one model.
async fn model_has_vision(client: &reqwest::Client, root: &str, name: &str) -> bool {
    let resp = client
        .post(format!("{root}/api/show"))
        .json(&serde_json::json!({ "model": name }))
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let Ok(resp) = resp else { return false };
    if !resp.status().is_success() {
        return false;
    }
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return false;
    };
    v["capabilities"]
        .as_array()
        .is_some_and(|caps| caps.iter().any(|c| c.as_str() == Some("vision")))
}

/// Whether `id` from an OpenAI-compatible `/models` list names the same model
/// as `name` from Ollama's `/api/tags`.
///
/// Ollama serves both, but not always identically: `/api/tags` always carries
/// the `:tag` suffix while `/v1/models` has been seen to drop `:latest`.
pub fn model_id_matches(id: &str, name: &str) -> bool {
    id == name
        || name.strip_suffix(":latest").is_some_and(|stem| stem == id)
        || id.strip_suffix(":latest").is_some_and(|stem| stem == name)
}
