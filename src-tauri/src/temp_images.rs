//! Ephemeral image storage for SSE delivery.
//!
//! When output/preview images arrive over the ComfyUI WebSocket they can be
//! multi-megabyte PNGs.  Sending these inline (base64) through SSE causes
//! reverse proxies (Cloudflare, nginx) to drop the event.
//!
//! Instead we write the raw bytes to a temp directory and send a lightweight
//! SSE event containing just the filename.  Browser clients then fetch the
//! image via `GET /internal-api/_temp_image/{filename}`.

use std::path::PathBuf;
use std::sync::OnceLock;

static TEMP_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Initialise (and clean) the temp image directory.
/// Must be called once during app startup.
pub fn init() {
    let dir = temp_dir();
    // Clean any leftovers from a previous session
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let _ = std::fs::create_dir_all(&dir);
}

/// Return the temp image directory, creating it if needed.
fn temp_dir() -> PathBuf {
    TEMP_DIR
        .get_or_init(|| {
            let base = crate::config::app_data_dir()
                .unwrap_or_else(|| std::env::temp_dir().join("mooshieui"));
            base.join("temp_images")
        })
        .clone()
}

/// Save raw image bytes and return a unique filename.
/// The extension is derived from the data format.
pub fn save(data: &[u8], ext: &str) -> Option<String> {
    let dir = temp_dir();
    let _ = std::fs::create_dir_all(&dir);

    let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    let path = dir.join(&filename);
    match std::fs::write(&path, data) {
        Ok(()) => Some(filename),
        Err(e) => {
            log::error!("Failed to write temp image {}: {}", path.display(), e);
            None
        }
    }
}

/// Read a temp image by filename (returns bytes).
pub fn load(filename: &str) -> Option<Vec<u8>> {
    // Reject path traversal
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return None;
    }
    let path = temp_dir().join(filename);
    std::fs::read(&path).ok()
}

/// Delete a specific temp image (called after the browser has fetched it).
pub fn remove(filename: &str) {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return;
    }
    let path = temp_dir().join(filename);
    let _ = std::fs::remove_file(path);
}

/// Remove all temp images older than `max_age` seconds.
pub fn cleanup(max_age_secs: u64) {
    let dir = temp_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(max_age_secs);
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if modified < cutoff {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}
