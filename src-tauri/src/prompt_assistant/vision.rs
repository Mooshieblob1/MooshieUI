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

/// How many reference images one turn may carry.
///
/// Four is what the NovelAI enhance modal offers. The ceiling exists because
/// every image is inlined into the request body: a fifth costs another ~200 KB
/// of base64 on a request that already carries four, and no provider reads a
/// long image list as carefully as a short one.
pub const MAX_VISION_IMAGES: usize = 4;

/// Strip a `data:image/png;base64,` prefix if the client left one on.
///
/// The frontend helper that produces these (`fileToNovelAiBase64`) already cuts
/// the prefix, but a caller that hands over a canvas `toDataURL()` unmodified
/// is doing the obvious thing, and silently accepting both is cheaper than a
/// round of "why is my reference ignored".
fn strip_data_uri(s: &str) -> &str {
    match s.split_once("base64,") {
        Some((prefix, rest)) if prefix.starts_with("data:") => rest,
        _ => s,
    }
}

/// Decode client-supplied base64 images into vision payloads, in order.
///
/// Unlike `load_input_frame` these arrive as bytes rather than as a ComfyUI
/// upload name, because the feature behind them (the NovelAI prompt enhance)
/// runs on a backend that may have no ComfyUI process at all. The client has
/// already downscaled each one, and they are re-encoded here anyway so that a
/// caller which skipped that step cannot inline a 12 MP PNG.
///
/// Order is meaningful: the user turn names the images by position, so a
/// failed decode is dropped from the end of the log rather than silently
/// renumbering the survivors. An image that cannot be read degrades to a
/// text-only reference the same way a missing frame does.
pub async fn load_inline_images(data: Vec<String>) -> Vec<VisionImage> {
    let data: Vec<String> = data
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .take(MAX_VISION_IMAGES)
        .collect();
    if data.is_empty() {
        return Vec::new();
    }
    // One blocking hop for the whole batch: four decode-resize-encode passes
    // back to back would otherwise stall the runtime that serves browser mode.
    let decoded = tokio::task::spawn_blocking(move || {
        data.iter()
            .enumerate()
            .filter_map(|(i, raw)| {
                let bytes = match base64::engine::general_purpose::STANDARD
                    .decode(strip_data_uri(raw.trim()))
                {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!(
                            "[prompt-assistant] reference image {i} is not valid base64: {e}"
                        );
                        return None;
                    }
                };
                match encode_downscaled(&bytes, VISION_MAX_PIXELS) {
                    Ok(img) => Some(img),
                    Err(e) => {
                        log::warn!("[prompt-assistant] could not encode reference image {i}: {e}");
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
    })
    .await;
    match decoded {
        Ok(images) => images,
        Err(e) => {
            log::warn!("[prompt-assistant] reference image encode task failed: {e}");
            Vec::new()
        }
    }
}

/// Gather every image one turn should carry, in the order the model sees them.
///
/// The two sources exist because the two callers hold images differently: a
/// ComfyUI input filename for the video paths, raw base64 for the NovelAI
/// prompt enhance, whose users may have no ComfyUI process running at all. The
/// filename goes first when both are present, since it is the frame the turn is
/// *about* and the base64 ones are references to it.
pub async fn collect_images(
    state: &AppState,
    filename: Option<String>,
    data: Option<Vec<String>>,
) -> Vec<VisionImage> {
    let mut images: Vec<VisionImage> = load_input_frame(state, filename)
        .await
        .into_iter()
        .collect();
    let room = MAX_VISION_IMAGES.saturating_sub(images.len());
    if room > 0 {
        if let Some(data) = data {
            images.extend(load_inline_images(data).await.into_iter().take(room));
        }
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real PNG, small enough to inline: the decode path is what is under
    /// test, so a handcrafted byte string would only test the error branch.
    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn strips_a_data_uri_prefix() {
        assert_eq!(strip_data_uri("data:image/png;base64,AAAA"), "AAAA");
        assert_eq!(strip_data_uri("data:image/jpeg;base64,QQ=="), "QQ==");
    }

    #[test]
    fn leaves_bare_base64_alone() {
        assert_eq!(strip_data_uri("AAAA"), "AAAA");
        // A payload that happens to contain the marker without the data: scheme
        // is data, not a prefix, and must survive intact.
        assert_eq!(strip_data_uri("xxbase64,yy"), "xxbase64,yy");
    }

    #[test]
    fn encodes_as_jpeg_within_the_pixel_budget() {
        let img = encode_downscaled(&png_bytes(64, 32), VISION_MAX_PIXELS).unwrap();
        assert_eq!(img.media_type, "image/jpeg");
        assert!(!img.base64.is_empty());
    }

    #[test]
    fn downscales_past_the_pixel_budget() {
        // 200x200 = 40k pixels, asked to fit in 10k: the result must be smaller
        // in both directions, not merely re-encoded.
        let img = encode_downscaled(&png_bytes(200, 200), 10_000).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&img.base64)
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.width() < 200 && decoded.height() < 200);
        assert!(u64::from(decoded.width()) * u64::from(decoded.height()) <= 10_000);
    }

    #[tokio::test]
    async fn loads_inline_images_in_order() {
        let images = load_inline_images(vec![b64(&png_bytes(8, 8)), b64(&png_bytes(16, 16))]).await;
        assert_eq!(images.len(), 2);
        assert!(images.iter().all(|i| i.media_type == "image/jpeg"));
    }

    #[tokio::test]
    async fn skips_blank_and_undecodable_entries() {
        let images = load_inline_images(vec![
            "   ".to_string(),
            "not base64 at all!!".to_string(),
            b64(b"still not an image"),
            b64(&png_bytes(8, 8)),
        ])
        .await;
        // Only the real PNG survives; the rest degrade to nothing rather than
        // failing the whole turn.
        assert_eq!(images.len(), 1);
    }

    #[tokio::test]
    async fn caps_the_batch() {
        let many: Vec<String> = (0..MAX_VISION_IMAGES + 3)
            .map(|_| b64(&png_bytes(8, 8)))
            .collect();
        assert_eq!(load_inline_images(many).await.len(), MAX_VISION_IMAGES);
    }

    #[tokio::test]
    async fn empty_input_makes_no_images() {
        assert!(load_inline_images(Vec::new()).await.is_empty());
        assert!(load_inline_images(vec![String::new()]).await.is_empty());
    }
}
