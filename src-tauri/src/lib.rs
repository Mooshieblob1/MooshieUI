pub mod commands;
pub mod comfyui;
pub mod config;
pub mod error;
pub mod state;
pub mod templates;

use config::AppConfig;
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = AppConfig::default();
    let app_state = AppState::new(config);

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
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
            commands::websocket::connect_ws,
            commands::websocket::disconnect_ws,
            commands::workflow::generate,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
