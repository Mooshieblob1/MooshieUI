//! Single-ZIP backup and restore engine for user preferences, local stores,
//! sanitized app configuration, and wildcard files.
//!
//! Compiled in both `desktop` and `server` builds (zero Tauri dependencies).

use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::commands::wildcards::{self, WildcardFile};
use crate::config::{
    local_trust_enabled, normalize_config_fields, preserve_redacted_secrets, preserve_secrets,
    save_config, validate_civitai_proxy, AppConfig,
};
use crate::state::AppState;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_ZIP_UPLOAD_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_UNCOMPRESSED_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_WILDCARDS_BYTES: u64 = 10 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 5_000;

/// Config keys containing credentials or tokens that are NEVER written to backups.
pub const SECRET_CONFIG_KEYS: &[&str] = &[
    "civitai_api_key",
    "civitai_proxy",
    "novelai_api_key",
    "elevenlabs_api_key",
    "fal_api_key",
    "segmind_api_key",
    "webhook_url",
    "network_proxy",
    "pip_index_url",
    "llm_external_api_key",
    "llm_oauth_refresh_token",
    "llm_oauth_client_id",
    "llm_oauth_expires_at",
];

/// Config keys pointing to machine-local filesystem paths; marked in `app_config.json`
/// and kept opt-in (unchecked by default) during restore.
pub const MACHINE_LOCAL_CONFIG_KEYS: &[&str] = &[
    "comfyui_path",
    "venv_path",
    "extra_model_paths",
    "gallery_path",
    "tls_cert_path",
    "tls_key_path",
    "linked_gallery_folders",
    "interrogator_custom_models",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BackupBundleInput {
    pub user_prefs: serde_json::Value,
    pub local_extras: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct BackupCounts {
    pub characters: usize,
    pub prompt_presets: usize,
    pub styles: usize,
    pub lora_presets: usize,
    pub lora_triggers: usize,
    pub profiles: usize,
    pub artist_favourites: usize,
    pub prompt_history: usize,
    pub comic_books: usize,
    pub generation_tabs: usize,
    pub wildcards: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema_version: u32,
    pub app_version: String,
    pub exported_at: String,
    pub username: String,
    pub includes_config: bool,
    pub counts: BackupCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInspection {
    pub manifest: BackupManifest,
    pub user_prefs: serde_json::Value,
    pub local_extras: serde_json::Value,
    pub has_app_config: bool,
    pub machine_local_paths: Vec<String>,
    pub wildcard_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ApplyBackupAdminOptions {
    pub apply_app_config: bool,
    pub include_machine_local_paths: bool,
    pub apply_wildcards: bool,
    pub replace_wildcards: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApplyBackupAdminResult {
    pub config_applied: bool,
    pub wildcards_written: usize,
}

pub struct BackupBuildParams {
    pub app_version: String,
    pub username: String,
    pub bundle: BackupBundleInput,
    pub app_config: Option<AppConfig>,
    pub wildcards: Vec<WildcardFile>,
}

#[derive(Debug)]
struct ParsedBackupArchive {
    inspection: BackupInspection,
    app_config: Option<serde_json::Value>,
    wildcards: Vec<WildcardFile>,
}

#[derive(Debug, Clone, Copy)]
struct ZipLimits {
    max_zip_bytes: usize,
    max_uncompressed_bytes: u64,
    max_wildcards_bytes: u64,
}

impl Default for ZipLimits {
    fn default() -> Self {
        Self {
            max_zip_bytes: MAX_ZIP_UPLOAD_BYTES,
            max_uncompressed_bytes: MAX_UNCOMPRESSED_BYTES,
            max_wildcards_bytes: MAX_WILDCARDS_BYTES,
        }
    }
}

fn ensure_object(val: serde_json::Value) -> serde_json::Value {
    if val.is_object() {
        val
    } else {
        serde_json::json!({})
    }
}

fn array_len(root: &serde_json::Value, path: &[&str]) -> usize {
    let mut cur = root;
    for key in path {
        let Some(next) = cur.get(*key) else {
            return 0;
        };
        cur = next;
    }
    cur.as_array().map_or(0, Vec::len)
}

fn object_len(root: &serde_json::Value, path: &[&str]) -> usize {
    let mut cur = root;
    for key in path {
        let Some(next) = cur.get(*key) else {
            return 0;
        };
        cur = next;
    }
    cur.as_object().map_or(0, serde_json::Map::len)
}

pub fn compute_counts(
    user_prefs: &serde_json::Value,
    local_extras: &serde_json::Value,
    wildcards_count: usize,
) -> BackupCounts {
    BackupCounts {
        characters: array_len(user_prefs, &["characters", "characters"]),
        prompt_presets: array_len(user_prefs, &["prompt_presets", "presets"]),
        styles: array_len(user_prefs, &["styles", "styles"]),
        lora_presets: array_len(user_prefs, &["lora_presets", "presets"]),
        lora_triggers: object_len(user_prefs, &["lora_presets", "triggers"]),
        profiles: array_len(user_prefs, &["profiles", "profiles"]),
        artist_favourites: array_len(user_prefs, &["artist_favourites", "favourites"]),
        prompt_history: array_len(user_prefs, &["prompt_history"]),
        comic_books: array_len(local_extras, &["comic", "books"]),
        generation_tabs: array_len(local_extras, &["generationTabs", "tabs"]),
        wildcards: wildcards_count,
    }
}

/// Strip all credentials and tokens from an [`AppConfig`] and wrap it with
/// explicit `machine_local_keys` metadata for `app_config.json`.
pub fn sanitize_config_for_backup(config: &AppConfig) -> Result<serde_json::Value, String> {
    let mut val = serde_json::to_value(config).map_err(|e| e.to_string())?;
    if let Some(obj) = val.as_object_mut() {
        for key in SECRET_CONFIG_KEYS {
            obj.remove(*key);
        }
    }
    Ok(serde_json::json!({
        "config": val,
        "machine_local_keys": MACHINE_LOCAL_CONFIG_KEYS,
    }))
}

fn strip_secrets_from_config_value(val: &mut serde_json::Value) {
    if let Some(obj) = val.as_object_mut() {
        for key in SECRET_CONFIG_KEYS {
            obj.remove(*key);
        }
    }
}

fn present_machine_local_paths(config_obj: &serde_json::Value) -> Vec<String> {
    let mut present = Vec::new();
    for key in MACHINE_LOCAL_CONFIG_KEYS {
        let Some(val) = config_obj.get(*key) else {
            continue;
        };
        let non_empty = match val {
            serde_json::Value::Null => false,
            serde_json::Value::String(s) => !s.trim().is_empty(),
            serde_json::Value::Array(a) => !a.is_empty(),
            _ => true,
        };
        if non_empty {
            present.push((*key).to_string());
        }
    }
    present
}

fn write_json_entry<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    name: &str,
    value: &serde_json::Value,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    zip.start_file(name, options).map_err(|e| e.to_string())?;
    zip.write_all(&bytes).map_err(|e| e.to_string())?;
    Ok(())
}

/// Build an in-memory `.zip` backup archive and return `(zip_bytes, manifest)`.
pub fn build_backup_zip_with_manifest(
    params: BackupBuildParams,
) -> Result<(Vec<u8>, BackupManifest), String> {
    build_backup_zip_with_limits(params, MAX_WILDCARDS_BYTES)
}

fn build_backup_zip_with_limits(
    params: BackupBuildParams,
    max_wildcards_bytes: u64,
) -> Result<(Vec<u8>, BackupManifest), String> {
    let user_prefs = ensure_object(params.bundle.user_prefs);
    let local_extras = ensure_object(params.bundle.local_extras);

    let mut included_wildcards: Vec<WildcardFile> = Vec::new();
    let mut wildcard_bytes: u64 = 0;
    for wc in params.wildcards {
        let rel = wc.path.trim().replace('\\', "/");
        if !wildcards::is_valid_relative_path(&rel) {
            continue;
        }
        let len = wc.content.len() as u64;
        if wildcard_bytes.saturating_add(len) > max_wildcards_bytes {
            continue;
        }
        wildcard_bytes = wildcard_bytes.saturating_add(len);
        included_wildcards.push(WildcardFile {
            path: rel,
            content: wc.content,
        });
    }

    let manifest = BackupManifest {
        schema_version: SCHEMA_VERSION,
        app_version: params.app_version,
        exported_at: chrono::Utc::now().to_rfc3339(),
        username: params.username,
        includes_config: params.app_config.is_some(),
        counts: compute_counts(&user_prefs, &local_extras, included_wildcards.len()),
    };

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut buf = Vec::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buf));
        let manifest_val = serde_json::to_value(&manifest).map_err(|e| e.to_string())?;
        write_json_entry(&mut zip, "manifest.json", &manifest_val, options)?;
        write_json_entry(&mut zip, "user_prefs.json", &user_prefs, options)?;
        write_json_entry(&mut zip, "local_extras.json", &local_extras, options)?;

        if let Some(ref cfg) = params.app_config {
            let sanitized = sanitize_config_for_backup(cfg)?;
            write_json_entry(&mut zip, "app_config.json", &sanitized, options)?;
        }

        for wc in &included_wildcards {
            let entry_name = format!("wildcards/{}", wc.path);
            zip.start_file(&entry_name, options)
                .map_err(|e| e.to_string())?;
            zip.write_all(wc.content.as_bytes())
                .map_err(|e| e.to_string())?;
        }

        zip.finish().map_err(|e| e.to_string())?;
    }

    Ok((buf, manifest))
}

fn normalized_enclosed_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn parse_backup_zip_bytes(bytes: &[u8], limits: ZipLimits) -> Result<ParsedBackupArchive, String> {
    if bytes.len() > limits.max_zip_bytes {
        return Err("Backup archive exceeds maximum upload size limit.".to_string());
    }

    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("Invalid backup ZIP archive: {e}"))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err("Backup archive contains too many entries.".to_string());
    }

    let mut total_uncompressed: u64 = 0;
    let mut wildcards_uncompressed: u64 = 0;

    let mut manifest: Option<BackupManifest> = None;
    let mut user_prefs: Option<serde_json::Value> = None;
    let mut local_extras: Option<serde_json::Value> = None;
    let mut app_config: Option<serde_json::Value> = None;
    let mut wildcards: Vec<WildcardFile> = Vec::new();

    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("Invalid backup ZIP entry: {e}"))?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| "Unsafe path in backup archive (zip-slip rejected).".to_string())?
            .to_path_buf();
        if entry.is_symlink() {
            return Err("Symlinks are not allowed in backup archive.".to_string());
        }
        if entry.is_dir() {
            continue;
        }

        let remaining = limits
            .max_uncompressed_bytes
            .saturating_sub(total_uncompressed);
        let mut entry_bytes = Vec::new();
        entry
            .take(remaining.saturating_add(1))
            .read_to_end(&mut entry_bytes)
            .map_err(|e| format!("Failed reading backup entry: {e}"))?;
        if entry_bytes.len() as u64 > remaining {
            return Err("Backup archive exceeds 128 MB uncompressed size limit.".to_string());
        }
        total_uncompressed = total_uncompressed.saturating_add(entry_bytes.len() as u64);

        let rel = normalized_enclosed_path(&enclosed);
        match rel.as_str() {
            "manifest.json" => {
                let parsed: BackupManifest = serde_json::from_slice(&entry_bytes)
                    .map_err(|e| format!("Invalid manifest.json: {e}"))?;
                if parsed.schema_version == 0 || parsed.schema_version > SCHEMA_VERSION {
                    return Err(format!(
                        "Unsupported backup schema version: {}",
                        parsed.schema_version
                    ));
                }
                manifest = Some(parsed);
            }
            "user_prefs.json" => {
                let parsed: serde_json::Value = serde_json::from_slice(&entry_bytes)
                    .map_err(|e| format!("Invalid user_prefs.json: {e}"))?;
                if !parsed.is_object() {
                    return Err("user_prefs.json must be a JSON object.".to_string());
                }
                user_prefs = Some(parsed);
            }
            "local_extras.json" => {
                let parsed: serde_json::Value = serde_json::from_slice(&entry_bytes)
                    .map_err(|e| format!("Invalid local_extras.json: {e}"))?;
                if !parsed.is_object() {
                    return Err("local_extras.json must be a JSON object.".to_string());
                }
                local_extras = Some(parsed);
            }
            "app_config.json" => {
                let parsed: serde_json::Value = serde_json::from_slice(&entry_bytes)
                    .map_err(|e| format!("Invalid app_config.json: {e}"))?;
                if !parsed.is_object() {
                    return Err("app_config.json must be a JSON object.".to_string());
                }
                app_config = Some(parsed);
            }
            _ if rel.starts_with("wildcards/") => {
                let wc_rel = rel.strip_prefix("wildcards/").unwrap_or("");
                if !wildcards::is_valid_relative_path(wc_rel) {
                    continue;
                }
                if wildcards_uncompressed.saturating_add(entry_bytes.len() as u64)
                    > limits.max_wildcards_bytes
                {
                    return Err("Backup wildcards exceed 10 MB size limit.".to_string());
                }
                wildcards_uncompressed =
                    wildcards_uncompressed.saturating_add(entry_bytes.len() as u64);
                let content = String::from_utf8_lossy(&entry_bytes)
                    .trim_start_matches('\u{feff}')
                    .to_string();
                wildcards.push(WildcardFile {
                    path: wc_rel.to_string(),
                    content,
                });
            }
            _ => {
                // Unknown auxiliary entries are ignored rather than written to disk.
            }
        }
    }

    let mut manifest =
        manifest.ok_or_else(|| "Backup archive is missing manifest.json.".to_string())?;
    let user_prefs = user_prefs.unwrap_or_else(|| serde_json::json!({}));
    let local_extras = local_extras.unwrap_or_else(|| serde_json::json!({}));
    manifest.counts = compute_counts(&user_prefs, &local_extras, wildcards.len());
    manifest.includes_config = app_config.is_some();

    let machine_local_paths = app_config
        .as_ref()
        .map(|raw| {
            let cfg_obj = raw.get("config").unwrap_or(raw);
            present_machine_local_paths(cfg_obj)
        })
        .unwrap_or_default();
    let wildcard_paths = wildcards.iter().map(|w| w.path.clone()).collect();

    Ok(ParsedBackupArchive {
        inspection: BackupInspection {
            manifest,
            user_prefs,
            local_extras,
            has_app_config: app_config.is_some(),
            machine_local_paths,
            wildcard_paths,
        },
        app_config,
        wildcards,
    })
}

