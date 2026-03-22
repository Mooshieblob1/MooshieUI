use std::collections::HashMap;
use std::io::{Cursor, Read as _, Write as _};

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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Embed metadata into PNG image bytes using the specified mode.
pub fn embed_png_metadata(
    image_bytes: &[u8],
    params: &HashMap<String, String>,
    mode: MetadataMode,
) -> Result<Vec<u8>, String> {
    let json_text = format_swarmui_json(params);

    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let mut reader = decoder.read_info().map_err(|e| format!("PNG decode error: {}", e))?;
    let info = reader.info().clone();

    let mut buf = vec![0u8; reader.output_buffer_size()];
    let output_info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame read error: {}", e))?;
    buf.truncate(output_info.buffer_size());

    // If stealth alpha is requested, embed bits into pixel data
    let is_16bit = info.bit_depth == png::BitDepth::Sixteen;
    let (pixel_buf, color_type) = if mode == MetadataMode::StealthAlpha || mode == MetadataMode::Both
    {
        if is_16bit {
            let (mut rgba16, w, h) = to_rgba16(&buf, info.color_type, info.width, info.height)?;
            encode_stealth_alpha(&mut rgba16, w, h, 8, &json_text)?; // 8 bytes/pixel, alpha LSB at +7
            (rgba16, png::ColorType::Rgba)
        } else {
            let (mut rgba, w, h) = to_rgba8(&buf, info.color_type, info.width, info.height)?;
            encode_stealth_alpha(&mut rgba, w, h, 4, &json_text)?; // 4 bytes/pixel, alpha LSB at +3
            (rgba, png::ColorType::Rgba)
        }
    } else {
        (buf, info.color_type)
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

        if mode == MetadataMode::TextChunk || mode == MetadataMode::Both {
            encoder
                .add_text_chunk("parameters".to_string(), json_text)
                .map_err(|e| format!("Failed to add text chunk: {}", e))?;
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
    let reader = decoder.read_info().map_err(|e| format!("PNG decode error: {}", e))?;
    let info = reader.info();

    for chunk in &info.uncompressed_latin1_text {
        if chunk.keyword == "parameters" {
            let text = chunk.text.trim();
            if text.starts_with('{') {
                if let Some(parsed) = parse_swarmui_json(text) {
                    return Ok(Some(parsed));
                }
            }
            return Ok(Some(parse_a1111_params(text)));
        }
    }

    Ok(None)
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
        _ => Err(format!("Unsupported color type for 16-bit: {:?}", color_type)),
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
    let mut reader = decoder.read_info().map_err(|e| format!("PNG decode error: {}", e))?;
    let info = reader.info().clone();

    // Only RGBA images can have stealth alpha
    if info.color_type != png::ColorType::Rgba {
        return Ok(None);
    }

    let bpp: usize = if info.bit_depth == png::BitDepth::Sixteen { 8 } else { 4 };

    let mut buf = vec![0u8; reader.output_buffer_size()];
    let output_info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame read error: {}", e))?;
    buf.truncate(output_info.buffer_size());

    let w = info.width as usize;
    let h = info.height as usize;
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
    if data_bits == 0 || data_bits % 8 != 0 {
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
// GZip helpers
// ---------------------------------------------------------------------------

fn gzip_compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
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
fn format_swarmui_json(params: &HashMap<String, String>) -> String {
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
            image_params.insert("upscalemodel".to_string(), serde_json::Value::String(v.clone()));
        }
    }
    if let Some(v) = params.get("upscale_scale") {
        if !v.is_empty() {
            image_params.insert("upscalescale".to_string(), serde_json::Value::String(v.clone()));
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

    serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string())
}

/// Parse SwarmUI JSON format back into our flat key-value map.
fn parse_swarmui_json(text: &str) -> Option<HashMap<String, String>> {
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
            if line.starts_with("Steps:")
                || line.starts_with("Sampler:")
                || line.starts_with("CFG")
            {
                settings_line = Some(*line);
            } else {
                positive_lines.push(*line);
            }
        } else if in_negative {
            if line.starts_with("Steps:")
                || line.starts_with("Sampler:")
                || line.starts_with("CFG")
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
        let depth = if bit16 { png::BitDepth::Sixteen } else { png::BitDepth::Eight };
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
        params.insert("positive_prompt".to_string(), "test prompt hello world".to_string());
        params.insert("seed".to_string(), "12345".to_string());
        params.insert("steps".to_string(), "20".to_string());

        let embedded = embed_png_metadata(&png_bytes, &params, MetadataMode::Both).unwrap();

        // Read back
        let result = read_png_metadata(&embedded).unwrap();
        assert!(result.is_some(), "Should find metadata");
        let read_params = result.unwrap();
        assert_eq!(read_params.get("positive_prompt").unwrap(), "test prompt hello world");
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
        assert!(result.is_some(), "Should find stealth metadata in 16-bit image");
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

        let mut buf = vec![0u8; reader.output_buffer_size()];
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

}
