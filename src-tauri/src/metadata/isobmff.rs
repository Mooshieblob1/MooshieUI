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
}
