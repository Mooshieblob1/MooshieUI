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
    let admin_user = std::env::var("MOOSHIEUI_ADMIN_USER").ok();
    let admin_pass = std::env::var("MOOSHIEUI_ADMIN_PASS").ok();

    match (&admin_user, &admin_pass) {
        (Some(user), Some(pass)) if !user.trim().is_empty() && pass.len() >= 4 => {
            if pass == "changeme" {
                log::warn!("============================================================");
                log::warn!("  Using default admin password 'changeme'.");
                log::warn!("  Change MOOSHIEUI_ADMIN_PASS before exposing this server!");
                log::warn!("============================================================");
            }
            let auth = AuthState::new();
            match auth.create_account(user, pass) {
                Ok(()) => {
                    // Promote to admin so they have full access remotely (account management, settings, etc.)
                    let _ = auth.set_account_role(user, "admin");
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
        (Some(_), Some(pass)) if pass.len() < 4 => {
            log::error!(
                "MOOSHIEUI_ADMIN_PASS is set but too short ({} chars, minimum 4). \
                 No admin account was created — you will be locked out!",
                pass.len()
            );
            std::process::exit(1);
        }
        (Some(_), None) | (None, Some(_)) => {
            log::error!(
                "Both MOOSHIEUI_ADMIN_USER and MOOSHIEUI_ADMIN_PASS must be set together. \
                 No admin account was created — you will be locked out!"
            );
            std::process::exit(1);
        }
        _ => {
            // Neither env var set — desktop mode or pre-existing accounts.
            log::debug!("No admin env vars set, skipping admin seeding");
        }
    }

    // Clean up and create temp image directory
    temp_images::init();

    // Start the web server (always LAN-enabled in server mode)
    let server_state = state.clone();
    let (actual_port, server_handle) = webserver::start_server(server_state, port, true).await;

    log::info!("Web server listening on 0.0.0.0:{}", actual_port);

    // Auto-start ComfyUI if configured
    if auto_start {
        let multi_gpu = !state.gpu_manager.is_single_worker();

        if multi_gpu {
            // Multi-GPU mode: start all configured workers
            log::info!(
                "Auto-starting ComfyUI on {} GPU workers...",
                state.gpu_manager.workers.len()
            );
            let results = process::start_all_workers(&state).await;
            for (wid, res) in &results {
                if let Err(e) = res {
                    log::error!("Worker {} failed to start: {}", wid, e);
                }
            }

            // Wait for all workers to become ready (in parallel)
            process::wait_all_workers_ready(&state, 120).await;

            // Connect WebSocket for each ready worker
            for worker in &state.gpu_manager.workers {
                let status = *worker.status.read().await;
                if status == comfyui_desktop_lib::comfyui::gpu_manager::WorkerStatus::Idle {
                    let event_tx = state.event_tx.clone();
                    if let Err(e) =
                        websocket::connect_websocket_for_worker(&state, worker, event_tx).await
                    {
                        log::error!("Worker {} WebSocket failed: {}", worker.id, e);
                    }
                }
            }

            state.broadcast("comfyui:server_ready", serde_json::json!(null));
        } else {
            // Single-worker mode (backward compat)
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
    }

    // Wait for shutdown signal
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for ctrl-c");
    log::info!("Shutdown signal received, cleaning up...");

    // Kill ComfyUI process(es)
    if !state.gpu_manager.is_single_worker() {
        log::info!("Shutting down all GPU workers...");
        process::stop_all_workers(&state).await;
    } else {
        let mut proc = state.comfyui_process.lock().await;
        if let Some(ref mut child) = *proc {
            log::info!("Shutting down ComfyUI process...");
            let _ = child.start_kill();
            *proc = None;
        }
    }

    server_handle.abort();
    log::info!("MooshieUI Server stopped.");
}
