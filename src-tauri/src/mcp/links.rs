//! Download links for full-size images, so an assistant (or a chat bot it
//! posts for) can fetch the real file from the MooshieUI machine instead of a
//! preview.
//!
//! A link is `GET <base>/files/<expires>/<signature>/<kind>/<name>`: a gallery
//! file (`g`) or an Eagle item (`e`). The signature is an HMAC of the rest,
//! keyed from the MCP token, so a link can't be made for another file or kept
//! past its expiry, it never reveals the token itself, and pressing New token
//! in Settings voids every link handed out before.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path as AxumPath, State as AxumState};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::state::AppState;

/// How long a link works unless the tool asks otherwise.
pub const DEFAULT_HOURS: u64 = 24;
/// Longest a link may last.
pub const MAX_HOURS: u64 = 24 * 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Gallery,
    Eagle,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Self::Gallery => "g",
            Self::Eagle => "e",
        }
    }

    fn parse(code: &str) -> Option<Self> {
        match code {
            "g" => Some(Self::Gallery),
            "e" => Some(Self::Eagle),
            _ => None,
        }
    }
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

fn signature(token: &str, kind: Kind, name: &str, expires: u64) -> String {
    let key = hmac_sha256(token.as_bytes(), b"mooshieui-mcp-file-links");
    let mac = hmac_sha256(
        &key,
        format!("{}\n{name}\n{expires}", kind.code()).as_bytes(),
    );
    mac[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn encode(name: &str) -> String {
    percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// Where links point: the public address when the caller came through the
/// tunnel or the owner's own address, else this machine.
pub fn base_url(proxied: bool) -> Option<String> {
    let settings = super::current_settings();
    if proxied {
        let tunnel = super::tunnel::status();
        if tunnel.state == super::tunnel::TunnelState::Running {
            if let Some(url) = tunnel.url {
                return Some(url.trim_end_matches('/').to_string());
            }
        }
        return (!settings.public_url.is_empty()).then(|| settings.public_url.clone());
    }
    Some(format!("http://127.0.0.1:{}", settings.port))
}

/// A signed link to `name`, valid for `hours`.
pub fn make(base: &str, kind: Kind, name: &str, hours: u64) -> Option<String> {
    let token = super::current_settings().token;
    if token.is_empty() {
        return None;
    }
    let expires = now_secs() + hours.clamp(1, MAX_HOURS) * 3600;
    let sig = signature(&token, kind, name, expires);
    Some(format!(
        "{}/files/{expires}/{sig}/{}/{}",
        base.trim_end_matches('/'),
        kind.code(),
        encode(name)
    ))
}

fn verify(token: &str, kind: Kind, name: &str, expires: u64, sig: &str) -> bool {
    if token.is_empty() || expires < now_secs() {
        return false;
    }
    let expected = signature(token, kind, name, expires);
    bool::from(expected.as_bytes().ct_eq(sig.as_bytes()))
}

fn content_type(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    match lower.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    }
}

/// The file's bytes as served: browsers and chat apps can't show JPEG XL, so
/// those go out as PNG.
fn servable(path: &std::path::Path) -> Result<(Vec<u8>, &'static str, String), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.to_ascii_lowercase().ends_with(".jxl") {
        let img = crate::commands::api::decode_gallery_image(&bytes)?;
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let png_name = format!(
            "{}.png",
            name.trim_end_matches(".jxl").trim_end_matches(".JXL")
        );
        return Ok((out.into_inner(), "image/png", png_name));
    }
    let ct = content_type(&name);
    Ok((bytes, ct, name))
}

/// `GET /files/<expires>/<sig>/<kind>/<name>`.
pub async fn serve(
    AxumState(state): AxumState<Arc<AppState>>,
    AxumPath((expires, sig, kind, name)): AxumPath<(u64, String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let settings = super::current_settings();
    if !crate::webserver::host_header_is_local(&headers)
        && !super::host_is_public(&headers, &settings.public_url)
    {
        return (StatusCode::FORBIDDEN, "Host not allowed").into_response();
    }
    let Some(kind) = Kind::parse(&kind) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !verify(&settings.token, kind, &name, expires, &sig) {
        return (StatusCode::FORBIDDEN, "This link is wrong or has expired").into_response();
    }
    let path = match kind {
        Kind::Gallery => super::tools::gallery_dir()
            .and_then(|dir| super::tools::gallery_image_path(&dir, &name)),
        Kind::Eagle => crate::eagle::item_file(&state.http_client, &name)
            .await
            .map_err(|e| e.to_string()),
    };
    let path = match path {
        Ok(p) => p,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    match tokio::task::spawn_blocking(move || servable(&path)).await {
        Ok(Ok((bytes, ct, filename))) => (
            [
                (header::CONTENT_TYPE, ct.to_string()),
                (header::CACHE_CONTROL, "private, max-age=3600".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("inline; filename=\"{}\"", filename.replace('"', "")),
                ),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            ],
            Body::from(bytes),
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_the_rfc_4231_vector() {
        // RFC 4231 test case 2.
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn links_are_bound_to_file_token_and_expiry() {
        let exp = now_secs() + 60;
        let sig = signature("tok", Kind::Gallery, "a.png", exp);
        assert!(verify("tok", Kind::Gallery, "a.png", exp, &sig));
        assert!(!verify("tok", Kind::Gallery, "b.png", exp, &sig));
        assert!(!verify("tok", Kind::Eagle, "a.png", exp, &sig));
        assert!(!verify("other", Kind::Gallery, "a.png", exp, &sig));
        assert!(!verify("tok", Kind::Gallery, "a.png", exp + 1, &sig));
        assert!(!verify("", Kind::Gallery, "a.png", exp, &sig));
        let old = now_secs() - 1;
        let sig = signature("tok", Kind::Gallery, "a.png", old);
        assert!(!verify("tok", Kind::Gallery, "a.png", old, &sig));
    }

    #[test]
    fn names_are_escaped_in_the_path() {
        assert_eq!(encode("a b/c.png"), "a%20b%2Fc%2Epng");
        assert_eq!(content_type("X.WEBP"), "image/webp");
    }
}
