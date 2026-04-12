//! Multi-GPU worker manager — distributes ComfyUI prompts across N GPU backends.
//!
//! Adapted from SwarmUI's `BackendHandler` pattern:
//! - Each GPU gets its own ComfyUI process on a dedicated port
//! - A dispatch loop assigns queued prompts to idle workers
//! - Workers are reserved atomically (one prompt at a time per GPU)
//! - If a worker errors, it's marked as failed and skipped

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Child;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::config::{AppConfig, GpuWorkerConfig};
use crate::error::AppError;

/// Status of a GPU worker backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerStatus {
    /// Not started yet.
    Stopped,
    /// Starting up (process spawned, waiting for HTTP ready).
    Starting,
    /// Ready and idle — can accept a prompt.
    Idle,
    /// Currently executing a prompt.
    Running,
    /// Process crashed or failed to start.
    Error,
    /// Administratively disabled.
    Disabled,
}

/// A single GPU worker — owns one ComfyUI process.
pub struct GpuWorker {
    /// Worker ID (0-indexed, matches gpu_workers config index).
    pub id: u32,
    /// GPU index (CUDA_VISIBLE_DEVICES value).
    pub gpu_index: u32,
    /// Port this ComfyUI instance listens on.
    pub port: u16,
    /// HTTP base URL for this worker's ComfyUI API.
    pub base_url: String,
    /// Current status.
    pub status: RwLock<WorkerStatus>,
    /// Whether this worker is currently reserved (executing a prompt).
    pub reserved: AtomicBool,
    /// Timestamp (millis since epoch) of last time this worker was released.
    pub last_released: AtomicU64,
    /// The child process handle.
    pub process: Mutex<Option<Child>>,
    /// WebSocket task handle for this worker.
    pub ws_handle: Mutex<Option<JoinHandle<()>>>,
    /// Friendly label (e.g. GPU name from nvidia-smi).
    pub label: String,
    /// VRAM mode override for this specific worker (falls back to global).
    pub vram_mode: Option<String>,
}

impl GpuWorker {
    pub fn new(id: u32, cfg: &GpuWorkerConfig, base_port: u16) -> Self {
        let port = cfg.port.unwrap_or(base_port + id as u16);
        Self {
            id,
            gpu_index: cfg.gpu_index,
            port,
            base_url: format!("http://127.0.0.1:{}", port),
            status: RwLock::new(if cfg.enabled {
                WorkerStatus::Stopped
            } else {
                WorkerStatus::Disabled
            }),
            reserved: AtomicBool::new(false),
            last_released: AtomicU64::new(0),
            process: Mutex::new(None),
            ws_handle: Mutex::new(None),
            label: cfg
                .label
                .clone()
                .unwrap_or_else(|| format!("GPU {}", cfg.gpu_index)),
            vram_mode: cfg.vram_mode.clone(),
        }
    }

    /// Whether this worker can accept a new prompt right now.
    pub async fn is_available(&self) -> bool {
        let status = *self.status.read().await;
        status == WorkerStatus::Idle && !self.reserved.load(Ordering::Acquire)
    }

    /// Reserve this worker for a prompt. Returns false if already reserved.
    pub fn try_reserve(&self) -> bool {
        self.reserved
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Release this worker after prompt completion/error.
    pub fn release(&self) {
        self.reserved.store(false, Ordering::Release);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.last_released.store(now, Ordering::Release);
    }
}

/// Manages N GPU workers and distributes prompts across them.
pub struct GpuManager {
    /// All workers, indexed by worker ID.
    pub workers: Vec<Arc<GpuWorker>>,
    /// Notified when a worker becomes available (prompt finished).
    pub worker_available: Notify,
    /// Shared HTTP client for all workers.
    pub http_client: reqwest::Client,
}

impl GpuManager {
    /// Create a new manager from config. If no gpu_workers are configured,
    /// creates a single default worker on the configured port.
    pub fn new(config: &AppConfig, http_client: reqwest::Client) -> Self {
        let workers = if config.gpu_workers.is_empty() {
            // Backward compat: single worker on the existing port
            let default_cfg = GpuWorkerConfig {
                gpu_index: 0,
                port: Some(config.server_port),
                enabled: true,
                label: None,
                vram_mode: None,
            };
            vec![Arc::new(GpuWorker::new(
                0,
                &default_cfg,
                config.server_port,
            ))]
        } else {
            config
                .gpu_workers
                .iter()
                .enumerate()
                .map(|(i, cfg)| Arc::new(GpuWorker::new(i as u32, cfg, config.server_port)))
                .collect()
        };

        Self {
            workers,
            worker_available: Notify::new(),
            http_client,
        }
    }

