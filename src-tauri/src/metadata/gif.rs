//! GIF Comment Extension reading.
//!
//! Pillow writes the comment right after the global colour table, but nothing
//! in the format requires that, so this walks the block stream properly and
//! skips image data rather than guessing at an offset.

/// Largest comment this will assemble, matching the ISOBMFF payload cap.
const MAX_PAYLOAD: usize = 1024 * 1024;

/// Most blocks walked before giving up on a file.
const MAX_BLOCKS: usize = 8192;

/// The first Comment Extension payload in a GIF, sub-blocks concatenated.
pub(super) fn read_comment(bytes: &[u8]) -> Option<String> {
    let mut off = header_end(bytes)?;

    for _ in 0..MAX_BLOCKS {
        match *bytes.get(off)? {
            // Trailer.
            0x3B => return None,
            // Extension introducer: label, then sub-blocks.
            0x21 => {
                let label = *bytes.get(off + 1)?;
                let (data, next) = sub_blocks(bytes, off + 2)?;
                if label == 0xFE {
                    return Some(String::from_utf8_lossy(&data).into_owned());
                }
                off = next;
            }
            // Image descriptor: 9 fixed bytes, an optional local colour table,
            // then the LZW minimum code size and the image's sub-blocks.
            0x2C => {
                let flags = *bytes.get(off + 9)?;
                let mut cursor = off + 10;
                if flags & 0x80 != 0 {
                    cursor += 3 * (1usize << ((flags & 0x07) + 1));
                }
                cursor += 1; // LZW minimum code size
                let (_, next) = sub_blocks(bytes, cursor)?;
                off = next;
            }
            _ => return None,
        }
    }
    None
}

/// Offset of the first block, past the signature, the logical screen
/// descriptor, and the global colour table if there is one.
fn header_end(bytes: &[u8]) -> Option<usize> {
    let sig = bytes.get(..6)?;
    if sig != b"GIF87a" && sig != b"GIF89a" {
        return None;
    }
    let flags = *bytes.get(10)?;
    let mut off = 13usize;
    if flags & 0x80 != 0 {
        off += 3 * (1usize << ((flags & 0x07) + 1));
    }
    if off > bytes.len() {
        return None;
    }
    Some(off)
}

/// Read a sub-block chain starting at `off`, returning the concatenated bytes
/// and the offset just past the terminating zero-length block.
fn sub_blocks(bytes: &[u8], mut off: usize) -> Option<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    for _ in 0..MAX_BLOCKS {
        let len = *bytes.get(off)? as usize;
        off += 1;
        if len == 0 {
            return Some((out, off));
        }
        let chunk = bytes.get(off..off + len)?;
        if out.len() + chunk.len() > MAX_PAYLOAD {
            return None;
        }
        out.extend_from_slice(chunk);
        off += len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GIF89a header, logical screen descriptor, and a global colour table of
    /// two entries, followed by `blocks` and the trailer.
    fn gif(blocks: &[u8]) -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&4u16.to_le_bytes()); // width
        out.extend_from_slice(&4u16.to_le_bytes()); // height
        out.push(0x80); // global colour table present, size 2^1
        out.push(0); // background colour index
        out.push(0); // pixel aspect ratio
        out.extend_from_slice(&[0, 0, 0, 255, 255, 255]); // the table
        out.extend_from_slice(blocks);
        out.push(0x3B); // trailer
        out
    }

    /// A comment extension whose payload is split into 255-byte sub-blocks,
    /// which is what Pillow emits for anything longer than that.
    fn comment_ext(text: &[u8]) -> Vec<u8> {
        let mut out = vec![0x21, 0xFE];
        for chunk in text.chunks(255) {
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
        out.push(0); // block terminator
        out
    }

    /// A minimal image descriptor with one empty LZW data sub-block, so the
    /// walker has to skip past real image data to reach anything after it.
    fn image_block() -> Vec<u8> {
        let mut out = vec![0x2C];
        out.extend_from_slice(&[0, 0, 0, 0]); // left, top
        out.extend_from_slice(&[4, 0, 4, 0]); // width, height
        out.push(0); // no local colour table
        out.push(2); // LZW minimum code size
        out.push(1); // one data sub-block
        out.push(0x00);
        out.push(0); // block terminator
        out
    }

    #[test]
    fn reads_a_single_sub_block_comment() {
        let buf = gif(&comment_ext(b"{\"seed\":\"5\"}"));
        assert_eq!(read_comment(&buf).as_deref(), Some("{\"seed\":\"5\"}"));
    }

    #[test]
    fn joins_multiple_sub_blocks() {
        let long = "x".repeat(600);
        let buf = gif(&comment_ext(long.as_bytes()));
        assert_eq!(read_comment(&buf).as_deref(), Some(long.as_str()));
    }

    #[test]
    fn finds_a_comment_after_image_data() {
        let mut blocks = image_block();
        blocks.extend_from_slice(&comment_ext(b"trailing"));
        let buf = gif(&blocks);
        assert_eq!(read_comment(&buf).as_deref(), Some("trailing"));
    }

    #[test]
    fn returns_none_when_there_is_no_comment() {
        let buf = gif(&image_block());
        assert!(read_comment(&buf).is_none());
    }

    #[test]
    fn returns_none_for_a_truncated_file() {
        let buf = gif(&comment_ext(b"cut short"));
        for cut in [3usize, 8, 14, 18] {
            assert!(read_comment(&buf[..cut.min(buf.len())]).is_none());
        }
    }

    #[test]
    fn returns_none_when_a_sub_block_runs_past_the_end() {
        let mut buf = gif(&[]);
        buf.truncate(buf.len() - 1); // drop the trailer
        buf.extend_from_slice(&[0x21, 0xFE, 200]); // claims 200 bytes, has none
        assert!(read_comment(&buf).is_none());
    }

    #[test]
    fn returns_none_for_bytes_that_are_not_a_gif() {
        assert!(read_comment(b"\x89PNG\r\n\x1a\n").is_none());
        assert!(read_comment(b"").is_none());
    }
}
