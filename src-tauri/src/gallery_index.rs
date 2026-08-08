//! Write-only SQLite index of gallery images.
//!
//! The index mirrors per-image metadata into a queryable form so future
//! features (search UI, sidecar-less metadata browsing, duplicate detection)
//! can read it without walking the gallery directory and parsing every
//! image. Writes are best-effort and never block or fail the save/delete
//! paths. The only reader is `video_durations()`, used by the gallery
//! listing; it is equally best-effort and degrades to an empty map.
//!
//! The DB lives at `{gallery_dir}/index.sqlite` and is initialized lazily on
//! first use. FTS5 provides full-text search over prompt, negative_prompt,
//! and checkpoint.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rusqlite::{params, Connection};

use crate::metadata::ImageFormat;

static DB: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();

fn db_path() -> Option<PathBuf> {
    crate::config::gallery_dir().map(|d| d.join("index.sqlite"))
}

fn conn() -> &'static Mutex<Option<Connection>> {
    DB.get_or_init(|| {
        let Some(path) = db_path() else {
            log::warn!("gallery_index: gallery_dir() unavailable, index disabled");
            return Mutex::new(None);
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match Connection::open(&path) {
            Ok(c) => {
                if let Err(e) = init_schema(&c) {
                    log::warn!("gallery_index: schema init failed: {e}; index disabled");
                    return Mutex::new(None);
                }
                log::info!("gallery_index: opened at {}", path.display());
                Mutex::new(Some(c))
            }
            Err(e) => {
                log::warn!(
                    "gallery_index: failed to open {}: {e}; index disabled",
                    path.display()
                );
                Mutex::new(None)
            }
        }
    })
}

fn init_schema(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;

        CREATE TABLE IF NOT EXISTS images (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            path            TEXT    NOT NULL UNIQUE,
            format          TEXT    NOT NULL,
            file_size       INTEGER NOT NULL DEFAULT 0,
            width           INTEGER,
            height          INTEGER,
            created_at      INTEGER NOT NULL,
            checkpoint      TEXT,
            sampler         TEXT,
            scheduler       TEXT,
            cfg             REAL,
            steps           INTEGER,
            seed            INTEGER,
            bit_depth       TEXT,
            prompt          TEXT,
            negative_prompt TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_images_created_at ON images(created_at);
        CREATE INDEX IF NOT EXISTS idx_images_checkpoint ON images(checkpoint);

        CREATE VIRTUAL TABLE IF NOT EXISTS images_fts USING fts5(
            prompt, negative_prompt, checkpoint,
            content='images', content_rowid='id'
        );

        CREATE TRIGGER IF NOT EXISTS images_ai AFTER INSERT ON images BEGIN
            INSERT INTO images_fts(rowid, prompt, negative_prompt, checkpoint)
            VALUES (new.id, new.prompt, new.negative_prompt, new.checkpoint);
        END;

        CREATE TRIGGER IF NOT EXISTS images_ad AFTER DELETE ON images BEGIN
            INSERT INTO images_fts(images_fts, rowid, prompt, negative_prompt, checkpoint)
            VALUES ('delete', old.id, old.prompt, old.negative_prompt, old.checkpoint);
        END;

        CREATE TRIGGER IF NOT EXISTS images_au AFTER UPDATE ON images BEGIN
            INSERT INTO images_fts(images_fts, rowid, prompt, negative_prompt, checkpoint)
            VALUES ('delete', old.id, old.prompt, old.negative_prompt, old.checkpoint);
            INSERT INTO images_fts(rowid, prompt, negative_prompt, checkpoint)
            VALUES (new.id, new.prompt, new.negative_prompt, new.checkpoint);
        END;
        "#,
    )?;
    apply_migrations(c);
    Ok(())
}

