pub mod commands;
pub mod comfyui;
pub mod config;
pub mod error;
pub mod setup;
pub mod state;
pub mod templates;

use config::load_persisted_config;
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = load_persisted_config();
    let app_state = AppState::new(config);

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(app_state)
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
            commands::api::get_output_image,
            commands::api::get_client_id,
            commands::api::download_model,
            commands::api::save_image_file,
            commands::api::save_to_gallery,
            commands::api::list_gallery_images,
            commands::api::load_gallery_image,
            commands::api::delete_gallery_image,
            commands::api::copy_image_to_clipboard,
            commands::websocket::connect_ws,
            commands::websocket::disconnect_ws,
            commands::workflow::generate,
            setup::check_setup,
            setup::detect_gpu,
            setup::run_setup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