    /// Get the number of enabled workers.
    pub async fn enabled_count(&self) -> usize {
        let mut count = 0;
        for w in &self.workers {
            if *w.status.read().await != WorkerStatus::Disabled {
                count += 1;
            }
        }
        count
    }

    /// Find the best available worker for a new prompt.
    /// Prefers idle workers, with ties broken by least-recently-used.
    pub async fn find_available(&self) -> Option<Arc<GpuWorker>> {
        let mut best: Option<Arc<GpuWorker>> = None;
        let mut best_time: u64 = u64::MAX;

        for worker in &self.workers {
            if !worker.is_available().await {
                continue;
            }
            let released = worker.last_released.load(Ordering::Acquire);
            if released < best_time {
                best_time = released;
                best = Some(Arc::clone(worker));
            }
        }

        best
    }

    /// Wait for any worker to become available, with timeout.
    /// Returns the worker, or None on timeout.
    pub async fn wait_for_available(&self, timeout: Duration) -> Option<Arc<GpuWorker>> {
        // Fast path: check immediately
        if let Some(w) = self.find_available().await {
            return Some(w);
        }

        // Wait for notification
        match tokio::time::timeout(timeout, self.worker_available.notified()).await {
            Ok(()) => self.find_available().await,
            Err(_) => None,
        }
    }

    /// Submit a prompt to the best available worker.
    /// Blocks until a worker is free (up to timeout).
    /// Returns (worker_id, prompt_response) on success.
    pub async fn submit_prompt(
        &self,
        workflow: serde_json::Value,
        client_id: &str,
        timeout: Duration,
    ) -> Result<(u32, crate::comfyui::types::PromptResponse), AppError> {
        let worker = self
            .wait_for_available(timeout)
            .await
            .ok_or_else(|| AppError::Other("All GPU workers are busy — try again later".into()))?;

        if !worker.try_reserve() {
            // Race condition: another request grabbed it. Retry once.
            let worker = self.find_available().await.ok_or_else(|| {
                AppError::Other("All GPU workers are busy — try again later".into())
            })?;
            if !worker.try_reserve() {
                return Err(AppError::Other(
                    "All GPU workers are busy — try again later".into(),
                ));
            }
            return self.do_submit(&worker, workflow, client_id).await;
        }

        self.do_submit(&worker, workflow, client_id).await
    }

