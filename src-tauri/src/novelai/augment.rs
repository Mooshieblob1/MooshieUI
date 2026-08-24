//! Director Tools: NovelAI's `/ai/augment-image` endpoint.
//!
//! Six one-shot image operations that take an image and give one or more back.
//! They share the generation path's host, bearer auth and zip response, so the
//! only genuinely new thing here is the request shape.
//!
//! Results are delivered through the same synthetic `nai-{uuid}` prompt id and
//! the same `comfyui:*` events a generation uses. That is what makes the
//! progress bar, the cancel button, the session-output grid and the gallery all
//! work without knowing this feature exists.
//!
//! Not to be confused with the `director_reference_*` fields in `params.rs`.
//! Those are Precise Reference, a block inside the generate-image payload, and
//! are unrelated to these tools beyond sharing a word.

use std::sync::Arc;

use base64::Engine as _;
use serde_json::Value;

use crate::error::AppError;
use crate::state::AppState;

use super::{EventSink, NovelAiClient};

/// The six tools, named by the `req_type` NovelAI expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectorTool {
    BgRemoval,
    LineArt,
    Sketch,
    Colorize,
    Emotion,
    Declutter,
}

impl DirectorTool {
    /// Parse the wire name. The frontend sends the `req_type` itself rather
    /// than a friendlier alias, so there is exactly one spelling of each tool
    /// in the codebase.
    pub fn parse(name: &str) -> Result<Self, AppError> {
        match name.trim() {
            "bg-removal" => Ok(Self::BgRemoval),
            "lineart" => Ok(Self::LineArt),
            "sketch" => Ok(Self::Sketch),
            "colorize" => Ok(Self::Colorize),
            "emotion" => Ok(Self::Emotion),
            "declutter" => Ok(Self::Declutter),
            other => Err(AppError::Other(format!("Unknown Director Tool: {other}"))),
        }
    }

    pub fn req_type(self) -> &'static str {
        match self {
            Self::BgRemoval => "bg-removal",
            Self::LineArt => "lineart",
            Self::Sketch => "sketch",
            Self::Colorize => "colorize",
            Self::Emotion => "emotion",
            Self::Declutter => "declutter",
        }
    }

    /// Whether this tool reads the `defry` and `prompt` fields at all.
    ///
    /// The other four take the image and nothing else, and sending them extras
    /// is a guess about an endpoint that is only documented by observation, so
    /// the fields are omitted rather than sent as defaults.
    pub fn takes_extras(self) -> bool {
        matches!(self, Self::Colorize | Self::Emotion)
    }
}

/// NovelAI's `defry` range. Out-of-range values are clamped rather than
/// rejected: the slider cannot produce one, so a bad value here means a stale
/// client, and refusing the request would cost the user a round trip for
/// nothing.
const DEFRY_MAX: u8 = 5;

/// What one Director Tools run needs. Deserialized straight off the IPC call.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct AugmentParams {
    /// The `req_type` wire name, parsed by [`DirectorTool::parse`].
    pub tool: String,
    /// Base64 PNG. A `data:` URI prefix is tolerated and stripped.
    pub image: String,
    #[serde(default)]
    pub defry: u8,
    /// Free-text guidance. Colorize takes it directly; Emotion appends it after
    /// the mood.
    #[serde(default)]
    pub prompt: String,
    /// Emotion only. Ignored by every other tool.
    #[serde(default)]
    pub mood: String,
}

/// Drop the `data:image/png;base64,` header a browser `FileReader` leaves on.
fn strip_data_uri(image: &str) -> &str {
    match image.find(";base64,") {
        Some(index) if image.starts_with("data:") => &image[index + ";base64,".len()..],
        _ => image,
    }
}

/// Decode far enough to read the pixel dimensions.
///
/// NovelAI wants `width` and `height` alongside the image, and `OutputImage`
/// carries neither, so they are derived here instead of asking the frontend to
/// decode the same bytes a second time.
fn image_dimensions(image_b64: &str) -> Result<(u32, u32), AppError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(strip_data_uri(image_b64).trim())
        .map_err(|err| {
            AppError::Other(format!("Director Tools image is not valid base64: {err}"))
        })?;
    let decoded = image::load_from_memory(&bytes)
        .map_err(|err| AppError::Other(format!("Director Tools image could not be read: {err}")))?;
    Ok((decoded.width(), decoded.height()))
}

