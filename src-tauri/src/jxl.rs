//! JPEG XL (JXL) encode/decode helpers and ISO-BMFF container utilities.
//!
//! Uses `jxl-encoder` for pure-Rust visually-lossless encoding (distance 1.0)
//! and `jxl-oxide` for decoding. Metadata is stored in an `xml ` box inside
//! the JXL container (ISO-BMFF), carrying the same SwarmUI-compatible JSON
//! strings used by the PNG pipeline.

use crate::error::AppError;

const JXL_SIGNATURE_BOX: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
];

/// A canonical `ftyp` box declaring `jxl ` as the major brand.
fn ftyp_box() -> Vec<u8> {
    let mut b = Vec::with_capacity(20);
    b.extend_from_slice(&20u32.to_be_bytes());
    b.extend_from_slice(b"ftyp");
    b.extend_from_slice(b"jxl ");
    b.extend_from_slice(&0u32.to_be_bytes());
    b.extend_from_slice(b"jxl ");
    b
}

/// Build an ISO-BMFF box with the given 4-byte type and payload.
fn make_box(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(payload.len() + 16);
    push_box(&mut b, typ, payload);
    b
}

/// Append one ISO-BMFF box to `out`. A payload too large for the 32-bit size
/// field gets the 64-bit `largesize` form instead of a truncated header.
fn push_box(out: &mut Vec<u8>, typ: &[u8; 4], payload: &[u8]) {
    match u32::try_from(8 + payload.len()) {
        Ok(size) => {
            out.extend_from_slice(&size.to_be_bytes());
            out.extend_from_slice(typ);
        }
        Err(_) => {
            out.extend_from_slice(&1u32.to_be_bytes());
            out.extend_from_slice(typ);
            out.extend_from_slice(&(16 + payload.len() as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
}

/// True if the bytes appear to be a JXL container (ISO-BMFF), false for a
/// naked codestream.
fn is_container(bytes: &[u8]) -> bool {
    bytes.starts_with(&JXL_SIGNATURE_BOX)
}

/// True if the bytes appear to be a naked JXL codestream.
fn is_codestream(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0x0A
}

/// Encode an 8-bit RGBA image as a visually-lossless JXL (distance 1.0).
///
/// Visually lossless, **not** mathematically lossless: decoded pixels come back
/// close to the originals, not byte-identical. Do not rely on byte equality.
pub fn encode_rgba8_visually_lossless(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, AppError> {
    use jxl_encoder::{LossyConfig, PixelLayout};
    LossyConfig::new(1.0)
        .encode(rgba, width, height, PixelLayout::Rgba8)
        .map_err(|e| AppError::Other(format!("jxl encode (8-bit) failed: {:?}", e)))
}

/// Encode a 16-bit RGBA image (native-endian `u16` pairs) as a visually-lossless JXL (distance 1.0).
///
/// Visually lossless, **not** mathematically lossless: decoded pixels come back
/// close to the originals, not byte-identical. Do not rely on byte equality.
pub fn encode_rgba16_visually_lossless(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, AppError> {
    use jxl_encoder::{LossyConfig, PixelLayout};
    // jxl-encoder expects &[u8] with native-endian u16 pairs — same layout Python sends
    LossyConfig::new(1.0)
        .encode(rgba, width, height, PixelLayout::Rgba16)
        .map_err(|e| AppError::Other(format!("jxl encode (16-bit) failed: {:?}", e)))
}

/// Encode an 8-bit RGBA image as a lossless PNG.
/// Used for Tauri desktop mode where JXL can't be decoded by WebView2.
pub fn encode_rgba8_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, AppError> {
    let img =
        image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| AppError::Other("Invalid RGBA8 dimensions for PNG encode".into()))?;
    let mut buf = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut buf),
        img.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(buf)
}

/// Encode a 16-bit RGBA image (native-endian `u16` bytes) as a lossless PNG.
/// Used for Tauri desktop mode where JXL can't be decoded by WebView2.
pub fn encode_rgba16_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, AppError> {
    // Python sends native-endian uint16 bytes (little-endian on x86/x64).
    let pixels_u16: Vec<u16> = rgba
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect();
    let img = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(width, height, pixels_u16)
        .ok_or_else(|| AppError::Other("Invalid RGBA16 dimensions for PNG encode".into()))?;
    let mut buf = Vec::new();
    // image crate stores u16 in native endian; PNG requires big-endian.
    // write_to handles the byte-swap internally.
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(buf)
}

/// `(type, payload_range)` for each top-level box, in file order.
type BoxList = Vec<([u8; 4], std::ops::Range<usize>)>;

/// Iterate top-level ISO-BMFF boxes, yielding `(type, payload_range)` for each.
/// Only valid for container input (with the JXL signature box).
fn iter_boxes(bytes: &[u8]) -> BoxList {
    walk_boxes(bytes).0
}

/// Walk the top-level boxes of a JXL container.
///
/// Returns the boxes that parsed plus whether the walk stopped at a malformed
/// header. The file is untrusted (anything dropped on the window reaches
/// here), so every size is checked before it becomes a range: a box smaller
/// than its own header, or one claiming to run past the end, ends the walk
/// instead of producing an inverted or out-of-bounds range.
fn walk_boxes(bytes: &[u8]) -> (BoxList, bool) {
    let mut out = Vec::new();
    if !is_container(bytes) {
        return (out, false);
    }
    let mut pos = JXL_SIGNATURE_BOX.len();
    while bytes.len() - pos >= 8 {
        let size = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let typ = [
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ];
        // (header length, total box length); size=0 means the box extends to
        // the end of the file.
        let (header, len) = match size {
            0 => (8, bytes.len() - pos),
            // size=1 means a 64-bit `largesize` follows the type field; the box
            // (including its 16-byte header) spans `largesize` bytes from `pos`.
            1 => {
                let Some(large) = bytes.get(pos + 8..pos + 16) else {
                    return (out, true);
                };
                let mut be = [0u8; 8];
                be.copy_from_slice(large);
                let Ok(large) = usize::try_from(u64::from_be_bytes(be)) else {
                    return (out, true);
                };
                (16, large)
            }
            n => (8, n),
        };
        // A box must cover its own header (sizes 2..=7 used to yield an
        // inverted payload range) and must not run past the end of the file.
        let box_end = match pos.checked_add(len) {
            Some(end) if len >= header && end <= bytes.len() => end,
            _ => return (out, true),
        };
        out.push((typ, pos + header..box_end));
        pos = box_end;
    }
    (out, false)
}

/// Read the first `xml ` (XMP) box from a JXL container, returning its UTF-8
/// payload if present.
pub fn read_xmp_box(jxl: &[u8]) -> Option<String> {
    for (typ, range) in iter_boxes(jxl) {
        if &typ == b"xml " {
            if let Ok(s) = std::str::from_utf8(&jxl[range]) {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Return a JXL container with the given XMP string embedded in an `xml ` box.
///
/// Accepts either a naked codestream (from `encode_*_visually_lossless`) or an existing
/// container. In the container case, any existing `xml ` boxes are replaced
/// with the new one; all other boxes are preserved in original order, and the
/// `xml ` box is placed before the codestream (`jxlc`/`jxlp`) box.
pub fn wrap_with_xmp(jxl: &[u8], xmp: &str) -> Result<Vec<u8>, AppError> {
    let xml_box = make_box(b"xml ", xmp.as_bytes());

    if is_codestream(jxl) {
        let mut out = Vec::with_capacity(jxl.len() + xmp.len() + 64);
        out.extend_from_slice(&JXL_SIGNATURE_BOX);
        out.extend_from_slice(&ftyp_box());
        out.extend_from_slice(&xml_box);
        out.extend_from_slice(&make_box(b"jxlc", jxl));
        return Ok(out);
    }

    if !is_container(jxl) {
        return Err(AppError::Other(
            "wrap_with_xmp: input is neither a JXL container nor a naked codestream".into(),
        ));
    }

    let (boxes, malformed) = walk_boxes(jxl);
    if malformed {
        // Rebuilding from the parsed prefix would silently drop every box
        // after the bad header, the codestream included.
        return Err(AppError::Other(
            "wrap_with_xmp: JXL container has a malformed box".into(),
        ));
    }
    let mut out = Vec::with_capacity(jxl.len() + xmp.len() + 16);
    out.extend_from_slice(&JXL_SIGNATURE_BOX);

    let mut xml_inserted = false;
    for (typ, range) in &boxes {
        if typ == b"xml " {
            // Drop existing XMP box; we will re-insert a single fresh one.
            continue;
        }
        if !xml_inserted && (typ == b"jxlc" || typ == b"jxlp") {
            out.extend_from_slice(&xml_box);
            xml_inserted = true;
        }
        // Recreate the box with its original payload.
        push_box(&mut out, typ, &jxl[range.clone()]);
    }
    if !xml_inserted {
        // No codestream box found (shouldn't happen for a valid file) —
        // append XMP at the end as a best-effort.
        out.extend_from_slice(&xml_box);
    }
    Ok(out)
}

/// Decoded image with 8-bit RGBA pixels in row-major order.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decode a JXL (container or codestream) into 8-bit RGBA pixels.
pub fn decode_to_rgba8(jxl: &[u8]) -> Result<DecodedImage, AppError> {
    use jxl_oxide::{JxlImage, PixelFormat};

    let image = JxlImage::builder()
        .read(std::io::Cursor::new(jxl))
        .map_err(|e| AppError::Other(format!("jxl decode (open): {}", e)))?;
    let width = image.width();
    let height = image.height();
    let render = image
        .render_frame(0)
        .map_err(|e| AppError::Other(format!("jxl decode (render): {}", e)))?;
    let stream = render.stream();
    let channels = stream.channels() as usize;
    let mut buf = vec![0f32; (width as usize) * (height as usize) * channels];
    let mut stream = stream;
    stream.write_to_buffer(&mut buf);

    let total_px = (width as usize) * (height as usize);
    let mut rgba = Vec::with_capacity(total_px * 4);
    match image.pixel_format() {
        PixelFormat::Rgba => {
            debug_assert_eq!(channels, 4);
            for px in buf.chunks_exact(4) {
                rgba.push(clamp_to_u8(px[0]));
                rgba.push(clamp_to_u8(px[1]));
                rgba.push(clamp_to_u8(px[2]));
                rgba.push(clamp_to_u8(px[3]));
            }
        }
        PixelFormat::Rgb => {
            debug_assert_eq!(channels, 3);
            for px in buf.chunks_exact(3) {
                rgba.push(clamp_to_u8(px[0]));
                rgba.push(clamp_to_u8(px[1]));
                rgba.push(clamp_to_u8(px[2]));
                rgba.push(255);
            }
        }
        PixelFormat::Graya => {
            debug_assert_eq!(channels, 2);
            for px in buf.chunks_exact(2) {
                let g = clamp_to_u8(px[0]);
                rgba.push(g);
                rgba.push(g);
                rgba.push(g);
                rgba.push(clamp_to_u8(px[1]));
            }
        }
        PixelFormat::Gray => {
            debug_assert_eq!(channels, 1);
            for px in buf.iter() {
                let g = clamp_to_u8(*px);
                rgba.push(g);
                rgba.push(g);
                rgba.push(g);
                rgba.push(255);
            }
        }
        other => {
            return Err(AppError::Other(format!(
                "jxl decode: unsupported pixel format {:?}",
                other
            )));
        }
    }

    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

#[inline]
fn clamp_to_u8(v: f32) -> u8 {
    let scaled = (v * 255.0 + 0.5).clamp(0.0, 255.0);
    scaled as u8
}

/// Encode raw RGBA pixels directly to WebP (for WebView2 display).
///
/// `pixels` may be 8-bit (`is_16 = false`) or native-endian (little-endian on
/// Windows x64) 16-bit-per-channel (`is_16 = true`). 16-bit channels are
/// downsampled to 8-bit by taking the high byte (byte index 1 in LE layout).
pub fn encode_rgba8_webp_from_raw(
    pixels: &[u8],
    width: u32,
    height: u32,
    is_16: bool,
) -> Result<Vec<u8>, AppError> {
    let rgba8: Vec<u8> = if is_16 {
        // Python sends native-endian uint16 (little-endian on Windows/x64).
        // In LE layout: byte[0]=low, byte[1]=high.  Take the high byte for
        // an 8-bit approximation, just as jxl-oxide's clamp_to_u8 would.
        pixels.chunks_exact(2).map(|pair| pair[1]).collect()
    } else {
        pixels.to_vec()
    };

    let img = image::RgbaImage::from_raw(width, height, rgba8)
        .ok_or_else(|| AppError::Other("WebP encode: pixel buffer size mismatch".to_string()))?;
    let dyn_img = image::DynamicImage::ImageRgba8(img);
    let mut buf = std::io::Cursor::new(Vec::new());
    dyn_img
        .write_to(&mut buf, image::ImageFormat::WebP)
        .map_err(|e| AppError::Other(format!("WebP encode failed: {}", e)))?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `width` x `height` RGBA8 smooth gradient — a stand-in for the
    /// kind of image the gallery actually stores.
    fn gradient_rgba8(width: u32, height: u32) -> Vec<u8> {
        let mut px = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                px.push((x * 4) as u8);
                px.push((y * 4) as u8);
                px.push(((x + y) * 2) as u8);
                px.push(255);
            }
        }
        px
    }

    /// Max and mean absolute per-byte difference between two equal-length buffers.
    fn abs_error(a: &[u8], b: &[u8]) -> (u32, f64) {
        assert_eq!(a.len(), b.len(), "buffers must be the same length");
        let max = a
            .iter()
            .zip(b)
            .map(|(&x, &y)| x.abs_diff(y) as u32)
            .max()
            .unwrap_or(0);
        let mean = a
            .iter()
            .zip(b)
            .map(|(&x, &y)| x.abs_diff(y) as f64)
            .sum::<f64>()
            / a.len() as f64;
        (max, mean)
    }

    #[test]
    fn roundtrip_2x2_preserves_shape_and_alpha() {
        let rgba = [
            255, 0, 0, 255, //
            0, 255, 0, 255, //
            0, 0, 255, 255, //
            255, 255, 255, 128, //
        ];
        let encoded = encode_rgba8_visually_lossless(&rgba, 2, 2).expect("encode");
        let decoded = decode_to_rgba8(&encoded).expect("decode");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba.len(), 16);
        // Alpha survives the float pipeline exactly for 8-bit values.
        for i in (3..16).step_by(4) {
            assert_eq!(rgba[i], decoded.rgba[i], "alpha at byte {} changed", i);
        }
        // Colour is deliberately not asserted here. A 2x2 of saturated primaries
        // gives the encoder no spatial context, so a distance-1.0 encode can move
        // a channel by ~96/255. Fidelity is pinned on realistic images below.
    }

    #[test]
    fn roundtrip_gradient_stays_within_visually_lossless_envelope() {
        let (w, h) = (64, 64);
        let rgba = gradient_rgba8(w, h);
        let encoded = encode_rgba8_visually_lossless(&rgba, w, h).expect("encode");
        let decoded = decode_to_rgba8(&encoded).expect("decode");
        assert_eq!(decoded.width, w);
        assert_eq!(decoded.height, h);
        assert_eq!(decoded.rgba.len(), rgba.len());

        let (max, mean) = abs_error(&rgba, &decoded.rgba);
        // Observed on jxl-encoder at distance 1.0: max 17, mean 0.553. These are
        // envelopes, not exact expectations — tighten only if the encoder improves.
        assert!(mean < 2.0, "mean abs error {mean:.3} exceeds envelope");
        assert!(max <= 32, "max abs error {max} exceeds envelope");
    }

    #[test]
    fn roundtrip_flat_image_is_effectively_exact() {
        let (w, h) = (64, 64);
        let rgba = vec![128u8; (w * h * 4) as usize];
        let encoded = encode_rgba8_visually_lossless(&rgba, w, h).expect("encode");
        let decoded = decode_to_rgba8(&encoded).expect("decode");

        let (max, _) = abs_error(&rgba, &decoded.rgba);
        // A constant image has nothing to lose: distance 1.0 reproduces it exactly.
        assert!(max <= 1, "flat image drifted by {max}");
    }

    #[test]
    fn xmp_box_roundtrip() {
        let rgba = [10u8; 16];
        let encoded = encode_rgba8_visually_lossless(&rgba, 2, 2).expect("encode");
        let xmp = r#"{"sui_image_params":"{\"prompt\":\"test\"}"}"#;
        let wrapped = wrap_with_xmp(&encoded, xmp).expect("wrap");
        assert!(is_container(&wrapped), "wrapped output must be container");
        let read_back = read_xmp_box(&wrapped).expect("xmp present");
        assert_eq!(read_back, xmp);
        // Still decodable after wrapping.
        let decoded = decode_to_rgba8(&wrapped).expect("decode wrapped");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
    }

    #[test]
    fn iter_boxes_parses_64bit_extended_size() {
        // A box with size==1 carries its real length in a trailing 64-bit
        // `largesize` field; the payload then starts after the 16-byte header.
        let payload = b"hello";
        let total: u64 = 16 + payload.len() as u64;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&JXL_SIGNATURE_BOX);
        bytes.extend_from_slice(&1u32.to_be_bytes()); // size = 1 (extended)
        bytes.extend_from_slice(b"xml "); // type
        bytes.extend_from_slice(&total.to_be_bytes()); // 64-bit largesize
        bytes.extend_from_slice(payload);

        let boxes = iter_boxes(&bytes);
        assert_eq!(boxes.len(), 1, "should parse one extended-size box");
        let (typ, range) = &boxes[0];
        assert_eq!(typ, b"xml ");
        assert_eq!(&bytes[range.clone()], payload);
    }

    /// The signature box followed by `boxes`, as raw bytes.
    fn container(boxes: &[&[u8]]) -> Vec<u8> {
        let mut out = JXL_SIGNATURE_BOX.to_vec();
        for b in boxes {
            out.extend_from_slice(b);
        }
        out
    }

    #[test]
    fn a_box_smaller_than_its_header_is_malformed_not_a_panic() {
        // Sizes 2..=7 used to produce the inverted range `pos+8..pos+size`,
        // which panicked when sliced.
        for size in 2u32..8 {
            let mut bad = size.to_be_bytes().to_vec();
            bad.extend_from_slice(b"xml ");
            bad.extend_from_slice(b"payload!");
            let bytes = container(&[&bad]);
            assert!(read_xmp_box(&bytes).is_none(), "size {size}");
            assert!(iter_boxes(&bytes).is_empty(), "size {size}");
            assert!(wrap_with_xmp(&bytes, "{}").is_err(), "size {size}");
        }
    }

    #[test]
    fn boxes_claiming_to_run_past_the_end_are_malformed() {
        let mut past = 400u32.to_be_bytes().to_vec();
        past.extend_from_slice(b"xml short");
        let bytes = container(&[&past]);
        assert!(read_xmp_box(&bytes).is_none());
        assert!(wrap_with_xmp(&bytes, "{}").is_err());

        // A 64-bit size near u64::MAX must not wrap around to a small end.
        let mut huge = 1u32.to_be_bytes().to_vec();
        huge.extend_from_slice(b"xml ");
        huge.extend_from_slice(&u64::MAX.to_be_bytes());
        huge.extend_from_slice(b"data");
        let bytes = container(&[&huge]);
        assert!(read_xmp_box(&bytes).is_none());
        assert!(wrap_with_xmp(&bytes, "{}").is_err());

        // A 64-bit size smaller than the 16-byte header it sits in.
        let mut tiny = 1u32.to_be_bytes().to_vec();
        tiny.extend_from_slice(b"xml ");
        tiny.extend_from_slice(&9u64.to_be_bytes());
        let bytes = container(&[&tiny]);
        assert!(iter_boxes(&bytes).is_empty());

        // A truncated 64-bit size field.
        let mut cut = 1u32.to_be_bytes().to_vec();
        cut.extend_from_slice(b"xml ");
        cut.extend_from_slice(&[0, 0, 0]);
        assert!(iter_boxes(&container(&[&cut])).is_empty());
    }

    #[test]
    fn a_malformed_box_after_good_ones_keeps_the_good_prefix_readable() {
        let good = make_box(b"xml ", b"kept");
        let mut bad = 3u32.to_be_bytes().to_vec();
        bad.extend_from_slice(b"jxlc");
        let bytes = container(&[&good, &bad]);
        assert_eq!(read_xmp_box(&bytes).as_deref(), Some("kept"));
        // Rewriting it would drop everything after the bad header.
        assert!(wrap_with_xmp(&bytes, "new").is_err());
    }

    #[test]
    fn a_zero_size_box_runs_to_the_end_and_a_short_tail_is_ignored() {
        let mut to_end = 0u32.to_be_bytes().to_vec();
        to_end.extend_from_slice(b"xml ");
        to_end.extend_from_slice(b"tail");
        assert_eq!(
            read_xmp_box(&container(&[&to_end])).as_deref(),
            Some("tail")
        );

        // Fewer than 8 trailing bytes cannot hold a header; they end the walk
        // without marking the file malformed.
        let good = make_box(b"xml ", b"ok");
        let bytes = container(&[&good, b"\0\0\0"]);
        assert_eq!(iter_boxes(&bytes).len(), 1);
        assert!(!walk_boxes(&bytes).1);
    }

    #[test]
    fn xmp_replace_existing() {
        let rgba = [10u8; 16];
        let encoded = encode_rgba8_visually_lossless(&rgba, 2, 2).expect("encode");
        let first = wrap_with_xmp(&encoded, "first").expect("wrap1");
        let second = wrap_with_xmp(&first, "second").expect("wrap2");
        assert_eq!(read_xmp_box(&second).as_deref(), Some("second"));
    }
}