/// Additive, idempotent column migrations. SQLite has no ADD COLUMN IF NOT
/// EXISTS, so a re-run fails with "duplicate column name" — that exact error
/// is expected and swallowed; anything else is logged.
fn apply_migrations(c: &Connection) {
    for ddl in [
        "ALTER TABLE images ADD COLUMN media_type TEXT NOT NULL DEFAULT 'image'",
        "ALTER TABLE images ADD COLUMN duration_seconds REAL",
        "ALTER TABLE images ADD COLUMN fps REAL",
        "ALTER TABLE images ADD COLUMN poster_path TEXT",
    ] {
        if let Err(e) = c.execute_batch(ddl) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                log::warn!("gallery_index: migration failed ({ddl}): {msg}");
            }
        }
    }
}

fn format_label(fmt: ImageFormat) -> &'static str {
    match fmt {
        ImageFormat::Png => "png",
        ImageFormat::Jxl => "jxl",
        ImageFormat::WebP => "webp",
        ImageFormat::Mp4 => "mp4",
        ImageFormat::Avif => "avif",
        ImageFormat::Gif => "gif",
        ImageFormat::Unknown => "unknown",
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Read a JSON value as i64, accepting either a number or a decimal string.
fn json_i64(v: &serde_json::Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Read a JSON value as f64, accepting either a number or a decimal string.
fn json_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Parse the embedded SwarmUI `sui_image_params` JSON into queryable columns.
fn extract_params(metadata: &HashMap<String, String>) -> ParsedParams {
    let mut p = ParsedParams::default();
    if let Some(raw) = metadata.get("sui_image_params") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(obj) = v.as_object() {
                p.prompt = obj
                    .get("prompt")
                    .and_then(|x| x.as_str())
                    .map(str::to_owned);
                p.negative_prompt = obj
                    .get("negativeprompt")
                    .and_then(|x| x.as_str())
                    .map(str::to_owned);
                p.checkpoint = obj.get("model").and_then(|x| x.as_str()).map(str::to_owned);
                p.sampler = obj
                    .get("sampler")
                    .and_then(|x| x.as_str())
                    .map(str::to_owned);
                p.scheduler = obj
                    .get("scheduler")
                    .and_then(|x| x.as_str())
                    .map(str::to_owned);
                // MooshieUI writes these SwarmUI values as JSON strings (see
                // metadata::format_swarmui_json); SwarmUI itself uses numbers.
                p.cfg = obj.get("cfgscale").and_then(json_f64);
                p.steps = obj.get("steps").and_then(json_i64);
                p.seed = obj.get("seed").and_then(json_i64);
                p.width = obj.get("width").and_then(|x| x.as_i64());
                p.height = obj.get("height").and_then(|x| x.as_i64());
            }
        }
    }
    if let Some(extra_raw) = metadata.get("mooshie_extra") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(extra_raw) {
            if let Some(obj) = v.as_object() {
                if p.bit_depth.is_none() {
                    p.bit_depth = obj
                        .get("bit_depth")
                        .and_then(|x| x.as_str())
                        .map(str::to_owned);
                }
            }
        }
    }
    p
}

#[derive(Default)]
struct ParsedParams {
    prompt: Option<String>,
    negative_prompt: Option<String>,
    checkpoint: Option<String>,
    sampler: Option<String>,
    scheduler: Option<String>,
    cfg: Option<f64>,
    steps: Option<i64>,
    seed: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
    bit_depth: Option<String>,
}

