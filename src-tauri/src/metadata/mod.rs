use std::collections::HashMap;
use std::io::{Cursor, Read as _, Write as _};

mod gif;
mod isobmff;

/// How to embed metadata into a PNG image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataMode {
    /// Standard PNG tEXt chunk ("parameters"). Fast, no pixel modification.
    TextChunk,
    /// Stealth alpha-channel LSB encoding (SwarmUI-compatible). Survives re-uploads.
    StealthAlpha,
    /// Both text chunk and stealth alpha.
    Both,
}

impl MetadataMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "stealth" => Self::StealthAlpha,
            "both" => Self::Both,
            _ => Self::TextChunk,
        }
    }
}

pub fn is_png_16bit(image_bytes: &[u8]) -> Result<bool, String> {
    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let reader = decoder
        .read_info()
        .map_err(|e| format!("PNG decode error: {}", e))?;
    Ok(reader.info().bit_depth == png::BitDepth::Sixteen)
}

/// What image or video container a byte slice represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jxl,
    WebP,
    Mp4,
    Avif,
    Gif,
    Unknown,
}

/// Sniff the container format from the first few bytes.
pub fn detect_format(bytes: &[u8]) -> ImageFormat {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ImageFormat::Png
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        ImageFormat::WebP
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        ImageFormat::Gif
    } else if bytes.len() >= 12
        && bytes[0..12]
            == [
                0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
            ]
    {
        ImageFormat::Jxl
    } else if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0x0A {
        // Naked JXL codestream
        ImageFormat::Jxl
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        // ISO BMFF: size box then "ftyp" brand at offset 4. AVIF is also ISOBMFF,
        // so the brand list decides which of the two this is; without the check
        // every AVIF fell into the mp4 arm and read as having no metadata.
        if isobmff::is_avif(bytes) {
            ImageFormat::Avif
        } else {
            ImageFormat::Mp4
        }
    } else {
        ImageFormat::Unknown
    }
}

/// Embed SwarmUI-compatible metadata into a JXL file by writing an `xml ` box.
/// Accepts a naked codestream or a container and always returns a valid container.
pub fn embed_jxl_metadata(
    image_bytes: &[u8],
    params: &HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    let xmp = format_swarmui_json(params);
    crate::jxl::wrap_with_xmp(image_bytes, &xmp).map_err(|e| e.to_string())
}

/// Read MooshieUI/SwarmUI metadata from a JXL file's `xml ` box, if present.
pub fn read_jxl_metadata(image_bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    let Some(xmp) = crate::jxl::read_xmp_box(image_bytes) else {
        return Ok(None);
    };
    Ok(parse_swarmui_json(xmp.trim()))
}

/// Format-aware dispatcher: returns metadata for any container we can read.
pub fn read_image_metadata(bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    match detect_format(bytes) {
        ImageFormat::Png => read_png_metadata(bytes),
        ImageFormat::Jxl => read_jxl_metadata(bytes),
        ImageFormat::WebP => read_webp_metadata(bytes),
        ImageFormat::Mp4 => read_mp4_metadata(bytes),
        ImageFormat::Avif => read_avif_metadata(bytes),
        ImageFormat::Gif => read_gif_metadata(bytes),
        ImageFormat::Unknown => Ok(None),
    }
}

/// Read metadata from mp4 bytes.
///
/// The container-native `udta` comment is tried before the `uuid` XMP box on
/// purpose. We write both from one payload so they always agree, but a
/// third-party tool that edits metadata edits the container-native copy,
/// because that is what exiftool and ffmpeg touch, and leaves a stale `uuid`
/// behind. Canonical-first means someone else's edit wins over our sidecar.
pub fn read_mp4_metadata(bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    let text = isobmff::read_udta_comment(bytes).or_else(|| isobmff::read_uuid_xmp(bytes));
    Ok(text.and_then(|t| parse_swarmui_json(t.trim())))
}

/// Read metadata from AVIF bytes: the Exif item first, then the `uuid` box.
pub fn read_avif_metadata(bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    let text = isobmff::read_avif_exif(bytes)
        .as_deref()
        .and_then(read_exif_user_comment)
        .or_else(|| isobmff::read_uuid_xmp(bytes));
    Ok(text.and_then(|t| parse_swarmui_json(t.trim())))
}

/// Read metadata from a GIF Comment Extension. GIF has no `uuid` equivalent.
pub fn read_gif_metadata(bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    Ok(gif::read_comment(bytes).and_then(|t| parse_swarmui_json(t.trim())))
}

/// Copy an mp4 or avif file's container-native metadata into a top-level Adobe
/// XMP `uuid` box, rewriting the file in place. Returns whether anything was
/// written.
///
/// This is the second half of the two-carrier split: Python writes the
/// container-native copy because it holds the frames, and this mirrors it so
/// the payload also lives somewhere Discord's `moov/udta` scrubber does not
/// reach. Reading the payload back out rather than passing it in is what keeps
/// generation parameters from having to thread through the websocket layer.
///
/// Best-effort throughout. Every failure is a `false`, never an error: a video
/// with no `uuid` mirror is still a perfectly good video.
pub fn mirror_uuid_sidecar(path: &std::path::Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let json = match detect_format(&bytes) {
        ImageFormat::Mp4 => isobmff::read_udta_comment(&bytes),
        ImageFormat::Avif => isobmff::read_avif_exif(&bytes)
            .as_deref()
            .and_then(read_exif_user_comment),
        _ => None,
    };
    let Some(json) = json else {
        return false;
    };
    // Already mirrored, by us or by an earlier pass over the same file.
    if isobmff::read_uuid_xmp(&bytes).as_deref() == Some(json.as_str()) {
        return false;
    }
    let Some(out) = isobmff::append_uuid_xmp(&bytes, &json) else {
        return false;
    };
    // Swap rather than overwrite: `fs::write` truncates the video before it
    // writes a byte, so an interrupted write would leave the gallery holding a
    // ruined file. The temp name is derived from the target so it lands in the
    // same directory, which keeps the rename on one volume and atomic.
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".uuidtmp");
    let tmp = std::path::PathBuf::from(tmp);
    if std::fs::write(&tmp, out).is_err() {
        std::fs::remove_file(&tmp).ok();
        return false;
    }
    if std::fs::rename(&tmp, path).is_err() {
        std::fs::remove_file(&tmp).ok();
        return false;
    }
    true
}

/// Append a `uuid` XMP box to ISOBMFF bytes. Test-only: production code reaches
/// the same writer through `mirror_uuid_sidecar`, which reads the payload out of
/// the file rather than taking it from a caller.
#[cfg(test)]
pub fn embed_uuid_for_test(bytes: &[u8], json: &str) -> Vec<u8> {
    isobmff::append_uuid_xmp(bytes, json).expect("test fixture is walkable ISOBMFF")
}