    /// Actually POST the prompt to a specific worker.
    async fn do_submit(
        &self,
        worker: &Arc<GpuWorker>,
        workflow: serde_json::Value,
        client_id: &str,
    ) -> Result<(u32, crate::comfyui::types::PromptResponse), AppError> {
        {
            let mut status = worker.status.write().await;
            *status = WorkerStatus::Running;
        }

        let body = serde_json::json!({
            "prompt": workflow,
            "client_id": client_id,
        });

        let url = format!("{}/prompt", worker.base_url);
        let resp = self.http_client.post(&url).json(&body).send().await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let prompt_resp: crate::comfyui::types::PromptResponse = r.json().await?;
                // Worker stays Running — will be released when execution completes
                // (via WebSocket execution_complete event)
                Ok((worker.id, prompt_resp))
            }
            Ok(r) => {
                worker.release();
                {
                    let mut status = worker.status.write().await;
                    *status = WorkerStatus::Idle;
                }
                self.worker_available.notify_one();
                let status_code = r.status().as_u16();
                let body = r.text().await.unwrap_or_default();
                Err(AppError::ApiError {
                    status: status_code,
                    message: body,
                })
            }
            Err(e) => {
                worker.release();
                {
                    let mut status = worker.status.write().await;
                    *status = WorkerStatus::Error;
                }
                self.worker_available.notify_one();
                Err(AppError::ConnectionFailed(format!(
                    "Worker {} (GPU {}): {}",
                    worker.id, worker.gpu_index, e
                )))
            }
        }
    }

    /// Mark a worker as idle after its prompt finishes (called from WS event handler).
    pub async fn mark_worker_idle(&self, worker_id: u32) {
        if let Some(worker) = self.workers.get(worker_id as usize) {
            worker.release();
            {
                let mut status = worker.status.write().await;
                if *status == WorkerStatus::Running {
                    *status = WorkerStatus::Idle;
                }
            }
            self.worker_available.notify_one();
        }
    }

    /// Mark a worker as errored (called from WS on execution_error).
    pub async fn mark_worker_error_then_idle(&self, worker_id: u32) {
        if let Some(worker) = self.workers.get(worker_id as usize) {
            worker.release();
            {
                let mut status = worker.status.write().await;
                // Mark idle so it can try the next prompt — the error is transient
                // (e.g. OOM on one prompt doesn't mean the GPU is dead).
                *status = WorkerStatus::Idle;
            }
            self.worker_available.notify_one();
        }
    }

    /// Find which worker is handling a given prompt by checking each worker's queue.
    pub async fn worker_for_prompt(&self, _prompt_id: &str) -> Option<u32> {
        // The prompt→worker mapping is tracked externally (in PromptQueue).
        // This is a fallback that checks which worker is currently running.
        for worker in &self.workers {
            let status = *worker.status.read().await;
            if status == WorkerStatus::Running && worker.reserved.load(Ordering::Acquire) {
                return Some(worker.id);
            }
        }
        None
    }

    /// Get status summary for all workers (for frontend display).
    pub async fn worker_statuses(&self) -> Vec<WorkerStatusInfo> {
        let mut out = Vec::with_capacity(self.workers.len());
        for w in &self.workers {
            out.push(WorkerStatusInfo {
                id: w.id,
                gpu_index: w.gpu_index,
                label: w.label.clone(),
                port: w.port,
                status: *w.status.read().await,
                reserved: w.reserved.load(Ordering::Acquire),
            });
        }
        out
    }

    /// Perform an API GET on the first available or specified worker.
    pub async fn api_get(&self, path: &str) -> Result<serde_json::Value, AppError> {
        // Use the first idle (or any running) worker for read-only API calls
        let worker = self
            .first_ready_worker()
            .await
            .ok_or_else(|| AppError::ConnectionFailed("No GPU workers are ready".into()))?;
        let url = format!("{}{}", worker.base_url, path);
        let resp = self.http_client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::ApiError {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    /// Perform an API POST on the first available or specified worker.
    pub async fn api_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let worker = self
            .first_ready_worker()
            .await
            .ok_or_else(|| AppError::ConnectionFailed("No GPU workers are ready".into()))?;
        let url = format!("{}{}", worker.base_url, path);
        let resp = self.http_client.post(&url).json(body).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::ApiError {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        let text = resp.text().await?;
        if text.trim().is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// Interrupt generation on a specific worker or all workers.
    pub async fn interrupt(&self, worker_id: Option<u32>) -> Result<(), AppError> {
        match worker_id {
            Some(id) => {
                if let Some(w) = self.workers.get(id as usize) {
                    let url = format!("{}/interrupt", w.base_url);
                    self.http_client.post(&url).send().await?;
                }
            }
            None => {
                for w in &self.workers {
                    let status = *w.status.read().await;
                    if status == WorkerStatus::Running {
                        let url = format!("{}/interrupt", w.base_url);
                        let _ = self.http_client.post(&url).send().await;
                    }
                }
            }
        }
        Ok(())
    }

    /// Get the first worker that is idle or running (for model/sampler queries).
    async fn first_ready_worker(&self) -> Option<Arc<GpuWorker>> {
        for w in &self.workers {
            let status = *w.status.read().await;
            if status == WorkerStatus::Idle || status == WorkerStatus::Running {
                return Some(Arc::clone(w));
            }
        }
        None
    }

    /// Check if we're in single-worker mode (backward-compat, no gpu_workers configured).
    pub fn is_single_worker(&self) -> bool {
        self.workers.len() == 1
    }
}

/// Serializable status info for frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkerStatusInfo {
    pub id: u32,
    pub gpu_index: u32,
    pub label: String,
    pub port: u16,
    pub status: WorkerStatus,
    pub reserved: bool,
}

/// Detect available NVIDIA GPUs via nvidia-smi.
/// Returns Vec<(index, name, vram_mb)>.
pub fn detect_gpus() -> Vec<(u32, String, u64)> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                    if parts.len() >= 3 {
                        let idx = parts[0].parse::<u32>().ok()?;
                        let name = parts[1].to_string();
                        let vram = parts[2].parse::<u64>().ok()?;
                        Some((idx, name, vram))
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => vec![],
    }
}

/// Auto-generate worker configs from detected GPUs.
/// Each GPU gets its own worker with sequential ports starting from base_port.
pub fn auto_configure_workers(base_port: u16) -> Vec<GpuWorkerConfig> {
    let gpus = detect_gpus();
    if gpus.is_empty() {
        return vec![];
    }
    gpus.iter()
        .map(|(idx, name, vram)| {
            let vram_mode = if *vram >= 40_000 {
                Some("high".to_string())
            } else if *vram >= 8_000 {
                None // Use default
            } else {
                Some("low".to_string())
            };
            GpuWorkerConfig {
                gpu_index: *idx,
                port: Some(base_port + *idx as u16),
                enabled: true,
                label: Some(name.clone()),
                vram_mode,
            }
        })
        .collect()
}
