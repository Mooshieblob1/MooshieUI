use tokio::process::Child;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use crate::config::AppConfig;

pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub comfyui_process: Mutex<Option<Child>>,
    pub ws_handle: Mutex<Option<JoinHandle<()>>>,
    pub client_id: String,
    pub http_client: reqwest::Client,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: RwLock::new(config),
            comfyui_process: Mutex::new(None),
            ws_handle: Mutex::new(None),
            client_id: uuid::Uuid::new_v4().to_string(),
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn base_url(&self) -> String {
        let config = self.config.read().await;
        config.server_url.clone()
    }
}
