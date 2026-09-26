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

/// A user-supplied ONNX tagger folder registered as a custom model.
/// MooshieUI never downloads or deletes these files; the user manages them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomInterrogatorModel {
    /// Stable id derived from the folder name (e.g. "custom-my-tagger").
    pub id: String,
    /// Human-readable display name shown in the dropdown.
    pub label: String,
    /// Absolute path to the folder containing model.onnx and selected_tags.csv.
    pub path: String,
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
    /// Interrogator: id of the selected tagger from `interrogator::INTERROGATOR_MODELS`.
    /// Unknown ids produce a clear error at run time.
    pub interrogator_model: String,
    /// User-supplied ONNX tagger folders registered as custom models.
    /// MooshieUI never downloads or deletes files from these paths.
    #[serde(default)]
    pub interrogator_custom_models: Vec<CustomInterrogatorModel>,
    /// Prompt assistant: selected/installed catalog model id (None = not chosen yet).
    pub prompt_assistant_model_id: Option<String>,
    /// Prompt assistant: idle seconds before the llama-server subprocess is unloaded.
    #[serde(default = "default_llm_idle_timeout")]
    pub prompt_assistant_idle_timeout_secs: u64,
    /// Prompt assistant: true once the user has completed first-run setup.
    pub prompt_assistant_setup_done: bool,
    /// Optional CivitAI API key for authenticated hash lookups and metadata fetching
    pub civitai_api_key: Option<String>,
    /// Optional NovelAI API key. Required before any NovelAI model can be used.
    #[serde(default)]
    pub novelai_api_key: Option<String>,
    /// Custom gallery directory. When `None`, defaults to `{app_data_dir}/gallery`.
    pub gallery_path: Option<String>,
    /// Run the UI in the default web browser instead of the Tauri window.
    pub browser_mode: bool,
    /// Port for the embedded UI web server (used in browser mode). Defaults to 3200.
    pub ui_server_port: u16,
    /// Enable LAN access (bind to 0.0.0.0 instead of 127.0.0.1). Only effective in browser mode.
    pub lan_enabled: bool,
    /// Shut the backend (and ComfyUI with it) down when the browser tab stops
    /// sending heartbeats. Only armed in single-user browser mode. Turn this
    /// off when the machine sleeps or the browser freezes background tabs and
    /// the backend should survive it.
    #[serde(default = "default_true")]
    pub browser_auto_shutdown: bool,
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
    /// Refresh token for providers whose sign-in issues a *short-lived* access
    /// token (Nous Portal). Empty for every other provider: an API key does not
    /// expire, and OpenRouter's PKCE flow issues a long-lived key rather than
    /// an OAuth token pair. Secret, and redacted the same way the key is.
    #[serde(default)]
    pub llm_oauth_refresh_token: String,
    /// OAuth client id this install registered for itself via RFC 7591 dynamic
    /// client registration. Needed to redeem `llm_oauth_refresh_token`, and
    /// per-install rather than baked into the binary, so MooshieUI never has to
    /// impersonate someone else's registered client.
    #[serde(default)]
    pub llm_oauth_client_id: String,
    /// Unix seconds after which `llm_external_api_key` stops being accepted.
    /// `0` means the credential does not expire, which is the case for every
    /// API key and for OpenRouter's issued key.
    #[serde(default)]
    pub llm_oauth_expires_at: i64,
    /// OAuth client id to present to xAI. Empty by default and never shipped
    /// with a value: xAI allowlists client ids and has issued none to this
    /// project, so signing in to a SuperGrok subscription stays off until
    /// whoever runs the install supplies one. Unlike `llm_oauth_client_id` this
    /// is operator configuration rather than a session artifact, so it outlives
    /// signing out. Not a secret -- OAuth client ids are public by design.
    #[serde(default)]
    pub llm_xai_client_id: String,
    /// Scope string for the xAI sign-in. Empty means the built-in default
    /// (`openid profile email offline_access api:access`); an operator whose
    /// client id was issued for a narrower or wider grant can override it.
    #[serde(default)]
    pub llm_xai_scope: String,
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
    /// When true, video outputs (ComfyUI generations, RIFE interpolations) are
    /// NOT automatically moved into the gallery. The frontend receives the raw
    /// output path and must explicitly call `save_video_to_gallery_manual` to
    /// persist the clip. Mirrors the image-side `manualSaveMode` toggle.
    #[serde(default)]
    pub manual_save_mode: bool,
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
            server_url: "http://127.0.0.1:18288".to_string(),
            server_port: 18288,
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
            interrogator_model: crate::interrogator::DEFAULT_INTERROGATOR_MODEL.to_string(),
            interrogator_custom_models: vec![],
            prompt_assistant_model_id: None,
            prompt_assistant_idle_timeout_secs: 30,
            prompt_assistant_setup_done: false,
            civitai_api_key: None,
            novelai_api_key: None,
            gallery_path: None,
            browser_mode: false,
            ui_server_port: 3200,
            lan_enabled: false,
            browser_auto_shutdown: true,
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
            llm_oauth_refresh_token: String::new(),
            llm_oauth_client_id: String::new(),
            llm_oauth_expires_at: 0,
            llm_xai_client_id: String::new(),
            llm_xai_scope: String::new(),
            report_endpoint: default_report_endpoint(),
            gallery_never_expire: false,
            manual_save_mode: false,
        }
    }
}

