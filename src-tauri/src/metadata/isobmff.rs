//! ISOBMFF (mp4 and avif) box mechanics.
//!
//! Every function here takes bytes that came from an arbitrary file a user
//! dropped on the window, so every size read off disk is bounds-checked against
//! the slice that contains it. Malformed input yields `None` or a short list,
//! never a panic and never an allocation sized by an attacker.

/// Largest payload any reader here will copy out of a box.
pub(super) const MAX_PAYLOAD: usize = 1024 * 1024;

/// Longest box path a lookup will follow. Our deepest real path is
/// `moov/udta/meta/ilst/<key>/data`, six segments.
pub(super) const MAX_DEPTH: usize = 8;

/// Most siblings enumerated at one level. A file of a million empty boxes is
/// not something we owe a complete answer to.
const MAX_SIBLINGS: usize = 4096;

/// One box header located inside a slice. All three offsets are relative to the
/// slice the box was found in, not to the whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BoxHeader {
    pub kind: [u8; 4],
    /// Offset of the size field.
    pub start: usize,
    /// Offset of the first payload byte, after the header.
    pub body: usize,
    /// Offset one past the last payload byte.
    pub end: usize,
}

fn u32_at(slice: &[u8], off: usize) -> Option<u32> {
    let b = slice.get(off..off + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64_at(slice: &[u8], off: usize) -> Option<u64> {
    let b = slice.get(off..off + 8)?;
    Some(u64::from_be_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

/// Enumerate the boxes directly inside `slice`.
///
/// Stops at the first malformed header rather than reporting an error: the
/// callers all want whatever prefix parsed cleanly, and a truncated download is
/// far more common than a hostile file.
pub(super) fn boxes(slice: &[u8]) -> Vec<BoxHeader> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= slice.len() && out.len() < MAX_SIBLINGS {
        let Some(size32) = u32_at(slice, off) else {
            break;
        };
        let mut kind = [0u8; 4];
        kind.copy_from_slice(&slice[off + 4..off + 8]);

        let (size, header) = match size32 {
            // 1 means the real size is a 64-bit value after the type.
            1 => match u64_at(slice, off + 8) {
                Some(large) => (large as usize, 16usize),
                None => break,
            },
            // 0 means the box runs to the end of the enclosing slice.
            0 => (slice.len() - off, 8usize),
            n => (n as usize, 8usize),
        };

        // A box cannot be smaller than its own header, and cannot claim to
        // extend past the slice that contains it.
        if size < header || off + size > slice.len() {
            break;
        }

        out.push(BoxHeader {
            kind,
            start: off,
            body: off + header,
            end: off + size,
        });
        off += size;
    }
    out
}

/// The payload of the box reached by following `path` from `slice`.
///
/// `meta` is a FullBox: four bytes of version and flags sit between its header
/// and its children, and descending into it without skipping them finds
/// garbage. It is the only such container on any path we use.
pub(super) fn find_path<'a>(slice: &'a [u8], path: &[&[u8; 4]]) -> Option<&'a [u8]> {
    if path.is_empty() || path.len() > MAX_DEPTH {
        return None;
    }
    let mut current = slice;
    for (i, want) in path.iter().enumerate() {
        let found = boxes(current).into_iter().find(|b| b.kind == **want)?;
        current = current.get(found.body..found.end)?;
        // Only skip the FullBox preamble when we are about to descend further.
        if i + 1 < path.len() && *want == b"meta" {
            current = current.get(4..)?;
        }
    }
    Some(current)
}

/// Whether the `ftyp` box names AVIF as its major brand or lists it as a
/// compatible brand. `avis` is the animated-sequence brand, which is what an
/// animated AVIF export declares.
pub(super) fn is_avif(bytes: &[u8]) -> bool {
    let Some(ftyp) = boxes(bytes).into_iter().find(|b| b.kind == *b"ftyp") else {
        return false;
    };
    let Some(payload) = bytes.get(ftyp.body..ftyp.end) else {
        return false;
    };
    // major_brand (4) + minor_version (4) + compatible_brands (4 each). Checking
    // every 4-byte group covers the major brand and the list in one pass; the
    // minor version is a number and will not collide with these tags.
    payload
        .chunks_exact(4)
        .enumerate()
        .any(|(i, tag)| i != 1 && (tag == b"avif" || tag == b"avis"))
}

/// The `comment` value from `moov/udta`, whichever of the two spellings the
/// muxer used.
///
/// Order matters only in that mdta is what our own writer produces. The
/// QuickTime atom is the fallback because a plain `ffmpeg -c copy` without
/// `-movflags use_metadata_tags` rewrites a standard comment into that form.
pub(super) fn read_udta_comment(bytes: &[u8]) -> Option<String> {
    read_mdta_comment(bytes).or_else(|| read_qt_comment(bytes))
}

/// `moov/udta/meta` holds a `keys` table of key names and an `ilst` of values
/// whose four-character box "type" is the 1-based index into that table.
fn read_mdta_comment(bytes: &[u8]) -> Option<String> {
    let keys = find_path(bytes, &[b"moov", b"udta", b"meta", b"keys"])?;
    let index = mdta_key_index(keys, b"comment")?;
    let ilst = find_path(bytes, &[b"moov", b"udta", b"meta", b"ilst"])?;

    let wanted = index.to_be_bytes();
    let item = boxes(ilst).into_iter().find(|b| b.kind == wanted)?;
    let item_body = ilst.get(item.body..item.end)?;

    let data = boxes(item_body).into_iter().find(|b| b.kind == *b"data")?;
    // data payload: 4 bytes type indicator, 4 bytes locale, then the value.
    let value = item_body.get(data.body + 8..data.end)?;
    if value.len() > MAX_PAYLOAD {
        return None;
    }
    Some(String::from_utf8_lossy(value).into_owned())
}

/// 1-based index of `name` in a `keys` table, or `None` if it is absent.
fn mdta_key_index(keys: &[u8], name: &[u8]) -> Option<u32> {
    // keys payload: 4 bytes version/flags, 4 bytes entry count, then entries of
    // (4 byte size, 4 byte namespace, key name).
    let count = u32_at(keys, 4)?.min(MAX_SIBLINGS as u32);
    let mut off = 8usize;
    for i in 0..count {
        let size = u32_at(keys, off)? as usize;
        if size < 8 || off + size > keys.len() {
            return None;
        }
        if keys.get(off + 8..off + size)? == name {
            return Some(i + 1);
        }
        off += size;
    }
    None
}

/// A bare `(c)cmt` atom directly under `udta`: 2 bytes length, 2 bytes
/// language, then the text.
fn read_qt_comment(bytes: &[u8]) -> Option<String> {
    let cmt = find_path(bytes, &[b"moov", b"udta", b"\xa9cmt"])?;
    let text = cmt.get(4..)?;
    if text.is_empty() || text.len() > MAX_PAYLOAD || text.iter().all(|&b| b == 0) {
        return None;
    }
    Some(String::from_utf8_lossy(text).into_owned())
}

/// The raw TIFF blob from an AVIF `Exif` item.
///
/// AVIF stores metadata as a HEIF item: `meta/iinf` names it, `meta/iloc` says
/// where its bytes are. Only construction method 0 (an offset into the file) is
/// handled, which is what libavif and therefore Pillow write.
pub(super) fn read_avif_exif(bytes: &[u8]) -> Option<Vec<u8>> {
    let meta = find_path(bytes, &[b"meta"])?.get(4..)?;
    let item_id = exif_item_id(find_path(meta, &[b"iinf"])?)?;
    let (offset, length) = item_extent(find_path(meta, &[b"iloc"])?, item_id)?;

    if length == 0 || length > MAX_PAYLOAD {
        return None;
    }
    let payload = bytes.get(offset..offset.checked_add(length)?)?;

    // The item payload is a 4-byte tiff_header_offset followed by the TIFF
    // stream, but writers disagree about whether the offset is included. Find
    // the byte-order mark near the front instead of trusting the count.
    let head = payload.get(..payload.len().min(16))?;
    let start = head
        .windows(2)
        .position(|w| w == b"II" || w == b"MM")
        .unwrap_or(0);
    Some(payload.get(start..)?.to_vec())
}

/// The item ID of the entry in `iinf` whose type is `Exif`.
fn exif_item_id(iinf: &[u8]) -> Option<u32> {
    let version = *iinf.first()?;
    // entry_count is 16-bit in version 0 and 32-bit from version 1.
    let body_start = if version == 0 { 6 } else { 8 };
    let entries = iinf.get(body_start..)?;

    for b in boxes(entries) {
        if b.kind != *b"infe" {
            continue;
        }
        let body = entries.get(b.body..b.end)?;
        let infe_version = *body.first()?;
        // infe v2 uses a 16-bit item ID, v3 a 32-bit one. v0/v1 predate the
        // item_type field entirely and never carry Exif.
        let (id, type_at) = match infe_version {
            2 => (
                u16::from_be_bytes([*body.get(4)?, *body.get(5)?]) as u32,
                8usize,
            ),
            3 => (u32_at(body, 4)?, 10usize),
            _ => continue,
        };
        if body.get(type_at..type_at + 4)? == b"Exif" {
            return Some(id);
        }
    }
    None
}

/// The (offset, length) of the first extent of `item_id` in an `iloc` table.
fn item_extent(iloc: &[u8], item_id: u32) -> Option<(usize, usize)> {
    let version = *iloc.first()?;
    let sizes = *iloc.get(4)?;
    let offset_size = (sizes >> 4) as usize;
    let length_size = (sizes & 0x0f) as usize;
    let packed = *iloc.get(5)?;
    let base_offset_size = (packed >> 4) as usize;
    let index_size = if version == 1 || version == 2 {
        (packed & 0x0f) as usize
    } else {
        0
    };

    let mut off = 6usize;
    let count = if version < 2 {
        let c = u16::from_be_bytes([*iloc.get(off)?, *iloc.get(off + 1)?]) as u32;
        off += 2;
        c
    } else {
        let c = u32_at(iloc, off)?;
        off += 4;
        c
    };

    // A field width other than 0, 4 or 8 is legal in the spec but never
    // produced by any encoder we write for; refuse rather than guess.
    let read_int = |slice: &[u8], at: usize, width: usize| -> Option<u64> {
        match width {
            0 => Some(0),
            4 => u32_at(slice, at).map(u64::from),
            8 => u64_at(slice, at),
            _ => None,
        }
    };

    for _ in 0..count.min(MAX_SIBLINGS as u32) {
        let id = if version < 2 {
            let v = u16::from_be_bytes([*iloc.get(off)?, *iloc.get(off + 1)?]) as u32;
            off += 2;
            v
        } else {
            let v = u32_at(iloc, off)?;
            off += 4;
            v
        };
        if version == 1 || version == 2 {
            off += 2; // construction_method
        }
        off += 2; // data_reference_index
        let base = read_int(iloc, off, base_offset_size)?;
        off += base_offset_size;
        let extents = u16::from_be_bytes([*iloc.get(off)?, *iloc.get(off + 1)?]);
        off += 2;

        for e in 0..extents {
            off += index_size;
            let extent_offset = read_int(iloc, off, offset_size)?;
            off += offset_size;
            let extent_length = read_int(iloc, off, length_size)?;
            off += length_size;
            if id == item_id && e == 0 {
                let start = base.checked_add(extent_offset)?;
                return Some((start.try_into().ok()?, extent_length.try_into().ok()?));
            }
        }
    }
    None
}

/// Adobe's XMP identifier for a top-level `uuid` box, BE7ACFCB-97A9-42E8-9C71-999491E3AFAC.
///
/// This box sits beside `moov` rather than inside it, which is the whole point:
/// decoders skip unknown top-level boxes, and Discord's mp4 scrubber walks
/// `moov/udta` and neutralises every child it finds there without ever looking
/// at the top level.
const XMP_UUID: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

const XMP_OPEN: &str = "<mooshie:parameters>";
const XMP_CLOSE: &str = "</mooshie:parameters>";

/// Wrap `json` in the smallest XMP packet exiftool will still recognise.
fn xmp_packet(json: &str) -> String {
    let escaped = json
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:mooshie=\"http://mooshieui.local/ns/1.0/\">\
{XMP_OPEN}{escaped}{XMP_CLOSE}\
</rdf:Description></rdf:RDF></x:xmpmeta>\
<?xpacket end=\"w\"?>"
    )
}

