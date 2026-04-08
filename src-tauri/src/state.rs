use std::sync::Arc;

use tokio::process::Child;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::task::JoinHandle;

use crate::config::AppConfig;
use crate::interrogator::InterrogatorState;

/// Stored Tauri AppHandle so the headless web server can control the window.
pub type TauriAppHandle = tauri::AppHandle;

/// An event that can be sent to both Tauri and browser SSE clients.
#[derive(Clone, Debug)]
pub struct BroadcastEvent {
    pub event: String,
    pub payload: serde_json::Value,
}

pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub comfyui_process: Mutex<Option<Child>>,
    pub ws_handle: Mutex<Option<JoinHandle<()>>>,
    pub client_id: String,
    pub http_client: reqwest::Client,
    pub interrogator: Arc<RwLock<InterrogatorState>>,
    /// Broadcast channel for SSE events in browser mode.
    pub event_tx: broadcast::Sender<BroadcastEvent>,
    /// Timestamp of last heartbeat from browser client.
    pub last_heartbeat: Mutex<std::time::Instant>,
    /// Tauri AppHandle — set after app setup so the web server can show/hide the window.
    pub app_handle: Mutex<Option<TauriAppHandle>>,
    /// Set to true when switching from browser mode to app mode.
    /// Prevents the heartbeat watchdog from killing the process.
    pub app_mode_active: std::sync::atomic::AtomicBool,
    /// True once the embedded web server has been started (prevents double-bind).
    pub web_server_running: std::sync::atomic::AtomicBool,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            config: RwLock::new(config),
            comfyui_process: Mutex::new(None),
            ws_handle: Mutex::new(None),
            client_id: uuid::Uuid::new_v4().to_string(),
            http_client: reqwest::Client::new(),
            interrogator: Arc::new(RwLock::new(InterrogatorState::new())),
            event_tx,
            last_heartbeat: Mutex::new(std::time::Instant::now()),
            app_handle: Mutex::new(None),
            app_mode_active: std::sync::atomic::AtomicBool::new(false),
            web_server_running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub async fn base_url(&self) -> String {
        let config = self.config.read().await;
        config.server_url.clone()
    }

    /// Broadcast an event to SSE clients.
    pub fn broadcast(&self, event: &str, payload: serde_json::Value) {
        let _ = self.event_tx.send(BroadcastEvent {
            event: event.to_string(),
            payload,
        });
    }
}
