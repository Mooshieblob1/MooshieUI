//! GIF Comment Extension reading.
//!
//! Pillow writes the comment right after the global colour table, but nothing
//! in the format requires that, so this walks the block stream properly and
//! skips image data rather than guessing at an offset.

/// Largest comment this will assemble, matching the ISOBMFF payload cap.
const MAX_PAYLOAD: usize = 1024 * 1024;

/// The first Comment Extension payload in a GIF, sub-blocks concatenated.
///
/// Everything that is not the comment is skipped without being copied, so an
/// animation of any length is walked in full: every block and sub-block
/// consumes at least one byte of the file, which bounds the walk by the file's
/// own length rather than by an arbitrary block count.
pub(super) fn read_comment(bytes: &[u8]) -> Option<String> {
    let mut off = header_end(bytes)?;

    // Each pass consumes at least one byte, so this can never spin.
    for _ in 0..bytes.len() {
        match *bytes.get(off)? {
            // Trailer.
            0x3B => return None,
            // Extension introducer: label, then sub-blocks.
            0x21 => {
                let label = *bytes.get(off + 1)?;
                if label == 0xFE {
                    let (data, _) = sub_blocks(bytes, off + 2)?;
                    return Some(String::from_utf8_lossy(&data).into_owned());
                }
                off = skip_sub_blocks(bytes, off + 2)?;
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
                off = skip_sub_blocks(bytes, cursor)?;
            }
            _ => return None,
        }
    }
    None
}

/// Offset just past the terminating zero-length block of the sub-block chain
/// starting at `off`, without copying any of it. Image data can run to
/// megabytes per frame; only the comment is ever assembled.
fn skip_sub_blocks(bytes: &[u8], mut off: usize) -> Option<usize> {
    loop {
        let len = *bytes.get(off)? as usize;
        off += 1;
        if len == 0 {
            return Some(off);
        }
        off += len;
        if off > bytes.len() {
            return None;
        }
    }
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
/// and the offset just past the terminating zero-length block. `None` once the
/// assembled payload would pass [`MAX_PAYLOAD`].
fn sub_blocks(bytes: &[u8], mut off: usize) -> Option<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    loop {
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

    /// An image descriptor whose LZW data is `sub_blocks` full 255-byte
    /// sub-blocks, standing in for a large real frame.
    fn big_image_block(sub_blocks: usize) -> Vec<u8> {
        let mut out = vec![0x2C];
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&[4, 0, 4, 0]);
        out.push(0);
        out.push(2);
        for _ in 0..sub_blocks {
            out.push(255);
            out.extend_from_slice(&[0xAA; 255]);
        }
        out.push(0);
        out
    }

    #[test]
    fn finds_a_comment_after_more_than_a_megabyte_of_image_data() {
        // ~1.3 MiB of LZW data in one frame: the old walker copied image data
        // through the comment assembler and gave up past 1 MiB.
        let mut blocks = big_image_block(5_200);
        blocks.extend_from_slice(&comment_ext(b"after a big frame"));
        let buf = gif(&blocks);
        assert_eq!(read_comment(&buf).as_deref(), Some("after a big frame"));
    }

    #[test]
    fn finds_a_comment_after_more_than_8192_blocks() {
        // A long animation: a graphic control extension plus a frame per
        // step, well past the old 8192-block ceiling.
        let mut blocks = Vec::new();
        for _ in 0..5_000 {
            blocks.extend_from_slice(&[0x21, 0xF9, 4, 0, 10, 0, 0, 0]);
            blocks.extend_from_slice(&image_block());
        }
        blocks.extend_from_slice(&comment_ext(b"{\"frames\":5000}"));
        let buf = gif(&blocks);
        assert_eq!(read_comment(&buf).as_deref(), Some("{\"frames\":5000}"));
    }

    #[test]
    fn a_comment_past_the_payload_cap_is_refused() {
        let huge = vec![b'x'; MAX_PAYLOAD + 1];
        assert!(read_comment(&gif(&comment_ext(&huge))).is_none());
    }

    #[test]
    fn image_data_running_past_the_end_is_refused() {
        let mut blocks = big_image_block(2);
        blocks.pop(); // drop the terminator
        blocks.push(200); // claims 200 more bytes, has none
        let mut buf = gif(&blocks);
        buf.pop(); // and no trailer either
        assert!(read_comment(&buf).is_none());
    }

    #[test]
    fn returns_none_for_bytes_that_are_not_a_gif() {
        assert!(read_comment(b"\x89PNG\r\n\x1a\n").is_none());
        assert!(read_comment(b"").is_none());
    }
}
