use base64::Engine;
use futures_util::StreamExt;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::connect_async;

use crate::error::AppError;
use crate::state::AppState;

pub async fn connect_websocket(
    app_handle: AppHandle,
    state: &AppState,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
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
    let tx = event_tx.clone();
    let task = tokio::spawn(async move {
        // Helper to emit to both Tauri and SSE broadcast
        let emit = |event: &str, payload: serde_json::Value| {
            let _ = app.emit(event, payload.clone());
            let _ = tx.send(crate::state::BroadcastEvent {
                event: event.to_string(),
                payload,
            });
        };
        // Split emit: send full payload to Tauri (in-process), lightweight to SSE
        let emit_split =
            |event: &str, tauri_payload: serde_json::Value, sse_payload: serde_json::Value| {
                let _ = app.emit(event, tauri_payload);
                let _ = tx.send(crate::state::BroadcastEvent {
                    event: event.to_string(),
                    payload: sse_payload,
                });
            };
        let result = connect_async(&ws_url).await;
        let (ws_stream, _) = match result {
            Ok(s) => s,
            Err(e) => {
                log::error!("WebSocket connection failed: {}", e);
                emit(
                    "comfyui:connection",
                    serde_json::json!({"connected": false}),
                );
                return;
            }
        };

        emit("comfyui:connection", serde_json::json!({"connected": true}));

        let (_, mut read) = ws_stream.split();
        let mut current_prompt_id: Option<String> = None;

        while let Some(msg) = read.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                        let event_type = parsed["type"].as_str().unwrap_or("unknown");
                        let data = &parsed["data"];

                        if let Some(prompt_id) = data["prompt_id"].as_str() {
                            match event_type {
                                "execution_start" => {
                                    current_prompt_id = Some(prompt_id.to_string());
                                }
                                "executing" => {
                                    if data["node"].is_null() {
                                        if current_prompt_id.as_deref() == Some(prompt_id) {
                                            current_prompt_id = None;
                                        }
                                    } else {
                                        current_prompt_id = Some(prompt_id.to_string());
                                    }
                                }
                                _ => {}
                            }
                        }

                        let event_name = format!("comfyui:{}", event_type);
                        emit(&event_name, data.clone());
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Binary(data)) => {
                    if data.len() < 4 {
                        continue;
                    }
                    let event_type = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);

                    // Skip binary events if we don't know which prompt they belong to
                    // (prevents cross-user event leaking via SSE)
                    if current_prompt_id.is_none() && matches!(event_type, 1 | 2 | 4 | 100) {
                        continue;
                    }

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
                            let ext = if format_type == 2 { "png" } else { "jpg" };
                            let image_data = &data[8..];
                            let prompt_id_str = current_prompt_id.as_deref().unwrap();

                            // Tauri: inline base64 (fast, in-process)
                            let b64 = base64::engine::general_purpose::STANDARD.encode(image_data);
                            let tauri_payload = serde_json::json!({ "image": b64, "format": format, "prompt_id": prompt_id_str });

                            // SSE: save to temp file, send reference
                            let sse_payload = if let Some(temp_filename) =
                                crate::temp_images::save(image_data, ext)
                            {
                                serde_json::json!({ "temp_filename": temp_filename, "format": format, "prompt_id": prompt_id_str })
                            } else {
                                tauri_payload.clone() // fallback to inline
                            };

                            emit_split("comfyui:preview", tauri_payload, sse_payload);
                        }
                        4 => {
                            // PREVIEW_IMAGE_WITH_METADATA
                            if data.len() < 8 {
                                continue;
                            }
                            let meta_len =
                                u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
                            let image_start = 8 + meta_len;
                            if image_start < data.len() {
                                let image_data = &data[image_start..];
                                let prompt_id_str = current_prompt_id.as_deref().unwrap();

                                let b64 =
                                    base64::engine::general_purpose::STANDARD.encode(image_data);
                                let tauri_payload = serde_json::json!({ "image": b64, "format": "jpeg", "prompt_id": prompt_id_str });

                                let sse_payload = if let Some(temp_filename) =
                                    crate::temp_images::save(image_data, "jpg")
                                {
                                    serde_json::json!({ "temp_filename": temp_filename, "format": "jpeg", "prompt_id": prompt_id_str })
                                } else {
                                    tauri_payload.clone()
                                };

                                emit_split("comfyui:preview", tauri_payload, sse_payload);
                            }
                        }
                        100 => {
                            // MOOSHIE_OUTPUT_IMAGE — full-res PNG from MooshieSaveImage
                            // Layout: event_type(4) + format_tag(4) + image_data
                            //   format_tag: 1 = 8-bit PNG, 2 = 16-bit PNG
                            if data.len() < 8 {
                                continue;
                            }
                            let format_tag =
                                u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                            let started = Instant::now();
                            let bit_depth = if format_tag == 2 { 16 } else { 8 };
                            let image_data = &data[8..];
                            let b64 = base64::engine::general_purpose::STANDARD.encode(image_data);
                            let encode_ms = started.elapsed().as_millis() as u64;
                            let prompt_id_str = current_prompt_id.as_deref().unwrap();

                            if bit_depth == 16 && encode_ms > 250 {
                                log::warn!(
                                    "Slow 16-bit output WS payload processing: encode_ms={} bytes={} prompt_id={}",
                                    encode_ms,
                                    image_data.len(),
                                    prompt_id_str,
                                );
                            }

                            // Tauri: inline base64 (in-process, reliable)
                            let tauri_payload = serde_json::json!({
                                "image": b64,
                                "bit_depth": bit_depth,
                                "image_bytes": image_data.len(),
                                "encode_ms": encode_ms,
                                "prompt_id": prompt_id_str,
                            });

                            // SSE: save to temp file (avoids multi-MB SSE payload)
                            let sse_payload = if let Some(temp_filename) =
                                crate::temp_images::save(image_data, "png")
                            {
                                serde_json::json!({
                                    "temp_filename": temp_filename,
                                    "bit_depth": bit_depth,
                                    "image_bytes": image_data.len(),
                                    "encode_ms": encode_ms,
                                    "prompt_id": prompt_id_str,
                                })
                            } else {
                                tauri_payload.clone()
                            };

                            emit_split("comfyui:output_image", tauri_payload, sse_payload);
                        }
                        _ => {}
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false}),
                    );
                    break;
                }
                Err(e) => {
                    log::error!("WebSocket error: {}", e);
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false}),
                    );
                    break;
                }
                _ => {}
            }
        }
    });

    *handle = Some(task);
    Ok(())
}

