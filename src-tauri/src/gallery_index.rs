//! Write-only SQLite index of gallery images.
//!
//! The index mirrors per-image metadata into a queryable form so future
//! features (search UI, sidecar-less metadata browsing, duplicate detection)
//! can read it without walking the gallery directory and parsing every
//! image. Writes are best-effort and never block or fail the save/delete
//! paths. The only reader is `video_durations()`, used by the gallery
//! listing; it is equally best-effort and degrades to an empty map.
//!
//! The DB lives at `{gallery_dir}/index.sqlite` and is opened lazily on first
//! use, and reopened whenever the gallery directory changes (Settings can
//! point the gallery elsewhere at runtime), so rows always land in the index
//! of the gallery they describe. FTS5 provides full-text search over prompt,
//! negative_prompt, and checkpoint.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::metadata::ImageFormat;

/// The open index and the gallery directory it belongs to. `conn` is `None`
/// when opening that gallery's index failed; it is retried only once the
/// gallery directory changes, so a broken index is not reopened per call.
struct IndexDb {
    dir: PathBuf,
    conn: Option<Connection>,
}

static DB: Mutex<Option<IndexDb>> = Mutex::new(None);

fn open_index(dir: &Path) -> Option<Connection> {
    let path = dir.join("index.sqlite");
    let _ = std::fs::create_dir_all(dir);
    match Connection::open(&path) {
        Ok(c) => {
            if let Err(e) = init_schema(&c) {
                log::warn!("gallery_index: schema init failed: {e}; index disabled");
                return None;
            }
            log::info!("gallery_index: opened at {}", path.display());
            Some(c)
        }
        Err(e) => {
            log::warn!(
                "gallery_index: failed to open {}: {e}; index disabled",
                path.display()
            );
            None
        }
    }
}

/// Run `f` on the index of gallery `dir`, (re)opening `slot` when it holds
/// another gallery's index. `None` when the index is unavailable.
fn with_conn_in<T>(
    slot: &mut Option<IndexDb>,
    dir: &Path,
    f: impl FnOnce(&Connection, &Path) -> T,
) -> Option<T> {
    if slot.as_ref().is_none_or(|db| db.dir != dir) {
        *slot = Some(IndexDb {
            dir: dir.to_path_buf(),
            conn: open_index(dir),
        });
    }
    let IndexDb { dir, conn } = slot.as_ref()?;
    conn.as_ref().map(|c| f(c, dir))
}

/// Run `f` on the index of the current gallery directory. Best-effort: `None`
/// when the gallery directory or its index is unavailable.
fn with_conn<T>(f: impl FnOnce(&Connection, &Path) -> T) -> Option<T> {
    let dir = crate::config::gallery_dir()?;
    let Ok(mut slot) = DB.lock() else {
        log::warn!("gallery_index: mutex poisoned, skipping");
        return None;
    };
    with_conn_in(&mut slot, &dir, f)
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

/// Parse image metadata into queryable columns: the embedded SwarmUI
/// `sui_image_params` JSON, or the flat key map the save commands receive.
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
    // What the save commands actually pass: the frontend's flat map
    // (`buildPngMetadata`), which only becomes `sui_image_params` inside the
    // file. Fills whatever the SwarmUI JSON above did not.
    let flat = |key: &str| {
        metadata
            .get(key)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    p.prompt = p.prompt.or_else(|| flat("positive_prompt"));
    p.negative_prompt = p.negative_prompt.or_else(|| flat("negative_prompt"));
    p.checkpoint = p.checkpoint.or_else(|| flat("model"));
    p.sampler = p.sampler.or_else(|| flat("sampler"));
    p.scheduler = p.scheduler.or_else(|| flat("scheduler"));
    p.cfg = p.cfg.or_else(|| flat("cfg").and_then(|v| v.parse().ok()));
    p.steps = p
        .steps
        .or_else(|| flat("steps").and_then(|v| v.parse().ok()));
    p.seed = p.seed.or_else(|| flat("seed").and_then(|v| v.parse().ok()));
    if p.width.is_none() || p.height.is_none() {
        if let Some((w, h)) = flat("size").as_deref().and_then(|s| s.split_once('x')) {
            p.width = p.width.or_else(|| w.trim().parse().ok());
            p.height = p.height.or_else(|| h.trim().parse().ok());
        }
    }
    p.bit_depth = p.bit_depth.or_else(|| flat("bit_depth"));
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
    with_conn(|c, _| upsert_in(c, path, file_size, format, metadata));
}

fn upsert_in(
    c: &Connection,
    path: &Path,
    file_size: u64,
    format: ImageFormat,
    metadata: Option<&HashMap<String, String>>,
) {
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
    with_conn(|c, _| upsert_video_in(c, path, file_size, meta));
}

fn upsert_video_in(c: &Connection, path: &Path, file_size: u64, meta: &VideoIndexMeta) {
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
    let video_str = video_path.to_string_lossy().to_string();
    let poster_str = poster_path.to_string_lossy().to_string();
    let res = with_conn(|c, _| {
        c.execute(
            "UPDATE images SET poster_path = ?1 WHERE path = ?2",
            params![poster_str, video_str],
        )
    });
    if let Some(Err(e)) = res {
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
    with_conn(|c, _| {
        c.query_row(
            "SELECT width, height FROM images WHERE path = ?1",
            [path],
            |row| Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32)),
        )
        .ok()
    })
    .flatten()
    .filter(|(w, h)| *w > 0 && *h > 0)
}

