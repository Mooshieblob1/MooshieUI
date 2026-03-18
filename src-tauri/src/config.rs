use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// VRAM management mode: "auto", "high", "normal", "low", "none"
    pub vram_mode: String,
    /// Keep ComfyUI running after the app closes (default: false)
    pub keep_alive: bool,
    /// UI theme: "dark", "light"
    pub theme: String,
    /// UI font scale multiplier (1.0 = default)
    pub font_scale: f64,
    pub setup_complete: bool,
    /// Optional shared model directory (e.g. from another ComfyUI/Forge install)
    pub extra_model_paths: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServerMode {
    #[serde(alias = "AutoLaunch")]
    AutoLaunch,
    #[serde(alias = "Remote")]
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
            default_sampler: "euler_cfg_pp".to_string(),
            default_scheduler: "sgm_uniform".to_string(),
            default_steps: 20,
            default_cfg: 1.4,
            default_width: 1024,
            default_height: 1024,
            vram_mode: "normal".to_string(),
            keep_alive: false,
            theme: "dark".to_string(),
            font_scale: 1.0,
            setup_complete: false,
            extra_model_paths: None,
        }
    }
}

const APP_IDENTIFIER: &str = "com.mooshieui.desktop";
const OLD_APP_IDENTIFIER: &str = "com.comfyui.desktop";

/// Get the app data directory path (platform-appropriate).
pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_IDENTIFIER))
}

/// Migrate data from the old `com.comfyui.desktop` directory to the new one.
/// Copies config.json if the new directory doesn't have one yet.
fn migrate_from_old_data_dir() {
    let data_dir = match dirs::data_dir() {
        Some(d) => d,
        None => return,
    };
    let old_dir = data_dir.join(OLD_APP_IDENTIFIER);
    let new_dir = data_dir.join(APP_IDENTIFIER);

    // Only migrate if old dir exists and new config doesn't
    if !old_dir.exists() {
        return;
    }
    let new_config = new_dir.join("config.json");
    if new_config.exists() {
        return;
    }

    let old_config = old_dir.join("config.json");
    if old_config.exists() {
        if let Err(e) = std::fs::create_dir_all(&new_dir) {
            eprintln!("Migration: failed to create new data dir: {}", e);
            return;
        }
        if let Err(e) = std::fs::copy(&old_config, &new_config) {
            eprintln!("Migration: failed to copy config.json: {}", e);
        } else {
            println!("Migrated config from {} to {}", old_dir.display(), new_dir.display());
        }
    }
}

/// Load persisted config from disk, falling back to defaults.
pub fn load_persisted_config() -> AppConfig {
    migrate_from_old_data_dir();

    if let Some(dir) = app_data_dir() {
        let config_path = dir.join("config.json");
        if let Ok(json) = std::fs::read_to_string(&config_path) {
            match serde_json::from_str::<AppConfig>(&json) {
                Ok(config) => {
                    eprintln!("Loaded config from {}: comfyui_path={}, venv_path={}",
                        config_path.display(), config.comfyui_path, config.venv_path);
                    return config;
                }
                Err(e) => {
                    eprintln!("Failed to parse {}: {}", config_path.display(), e);
                }
            }
        }
    }
    eprintln!("Using default config (no persisted config found)");
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