/// Upsert a gallery image into the index. Best-effort: errors are logged, not returned.
pub fn upsert(
    path: &Path,
    file_size: u64,
    format: ImageFormat,
    metadata: Option<&HashMap<String, String>>,
) {
    let guard = conn().lock();
    let Ok(mut guard) = guard else {
        log::warn!("gallery_index: mutex poisoned, skipping upsert");
        return;
    };
    let Some(ref mut c) = *guard else {
        return; // DB disabled
    };

    let path_str = path.to_string_lossy().to_string();
    let params_parsed = metadata.map(extract_params).unwrap_or_default();

    let res = c.execute(
        r#"
        INSERT INTO images (
            path, format, file_size, width, height, created_at,
            checkpoint, sampler, scheduler, cfg, steps, seed, bit_depth,
            prompt, negative_prompt
        )
        VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15
        )
        ON CONFLICT(path) DO UPDATE SET
            format          = excluded.format,
            file_size       = excluded.file_size,
            width           = excluded.width,
            height          = excluded.height,
            checkpoint      = excluded.checkpoint,
            sampler         = excluded.sampler,
            scheduler       = excluded.scheduler,
            cfg             = excluded.cfg,
            steps           = excluded.steps,
            seed            = excluded.seed,
            bit_depth       = excluded.bit_depth,
            prompt          = excluded.prompt,
            negative_prompt = excluded.negative_prompt
        "#,
        params![
            path_str,
            format_label(format),
            file_size as i64,
            params_parsed.width,
            params_parsed.height,
            now_ms(),
            params_parsed.checkpoint,
            params_parsed.sampler,
            params_parsed.scheduler,
            params_parsed.cfg,
            params_parsed.steps,
            params_parsed.seed,
            params_parsed.bit_depth,
            params_parsed.prompt,
            params_parsed.negative_prompt,
        ],
    );

    if let Err(e) = res {
        log::warn!("gallery_index: upsert failed for {}: {e}", path_str);
    }
}

/// Metadata for an indexed video, computed by the save pipeline.
pub struct VideoIndexMeta {
    pub duration_seconds: f64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    /// Absolute path of the `{stem}_poster.webp` sidecar, if one was saved.
    pub poster_path: Option<String>,
}

/// Upsert a gallery video into the index. Best-effort: errors are logged, not
/// returned. Videos carry no embedded generation metadata in v1, so
/// prompt/seed/checkpoint stay NULL (enrichment is a later PR).
pub fn upsert_video(path: &Path, file_size: u64, meta: &VideoIndexMeta) {
    let guard = conn().lock();
    let Ok(mut guard) = guard else {
        log::warn!("gallery_index: mutex poisoned, skipping video upsert");
        return;
    };
    let Some(ref mut c) = *guard else {
        return; // DB disabled
    };

    let path_str = path.to_string_lossy().to_string();
    let res = c.execute(
        r#"
        INSERT INTO images (
            path, format, file_size, width, height, created_at,
            media_type, duration_seconds, fps, poster_path
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'video', ?7, ?8, ?9)
        ON CONFLICT(path) DO UPDATE SET
            format           = excluded.format,
            file_size        = excluded.file_size,
            width            = excluded.width,
            height           = excluded.height,
            media_type       = excluded.media_type,
            duration_seconds = excluded.duration_seconds,
            fps              = excluded.fps,
            poster_path      = excluded.poster_path
        "#,
        params![
            path_str,
            format_label(ImageFormat::Mp4),
            file_size as i64,
            meta.width as i64,
            meta.height as i64,
            now_ms(),
            meta.duration_seconds,
            meta.fps,
            meta.poster_path,
        ],
    );

    if let Err(e) = res {
        log::warn!("gallery_index: video upsert failed for {}: {e}", path_str);
    }
}

/// Point a video row at a new poster path after a rename. Best-effort.
pub fn update_poster_path(video_path: &Path, poster_path: &Path) {
    let guard = conn().lock();
    let Ok(guard) = guard else {
        return;
    };
    let Some(ref c) = *guard else {
        return;
    };
    let video_str = video_path.to_string_lossy().to_string();
    let poster_str = poster_path.to_string_lossy().to_string();
    if let Err(e) = c.execute(
        "UPDATE images SET poster_path = ?1 WHERE path = ?2",
        params![poster_str, video_str],
    ) {
        log::warn!("gallery_index: poster update failed for {video_str}: {e}");
    }
}

/// Playback metadata for one indexed video row.
#[derive(Debug, Clone, Copy)]
pub struct VideoMeta {
    pub duration_seconds: f64,
    /// `None` when the row predates the `fps` column or the encoder did not
    /// report a rate. Callers fall back to 24.
    pub fps: Option<f64>,
}

