//! Decoding of NovelAI responses: the ZIP-of-PNG body and the SSE stream.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Unpack the ZIP archive NovelAI returns from `/ai/generate-image`.
///
/// The archive holds one entry per sample, named `image_0.png` and up. Entries
/// are returned in archive order, which matches sample order.
pub fn unpack_images(body: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(body))
        .map_err(|e| format!("NovelAI returned an unreadable archive: {e}"))?;

    let mut images = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("failed to read archive entry {i}: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("failed to extract archive entry {i}: {e}"))?;
        if !bytes.is_empty() {
            images.push(bytes);
        }
    }

    if images.is_empty() {
        return Err("NovelAI returned an archive with no images".into());
    }
    Ok(images)
}

/// One decoded frame from `/ai/generate-image-stream`.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// An intermediate preview. `step` is zero-based.
    Intermediate { image: Vec<u8>, step: u32 },
    /// The final image for a sample.
    Final { image: Vec<u8> },
    /// An error frame. NovelAI bills nothing for these but does send them.
    Error { message: String },
}

/// Incremental SSE parser.
///
/// NovelAI's stream is standard `event:`/`data:` SSE with frames separated by a
/// blank line. Bytes arrive in arbitrary chunks, so the parser buffers until it
/// sees a complete frame.
#[derive(Debug, Default)]
pub struct StreamDecoder {
    buffer: String,
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk and return every event that became complete.
    ///
    /// Undecodable frames are skipped rather than failing the generation: a
    /// malformed preview must not lose an image the user already paid for.
    pub fn push(&mut self, chunk: &str) -> Vec<StreamEvent> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();

        // Frames end at a blank line. Normalise CRLF first so both forms split.
        while let Some(end) = find_frame_end(&self.buffer) {
            let frame: String = self.buffer.drain(..end.0).collect();
            self.buffer.drain(..end.1);
            if let Some(event) = decode_frame(&frame) {
                events.push(event);
            }
        }
        events
    }
}

/// Returns `(frame_len, separator_len)` for the first complete frame.
fn find_frame_end(buffer: &str) -> Option<(usize, usize)> {
    let lf = buffer.find("\n\n").map(|i| (i, 2));
    let crlf = buffer.find("\r\n\r\n").map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn decode_frame(frame: &str) -> Option<StreamEvent> {
    let mut event_name = String::new();
    let mut data = String::new();

    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }

    if data.is_empty() {
        return None;
    }
    let payload: Value = serde_json::from_str(&data).ok()?;

    if event_name == "error" || payload.get("error").is_some() {
        let message = payload
            .get("error")
            .and_then(|v| v.as_str())
            .or_else(|| payload.get("message").and_then(|v| v.as_str()))
            .unwrap_or("NovelAI reported a stream error")
            .to_string();
        return Some(StreamEvent::Error { message });
    }

    let encoded = payload.get("image").and_then(|v| v.as_str())?;
    let image = decode_base64(encoded)?;
    let step = payload
        .get("step_ix")
        .and_then(|v| v.as_u64())
        .unwrap_or_default() as u32;

    // NovelAI names the terminal frame `final`; everything else is a preview.
    if event_name == "final" || event_name == "newImage" && payload.get("step_ix").is_none() {
        Some(StreamEvent::Final { image })
    } else {
        Some(StreamEvent::Intermediate { image, step })
    }
}

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// NovelAI's `/user/subscription` response.
///
/// Only the fields MooshieUI displays are named. `extra` keeps everything else
/// so the Opus allowance fields can be identified from a real response without
/// another round of guessing at names.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Subscription {
    #[serde(default)]
    pub tier: u8,
    #[serde(default)]
    pub active: bool,
    #[serde(default, rename = "expiresAt")]
    pub expires_at: Option<i64>,
    #[serde(default, rename = "trainingStepsLeft")]
    pub training_steps_left: Option<TrainingSteps>,
    #[serde(default)]
    pub perks: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrainingSteps {
    #[serde(default, rename = "fixedTrainingStepsLeft")]
    pub fixed: i64,
    #[serde(default, rename = "purchasedTrainingSteps")]
    pub purchased: i64,
}

