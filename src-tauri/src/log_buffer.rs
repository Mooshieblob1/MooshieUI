//! Bounded in-memory ring buffers for diagnostic log capture.
//!
//! Two buffers are maintained:
//! - `rust`: fed by a custom `log::Log` implementation that also writes to
//!   stderr, so every `log::info!`/`warn!`/`error!` ends up in the buffer.
//! - `frontend`: fed explicitly by the `append_frontend_logs` command, which
//!   the UI calls when exporting diagnostics.
//!
//! Both buffers are capped and discard oldest entries first so they can't
//! grow unboundedly during long sessions.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

const MAX_LINES: usize = 2000;

static RUST_BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static FRONTEND_BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn rust_buf() -> &'static Mutex<VecDeque<String>> {
    RUST_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LINES)))
}

fn frontend_buf() -> &'static Mutex<VecDeque<String>> {
    FRONTEND_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LINES)))
}

fn push_bounded(buffer: &Mutex<VecDeque<String>>, line: String) {
    if let Ok(mut b) = buffer.lock() {
        while b.len() >= MAX_LINES {
            b.pop_front();
        }
        b.push_back(line);
    }
}

/// Append a batch of frontend log lines to the ring buffer.
pub fn push_frontend_lines<I, S>(lines: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let buf = frontend_buf();
    for line in lines {
        push_bounded(buf, line.into());
    }
}

/// Return a snapshot of the Rust-side log ring buffer.
pub fn snapshot_rust() -> Vec<String> {
    rust_buf()
        .lock()
        .map(|b| b.iter().cloned().collect())
        .unwrap_or_default()
}

/// Return a snapshot of the frontend log ring buffer.
pub fn snapshot_frontend() -> Vec<String> {
    frontend_buf()
        .lock()
        .map(|b| b.iter().cloned().collect())
        .unwrap_or_default()
}

struct RingLogger {
    level: LevelFilter,
}

impl Log for RingLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let ts = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f");
        let line = format!(
            "[{}] {:<5} {} - {}",
            ts,
            record.level(),
            record.target(),
            record.args()
        );
        // Best-effort stderr write — a closed stderr shouldn't crash logging.
        let _ = writeln!(std::io::stderr(), "{}", line);
        push_bounded(rust_buf(), line);
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

/// Initialise the ring-buffer-backed logger. Safe to call multiple times;
/// subsequent calls after the first are no-ops.
pub fn init() {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| {
            // Support simple `info` / `debug` / `warn` and env_logger-style
            // `mooshie=debug,hyper=warn` (take the first level we can parse).
            s.split(',')
                .map(|chunk| chunk.rsplit('=').next().unwrap_or(chunk).trim())
                .find_map(|lvl| lvl.parse::<Level>().ok())
        })
        .map(|lvl| lvl.to_level_filter())
        .unwrap_or(LevelFilter::Info);

    // set_boxed_logger fails if a logger is already installed; ignore that
    // case so tests / repeat calls don't panic.
    if log::set_boxed_logger(Box::new(RingLogger { level })).is_ok() {
        log::set_max_level(level);
    }
}