/// Inspect a backup `.zip` byte slice and return its manifest, parsed payloads,
/// and summary counts without mutating any local files or config.
pub fn inspect_backup_zip_bytes(bytes: &[u8]) -> Result<BackupInspection, String> {
    Ok(parse_backup_zip_bytes(bytes, ZipLimits::default())?.inspection)
}

/// Merge a backup's `app_config.json` payload onto `current`, always preserving
/// every secret on `current` and keeping machine-local paths unless
/// `include_machine_local_paths` is explicitly `true`.
pub fn merge_restored_config(
    current: &AppConfig,
    raw_app_config: &serde_json::Value,
    include_machine_local_paths: bool,
) -> Result<AppConfig, String> {
    let mut cfg_val = raw_app_config
        .get("config")
        .unwrap_or(raw_app_config)
        .clone();
    strip_secrets_from_config_value(&mut cfg_val);

    let mut restored: AppConfig = serde_json::from_value(cfg_val)
        .map_err(|e| format!("Invalid app_config.json payload: {e}"))?;

    // Always carry forward every secret/credential on the target host.
    preserve_secrets(&mut restored, current);
    preserve_redacted_secrets(&mut restored, current);
    restored
        .novelai_api_key
        .clone_from(&current.novelai_api_key);

    if !include_machine_local_paths {
        restored.comfyui_path.clone_from(&current.comfyui_path);
        restored.venv_path.clone_from(&current.venv_path);
        restored
            .extra_model_paths
            .clone_from(&current.extra_model_paths);
        restored.gallery_path.clone_from(&current.gallery_path);
        restored.tls_cert_path.clone_from(&current.tls_cert_path);
        restored.tls_key_path.clone_from(&current.tls_key_path);
        restored
            .linked_gallery_folders
            .clone_from(&current.linked_gallery_folders);
        restored
            .interrogator_custom_models
            .clone_from(&current.interrogator_custom_models);
        restored.setup_complete = current.setup_complete;
    }

    normalize_config_fields(&mut restored);
    validate_civitai_proxy(restored.civitai_proxy.as_deref())?;
    Ok(restored)
}