/// Pixel dimensions for one indexed video, by gallery path.
pub fn video_dimensions(path: &str) -> Option<(u32, u32)> {
    let guard = conn().lock().ok()?;
    let c = guard.as_ref()?;
    c.query_row(
        "SELECT width, height FROM images WHERE path = ?1",
        [path],
        |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32)),
    )
    .ok()
    .filter(|(w, h)| *w > 0 && *h > 0)
}

/// Source frame rate for one indexed video, by gallery path.
///
/// `None` when the row is unknown, predates the `fps` column, or stored a rate
/// the encoder never reported. Callers treat that as "unverifiable" rather than
/// substituting a default, because a wrong assumed rate is worse than no check.
pub fn video_fps(path: &str) -> Option<f64> {
    let guard = conn().lock().ok()?;
    let c = guard.as_ref()?;
    c.query_row("SELECT fps FROM images WHERE path = ?1", [path], |row| {
        row.get::<_, Option<f64>>(0)
    })
    .ok()
    .flatten()
    .filter(|v| *v > 0.0)
}

/// One query for the whole video table, keyed by gallery path.
///
/// This is the first **reader** on the index. The gallery listing needs a
/// duration badge per mp4, and one query for the whole table beats a lookup
/// per directory entry. Keyed by basename rather than by full path because
/// the listing walks a directory while the stored paths are absolute; gallery
/// video names embed a prompt UUID (`{promptId}__video__{stem}.mp4`), so
/// basenames are unique in practice.
///
/// Best-effort like every other function here: an unavailable or broken index
/// yields an empty map and the listing simply shows no duration badges.
pub fn video_meta() -> HashMap<String, VideoMeta> {
    let mut out = HashMap::new();
    let guard = conn().lock();
    let Ok(guard) = guard else {
        return out;
    };
    let Some(ref c) = *guard else {
        return out;
    };
    let mut stmt = match c.prepare(
        "SELECT path, duration_seconds, fps FROM images \
         WHERE media_type = 'video' AND duration_seconds IS NOT NULL",
    ) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("gallery_index: video meta query prepare failed: {e}");
            return out;
        }
    };
    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, Option<f64>>(2)?,
        ))
    }) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("gallery_index: video meta query failed: {e}");
            return out;
        }
    };
    for row in rows.flatten() {
        let name = Path::new(&row.0)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(row.0.as_str())
            .to_string();
        out.insert(
            name,
            VideoMeta {
                duration_seconds: row.1,
                // A stored 0.0 is meaningless as a frame rate; treat it as absent.
                fps: row.2.filter(|v| *v > 0.0),
            },
        );
    }
    out
}

/// Update an image's path in the index after a rename. Best-effort: errors are
/// logged. Preserves the existing row (and its metadata/created_at) rather than
/// dropping and re-inserting.
pub fn rename(old_path: &Path, new_path: &Path) {
    let guard = conn().lock();
    let Ok(guard) = guard else {
        return;
    };
    let Some(ref c) = *guard else {
        return;
    };
    let old_str = old_path.to_string_lossy().to_string();
    let new_str = new_path.to_string_lossy().to_string();
    if let Err(e) = c.execute(
        "UPDATE images SET path = ?1 WHERE path = ?2",
        params![new_str, old_str],
    ) {
        log::warn!("gallery_index: rename failed for {old_str} -> {new_str}: {e}");
    }
}

/// Remove a gallery image from the index. Best-effort: errors are logged.
pub fn remove(path: &Path) {
    let guard = conn().lock();
    let Ok(guard) = guard else {
        return;
    };
    let Some(ref c) = *guard else {
        return;
    };
    let path_str = path.to_string_lossy().to_string();
    if let Err(e) = c.execute("DELETE FROM images WHERE path = ?1", params![path_str]) {
        log::warn!("gallery_index: remove failed for {}: {e}", path_str);
    }
}
