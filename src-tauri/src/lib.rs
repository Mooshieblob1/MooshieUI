pub mod auth;
pub mod comfyui;
pub mod commands;
pub mod config;
pub mod error;
pub mod gallery_index;
#[cfg(any(feature = "desktop", feature = "server"))]
pub mod interrogator;
pub mod jxl;
pub mod log_buffer;
pub mod metadata;
#[cfg(feature = "desktop")]
pub mod setup;
pub mod state;
pub mod temp_images;
pub mod templates;
pub mod user_prefs;
pub mod webserver;

use std::sync::Arc;

use config::load_persisted_config;
use state::AppState;

/// Fix Wayland rendering in AppImage builds.
///
/// The linuxdeploy GTK plugin sets `GDK_BACKEND=x11` and bundles its own
/// library stubs. On Wayland compositors (especially Arch-based distros like
/// CachyOS), WebKitGTK shows a white screen because the bundled libs don't
/// include `libwayland-client`. We fix this by:
///   1. Removing `GDK_BACKEND=x11` so GTK picks the native Wayland backend.
///   2. Setting `LD_PRELOAD` to the system `libwayland-client.so.0`.
///   3. Re-executing the process with the corrected environment.
///
/// A sentinel env var `_MOOSHIEUI_WAYLAND_FIXED` prevents infinite re-exec.
#[cfg(all(feature = "desktop", target_os = "linux"))]
fn fix_wayland_appimage_env() {
    // Only relevant inside an AppImage on a Wayland session, and only once.
    if std::env::var("APPIMAGE").is_err()
        || std::env::var("WAYLAND_DISPLAY").is_err()
        || std::env::var("_MOOSHIEUI_WAYLAND_FIXED").is_ok()
    {
        return;
    }

    // Search common library paths for the versioned libwayland-client.
    // Arch/CachyOS uses /usr/lib/, Debian/Ubuntu uses the multiarch path,
    // Fedora/RHEL uses /usr/lib64/.
    let search_paths = [
        "/usr/lib/libwayland-client.so.0",
        "/usr/lib/x86_64-linux-gnu/libwayland-client.so.0",
        "/usr/lib64/libwayland-client.so.0",
        // Unversioned fallback
        "/usr/lib/libwayland-client.so",
        "/usr/lib/x86_64-linux-gnu/libwayland-client.so",
        "/usr/lib64/libwayland-client.so",
    ];

    let wayland_lib = match search_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists())
    {
        Some(path) => *path,
        None => return, // No libwayland-client found; nothing we can do
    };

    // Remove the forced X11 backend so GTK uses native Wayland
    std::env::remove_var("GDK_BACKEND");

    // Prepend to any existing LD_PRELOAD
    let preload = match std::env::var("LD_PRELOAD") {
        Ok(existing) if !existing.is_empty() => format!("{}:{}", wayland_lib, existing),
        _ => wayland_lib.to_string(),
    };
    std::env::set_var("LD_PRELOAD", &preload);
    std::env::set_var("_MOOSHIEUI_WAYLAND_FIXED", "1");

    // Re-exec ourselves with the corrected environment
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();

    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(&exe).args(&args).exec();
    // exec() only returns on error
    eprintln!("Failed to re-exec for Wayland fix: {}", err);
}

