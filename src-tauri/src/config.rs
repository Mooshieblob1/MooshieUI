use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Per-GPU worker configuration for multi-GPU setups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuWorkerConfig {
    /// CUDA device index (maps to CUDA_VISIBLE_DEVICES).
    pub gpu_index: u32,
    /// Port for this worker's ComfyUI instance. Auto-assigned if None.
    pub port: Option<u16>,
    /// Whether this worker is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Human-readable label (e.g. "RTX 4090").
    pub label: Option<String>,
    /// VRAM mode override ("high", "normal", "low", "none"). Falls back to global.
    pub vram_mode: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_llm_idle_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeTone {
    pub main: String,
    pub sub: String,
    pub trim: String,
    pub background: String,
    pub text: String,
}

impl Default for ThemeTone {
    fn default() -> Self {
        Self {
            main: "#ffcc00".to_string(),
            sub: "#404040".to_string(),
            trim: "#ffd54d".to_string(),
            background: "#0a0a0a".to_string(),
            text: "#f5f5f5".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeProfile {
    pub id: String,
    pub name: String,
    pub palette: String,
    #[serde(default)]
    pub dark: ThemeTone,
    #[serde(default)]
    pub light: ThemeTone,
    pub background_image: Option<String>,
    pub background_fade: f64,
    pub logo_image: Option<String>,
    pub hide_branding: bool,
}

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
    /// Automatically start ComfyUI when the app launches (default: true)
    pub auto_start: bool,
    /// UI theme mode: "dark", "light"
    pub theme: String,
    /// UI color palette shared across dark and light modes.
    pub theme_palette: String,
    /// UI font scale multiplier (1.0 = default)
    pub font_scale: f64,
    pub setup_complete: bool,
    /// Optional shared model directory (e.g. from another ComfyUI/Forge install)
    pub extra_model_paths: Option<String>,
    /// Interrogator: general tag confidence threshold (0.0–1.0)
    pub interrogator_general_threshold: f32,
    /// Interrogator: character tag confidence threshold (0.0–1.0)
    pub interrogator_character_threshold: f32,
    /// Prompt assistant: selected/installed catalog model id (None = not chosen yet).
    pub prompt_assistant_model_id: Option<String>,
    /// Prompt assistant: idle seconds before the llama-server subprocess is unloaded.
    #[serde(default = "default_llm_idle_timeout")]
    pub prompt_assistant_idle_timeout_secs: u64,
    /// Prompt assistant: true once the user has completed first-run setup.
    pub prompt_assistant_setup_done: bool,
    /// Optional CivitAI API key for authenticated hash lookups and metadata fetching
    pub civitai_api_key: Option<String>,
    /// Custom gallery directory. When `None`, defaults to `{app_data_dir}/gallery`.
    pub gallery_path: Option<String>,
    /// Run the UI in the default web browser instead of the Tauri window.
    pub browser_mode: bool,
    /// Port for the embedded UI web server (used in browser mode). Defaults to 3200.
    pub ui_server_port: u16,
    /// Enable LAN access (bind to 0.0.0.0 instead of 127.0.0.1). Only effective in browser mode.
    pub lan_enabled: bool,
    /// Attention backend: "default", "sage_v1", "sage_v2", "flash_v1", "flash_v2"
    pub attention_backend: String,
    /// Multi-GPU worker configs. When empty, single-worker mode (backward compat).
    #[serde(default)]
    pub gpu_workers: Vec<GpuWorkerConfig>,
    /// Optional HTTP(S) proxy for git clone and pip when installing ControlNet custom nodes.
    /// Example: `http://127.0.0.1:7890`. Also applied via HTTP_PROXY / HTTPS_PROXY env vars.
    pub network_proxy: Option<String>,
    /// Optional PyPI index URL for pip/uv installs (e.g. a regional mirror).
    /// Example: `https://pypi.tuna.tsinghua.edu.cn/simple`
    pub pip_index_url: Option<String>,
    /// Optional gallery output filename template.
    /// Supported keys: {prompt_id}, {mode}, {index}, {date}, {time}, {model}, {seed}
    pub output_filename_template: Option<String>,
    /// Optional webhook URL for generation/image events.
    pub webhook_url: Option<String>,
    /// Enabled webhook event names (e.g. "image_saved").
    #[serde(default)]
    pub webhook_events: Vec<String>,
    /// Whether webhook payloads include prompt/metadata fields.
    pub webhook_include_sensitive: bool,
    /// Allow localhost/private webhook targets.
    pub webhook_allow_private_targets: bool,
    /// Active custom theme profile ID. Null = built-in palette only.
    pub theme_profile_id: Option<String>,
    /// User-defined custom theme profiles.
    #[serde(default)]
    pub theme_profiles: Vec<ThemeProfile>,
    /// Optional TLS certificate PEM path for the browser-mode web server.
    pub tls_cert_path: Option<String>,
    /// Optional TLS private key PEM path for the browser-mode web server.
    pub tls_key_path: Option<String>,
    /// Prompt assistant: use an external OpenAI-compatible endpoint (LM Studio,
    /// OpenAI, OpenRouter, ...) instead of the bundled local llama-server.
    #[serde(default)]
    pub llm_external_enabled: bool,
    /// External LLM provider id (`anthropic`, `openai`, `xai`, `openrouter`,
    /// `custom`). Selects the wire format and the API root; the credentials and
    /// model still live in the `llm_external_*` fields below. Installs that
    /// predate this field default to `custom`, which is the OpenAI-compatible
    /// behaviour they already had.
    #[serde(default = "default_llm_provider")]
    pub llm_provider: String,
    /// External LLM API root, e.g. `http://localhost:1234/v1` or `https://api.openai.com/v1`.
    /// `/chat/completions` is appended.
    #[serde(default)]
    pub llm_external_base_url: String,
    /// External LLM API key (sent as a Bearer token; leave empty for keyless local servers).
    #[serde(default)]
    pub llm_external_api_key: String,
    /// External LLM model name (e.g. `gpt-4o-mini`, or the model id LM Studio exposes).
    #[serde(default)]
    pub llm_external_model: String,
    /// Report proxy endpoint (Cloudflare Tunnel URL). When set, in-app error
    /// reports POST here instead of opening a prefilled GitHub issue. Defaults to
    /// the hosted proxy; set to null/empty to fall back to prefilled issues.
    #[serde(default = "default_report_endpoint")]
    pub report_endpoint: Option<String>,
    /// Disable the 7-day gallery image auto-expiry entirely (default: false).
    /// The expiry exists as a disk-usage safety net for shared/public servers;
    /// this is an opt-in escape hatch for single-owner setups (e.g. a home
    /// server reached remotely over Tailscale/LAN under one's own account)
    /// where there's no untrusted guest to protect against.
    #[serde(default)]
    pub gallery_never_expire: bool,
}

/// Default report proxy endpoint. In-app error reports post here unless the
/// config explicitly overrides it (null/empty falls back to prefilled issues).
fn default_report_endpoint() -> Option<String> {
    Some("https://report.mooshieblob.com/report".to_string())
}

/// Default external LLM provider id. Kept as a literal because `config` is
/// compiled in builds that gate out `prompt_assistant`; the registry asserts it
/// matches `providers::DEFAULT_PROVIDER`.
fn default_llm_provider() -> String {
    "custom".to_string()
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
            auto_start: true,
            theme: "dark".to_string(),
            theme_palette: "mooshie".to_string(),
            font_scale: 1.0,
            setup_complete: false,
            extra_model_paths: None,
            interrogator_general_threshold: 0.30,
            interrogator_character_threshold: 0.85,
            prompt_assistant_model_id: None,
            prompt_assistant_idle_timeout_secs: 30,
            prompt_assistant_setup_done: false,
            civitai_api_key: None,
            gallery_path: None,
            browser_mode: false,
            ui_server_port: 3200,
            lan_enabled: false,
            attention_backend: "default".to_string(),
            gpu_workers: vec![],
            network_proxy: None,
            pip_index_url: None,
            output_filename_template: None,
            webhook_url: None,
            webhook_events: vec!["image_saved".to_string()],
            webhook_include_sensitive: false,
            webhook_allow_private_targets: false,
            theme_profile_id: None,
            theme_profiles: vec![],
            tls_cert_path: None,
            tls_key_path: None,
            llm_external_enabled: false,
            llm_provider: default_llm_provider(),
            llm_external_base_url: String::new(),
            llm_external_api_key: String::new(),
            llm_external_model: String::new(),
            report_endpoint: default_report_endpoint(),
            gallery_never_expire: false,
        }
    }
}

/// Serialize config for a browser-mode client. Secrets are included only for admins/moderators.
pub fn config_to_client_json(
    config: &AppConfig,
    include_secrets: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(config)?;
    if !include_secrets {
        if let Some(obj) = value.as_object_mut() {
            let configured = obj
                .get("civitai_api_key")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty());
            obj.insert("civitai_api_key".to_string(), serde_json::Value::Null);
            obj.insert(
                "civitai_api_key_configured".to_string(),
                serde_json::json!(configured),
            );
            // The external-LLM key is a provider credential (Anthropic, OpenAI,
            // xAI, OpenRouter, ...) and must never reach a non-admin client.
            // Regular users cannot call `update_config`, so blanking it here
            // costs them nothing.
            let llm_configured = obj
                .get("llm_external_api_key")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty());
            obj.insert(
                "llm_external_api_key".to_string(),
                serde_json::Value::String(String::new()),
            );
            obj.insert(
                "llm_external_api_key_configured".to_string(),
                serde_json::json!(llm_configured),
            );
        }
    }
    Ok(value)
}

