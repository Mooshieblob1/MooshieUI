use std::sync::Arc;

use tauri::State;

use crate::config::{save_config, AppConfig};
use crate::error::AppError;
use crate::state::AppState;
use crate::webserver;

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig, AppError> {
    let config = state.config.read().await;
    Ok(config.clone())
}

#[tauri::command]
pub async fn update_config(
    state: State<'_, Arc<AppState>>,
    config: AppConfig,
) -> Result<(), AppError> {
    save_config(&config).map_err(AppError::Other)?;
    let mut current = state.config.write().await;
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
        // Reset to default
        let mut cfg = state.config.write().await;
        cfg.gallery_path = None;
        save_config(&cfg).map_err(AppError::Other)?;
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

        let mut cfg = state.config.write().await;
        cfg.gallery_path = Some(trimmed.clone());
        save_config(&cfg).map_err(AppError::Other)?;
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

    // Save browser_mode = true
    let (mut port, lan_enabled) = {
        let mut cfg = state.config.write().await;
        cfg.browser_mode = true;
        save_config(&cfg).map_err(AppError::Other)?;
        (cfg.ui_server_port, cfg.lan_enabled)
    };
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
        port = actual_port;
    }

    // Always start a new heartbeat watchdog — the previous one exits when
    // app_mode_active is set, so we need a fresh one for the new browser session.
    {
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
        Err(e) => log::error!("switch_to_browser_mode: open::that failed: {}", e),
    }

    // Hide the Tauri window
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("main") {
        log::info!("switch_to_browser_mode: hiding window");
        let _ = win.hide();
    } else {
        log::warn!("switch_to_browser_mode: no 'main' window to hide");
    }

    log::info!("switch_to_browser_mode: done");
    Ok(())
}
