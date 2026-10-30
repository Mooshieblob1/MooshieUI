//! Hands ComfyUI's VRAM and RAM back after a stretch with no generations.
//!
//! ComfyUI keeps the last models in VRAM, and its RAM-pressure cache keeps
//! their weights and node outputs in RAM, until the next run or until memory
//! runs short. An app left open between sessions (or used only for NovelAI)
//! therefore holds several GB it is not using. After
//! `comfyui_idle_unload_minutes` without ComfyUI activity this asks every idle
//! managed worker to unload its models and clear its cache (`POST /free`).
//! ComfyUI itself keeps running; the next generation reloads what it needs.
//!
//! Activity is any `comfyui:*` event on the shared broadcast channel, so image,
//! video and music jobs from the UI, other LAN users and the MCP server all
//! count. A user's own ComfyUI (remote mode) is never touched.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;

use super::gpu_manager::WorkerStatus;
use crate::config::ServerMode;
use crate::state::AppState;

/// Default for `AppConfig::comfyui_idle_unload_minutes`.
pub const DEFAULT_IDLE_UNLOAD_MINUTES: u32 = 10;

/// How often the idle clock is checked.
const CHECK_EVERY: Duration = Duration::from_secs(30);

static STARTED: AtomicBool = AtomicBool::new(false);

/// Start the unloader once per process; later calls do nothing.
pub fn spawn(state: Arc<AppState>) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    #[cfg(feature = "desktop")]
    tauri::async_runtime::spawn(run(state));
    #[cfg(not(feature = "desktop"))]
    tokio::spawn(run(state));
}

/// Whether an idle stretch has run long enough to unload. `minutes == 0`
/// turns the feature off, and one unload per idle stretch is enough.
fn unload_due(idle_for: Duration, minutes: u32, already_unloaded: bool) -> bool {
    minutes > 0 && !already_unloaded && idle_for >= Duration::from_secs(u64::from(minutes) * 60)
}

async fn run(state: Arc<AppState>) {
    let mut events = state.event_tx.subscribe();
    let mut ticker = tokio::time::interval(CHECK_EVERY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_activity = Instant::now();
    // Nothing is loaded at startup; the first generation clears this.
    let mut unloaded = true;

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) if event.event.starts_with("comfyui:") => {
                    last_activity = Instant::now();
                    unloaded = false;
                }
                Ok(_) => {}
                // Missed events were certainly busy ones.
                Err(RecvError::Lagged(_)) => {
                    last_activity = Instant::now();
                    unloaded = false;
                }
                Err(RecvError::Closed) => return,
            },
            _ = ticker.tick() => {
                let (minutes, managed) = {
                    let config = state.config.read().await;
                    (
                        config.comfyui_idle_unload_minutes,
                        config.server_mode == ServerMode::AutoLaunch,
                    )
                };
                if !managed || !unload_due(last_activity.elapsed(), minutes, unloaded) {
                    continue;
                }
                if !state.prompt_queue.is_empty() || any_worker_busy(&state).await {
                    // Queued or running work: wait for its events to reset the clock.
                    continue;
                }
                unload_idle_workers(&state, minutes).await;
                unloaded = true;
            }
        }
    }
}

async fn any_worker_busy(state: &AppState) -> bool {
    for worker in &state.gpu_manager.workers {
        if worker.reserved.load(Ordering::Acquire)
            || *worker.status.read().await == WorkerStatus::Running
        {
            return true;
        }
    }
    false
}

async fn unload_idle_workers(state: &AppState, minutes: u32) {
    for worker in &state.gpu_manager.workers {
        if *worker.status.read().await != WorkerStatus::Idle {
            continue;
        }
        log::info!(
            "ComfyUI idle for {minutes} min: unloading models on worker {} to free VRAM and RAM",
            worker.id
        );
        let result = state
            .http_client
            .post(format!("{}/free", worker.base_url()))
            .json(&serde_json::json!({ "unload_models": true, "free_memory": true }))
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        if let Err(e) = result {
            log::warn!("Idle unload on worker {} failed: {e}", worker.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unload_due;
    use std::time::Duration;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn unloads_once_after_the_idle_time() {
        assert!(!unload_due(9 * MIN, 10, false));
        assert!(unload_due(10 * MIN, 10, false));
        assert!(unload_due(60 * MIN, 10, false));
        // Already unloaded this idle stretch.
        assert!(!unload_due(60 * MIN, 10, true));
    }

    #[test]
    fn zero_minutes_turns_it_off() {
        assert!(!unload_due(24 * 60 * MIN, 0, false));
    }
}
