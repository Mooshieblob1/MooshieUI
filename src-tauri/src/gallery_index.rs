//! Write-only SQLite index of gallery images.
//!
//! The index mirrors per-image metadata into a queryable form so future
//! features (search UI, sidecar-less metadata browsing, duplicate detection)
//! can read it without walking the gallery directory and parsing every
//! image. At present there is **no reader** — writes are best-effort and
//! never block or fail the save/delete paths.
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
    )
}

fn format_label(fmt: ImageFormat) -> &'static str {
    match fmt {
        ImageFormat::Png => "png",
        ImageFormat::Jxl => "jxl",
        ImageFormat::Unknown => "unknown",
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
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
                p.cfg = obj.get("cfgscale").and_then(|x| x.as_f64());
                p.steps = obj.get("steps").and_then(|x| x.as_i64());
                p.seed = obj.get("seed").and_then(|x| x.as_i64());
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
