use serde::{Deserialize, Serialize};

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
        }
    }
}