/// Source frame rate for one indexed video, by gallery path.
///
/// `None` when the row is unknown, predates the `fps` column, or stored a rate
/// the encoder never reported. Callers treat that as "unverifiable" rather than
/// substituting a default, because a wrong assumed rate is worse than no check.
pub fn video_fps(path: &str) -> Option<f64> {
    with_conn(|c, _| {
        c.query_row("SELECT fps FROM images WHERE path = ?1", [path], |row| {
            row.get::<_, Option<f64>>(0)
        })
        .ok()
        .flatten()
    })
    .flatten()
    .filter(|v| *v > 0.0)
}

/// The key a stored gallery path is matched on: its path relative to the
/// gallery root, `/`-separated (`clip.mp4`, `users/alice/clip.mp4`), and
/// whether it came from the current root (`true`) or had to be rebuilt.
///
/// Rows written before the gallery moved (the folder was relocated with its
/// index, or the setting now points at a copy) are not under `root`. The
/// gallery layout is `<root>/<file>` or `<root>/users/<name>/<file>`, so their
/// key is rebuilt from the tail of the stored path.
fn gallery_relative_key(root: &Path, stored: &Path) -> (String, bool) {
    let parts = |p: &Path| -> Vec<String> {
        p.components()
            .filter_map(|c| match c {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    if let Ok(rel) = stored.strip_prefix(root) {
        return (parts(rel).join("/"), true);
    }
    let parts = parts(stored);
    let key = match parts.as_slice() {
        [.., users, name, file] if users == "users" => format!("users/{name}/{file}"),
        [.., file] => file.clone(),
        [] => String::new(),
    };
    (key, false)
}

/// Pick the rows for files directly inside `dir` (the gallery root or one
/// user's folder under it) and key them by file name. A row stored under the
/// current root wins over a rebuilt legacy key for the same file.
fn video_meta_for_dir(
    root: &Path,
    dir: &Path,
    rows: impl IntoIterator<Item = (String, f64, Option<f64>)>,
) -> HashMap<String, VideoMeta> {
    let mut out = HashMap::new();
    let Ok(dir_rel) = dir.strip_prefix(root) else {
        return out;
    };
    let (dir_key, _) = gallery_relative_key(Path::new(""), dir_rel);
    let prefix = if dir_key.is_empty() {
        String::new()
    } else {
        format!("{dir_key}/")
    };
    for (path, duration_seconds, fps) in rows {
        let (key, exact) = gallery_relative_key(root, Path::new(&path));
        let Some(name) = key.strip_prefix(&prefix) else {
            continue;
        };
        if name.is_empty() || name.contains('/') {
            continue;
        }
        let meta = VideoMeta {
            duration_seconds,
            // A stored 0.0 is meaningless as a frame rate; treat it as absent.
            fps: fps.filter(|v| *v > 0.0),
        };
        if exact {
            out.insert(name.to_string(), meta);
        } else {
            out.entry(name.to_string()).or_insert(meta);
        }
    }
    out
}

/// Playback metadata for the videos directly inside `dir` (the gallery root,
/// or one user's folder in LAN mode), keyed by file name. One query for the
/// whole video table beats a lookup per directory entry.
///
/// Rows are matched by their path relative to the gallery root, not by bare
/// file name, so one user's listing never picks up another user's (or the
/// root's) video that happens to share a name.
///
/// Best-effort like every other function here: an unavailable or broken index
/// yields an empty map and the listing simply shows no duration badges.
pub fn video_meta(dir: &Path) -> HashMap<String, VideoMeta> {
    with_conn(|c, root| video_meta_in(c, root, dir)).unwrap_or_default()
}

fn video_meta_in(c: &Connection, root: &Path, dir: &Path) -> HashMap<String, VideoMeta> {
    let out = HashMap::new();
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
    video_meta_for_dir(root, dir, rows.flatten())
}

/// One indexed image, as the MCP gallery tools report it.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IndexedImage {
    pub filename: String,
    pub prompt: Option<String>,
    pub negative_prompt: Option<String>,
    pub model: Option<String>,
    pub seed: Option<i64>,
    pub steps: Option<i64>,
    pub cfg: Option<f64>,
    pub sampler: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

const INDEXED_COLUMNS: &str =
    "path, prompt, negative_prompt, checkpoint, seed, steps, cfg, sampler, width, height";

fn indexed_image(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, IndexedImage)> {
    let path: String = row.get(0)?;
    let filename = Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((
        path,
        IndexedImage {
            filename,
            prompt: row.get(1)?,
            negative_prompt: row.get(2)?,
            model: row.get(3)?,
            seed: row.get(4)?,
            steps: row.get(5)?,
            cfg: row.get(6)?,
            sampler: row.get(7)?,
            width: row.get(8)?,
            height: row.get(9)?,
        },
    ))
}

/// Turn free text into an FTS5 query: every word must appear, each one as a
/// quoted prefix so `cat` also finds `cats`. FTS syntax in the input (quotes,
/// `OR`, `NEAR`, column filters) is neutralised. `None` when nothing is left.
fn fts_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .take(16)
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Full-text search over prompt, negative prompt and model for the images
/// directly inside `dir`, best match first.
pub fn search(dir: &Path, text: &str, limit: usize) -> Vec<IndexedImage> {
    with_conn(|c, _| search_in(c, dir, text, limit)).unwrap_or_default()
}

fn search_in(c: &Connection, dir: &Path, text: &str, limit: usize) -> Vec<IndexedImage> {
    let Some(query) = fts_query(text) else {
        return Vec::new();
    };
    let sql = format!(
        "SELECT {} FROM images JOIN images_fts ON images_fts.rowid = images.id \
         WHERE images_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        INDEXED_COLUMNS
            .split(", ")
            .map(|col| format!("images.{col}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut stmt = match c.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("gallery_index: search prepare failed: {e}");
            return Vec::new();
        }
    };
    // Rows from users' folders share the index; over-fetch, then keep `dir`'s.
    let fetch = (limit.saturating_mul(4)).max(50) as i64;
    let rows = match stmt.query_map(params![query, fetch], indexed_image) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("gallery_index: search failed: {e}");
            return Vec::new();
        }
    };
    rows.flatten()
        .filter(|(path, _)| Path::new(path).parent() == Some(dir))
        .map(|(_, img)| img)
        .take(limit)
        .collect()
}

/// Index rows for the given files, keyed by full path. Files without a row
/// are simply missing from the map.
pub fn rows_for(paths: &[PathBuf]) -> HashMap<String, IndexedImage> {
    with_conn(|c, _| rows_for_in(c, paths)).unwrap_or_default()
}

fn rows_for_in(c: &Connection, paths: &[PathBuf]) -> HashMap<String, IndexedImage> {
    let sql = format!("SELECT {INDEXED_COLUMNS} FROM images WHERE path = ?1");
    let Ok(mut stmt) = c.prepare(&sql) else {
        return HashMap::new();
    };
    paths
        .iter()
        .filter_map(|p| {
            stmt.query_row([p.to_string_lossy().as_ref()], indexed_image)
                .ok()
        })
        .collect()
}

/// Update an image's path in the index after a rename. Best-effort: errors are
/// logged. Preserves the existing row (and its metadata/created_at) rather than
/// dropping and re-inserting.
pub fn rename(old_path: &Path, new_path: &Path) {
    let old_str = old_path.to_string_lossy().to_string();
    let new_str = new_path.to_string_lossy().to_string();
    let res = with_conn(|c, _| {
        c.execute(
            "UPDATE images SET path = ?1 WHERE path = ?2",
            params![new_str, old_str],
        )
    });
    if let Some(Err(e)) = res {
        log::warn!("gallery_index: rename failed for {old_str} -> {new_str}: {e}");
    }
}

/// Remove a gallery image from the index. Best-effort: errors are logged.
pub fn remove(path: &Path) {
    let path_str = path.to_string_lossy().to_string();
    let res = with_conn(|c, _| c.execute("DELETE FROM images WHERE path = ?1", params![path_str]));
    if let Some(Err(e)) = res {
        log::warn!("gallery_index: remove failed for {}: {e}", path_str);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mooshieui-gallery-index-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn row_count(c: &Connection) -> i64 {
        c.query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn index_follows_the_gallery_directory() {
        let base = scratch_dir("follow");
        let (old, new) = (base.join("old"), base.join("new"));
        let mut slot = None;
        with_conn_in(&mut slot, &old, |c, _| {
            upsert_in(c, &old.join("a.png"), 1, ImageFormat::Png, None)
        })
        .unwrap();
        // The gallery moves: the next write goes to the new gallery's index.
        with_conn_in(&mut slot, &new, |c, root| {
            assert_eq!(root, new.as_path());
            upsert_in(c, &new.join("b.png"), 1, ImageFormat::Png, None);
            assert_eq!(row_count(c), 1);
        })
        .unwrap();
        let old_db = Connection::open(old.join("index.sqlite")).unwrap();
        assert_eq!(row_count(&old_db), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    fn flat_metadata(prompt: &str) -> HashMap<String, String> {
        HashMap::from([
            ("positive_prompt".to_string(), prompt.to_string()),
            ("negative_prompt".to_string(), "blurry".to_string()),
            (
                "model".to_string(),
                "illustrious_v2.safetensors".to_string(),
            ),
            ("seed".to_string(), "42".to_string()),
            ("steps".to_string(), "28".to_string()),
            ("cfg".to_string(), "5.5".to_string()),
            ("size".to_string(), "832x1216".to_string()),
        ])
    }

    #[test]
    fn the_flat_save_map_fills_the_columns() {
        let p = extract_params(&flat_metadata("a cat"));
        assert_eq!(p.prompt.as_deref(), Some("a cat"));
        assert_eq!(p.checkpoint.as_deref(), Some("illustrious_v2.safetensors"));
        assert_eq!(p.seed, Some(42));
        assert_eq!(p.steps, Some(28));
        assert_eq!(p.cfg, Some(5.5));
        assert_eq!((p.width, p.height), (Some(832), Some(1216)));
    }

    #[test]
    fn fts_queries_are_quoted_prefixes() {
        assert_eq!(fts_query("red cat").as_deref(), Some("\"red\"* \"cat\"*"));
        assert_eq!(
            fts_query("\"x\" OR prompt:y").as_deref(),
            Some("\"x\"* \"OR\"* \"prompt\"* \"y\"*")
        );
        assert_eq!(fts_query("  ,,  "), None);
    }

    #[test]
    fn search_finds_prompts_in_the_listed_folder_only() {
        let root = scratch_dir("search");
        let alice = root.join("users").join("alice");
        let mut slot = None;
        with_conn_in(&mut slot, &root, |c, _| {
            let meta = flat_metadata("red cat on a sofa");
            upsert_in(c, &root.join("a.png"), 1, ImageFormat::Png, Some(&meta));
            upsert_in(c, &alice.join("b.png"), 1, ImageFormat::Png, Some(&meta));
            let other = flat_metadata("blue dog");
            upsert_in(c, &root.join("c.png"), 1, ImageFormat::Png, Some(&other));

            let hits = search_in(c, &root, "cats sofa", 10);
            assert!(hits.is_empty(), "every word must match: {hits:?}");
            let hits = search_in(c, &root, "red cat", 10);
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].filename, "a.png");
            assert_eq!(hits[0].seed, Some(42));
            let hits = search_in(c, &root, "illustrious", 10);
            assert_eq!(hits.len(), 2);

            let rows = rows_for_in(c, &[root.join("c.png"), root.join("missing.png")]);
            assert_eq!(rows.len(), 1);
            let c_png = root.join("c.png").to_string_lossy().into_owned();
            assert_eq!(rows[&c_png].prompt.as_deref(), Some("blue dog"));
        })
        .unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn video_meta_is_scoped_to_the_listed_folder() {
        let root = Path::new("/g");
        let row = |p: &str, d: f64| (p.to_string(), d, Some(24.0));
        let rows = || {
            vec![
                row("/g/a.mp4", 1.0),
                row("/g/users/alice/b.mp4", 2.0),
                row("/g/users/bob/a.mp4", 3.0),
                // Written before the gallery moved from /old/g.
                row("/old/g/c.mp4", 4.0),
                row("/old/g/users/alice/d.mp4", 5.0),
                row("/old/g/a.mp4", 9.0),
            ]
        };
        let names = |m: &HashMap<String, VideoMeta>| {
            let mut v: Vec<_> = m.keys().cloned().collect();
            v.sort();
            v
        };

        let top = video_meta_for_dir(root, root, rows());
        assert_eq!(names(&top), ["a.mp4", "c.mp4"]);
        assert_eq!(top["a.mp4"].duration_seconds, 1.0, "current root wins");

        let alice = video_meta_for_dir(root, &root.join("users").join("alice"), rows());
        assert_eq!(names(&alice), ["b.mp4", "d.mp4"]);

        let bob = video_meta_for_dir(root, &root.join("users").join("bob"), rows());
        assert_eq!(names(&bob), ["a.mp4"]);
        assert_eq!(bob["a.mp4"].duration_seconds, 3.0);

        assert!(video_meta_for_dir(root, Path::new("/elsewhere"), rows()).is_empty());
    }
}
