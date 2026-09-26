use sha2::{Digest, Sha256};

/// Collapse volatile parts of a message so near-identical errors share a signature.
///
/// Runs of decimal digits and `0x`-prefixed hex literals collapse to `#`, as do
/// path/drive separators. Ordinary words are left alone even when they happen to
/// be spelled with hex letters ("bad", "fed", "decode"), so unrelated errors do
/// not merge into one issue.
pub fn normalize(raw: &str) -> String {
    let lower: Vec<char> = raw.to_lowercase().chars().collect();
    let mut out = String::with_capacity(lower.len());
    let mut prev_space = false;
    let mut prev_hash = false;
    let mut i = 0;
    while i < lower.len() {
        let ch = lower[i];
        let mapped = if ch == '0'
            && lower.get(i + 1) == Some(&'x')
            && lower.get(i + 2).is_some_and(|c| c.is_ascii_hexdigit())
        {
            // 0x literal: consume the prefix and every hex digit after it.
            i += 2;
            while lower.get(i + 1).is_some_and(|c| c.is_ascii_hexdigit()) {
                i += 1;
            }
            '#'
        } else if ch.is_ascii_digit() {
            while lower.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                i += 1;
            }
            '#'
        } else if ch == '/' || ch == '\\' || ch == ':' {
            '#'
        } else if ch.is_whitespace() {
            ' '
        } else {
            ch
        };
        i += 1;
        if mapped == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
            prev_hash = false;
        } else if mapped == '#' {
            if !prev_hash {
                out.push('#');
            }
            prev_hash = true;
            prev_space = false;
        } else {
            out.push(mapped);
            prev_space = false;
            prev_hash = false;
        }
    }
    out.trim().to_string()
}

/// Stable short hex signature for an error, used for dedup markers.
pub fn signature(error_code: &str, raw_message: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(error_code.as_bytes());
    hasher.update(b"\n");
    hasher.update(normalize(raw_message).as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

pub fn marker(sig: &str) -> String {
    format!("<!-- mooshie-sig: {sig} -->")
}

/// True when the issue body ends with the proxy-appended marker for `sig`.
///
/// Only the final line counts: user-supplied text earlier in the body (note,
/// error message, logs) must not be able to claim a signature.
pub fn body_has_marker(body: &str, sig: &str) -> bool {
    body.trim_end()
        .lines()
        .next_back()
        .is_some_and(|last| last.trim() == marker(sig))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_stable_across_varying_numbers() {
        let a = signature(
            "out_of_memory",
            "CUDA OOM: tried to allocate 2048 MB at 0x7ff",
        );
        let b = signature(
            "out_of_memory",
            "CUDA OOM: tried to allocate 512 MB at 0x1ab",
        );
        assert_eq!(a, b, "digit/hex differences must not change the signature");
    }

    #[test]
    fn signature_differs_by_error_code() {
        let a = signature("out_of_memory", "same text");
        let b = signature("disk_full", "same text");
        assert_ne!(a, b);
    }

    #[test]
    fn signature_is_16_hex_chars() {
        let s = signature("generic", "anything");
        assert_eq!(s.len(), 16);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hex_letters_in_words_are_not_collapsed() {
        assert_eq!(normalize("bad value"), "bad value");
        assert_ne!(
            signature("generic", "bad input"),
            signature("generic", "fed input"),
            "words spelled with hex letters must keep their identity"
        );
    }

    #[test]
    fn digit_runs_and_hex_literals_collapse() {
        assert_eq!(normalize("tried 2048 MB at 0x7ffDEAD"), "tried # mb at #");
        assert_eq!(normalize("line 12, col 345"), "line #, col #");
        assert_eq!(normalize("C:\\Users\\x"), "c#users#x");
        // "0x" with no hex digits after it is an ordinary token.
        assert_eq!(normalize("0xg"), "#xg");
    }

    #[test]
    fn marker_roundtrips() {
        let sig = signature("generic", "x");
        let body = format!("some body\n{}", marker(&sig));
        assert!(body_has_marker(&body, &sig));
        assert!(!body_has_marker("no marker here", &sig));
        assert!(body_has_marker(&format!("{body}\r\n\n"), &sig));
    }

    #[test]
    fn marker_in_user_text_is_not_trusted() {
        let sig = signature("generic", "x");
        // A marker smuggled in earlier in the body (e.g. inside the user note)
        // must not match when the proxy's own last-line marker differs.
        let body = format!(
            "{}\nuser text\n{}",
            marker(&sig),
            marker("0000000000000000")
        );
        assert!(!body_has_marker(&body, &sig));
        assert!(!body_has_marker(&format!("x {}", marker(&sig)), &sig));
    }
}
