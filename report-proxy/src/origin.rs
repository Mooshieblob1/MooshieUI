//! Which browser origins may post reports (see RUNBOOK.md, "Origin policy").
//!
//! The desktop app posts from its Tauri webview. Browser mode posts from wherever
//! the user's own MooshieUI server is reachable (a LAN address, localhost, a
//! tunnel domain), which no allowlist can name, so a web origin is accepted but
//! draws on its own small share of the GitHub write budget. That keeps a page
//! making its visitors' browsers post reports from crowding out the app. Opaque
//! and non-web origins are refused outright.

use axum::http::HeaderValue;

/// Origins of the desktop webview: `tauri://localhost` on macOS/Linux,
/// `http://tauri.localhost` on Windows (`https://` with `useHttpsScheme`).
pub const TAURI_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginClass {
    /// No Origin header: not a browser (curl, scripts). The app header gate and
    /// the budgets still apply; CORS never protected against these.
    Absent,
    /// The desktop app's webview.
    App,
    /// Any other http(s) origin: browser mode, or some other website.
    Web,
    /// `null` (sandboxed frames, file: pages), extension and other schemes, or a
    /// malformed value. Never a MooshieUI client.
    Rejected,
}

pub fn classify(origin: Option<&HeaderValue>) -> OriginClass {
    let Some(origin) = origin else {
        return OriginClass::Absent;
    };
    let Ok(origin) = origin.to_str() else {
        return OriginClass::Rejected;
    };
    if TAURI_ORIGINS.contains(&origin) {
        OriginClass::App
    } else if is_web_origin(origin) {
        OriginClass::Web
    } else {
        OriginClass::Rejected
    }
}

/// CORS predicate: the preflight succeeds only for the app and web origins.
pub fn allowed_by_cors(origin: &HeaderValue) -> bool {
    matches!(classify(Some(origin)), OriginClass::App | OriginClass::Web)
}

/// A serialized http(s) origin: scheme, host and optional port, nothing else.
fn is_web_origin(origin: &str) -> bool {
    let Some(authority) = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
    else {
        return false;
    };
    !authority.is_empty()
        && authority
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(origin: &str) -> OriginClass {
        classify(Some(&HeaderValue::from_str(origin).unwrap()))
    }

    #[test]
    fn desktop_webview_origins_are_the_app() {
        for origin in TAURI_ORIGINS {
            assert_eq!(class(origin), OriginClass::App, "{origin}");
        }
    }

    #[test]
    fn browser_mode_hosts_are_web_origins() {
        for origin in [
            "http://localhost:3200",
            "http://192.168.1.20:3200",
            "http://[::1]:3200",
            "https://mooshie.example.com",
            "https://xn--mooshi-gva.example",
        ] {
            assert_eq!(class(origin), OriginClass::Web, "{origin}");
        }
    }

    #[test]
    fn opaque_and_foreign_origins_are_rejected() {
        for origin in [
            "null",
            "",
            "file://",
            "chrome-extension://abcdefgh",
            "moz-extension://1234",
            "tauri://evil.example",
            "https://",
            "https://user@host.example",
            "https://host.example/path",
        ] {
            assert_eq!(class(origin), OriginClass::Rejected, "{origin:?}");
        }
        let binary = HeaderValue::from_bytes(b"https://h\xffst").unwrap();
        assert_eq!(classify(Some(&binary)), OriginClass::Rejected);
    }

    #[test]
    fn missing_origin_is_not_a_browser() {
        assert_eq!(classify(None), OriginClass::Absent);
    }
}
