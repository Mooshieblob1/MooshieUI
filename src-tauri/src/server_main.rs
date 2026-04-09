//! Headless MooshieUI server — no Tauri, no GUI.
//!
//! Serves the Svelte frontend via axum, manages ComfyUI as a child process,
//! and relays WebSocket events to SSE clients. Designed for Docker / K8s.

use std::sync::Arc;

use comfyui_desktop_lib::auth::AuthState;
use comfyui_desktop_lib::comfyui::{process, websocket};
use comfyui_desktop_lib::config::load_persisted_config;
use comfyui_desktop_lib::state::AppState;
use comfyui_desktop_lib::{temp_images, webserver};

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("MooshieUI Server starting...");

    let config = load_persisted_config();
    let port = config.ui_server_port;
    let auto_start = config.auto_start;
    let state = Arc::new(AppState::new(config));

    // Seed admin account from env vars if provided and not already created.
    // Usage: MOOSHIEUI_ADMIN_USER=blob MOOSHIEUI_ADMIN_PASS=secret
    if let (Ok(user), Ok(pass)) = (
        std::env::var("MOOSHIEUI_ADMIN_USER"),
        std::env::var("MOOSHIEUI_ADMIN_PASS"),
    ) {
        if !user.trim().is_empty() && pass.len() >= 4 {
            let auth = AuthState::new();
            match auth.create_account(&user, &pass) {
                Ok(()) => {
                    // Promote to admin so they have full access remotely (account management, settings, etc.)
                    let _ = auth.set_account_role(&user, "admin");
                    log::info!("Created admin account '{}' from environment", user);
                }
                Err(e) if e.contains("already exists") => {
                    log::debug!("Admin account '{}' already exists, skipping", user);
                }
                Err(e) => {
                    log::error!("Failed to create admin account: {}", e);
                }
            }
        }
    }

    // Clean up and create temp image directory
    temp_images::init();

    // Start the web server (always LAN-enabled in server mode)
    let server_state = state.clone();
    let server_handle = tokio::spawn(async move {
        webserver::start_server(server_state, port, true).await;
    });

    log::info!("Web server listening on 0.0.0.0:{}", port);

    // Auto-start ComfyUI if configured
    if auto_start {
        log::info!("Auto-starting ComfyUI...");
        match process::start_comfyui_process(&state).await {
            Ok(result) => {
                log::info!("ComfyUI start result: {:?}", result);

                // Wait for ComfyUI to become ready
                match process::wait_for_ready(&state, 120).await {
                    Ok(()) => {
                        log::info!("ComfyUI server is ready");
                        let event_tx = state.event_tx.clone();
                        if let Err(e) =
                            websocket::connect_websocket_headless(&state, event_tx).await
                        {
                            log::error!("Failed to connect WebSocket: {}", e);
                        }
                        state.broadcast("comfyui:server_ready", serde_json::json!(null));
                    }
                    Err(e) => {
                        log::error!("ComfyUI failed to become ready: {}", e);
                        state.broadcast(
                            "comfyui:server_error",
                            serde_json::json!({
                                "error": e.to_string(),
                                "crashed": e.to_string().contains("exited with"),
                            }),
                        );
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to start ComfyUI: {}", e);
            }
        }
    }

    // Wait for shutdown signal
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for ctrl-c");
    log::info!("Shutdown signal received, cleaning up...");

    // Kill ComfyUI process
    let mut proc = state.comfyui_process.lock().await;
    if let Some(ref mut child) = *proc {
        log::info!("Shutting down ComfyUI process...");
        let _ = child.start_kill();
        *proc = None;
    }

    server_handle.abort();
    log::info!("MooshieUI Server stopped.");
}