/// Connect the WebSocket to ComfyUI without requiring an AppHandle.
/// Events are only sent to the broadcast channel (SSE clients).
/// Handles prompt queue cleanup on completion/error for multi-user isolation.
pub async fn connect_websocket_headless(
    state: &AppState,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
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

    let tx = event_tx.clone();

    let task = tokio::spawn(async move {
        let emit = |event: &str, payload: serde_json::Value| {
            let _ = tx.send(crate::state::BroadcastEvent {
                event: event.to_string(),
                payload,
            });
        };
        let result = connect_async(&ws_url).await;
        let (ws_stream, _) = match result {
            Ok(s) => s,
            Err(e) => {
                log::error!("WebSocket connection failed (headless): {}", e);
                emit(
                    "comfyui:connection",
                    serde_json::json!({"connected": false}),
                );
                return;
            }
        };

        emit("comfyui:connection", serde_json::json!({"connected": true}));

        let (_, mut read) = ws_stream.split();
        let mut current_prompt_id: Option<String> = None;

        while let Some(msg) = read.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                        let event_type = parsed["type"].as_str().unwrap_or("unknown");
                        let data = &parsed["data"];

                        if let Some(prompt_id) = data["prompt_id"].as_str() {
                            match event_type {
                                "execution_start" => {
                                    current_prompt_id = Some(prompt_id.to_string());
                                }
                                "executing" => {
                                    if data["node"].is_null() {
                                        if current_prompt_id.as_deref() == Some(prompt_id) {
                                            current_prompt_id = None;
                                        }
                                    } else {
                                        current_prompt_id = Some(prompt_id.to_string());
                                    }
                                }
                                _ => {}
                            }
                        }

                        let event_name = format!("comfyui:{}", event_type);
                        emit(&event_name, data.clone());
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Binary(data)) => {
                    if data.len() < 4 {
                        continue;
                    }
                    let event_type = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    // Skip binary events if we don't know which prompt they belong to
                    if current_prompt_id.is_none() && matches!(event_type, 1 | 2 | 4 | 100) {
                        continue;
                    }
                    match event_type {
                        1 | 2 => {
                            if data.len() < 8 {
                                continue;
                            }
                            let format_type =
                                u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                            let format = if format_type == 2 { "png" } else { "jpeg" };
                            let ext = if format_type == 2 { "png" } else { "jpg" };
                            let image_data = &data[8..];
                            let prompt_id_str = current_prompt_id.as_deref().unwrap();

                            // Headless: always save to temp file (SSE-only path)
                            let payload = if let Some(temp_filename) =
                                crate::temp_images::save(image_data, ext)
                            {
                                serde_json::json!({ "temp_filename": temp_filename, "format": format, "prompt_id": prompt_id_str })
                            } else {
                                let b64 =
                                    base64::engine::general_purpose::STANDARD.encode(image_data);
                                serde_json::json!({ "image": b64, "format": format, "prompt_id": prompt_id_str })
                            };
                            emit("comfyui:preview", payload);
                        }
                        4 => {
                            if data.len() < 8 {
                                continue;
                            }
                            let meta_len =
                                u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
                            let image_start = 8 + meta_len;
                            if image_start < data.len() {
                                let image_data = &data[image_start..];
                                let prompt_id_str = current_prompt_id.as_deref().unwrap();

                                let payload = if let Some(temp_filename) =
                                    crate::temp_images::save(image_data, "jpg")
                                {
                                    serde_json::json!({ "temp_filename": temp_filename, "format": "jpeg", "prompt_id": prompt_id_str })
                                } else {
                                    let b64 = base64::engine::general_purpose::STANDARD
                                        .encode(image_data);
                                    serde_json::json!({ "image": b64, "format": "jpeg", "prompt_id": prompt_id_str })
                                };
                                emit("comfyui:preview", payload);
                            }
                        }
                        100 => {
                            if data.len() < 8 {
                                continue;
                            }
                            let format_tag =
                                u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                            let started = Instant::now();
                            let bit_depth = if format_tag == 2 { 16 } else { 8 };
                            let image_data = &data[8..];
                            let encode_ms = started.elapsed().as_millis() as u64;
                            let prompt_id_str = current_prompt_id.as_deref().unwrap();

                            if bit_depth == 16 && encode_ms > 250 {
                                log::warn!(
                                    "Slow 16-bit output WS payload processing (headless): encode_ms={} bytes={} prompt_id={}",
                                    encode_ms,
                                    image_data.len(),
                                    prompt_id_str,
                                );
                            }

                            // Headless: always save to temp file (SSE-only path)
                            let payload = if let Some(temp_filename) =
                                crate::temp_images::save(image_data, "png")
                            {
                                serde_json::json!({
                                    "temp_filename": temp_filename,
                                    "bit_depth": bit_depth,
                                    "image_bytes": image_data.len(),
                                    "encode_ms": encode_ms,
                                    "prompt_id": prompt_id_str,
                                })
                            } else {
                                let b64 =
                                    base64::engine::general_purpose::STANDARD.encode(image_data);
                                serde_json::json!({
                                    "image": b64,
                                    "bit_depth": bit_depth,
                                    "image_bytes": image_data.len(),
                                    "encode_ms": encode_ms,
                                    "prompt_id": prompt_id_str,
                                })
                            };
                            emit("comfyui:output_image", payload);
                        }
                        _ => {}
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false}),
                    );
                    break;
                }
                Err(e) => {
                    log::error!("WebSocket error (headless): {}", e);
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false}),
                    );
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