/// Resolve the gallery directory.
/// Uses `AppConfig::gallery_path` if set, otherwise falls back to `{app_data_dir}/gallery`.
pub fn gallery_dir() -> Option<PathBuf> {
    // Try to read the config file to check for a custom gallery path
    let data_dir = app_data_dir()?;
    let config_path = data_dir.join("config.json");
    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(cfg) = serde_json::from_str::<AppConfig>(&content) {
            if let Some(ref custom) = cfg.gallery_path {
                let p = PathBuf::from(custom.trim());
                if !p.as_os_str().is_empty() {
                    return Some(p);
                }
            }
        }
    }
    Some(data_dir.join("gallery"))
}

const APP_IDENTIFIER: &str = "com.mooshieui.desktop";
const OLD_APP_IDENTIFIER: &str = "com.comfyui.desktop";

/// The platform-default app data directory (always the same location).
/// Used to store the bootstrap pointer file that redirects to the real data dir.
fn platform_default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_IDENTIFIER))
}

/// Read the custom data directory from the bootstrap pointer file.
/// The pointer lives at `{platform_default}/data_dir.txt` and contains
/// a single line with the absolute path to the real data directory.
fn load_custom_data_dir() -> Option<PathBuf> {
    let pointer = platform_default_data_dir()?.join("data_dir.txt");
    let content = std::fs::read_to_string(&pointer).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = PathBuf::from(trimmed);
    if p.as_os_str().is_empty() {
        return None;
    }
    Some(p)
}