/// Format-aware dispatcher: embeds metadata into PNG, JXL, or WebP bytes and
/// returns the result in the **same** container format, so callers exporting or
/// copying raw bytes never have to transcode just to attach metadata.
///
/// JXL carries metadata in an `xml ` box and has no stealth-alpha variant, so
/// `mode` is ignored there.
pub fn embed_image_metadata(
    image_bytes: &[u8],
    params: &HashMap<String, String>,
    mode: MetadataMode,
) -> Result<Vec<u8>, String> {
    match detect_format(image_bytes) {
        ImageFormat::Png => embed_png_metadata(image_bytes, params, mode),
        ImageFormat::Jxl => embed_jxl_metadata(image_bytes, params),
        ImageFormat::WebP => embed_webp_metadata(image_bytes, params, mode),
        ImageFormat::Mp4 => Err("Metadata embedding is not supported for mp4 video".to_string()),
        ImageFormat::Avif => Err("Metadata embedding is not supported for avif".to_string()),
        ImageFormat::Gif => Err("Metadata embedding is not supported for gif".to_string()),
        ImageFormat::Unknown => Err("Unsupported image format for metadata embedding".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Attach the SwarmUI JSON under the `parameters` keyword.
///
/// `tEXt` is what A1111, SwarmUI and every downstream reader expect, but the PNG
/// spec restricts it to ISO 8859-1: a single CJK character or emoji in the prompt
/// makes `write_header` fail, and callers then save the image with **no** metadata
/// at all. `iTXt` is the spec's UTF-8 chunk type (same keyword, read by PIL /
/// ImageSharp / exiftool), so non-Latin-1 payloads fall back to it instead of
/// being dropped.
fn add_parameters_chunk<W: std::io::Write>(
    encoder: &mut png::Encoder<'_, W>,
    json_text: String,
) -> Result<(), String> {
    // Mirrors the png crate's own Latin-1 predicate (`u8::try_from(c as u32)`).
    if json_text.chars().all(|c| (c as u32) < 0x100) {
        encoder
            .add_text_chunk("parameters".to_string(), json_text)
            .map_err(|e| format!("Failed to add text chunk: {}", e))
    } else {
        encoder
            .add_itxt_chunk("parameters".to_string(), json_text)
            .map_err(|e| format!("Failed to add iTXt chunk: {}", e))
    }
}

/// Embed metadata into PNG image bytes using the specified mode.
pub fn embed_png_metadata(
    image_bytes: &[u8],
    params: &HashMap<String, String>,
    mode: MetadataMode,
) -> Result<Vec<u8>, String> {
    let json_text = format_swarmui_json(params);

    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG decode error: {}", e))?;
    let info = reader.info().clone();

    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .expect("PNG output buffer size unavailable")
    ];
    let output_info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame read error: {}", e))?;
    buf.truncate(output_info.buffer_size());

    // If stealth alpha is requested, embed bits into pixel data.
    // If stealth fails (e.g. unsupported color type), fall back to text_chunk only.
    let is_16bit = info.bit_depth == png::BitDepth::Sixteen;
    let (pixel_buf, color_type, effective_mode) = if mode == MetadataMode::StealthAlpha
        || mode == MetadataMode::Both
    {
        let stealth_result = if is_16bit {
            to_rgba16(&buf, info.color_type, info.width, info.height).and_then(
                |(mut rgba16, w, h)| {
                    encode_stealth_alpha(&mut rgba16, w, h, 8, &json_text)?;
                    Ok((rgba16, w, h))
                },
            )
        } else {
            to_rgba8(&buf, info.color_type, info.width, info.height).and_then(|(mut rgba, w, h)| {
                encode_stealth_alpha(&mut rgba, w, h, 4, &json_text)?;
                Ok((rgba, w, h))
            })
        };
        match stealth_result {
            Ok((pixels, _w, _h)) => (pixels, png::ColorType::Rgba, mode),
            Err(e) => {
                log::warn!(
                    "Stealth alpha encoding failed ({}), falling back to text_chunk only",
                    e
                );
                (buf, info.color_type, MetadataMode::TextChunk)
            }
        }
    } else {
        (buf, info.color_type, mode)
    };

    // Re-encode PNG
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, info.width, info.height);
        encoder.set_color(color_type);
        encoder.set_depth(info.bit_depth);
        if let Some(srgb) = info.srgb {
            encoder.set_source_srgb(srgb);
        }

        if effective_mode == MetadataMode::TextChunk || effective_mode == MetadataMode::Both {
            add_parameters_chunk(&mut encoder, json_text)?;
        }

        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG encode error: {}", e))?;
        writer
            .write_image_data(&pixel_buf)
            .map_err(|e| format!("PNG write error: {}", e))?;
    }

    Ok(output)
}

/// Encode raw 8-bit RGBA pixels as a PNG with embedded metadata in a single
/// pass. Fast path for callers that already hold decoded pixels (e.g. after a
/// JXL decode), avoiding the decode/re-encode round-trip of
/// [`embed_png_metadata`]. 8-bit only — the JXL decoder always yields RGBA8.
pub fn encode_png_with_metadata_rgba8(
    rgba: &[u8],
    width: u32,
    height: u32,
    params: &HashMap<String, String>,
    mode: MetadataMode,
) -> Result<Vec<u8>, String> {
    let json_text = format_swarmui_json(params);

    let mut pixel_buf = rgba.to_vec();
    let effective_mode = if mode == MetadataMode::StealthAlpha || mode == MetadataMode::Both {
        match encode_stealth_alpha(&mut pixel_buf, width, height, 4, &json_text) {
            Ok(()) => mode,
            Err(e) => {
                log::warn!(
                    "Stealth alpha encoding failed ({}), falling back to text_chunk only",
                    e
                );
                pixel_buf.copy_from_slice(rgba);
                MetadataMode::TextChunk
            }
        }
    } else {
        mode
    };

    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        if effective_mode == MetadataMode::TextChunk || effective_mode == MetadataMode::Both {
            add_parameters_chunk(&mut encoder, json_text)?;
        }

        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG encode error: {}", e))?;
        writer
            .write_image_data(&pixel_buf)
            .map_err(|e| format!("PNG write error: {}", e))?;
    }

    Ok(output)
}

/// Read metadata from PNG bytes.
/// Tries stealth alpha first, then PNG text chunks (SwarmUI JSON → A1111 fallback).
pub fn read_png_metadata(image_bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    // Try stealth alpha first
    if let Ok(Some(params)) = read_stealth_alpha(image_bytes) {
        return Ok(Some(params));
    }

    // Fall back to text chunks
    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let reader = decoder
        .read_info()
        .map_err(|e| format!("PNG decode error: {}", e))?;
    let info = reader.info();

    // tEXt first (what A1111 and our own Latin-1 writes use), then iTXt/zTXt —
    // where non-Latin-1 prompts and other tools' UTF-8 metadata live.
    let raw_text = info
        .uncompressed_latin1_text
        .iter()
        .find(|c| c.keyword == "parameters")
        .map(|c| c.text.clone())
        .or_else(|| {
            info.utf8_text
                .iter()
                .find(|c| c.keyword == "parameters")
                .and_then(|c| c.get_text().ok())
        })
        .or_else(|| {
            info.compressed_latin1_text
                .iter()
                .find(|c| c.keyword == "parameters")
                .and_then(|c| c.get_text().ok())
        });

    let Some(raw_text) = raw_text else {
        return Ok(None);
    };
    let text = raw_text.trim();
    if text.starts_with('{') {
        if let Some(parsed) = parse_swarmui_json(text) {
            return Ok(Some(parsed));
        }
    }
    Ok(Some(parse_a1111_params(text)))
}

// ---------------------------------------------------------------------------
// Stealth alpha encoding (SwarmUI-compatible)
// ---------------------------------------------------------------------------

const STEALTH_MAGIC: &[u8] = b"stealth_pngcomp";

/// Convert raw 8-bit pixel buffer to RGBA8 (adding alpha if needed).
fn to_rgba8(
    buf: &[u8],
    color_type: png::ColorType,
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let pixel_count = (width as usize) * (height as usize);
    match color_type {
        png::ColorType::Rgba => Ok((buf.to_vec(), width, height)),
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity(pixel_count * 4);
            for chunk in buf.chunks_exact(3) {
                rgba.extend_from_slice(chunk);
                rgba.push(255);
            }
            Ok((rgba, width, height))
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity(pixel_count * 4);
            for chunk in buf.chunks_exact(2) {
                let g = chunk[0];
                let a = chunk[1];
                rgba.extend_from_slice(&[g, g, g, a]);
            }
            Ok((rgba, width, height))
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity(pixel_count * 4);
            for &g in buf.iter() {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
            Ok((rgba, width, height))
        }
        _ => Err(format!("Unsupported color type: {:?}", color_type)),
    }
}