/// Apply admin-level items (`app_config.json` and `wildcards/`) from a backup `.zip`
/// onto the running [`AppState`] and disk.
pub async fn apply_backup_admin_data_bytes(
    state: &AppState,
    bytes: &[u8],
    opts: &ApplyBackupAdminOptions,
) -> Result<ApplyBackupAdminResult, String> {
    let owned = bytes.to_vec();
    let parsed =
        tokio::task::spawn_blocking(move || parse_backup_zip_bytes(&owned, ZipLimits::default()))
            .await
            .map_err(|e| e.to_string())??;

    let mut result = ApplyBackupAdminResult::default();

    if opts.apply_app_config {
        if let Some(ref raw_cfg) = parsed.app_config {
            let mut current = state.config.write().await;
            let merged =
                merge_restored_config(&current, raw_cfg, opts.include_machine_local_paths)?;
            save_config(&merged)?;
            state.trust_localhost.store(
                local_trust_enabled(merged.trust_localhost),
                std::sync::atomic::Ordering::SeqCst,
            );
            let upnp_enabled = merged.upnp_enabled;
            *current = merged;
            drop(current);
            crate::lan_upnp::sync(upnp_enabled);
            result.config_applied = true;
        }
    }

    if opts.apply_wildcards && !parsed.wildcards.is_empty() {
        let replace = opts.replace_wildcards;
        let wildcards = parsed.wildcards;
        let written = tokio::task::spawn_blocking(move || -> Result<usize, String> {
            let root = wildcards::wildcards_dir().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            if replace {
                let existing = wildcards::list_in(&root).map_err(|e| e.to_string())?;
                for file in existing.files {
                    wildcards::delete_in(&root, &file.path).map_err(|e| e.to_string())?;
                }
            }
            let mut count = 0;
            for wc in wildcards {
                wildcards::save_in(&root, &wc.path, &wc.content).map_err(|e| e.to_string())?;
                count += 1;
            }
            Ok(count)
        })
        .await
        .map_err(|e| e.to_string())??;
        result.wildcards_written = written;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config_with_secrets() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.default_checkpoint = Some("anima_v1.safetensors".to_string());
        cfg.default_steps = 28;
        cfg.comfyui_path = "D:\\ComfyUI".to_string();
        cfg.venv_path = "D:\\ComfyUI\\venv".to_string();
        cfg.extra_model_paths = Some("E:\\Models".to_string());
        cfg.gallery_path = Some("D:\\Gallery".to_string());
        cfg.tls_cert_path = Some("C:\\certs\\cert.pem".to_string());
        cfg.tls_key_path = Some("C:\\certs\\key.pem".to_string());
        cfg.linked_gallery_folders = vec!["D:\\Linked".to_string()];
        cfg.setup_complete = true;

        cfg.civitai_api_key = Some("secret-civitai-key".to_string());
        cfg.civitai_proxy = Some("http://user:pass@127.0.0.1:8080".to_string());
        cfg.novelai_api_key = Some("secret-nai-key".to_string());
        cfg.elevenlabs_api_key = Some("secret-eleven-key".to_string());
        cfg.fal_api_key = Some("secret-fal-key".to_string());
        cfg.segmind_api_key = Some("secret-segmind-key".to_string());
        cfg.webhook_url = Some("https://hooks.example.com/secret-token".to_string());
        cfg.network_proxy = Some("http://proxy-user:proxy-pass@10.0.0.1:3128".to_string());
        cfg.pip_index_url = Some("https://user:pass@pypi.internal/simple".to_string());
        cfg.llm_external_api_key = "sk-secret-llm".to_string();
        cfg.llm_oauth_refresh_token = "rt-secret-refresh".to_string();
        cfg.llm_oauth_client_id = "cid-secret-client".to_string();
        cfg.llm_oauth_expires_at = 1_999_999_999;
        cfg
    }

    fn sample_bundle() -> BackupBundleInput {
        BackupBundleInput {
            user_prefs: serde_json::json!({
                "characters": { "characters": [{ "id": "c1", "name": "Hana" }], "samples": [] },
                "prompt_presets": { "presets": [{ "id": "p1", "name": "Lighting" }] },
                "styles": { "styles": [{ "id": "s1", "name": "Painterly" }] },
                "lora_presets": {
                    "presets": [{ "id": "lp1", "name": "Outfit" }],
                    "triggers": { "foo.safetensors": "foo_trigger" }
                },
                "profiles": { "profiles": [{ "id": "pr1", "name": "Anime" }] },
                "artist_favourites": { "favourites": [{ "slug": "krenz" }], "categories": [] },
                "prompt_history": [{ "id": "h1", "positivePrompt": "1girl" }]
            }),
            local_extras: serde_json::json!({
                "comic": { "books": [{ "id": "b1", "title": "Book 1" }], "activeBookId": "b1" },
                "generationTabs": { "activeId": "t1", "tabs": [{ "id": "t1", "snapshot": null }] }
            }),
        }
    }

    #[test]
    fn round_trip_export_and_inspect() {
        let cfg = sample_config_with_secrets();
        let (zip_bytes, manifest) = build_backup_zip_with_manifest(BackupBuildParams {
            app_version: "2.4.6".to_string(),
            username: "admin".to_string(),
            bundle: sample_bundle(),
            app_config: Some(cfg),
            wildcards: vec![
                WildcardFile {
                    path: "hair/colors.txt".to_string(),
                    content: "silver\ngold".to_string(),
                },
                WildcardFile {
                    path: "../escape.txt".to_string(),
                    content: "bad".to_string(),
                },
            ],
        })
        .unwrap();

        assert_eq!(manifest.schema_version, SCHEMA_VERSION);
        assert!(manifest.includes_config);
        assert_eq!(manifest.counts.characters, 1);
        assert_eq!(manifest.counts.lora_triggers, 1);
        assert_eq!(manifest.counts.comic_books, 1);
        assert_eq!(manifest.counts.wildcards, 1);

        let inspected = inspect_backup_zip_bytes(&zip_bytes).unwrap();
        assert_eq!(inspected.manifest.counts, manifest.counts);
        assert!(inspected.has_app_config);
        assert_eq!(inspected.wildcard_paths, vec!["hair/colors.txt"]);
        assert!(inspected
            .machine_local_paths
            .contains(&"comfyui_path".to_string()));
        assert!(inspected
            .machine_local_paths
            .contains(&"gallery_path".to_string()));
        assert_eq!(
            inspected.user_prefs["characters"]["characters"][0]["name"],
            "Hana"
        );
        assert_eq!(
            inspected.local_extras["comic"]["books"][0]["title"],
            "Book 1"
        );
    }

    #[test]
    fn strips_all_secrets_from_exported_zip_and_preserves_on_restore() {
        let source_cfg = sample_config_with_secrets();
        let (zip_bytes, _) = build_backup_zip_with_manifest(BackupBuildParams {
            app_version: "2.4.6".to_string(),
            username: "admin".to_string(),
            bundle: sample_bundle(),
            app_config: Some(source_cfg),
            wildcards: vec![],
        })
        .unwrap();

        // Read raw app_config.json directly out of the archive and assert zero secret strings leak.
        let mut archive = ZipArchive::new(Cursor::new(&zip_bytes)).unwrap();
        let mut raw_cfg = String::new();
        archive
            .by_name("app_config.json")
            .unwrap()
            .read_to_string(&mut raw_cfg)
            .unwrap();

        for secret in [
            "secret-civitai-key",
            "user:pass@127.0.0.1",
            "secret-nai-key",
            "secret-eleven-key",
            "secret-fal-key",
            "secret-segmind-key",
            "secret-token",
            "proxy-user:proxy-pass",
            "user:pass@pypi.internal",
            "sk-secret-llm",
            "rt-secret-refresh",
            "cid-secret-client",
        ] {
            assert!(
                !raw_cfg.contains(secret),
                "secret value `{secret}` leaked into app_config.json"
            );
        }

        // Restoring onto a target config preserves target's secrets & keeps machine-local paths off by default.
        let mut target_cfg = AppConfig::default();
        target_cfg.comfyui_path = "/opt/comfyui".to_string();
        target_cfg.gallery_path = Some("/srv/gallery".to_string());
        target_cfg.setup_complete = false;
        target_cfg.civitai_api_key = Some("target-civitai-key".to_string());
        target_cfg.novelai_api_key = Some("target-nai-key".to_string());
        target_cfg.elevenlabs_api_key = Some("target-eleven-key".to_string());
        target_cfg.llm_external_api_key = "target-llm-key".to_string();

        let parsed_cfg: serde_json::Value = serde_json::from_str(&raw_cfg).unwrap();
        let merged_no_local = merge_restored_config(&target_cfg, &parsed_cfg, false).unwrap();
        assert_eq!(merged_no_local.default_steps, 28);
        assert_eq!(
            merged_no_local.default_checkpoint.as_deref(),
            Some("anima_v1.safetensors")
        );
        assert_eq!(merged_no_local.comfyui_path, "/opt/comfyui");
        assert_eq!(
            merged_no_local.gallery_path.as_deref(),
            Some("/srv/gallery")
        );
        assert!(!merged_no_local.setup_complete);
        assert_eq!(
            merged_no_local.civitai_api_key.as_deref(),
            Some("target-civitai-key")
        );
        assert_eq!(
            merged_no_local.novelai_api_key.as_deref(),
            Some("target-nai-key")
        );
        assert_eq!(
            merged_no_local.elevenlabs_api_key.as_deref(),
            Some("target-eleven-key")
        );
        assert_eq!(merged_no_local.llm_external_api_key, "target-llm-key");

        // Opting into machine-local paths copies paths from backup while still keeping secrets.
        let merged_with_local = merge_restored_config(&target_cfg, &parsed_cfg, true).unwrap();
        assert_eq!(merged_with_local.comfyui_path, "D:\\ComfyUI");
        assert_eq!(
            merged_with_local.gallery_path.as_deref(),
            Some("D:\\Gallery")
        );
        assert!(merged_with_local.setup_complete);
        assert_eq!(
            merged_with_local.civitai_api_key.as_deref(),
            Some("target-civitai-key")
        );
    }

    #[test]
    fn rejects_zip_slip_entries() {
        let mut buf = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            zip.start_file("../evil.json", opts).unwrap();
            zip.write_all(b"{}").unwrap();
            zip.finish().unwrap();
        }
        let err = inspect_backup_zip_bytes(&buf).unwrap_err();
        assert!(err.contains("zip-slip") || err.contains("Unsafe"));
    }

    #[test]
    fn rejects_oversized_uncompressed_archive() {
        let mut buf = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file("manifest.json", opts).unwrap();
            zip.write_all(
                br#"{"schema_version":1,"app_version":"2.4.6","exported_at":"2026-01-01T00:00:00Z","username":"admin","includes_config":false,"counts":{}}"#,
            )
            .unwrap();
            zip.start_file("user_prefs.json", opts).unwrap();
            // Highly compressible payload exceeding a 1024-byte test limit.
            zip.write_all(&vec![b' '; 2048]).unwrap();
            zip.finish().unwrap();
        }

        let err = parse_backup_zip_bytes(
            &buf,
            ZipLimits {
                max_zip_bytes: MAX_ZIP_UPLOAD_BYTES,
                max_uncompressed_bytes: 1024,
                max_wildcards_bytes: MAX_WILDCARDS_BYTES,
            },
        )
        .unwrap_err();
        assert!(err.contains("128 MB") || err.contains("limit"));
    }

    #[test]
    fn enforces_wildcards_size_cap_on_export_and_inspect() {
        let big = "a".repeat(2048);
        let small = "b".repeat(512);
        let (zip_bytes, manifest) = build_backup_zip_with_limits(
            BackupBuildParams {
                app_version: "2.4.6".to_string(),
                username: "admin".to_string(),
                bundle: BackupBundleInput::default(),
                app_config: None,
                wildcards: vec![
                    WildcardFile {
                        path: "big.txt".to_string(),
                        content: big,
                    },
                    WildcardFile {
                        path: "small.txt".to_string(),
                        content: small,
                    },
                ],
            },
            1024,
        )
        .unwrap();
        assert_eq!(manifest.counts.wildcards, 1);
        let inspected = inspect_backup_zip_bytes(&zip_bytes).unwrap();
        assert_eq!(inspected.wildcard_paths, vec!["small.txt"]);
    }
}