/// Build the `/ai/augment-image` body.
///
/// Per-tool extras are merged at the top level, not nested under a
/// `parameters` object the way generation does it.
pub fn build_request(
    tool: DirectorTool,
    image_b64: &str,
    width: u32,
    height: u32,
    params: &AugmentParams,
) -> Value {
    let mut body = serde_json::json!({
        "req_type": tool.req_type(),
        "width": width,
        "height": height,
        "image": strip_data_uri(image_b64).trim(),
    });

    if tool.takes_extras() {
        let object = body.as_object_mut().expect("json! built an object");
        object.insert("defry".into(), Value::from(params.defry.min(DEFRY_MAX)));
        object.insert("prompt".into(), Value::from(tool_prompt(tool, params)));
    }

    body
}

/// The `prompt` field as the endpoint wants it.
///
/// Emotion has no separate mood field: the mood and the guidance are one string
/// joined by a literal `;;`, as in `happy;;silver hair`. The separator is sent
/// even with nothing after it, because that is the shape the endpoint is known
/// to accept and a bare mood is not.
fn tool_prompt(tool: DirectorTool, params: &AugmentParams) -> String {
    match tool {
        DirectorTool::Emotion => format!("{};;{}", params.mood.trim(), params.prompt.trim()),
        _ => params.prompt.trim().to_string(),
    }
}

/// A validated run, ready to spawn.
///
/// Splitting validation out of [`run`] lets the command reject an unknown tool
/// or an unreadable image as a plain command error, before a prompt id the
/// frontend has already committed to exists. It also means the image is decoded
/// once rather than once per caller.
pub struct PreparedAugment {
    tool: DirectorTool,
    width: u32,
    height: u32,
    params: AugmentParams,
}

impl PreparedAugment {
    pub fn prepare(params: AugmentParams) -> Result<Self, AppError> {
        let tool = DirectorTool::parse(&params.tool)?;
        let (width, height) = image_dimensions(&params.image)?;
        Ok(Self {
            tool,
            width,
            height,
            params,
        })
    }
}

/// Run a Director Tool to completion, emitting progress as it goes.
///
/// Errors are emitted as `comfyui:execution_error` *and* returned, mirroring
/// [`super::run`], so a spawned caller can log while the frontend clears its
/// pending state.
pub async fn run(
    state: Arc<AppState>,
    sink: EventSink,
    prompt_id: String,
    prepared: PreparedAugment,
) -> Result<(), AppError> {
    match run_inner(&state, &sink, &prompt_id, &prepared).await {
        Ok(()) => Ok(()),
        Err(err) => {
            sink.emit(
                "comfyui:execution_error",
                serde_json::json!({
                    "prompt_id": prompt_id,
                    "error": err.to_string(),
                    "exception_message": err.to_string(),
                    "node_type": "NovelAI Director Tools",
                }),
            );
            Err(err)
        }
    }
}