#[cfg(feature = "desktop")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::{Manager, RunEvent};

    // Enable Rust log output in desktop mode. A ring-buffer logger is used
    // (instead of plain env_logger) so the diagnostics export can include
    // recent Rust-side logs even when the user has no dev console access.
    log_buffer::init();

    // Fix WebKitGTK scroll jank and rendering glitches on NVIDIA + Wayland.
    // The DMA-BUF renderer is broken with NVIDIA proprietary drivers.
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        fix_wayland_appimage_env();
    }

    let config = load_persisted_config();
    let browser_mode = config.browser_mode;
    let ui_server_port = config.ui_server_port;
    let lan_enabled = config.lan_enabled;

    let app_state = Arc::new(AppState::new(config));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .setup(move |_app| {
            // Clean up and create temp image directory
            temp_images::init();

            // Start the shared cleanup reactors unconditionally so workers
            // get released after each prompt completes, even in pure Tauri
            // desktop mode where the embedded web server is not started.
            {
                let shared_state: Arc<AppState> = _app.state::<Arc<AppState>>().inner().clone();
                webserver::spawn_prompt_cleanup_reactor(shared_state.clone());
                webserver::spawn_stuck_worker_watchdog(shared_state);
            }

            // Store the AppHandle so the web server can show/hide the window later.
            {
                let shared_state: Arc<AppState> = _app.state::<Arc<AppState>>().inner().clone();
                let handle = _app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    *shared_state.app_handle.lock().await = Some(handle);
                });
            }

            // In browser mode: hide the window, start web server, open browser
            if browser_mode {
                use tauri::Manager;
                if let Some(main_window) = _app.get_webview_window("main") {
                    let _ = main_window.hide();
                }

                // Share the same AppState between Tauri and the web server
                let shared_state: Arc<AppState> = _app.state::<Arc<AppState>>().inner().clone();

                let state_for_server = shared_state.clone();
                // Bind synchronously so we know the actual port (it may have
                // fallen back from `ui_server_port` if that port was in use).
                let actual_port = tauri::async_runtime::block_on(async move {
                    let (p, _handle) =
                        webserver::start_server(state_for_server, ui_server_port, lan_enabled)
                            .await;
                    p
                });
                // Only start heartbeat watchdog in single-user browser mode.
                // With LAN access enabled, multiple users may connect and we
                // must NOT shut down when one browser tab closes.
                if !lan_enabled {
                    let state_for_watchdog = shared_state.clone();
                    tauri::async_runtime::spawn(async move {
                        // 120s: browsers throttle background setInterval to ~1 min;
                        // we need a timeout well above that to avoid killing the
                        // process while generation is running in a background tab.
                        webserver::start_heartbeat_watchdog(state_for_watchdog, 120);
                    });
                }

                // Open the default browser
                let url = format!("http://127.0.0.1:{}", actual_port);
                log::info!("Opening browser at {}", url);
                let _ = open::that(&url);
            } else {
                // Normal app mode — configure WebView
                #[cfg(target_os = "linux")]
                {
                    use tauri::Manager;
                    if let Some(main_window) = _app.get_webview_window("main") {
                        let _ = main_window.with_webview(|webview| {
                            use webkit2gtk::WebViewExt;
                            if let Some(settings) = webview.inner().settings() {
                                use webkit2gtk::SettingsExt;
                                settings.set_enable_smooth_scrolling(true);
                                settings.set_enable_page_cache(true);
                                settings.set_hardware_acceleration_policy(
                                    webkit2gtk::HardwareAccelerationPolicy::Always,
                                );
                                settings.set_enable_developer_extras(true);
                            }
                        });
                    }
                }
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("thumbnail", |ctx, request, responder| {
            let _app_handle = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let uri = request.uri().to_string();
                // URL format varies by platform:
                //   macOS/Linux: thumbnail://localhost/{filename}?size={max_size}
                //   Windows:     https://thumbnail.localhost/{filename}?size={max_size}
                let path = uri
                    .strip_prefix("https://thumbnail.localhost/")
                    .or_else(|| uri.strip_prefix("http://thumbnail.localhost/"))
                    .or_else(|| uri.strip_prefix("thumbnail://localhost/"))
                    .or_else(|| uri.strip_prefix("thumbnail:///"))
                    .unwrap_or("");
                let (filename_encoded, query) = path.split_once('?').unwrap_or((path, ""));
                let filename = percent_encoding::percent_decode_str(filename_encoded)
                    .decode_utf8()
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| filename_encoded.to_string());
                let max_size: u32 = query
                    .split('&')
                    .find_map(|p| p.strip_prefix("size="))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(256);

                let gallery_dir = match config::gallery_dir() {
                    Some(d) => d,
                    None => {
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(500)
                                .body(b"No app data dir".to_vec())
                                .unwrap(),
                        );
                        return;
                    }
                };

                match commands::api::generate_thumbnail(&gallery_dir, &filename, max_size) {
                    Ok(data) => {
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(200)
                                .header("Content-Type", "image/webp")
                                .header("Cache-Control", "no-cache")
                                .body(data)
                                .unwrap(),
                        );
                    }
                    Err(e) => {
                        log::warn!("Thumbnail generation failed for '{}': {}", filename, e);
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(404)
                                .body(format!("Thumbnail error: {}", e).into_bytes())
                                .unwrap(),
                        );
                    }
                }
            });
        })
        .register_asynchronous_uri_scheme_protocol("gallery", |ctx, request, responder| {
            let _app_handle = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let uri = request.uri().to_string();
                // URL format varies by platform:
                //   macOS/Linux: gallery://localhost/{filename}
                //   Windows:     https://gallery.localhost/{filename}
                let path = uri
                    .strip_prefix("https://gallery.localhost/")
                    .or_else(|| uri.strip_prefix("http://gallery.localhost/"))
                    .or_else(|| uri.strip_prefix("gallery://localhost/"))
                    .or_else(|| uri.strip_prefix("gallery:///"))
                    .unwrap_or("");
                let (filename_encoded, _query) = path.split_once('?').unwrap_or((path, ""));
                let filename = percent_encoding::percent_decode_str(filename_encoded)
                    .decode_utf8()
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| filename_encoded.to_string());

                if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                    responder.respond(
                        tauri::http::Response::builder()
                            .status(400)
                            .body(b"Invalid filename".to_vec())
                            .unwrap(),
                    );
                    return;
                }

                let gallery_dir = match config::gallery_dir() {
                    Some(d) => d,
                    None => {
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(500)
                                .body(b"No gallery dir".to_vec())
                                .unwrap(),
                        );
                        return;
                    }
                };

                let file_path = gallery_dir.join(&filename);
                match std::fs::read(&file_path) {
                    Ok(data) => {
                        // Detect content type from extension
                        let content_type = if filename.ends_with(".webp") {
                            "image/webp"
                        } else if filename.ends_with(".jpg") || filename.ends_with(".jpeg") {
                            "image/jpeg"
                        } else {
                            "image/png"
                        };
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(200)
                                .header("Content-Type", content_type)
                                .header("Cache-Control", "no-cache")
                                .body(data)
                                .unwrap(),
                        );
                    }
                    Err(e) => {
                        log::warn!("Gallery image read failed for '{}': {}", filename, e);
                        responder.respond(
                            tauri::http::Response::builder()
                                .status(404)
                                .body(format!("Not found: {}", e).into_bytes())
                                .unwrap(),
                        );
                    }
                }
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::server::start_comfyui,
            commands::server::stop_comfyui,
            commands::server::check_server_health,
            commands::api::get_models,
            commands::api::get_samplers,
            commands::api::get_embeddings,
            commands::api::get_queue,
            commands::api::get_history,
            commands::api::interrupt_generation,
            commands::api::delete_queue_item,
            commands::api::upload_image,
            commands::api::upload_image_bytes,
            commands::api::get_output_image,
            commands::api::get_client_id,
            commands::api::download_model,
            commands::api::get_model_install_dirs,
            commands::api::open_directory,
            commands::api::save_image_file,
            commands::api::embed_png_metadata_bytes,
            commands::api::save_to_gallery,
            commands::api::save_to_gallery_bytes,
            commands::api::save_to_gallery_temp,
            commands::api::list_gallery_images,
            commands::api::list_gallery_image_entries,
            commands::api::load_gallery_image,
            commands::api::load_gallery_image_display,
            commands::api::load_gallery_image_png,
            commands::api::read_temp_image,
            commands::api::get_gallery_image_path,
            commands::api::delete_gallery_image,
            commands::api::rename_gallery_image,
            commands::api::copy_image_to_clipboard,
            commands::api::copy_bytes_to_clipboard,
            commands::api::find_model_by_hash,
            commands::api::hash_model_file,
            commands::api::civitai_lookup_hash,
            commands::api::cdn_proxy_fetch,
            commands::api::civitai_search_models,
            commands::api::civitai_list_architectures,
            commands::api::read_modelspec,
            commands::api::get_lora_civitai_info,
            commands::api::get_checkpoint_civitai_info,
            commands::api::read_image_metadata,
            commands::api::read_image_metadata_bytes,
            commands::api::read_image_metadata_path,
            commands::api::fetch_release_notes,
            commands::api::import_image_directory,
            commands::api::export_logs,
            commands::api::append_frontend_logs,
            commands::api::get_logs,
            commands::api::check_node_available,
            commands::api::is_custom_node_installed,
            commands::api::install_custom_node,
            commands::api::install_pip_package,
            commands::websocket::connect_ws,
            commands::websocket::disconnect_ws,
            commands::workflow::generate,
            commands::workflow::generate_controlnet_preprocessor_preview,
            commands::config::get_config,
            commands::config::update_config,
            commands::config::get_gallery_path,
            commands::config::set_gallery_path,
            commands::config::switch_to_browser_mode,
            commands::interrogator::interrogate_image,
            commands::interrogator::interrogate_image_path,
            commands::interrogator::interrogate_gallery_image,
            commands::interrogator::interrogate_clipboard,
            commands::api::fetch_cached_image,
            commands::api::read_clipboard_image,
            commands::api::get_gpu_stats,
            commands::api::check_attention_backend,
            commands::api::install_attention_backend,
            commands::api::get_compute_capability,
            setup::check_setup,
            setup::detect_gpu,
            setup::run_setup,
            setup::set_install_path,
            setup::get_install_path,
            setup::detect_model_directories,
            setup::move_installation,
            setup::reinstall_pytorch,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { .. } = event {
            let state = app_handle.state::<Arc<AppState>>();
            let keep_alive = {
                let config = state.config.blocking_read();
                config.keep_alive
            };
            if !keep_alive {
                // Kill ComfyUI process on app exit
                let mut process = state.comfyui_process.blocking_lock();
                if let Some(ref mut child) = *process {
                    log::info!("Shutting down ComfyUI process...");
                    // Use start_kill (non-async) for synchronous shutdown
                    let _ = child.start_kill();
                    *process = None;
                }
                // Also kill anything on the port as a safety net
                let port = state.config.blocking_read().server_port;
                #[cfg(target_os = "linux")]
                {
                    let _ = std::process::Command::new("fuser")
                        .args(["-k", &format!("{}/tcp", port)])
                        .output();
                }
                #[cfg(target_os = "macos")]
                {
                    if let Ok(output) = std::process::Command::new("lsof")
                        .args(["-ti", &format!(":{}", port)])
                        .output()
                    {
                        for pid in String::from_utf8_lossy(&output.stdout).lines() {
                            if pid.trim().parse::<u32>().is_ok() {
                                let _ = std::process::Command::new("kill")
                                    .args(["-9", pid.trim()])
                                    .output();
                            }
                        }
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    #[allow(unused_imports)]
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;

                    if let Ok(output) = std::process::Command::new("cmd")
                        .args([
                            "/C",
                            &format!("netstat -ano | findstr :{} | findstr LISTENING", port),
                        ])
                        .creation_flags(CREATE_NO_WINDOW)
                        .output()
                    {
                        for line in String::from_utf8_lossy(&output.stdout).lines() {
                            if let Some(pid) = line.split_whitespace().last() {
                                if pid.parse::<u32>().is_ok() {
                                    let _ = std::process::Command::new("taskkill")
                                        .args(["/F", "/PID", pid])
                                        .creation_flags(CREATE_NO_WINDOW)
                                        .output();
                                }
                            }
                        }
                    }
                }
            } else {
                log::info!("Keeping ComfyUI running (keep_alive=true)");
            }
        }
    });
}