impl Subscription {
    /// Anlas is the sum of the monthly allowance and any purchased balance.
    pub fn anlas(&self) -> i64 {
        self.training_steps_left
            .as_ref()
            .map_or(0, |t| t.fixed + t.purchased)
    }

    /// Opus is tier 3. Only an active subscription grants the free generations.
    pub fn is_opus(&self) -> bool {
        self.active && self.tier >= 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use std::io::Write;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, bytes) in entries {
                w.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(bytes).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn unpacks_images_in_archive_order() {
        let zip = zip_of(&[("image_0.png", b"first"), ("image_1.png", b"second")]);
        let images = unpack_images(&zip).unwrap();
        assert_eq!(images, vec![b"first".to_vec(), b"second".to_vec()]);
    }

    #[test]
    fn an_empty_archive_is_an_error_not_an_empty_success() {
        let zip = zip_of(&[]);
        assert!(unpack_images(&zip).is_err());
    }

    #[test]
    fn a_non_archive_body_reports_clearly() {
        let err = unpack_images(b"{\"message\":\"Unauthorized\"}").unwrap_err();
        assert!(err.contains("unreadable archive"), "{err}");
    }

    #[test]
    fn decodes_a_preview_then_a_final_frame() {
        let mut d = StreamDecoder::new();
        let events = d.push(&format!(
            "event: newImage\ndata: {{\"step_ix\":4,\"image\":\"{}\"}}\n\n",
            b64(b"preview")
        ));
        assert_eq!(
            events,
            vec![StreamEvent::Intermediate {
                image: b"preview".to_vec(),
                step: 4
            }]
        );

        let events = d.push(&format!(
            "event: final\ndata: {{\"image\":\"{}\"}}\n\n",
            b64(b"done")
        ));
        assert_eq!(
            events,
            vec![StreamEvent::Final {
                image: b"done".to_vec()
            }]
        );
    }

    #[test]
    fn buffers_across_chunk_boundaries() {
        let payload = format!("event: final\ndata: {{\"image\":\"{}\"}}", b64(b"done"));
        let (head, tail) = payload.split_at(20);
        let mut d = StreamDecoder::new();
        assert!(d.push(head).is_empty());
        assert!(d.push(tail).is_empty());
        let events = d.push("\n\n");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn handles_crlf_frames() {
        let mut d = StreamDecoder::new();
        let events = d.push(&format!(
            "event: final\r\ndata: {{\"image\":\"{}\"}}\r\n\r\n",
            b64(b"done")
        ));
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn a_malformed_frame_is_skipped_not_fatal() {
        let mut d = StreamDecoder::new();
        let events = d.push(&format!(
            "event: newImage\ndata: not json\n\nevent: final\ndata: {{\"image\":\"{}\"}}\n\n",
            b64(b"done")
        ));
        assert_eq!(
            events,
            vec![StreamEvent::Final {
                image: b"done".to_vec()
            }]
        );
    }

    #[test]
    fn error_frames_surface_their_message() {
        let mut d = StreamDecoder::new();
        let events = d.push("event: error\ndata: {\"error\":\"out of Anlas\"}\n\n");
        assert_eq!(
            events,
            vec![StreamEvent::Error {
                message: "out of Anlas".into()
            }]
        );
    }

    #[test]
    fn subscription_sums_anlas_and_detects_opus() {
        let sub: Subscription = serde_json::from_str(
            r#"{"tier":3,"active":true,"trainingStepsLeft":{"fixedTrainingStepsLeft":1000,"purchasedTrainingSteps":250},"unknownOpusField":42}"#,
        )
        .unwrap();
        assert_eq!(sub.anlas(), 1250);
        assert!(sub.is_opus());
        assert!(sub.extra.contains_key("unknownOpusField"));
    }

    #[test]
    fn an_expired_opus_tier_is_not_opus() {
        let sub: Subscription = serde_json::from_str(r#"{"tier":3,"active":false}"#).unwrap();
        assert!(!sub.is_opus());
        assert_eq!(sub.anlas(), 0);
    }
}