async fn run_inner(
    state: &Arc<AppState>,
    sink: &EventSink,
    prompt_id: &str,
    prepared: &PreparedAugment,
) -> Result<(), AppError> {
    let PreparedAugment {
        tool,
        width,
        height,
        params,
    } = prepared;
    let (tool, width, height) = (*tool, *width, *height);

    let api_key = {
        let config = state.config.read().await;
        config.novelai_api_key.clone().unwrap_or_default()
    };
    let client = NovelAiClient::new(&state.http_client, &api_key)?;
    let body = build_request(tool, &params.image, width, height, params);

    // The endpoint does not stream, so there is no real progress to report.
    // A single 0-of-1 tick still gets the frontend's progress bar on screen and
    // the cancel button live, which is what the wait needs.
    sink.emit(
        "comfyui:progress",
        serde_json::json!({
            "prompt_id": prompt_id,
            "value": 0,
            "max": 1,
            "node": "NovelAI Director Tools",
        }),
    );

    log::info!(
        "NovelAI Director Tools {prompt_id}: {} on {width}x{height}",
        tool.req_type()
    );

    let images = client.augment_image(&body).await?;

    // Cancelling cannot recall a request that is already paid for, so a
    // cancelled run still delivers nothing rather than half a result set: the
    // user asked for it to stop, and the images are reproducible from the
    // source, which is still where it was.
    if state.prompt_queue.is_cancelled(prompt_id) {
        state.prompt_queue.cleanup_alias(prompt_id);
        return Ok(());
    }

    // Background removal answers with several images (NovelAI's own UI shows
    // Masked, Generated and Blend), so every returned image is delivered rather
    // than assuming the one-image shape the generation path can assume.
    log::info!(
        "NovelAI Director Tools {prompt_id}: {} returned {} image(s)",
        tool.req_type(),
        images.len()
    );
    for image in &images {
        super::deliver_image(sink, prompt_id, image).await;
    }

    // `node: null` is the frontend's completion signal.
    sink.emit(
        "comfyui:executing",
        serde_json::json!({ "prompt_id": prompt_id, "node": serde_json::Value::Null }),
    );
    state.prompt_queue.cleanup_alias(prompt_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(tool: &str) -> AugmentParams {
        AugmentParams {
            tool: tool.into(),
            image: "aW1hZ2U=".into(),
            ..Default::default()
        }
    }

    #[test]
    fn every_tool_round_trips_through_its_wire_name() {
        for name in [
            "bg-removal",
            "lineart",
            "sketch",
            "colorize",
            "emotion",
            "declutter",
        ] {
            let tool = DirectorTool::parse(name).expect("known tool");
            assert_eq!(tool.req_type(), name);
        }
        assert!(DirectorTool::parse("upscale").is_err());
        // Whitespace from a hand-edited client should not cost a round trip.
        assert_eq!(
            DirectorTool::parse(" sketch ").expect("trimmed"),
            DirectorTool::Sketch
        );
    }

    /// The four extra-less tools must send exactly four fields. An unexpected
    /// `prompt` or `defry` is a guess about an endpoint that is documented only
    /// by observation.
    #[test]
    fn a_tool_without_extras_sends_only_the_image_and_its_size() {
        let mut input = params("lineart");
        input.defry = 3;
        input.prompt = "should not be sent".into();
        let body = build_request(DirectorTool::LineArt, &input.image, 832, 1216, &input);

        let object = body.as_object().expect("object body");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["height", "image", "req_type", "width"]);
        assert_eq!(body["req_type"], "lineart");
        assert_eq!(body["width"], 832);
        assert_eq!(body["height"], 1216);
    }

    #[test]
    fn colorize_sends_its_prompt_and_a_clamped_defry() {
        let mut input = params("colorize");
        input.defry = 9;
        input.prompt = "  silver hair  ".into();
        let body = build_request(DirectorTool::Colorize, &input.image, 1024, 1024, &input);

        assert_eq!(body["defry"], 5);
        assert_eq!(body["prompt"], "silver hair");
        // The mood belongs to Emotion alone and must not leak in here.
        assert!(!body["prompt"].as_str().expect("string").contains(";;"));
    }

    /// Emotion has no mood field of its own: it rides in the prompt, joined by
    /// a literal `;;`.
    #[test]
    fn emotion_joins_its_mood_and_prompt_with_a_double_semicolon() {
        let mut input = params("emotion");
        input.mood = "happy".into();
        input.prompt = "silver hair".into();
        let body = build_request(DirectorTool::Emotion, &input.image, 640, 640, &input);
        assert_eq!(body["prompt"], "happy;;silver hair");

        // The separator survives an empty guidance field, because a bare mood
        // is not a shape the endpoint is known to accept.
        input.prompt = String::new();
        let body = build_request(DirectorTool::Emotion, &input.image, 640, 640, &input);
        assert_eq!(body["prompt"], "happy;;");
    }

    #[test]
    fn a_data_uri_header_is_stripped_before_the_image_is_sent() {
        let mut input = params("sketch");
        input.image = "data:image/png;base64,aW1hZ2U=".into();
        let body = build_request(DirectorTool::Sketch, &input.image, 64, 64, &input);
        assert_eq!(body["image"], "aW1hZ2U=");

        // A bare base64 payload is passed through untouched.
        assert_eq!(strip_data_uri("aW1hZ2U="), "aW1hZ2U=");
        // A `;base64,` appearing in something that is not a data URI is not a
        // header, and slicing there would corrupt the payload.
        assert_eq!(strip_data_uri("x;base64,y"), "x;base64,y");
    }

    #[test]
    fn dimensions_come_from_the_image_itself() {
        // A 2x3 PNG, encoded the way the frontend sends one.
        let mut png = Vec::new();
        let buffer = image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255]));
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        let encoded = base64::engine::general_purpose::STANDARD.encode(&png);

        assert_eq!(image_dimensions(&encoded).expect("dimensions"), (2, 3));
        assert_eq!(
            image_dimensions(&format!("data:image/png;base64,{encoded}")).expect("dimensions"),
            (2, 3)
        );
        // Garbage is reported before any Anlas is spent.
        assert!(image_dimensions("not base64!").is_err());
        assert!(image_dimensions("aW1hZ2U=").is_err());
    }
}
