//! Minimal single-range `Range: bytes=...` parsing for mp4 serving.
//! Multi-range requests and non-byte units return None (caller sends 200).

/// Cap on the length of any single range the server hands back. A 15 s H3 mp4
/// can be hundreds of MB and the callers buffer the whole range in memory, so
/// never slurp the remainder. Every range is clamped to this, not only the
/// open-ended ones (`bytes=0-<huge>` and big suffix ranges would otherwise
/// allocate the whole file); players re-request as they play and follow a
/// shorter 206 with another Range request.
pub const OPEN_END_CHUNK: u64 = 8 * 1024 * 1024;

/// Parse a `Range` header against a resource of `len` bytes into an inclusive
/// `(start, end)` pair, per RFC 7233 (single range only). The returned range
/// never spans more than `OPEN_END_CHUNK` bytes.
pub fn parse(header: &str, len: u64) -> Option<(u64, u64)> {
    let (start, end) = parse_unclamped(header, len)?;
    Some((start, end.min(start.saturating_add(OPEN_END_CHUNK - 1))))
}

fn parse_unclamped(header: &str, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start_s, end_s) = spec.split_once('-')?;
    let (start_s, end_s) = (start_s.trim(), end_s.trim());
    if start_s.is_empty() {
        // Suffix range: last N bytes.
        let n: u64 = end_s.parse().ok()?;
        if n == 0 {
            return None;
        }
        return Some((len.saturating_sub(n), len - 1));
    }
    let start: u64 = start_s.parse().ok()?;
    if start >= len {
        return None;
    }
    // Open-ended: to EOF here; `parse` applies the chunk cap to every range.
    let end = if end_s.is_empty() {
        len - 1
    } else {
        end_s.parse::<u64>().ok()?.min(len - 1)
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_range() {
        assert_eq!(parse("bytes=0-1023", 4096), Some((0, 1023)));
    }

    #[test]
    fn open_range_is_capped() {
        assert_eq!(
            parse("bytes=100-", 1_000_000_000),
            Some((100, 99 + OPEN_END_CHUNK))
        );
    }

    #[test]
    fn open_range_clamps_to_len() {
        assert_eq!(parse("bytes=10-", 20), Some((10, 19)));
    }

    #[test]
    fn closed_range_is_capped() {
        assert_eq!(
            parse("bytes=0-999999999", 1_000_000_000),
            Some((0, OPEN_END_CHUNK - 1))
        );
        assert_eq!(
            parse(&format!("bytes=5-{}", u64::MAX), 1_000_000_000),
            Some((5, 4 + OPEN_END_CHUNK))
        );
    }

    #[test]
    fn suffix_range_is_capped() {
        let len = 1_000_000_000;
        assert_eq!(parse("bytes=-999999999", len), Some((1, OPEN_END_CHUNK)));
        // A suffix larger than the file starts at 0 and is still capped.
        assert_eq!(
            parse(&format!("bytes=-{}", u64::MAX), len),
            Some((0, OPEN_END_CHUNK - 1))
        );
    }

    #[test]
    fn range_exactly_at_the_cap_is_untouched() {
        assert_eq!(
            parse(&format!("bytes=0-{}", OPEN_END_CHUNK - 1), 1_000_000_000),
            Some((0, OPEN_END_CHUNK - 1))
        );
    }

    #[test]
    fn end_clamps_to_len() {
        assert_eq!(parse("bytes=0-999999", 100), Some((0, 99)));
    }

    #[test]
    fn suffix_range() {
        assert_eq!(parse("bytes=-500", 4096), Some((3596, 4095)));
    }

    #[test]
    fn rejects_start_past_eof() {
        assert_eq!(parse("bytes=4096-", 4096), None);
    }

    #[test]
    fn rejects_multi_range_and_garbage() {
        assert_eq!(parse("bytes=0-1,5-9", 4096), None);
        assert_eq!(parse("items=0-1", 4096), None);
        assert_eq!(parse("bytes=a-b", 4096), None);
        assert_eq!(parse("bytes=5-2", 4096), None);
        assert_eq!(parse("bytes=-0", 4096), None);
        assert_eq!(parse("bytes=0-", 0), None);
    }
}
