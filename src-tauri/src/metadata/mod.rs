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
///
/// The file is never read whole. Videos run to gigabytes, so only the box
/// headers and the small boxes that carry metadata are read, and the new box
/// is appended in place. Appending moves no existing byte, which is what makes
/// writing into the live file safe: an interrupted append leaves the original
/// boxes intact, and a failed one is truncated back off. No temporary sibling
/// file is created, so there is no predictable temp name to race either.
///
/// Blocking I/O: async callers should run it on a blocking thread.
pub fn mirror_uuid_sidecar(path: &std::path::Path) -> bool {
    mirror_uuid_sidecar_inner(path).unwrap_or(false)
}

fn mirror_uuid_sidecar_inner(path: &std::path::Path) -> std::io::Result<bool> {
    // Written in place, so never through a symlink to somewhere else.
    if !std::fs::symlink_metadata(path)?.file_type().is_file() {
        return Ok(false);
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let Some(mut iso) = IsoFile::open(file)? else {
        return Ok(false);
    };
    // Appending is only safe when the walk accounts for every byte.
    let Some(last) = iso.boxes.last().copied() else {
        return Ok(false);
    };
    if last.end != iso.len {
        return Ok(false);
    }
    let Some(json) = iso.container_comment()? else {
        return Ok(false);
    };
    // Already mirrored, by us or by an earlier pass over the same file.
    if iso.uuid_xmp()?.as_deref() == Some(json.as_str()) {
        return Ok(false);
    }
    let Some(uuid_box) = isobmff::uuid_xmp_box(&json) else {
        return Ok(false);
    };

    // A trailing Adobe uuid box is ours from a previous pass: replace it
    // rather than stacking another copy (see `isobmff::append_uuid_xmp`).
    let keep = if iso.is_xmp_uuid(&last)? {
        last.start
    } else {
        iso.len
    };
    let file = &mut iso.file;
    if let Err(e) = write_at_end(file, keep, &uuid_box) {
        // Leave the file ending on a whole box so it stays walkable.
        let _ = file.set_len(keep);
        return Err(e);
    }
    Ok(true)
}

/// Truncate `file` to `keep` bytes and append `data` there.
fn write_at_end(file: &mut std::fs::File, keep: u64, data: &[u8]) -> std::io::Result<()> {
    use std::io::{Seek as _, SeekFrom};
    file.set_len(keep)?;
    file.seek(SeekFrom::Start(keep))?;
    file.write_all(data)?;
    file.sync_all()
}

/// Largest `moov` / `meta` box read into memory to find the metadata inside
/// it. Sample tables for even long clips are a few megabytes.
const MAX_METADATA_BOX: u64 = 64 * 1024 * 1024;

/// Largest non-ISOBMFF file [`read_file_metadata`] will read whole.
const MAX_WHOLE_FILE_METADATA_READ: u64 = 256 * 1024 * 1024;

/// An mp4 or avif file opened for metadata access, with its top-level boxes
/// walked by seeking instead of by reading the file into memory.
struct IsoFile {
    file: std::fs::File,
    len: u64,
    boxes: Vec<isobmff::FileBox>,
    avif: bool,
}

impl IsoFile {
    /// `None` when the file is not ISOBMFF (its first box is not `ftyp`).
    fn open(mut file: std::fs::File) -> std::io::Result<Option<Self>> {
        let len = file.metadata()?.len();
        let boxes = isobmff::file_boxes(&mut file, len)?;
        let Some(ftyp) = boxes.first().copied().filter(|b| b.kind == *b"ftyp") else {
            return Ok(None);
        };
        let mut iso = Self {
            file,
            len,
            boxes,
            avif: false,
        };
        let Some(ftyp_bytes) = iso.read_box(&ftyp, 4096)? else {
            return Ok(None);
        };
        iso.avif = isobmff::is_avif(&ftyp_bytes);
        Ok(Some(iso))
    }

    fn read_at(&mut self, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
        use std::io::{Seek as _, SeekFrom};
        self.file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len];
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// The whole box, header included, or `None` when it is larger than `cap`.
    fn read_box(&mut self, b: &isobmff::FileBox, cap: u64) -> std::io::Result<Option<Vec<u8>>> {
        let len = b.end - b.start;
        if len > cap {
            return Ok(None);
        }
        self.read_at(b.start, len as usize).map(Some)
    }

    fn first_box(&self, kind: &[u8; 4]) -> Option<isobmff::FileBox> {
        self.boxes.iter().find(|b| b.kind == *kind).copied()
    }

    fn is_xmp_uuid(&mut self, b: &isobmff::FileBox) -> std::io::Result<bool> {
        if b.kind != *b"uuid" || b.end - b.body < 16 {
            return Ok(false);
        }
        let id = self.read_at(b.body, 16)?;
        Ok(isobmff::is_xmp_uuid(b.kind, &id))
    }

    /// The container-native payload: the `moov/udta` comment for mp4, the
    /// Exif UserComment for avif.
    fn container_comment(&mut self) -> std::io::Result<Option<String>> {
        if self.avif {
            let Some(meta) = self.first_box(b"meta") else {
                return Ok(None);
            };
            let Some(meta_bytes) = self.read_box(&meta, MAX_METADATA_BOX)? else {
                return Ok(None);
            };
            let Some((offset, length)) = isobmff::avif_exif_extent(&meta_bytes) else {
                return Ok(None);
            };
            if (offset as u64).saturating_add(length as u64) > self.len {
                return Ok(None);
            }
            let payload = self.read_at(offset as u64, length)?;
            Ok(isobmff::exif_from_item_payload(&payload)
                .as_deref()
                .and_then(read_exif_user_comment))
        } else {
            let Some(moov) = self.first_box(b"moov") else {
                return Ok(None);
            };
            let Some(moov_bytes) = self.read_box(&moov, MAX_METADATA_BOX)? else {
                return Ok(None);
            };
            Ok(isobmff::read_udta_comment(&moov_bytes))
        }
    }

    /// The payload of the first top-level Adobe XMP `uuid` box, if any.
    fn uuid_xmp(&mut self) -> std::io::Result<Option<String>> {
        let uuids: Vec<isobmff::FileBox> = self
            .boxes
            .iter()
            .filter(|b| b.kind == *b"uuid")
            .copied()
            .collect();
        for b in uuids {
            if !self.is_xmp_uuid(&b)? {
                continue;
            }
            // The first Adobe box decides, exactly as `read_uuid_xmp` does.
            let cap = (isobmff::MAX_PAYLOAD + 64) as u64;
            return Ok(self
                .read_box(&b, cap)?
                .and_then(|bytes| isobmff::read_uuid_xmp(&bytes)));
        }
        Ok(None)
    }
}

/// Read metadata straight from a file on disk.
///
/// mp4 and avif are walked box by box, so a multi-gigabyte video costs a few
/// small reads instead of a full load; the order matches
/// [`read_mp4_metadata`] and [`read_avif_metadata`]. Any other format is read
/// whole, up to a sane size. `None` for no metadata or any I/O failure.
///
/// Blocking I/O: async callers should run it on a blocking thread.
pub fn read_file_metadata(path: &std::path::Path) -> Option<HashMap<String, String>> {
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if let Some(mut iso) = IsoFile::open(file).ok()? {
        let text = match iso.container_comment().ok()? {
            Some(text) => Some(text),
            None => iso.uuid_xmp().ok()?,
        };
        return text.and_then(|t| parse_swarmui_json(t.trim()));
    }
    if len > MAX_WHOLE_FILE_METADATA_READ {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    read_image_metadata(&bytes).ok().flatten()
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
///
/// A PNG that still carries NovelAI's own chunks comes back untouched. Every
/// export and clipboard route reaches this one function, and embedding here
/// means a full decode and re-encode, which would drop those chunks and
/// overwrite the stealth alpha that novelai.net reads. Preserving the bytes
/// costs nothing: the metadata this call would have written says the same
/// thing, and the app reads NovelAI's chunks back just as happily as its own.
/// Once a local post-process has re-encoded the image the chunks are already
/// gone, so that case falls through and is embedded as normal.
pub fn embed_image_metadata(
    image_bytes: &[u8],
    params: &HashMap<String, String>,
    mode: MetadataMode,
) -> Result<Vec<u8>, String> {
    match detect_format(image_bytes) {
        ImageFormat::Png if png_carries_novelai_metadata(image_bytes) => {
            log::info!("embed_image_metadata: preserving NovelAI PNG bytes verbatim");
            Ok(image_bytes.to_vec())
        }
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

    let mut decoder = png::Decoder::new(Cursor::new(image_bytes));
    // Normalise every input to 8/16-bit gray, gray+alpha, RGB or RGBA: palette
    // images are expanded to RGB(A), 1/2/4-bit grayscale is widened to 8 bits,
    // and a tRNS chunk becomes a real alpha channel. Without this, packed
    // low-bit samples were read as 8-bit ones (and indexed an RGBA buffer out
    // of bounds in stealth mode), and an indexed image was re-encoded without
    // its palette, which the encoder always rejected.
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG decode error: {}", e))?;
    let info = reader.info().clone();
    let (out_color, out_depth) = reader.output_color_type();

    let mut buf = png_frame_buffer(reader.output_buffer_size())?;
    let output_info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG frame read error: {}", e))?;
    buf.truncate(output_info.buffer_size());

    // If stealth alpha is requested, embed bits into pixel data.
    // If stealth fails (e.g. unsupported color type), fall back to text_chunk only.
    let is_16bit = out_depth == png::BitDepth::Sixteen;
    let (pixel_buf, color_type, effective_mode) = if mode == MetadataMode::StealthAlpha
        || mode == MetadataMode::Both
    {
        let stealth_result = if is_16bit {
            to_rgba16(&buf, out_color, info.width, info.height).and_then(|(mut rgba16, w, h)| {
                encode_stealth_alpha(&mut rgba16, w, h, 8, &json_text)?;
                Ok((rgba16, w, h))
            })
        } else {
            to_rgba8(&buf, out_color, info.width, info.height).and_then(|(mut rgba, w, h)| {
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
                (buf, out_color, MetadataMode::TextChunk)
            }
        }
    } else {
        (buf, out_color, mode)
    };

    // Re-encode PNG, carrying the colour-space and density chunks across.
    // tRNS and PLTE are not carried: the expansion above folded them into
    // the pixels.
    let mut out_info = png::Info::with_size(info.width, info.height);
    out_info.color_type = color_type;
    out_info.bit_depth = out_depth;
    out_info.srgb = info.srgb;
    out_info.source_gamma = info.gama_chunk;
    out_info.source_chromaticities = info.chrm_chunk;
    out_info.icc_profile = info.icc_profile.clone();
    out_info.pixel_dims = info.pixel_dims;
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::with_info(&mut output, out_info)
            .map_err(|e| format!("PNG encode error: {}", e))?;

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
                .and_then(itxt_text)
        })
        .or_else(|| {
            info.compressed_latin1_text
                .iter()
                .find(|c| c.keyword == "parameters")
                .and_then(ztxt_text)
        });

    let Some(raw_text) = raw_text else {
        // No `parameters` chunk. NovelAI does not write one: it spreads its
        // metadata across chunks of its own (`Software`, `Source`, `Comment`),
        // so an image straight off novelai.net lands here rather than above.
        return Ok(crate::novelai::metadata::parse_chunks(&png_text_chunks(
            info,
        )));
    };
    let text = raw_text.trim();
    if text.starts_with('{') {
        if let Some(parsed) = parse_swarmui_json(text) {
            return Ok(Some(parsed));
        }
    }
    Ok(Some(parse_a1111_params(text)))
}

/// Does this PNG still carry NovelAI's own metadata chunks?
///
/// The one thing that would destroy them is re-encoding, so this is the test
/// for whether a PNG should be written to the gallery byte for byte instead of
/// going through the usual embed. A pure NovelAI generation answers yes; the
/// same image after local post-processing answers no, because the re-encode
/// that post-processing performed already dropped the chunks.
///
/// Anything that is not a decodable PNG answers no, which is the safe way
/// round: the caller then takes the normal path and the file is handled as it
/// always was.
pub fn png_carries_novelai_metadata(image_bytes: &[u8]) -> bool {
    let decoder = png::Decoder::new(Cursor::new(image_bytes));
    let Ok(reader) = decoder.read_info() else {
        return false;
    };
    crate::novelai::metadata::is_novelai_chunks(&png_text_chunks(reader.info()))
}

/// Collect every PNG text chunk into a keyword-to-text map.
///
/// Readers that key off a single well-known keyword can index the decoder's
/// three chunk lists directly; a reader that has to look at the whole set (the
/// NovelAI one does, its metadata is split across five chunks) needs them
/// flattened first. Later encodings win, which only matters for a file that
/// wrote the same keyword twice.
fn png_text_chunks(info: &png::Info<'_>) -> HashMap<String, String> {
    let mut chunks = HashMap::new();
    for chunk in &info.uncompressed_latin1_text {
        chunks.insert(chunk.keyword.clone(), chunk.text.clone());
    }
    for chunk in &info.compressed_latin1_text {
        if let Some(text) = ztxt_text(chunk) {
            chunks.insert(chunk.keyword.clone(), text);
        }
    }
    for chunk in &info.utf8_text {
        if let Some(text) = itxt_text(chunk) {
            chunks.insert(chunk.keyword.clone(), text);
        }
    }
    chunks
}

/// Largest decompressed zTXt/iTXt chunk we will read. Real prompts and
/// workflows are kilobytes; `get_text()` inflates without any bound, so a
/// small crafted chunk could expand to gigabytes.
const MAX_TEXT_CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// Text of a zTXt chunk, or `None` if it is corrupt or inflates past
/// [`MAX_TEXT_CHUNK_BYTES`].
fn ztxt_text(chunk: &png::text_metadata::ZTXtChunk) -> Option<String> {
    let mut chunk = chunk.clone();
    chunk
        .decompress_text_with_limit(MAX_TEXT_CHUNK_BYTES)
        .ok()?;
    chunk.get_text().ok()
}

/// Text of an iTXt chunk, or `None` if it is corrupt or inflates past
/// [`MAX_TEXT_CHUNK_BYTES`].
fn itxt_text(chunk: &png::text_metadata::ITXtChunk) -> Option<String> {
    let mut chunk = chunk.clone();
    chunk
        .decompress_text_with_limit(MAX_TEXT_CHUNK_BYTES)
        .ok()?;
    chunk.get_text().ok()
}

/// Largest decoded PNG frame we will allocate: 1 GiB, enough for a 16384 x
/// 16384 RGBA8 image. The size comes straight from the header, and a 68-byte
/// file can claim billions of rows; allocating that aborts the process.
const MAX_PNG_FRAME_BYTES: usize = 1024 * 1024 * 1024;

/// Allocate the output buffer for one decoded PNG frame, refusing headers
/// that claim more than [`MAX_PNG_FRAME_BYTES`].
fn png_frame_buffer(size: Option<usize>) -> Result<Vec<u8>, String> {
    match size {
        Some(n) if n <= MAX_PNG_FRAME_BYTES => Ok(vec![0u8; n]),
        Some(n) => Err(format!("PNG too large to decode ({n} bytes)")),
        None => Err("PNG output buffer size unavailable".into()),
    }
}

/// The 8 bytes every PNG starts with.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Walk a PNG's chunk list, returning each chunk's type alongside the byte
/// range of the whole chunk (length field, type, data and CRC).
///
/// `None` for anything that is not a structurally sound PNG, so callers can
/// fall back to leaving the bytes alone rather than producing a broken file.
fn png_chunk_spans(bytes: &[u8]) -> Option<Vec<([u8; 4], std::ops::Range<usize>)>> {
    if bytes.len() < 8 || bytes[..8] != PNG_SIGNATURE {
        return None;
    }
    let mut spans = Vec::new();
    let mut pos = 8usize;
    while pos + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
        // 12 = the 4-byte length, the 4-byte type and the 4-byte CRC that
        // bracket the data.
        let end = pos.checked_add(12)?.checked_add(len)?;
        if end > bytes.len() {
            return None;
        }
        let mut kind = [0u8; 4];
        kind.copy_from_slice(&bytes[pos + 4..pos + 8]);
        let is_end = kind == *b"IEND";
        spans.push((kind, pos..end));
        if is_end {
            break;
        }
        pos = end;
    }
    Some(spans)
}

/// Splice `source`'s text chunks into `target`, byte for byte.
///
/// The NovelAI face pass repaints pixels, so its result has to be re-encoded,
/// and a re-encode drops the `Title` / `Description` / `Software` / `Source` /
/// `Comment` chunks that are the only thing novelai.net reads when an image is
/// dragged back onto it. Carrying the originals across restores that, and
/// re-arms the verbatim-preserve branch in `save_to_gallery_inner` so nothing
/// downstream rewrites them again.
///
/// The chunks are copied as raw bytes rather than decoded and re-added: their
/// CRCs are already correct, and a round trip through the png crate's Latin-1
/// text would mangle a Japanese prompt that NovelAI wrote as UTF-8.
///
/// The stealth-alpha payload cannot be carried across the same way. It is one
/// sequential bitstream over the whole image, so a repainted rectangle in the
/// middle destroys everything after it; the text chunks are what survives a
/// face pass.
///
/// Returns `None` when there is nothing to copy, or when either side is not a
/// valid PNG, which callers read as "keep the bytes you already have".
pub fn copy_png_text_chunks(source: &[u8], target: &[u8]) -> Option<Vec<u8>> {
    let mut text = Vec::new();
    for (kind, span) in png_chunk_spans(source)? {
        if kind == *b"tEXt" || kind == *b"zTXt" || kind == *b"iTXt" {
            text.extend_from_slice(&source[span]);
        }
    }
    if text.is_empty() {
        return None;
    }

    // Before the first IDAT. Text chunks may sit anywhere between IHDR and
    // IEND, but the colour-space chunks an encoder writes ahead of the pixel
    // data must keep their order, and inserting here disturbs none of them.
    let insert_at = png_chunk_spans(target)?
        .into_iter()
        .find(|(kind, _)| *kind == *b"IDAT")
        .map(|(_, span)| span.start)?;

    let mut out = Vec::with_capacity(target.len() + text.len());
    out.extend_from_slice(&target[..insert_at]);
    out.extend_from_slice(&text);
    out.extend_from_slice(&target[insert_at..]);
    Some(out)
}

// ---------------------------------------------------------------------------
// Stealth alpha encoding (SwarmUI-compatible)
// ---------------------------------------------------------------------------

const STEALTH_MAGIC: &[u8] = b"stealth_pngcomp";

/// Check that `buf` holds exactly `width * height` pixels of `bytes_per_pixel`
/// bytes each. The converters below index by that geometry, so a buffer of any
/// other shape (packed sub-byte samples, a truncated frame) is refused rather
/// than silently misread.
fn check_pixel_len(
    buf: &[u8],
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
) -> Result<usize, String> {
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or("image dimensions overflow")?;
    let expected = pixel_count
        .checked_mul(bytes_per_pixel)
        .ok_or("image dimensions overflow")?;
    if buf.len() != expected {
        return Err(format!(
            "pixel buffer is {} bytes, expected {expected} for {width}x{height}",
            buf.len()
        ));
    }
    Ok(pixel_count)
}

/// Channels per pixel for the four colour types the converters accept.
fn channel_count(color_type: png::ColorType) -> Option<usize> {
    match color_type {
        png::ColorType::Grayscale => Some(1),
        png::ColorType::GrayscaleAlpha => Some(2),
        png::ColorType::Rgb => Some(3),
        png::ColorType::Rgba => Some(4),
        png::ColorType::Indexed => None,
    }
}

/// Convert raw 8-bit pixel buffer to RGBA8 (adding alpha if needed).
fn to_rgba8(
    buf: &[u8],
    color_type: png::ColorType,
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    let channels = channel_count(color_type)
        .ok_or_else(|| format!("Unsupported color type: {:?}", color_type))?;
    let pixel_count = check_pixel_len(buf, width, height, channels)?;
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
    let channels = channel_count(color_type)
        .ok_or_else(|| format!("Unsupported color type for 16-bit: {:?}", color_type))?;
    let pixel_count = check_pixel_len(buf, width, height, channels * 2)?;
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
    check_pixel_len(rgba, width, height, bpp)?;

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

    let mut buf = png_frame_buffer(reader.output_buffer_size())?;
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
    // Every read below indexes by that geometry; a buffer too short for it
    // is not a stealth carrier.
    if pixel_count
        .checked_mul(bpp)
        .is_none_or(|needed| buf.len() < needed)
    {
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

    Ok(parse_stealth_payload(json_text.trim()))
}

/// Parse a decoded stealth-alpha payload, whichever tool wrote it.
///
/// Both SwarmUI and NovelAI hide JSON in the alpha LSBs with the same envelope,
/// so the carrier cannot say which one it is; the shape of the JSON inside can.
/// Each parser declines anything that is not its own, so trying them in turn is
/// safe, and NovelAI goes last because SwarmUI's format is the common case.
fn parse_stealth_payload(text: &str) -> Option<HashMap<String, String>> {
    parse_swarmui_json(text).or_else(|| crate::novelai::metadata::parse_stealth_json(text))
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

/// Largest stealth-alpha payload we will inflate. The JSON it carries is
/// kilobytes, but the gzip stream is attacker-supplied and a megabyte of
/// pixels can inflate to gigabytes.
const MAX_STEALTH_PAYLOAD_BYTES: u64 = 16 * 1024 * 1024;

fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(data));
    let mut out = Vec::new();
    decoder
        .take(MAX_STEALTH_PAYLOAD_BYTES + 1)
        .read_to_end(&mut out)?;
    if out.len() as u64 > MAX_STEALTH_PAYLOAD_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stealth metadata payload is too large",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod hostile_input_tests {
    use super::*;

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(&crc.sum().to_be_bytes());
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// Hand-built 8-bit RGBA PNG, so the header can claim any size.
    fn png_with(w: u32, h: u32, extra: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        for (kind, data) in extra {
            chunk(&mut out, kind, data);
        }
        chunk(&mut out, b"IDAT", &zlib(&[0u8; 5]));
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn png_claiming_a_huge_frame_is_refused_not_allocated() {
        // 68 bytes on disk, ~2.7e17 bytes of claimed pixels: allocating that
        // used to abort the whole process.
        let png = png_with(16_000_000, 0xFFFF_FFFF, &[]);
        assert!(read_stealth_alpha(&png).is_err());
        assert!(embed_png_metadata(&png, &HashMap::new(), MetadataMode::StealthAlpha).is_err());
        let _ = read_png_metadata(&png);
    }

    #[test]
    fn compressed_text_chunks_are_inflated_with_a_limit() {
        let mut bomb = b"parameters\0\0".to_vec();
        bomb.extend_from_slice(&zlib(&vec![b'A'; MAX_TEXT_CHUNK_BYTES + 1]));
        let mut small = b"Comment\0\0".to_vec();
        small.extend_from_slice(&zlib(b"hello"));
        let png = png_with(1, 1, &[(*b"zTXt", bomb), (*b"zTXt", small)]);

        let reader = png::Decoder::new(Cursor::new(&png)).read_info().unwrap();
        let chunks = &reader.info().compressed_latin1_text;
        assert_eq!(ztxt_text(&chunks[0]), None);
        assert_eq!(ztxt_text(&chunks[1]).as_deref(), Some("hello"));

        let texts = png_text_chunks(reader.info());
        assert!(!texts.contains_key("parameters"));
        assert_eq!(texts.get("Comment").map(String::as_str), Some("hello"));
    }

    #[test]
    fn stealth_gzip_payload_is_inflated_with_a_limit() {
        let bomb = gzip_compress(&vec![b' '; MAX_STEALTH_PAYLOAD_BYTES as usize + 1]).unwrap();
        assert!(gzip_decompress(&bomb).is_err());
        let ok = gzip_compress(b"{\"sui_image_params\":{}}").unwrap();
        assert_eq!(gzip_decompress(&ok).unwrap(), b"{\"sui_image_params\":{}}");
    }
}

#[cfg(test)]
mod png_normalisation_tests {
    use super::*;

    fn params() -> HashMap<String, String> {
        let mut params = HashMap::new();
        params.insert("positive_prompt".to_string(), "low bit depth".to_string());
        params.insert("seed".to_string(), "4242".to_string());
        params
    }

    fn assert_read_back(png_bytes: &[u8]) {
        let read = read_png_metadata(png_bytes).unwrap().unwrap();
        assert_eq!(
            read.get("positive_prompt").map(String::as_str),
            Some("low bit depth")
        );
        assert_eq!(read.get("seed").map(String::as_str), Some("4242"));
    }

    /// Encode a PNG with an arbitrary colour type / depth and optional
    /// palette, tRNS, gAMA and iCCP chunks.
    fn encode(
        (w, h): (u32, u32),
        color: png::ColorType,
        depth: png::BitDepth,
        data: &[u8],
        palette: Option<&[u8]>,
        trns: Option<&[u8]>,
        colour_space: bool,
    ) -> Vec<u8> {
        let mut info = png::Info::with_size(w, h);
        info.color_type = color;
        info.bit_depth = depth;
        info.palette = palette.map(|p| std::borrow::Cow::Owned(p.to_vec()));
        info.trns = trns.map(|t| std::borrow::Cow::Owned(t.to_vec()));
        if colour_space {
            info.source_gamma = Some(png::ScaledFloat::new(0.5));
            info.icc_profile = Some(std::borrow::Cow::Owned(b"not really an icc".to_vec()));
            info.pixel_dims = Some(png::PixelDimensions {
                xppu: 3780,
                yppu: 3780,
                unit: png::Unit::Meter,
            });
        }
        let mut out = Vec::new();
        {
            let encoder = png::Encoder::with_info(&mut out, info).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(data).unwrap();
        }
        out
    }

    /// Decode with no transformations, returning the output info and pixels.
    fn decode(bytes: &[u8]) -> (png::ColorType, png::BitDepth, Vec<u8>) {
        let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        (info.color_type, info.bit_depth, buf)
    }

    const MODES: [MetadataMode; 3] = [
        MetadataMode::TextChunk,
        MetadataMode::StealthAlpha,
        MetadataMode::Both,
    ];

    #[test]
    fn low_bit_grayscale_embeds_in_every_mode() {
        // 1/2/4-bit grayscale used to reach `to_rgba8` still packed, and
        // stealth mode then indexed past the end of the RGBA buffer.
        let (w, h) = (64u32, 64u32);
        for (depth, bits) in [
            (png::BitDepth::One, 1u32),
            (png::BitDepth::Two, 2),
            (png::BitDepth::Four, 4),
        ] {
            let row = (w * bits).div_ceil(8) as usize;
            // Every sample at its maximum value.
            let data = vec![0xFFu8; row * h as usize];
            let src = encode(
                (w, h),
                png::ColorType::Grayscale,
                depth,
                &data,
                None,
                None,
                false,
            );
            for mode in MODES {
                let out = embed_png_metadata(&src, &params(), mode)
                    .unwrap_or_else(|e| panic!("{bits}-bit {mode:?}: {e}"));
                assert_read_back(&out);
                let (color, out_depth, pixels) = decode(&out);
                assert_eq!(out_depth, png::BitDepth::Eight, "{bits}-bit {mode:?}");
                // A maximal low-bit sample widens to 255, not to its raw bits.
                assert_eq!(pixels[0], 255, "{bits}-bit {mode:?}");
                if mode == MetadataMode::TextChunk {
                    assert_eq!(color, png::ColorType::Grayscale);
                    assert_eq!(pixels.len(), (w * h) as usize);
                } else {
                    assert_eq!(color, png::ColorType::Rgba);
                }
            }
        }
    }

    #[test]
    fn indexed_png_keeps_its_colours_in_every_mode() {
        let (w, h) = (48u32, 48u32);
        let palette = [255, 0, 0, 0, 255, 0, 0, 0, 255, 10, 20, 30];
        let data: Vec<u8> = (0..w * h).map(|i| (i % 4) as u8).collect();
        let plain = encode(
            (w, h),
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            &data,
            Some(&palette),
            None,
            false,
        );
        for mode in MODES {
            // Used to fail every time: re-encoded as indexed without a PLTE.
            let out = embed_png_metadata(&plain, &params(), mode)
                .unwrap_or_else(|e| panic!("{mode:?}: {e}"));
            assert_read_back(&out);
            let (color, _, pixels) = decode(&out);
            let bpp = if color == png::ColorType::Rgb { 3 } else { 4 };
            assert_eq!(&pixels[bpp..bpp + 3], &[0, 255, 0], "{mode:?}");
            assert_eq!(&pixels[3 * bpp..3 * bpp + 3], &[10, 20, 30], "{mode:?}");
        }

        // With tRNS the transparency becomes a real alpha channel.
        let with_trns = encode(
            (w, h),
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            &data,
            Some(&palette),
            Some(&[0, 128]),
            false,
        );
        let out = embed_png_metadata(&with_trns, &params(), MetadataMode::TextChunk).unwrap();
        let (color, _, pixels) = decode(&out);
        assert_eq!(color, png::ColorType::Rgba);
        assert_eq!(&pixels[..4], &[255, 0, 0, 0]);
        assert_eq!(&pixels[4..8], &[0, 255, 0, 128]);
        assert_eq!(&pixels[8..12], &[0, 0, 255, 255]);
        assert_read_back(&out);
    }

    #[test]
    fn sub_byte_indexed_png_embeds() {
        let (w, h) = (40u32, 40u32);
        let palette = [0, 0, 0, 200, 100, 50];
        let data = vec![0b0101_0101u8; (w as usize).div_ceil(8) * h as usize];
        let src = encode(
            (w, h),
            png::ColorType::Indexed,
            png::BitDepth::One,
            &data,
            Some(&palette),
            None,
            false,
        );
        for mode in MODES {
            let out = embed_png_metadata(&src, &params(), mode)
                .unwrap_or_else(|e| panic!("{mode:?}: {e}"));
            assert_read_back(&out);
            let (color, _, pixels) = decode(&out);
            let bpp = if color == png::ColorType::Rgb { 3 } else { 4 };
            assert_eq!(&pixels[..3], &[0, 0, 0]);
            assert_eq!(&pixels[bpp..bpp + 3], &[200, 100, 50]);
        }
    }

    #[test]
    fn sixteen_bit_grayscale_trns_becomes_alpha() {
        let (w, h) = (40u32, 40u32);
        let data = vec![0x12u8; (w * h * 2) as usize];
        let src = encode(
            (w, h),
            png::ColorType::Grayscale,
            png::BitDepth::Sixteen,
            &data,
            None,
            Some(&[0x12, 0x12]),
            false,
        );
        let out = embed_png_metadata(&src, &params(), MetadataMode::TextChunk).unwrap();
        let (color, depth, pixels) = decode(&out);
        assert_eq!(
            (color, depth),
            (png::ColorType::GrayscaleAlpha, png::BitDepth::Sixteen)
        );
        // Every sample matched the tRNS key, so every pixel is transparent.
        assert_eq!(&pixels[..4], &[0x12, 0x12, 0, 0]);
        let stealth = embed_png_metadata(&src, &params(), MetadataMode::StealthAlpha).unwrap();
        assert_read_back(&stealth);
    }

    #[test]
    fn colour_space_and_density_chunks_are_carried() {
        let (w, h) = (40u32, 40u32);
        let data = vec![77u8; (w * h * 3) as usize];
        let src = encode(
            (w, h),
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            &data,
            None,
            None,
            true,
        );
        for mode in MODES {
            let out = embed_png_metadata(&src, &params(), mode).unwrap();
            let reader = png::Decoder::new(Cursor::new(&out)).read_info().unwrap();
            let info = reader.info();
            assert_eq!(
                info.gama_chunk,
                Some(png::ScaledFloat::new(0.5)),
                "{mode:?}"
            );
            assert_eq!(
                info.icc_profile.as_deref(),
                Some(&b"not really an icc"[..]),
                "{mode:?}"
            );
            assert_eq!(info.pixel_dims.map(|d| d.xppu), Some(3780), "{mode:?}");
        }
    }

    #[test]
    fn mis_sized_pixel_buffers_are_refused_not_indexed() {
        assert!(to_rgba8(&[0u8; 10], png::ColorType::Grayscale, 8, 8).is_err());
        assert!(to_rgba16(&[0u8; 10], png::ColorType::Rgb, 8, 8).is_err());
        assert!(to_rgba8(&[0u8; 64], png::ColorType::Indexed, 8, 8).is_err());
        let mut short = vec![0u8; 16];
        assert!(encode_stealth_alpha(&mut short, 64, 64, 4, "{}").is_err());
        assert_eq!(
            decode_stealth_alpha_pixels(&[0u8; 16], 64, 64, 4).unwrap(),
            None
        );
    }
}

#[cfg(test)]
mod video_file_tests {
    use super::*;

    fn iso_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    fn json() -> String {
        let mut params = HashMap::new();
        params.insert("positive_prompt".to_string(), "streamed".to_string());
        params.insert("seed".to_string(), "77".to_string());
        format_swarmui_json(&params)
    }

    /// ftyp, a `moov/udta/meta` mdta `comment`, and an `mdat` of `frames`
    /// bytes placed before the moov, the way a non-faststart mux lays it out.
    fn mp4_with_comment(text: &str, frames: usize) -> Vec<u8> {
        let name = b"comment";
        let mut entry = ((8 + name.len()) as u32).to_be_bytes().to_vec();
        entry.extend_from_slice(b"mdta");
        entry.extend_from_slice(name);
        let mut keys_body = vec![0u8; 4];
        keys_body.extend_from_slice(&1u32.to_be_bytes());
        keys_body.extend_from_slice(&entry);
        let mut data_body = 1u32.to_be_bytes().to_vec();
        data_body.extend_from_slice(&0u32.to_be_bytes());
        data_body.extend_from_slice(text.as_bytes());
        let ilst = iso_box(
            b"ilst",
            &iso_box(&1u32.to_be_bytes(), &iso_box(b"data", &data_body)),
        );
        let mut meta_body = vec![0u8; 4];
        meta_body.extend_from_slice(&iso_box(b"keys", &keys_body));
        meta_body.extend_from_slice(&ilst);

        let mut ftyp_body = b"isom".to_vec();
        ftyp_body.extend_from_slice(&0u32.to_be_bytes());
        ftyp_body.extend_from_slice(b"isom");
        let mut mp4 = iso_box(b"ftyp", &ftyp_body);
        mp4.extend_from_slice(&iso_box(b"mdat", &vec![0x5A; frames]));
        mp4.extend_from_slice(&iso_box(
            b"moov",
            &iso_box(b"udta", &iso_box(b"meta", &meta_body)),
        ));
        mp4
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mooshie-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn uuid_count(bytes: &[u8]) -> usize {
        isobmff::boxes(bytes)
            .iter()
            .filter(|b| b.kind == *b"uuid")
            .count()
    }

    #[test]
    fn mirroring_appends_in_place_without_moving_a_byte() {
        let dir = scratch("mirror-stream");
        let path = dir.join("clip.mp4");
        // Big enough that the mdat dwarfs everything the mirror reads.
        let mp4 = mp4_with_comment(&json(), 4 * 1024 * 1024);
        std::fs::write(&path, &mp4).unwrap();

        assert!(mirror_uuid_sidecar(&path));
        let after = std::fs::read(&path).unwrap();
        assert_eq!(&after[..mp4.len()], &mp4[..]);
        assert_eq!(
            isobmff::read_uuid_xmp(&after).as_deref(),
            Some(json().as_str())
        );
        assert_eq!(uuid_count(&after), 1);

        // A second pass finds it already mirrored and writes nothing.
        assert!(!mirror_uuid_sidecar(&path));
        assert_eq!(std::fs::read(&path).unwrap(), after);

        // No temp sibling is ever left behind.
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(names.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mirroring_replaces_its_own_stale_trailing_box() {
        let dir = scratch("mirror-replace");
        let path = dir.join("clip.mp4");
        let mp4 = mp4_with_comment(&json(), 64);
        let stale = isobmff::append_uuid_xmp(&mp4, "{\"stale\":true}").unwrap();
        std::fs::write(&path, &stale).unwrap();

        assert!(mirror_uuid_sidecar(&path));
        let after = std::fs::read(&path).unwrap();
        assert_eq!(after, isobmff::append_uuid_xmp(&mp4, &json()).unwrap());
        assert_eq!(uuid_count(&after), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mirroring_leaves_a_file_with_an_unwalkable_tail_alone() {
        let dir = scratch("mirror-tail");
        let path = dir.join("clip.mp4");
        let mut mp4 = mp4_with_comment(&json(), 64);
        mp4.extend_from_slice(&400u32.to_be_bytes());
        mp4.extend_from_slice(b"free");
        std::fs::write(&path, &mp4).unwrap();

        assert!(!mirror_uuid_sidecar(&path));
        assert_eq!(std::fs::read(&path).unwrap(), mp4);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn mirroring_never_writes_through_a_symlink() {
        let dir = scratch("mirror-link");
        let target = dir.join("real.mp4");
        let mp4 = mp4_with_comment(&json(), 64);
        std::fs::write(&target, &mp4).unwrap();
        let link = dir.join("link.mp4");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(!mirror_uuid_sidecar(&link));
        assert_eq!(std::fs::read(&target).unwrap(), mp4);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_metadata_reads_mp4_without_loading_it_and_other_formats_whole() {
        let dir = scratch("file-meta");
        let mp4_path = dir.join("clip.mp4");
        std::fs::write(&mp4_path, mp4_with_comment(&json(), 2 * 1024 * 1024)).unwrap();
        let read = read_file_metadata(&mp4_path).unwrap();
        assert_eq!(read.get("seed").map(String::as_str), Some("77"));

        // Only the uuid sidecar left (a scrubbed udta): still found.
        let mut ftyp_body = b"isom".to_vec();
        ftyp_body.extend_from_slice(&0u32.to_be_bytes());
        ftyp_body.extend_from_slice(b"isom");
        let mut bare = iso_box(b"ftyp", &ftyp_body);
        bare.extend_from_slice(&iso_box(b"moov", b"index"));
        let uuid_only = isobmff::append_uuid_xmp(&bare, &json()).unwrap();
        std::fs::write(&mp4_path, &uuid_only).unwrap();
        let read = read_file_metadata(&mp4_path).unwrap();
        assert_eq!(
            read.get("positive_prompt").map(String::as_str),
            Some("streamed")
        );

        std::fs::write(&mp4_path, &bare).unwrap();
        assert!(read_file_metadata(&mp4_path).is_none());
        assert!(read_file_metadata(&dir.join("missing.mp4")).is_none());

        // A non-ISOBMFF file goes through the in-memory dispatcher.
        let mut params = HashMap::new();
        params.insert("seed".to_string(), "5".to_string());
        let gif_path = dir.join("anim.gif");
        let mut gif_bytes = b"GIF89a".to_vec();
        gif_bytes.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0]);
        gif_bytes.extend_from_slice(&[0x21, 0xFE]);
        let comment = format_swarmui_json(&params);
        for chunk in comment.as_bytes().chunks(255) {
            gif_bytes.push(chunk.len() as u8);
            gif_bytes.extend_from_slice(chunk);
        }
        gif_bytes.extend_from_slice(&[0, 0x3B]);
        std::fs::write(&gif_path, &gif_bytes).unwrap();
        let read = read_file_metadata(&gif_path).unwrap();
        assert_eq!(read.get("seed").map(String::as_str), Some("5"));

        std::fs::remove_dir_all(&dir).ok();
    }
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

    /// A PNG carrying the tEXt chunks NovelAI writes into its own output.
    fn make_novelai_png(width: u32, height: u32) -> Vec<u8> {
        let mut output = Vec::new();
        let buf = vec![128u8; (width as usize) * (height as usize) * 4];
        {
            let mut encoder = png::Encoder::new(&mut output, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            for (keyword, text) in [
                ("Title", "AI generated image"),
                ("Description", "1girl, standing"),
                ("Software", "NovelAI"),
                ("Source", "NovelAI Diffusion V4 ABC123"),
                (
                    "Comment",
                    "{\"seed\": 577536437, \"width\": 8, \"height\": 8}",
                ),
            ] {
                encoder
                    .add_text_chunk(keyword.to_string(), text.to_string())
                    .unwrap();
            }
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&buf).unwrap();
        }
        output
    }

    /// NovelAI's own chunks are what novelai.net reads back on import, and a
    /// re-encode drops every one of them. Embedding must therefore hand a
    /// NovelAI PNG straight back rather than rewriting it.
    #[test]
    fn embedding_preserves_novelai_png_byte_for_byte() {
        let png_bytes = make_novelai_png(8, 8);
        assert!(png_carries_novelai_metadata(&png_bytes));

        let mut params = HashMap::new();
        params.insert("positive_prompt".to_string(), "1girl, standing".to_string());
        params.insert("seed".to_string(), "577536437".to_string());

        for mode in [
            MetadataMode::TextChunk,
            MetadataMode::StealthAlpha,
            MetadataMode::Both,
        ] {
            let embedded = embed_image_metadata(&png_bytes, &params, mode).unwrap();
            assert_eq!(embedded, png_bytes, "NovelAI PNG was rewritten in {mode:?}");
        }
    }

    /// The guard keys off NovelAI's chunks, so an image whose chunks a local
    /// post-process already stripped must take the normal embedding path.
    #[test]
    fn embedding_still_writes_metadata_for_non_novelai_png() {
        let png_bytes = make_test_png(8, 8, false);
        assert!(!png_carries_novelai_metadata(&png_bytes));

        let mut params = HashMap::new();
        params.insert("seed".to_string(), "577536437".to_string());

        let embedded = embed_image_metadata(&png_bytes, &params, MetadataMode::TextChunk).unwrap();
        assert_ne!(embedded, png_bytes);
        let decoder = png::Decoder::new(Cursor::new(embedded.as_slice()));
        let reader = decoder.read_info().unwrap();
        assert!(png_text_chunks(reader.info()).contains_key("parameters"));
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

    /// The NovelAI face pass repaints pixels and therefore re-encodes, which
    /// drops the chunks novelai.net reads on import. Splicing the originals
    /// into the composite is what puts them back.
    #[test]
    fn text_chunks_splice_into_a_re_encoded_png() {
        let source = make_novelai_png(8, 8);
        let composite = make_test_png(8, 8, false);
        assert!(!png_carries_novelai_metadata(&composite));

        let spliced = copy_png_text_chunks(&source, &composite).unwrap();
        assert!(png_carries_novelai_metadata(&spliced));

        let decoder = png::Decoder::new(Cursor::new(spliced.as_slice()));
        let mut reader = decoder.read_info().unwrap();
        let chunks = png_text_chunks(reader.info());
        assert_eq!(chunks.get("Software").map(String::as_str), Some("NovelAI"));
        assert_eq!(
            chunks.get("Comment").map(String::as_str),
            Some("{\"seed\": 577536437, \"width\": 8, \"height\": 8}")
        );

        // The pixels stay the composite's: the source is a metadata donor, not
        // an image one.
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);

        // And the app still reads the restored chunks back as NovelAI's.
        let params = read_png_metadata(&spliced).unwrap().unwrap();
        assert_eq!(
            params.get("mooshie_backend").map(String::as_str),
            Some("novelai")
        );
        assert_eq!(params.get("seed").map(String::as_str), Some("577536437"));
    }

    /// Nothing to copy, or nothing to copy into, must leave the caller with the
    /// bytes it already had rather than a half-written file.
    #[test]
    fn text_chunk_splice_declines_when_there_is_nothing_to_copy() {
        let source = make_test_png(8, 8, false);
        let composite = make_test_png(4, 4, false);
        assert!(copy_png_text_chunks(&source, &composite).is_none());
        assert!(copy_png_text_chunks(b"not a png at all", &composite).is_none());
        assert!(copy_png_text_chunks(&make_novelai_png(8, 8), b"nope").is_none());
    }
}
