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
///
/// `sample` is the index within the batch, so a multi-sample generation can be
/// reassembled in order even though NovelAI interleaves the samples.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// An intermediate preview. `step` is zero-based.
    Intermediate {
        image: Vec<u8>,
        sample: u32,
        step: u32,
    },
    /// The final image for a sample.
    Final { image: Vec<u8>, sample: u32 },
    /// An error frame. NovelAI bills nothing for these but does send them.
    Error { message: String },
}

/// A frame longer than this is treated as a corrupt length prefix.
///
/// Without the cap a garbled prefix would leave the decoder waiting forever for
/// bytes that are never coming, which the user would see as a generation that
/// hangs after they have already paid for it.
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Incremental decoder for NovelAI's `msgpack` image stream.
///
/// The stream is not SSE. Each frame is a big-endian `u32` byte count followed
/// by that many bytes of msgpack holding a map: `event_type` is one of
/// `intermediate`, `final` or `error`, alongside `image`, `samp_ix` and
/// `step_ix`, or `message` on an error. Bytes arrive in arbitrary chunks, so
/// the decoder buffers until a whole frame is present.
#[derive(Debug, Default)]
pub struct StreamDecoder {
    buffer: Vec<u8>,
    /// Set once a length prefix is impossible, to stop re-reporting it.
    poisoned: bool,
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk and return every event that became complete.
    ///
    /// Undecodable frames are skipped rather than failing the generation: a
    /// malformed preview must not lose an image the user already paid for.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        if self.poisoned {
            return events;
        }
        self.buffer.extend_from_slice(chunk);

        while self.buffer.len() >= 4 {
            let len = u32::from_be_bytes([
                self.buffer[0],
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
            ]) as usize;

            if len > MAX_FRAME_BYTES {
                self.poisoned = true;
                self.buffer.clear();
                events.push(StreamEvent::Error {
                    message: format!("NovelAI sent an unreadable stream frame of {len} bytes"),
                });
                return events;
            }
            if self.buffer.len() < 4 + len {
                break;
            }

            let frame = self.buffer[4..4 + len].to_vec();
            self.buffer.drain(..4 + len);
            if let Some(event) = decode_frame(&frame) {
                events.push(event);
            }
        }
        events
    }
}

fn decode_frame(frame: &[u8]) -> Option<StreamEvent> {
    let value = rmpv::decode::read_value(&mut &frame[..]).ok()?;
    let event_type = field(&value, "event_type")
        .and_then(as_text)
        .unwrap_or_default();

    if event_type == "error" {
        let message = field(&value, "message")
            .and_then(as_text)
            .unwrap_or_else(|| "NovelAI reported a stream error".to_string());
        return Some(StreamEvent::Error { message });
    }

    let image = field(&value, "image").and_then(as_bytes)?;
    let sample = field(&value, "samp_ix").and_then(as_u32).unwrap_or(0);

    if event_type == "final" {
        return Some(StreamEvent::Final { image, sample });
    }
    let step = field(&value, "step_ix").and_then(as_u32).unwrap_or(0);
    Some(StreamEvent::Intermediate {
        image,
        sample,
        step,
    })
}

