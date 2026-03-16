use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server_mode: ServerMode,
    pub server_url: String,
    pub server_port: u16,
    pub comfyui_path: String,
    pub venv_path: String,
    pub extra_args: Vec<String>,
    pub default_checkpoint: Option<String>,
    pub default_sampler: String,
    pub default_scheduler: String,
    pub default_steps: u32,
    pub default_cfg: f64,
    pub default_width: u32,
    pub default_height: u32,
    #[serde(default)]
    pub setup_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServerMode {
    AutoLaunch,
    Remote,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server_mode: ServerMode::AutoLaunch,
            server_url: "http://127.0.0.1:8188".to_string(),
            server_port: 8188,
            comfyui_path: String::new(),
            venv_path: String::new(),
            extra_args: vec![],
            default_checkpoint: None,
            default_sampler: "euler".to_string(),
            default_scheduler: "normal".to_string(),
            default_steps: 20,
            default_cfg: 7.0,
            default_width: 512,
            default_height: 512,
            setup_complete: false,
        }
    }
}

const APP_IDENTIFIER: &str = "com.comfyui.desktop";

/// Get the app data directory path (platform-appropriate).
pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_IDENTIFIER))
}

/// Load persisted config from disk, falling back to defaults.
pub fn load_persisted_config() -> AppConfig {
    if let Some(dir) = app_data_dir() {
        let config_path = dir.join("config.json");
        if let Ok(json) = std::fs::read_to_string(config_path) {
            if let Ok(config) = serde_json::from_str(&json) {
                return config;
            }
        }
    }
    AppConfig::default()
}

/// Save config to disk.
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let dir = app_data_dir().ok_or("Failed to determine app data directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create data dir: {}", e))?;
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("config.json"), json).map_err(|e| e.to_string())?;
    Ok(())
}