/// Save a custom data directory to the bootstrap pointer file.
pub fn save_custom_data_dir(path: &str) -> Result<(), String> {
    let default_dir =
        platform_default_data_dir().ok_or("Failed to determine platform data directory")?;
    std::fs::create_dir_all(&default_dir)
        .map_err(|e| format!("Failed to create data dir: {}", e))?;
    std::fs::write(default_dir.join("data_dir.txt"), path.trim())
        .map_err(|e| format!("Failed to write data_dir.txt: {}", e))?;
    Ok(())
}

/// Get the app data directory path.
/// Priority: MOOSHIEUI_DATA_DIR env var > bootstrap pointer file > platform default.
pub fn app_data_dir() -> Option<PathBuf> {
    // 1. Environment variable override (highest priority)
    if let Ok(custom) = std::env::var("MOOSHIEUI_DATA_DIR") {
        let p = PathBuf::from(custom.trim());
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    // 2. Bootstrap pointer file (user chose install location)
    if let Some(custom) = load_custom_data_dir() {
        return Some(custom);
    }
    // 3. Platform default
    platform_default_data_dir()
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
            println!(
                "Migrated config from {} to {}",
                old_dir.display(),
                new_dir.display()
            );
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
                    eprintln!(
                        "Loaded config from {}: comfyui_path={}, venv_path={}",
                        config_path.display(),
                        config.comfyui_path,
                        config.venv_path
                    );
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

pub(crate) fn normalize_config_fields(config: &mut AppConfig) {
    for field in [
        &mut config.network_proxy,
        &mut config.pip_index_url,
        &mut config.output_filename_template,
        &mut config.webhook_url,
        &mut config.theme_profile_id,
        &mut config.tls_cert_path,
        &mut config.tls_key_path,
    ] {
        match field {
            Some(p) if p.trim().is_empty() => *field = None,
            Some(p) => *p = p.trim().to_string(),
            None => {}
        }
    }
    for worker in &mut config.gpu_workers {
        if let Some(label) = &mut worker.label {
            let trimmed = label.trim().to_string();
            worker.label = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        }
        if let Some(mode) = &mut worker.vram_mode {
            let trimmed = mode.trim().to_string();
            worker.vram_mode = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        }
    }
    for profile in &mut config.theme_profiles {
        profile.name = profile.name.trim().to_string();
        if profile.name.is_empty() {
            profile.name = "Custom Theme".to_string();
        }
        profile.palette = profile.palette.trim().to_lowercase();
        if profile.palette.is_empty() {
            profile.palette = "custom".to_string();
        }
        profile.background_fade = profile.background_fade.clamp(0.0, 1.0);
        match &mut profile.background_image {
            Some(v) if v.trim().is_empty() => profile.background_image = None,
            Some(v) => *v = v.trim().to_string(),
            None => {}
        }
        match &mut profile.logo_image {
            Some(v) if v.trim().is_empty() => profile.logo_image = None,
            Some(v) => *v = v.trim().to_string(),
            None => {}
        }
    }
}

/// Carry forward secrets a full-config save cannot legitimately have sent.
///
/// `update_config` replaces the whole config, and its callers hold a snapshot
/// taken at page load. The provider commands (`set_llm_api_key`, the OAuth
/// flow) write `llm_external_api_key` straight into Rust config, so that
/// snapshot goes stale immediately and a later unrelated save would write the
/// old empty string back over a working key. `config_to_client_json` also
/// blanks the key for non-admin browser clients, which is the same hazard from
/// the other direction. An incoming empty key is therefore a stale echo, not an
/// intent to clear: clearing goes through `set_llm_api_key("")`.
pub(crate) fn preserve_secrets(incoming: &mut AppConfig, current: &AppConfig) {
    if incoming.llm_external_api_key.trim().is_empty() {
        incoming
            .llm_external_api_key
            .clone_from(&current.llm_external_api_key);
    }
}

/// Save config to disk.
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let mut config = config.clone();
    normalize_config_fields(&mut config);
    let dir = app_data_dir().ok_or("Failed to determine app data directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create data dir: {}", e))?;
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::write(dir.join("config.json"), json) {
        // On hosted deployments the config is a read-only mount (e.g. a Kubernetes
        // ConfigMap), so persistence cannot succeed. Downgrade those cases to a
        // warning instead of surfacing a hard error for every settings change;
        // the in-memory config still reflects the user's choice for this session.
        if matches!(
            e.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
        ) {
            eprintln!("Skipping config save (read-only config location): {}", e);
            return Ok(());
        }
        return Err(e.to_string());
    }
    Ok(())
}