/// Convert raw 16-bit pixel buffer to RGBA16 (adding alpha if needed).
/// Each channel is 2 bytes big-endian as output by the PNG decoder.
fn to_rgba16(
    buf: &[u8],
    color_type: png::ColorType,
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let pixel_count = (width as usize) * (height as usize);
    match color_type {
        png::ColorType::Rgba => Ok((buf.to_vec(), width, height)),
        png::ColorType::Rgb => {
            // 6 bytes/pixel → 8 bytes/pixel (add alpha = 0xFFFF)
            let mut rgba = Vec::with_capacity(pixel_count * 8);
            for chunk in buf.chunks_exact(6) {
                rgba.extend_from_slice(chunk);
                rgba.extend_from_slice(&[0xFF, 0xFF]); // alpha = 65535 BE
            }
            Ok((rgba, width, height))
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity(pixel_count * 8);
            for chunk in buf.chunks_exact(4) {
                // G(2 bytes) + A(2 bytes) → R,G,B,A (each 2 bytes)
                rgba.extend_from_slice(&chunk[0..2]); // R = G
                rgba.extend_from_slice(&chunk[0..2]); // G = G
                rgba.extend_from_slice(&chunk[0..2]); // B = G
                rgba.extend_from_slice(&chunk[2..4]); // A
            }
            Ok((rgba, width, height))
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity(pixel_count * 8);
            for chunk in buf.chunks_exact(2) {
                rgba.extend_from_slice(chunk); // R
                rgba.extend_from_slice(chunk); // G
                rgba.extend_from_slice(chunk); // B
                rgba.extend_from_slice(&[0xFF, 0xFF]); // A = 65535
            }
            Ok((rgba, width, height))
        }
        _ => Err(format!(
            "Unsupported color type for 16-bit: {:?}",
            color_type
        )),
    }
}

/// Map a sequential bit index to the buffer offset of the alpha channel byte
/// that carries stealth data, using **column-major** pixel traversal (x outer,
/// y inner) as required by the stealth pnginfo format.
///
/// `bpp` = bytes per pixel (4 for 8-bit RGBA, 8 for 16-bit RGBA).
///
/// For 8-bit RGBA (bpp=4): the alpha byte is at pixel_start + 3.
/// For 16-bit RGBA (bpp=8): alpha occupies bytes 6–7 (big-endian).  We encode
/// into byte 6 (the **high** byte) because most readers — including PIL and
/// sd-webui-stealth-pnginfo — open 16-bit PNGs as 8-bit, mapping the high
/// byte directly to the 8-bit alpha.  Writing bit 0 of the low byte (byte 7)
/// would be invisible after that conversion.
#[inline]
fn colmajor_alpha_offset(bit_idx: usize, width: usize, height: usize, bpp: usize) -> usize {
    let x = bit_idx / height;
    let y = bit_idx % height;
    // 8-bit: bpp=4, 4 - 4/4 = 3 (the single alpha byte)
    // 16-bit: bpp=8, 8 - 8/4 = 6 (the alpha high byte)
    (y * width + x) * bpp + (bpp - bpp / 4)
}

/// Encode metadata into the alpha channel LSBs of an RGBA pixel buffer.
/// Works for both 8-bit (bpp=4) and 16-bit (bpp=8) RGBA.
/// Format: magic_bits(120) + length_bits(32) + gzip_data_bits
/// Pixel traversal: column-major (compatible with sd-webui-stealth-pnginfo).
fn encode_stealth_alpha(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    bpp: usize,
    json_text: &str,
) -> Result<(), String> {
    let w = width as usize;
    let h = height as usize;

    // Set all alpha to max first (matching the Python implementation)
    // For 8-bit: alpha byte = 0xFF. For 16-bit: alpha = 0xFFFF (two bytes).
    for i in 0..(w * h) {
        let alpha_start = i * bpp + (bpp - bpp / 4); // start of alpha channel bytes
        for b in 0..(bpp / 4) {
            rgba[alpha_start + b] = 0xFF;
        }
    }

    // GZip compress the JSON
    let compressed = gzip_compress(json_text.as_bytes())
        .map_err(|e| format!("GZip compression failed: {}", e))?;

    // Build bit stream: magic + 32-bit length + data
    let data_bits = compressed.len() * 8;
    let magic_bits = STEALTH_MAGIC.len() * 8;
    let total_bits = magic_bits + 32 + data_bits;

    let available_pixels = w * h;
    if total_bits > available_pixels {
        return Err(format!(
            "Image too small for stealth metadata: need {} pixels, have {}",
            total_bits, available_pixels
        ));
    }

    let mut bit_idx = 0usize;

    // Write magic header bits (MSB first per byte)
    for &byte in STEALTH_MAGIC {
        for bit_pos in (0..8).rev() {
            let bit = (byte >> bit_pos) & 1;
            let off = colmajor_alpha_offset(bit_idx, w, h, bpp);
            rgba[off] = (rgba[off] & 0xFE) | bit;
            bit_idx += 1;
        }
    }

    // Write 32-bit length (MSB first, value = length of compressed data in bits)
    let len_val = data_bits as u32;
    for bit_pos in (0..32).rev() {
        let bit = ((len_val >> bit_pos) & 1) as u8;
        let off = colmajor_alpha_offset(bit_idx, w, h, bpp);
        rgba[off] = (rgba[off] & 0xFE) | bit;
        bit_idx += 1;
    }

    // Write compressed data bits (MSB first per byte)
    for &byte in &compressed {
        for bit_pos in (0..8).rev() {
            let bit = (byte >> bit_pos) & 1;
            let off = colmajor_alpha_offset(bit_idx, w, h, bpp);
            rgba[off] = (rgba[off] & 0xFE) | bit;
            bit_idx += 1;
        }
    }

    Ok(())
}

/// Read stealth alpha metadata from PNG bytes.
/// Handles both 8-bit and 16-bit RGBA images.
/// Uses column-major pixel traversal to match the stealth pnginfo format.
fn read_stealth_alpha(image_bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG decode error: {}", e))?;
    let info = reader.info().clone();

    // Only RGBA images can have stealth alpha
    if info.color_type != png::ColorType::Rgba {
        return Ok(None);
    }

    let bpp: usize = if info.bit_depth == png::BitDepth::Sixteen {
        8
    } else {
        4
    };

    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .expect("PNG output buffer size unavailable")
    ];
    let output_info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame read error: {}", e))?;
    buf.truncate(output_info.buffer_size());

    decode_stealth_alpha_pixels(&buf, info.width as usize, info.height as usize, bpp)
}