/// The JSON payload from a top-level Adobe XMP `uuid` box.
pub(super) fn read_uuid_xmp(bytes: &[u8]) -> Option<String> {
    let found = boxes(bytes)
        .into_iter()
        .find(|b| b.kind == *b"uuid" && bytes.get(b.body..b.body + 16) == Some(&XMP_UUID[..]))?;
    let packet = bytes.get(found.body + 16..found.end)?;
    if packet.len() > MAX_PAYLOAD {
        return None;
    }
    let text = String::from_utf8_lossy(packet);
    let start = text.find(XMP_OPEN)? + XMP_OPEN.len();
    let end = text.get(start..)?.find(XMP_CLOSE)? + start;
    Some(
        text.get(start..end)?
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

/// Append an Adobe XMP `uuid` box carrying `json` after every existing
/// top-level box. Returns `None` when `bytes` is not walkable ISOBMFF.
///
/// Appending at the very end moves no existing byte, so `stco` and `co64`
/// sample offsets stay valid no matter where `moov` sits relative to `mdat`.
///
/// An existing Adobe `uuid` box that is already last is ours from a previous
/// pass and gets truncated away first, so re-running never stacks copies. One
/// that sits earlier in the file belongs to a third-party tool: it is left
/// alone, and since `read_uuid_xmp` takes the first match, their copy keeps
/// winning. That is the same canonical-first rule the container-native reader
/// follows.
pub(super) fn append_uuid_xmp(bytes: &[u8], json: &str) -> Option<Vec<u8>> {
    let found = boxes(bytes);
    // A real ISOBMFF file starts with `ftyp` and the walk covers all of it.
    let last = found.last()?;
    if found.first()?.kind != *b"ftyp" || last.end != bytes.len() {
        return None;
    }

    let keep =
        if last.kind == *b"uuid" && bytes.get(last.body..last.body + 16) == Some(&XMP_UUID[..]) {
            last.start
        } else {
            bytes.len()
        };

    let packet = xmp_packet(json);
    let body_len = 16 + packet.len();
    let size = u32::try_from(8 + body_len).ok()?;

    let mut out = Vec::with_capacity(keep + 8 + body_len);
    out.extend_from_slice(bytes.get(..keep)?);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(b"uuid");
    out.extend_from_slice(&XMP_UUID);
    out.extend_from_slice(packet.as_bytes());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build one box: 32-bit size, four-character type, payload.
    fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// An `ftyp` box declaring `brand` as both major and compatible brand.
    fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
        let mut body = brand.to_vec();
        body.extend_from_slice(&0u32.to_be_bytes()); // minor version
        body.extend_from_slice(brand);
        bx(b"ftyp", &body)
    }

    #[test]
    fn walks_a_well_formed_top_level() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&bx(b"moov", b"abcd"));
        buf.extend_from_slice(&bx(b"mdat", b"efgh"));

        let found = boxes(&buf);
        let kinds: Vec<[u8; 4]> = found.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, vec![*b"ftyp", *b"moov", *b"mdat"]);
        assert_eq!(found[1].start, 20);
        assert_eq!(found[1].body, 28);
        assert_eq!(found[1].end, 32);
    }

    #[test]
    fn stops_at_a_truncated_box() {
        let mut buf = ftyp(b"isom");
        // Claims 400 bytes but only 4 follow.
        buf.extend_from_slice(&400u32.to_be_bytes());
        buf.extend_from_slice(b"moov");
        buf.extend_from_slice(b"abcd");

        let found = boxes(&buf);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, *b"ftyp");
    }

    #[test]
    fn rejects_a_size_below_the_header() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&4u32.to_be_bytes()); // size 4, impossible
        buf.extend_from_slice(b"junk");

        assert_eq!(boxes(&buf).len(), 1);
    }

    #[test]
    fn reads_a_64_bit_size() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&1u32.to_be_bytes()); // largesize follows
        buf.extend_from_slice(b"mdat");
        buf.extend_from_slice(&20u64.to_be_bytes()); // 16 header + 4 payload
        buf.extend_from_slice(b"data");

        let found = boxes(&buf);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].kind, *b"mdat");
        assert_eq!(found[1].end - found[1].body, 4);
    }

    #[test]
    fn a_zero_size_box_runs_to_the_end() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&0u32.to_be_bytes()); // to end of slice
        buf.extend_from_slice(b"mdat");
        buf.extend_from_slice(b"tail");

        let found = boxes(&buf);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].end, buf.len());
    }

    #[test]
    fn finds_a_nested_payload() {
        let inner = bx(b"udta", &bx(b"cmnt", b"hello"));
        let buf = bx(b"moov", &inner);

        let payload = find_path(&buf, &[b"moov", b"udta", b"cmnt"]).unwrap();
        assert_eq!(payload, b"hello");
    }

    #[test]
    fn skips_the_version_and_flags_of_a_meta_box() {
        // `meta` is a FullBox: 4 bytes of version/flags before its children.
        let mut meta_body = vec![0u8; 4];
        meta_body.extend_from_slice(&bx(b"ilst", b"payload"));
        let buf = bx(b"moov", &bx(b"meta", &meta_body));

        assert_eq!(
            find_path(&buf, &[b"moov", b"meta", b"ilst"]).unwrap(),
            b"payload"
        );
    }

    #[test]
    fn refuses_a_path_past_the_depth_cap() {
        let buf = bx(b"moov", b"");
        let deep: Vec<&[u8; 4]> = vec![b"moov"; MAX_DEPTH + 1];
        assert!(find_path(&buf, &deep).is_none());
    }

    #[test]
    fn detects_avif_brands() {
        assert!(is_avif(&ftyp(b"avif")));
        assert!(is_avif(&ftyp(b"avis")));
        assert!(!is_avif(&ftyp(b"isom")));
        assert!(!is_avif(&ftyp(b"mp42")));
        assert!(!is_avif(b"short"));
    }

    #[test]
    fn detects_avif_from_a_compatible_brand_only() {
        // Major brand `mif1`, compatible brands `mif1` then `avif`.
        let mut body = b"mif1".to_vec();
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(b"mif1");
        body.extend_from_slice(b"avif");
        assert!(is_avif(&bx(b"ftyp", &body)));
    }

    /// A `moov/udta/meta` subtree holding one mdta key named `comment`.
    /// This is the shape ffmpeg writes under `-movflags use_metadata_tags`.
    fn moov_with_mdta_comment(text: &str) -> Vec<u8> {
        let name = b"comment";
        let mut entry = ((8 + name.len()) as u32).to_be_bytes().to_vec();
        entry.extend_from_slice(b"mdta");
        entry.extend_from_slice(name);

        let mut keys_body = vec![0u8; 4]; // version + flags
        keys_body.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        keys_body.extend_from_slice(&entry);
        let keys = bx(b"keys", &keys_body);

        let mut data_body = 1u32.to_be_bytes().to_vec(); // type indicator: UTF-8
        data_body.extend_from_slice(&0u32.to_be_bytes()); // locale
        data_body.extend_from_slice(text.as_bytes());
        // An ilst child's four-character "type" is the 1-based key index.
        let item = bx(&1u32.to_be_bytes(), &bx(b"data", &data_body));
        let ilst = bx(b"ilst", &item);

        let mut meta_body = vec![0u8; 4];
        meta_body.extend_from_slice(&keys);
        meta_body.extend_from_slice(&ilst);
        bx(b"moov", &bx(b"udta", &bx(b"meta", &meta_body)))
    }

    /// A `moov/udta` holding a bare QuickTime (c)cmt atom, which is what ffmpeg
    /// writes for a standard comment when `use_metadata_tags` is not set.
    fn moov_with_qt_comment(text: &str) -> Vec<u8> {
        let mut body = (text.len() as u16).to_be_bytes().to_vec();
        body.extend_from_slice(&0u16.to_be_bytes()); // language
        body.extend_from_slice(text.as_bytes());
        bx(b"moov", &bx(b"udta", &bx(b"\xa9cmt", &body)))
    }

    #[test]
    fn reads_an_mdta_comment() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&moov_with_mdta_comment("{\"a\":\"テスト\"}"));
        buf.extend_from_slice(&bx(b"mdat", b"frames"));

        assert_eq!(
            read_udta_comment(&buf).as_deref(),
            Some("{\"a\":\"テスト\"}")
        );
    }

    #[test]
    fn falls_back_to_a_quicktime_comment_atom() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&moov_with_qt_comment("{\"b\":1}"));

        assert_eq!(read_udta_comment(&buf).as_deref(), Some("{\"b\":1}"));
    }

    #[test]
    fn returns_none_when_there_is_no_comment() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&bx(b"moov", &bx(b"udta", b"")));
        buf.extend_from_slice(&bx(b"mdat", b"frames"));

        assert!(read_udta_comment(&buf).is_none());
    }

    #[test]
    fn returns_none_for_a_discord_scrubbed_udta() {
        // Discord renames every child of udta to `skip` and zeroes the payload
        // in place, preserving the file size and every byte offset.
        let mut buf = ftyp(b"isom");
        let mut scrubbed = moov_with_mdta_comment("{\"a\":1}");
        // Rename the `meta` child of `udta` to `skip` and zero what follows.
        let needle = b"meta";
        let at = scrubbed
            .windows(4)
            .position(|w| w == needle)
            .expect("fixture contains a meta box");
        scrubbed[at..at + 4].copy_from_slice(b"skip");
        for byte in scrubbed[at + 4..].iter_mut() {
            *byte = 0;
        }
        buf.extend_from_slice(&scrubbed);

        assert!(read_udta_comment(&buf).is_none());
    }

    #[test]
    fn refuses_a_comment_larger_than_the_payload_cap() {
        let huge = "x".repeat(MAX_PAYLOAD + 1);
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&moov_with_mdta_comment(&huge));

        assert!(read_udta_comment(&buf).is_none());
    }

    /// A minimal AVIF: `ftyp`, a `meta` declaring one Exif item, and an `mdat`
    /// holding the TIFF blob the item's `iloc` extent points at.
    fn avif_with_exif(tiff: &[u8]) -> Vec<u8> {
        // The Exif item payload is a 4-byte tiff_header_offset then the TIFF.
        let mut item_payload = 0u32.to_be_bytes().to_vec();
        item_payload.extend_from_slice(tiff);

        // infe v2: version(1) flags(3) item_ID(u16) protection(u16) type(4) name
        let mut infe_body = vec![2u8, 0, 0, 0];
        infe_body.extend_from_slice(&1u16.to_be_bytes()); // item_ID 1
        infe_body.extend_from_slice(&0u16.to_be_bytes());
        infe_body.extend_from_slice(b"Exif");
        infe_body.push(0); // empty item name
        let infe = bx(b"infe", &infe_body);

        let mut iinf_body = vec![0u8, 0, 0, 0]; // version 0, flags
        iinf_body.extend_from_slice(&1u16.to_be_bytes()); // entry_count
        iinf_body.extend_from_slice(&infe);
        let iinf = bx(b"iinf", &iinf_body);

        // iloc v1, offset_size 4, length_size 4, base_offset_size 0, index 0
        let mut iloc_body = vec![1u8, 0, 0, 0];
        iloc_body.push(0x44); // offset_size 4 | length_size 4
        iloc_body.push(0x00); // base_offset_size 0 | index_size 0
        iloc_body.extend_from_slice(&1u16.to_be_bytes()); // item_count
        iloc_body.extend_from_slice(&1u16.to_be_bytes()); // item_ID 1
        iloc_body.extend_from_slice(&0u16.to_be_bytes()); // construction_method 0
        iloc_body.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_body.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        let offset_placeholder = iloc_body.len();
        iloc_body.extend_from_slice(&0u32.to_be_bytes()); // extent_offset, patched
        iloc_body.extend_from_slice(&(item_payload.len() as u32).to_be_bytes());

        let mut meta_body = vec![0u8; 4]; // meta FullBox preamble
        meta_body.extend_from_slice(&iinf);
        let iloc_at_in_meta = meta_body.len();
        meta_body.extend_from_slice(&bx(b"iloc", &iloc_body));

        let head = ftyp(b"avif");
        let meta = bx(b"meta", &meta_body);
        // mdat payload starts 8 bytes into the mdat box.
        let extent_offset = (head.len() + meta.len() + 8) as u32;

        let mut out = head;
        let mut meta = meta;
        // 8 for the meta header + 4 for its FullBox preamble is already folded
        // into iloc_at_in_meta, which is relative to meta_body.
        let patch = 8 + iloc_at_in_meta + 8 + offset_placeholder;
        meta[patch..patch + 4].copy_from_slice(&extent_offset.to_be_bytes());
        out.extend_from_slice(&meta);
        out.extend_from_slice(&bx(b"mdat", &item_payload));
        out
    }

    #[test]
    fn reads_an_avif_exif_item() {
        let tiff = b"II\x2a\x00\x08\x00\x00\x00 exif body";
        let buf = avif_with_exif(tiff);

        assert_eq!(read_avif_exif(&buf).as_deref(), Some(&tiff[..]));
    }

    #[test]
    fn returns_none_when_an_avif_has_no_exif_item() {
        let mut buf = ftyp(b"avif");
        let mut meta_body = vec![0u8; 4];
        meta_body.extend_from_slice(&bx(b"iinf", &[0u8, 0, 0, 0, 0, 0]));
        buf.extend_from_slice(&bx(b"meta", &meta_body));

        assert!(read_avif_exif(&buf).is_none());
    }

    #[test]
    fn returns_none_when_an_extent_points_past_the_file() {
        let tiff = b"II\x2a\x00\x08\x00\x00\x00";
        let mut buf = avif_with_exif(tiff);
        // Truncate the mdat away; the extent now points past the end.
        buf.truncate(buf.len() - 4);

        assert!(read_avif_exif(&buf).is_none());
    }

    #[test]
    fn appends_a_uuid_box_without_moving_a_byte() {
        let mut original = ftyp(b"isom");
        original.extend_from_slice(&bx(b"mdat", b"frames"));
        original.extend_from_slice(&bx(b"moov", b"index"));

        let out = append_uuid_xmp(&original, "{\"seed\":\"7\"}").unwrap();

        assert_eq!(&out[..original.len()], &original[..]);
        let kinds: Vec<[u8; 4]> = boxes(&out).iter().map(|b| b.kind).collect();
        assert_eq!(kinds, vec![*b"ftyp", *b"mdat", *b"moov", *b"uuid"]);
    }

    #[test]
    fn recovers_the_payload_it_appended() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&bx(b"moov", b"index"));

        let json = "{\"prompt\":\"a <fox> & a \\\"hound\\\"\",\"note\":\"テスト\"}";
        let out = append_uuid_xmp(&buf, json).unwrap();

        assert_eq!(read_uuid_xmp(&out).as_deref(), Some(json));
    }

    #[test]
    fn replaces_its_own_trailing_uuid_rather_than_stacking() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&bx(b"moov", b"index"));

        let once = append_uuid_xmp(&buf, "{\"seed\":\"1\"}").unwrap();
        let twice = append_uuid_xmp(&once, "{\"seed\":\"2\"}").unwrap();

        assert_eq!(
            boxes(&twice).iter().filter(|b| b.kind == *b"uuid").count(),
            1
        );
        assert_eq!(read_uuid_xmp(&twice).as_deref(), Some("{\"seed\":\"2\"}"));
    }

    #[test]
    fn refuses_to_append_to_non_isobmff() {
        assert!(append_uuid_xmp(b"\x89PNG\r\n\x1a\nnot a box", "{}").is_none());
        assert!(append_uuid_xmp(b"", "{}").is_none());
    }

    #[test]
    fn returns_none_when_there_is_no_uuid_box() {
        let mut buf = ftyp(b"isom");
        buf.extend_from_slice(&bx(b"moov", b"index"));

        assert!(read_uuid_xmp(&buf).is_none());
    }

    #[test]
    fn ignores_a_uuid_box_with_a_foreign_identifier() {
        let mut buf = ftyp(b"isom");
        let mut body = [0x11u8; 16].to_vec();
        body.extend_from_slice(b"<x>not ours</x>");
        buf.extend_from_slice(&bx(b"uuid", &body));

        assert!(read_uuid_xmp(&buf).is_none());
    }
}
