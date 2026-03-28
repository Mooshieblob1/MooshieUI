pub mod commands;
pub mod comfyui;
pub mod config;
pub mod error;
pub mod interrogator;
pub mod metadata;
pub mod setup;
pub mod state;
pub mod templates;

use config::load_persisted_config;
use state::AppState;
use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Fix WebKitGTK scroll jank and rendering glitches on NVIDIA + Wayland.
    // The DMA-BUF renderer is broken with NVIDIA proprietary drivers.
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    let config = load_persisted_config();
    let app_state = AppState::new(config);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .setup(|app| {
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                if let Some(main_window) = app.get_webview_window("main") {
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

                let gallery_dir = match config::app_data_dir() {
                    Some(d) => d.join("gallery"),
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
            commands::api::save_image_file,
            commands::api::save_to_gallery,
            commands::api::save_to_gallery_bytes,
            commands::api::list_gallery_images,
            commands::api::list_gallery_image_entries,
            commands::api::load_gallery_image,
            commands::api::get_gallery_image_path,
            commands::api::delete_gallery_image,
            commands::api::rename_gallery_image,
            commands::api::copy_image_to_clipboard,
            commands::api::find_model_by_hash,
            commands::api::hash_model_file,
            commands::api::civitai_lookup_hash,
            commands::api::civitai_search_models,
            commands::api::civitai_list_architectures,
            commands::api::read_modelspec,
            commands::api::get_lora_civitai_info,
            commands::api::read_image_metadata,
            commands::api::read_image_metadata_bytes,
            commands::api::read_image_metadata_path,
            commands::api::fetch_release_notes,
            commands::api::import_image_directory,
            commands::api::export_logs,
            commands::api::check_node_available,
            commands::api::is_custom_node_installed,
            commands::api::install_custom_node,
            commands::api::install_pip_package,
            commands::websocket::connect_ws,
            commands::websocket::disconnect_ws,
            commands::workflow::generate,
            commands::config::get_config,
            commands::config::update_config,
            commands::interrogator::interrogate_image,
            commands::interrogator::interrogate_image_path,
            commands::interrogator::interrogate_gallery_image,
            commands::interrogator::interrogate_clipboard,
            commands::api::read_clipboard_image,
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
            let state = app_handle.state::<AppState>();
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
                        .args(["/C", &format!("netstat -ano | findstr :{} | findstr LISTENING", port)])
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