/// The operator's optional credential-bearing fields, by client-facing name.
///
/// A CivitAI key is a bearer credential outright, and the three URLs routinely
/// carry one: a proxy as `http://user:pass@host`, a private package index or a
/// webhook with a token in its userinfo or query. Only the instance admin sees
/// them; every other client gets `null` plus a `{name}_configured` flag.
/// [`operator_secrets`] must list the same fields in the same order.
fn operator_secrets_mut(config: &mut AppConfig) -> [(&'static str, &mut Option<String>); 4] {
    [
        ("civitai_api_key", &mut config.civitai_api_key),
        ("webhook_url", &mut config.webhook_url),
        ("network_proxy", &mut config.network_proxy),
        ("pip_index_url", &mut config.pip_index_url),
    ]
}

/// Read-only twin of [`operator_secrets_mut`], same fields in the same order.
fn operator_secrets(config: &AppConfig) -> [&Option<String>; 4] {
    [
        &config.civitai_api_key,
        &config.webhook_url,
        &config.network_proxy,
        &config.pip_index_url,
    ]
}

fn is_set(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|v| !v.trim().is_empty())
}

/// Serialize config for a client.
///
/// `include_secrets` is for the instance admin only (the desktop owner, or a
/// browser client resolved to `UserRole::Admin`). Everyone else, moderators
/// included, gets the operator's credential fields blanked: a moderator can
/// already edit shared settings, but that is no reason to hand them the
/// owner's CivitAI or NovelAI key, or a proxy URL with a password in it.
///
/// The external-LLM credential never leaves Rust for any role, admin included.
/// The settings UI reads the key-free `LlmProviderState` projection instead,
/// and `preserve_secrets` keeps the stored credential across every full-config
/// save, so nothing ever needs the value on the client.
pub fn config_to_client_json(
    config: &AppConfig,
    include_secrets: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut redacted = config.clone();
    // The refresh token is strictly more dangerous than the access token it
    // mints: it survives the access token's expiry and can be redeemed
    // indefinitely until the user revokes it. The client id is this install's
    // private registration and is only ever used alongside it.
    redacted.llm_external_api_key.clear();
    redacted.llm_oauth_refresh_token.clear();
    redacted.llm_oauth_client_id.clear();

    let mut configured_flags: Vec<(&'static str, bool)> = Vec::new();
    if !include_secrets {
        // The NovelAI key spends the owner's money, so the client learns only
        // whether one is set.
        configured_flags.push(("novelai_api_key", is_set(&redacted.novelai_api_key)));
        redacted.novelai_api_key = None;
        for (name, field) in operator_secrets_mut(&mut redacted) {
            configured_flags.push((name, is_set(field)));
            *field = None;
        }
    }

    let mut value = serde_json::to_value(&redacted)?;
    if let Some(obj) = value.as_object_mut() {
        // Whether the provider row is authenticated; `LlmProviderState`
        // reports the same thing as `api_key_configured`.
        obj.insert(
            "llm_external_api_key_configured".to_string(),
            serde_json::json!(!config.llm_external_api_key.trim().is_empty()),
        );
        for (name, configured) in configured_flags {
            obj.insert(format!("{name}_configured"), serde_json::json!(configured));
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

/// Keep the external-LLM provider row exactly as the server has it.
///
/// Every one of these fields is written only by a dedicated command
/// (`set_llm_provider`, `set_llm_api_key`, `set_llm_model`, `set_llm_base_url`,
/// `set_llm_xai_client`, the OAuth sign-in) or by the background token refresh,
/// so whatever a full-config save carries for them is at best a stale echo of
/// a page-load snapshot. Accepting it would write back a key the user has
/// since cleared or replaced, an expired access token, or a refresh token the
/// provider already rotated out. It would also let a full-config save pair the
/// stored key with a different provider or base URL, sending it to a host
/// `set_llm_base_url` would have made it forget the key for. The one field
/// left to `update_config` is `llm_external_enabled`, which the settings page
/// toggles through its autosave.
fn keep_llm_provider_row(incoming: &mut AppConfig, current: &AppConfig) {
    incoming.llm_provider.clone_from(&current.llm_provider);
    incoming
        .llm_external_base_url
        .clone_from(&current.llm_external_base_url);
    incoming
        .llm_external_model
        .clone_from(&current.llm_external_model);
    incoming
        .llm_external_api_key
        .clone_from(&current.llm_external_api_key);
    incoming
        .llm_oauth_refresh_token
        .clone_from(&current.llm_oauth_refresh_token);
    incoming
        .llm_oauth_client_id
        .clone_from(&current.llm_oauth_client_id);
    incoming.llm_oauth_expires_at = current.llm_oauth_expires_at;
    incoming
        .llm_xai_client_id
        .clone_from(&current.llm_xai_client_id);
    incoming.llm_xai_scope.clone_from(&current.llm_xai_scope);
}

/// Carry forward secrets a full-config save cannot legitimately have sent.
///
/// `update_config` replaces the whole config, and its callers hold a snapshot
/// taken at page load, which the provider commands and the token refresh make
/// stale behind the frontend's back. The LLM provider row is therefore always
/// kept as the server has it (see [`keep_llm_provider_row`]); changing it goes
/// through its own commands, and signing out through `set_llm_api_key("")`.
pub(crate) fn preserve_secrets(incoming: &mut AppConfig, current: &AppConfig) {
    keep_llm_provider_row(incoming, current);
    // Blanked for non-admin clients, so an absent or empty NovelAI key is a
    // stale echo rather than an intent to clear. Clearing goes through
    // `set_novelai_api_key("")`.
    if incoming
        .novelai_api_key
        .as_deref()
        .is_none_or(|k| k.trim().is_empty())
    {
        incoming
            .novelai_api_key
            .clone_from(&current.novelai_api_key);
    }
}

/// Undo the redaction [`config_to_client_json`] applied for a non-admin client.
///
/// Such a client only ever saw `null` for the operator's credential fields, so
/// an empty one in its full-config save is that redaction coming back rather
/// than an intent to clear the owner's value. A non-empty value is a
/// deliberate, write-only replacement (the CivitAI section is open to
/// moderators) and goes through.
pub(crate) fn preserve_redacted_secrets(incoming: &mut AppConfig, current: &AppConfig) {
    for ((_, field), stored) in operator_secrets_mut(incoming)
        .into_iter()
        .zip(operator_secrets(current))
    {
        if !is_set(field) {
            field.clone_from(stored);
        }
    }
}

/// `OpenOptions` that create a file at mode 0600 on Unix, so a new file is
/// never readable by other local users, not even between creation and a later
/// `chmod`. Windows has no mode bits; the file inherits the directory's ACL.
fn private_create_options() -> std::fs::OpenOptions {
    #[allow(unused_mut)]
    let mut opts = std::fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts
}

/// A sibling temp path for `path`, unique per process and per call so two
/// concurrent saves of the same file cannot collide.
fn atomic_tmp_path(path: &std::path::Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data.json".to_string());
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}

/// Replace `path` with `bytes` so that a crash or a full disk can never leave
/// it truncated, and so that on Unix only its owner can read it.
///
/// Same approach as `user_secrets`: write a sibling temp file created at mode
/// 0600, fsync it, then rename it over `path`. A same-directory rename is
/// atomic on Unix and on Windows (Rust's `fs::rename` uses `MoveFileExW` with
/// `MOVEFILE_REPLACE_EXISTING`), and it carries the 0600 mode across.
///
/// Two layouts cannot take a rename and fall back to the plain in-place write
/// every save used before: a directory the process cannot create files in, and
/// a target that is itself a mount point (a Kubernetes ConfigMap `subPath`, a
/// Docker file bind mount), which `rename(2)` refuses with `EBUSY`. On a
/// read-only mount that in-place write then fails with the same error kind it
/// always did, so callers that tolerate a read-only config keep working.
pub fn write_private_file_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::{ErrorKind, Write};

    let tmp = atomic_tmp_path(path);
    let mut file = match private_create_options()
        .write(true)
        .create_new(true)
        .open(&tmp)
    {
        Ok(file) => file,
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            return std::fs::write(path, bytes);
        }
        Err(e) => return Err(e),
    };
    let staged = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    // A failed write (a full disk, say) must never fall back to writing in
    // place: that would truncate the good copy this function exists to keep.
    if let Err(e) = staged {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        if matches!(
            e.kind(),
            ErrorKind::ResourceBusy | ErrorKind::CrossesDevices
        ) {
            return std::fs::write(path, bytes);
        }
        return Err(e);
    }
    Ok(())
}

/// Save config to disk.
///
/// `config.json` holds every operator secret in plaintext, so it is written
/// atomically and owner-only (see [`write_private_file_atomic`]).
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let mut config = config.clone();
    normalize_config_fields(&mut config);
    let dir = app_data_dir().ok_or("Failed to determine app data directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create data dir: {}", e))?;
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    if let Err(e) = write_private_file_atomic(&dir.join("config.json"), json.as_bytes()) {
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

#[cfg(test)]
mod secret_handling_tests {
    use super::*;

    const CIVITAI: &str = "civitai-owner-key";
    const WEBHOOK: &str = "https://hooks.example/abc?token=webhook-secret";
    const PROXY: &str = "http://proxyuser:proxy-secret@10.0.0.2:3128";
    const PIP: &str = "https://pipuser:pip-secret@pypi.internal/simple";
    const NOVELAI: &str = "pst-owner-novelai";
    const LLM_KEY: &str = "sk-owner-llm";
    const REFRESH: &str = "refresh-owner-token";
    const CLIENT_ID: &str = "client-owner-registration";

    fn owner_config() -> AppConfig {
        AppConfig {
            civitai_api_key: Some(CIVITAI.into()),
            webhook_url: Some(WEBHOOK.into()),
            network_proxy: Some(PROXY.into()),
            pip_index_url: Some(PIP.into()),
            novelai_api_key: Some(NOVELAI.into()),
            llm_provider: "nous".into(),
            llm_external_base_url: "https://inference-api.nousresearch.com/v1".into(),
            llm_external_model: "nousresearch/hermes-4-405b".into(),
            llm_external_api_key: LLM_KEY.into(),
            llm_oauth_refresh_token: REFRESH.into(),
            llm_oauth_client_id: CLIENT_ID.into(),
            llm_oauth_expires_at: 1_900_000_000,
            llm_external_enabled: true,
            ..Default::default()
        }
    }

    /// What a client posts back after editing one unrelated field of the
    /// config it was served.
    fn echo(view: serde_json::Value) -> AppConfig {
        let mut echoed: AppConfig = serde_json::from_value(view).expect("client view must parse");
        echoed.server_port = 9191;
        normalize_config_fields(&mut echoed);
        echoed
    }

    #[test]
    fn operator_secret_accessors_list_the_same_fields_in_order() {
        let mut config = owner_config();
        let names: Vec<&str> = operator_secrets_mut(&mut config)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let values: Vec<Option<String>> = operator_secrets(&config).into_iter().cloned().collect();
        let json = serde_json::to_value(&config).unwrap();
        for (name, value) in names.iter().zip(values) {
            assert_eq!(json[name], serde_json::json!(value), "{name} out of order");
        }
    }

    #[test]
    fn a_non_admin_view_carries_no_secret_at_all() {
        let view = config_to_client_json(&owner_config(), false).unwrap();
        let text = view.to_string();
        for secret in [
            CIVITAI,
            "webhook-secret",
            "proxy-secret",
            "pip-secret",
            NOVELAI,
            LLM_KEY,
            REFRESH,
            CLIENT_ID,
        ] {
            assert!(
                !text.contains(secret),
                "{secret} leaked to a non-admin client"
            );
        }
        for name in [
            "civitai_api_key",
            "webhook_url",
            "network_proxy",
            "pip_index_url",
            "novelai_api_key",
        ] {
            assert_eq!(view[name], serde_json::Value::Null, "{name}");
            assert_eq!(view[format!("{name}_configured")], serde_json::json!(true));
        }
        assert_eq!(view["llm_external_api_key"], serde_json::json!(""));
        assert_eq!(
            view["llm_external_api_key_configured"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn unset_fields_are_reported_as_not_configured() {
        let view = config_to_client_json(&AppConfig::default(), false).unwrap();
        assert_eq!(view["civitai_api_key_configured"], serde_json::json!(false));
        assert_eq!(view["network_proxy_configured"], serde_json::json!(false));
        assert_eq!(
            view["llm_external_api_key_configured"],
            serde_json::json!(false)
        );
    }

    #[test]
    fn the_admin_view_keeps_operator_settings_but_never_the_llm_credential() {
        let view = config_to_client_json(&owner_config(), true).unwrap();
        assert_eq!(view["civitai_api_key"], serde_json::json!(CIVITAI));
        assert_eq!(view["network_proxy"], serde_json::json!(PROXY));
        assert_eq!(view["webhook_url"], serde_json::json!(WEBHOOK));
        assert_eq!(view["pip_index_url"], serde_json::json!(PIP));
        let text = view.to_string();
        for secret in [LLM_KEY, REFRESH, CLIENT_ID] {
            assert!(!text.contains(secret), "{secret} left Rust");
        }
        assert_eq!(
            view["llm_external_api_key_configured"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn a_non_admin_round_trip_keeps_every_stored_secret() {
        let current = owner_config();
        let mut incoming = echo(config_to_client_json(&current, false).unwrap());
        preserve_secrets(&mut incoming, &current);
        preserve_redacted_secrets(&mut incoming, &current);

        assert_eq!(incoming.server_port, 9191, "the real edit goes through");
        assert_eq!(incoming.civitai_api_key, current.civitai_api_key);
        assert_eq!(incoming.webhook_url, current.webhook_url);
        assert_eq!(incoming.network_proxy, current.network_proxy);
        assert_eq!(incoming.pip_index_url, current.pip_index_url);
        assert_eq!(incoming.novelai_api_key, current.novelai_api_key);
        assert_eq!(incoming.llm_external_api_key, LLM_KEY);
        assert_eq!(incoming.llm_oauth_refresh_token, REFRESH);
        assert_eq!(incoming.llm_oauth_client_id, CLIENT_ID);
        assert_eq!(incoming.llm_oauth_expires_at, current.llm_oauth_expires_at);
    }

    #[test]
    fn a_non_admin_can_still_replace_a_redacted_value() {
        let current = owner_config();
        let mut incoming = echo(config_to_client_json(&current, false).unwrap());
        incoming.civitai_api_key = Some("moderator-new-key".into());
        preserve_secrets(&mut incoming, &current);
        preserve_redacted_secrets(&mut incoming, &current);
        assert_eq!(
            incoming.civitai_api_key.as_deref(),
            Some("moderator-new-key")
        );
        assert_eq!(incoming.network_proxy, current.network_proxy);
    }

    #[test]
    fn an_admin_round_trip_keeps_the_llm_credential_and_may_clear_its_own_fields() {
        let current = owner_config();
        let mut incoming = echo(config_to_client_json(&current, true).unwrap());
        incoming.civitai_api_key = None;
        incoming.network_proxy = None;
        preserve_secrets(&mut incoming, &current);

        assert_eq!(incoming.llm_external_api_key, LLM_KEY);
        assert_eq!(incoming.llm_oauth_refresh_token, REFRESH);
        assert_eq!(incoming.llm_oauth_client_id, CLIENT_ID);
        // The admin saw these values, so an empty one is a real clear.
        assert_eq!(incoming.civitai_api_key, None);
        assert_eq!(incoming.network_proxy, None);
    }

    #[test]
    fn a_stale_autosave_cannot_resurrect_a_rotated_llm_session() {
        // The page loaded before a background refresh rotated both tokens.
        let mut current = owner_config();
        current.llm_external_api_key = "access-after-refresh".into();
        current.llm_oauth_refresh_token = "refresh-after-rotation".into();
        current.llm_oauth_expires_at = 1_950_000_000;
        let mut incoming = owner_config();
        incoming.llm_external_enabled = false;
        preserve_secrets(&mut incoming, &current);

        assert_eq!(incoming.llm_external_api_key, "access-after-refresh");
        assert_eq!(incoming.llm_oauth_refresh_token, "refresh-after-rotation");
        assert_eq!(incoming.llm_oauth_expires_at, 1_950_000_000);
        // The one provider field the settings page does save.
        assert!(!incoming.llm_external_enabled);
    }

    #[test]
    fn a_stale_autosave_cannot_bring_back_a_cleared_or_switched_key() {
        // Since page load the user switched to OpenAI, which discarded the
        // Nous session, and has not pasted a key yet.
        let current = AppConfig {
            llm_provider: "openai".into(),
            llm_external_base_url: "https://api.openai.com/v1".into(),
            llm_external_model: "gpt-4o-mini".into(),
            ..Default::default()
        };
        let mut incoming = owner_config();
        preserve_secrets(&mut incoming, &current);

        assert_eq!(incoming.llm_provider, "openai");
        assert_eq!(incoming.llm_external_base_url, "https://api.openai.com/v1");
        assert_eq!(incoming.llm_external_model, "gpt-4o-mini");
        assert!(incoming.llm_external_api_key.is_empty());
        assert!(incoming.llm_oauth_refresh_token.is_empty());
        assert!(incoming.llm_oauth_client_id.is_empty());
        assert_eq!(incoming.llm_oauth_expires_at, 0);
    }

    #[test]
    fn a_full_config_save_cannot_point_the_stored_key_at_another_host() {
        let current = AppConfig {
            llm_provider: "custom".into(),
            llm_external_base_url: "http://127.0.0.1:1234/v1".into(),
            llm_external_api_key: LLM_KEY.into(),
            ..Default::default()
        };
        let mut incoming = current.clone();
        incoming.llm_external_base_url = "https://attacker.example/v1".into();
        incoming.llm_provider = "openai".into();
        preserve_secrets(&mut incoming, &current);
        assert_eq!(incoming.llm_external_base_url, "http://127.0.0.1:1234/v1");
        assert_eq!(incoming.llm_provider, "custom");
        assert_eq!(incoming.llm_external_api_key, LLM_KEY);
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mooshie-config-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_atomic_write_replaces_the_file_and_leaves_no_temp_behind() {
        let dir = scratch_dir("atomic");
        let path = dir.join("config.json");
        write_private_file_atomic(&path, b"{\"first\":true}").unwrap();
        write_private_file_atomic(&path, b"{\"second\":true}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"second\":true}");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["config.json".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn an_atomic_write_is_owner_only_even_over_a_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch_dir("mode");
        let path = dir.join("config.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private_file_atomic(&path, b"{\"secret\":1}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