fn field<'a>(value: &'a rmpv::Value, key: &str) -> Option<&'a rmpv::Value> {
    let rmpv::Value::Map(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

fn as_text(value: &rmpv::Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn as_bytes(value: &rmpv::Value) -> Option<Vec<u8>> {
    match value {
        rmpv::Value::Binary(bytes) => Some(bytes.clone()),
        // Some encoders emit the payload as a msgpack string instead of a bin.
        rmpv::Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

fn as_u32(value: &rmpv::Value) -> Option<u32> {
    value.as_u64().map(|v| v as u32)
}

/// NovelAI's `/user/subscription` response.
///
/// Only the fields MooshieUI displays are named. `extra` keeps everything else,
/// which is how `usage` was identified from a real response.
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
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Derived from `usage` for the UI. Never read from NovelAI's response.
    #[serde(default, skip_deserializing, rename = "opusAllowance")]
    pub opus_allowance: Option<OpusAllowance>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// The Opus generation allowance behind the V5 usage bar.
///
/// `percent` is the allowance *remaining*, confirmed against the NovelAI web
/// app, which labels the same field "% of Opus Generations remaining".
/// `time_until_next_percent` is the seconds until one more point is restored,
/// and `is_negative` marks an account that has spent past the allowance.
///
/// These are the raw numbers. `OpusAllowance` holds what the bar draws.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub percent: i64,
    #[serde(default, rename = "isNegative")]
    pub is_negative: bool,
    #[serde(default, rename = "timeUntilNextPercent")]
    pub time_until_next_percent: i64,
}

/// What the Opus usage bar actually draws, derived from [`Usage`].
///
/// The arithmetic mirrors NovelAI's own web app so the two readouts agree.
/// `approx_images` is an estimate NovelAI itself presents as approximate: the
/// allowance is a percentage, not an image count, and the conversion is a
/// fixed ratio the site applies.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OpusAllowance {
    /// Allowance remaining, floored at 0 but *not* capped at 100.
    ///
    /// Anlatan hands out bonus allowance from time to time, which puts the
    /// account above a full bar, so the readout has to be able to say 200%.
    pub percent: i64,
    /// [`Self::percent`] capped at 100, for the bar's width only.
    #[serde(rename = "barPercent")]
    pub bar_percent: i64,
    /// Above a full allowance, i.e. a bonus grant the bar cannot draw in full.
    #[serde(rename = "isBonus")]
    pub is_bonus: bool,
    /// Roughly how many images the remaining allowance covers.
    #[serde(rename = "approxImages")]
    pub approx_images: i64,
    /// Nothing left. Further generations cost Anlas.
    #[serde(rename = "isEmpty")]
    pub is_empty: bool,
    /// Nearly gone, worth warning about. Matches the site's threshold.
    #[serde(rename = "isLow")]
    pub is_low: bool,
    /// Refill rate, one decimal place, as the site reports it.
    #[serde(rename = "refillPercentPerDay")]
    pub refill_percent_per_day: f64,
    /// The same rate expressed as images.
    #[serde(rename = "refillImagesPerDay")]
    pub refill_images_per_day: i64,
    /// Seconds until the next point is restored, straight from the API.
    #[serde(rename = "secondsUntilNextPercent")]
    pub seconds_until_next_percent: i64,
}

/// NovelAI's own percent-to-images ratio, taken from its usage bar.
const IMAGES_PER_PERCENT: f64 = 17.3;

impl Usage {
    fn allowance(&self) -> OpusAllowance {
        // A negative balance displays as empty rather than as a negative bar.
        // No upper cap: a bonus grant legitimately reads above 100, and the
        // bar's width is capped separately so only the drawing is bounded.
        let percent = if self.is_negative {
            0
        } else {
            self.percent.max(0)
        };
        let per_day = if self.time_until_next_percent > 0 {
            (86_400.0 / self.time_until_next_percent as f64 * 10.0).round() / 10.0
        } else {
            0.0
        };
        OpusAllowance {
            percent,
            bar_percent: percent.min(100),
            is_bonus: percent > 100,
            approx_images: (IMAGES_PER_PERCENT * percent as f64).round() as i64,
            is_empty: self.is_negative || self.percent <= 0,
            is_low: self.is_negative || self.percent < 5,
            refill_percent_per_day: per_day,
            refill_images_per_day: (IMAGES_PER_PERCENT * per_day).round() as i64,
            seconds_until_next_percent: self.time_until_next_percent.max(0),
        }
    }
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

    /// Fill in [`Self::opus_allowance`] from `usage`.
    ///
    /// Only Opus accounts have an allowance, so lower tiers get `None` and the
    /// UI draws no bar, which is what the website does.
    pub fn derive_opus_allowance(&mut self) {
        self.opus_allowance = match (self.is_opus(), self.usage.as_ref()) {
            (true, Some(usage)) => Some(usage.allowance()),
            _ => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Build one length-prefixed msgpack frame, the way NovelAI sends them.
    fn frame(entries: &[(&str, rmpv::Value)]) -> Vec<u8> {
        let map = rmpv::Value::Map(
            entries
                .iter()
                .map(|(k, v)| (rmpv::Value::from(*k), v.clone()))
                .collect(),
        );
        let mut body = Vec::new();
        rmpv::encode::write_value(&mut body, &map).unwrap();
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&body);
        out
    }

    fn bin(bytes: &[u8]) -> rmpv::Value {
        rmpv::Value::Binary(bytes.to_vec())
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
        let events = d.push(&frame(&[
            ("event_type", "intermediate".into()),
            ("samp_ix", 0.into()),
            ("step_ix", 4.into()),
            ("image", bin(b"preview")),
        ]));
        assert_eq!(
            events,
            vec![StreamEvent::Intermediate {
                image: b"preview".to_vec(),
                sample: 0,
                step: 4,
            }]
        );

        let events = d.push(&frame(&[
            ("event_type", "final".into()),
            ("samp_ix", 0.into()),
            ("image", bin(b"done")),
        ]));
        assert_eq!(
            events,
            vec![StreamEvent::Final {
                image: b"done".to_vec(),
                sample: 0,
            }]
        );
    }

    #[test]
    fn buffers_across_chunk_boundaries() {
        let bytes = frame(&[("event_type", "final".into()), ("image", bin(b"done"))]);
        // Split inside the length prefix, then again inside the body: the
        // network decides where chunks land, not the frame boundaries.
        let mut d = StreamDecoder::new();
        assert!(d.push(&bytes[..2]).is_empty());
        assert!(d.push(&bytes[2..6]).is_empty());
        let events = d.push(&bytes[6..]);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn decodes_several_frames_from_one_chunk() {
        let mut bytes = frame(&[
            ("event_type", "intermediate".into()),
            ("step_ix", 1.into()),
            ("image", bin(b"a")),
        ]);
        bytes.extend(frame(&[
            ("event_type", "final".into()),
            ("image", bin(b"b")),
        ]));
        let mut d = StreamDecoder::new();
        assert_eq!(d.push(&bytes).len(), 2);
    }

    #[test]
    fn keeps_the_sample_index_so_a_batch_can_be_reassembled() {
        let mut d = StreamDecoder::new();
        let mut bytes = frame(&[
            ("event_type", "final".into()),
            ("samp_ix", 1.into()),
            ("image", bin(b"second")),
        ]);
        bytes.extend(frame(&[
            ("event_type", "final".into()),
            ("samp_ix", 0.into()),
            ("image", bin(b"first")),
        ]));
        let events = d.push(&bytes);
        assert_eq!(
            events,
            vec![
                StreamEvent::Final {
                    image: b"second".to_vec(),
                    sample: 1,
                },
                StreamEvent::Final {
                    image: b"first".to_vec(),
                    sample: 0,
                },
            ]
        );
    }

    #[test]
    fn a_malformed_frame_is_skipped_not_fatal() {
        // A frame whose body is not msgpack at all must not cost the user the
        // final image that follows it.
        let mut bytes = 3u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(&[0xc1, 0xc1, 0xc1]);
        bytes.extend(frame(&[
            ("event_type", "final".into()),
            ("image", bin(b"done")),
        ]));
        let mut d = StreamDecoder::new();
        assert_eq!(
            d.push(&bytes),
            vec![StreamEvent::Final {
                image: b"done".to_vec(),
                sample: 0,
            }]
        );
    }

    #[test]
    fn an_absurd_length_prefix_fails_loudly_instead_of_hanging() {
        let mut d = StreamDecoder::new();
        let events = d.push(&[0xff, 0xff, 0xff, 0xff]);
        assert!(matches!(events.as_slice(), [StreamEvent::Error { .. }]));
        // Poisoned, so the caller is not told the same thing on every chunk.
        assert!(d.push(&[0x00, 0x00, 0x00, 0x01, 0x90]).is_empty());
    }

    #[test]
    fn error_frames_surface_their_message() {
        let mut d = StreamDecoder::new();
        let events = d.push(&frame(&[
            ("event_type", "error".into()),
            ("message", "out of Anlas".into()),
        ]));
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
    fn a_real_opus_response_decodes_every_named_field() {
        // Captured from a live account, with the balances rounded off. Guards
        // the field names the Opus usage bar reads.
        let sub: Subscription = serde_json::from_str(
            r#"{"tier":3,"active":true,"paymentProcessor":"chargebee","expiresAt":1789956770,
                "perks":{"maxPriorityActions":1000,"contextTokens":8192},
                "paymentProcessorData":null,
                "trainingStepsLeft":{"fixedTrainingStepsLeft":4237,"purchasedTrainingSteps":2},
                "accountType":0,"isGracePeriod":false,"isPaypal":false,
                "usage":{"percent":69,"isNegative":false,"timeUntilNextPercent":7888}}"#,
        )
        .unwrap();
        assert!(sub.is_opus());
        assert_eq!(sub.anlas(), 4239);
        assert_eq!(sub.expires_at, Some(1789956770));
        let usage = sub.usage.expect("usage");
        assert_eq!(usage.percent, 69);
        assert!(!usage.is_negative);
        assert_eq!(usage.time_until_next_percent, 7888);
    }

    fn opus_with(percent: i64, is_negative: bool, seconds: i64) -> Subscription {
        let mut sub = Subscription {
            tier: 3,
            active: true,
            usage: Some(Usage {
                percent,
                is_negative,
                time_until_next_percent: seconds,
            }),
            ..Default::default()
        };
        sub.derive_opus_allowance();
        sub
    }

    #[test]
    fn the_allowance_bar_matches_the_websites_arithmetic() {
        // 69 percent remaining, one point back every 7888 seconds.
        let a = opus_with(69, false, 7888).opus_allowance.unwrap();
        assert_eq!(a.percent, 69);
        assert_eq!(a.bar_percent, 69);
        assert!(!a.is_bonus);
        assert_eq!(a.approx_images, 1194);
        assert!(!a.is_low && !a.is_empty);
        assert_eq!(a.refill_percent_per_day, 11.0);
        assert_eq!(a.refill_images_per_day, 190);
        assert_eq!(a.seconds_until_next_percent, 7888);
    }

    #[test]
    fn a_nearly_spent_allowance_reads_as_low() {
        assert!(opus_with(4, false, 7888).opus_allowance.unwrap().is_low);
        assert!(!opus_with(5, false, 7888).opus_allowance.unwrap().is_low);
    }

    #[test]
    fn a_negative_balance_draws_an_empty_bar_not_a_negative_one() {
        let a = opus_with(-12, true, 7888).opus_allowance.unwrap();
        assert_eq!(a.percent, 0);
        assert_eq!(a.bar_percent, 0);
        assert_eq!(a.approx_images, 0);
        assert!(a.is_empty && a.is_low);
    }

    #[test]
    fn a_bonus_grant_reads_past_a_full_bar() {
        // Anlatan doubled the allowance once already, so 200 percent is real.
        let a = opus_with(200, false, 7888).opus_allowance.unwrap();
        assert_eq!(a.percent, 200);
        assert_eq!(a.bar_percent, 100, "the bar itself never overflows");
        assert!(a.is_bonus);
        assert_eq!(a.approx_images, 3460);
        assert!(!a.is_low && !a.is_empty);

        // Exactly full is not a bonus, and still fills the bar.
        let a = opus_with(100, false, 7888).opus_allowance.unwrap();
        assert_eq!(a.bar_percent, 100);
        assert!(!a.is_bonus);
    }

    #[test]
    fn a_refill_time_of_zero_does_not_divide_by_zero() {
        let a = opus_with(50, false, 0).opus_allowance.unwrap();
        assert_eq!(a.refill_percent_per_day, 0.0);
        assert_eq!(a.refill_images_per_day, 0);
    }

    #[test]
    fn only_active_opus_accounts_get_an_allowance() {
        // The same usage payload on a lower tier draws no bar, as on the site.
        let mut sub = opus_with(69, false, 7888);
        sub.tier = 1;
        sub.derive_opus_allowance();
        assert!(sub.opus_allowance.is_none());

        let mut sub = opus_with(69, false, 7888);
        sub.active = false;
        sub.derive_opus_allowance();
        assert!(sub.opus_allowance.is_none());
    }

    #[test]
    fn a_response_without_usage_still_decodes() {
        // Free and lower tiers have no allowance bar at all.
        let sub: Subscription = serde_json::from_str(r#"{"tier":0,"active":false}"#).unwrap();
        assert!(sub.usage.is_none());
    }

    #[test]
    fn an_expired_opus_tier_is_not_opus() {
        let sub: Subscription = serde_json::from_str(r#"{"tier":3,"active":false}"#).unwrap();
        assert!(!sub.is_opus());
        assert_eq!(sub.anlas(), 0);
    }
}