/// Decode stealth-alpha metadata straight out of an interleaved RGBA pixel
/// buffer. Container-agnostic (PNG and WebP both funnel through here) — `bpp` is
/// bytes per pixel: 4 for RGBA8, 8 for RGBA16.
fn decode_stealth_alpha_pixels(
    buf: &[u8],
    w: usize,
    h: usize,
    bpp: usize,
) -> Result<Option<HashMap<String, String>>, String> {
    let pixel_count = w * h;
    let magic_bits = STEALTH_MAGIC.len() * 8;

    if pixel_count < magic_bits + 32 {
        return Ok(None);
    }

    // Read and verify magic header (column-major traversal, MSB first per byte)
    let mut magic_bytes = vec![0u8; STEALTH_MAGIC.len()];
    let mut bit_idx = 0usize;
    for mb in magic_bytes.iter_mut() {
        let mut byte_val = 0u8;
        for bit_pos in (0..8).rev() {
            let off = colmajor_alpha_offset(bit_idx, w, h, bpp);
            let bit = buf[off] & 1;
            byte_val |= bit << bit_pos;
            bit_idx += 1;
        }
        *mb = byte_val;
    }

    if magic_bytes != STEALTH_MAGIC {
        return Ok(None);
    }

    // Read 32-bit length
    let mut len_val = 0u32;
    for bit_pos in (0..32).rev() {
        let off = colmajor_alpha_offset(bit_idx, w, h, bpp);
        let bit = (buf[off] & 1) as u32;
        len_val |= bit << bit_pos;
        bit_idx += 1;
    }

    let data_bits = len_val as usize;
    if data_bits == 0 || !data_bits.is_multiple_of(8) {
        return Ok(None);
    }
    let data_bytes_len = data_bits / 8;

    if bit_idx + data_bits > pixel_count {
        return Ok(None);
    }

    // Read compressed data (column-major, MSB first per byte)
    let mut compressed = vec![0u8; data_bytes_len];
    for cb in compressed.iter_mut() {
        let mut byte_val = 0u8;
        for bit_pos in (0..8).rev() {
            let off = colmajor_alpha_offset(bit_idx, w, h, bpp);
            let bit = buf[off] & 1;
            byte_val |= bit << bit_pos;
            bit_idx += 1;
        }
        *cb = byte_val;
    }

    // Decompress
    let json_bytes = gzip_decompress(&compressed).map_err(|e| format!("GZip decompress: {}", e))?;
    let json_text = String::from_utf8(json_bytes).map_err(|e| format!("UTF-8 decode: {}", e))?;

    if let Some(params) = parse_swarmui_json(json_text.trim()) {
        Ok(Some(params))
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// WebP (RIFF) metadata
// ---------------------------------------------------------------------------
//
// WebP metadata parity with PNG/JXL needs two independent carriers:
//
//   * an `EXIF` RIFF chunk holding the SwarmUI JSON as the Exif sub-IFD
//     UserComment — the A1111/piexif convention, so exiftool, civitai and the
//     web UIs all read it;
//   * stealth alpha LSBs, which survive because every WebP this app writes is
//     VP8L (lossless) — `image`'s WebP encoder has no lossy mode, so pixels
//     round-trip bit-exactly.
//
// The RIFF surgery is hand-rolled rather than delegated to a crate so chunk
// ordering (VP8X, ICCP, image data, EXIF, XMP) is guaranteed to follow the spec
// and no new dependency is pulled in for ~100 lines of container edits.

/// EXIF-present flag in the VP8X feature byte.
const VP8X_FLAG_EXIF: u8 = 0x08;
/// Alpha-present flag in the VP8X feature byte.
const VP8X_FLAG_ALPHA: u8 = 0x10;

/// A single RIFF chunk: 4-byte FourCC plus its payload (padding excluded).
struct RiffChunk {
    id: [u8; 4],
    payload: Vec<u8>,
}

/// Split a WebP file into its RIFF chunks.
fn parse_webp_chunks(bytes: &[u8]) -> Result<Vec<RiffChunk>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err("Not a WebP (RIFF/WEBP) file".to_string());
    }
    // The RIFF size field covers everything after it. Clamp to the real buffer
    // length so a truncated or over-declared file still parses what it has.
    let riff_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let end = riff_size.saturating_add(8).min(bytes.len());

    let mut chunks = Vec::new();
    let mut pos = 12usize;
    while pos + 8 <= end {
        let mut id = [0u8; 4];
        id.copy_from_slice(&bytes[pos..pos + 4]);
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let data_start = pos + 8;
        let data_end = data_start
            .checked_add(size)
            .ok_or_else(|| "WebP chunk size overflow".to_string())?;
        if data_end > end {
            return Err(format!(
                "Truncated WebP chunk {}",
                String::from_utf8_lossy(&id)
            ));
        }
        chunks.push(RiffChunk {
            id,
            payload: bytes[data_start..data_end].to_vec(),
        });
        // Odd-sized chunks carry a single pad byte.
        pos = data_end + (size & 1);
    }
    Ok(chunks)
}

/// Reassemble a WebP file from chunks, in the given order.
fn write_webp_chunks(chunks: &[RiffChunk]) -> Vec<u8> {
    let mut body: Vec<u8> = b"WEBP".to_vec();
    for c in chunks {
        body.extend_from_slice(&c.id);
        body.extend_from_slice(&(c.payload.len() as u32).to_le_bytes());
        body.extend_from_slice(&c.payload);
        if !c.payload.len().is_multiple_of(2) {
            body.push(0);
        }
    }
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// Read canvas size and the alpha-used flag straight out of a VP8L chunk header
/// (signature byte 0x2F, then a 32-bit LSB-first field: 14 bits width-1,
/// 14 bits height-1, 1 bit alpha_is_used, 3 bits version).
fn parse_vp8l_header(payload: &[u8]) -> Option<(u32, u32, bool)> {
    if payload.len() < 5 || payload[0] != 0x2F {
        return None;
    }
    let bits = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
    let width = (bits & 0x3FFF) + 1;
    let height = ((bits >> 14) & 0x3FFF) + 1;
    let alpha = ((bits >> 28) & 1) == 1;
    Some((width, height, alpha))
}

/// Insert (or replace) the `EXIF` chunk of a WebP file, promoting a simple-format
/// file to the extended (VP8X) format when needed. Chunks are re-emitted in the
/// order the container spec requires: VP8X, ICCP, image data, EXIF, XMP.
fn set_webp_exif(bytes: &[u8], exif: &[u8]) -> Result<Vec<u8>, String> {
    let chunks = parse_webp_chunks(bytes)?;

    let mut vp8x = match chunks.iter().find(|c| &c.id == b"VP8X") {
        Some(c) => c.payload.clone(),
        None => {
            // Simple format: synthesize the 10-byte VP8X header from the VP8L
            // header. Lossy (VP8) simple files never come out of this app, so
            // there is nothing to derive a canvas from — let the caller fall
            // back to saving the image without a text chunk.
            let (w, h, alpha) = chunks
                .iter()
                .find(|c| &c.id == b"VP8L")
                .and_then(|c| parse_vp8l_header(&c.payload))
                .ok_or_else(|| {
                    "WebP has neither a VP8X header nor a VP8L chunk to derive the canvas from"
                        .to_string()
                })?;
            let mut p = vec![0u8; 10];
            if alpha {
                p[0] |= VP8X_FLAG_ALPHA;
            }
            p[4..7].copy_from_slice(&(w - 1).to_le_bytes()[0..3]);
            p[7..10].copy_from_slice(&(h - 1).to_le_bytes()[0..3]);
            p
        }
    };
    if vp8x.len() < 10 {
        return Err("Malformed VP8X chunk".to_string());
    }
    vp8x[0] |= VP8X_FLAG_EXIF;

    let mut icc: Vec<RiffChunk> = Vec::new();
    let mut image_data: Vec<RiffChunk> = Vec::new();
    let mut xmp: Vec<RiffChunk> = Vec::new();
    for c in chunks {
        match &c.id {
            // Rebuilt above / replaced below.
            b"VP8X" | b"EXIF" => {}
            b"ICCP" => icc.push(c),
            b"XMP " => xmp.push(c),
            _ => image_data.push(c),
        }
    }

    let mut out: Vec<RiffChunk> = Vec::with_capacity(image_data.len() + 3);
    out.push(RiffChunk {
        id: *b"VP8X",
        payload: vp8x,
    });
    out.extend(icc);
    out.extend(image_data);
    out.push(RiffChunk {
        id: *b"EXIF",
        payload: exif.to_vec(),
    });
    out.extend(xmp);
    Ok(write_webp_chunks(&out))
}

/// Extract the raw `EXIF` chunk payload from WebP bytes, if present.
fn get_webp_exif(bytes: &[u8]) -> Option<Vec<u8>> {
    parse_webp_chunks(bytes)
        .ok()?
        .into_iter()
        .find(|c| &c.id == b"EXIF")
        .map(|c| c.payload)
}

/// Build a minimal little-endian EXIF (TIFF) blob whose only tag is the Exif
/// sub-IFD UserComment (0x9286) carrying `text`. This mirrors what piexif (and
/// therefore A1111) writes for WebP, so external readers find the parameters
/// where they expect them.
fn build_exif_user_comment_blob(text: &str) -> Vec<u8> {
    // "UNICODE\0" + UTF-16BE rather than the ASCII charset code: prompts are
    // routinely non-Latin, and an ASCII prefix in front of UTF-8 bytes makes
    // strict readers mangle them.
    let mut payload: Vec<u8> = b"UNICODE\0".to_vec();
    for unit in text.encode_utf16() {
        payload.extend_from_slice(&unit.to_be_bytes());
    }

    const IFD0_OFFSET: u32 = 8; // right after the 8-byte TIFF header
    const EXIF_IFD_OFFSET: u32 = 26; // IFD0 = 2 count + 12 entry + 4 next
    const VALUE_OFFSET: u32 = 44; // Exif IFD = 2 count + 12 entry + 4 next

    let mut out = Vec::with_capacity(VALUE_OFFSET as usize + payload.len());
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&IFD0_OFFSET.to_le_bytes());
    // IFD0: a single entry, the Exif sub-IFD pointer.
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0x8769u16.to_le_bytes()); // ExifIFDPointer
    out.extend_from_slice(&4u16.to_le_bytes()); // LONG
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&EXIF_IFD_OFFSET.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
                                                // Exif sub-IFD: a single entry, UserComment.
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0x9286u16.to_le_bytes()); // UserComment
    out.extend_from_slice(&7u16.to_le_bytes()); // UNDEFINED
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&VALUE_OFFSET.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    debug_assert_eq!(out.len(), VALUE_OFFSET as usize);
    out.extend_from_slice(&payload);
    out
}

