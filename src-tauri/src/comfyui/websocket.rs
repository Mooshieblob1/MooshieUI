use base64::Engine;
use futures_util::StreamExt;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::connect_async;

use crate::error::AppError;
use crate::state::AppState;

pub async fn connect_websocket(
    app_handle: AppHandle,
    state: &AppState,
) -> Result<(), AppError> {
    // Disconnect existing
    let mut handle = state.ws_handle.lock().await;
    if let Some(h) = handle.take() {
        h.abort();
    }

    let base_url = state.base_url().await;
    let client_id = state.client_id.clone();
    let ws_url = base_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws_url = format!("{}/ws?clientId={}", ws_url, client_id);

    let app = app_handle.clone();
    let task = tokio::spawn(async move {
        let result = connect_async(&ws_url).await;
        let (ws_stream, _) = match result {
            Ok(s) => s,
            Err(e) => {
                log::error!("WebSocket connection failed: {}", e);
                let _ = app.emit("comfyui:connection", serde_json::json!({"connected": false}));
                return;
            }
        };

        let _ = app.emit("comfyui:connection", serde_json::json!({"connected": true}));

        let (_, mut read) = ws_stream.split();

        while let Some(msg) = read.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                        let event_type = parsed["type"].as_str().unwrap_or("unknown");
                        let data = &parsed["data"];
                        let event_name = format!("comfyui:{}", event_type);
                        let _ = app.emit(&event_name, data.clone());
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Binary(data)) => {
                    if data.len() < 4 {
                        continue;
                    }
                    let event_type =
                        u32::from_be_bytes([data[0], data[1], data[2], data[3]]);

                    match event_type {
                        1 | 2 => {
                            // PREVIEW_IMAGE or UNENCODED_PREVIEW_IMAGE
                            // Bytes 4-7: image format (1=JPEG, 2=PNG)
                            // Bytes 8+: image data
                            if data.len() < 8 {
                                continue;
                            }
                            let format_type =
                                u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                            let format = if format_type == 2 { "png" } else { "jpeg" };
                            let image_data = &data[8..];
                            let b64 = base64::engine::general_purpose::STANDARD
                                .encode(image_data);
                            let _ = app.emit(
                                "comfyui:preview",
                                serde_json::json!({ "image": b64, "format": format }),
                            );
                        }
                        4 => {
                            // PREVIEW_IMAGE_WITH_METADATA
                            if data.len() < 8 {
                                continue;
                            }
                            let meta_len = u32::from_be_bytes([
                                data[4], data[5], data[6], data[7],
                            ]) as usize;
                            let image_start = 8 + meta_len;
                            if image_start < data.len() {
                                let image_data = &data[image_start..];
                                let b64 = base64::engine::general_purpose::STANDARD
                                    .encode(image_data);
                                let _ = app.emit(
                                    "comfyui:preview",
                                    serde_json::json!({ "image": b64, "format": "jpeg" }),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                    let _ = app
                        .emit("comfyui:connection", serde_json::json!({"connected": false}));
                    break;
                }
                Err(e) => {
                    log::error!("WebSocket error: {}", e);
                    let _ = app
                        .emit("comfyui:connection", serde_json::json!({"connected": false}));
                    break;
                }
                _ => {}
            }
        }
    });

    *handle = Some(task);
    Ok(())
}

pub async fn disconnect_websocket(state: &AppState) -> Result<(), AppError> {
    let mut handle = state.ws_handle.lock().await;
    if let Some(h) = handle.take() {
        h.abort();
    }
    Ok(())
}
