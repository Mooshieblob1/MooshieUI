use std::sync::Arc;
use std::time::Duration;

use tauri::State;

use crate::config::{
    config_to_client_json, normalize_config_fields, preserve_secrets, save_config, AppConfig,
};
use crate::error::AppError;
use crate::state::AppState;
use crate::webserver;

/// The config as the desktop owner's UI sees it: everything, minus the
/// external-LLM credential. The settings page reads the key-free
/// `get_llm_provider` projection for that, and a copy cached here would only
/// be sent back, stale, by the next autosave.
#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, AppError> {
    let config = state.config.read().await;
    Ok(config_to_client_json(&config, true)?)
}

/// Apply `edit` to the live config and save it before releasing the write
/// lock. Saving a snapshot after dropping the lock lets a concurrent
/// `update_config` land in between and then be overwritten on disk by the
/// older snapshot. Same lock scope as `update_config` itself.
async fn edit_and_save<R>(
    state: &AppState,
    edit: impl FnOnce(&mut AppConfig) -> R,
) -> Result<R, AppError> {
    let mut cfg = state.config.write().await;
    let out = edit(&mut cfg);
    save_config(&cfg).map_err(AppError::Other)?;
    Ok(out)
}

#[tauri::command]
pub async fn update_config(
    state: State<'_, Arc<AppState>>,
    mut config: AppConfig,
) -> Result<(), AppError> {
    normalize_config_fields(&mut config);
    // Held across the save so a concurrent provider write cannot land between
    // the read that carries the key forward and the write that replaces config.
    let mut current = state.config.write().await;
    preserve_secrets(&mut config, &current);
    save_config(&config).map_err(AppError::Other)?;
    state.trust_localhost.store(
        crate::config::local_trust_enabled(config.trust_localhost),
        std::sync::atomic::Ordering::SeqCst,
    );
    *current = config;
    Ok(())
}

/// Return the resolved gallery directory path.
/// If a custom `gallery_path` is set, returns that; otherwise returns the default.
#[tauri::command]
pub async fn get_gallery_path(state: State<'_, Arc<AppState>>) -> Result<String, AppError> {
    let cfg = state.config.read().await;
    if let Some(ref custom) = cfg.gallery_path {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let dir = crate::config::app_data_dir()
        .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
        .join("gallery");
    Ok(dir.to_string_lossy().into_owned())
}

/// Set a custom gallery directory. Pass an empty string to reset to default.
/// Validates the path is a writable directory (or can be created).
#[tauri::command]
pub async fn set_gallery_path(
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<String, AppError> {
    let trimmed = path.trim().to_string();

    let resolved = if trimmed.is_empty() {
        // Reset to default.
        edit_and_save(&state, |cfg| cfg.gallery_path = None).await?;
        let dir = crate::config::app_data_dir()
            .ok_or_else(|| AppError::Other("Cannot find app data directory".into()))?
            .join("gallery");
        std::fs::create_dir_all(&dir)?;
        dir.to_string_lossy().into_owned()
    } else {
        let p = std::path::Path::new(&trimmed);
        // Create if it doesn't exist
        std::fs::create_dir_all(p)
            .map_err(|e| AppError::Other(format!("Cannot create gallery directory: {}", e)))?;
        // Verify writable by creating a temp file
        let test_file = p.join(".mooshie_write_test");
        std::fs::write(&test_file, b"test")
            .map_err(|e| AppError::Other(format!("Directory is not writable: {}", e)))?;
        let _ = std::fs::remove_file(&test_file);

        edit_and_save(&state, |cfg| cfg.gallery_path = Some(trimmed.clone())).await?;
        trimmed
    };

    Ok(resolved)
}

/// Switch to browser mode at runtime: save config, start the web server,
/// open the browser, and hide the Tauri window.
#[tauri::command]
pub async fn switch_to_browser_mode(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), AppError> {
    log::info!("switch_to_browser_mode: called");

    // Save browser_mode = true.
    let (mut port, lan_enabled) = edit_and_save(&state, |cfg| {
        cfg.browser_mode = true;
        (cfg.ui_server_port, cfg.lan_enabled)
    })
    .await?;
    log::info!(
        "switch_to_browser_mode: config saved, port={}, lan={}",
        port,
        lan_enabled
    );

    // Re-arm the heartbeat watchdog (in case we came from app mode)
    state
        .app_mode_active
        .store(false, std::sync::atomic::Ordering::SeqCst);
    // Refresh the heartbeat so the watchdog doesn't immediately fire
    {
        let mut hb = state.last_heartbeat.lock().await;
        *hb = std::time::Instant::now();
    }

    // Only start the web server if it isn't already running
    let server_was_running = state
        .web_server_running
        .load(std::sync::atomic::Ordering::SeqCst);
    log::info!(
        "switch_to_browser_mode: web_server_running={}",
        server_was_running
    );
    if !server_was_running {
        let shared_state: Arc<AppState> = state.inner().clone();
        let state_for_server = shared_state.clone();
        // Bind synchronously so we can open the browser at the right port
        // even when fallback ports were used.
        let (actual_port, _handle) =
            webserver::start_server(state_for_server, port, lan_enabled).await;
        if actual_port != port {
            edit_and_save(&state, |cfg| cfg.ui_server_port = actual_port).await?;
            log::info!(
                "switch_to_browser_mode: persisted fallback ui_server_port={}",
                actual_port
            );
        }
        port = actual_port;
    }

    // Start a new heartbeat watchdog — the previous one exits when
    // app_mode_active is set, so we need a fresh one for the new browser session.
    // Only in single-user browser mode: with LAN access enabled, multiple users
    // may connect (often from phones that suspend background tab timers), and we
    // must NOT shut down and cancel a running generation when one tab goes quiet.
    if !lan_enabled {
        let shared_state: Arc<AppState> = state.inner().clone();
        // 120s: browsers throttle background setInterval to ~1 min;
        // we need a timeout well above that to avoid killing the
        // process while generation is running in a background tab.
        webserver::start_heartbeat_watchdog(shared_state, 120);
    }

    // Open the browser
    let url = format!("http://127.0.0.1:{}", port);
    log::info!("switch_to_browser_mode: opening {}", url);
    match open::that(&url) {
        Ok(_) => log::info!("switch_to_browser_mode: open::that succeeded"),
        Err(e) => {
            log::error!("switch_to_browser_mode: open::that failed: {}", e);
            edit_and_save(&state, |cfg| cfg.browser_mode = false).await?;
            state
                .app_mode_active
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return Err(AppError::Other(format!(
                "Failed to open browser at {}: {}",
                url, e
            )));
        }
    }

    // Hide the Tauri window after returning — hiding synchronously from an
    // IPC call made by that same webview can deadlock the invoke on Windows.
    let app_for_hide = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        use tauri::Manager;
        if let Some(win) = app_for_hide.get_webview_window("main") {
            log::info!("switch_to_browser_mode: hiding window");
            let _ = win.hide();
        } else {
            log::warn!("switch_to_browser_mode: no 'main' window to hide");
        }
    });

    log::info!("switch_to_browser_mode: done");
    Ok(())
}
