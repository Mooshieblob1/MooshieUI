use sha2::{Digest, Sha256};

/// Collapse volatile parts of a message so near-identical errors share a signature.
pub fn normalize(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_space = false;
    let mut prev_hash = false;
    for ch in lower.chars() {
        let mapped = if ch.is_ascii_hexdigit() || ch == '/' || ch == '\\' || ch == ':' {
            '#'
        } else if ch.is_whitespace() {
            ' '
        } else {
            ch
        };
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

pub fn body_has_marker(body: &str, sig: &str) -> bool {
    body.contains(&marker(sig))
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
    fn marker_roundtrips() {
        let sig = signature("generic", "x");
        let body = format!("some body\n{}", marker(&sig));
        assert!(body_has_marker(&body, &sig));
        assert!(!body_has_marker("no marker here", &sig));
    }
}
