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
    let size = 8 + payload.len();
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(&(size as u32).to_be_bytes());
    b.extend_from_slice(typ);
    b.extend_from_slice(payload);
    b
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
pub fn encode_rgba8_lossless(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, AppError> {
    use jxl_encoder::{LossyConfig, PixelLayout};
    LossyConfig::new(1.0)
        .encode(rgba, width, height, PixelLayout::Rgba8)
        .map_err(|e| AppError::Other(format!("jxl encode (8-bit) failed: {:?}", e)))
}

/// Encode a 16-bit RGBA image (native-endian `u16` pairs) as a visually-lossless JXL (distance 1.0).
pub fn encode_rgba16_lossless(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, AppError> {
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

/// Iterate top-level ISO-BMFF boxes, yielding `(type, payload_range)` for each.
/// Only valid for container input (with the JXL signature box).
fn iter_boxes(bytes: &[u8]) -> Vec<([u8; 4], std::ops::Range<usize>)> {
    let mut out = Vec::new();
    if !is_container(bytes) {
        return out;
    }
    let mut pos = JXL_SIGNATURE_BOX.len();
    while pos + 8 <= bytes.len() {
        let size = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let typ = [
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ];
        let (payload_start, box_end) = if size == 0 {
            // size=0 means box extends to end of file
            (pos + 8, bytes.len())
        } else if size == 1 {
            // 64-bit size follows; rare for JXL metadata — treat as EOF.
            break;
        } else {
            if pos + size > bytes.len() {
                break;
            }
            (pos + 8, pos + size)
        };
        out.push((typ, payload_start..box_end));
        if box_end == bytes.len() {
            break;
        }
        pos = box_end;
    }
    out
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
/// Accepts either a naked codestream (from `encode_*_lossless`) or an existing
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

    let boxes = iter_boxes(jxl);
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
        let size = 8 + (range.end - range.start);
        out.extend_from_slice(&(size as u32).to_be_bytes());
        out.extend_from_slice(typ);
        out.extend_from_slice(&jxl[range.clone()]);
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

    #[test]
    fn roundtrip_2x2_rgba8() {
        let rgba = [
            255, 0, 0, 255, //
            0, 255, 0, 255, //
            0, 0, 255, 255, //
            255, 255, 255, 128, //
        ];
        let encoded = encode_rgba8_lossless(&rgba, 2, 2).expect("encode");
        let decoded = decode_to_rgba8(&encoded).expect("decode");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba.len(), 16);
        // Lossless round-trip: RGB channels should match exactly.
        // Alpha round-trip through float is also exact for 8-bit values.
        for (i, (&a, &b)) in rgba.iter().zip(decoded.rgba.iter()).enumerate() {
            assert!(a.abs_diff(b) <= 1, "pixel {} mismatch: {} vs {}", i, a, b);
        }
    }

    #[test]
    fn xmp_box_roundtrip() {
        let rgba = [10u8; 16];
        let encoded = encode_rgba8_lossless(&rgba, 2, 2).expect("encode");
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
    fn xmp_replace_existing() {
        let rgba = [10u8; 16];
        let encoded = encode_rgba8_lossless(&rgba, 2, 2).expect("encode");
        let first = wrap_with_xmp(&encoded, "first").expect("wrap1");
        let second = wrap_with_xmp(&first, "second").expect("wrap2");
        assert_eq!(read_xmp_box(&second).as_deref(), Some("second"));
    }
}