/// Decode a UTF-16 byte run into a String with the given endianness.
fn utf16_string(bytes: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|p| {
            if big_endian {
                u16::from_be_bytes([p[0], p[1]])
            } else {
                u16::from_le_bytes([p[0], p[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// Strip the 8-byte EXIF character-code prefix from a UserComment payload and
/// decode the remainder.
fn decode_user_comment(raw: &[u8]) -> String {
    if raw.len() >= 8 {
        let (prefix, body) = raw.split_at(8);
        if prefix == b"UNICODE\0" {
            // The charset code does not record endianness. Everything this app
            // writes is big-endian; probe for the JSON opening brace so files
            // from little-endian writers still read.
            let be = utf16_string(body, true);
            if be.contains('{') {
                return be;
            }
            let le = utf16_string(body, false);
            if le.contains('{') {
                return le;
            }
            return be;
        }
        if prefix == b"ASCII\0\0\0" || prefix == b"\0\0\0\0\0\0\0\0" {
            return String::from_utf8_lossy(body)
                .trim_end_matches('\0')
                .to_string();
        }
    }
    String::from_utf8_lossy(raw)
        .trim_end_matches('\0')
        .to_string()
}

/// Pull the UserComment text out of an EXIF (TIFF) blob, checking IFD0 and then
/// the Exif sub-IFD it points at. Handles both byte orders.
fn read_exif_user_comment(exif: &[u8]) -> Option<String> {
    // WebP EXIF chunks hold a bare TIFF stream, but the JPEG APP1 "Exif\0\0"
    // prefix shows up in the wild — tolerate it.
    let tiff = if exif.starts_with(b"Exif\0\0") {
        &exif[6..]
    } else {
        exif
    };
    if tiff.len() < 8 {
        return None;
    }
    let le = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let b = tiff.get(o..o + 2)?;
        Some(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let b = tiff.get(o..o + 4)?;
        Some(if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    if u16_at(2)? != 42 {
        return None;
    }

    // Returns (UserComment value range, Exif sub-IFD offset) for one IFD.
    let scan = |ifd: usize| -> (Option<(usize, usize)>, Option<usize>) {
        let mut user_comment = None;
        let mut sub_ifd = None;
        let Some(count) = u16_at(ifd) else {
            return (None, None);
        };
        for i in 0..count as usize {
            let e = ifd + 2 + i * 12;
            let (Some(tag), Some(ty), Some(cnt), Some(val)) =
                (u16_at(e), u16_at(e + 2), u32_at(e + 4), u32_at(e + 8))
            else {
                continue;
            };
            match tag {
                0x8769 => sub_ifd = Some(val as usize),
                0x9286 => {
                    let type_size = match ty {
                        1 | 2 | 6 | 7 => 1,
                        3 | 8 => 2,
                        4 | 9 | 11 => 4,
                        _ => 8,
                    };
                    let len = (cnt as usize).saturating_mul(type_size);
                    // Values of 4 bytes or fewer sit inline in the entry.
                    let start = if len <= 4 { e + 8 } else { val as usize };
                    user_comment = Some((start, len));
                }
                _ => {}
            }
        }
        (user_comment, sub_ifd)
    };

    let (mut found, sub_ifd) = scan(u32_at(4)? as usize);
    if found.is_none() {
        found = scan(sub_ifd?).0;
    }
    let (start, len) = found?;
    let raw = tiff.get(start..start.checked_add(len)?)?;
    Some(decode_user_comment(raw))
}

/// Embed SwarmUI-compatible metadata into WebP bytes.
///
/// Stealth alpha runs first (it rewrites pixels and re-encodes, which would drop
/// an already-spliced EXIF chunk), then the EXIF chunk goes in. As with PNG, a
/// stealth failure degrades to the text carrier alone rather than losing the
/// metadata entirely.
pub fn embed_webp_metadata(
    image_bytes: &[u8],
    params: &HashMap<String, String>,
    mode: MetadataMode,
) -> Result<Vec<u8>, String> {
    let json_text = format_swarmui_json(params);

    let (mut out, effective_mode) =
        if mode == MetadataMode::StealthAlpha || mode == MetadataMode::Both {
            match embed_webp_stealth_alpha(image_bytes, &json_text) {
                Ok(bytes) => (bytes, mode),
                Err(e) => {
                    log::warn!(
                        "WebP stealth alpha encoding failed ({}), falling back to EXIF only",
                        e
                    );
                    (image_bytes.to_vec(), MetadataMode::TextChunk)
                }
            }
        } else {
            (image_bytes.to_vec(), mode)
        };

    if effective_mode == MetadataMode::TextChunk || effective_mode == MetadataMode::Both {
        out = set_webp_exif(&out, &build_exif_user_comment_blob(&json_text))?;
    }

    Ok(out)
}

/// Decode a WebP, write the stealth-alpha bit stream into its alpha channel and
/// re-encode losslessly (VP8L), which preserves the LSBs exactly.
fn embed_webp_stealth_alpha(image_bytes: &[u8], json_text: &str) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory_with_format(image_bytes, image::ImageFormat::WebP)
        .map_err(|e| format!("WebP decode error: {}", e))?;
    let (width, height) = (img.width(), img.height());
    let mut rgba = img.to_rgba8().into_raw();
    encode_stealth_alpha(&mut rgba, width, height, 4, json_text)?;
    crate::jxl::encode_rgba8_webp_from_raw(&rgba, width, height, false).map_err(|e| e.to_string())
}

/// Read MooshieUI/SwarmUI metadata from WebP bytes: stealth alpha first (it
/// survives re-uploads), then the EXIF UserComment chunk.
pub fn read_webp_metadata(image_bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    if let Ok(Some(params)) = read_webp_stealth_alpha(image_bytes) {
        return Ok(Some(params));
    }
    let Some(exif) = get_webp_exif(image_bytes) else {
        return Ok(None);
    };
    let Some(text) = read_exif_user_comment(&exif) else {
        return Ok(None);
    };
    Ok(parse_swarmui_json(text.trim()))
}

fn read_webp_stealth_alpha(image_bytes: &[u8]) -> Result<Option<HashMap<String, String>>, String> {
    let img = image::load_from_memory_with_format(image_bytes, image::ImageFormat::WebP)
        .map_err(|e| format!("WebP decode error: {}", e))?;
    let (width, height) = (img.width(), img.height());
    let rgba = img.to_rgba8().into_raw();
    decode_stealth_alpha_pixels(&rgba, width as usize, height as usize, 4)
}

// ---------------------------------------------------------------------------
// GZip helpers
// ---------------------------------------------------------------------------

fn gzip_compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data)?;
    encoder.finish()
}

fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::read::GzDecoder::new(Cursor::new(data));
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// SwarmUI JSON formatting / parsing
// ---------------------------------------------------------------------------

/// Build SwarmUI-compatible JSON from the flat metadata map.
pub(crate) fn format_swarmui_json(params: &HashMap<String, String>) -> String {
    let mut image_params = serde_json::Map::new();

    let mappings: &[(&str, &str)] = &[
        ("positive_prompt", "prompt"),
        ("negative_prompt", "negativeprompt"),
        ("model", "model"),
        ("vae", "vae"),
        ("seed", "seed"),
        ("steps", "steps"),
        ("cfg", "cfgscale"),
        ("sampler", "sampler"),
        ("scheduler", "scheduler"),
        ("denoise", "denoise"),
        ("mode", "generationmode"),
        ("loras", "loras"),
    ];

    for &(internal, swarm) in mappings {
        if let Some(value) = params.get(internal) {
            if !value.is_empty() {
                image_params.insert(swarm.to_string(), serde_json::Value::String(value.clone()));
            }
        }
    }

    if let Some(size) = params.get("size") {
        if let Some((w, h)) = size.split_once('x') {
            if let (Ok(width), Ok(height)) = (w.parse::<u32>(), h.parse::<u32>()) {
                image_params.insert("width".to_string(), serde_json::json!(width));
                image_params.insert("height".to_string(), serde_json::json!(height));
            }
        }
    }

    if let Some(v) = params.get("upscale_model") {
        if !v.is_empty() {
            image_params.insert(
                "upscalemodel".to_string(),
                serde_json::Value::String(v.clone()),
            );
        }
    }
    if let Some(v) = params.get("upscale_scale") {
        if !v.is_empty() {
            image_params.insert(
                "upscalescale".to_string(),
                serde_json::Value::String(v.clone()),
            );
        }
    }
    if let Some(v) = params.get("upscale_denoise") {
        if !v.is_empty() {
            image_params.insert(
                "upscaledenoise".to_string(),
                serde_json::Value::String(v.clone()),
            );
        }
    }

    image_params.insert(
        "mooshie_version".to_string(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()),
    );

    let mut extra_data = serde_json::Map::new();
    let extra_keys = ["date", "generation_time"];
    for &key in &extra_keys {
        if let Some(value) = params.get(key) {
            if !value.is_empty() {
                extra_data.insert(key.to_string(), serde_json::Value::String(value.clone()));
            }
        }
    }

    let mut root = serde_json::Map::new();
    root.insert(
        "sui_image_params".to_string(),
        serde_json::Value::Object(image_params),
    );
    root.insert(
        "sui_extra_data".to_string(),
        serde_json::Value::Object(extra_data),
    );

    // MooshieUI marker + any mooshie_-prefixed extras
    let mut mooshie_extra = serde_json::Map::new();
    mooshie_extra.insert(
        "software".to_string(),
        serde_json::Value::String("MooshieUI".to_string()),
    );
    for (key, value) in params {
        if let Some(stripped) = key.strip_prefix("mooshie_") {
            if !value.is_empty() {
                mooshie_extra.insert(
                    stripped.to_string(),
                    serde_json::Value::String(value.clone()),
                );
            }
        }
    }
    root.insert(
        "mooshie_extra".to_string(),
        serde_json::Value::Object(mooshie_extra),
    );

    serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string())
}

/// Parse SwarmUI JSON format back into our flat key-value map.
pub(crate) fn parse_swarmui_json(text: &str) -> Option<HashMap<String, String>> {
    let root: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = root.as_object()?;

    let mut params = HashMap::new();

    if let Some(image_params) = obj.get("sui_image_params").and_then(|v| v.as_object()) {
        let reverse_mappings: &[(&str, &str)] = &[
            ("prompt", "positive_prompt"),
            ("negativeprompt", "negative_prompt"),
            ("model", "model"),
            ("vae", "vae"),
            ("seed", "seed"),
            ("steps", "steps"),
            ("cfgscale", "cfg"),
            ("sampler", "sampler"),
            ("scheduler", "scheduler"),
            ("denoise", "denoise"),
            ("generationmode", "mode"),
            ("loras", "loras"),
            ("upscalemodel", "upscale_model"),
            ("upscalescale", "upscale_scale"),
            ("upscaledenoise", "upscale_denoise"),
        ];

        for &(swarm, internal) in reverse_mappings {
            if swarm == "mooshie_version" {
                continue;
            }
            if let Some(value) = image_params.get(swarm) {
                let s = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !s.is_empty() {
                    params.insert(internal.to_string(), s);
                }
            }
        }

        if let (Some(w), Some(h)) = (image_params.get("width"), image_params.get("height")) {
            let ws = match w {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                _ => String::new(),
            };
            let hs = match h {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                _ => String::new(),
            };
            if !ws.is_empty() && !hs.is_empty() {
                params.insert("size".to_string(), format!("{}x{}", ws, hs));
            }
        }
    }

    if let Some(extra) = obj.get("sui_extra_data").and_then(|v| v.as_object()) {
        if let Some(date) = extra.get("date").and_then(|v| v.as_str()) {
            params.insert("date".to_string(), date.to_string());
        }
        if let Some(gen_time) = extra.get("generation_time").and_then(|v| v.as_str()) {
            params.insert("generation_time".to_string(), gen_time.to_string());
        }
    }

    // Round-trip MooshieUI extras back into the flat map
    if let Some(mooshie) = obj.get("mooshie_extra").and_then(|v| v.as_object()) {
        for (key, value) in mooshie {
            if key == "software" {
                continue;
            }
            let s = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if !s.is_empty() {
                params.insert(format!("mooshie_{}", key), s);
            }
        }
    }

    if params.is_empty() {
        None
    } else {
        Some(params)
    }
}

// ---------------------------------------------------------------------------
// Legacy A1111 parser
// ---------------------------------------------------------------------------

fn parse_a1111_params(text: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let lines: Vec<&str> = text.lines().collect();

    if lines.is_empty() {
        return params;
    }

    let mut positive_lines = Vec::new();
    let mut negative_lines = Vec::new();
    let mut settings_line = None;
    let mut in_negative = false;

    for line in &lines {
        if line.starts_with("Negative prompt: ") {
            in_negative = true;
            negative_lines.push(line.trim_start_matches("Negative prompt: "));
        } else if !in_negative && settings_line.is_none() {
            if line.starts_with("Steps:") || line.starts_with("Sampler:") || line.starts_with("CFG")
            {
                settings_line = Some(*line);
            } else {
                positive_lines.push(*line);
            }
        } else if in_negative {
            if line.starts_with("Steps:") || line.starts_with("Sampler:") || line.starts_with("CFG")
            {
                settings_line = Some(*line);
                in_negative = false;
            } else {
                negative_lines.push(*line);
            }
        }
    }

    params.insert("positive_prompt".to_string(), positive_lines.join("\n"));
    if !negative_lines.is_empty() {
        params.insert("negative_prompt".to_string(), negative_lines.join("\n"));
    }

    if let Some(settings) = settings_line {
        let mut current_key = String::new();
        let mut current_value = String::new();

        for part in settings.split(", ") {
            if let Some(colon_pos) = part.find(": ") {
                if !current_key.is_empty() {
                    store_setting(&mut params, &current_key, &current_value);
                }
                current_key = part[..colon_pos].to_string();
                current_value = part[colon_pos + 2..].to_string();
            } else if !current_key.is_empty() {
                current_value.push_str(", ");
                current_value.push_str(part);
            }
        }
        if !current_key.is_empty() {
            store_setting(&mut params, &current_key, &current_value);
        }
    }

    params
}

fn store_setting(params: &mut HashMap<String, String>, key: &str, value: &str) {
    let normalized_key = match key {
        "Steps" => "steps",
        "Sampler" => "sampler",
        "Scheduler" => "scheduler",
        "CFG scale" => "cfg",
        "Seed" => "seed",
        "Size" => "size",
        "Model" => "model",
        "VAE" => "vae",
        "Denoising strength" => "denoise",
        "Generation mode" => "mode",
        "LoRAs" => "loras",
        "Upscale model" => "upscale_model",
        "Upscale scale" => "upscale_scale",
        "Upscale denoise" => "upscale_denoise",
        other => other,
    };
    params.insert(normalized_key.to_string(), value.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_png(width: u32, height: u32, bit16: bool) -> Vec<u8> {
        let mut output = Vec::new();
        let bpp: usize = if bit16 { 6 } else { 3 }; // RGB
        let depth = if bit16 {
            png::BitDepth::Sixteen
        } else {
            png::BitDepth::Eight
        };
        let pixel_count = (width as usize) * (height as usize);
        let buf = vec![128u8; pixel_count * bpp];
        {
            let mut encoder = png::Encoder::new(&mut output, width, height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(depth);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&buf).unwrap();
        }
        output
    }

    #[test]
    fn stealth_alpha_round_trip_8bit() {
        let png_bytes = make_test_png(64, 64, false);
        let mut params = HashMap::new();
        params.insert(
            "positive_prompt".to_string(),
            "test prompt hello world".to_string(),
        );
        params.insert("seed".to_string(), "12345".to_string());
        params.insert("steps".to_string(), "20".to_string());

        let embedded = embed_png_metadata(&png_bytes, &params, MetadataMode::Both).unwrap();

        // Read back
        let result = read_png_metadata(&embedded).unwrap();
        assert!(result.is_some(), "Should find metadata");
        let read_params = result.unwrap();
        assert_eq!(
            read_params.get("positive_prompt").unwrap(),
            "test prompt hello world"
        );
        assert_eq!(read_params.get("seed").unwrap(), "12345");
    }

    #[test]
    fn stealth_alpha_round_trip_16bit() {
        let png_bytes = make_test_png(64, 64, true);
        let mut params = HashMap::new();
        params.insert("positive_prompt".to_string(), "16bit test".to_string());
        params.insert("model".to_string(), "test_model.safetensors".to_string());

        let embedded = embed_png_metadata(&png_bytes, &params, MetadataMode::StealthAlpha).unwrap();

        let result = read_png_metadata(&embedded).unwrap();
        assert!(
            result.is_some(),
            "Should find stealth metadata in 16-bit image"
        );
        let read_params = result.unwrap();
        assert_eq!(read_params.get("positive_prompt").unwrap(), "16bit test");
    }

    #[test]
    fn stealth_alpha_binary_format_check() {
        // Verify the raw bits match the Python format
        let png_bytes = make_test_png(64, 64, false);
        let mut params = HashMap::new();
        params.insert("positive_prompt".to_string(), "hello".to_string());

        let embedded = embed_png_metadata(&png_bytes, &params, MetadataMode::StealthAlpha).unwrap();

        // Decode the output PNG and check the first 120 alpha LSBs
        let decoder = png::Decoder::new(Cursor::new(&embedded));
        let mut reader = decoder.read_info().unwrap();
        let info = reader.info().clone();
        assert_eq!(info.color_type, png::ColorType::Rgba);

        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();

        let w = info.width as usize;
        let h = info.height as usize;

        // Read first 15 bytes (magic) from alpha LSBs in column-major order
        let mut magic = Vec::new();
        let mut bit_idx = 0usize;
        for _ in 0..15 {
            let mut byte_val = 0u8;
            for bit_pos in (0..8).rev() {
                let x = bit_idx / h;
                let y = bit_idx % h;
                let off = (y * w + x) * 4 + 3;
                let bit = buf[off] & 1;
                byte_val |= bit << bit_pos;
                bit_idx += 1;
            }
            magic.push(byte_val);
        }
        assert_eq!(&magic, b"stealth_pngcomp", "Magic header should match");
    }

    /// A deterministic non-uniform RGBA image, so a lossless round-trip is
    /// actually meaningful (a flat fill would pass even under lossy encoding).
    fn make_test_rgba(width: u32, height: u32) -> Vec<u8> {
        let mut buf = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                buf.push((x * 7 % 256) as u8);
                buf.push((y * 13 % 256) as u8);
                buf.push(((x + y) * 3 % 256) as u8);
                buf.push(255);
            }
        }
        buf
    }

    fn make_test_webp(width: u32, height: u32) -> Vec<u8> {
        let rgba = make_test_rgba(width, height);
        crate::jxl::encode_rgba8_webp_from_raw(&rgba, width, height, false).unwrap()
    }

    fn test_params() -> HashMap<String, String> {
        let mut params = HashMap::new();
        // Non-Latin text checks the UTF-16 UserComment encoding.
        params.insert(
            "positive_prompt".to_string(),
            "webp round trip テスト".to_string(),
        );
        params.insert("seed".to_string(), "987654321".to_string());
        params.insert("steps".to_string(), "28".to_string());
        params
    }

    fn assert_params_match(read: &HashMap<String, String>) {
        assert_eq!(
            read.get("positive_prompt").map(String::as_str),
            Some("webp round trip テスト")
        );
        assert_eq!(read.get("seed").map(String::as_str), Some("987654321"));
        assert_eq!(read.get("steps").map(String::as_str), Some("28"));
    }

    #[test]
    fn webp_format_detection() {
        assert_eq!(detect_format(&make_test_webp(32, 32)), ImageFormat::WebP);
    }

    #[test]
    fn webp_exif_round_trip() {
        let webp = make_test_webp(64, 48);
        let params = test_params();

        let embedded = embed_webp_metadata(&webp, &params, MetadataMode::TextChunk).unwrap();

        assert_eq!(detect_format(&embedded), ImageFormat::WebP);
        assert!(
            get_webp_exif(&embedded).is_some(),
            "EXIF chunk should be present"
        );
        // Text-chunk mode must not touch the pixels, so no stealth payload exists
        // and the generic reader has to fall through to EXIF.
        assert!(read_webp_stealth_alpha(&embedded).unwrap().is_none());
        let read = read_image_metadata(&embedded).unwrap().expect("metadata");
        assert_params_match(&read);
    }

    #[test]
    fn webp_stealth_alpha_round_trip_is_lossless() {
        let (w, h) = (64u32, 48u32);
        let webp = make_test_webp(w, h);
        let params = test_params();

        let embedded = embed_webp_metadata(&webp, &params, MetadataMode::StealthAlpha).unwrap();

        let read = read_webp_stealth_alpha(&embedded)
            .unwrap()
            .expect("stealth metadata");
        assert_params_match(&read);
        // Stealth-only mode writes no EXIF chunk.
        assert!(get_webp_exif(&embedded).is_none());

        // VP8L is lossless: RGB must survive bit-exactly (alpha carries the
        // payload in its LSBs, so only its top 7 bits are comparable).
        let original = make_test_rgba(w, h);
        let decoded = image::load_from_memory_with_format(&embedded, image::ImageFormat::WebP)
            .unwrap()
            .to_rgba8()
            .into_raw();
        assert_eq!(decoded.len(), original.len());
        for (i, (dec, orig)) in decoded
            .chunks_exact(4)
            .zip(original.chunks_exact(4))
            .enumerate()
        {
            assert_eq!(&dec[0..3], &orig[0..3], "RGB changed at pixel {}", i);
            assert_eq!(dec[3] | 1, orig[3] | 1, "alpha changed at pixel {}", i);
        }
    }

    #[test]
    fn webp_both_modes_round_trip() {
        let webp = make_test_webp(64, 48);
        let params = test_params();

        let embedded = embed_webp_metadata(&webp, &params, MetadataMode::Both).unwrap();

        // Both carriers must be independently readable.
        assert_params_match(
            &read_webp_stealth_alpha(&embedded)
                .unwrap()
                .expect("stealth metadata"),
        );
        let exif = get_webp_exif(&embedded).expect("EXIF chunk");
        let text = read_exif_user_comment(&exif).expect("UserComment");
        assert_params_match(&parse_swarmui_json(text.trim()).expect("parsed EXIF JSON"));
    }

    #[test]
    fn embed_image_metadata_preserves_container_format() {
        let params = test_params();

        let png = embed_image_metadata(&make_test_png(64, 64, false), &params, MetadataMode::Both)
            .unwrap();
        assert_eq!(detect_format(&png), ImageFormat::Png);
        assert_params_match(&read_image_metadata(&png).unwrap().expect("png metadata"));

        let webp =
            embed_image_metadata(&make_test_webp(64, 48), &params, MetadataMode::Both).unwrap();
        assert_eq!(detect_format(&webp), ImageFormat::WebP);
        assert_params_match(&read_image_metadata(&webp).unwrap().expect("webp metadata"));
    }

    /// PNG `tEXt` is ISO 8859-1 only, so a CJK prompt used to make the whole
    /// embed fail and the image was saved with no metadata at all. Those payloads
    /// must land in `iTXt` (UTF-8) and still read back.
    #[test]
    fn png_text_chunk_falls_back_to_itxt_for_non_latin1() {
        let png = embed_png_metadata(
            &make_test_png(32, 32, false),
            &test_params(),
            MetadataMode::TextChunk,
        )
        .unwrap();

        {
            let reader = png::Decoder::new(Cursor::new(&png[..]))
                .read_info()
                .unwrap();
            let info = reader.info();
            assert!(
                info.uncompressed_latin1_text.is_empty(),
                "Japanese text is not representable as tEXt"
            );
            assert!(
                info.utf8_text.iter().any(|c| c.keyword == "parameters"),
                "expected an iTXt fallback chunk"
            );
        }

        assert_params_match(&read_image_metadata(&png).unwrap().expect("png metadata"));
    }

    #[test]
    fn detects_mp4() {
        let mut bytes = vec![0x00, 0x00, 0x00, 0x20];
        bytes.extend_from_slice(b"ftypisom");
        bytes.extend_from_slice(&[0u8; 8]);
        assert_eq!(detect_format(&bytes), ImageFormat::Mp4);
    }

    /// Build one ISOBMFF box. Duplicated from the isobmff test module on
    /// purpose: these tests exercise the public dispatcher, not box internals.
    fn iso_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    fn iso_ftyp(brand: &[u8; 4]) -> Vec<u8> {
        let mut body = brand.to_vec();
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(brand);
        iso_box(b"ftyp", &body)
    }

    #[test]
    fn detects_avif_separately_from_mp4() {
        assert_eq!(detect_format(&iso_ftyp(b"avif")), ImageFormat::Avif);
        assert_eq!(detect_format(&iso_ftyp(b"avis")), ImageFormat::Avif);
        assert_eq!(detect_format(&iso_ftyp(b"isom")), ImageFormat::Mp4);
        assert_eq!(detect_format(&iso_ftyp(b"mp42")), ImageFormat::Mp4);
    }

    #[test]
    fn detects_gif() {
        assert_eq!(detect_format(b"GIF89a......."), ImageFormat::Gif);
        assert_eq!(detect_format(b"GIF87a......."), ImageFormat::Gif);
        assert_eq!(detect_format(b"GIF"), ImageFormat::Unknown);
    }

    #[test]
    fn reads_mp4_metadata_from_the_uuid_box() {
        let params = test_params();
        let json = format_swarmui_json(&params);

        let mut mp4 = iso_ftyp(b"isom");
        mp4.extend_from_slice(&iso_box(b"moov", b"index"));
        let with_uuid = crate::metadata::isobmff::append_uuid_xmp(&mp4, &json).unwrap();

        let read = read_image_metadata(&with_uuid).unwrap().unwrap();
        assert_params_match(&read);
    }

    #[test]
    fn mp4_without_metadata_reads_as_none() {
        let mut mp4 = iso_ftyp(b"isom");
        mp4.extend_from_slice(&iso_box(b"moov", b"index"));
        assert!(read_image_metadata(&mp4).unwrap().is_none());
    }

    #[test]
    fn malformed_input_never_errors() {
        // Sizes read straight off disk, pointed at nothing.
        let mut hostile = iso_ftyp(b"isom");
        hostile.extend_from_slice(&u32::MAX.to_be_bytes());
        hostile.extend_from_slice(b"moov");
        assert!(read_image_metadata(&hostile).is_ok());

        let mut avif = iso_ftyp(b"avif");
        avif.extend_from_slice(&[0xFF; 64]);
        assert!(read_image_metadata(&avif).is_ok());

        assert!(read_image_metadata(b"GIF89a\xff\xff\xff\xff\xff\xff\xff").is_ok());
        assert!(read_image_metadata(b"").is_ok());
    }

    #[test]
    fn embedding_into_video_containers_is_refused_not_panicked() {
        let params = test_params();
        assert!(
            embed_image_metadata(&iso_ftyp(b"avif"), &params, MetadataMode::TextChunk).is_err()
        );
        assert!(embed_image_metadata(b"GIF89a......", &params, MetadataMode::TextChunk).is_err());
    }

    #[test]
    fn mirrors_an_mp4_udta_comment_into_a_uuid_box() {
        let json = format_swarmui_json(&test_params());

        // moov/udta/meta with one mdta key named `comment`, the shape
        // `save_to(metadata={"comment": ...})` produces.
        let name = b"comment";
        let mut entry = ((8 + name.len()) as u32).to_be_bytes().to_vec();
        entry.extend_from_slice(b"mdta");
        entry.extend_from_slice(name);
        let mut keys_body = vec![0u8; 4];
        keys_body.extend_from_slice(&1u32.to_be_bytes());
        keys_body.extend_from_slice(&entry);
        let mut data_body = 1u32.to_be_bytes().to_vec();
        data_body.extend_from_slice(&0u32.to_be_bytes());
        data_body.extend_from_slice(json.as_bytes());
        let ilst = iso_box(
            b"ilst",
            &iso_box(&1u32.to_be_bytes(), &iso_box(b"data", &data_body)),
        );
        let mut meta_body = vec![0u8; 4];
        meta_body.extend_from_slice(&iso_box(b"keys", &keys_body));
        meta_body.extend_from_slice(&ilst);

        let mut mp4 = iso_ftyp(b"isom");
        mp4.extend_from_slice(&iso_box(
            b"moov",
            &iso_box(b"udta", &iso_box(b"meta", &meta_body)),
        ));
        mp4.extend_from_slice(&iso_box(b"mdat", b"frames"));

        let dir = std::env::temp_dir().join("mooshie_uuid_mirror_mp4");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        std::fs::write(&path, &mp4).unwrap();

        assert!(mirror_uuid_sidecar(&path));

        let after = std::fs::read(&path).unwrap();
        assert_eq!(&after[..mp4.len()], &mp4[..], "no existing byte moved");
        assert_params_match(&read_image_metadata(&after).unwrap().unwrap());
        // The mirror is what a Discord round trip leaves behind, so it has to
        // read on its own with the udta subtree gone.
        assert_eq!(
            crate::metadata::isobmff::read_uuid_xmp(&after).as_deref(),
            Some(json.as_str())
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mirroring_a_file_with_no_metadata_is_a_no_op() {
        let mut mp4 = iso_ftyp(b"isom");
        mp4.extend_from_slice(&iso_box(b"moov", b"index"));

        let dir = std::env::temp_dir().join("mooshie_uuid_mirror_bare");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        std::fs::write(&path, &mp4).unwrap();

        assert!(!mirror_uuid_sidecar(&path));
        assert_eq!(std::fs::read(&path).unwrap(), mp4, "file untouched");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mirroring_a_missing_or_unsupported_file_returns_false() {
        let missing = std::env::temp_dir().join("mooshie_no_such_clip.mp4");
        assert!(!mirror_uuid_sidecar(&missing));

        let dir = std::env::temp_dir().join("mooshie_uuid_mirror_png");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("not-a-video.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert!(!mirror_uuid_sidecar(&path));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// ASCII/Latin-1 payloads must keep using `tEXt`, which is the chunk
    /// A1111-compatible readers look for.
    #[test]
    fn png_text_chunk_stays_latin1_when_representable() {
        let mut params = HashMap::new();
        params.insert("positive_prompt".to_string(), "plain prompt".to_string());
        params.insert("seed".to_string(), "1".to_string());

        let png = embed_png_metadata(
            &make_test_png(32, 32, false),
            &params,
            MetadataMode::TextChunk,
        )
        .unwrap();

        let reader = png::Decoder::new(Cursor::new(&png[..]))
            .read_info()
            .unwrap();
        let info = reader.info();
        assert!(
            info.uncompressed_latin1_text
                .iter()
                .any(|c| c.keyword == "parameters"),
            "expected a tEXt chunk"
        );
        assert!(
            info.utf8_text.is_empty(),
            "iTXt fallback should not be used"
        );
    }
}
