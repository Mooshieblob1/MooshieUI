use base64::Engine;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Instant;
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::connect_async;

use crate::error::AppError;
use crate::state::AppState;

/// Result of processing a MOOSHIE_OUTPUT_IMAGE (event_type 100) binary frame.
struct ProcessedOutputImage {
    format: &'static str, // "jxl" or "png"
    ext: &'static str,    // file extension for the canonical image
    bit_depth: u8,
    image_bytes: Vec<u8>,           // encoded JXL or PNG bytes
    display_bytes: Option<Vec<u8>>, // WebP or PNG display copy (for JXL only)
    display_format: &'static str,   // "webp", "png", or "none"
    encode_ms: u64,
}

/// Decode a MOOSHIE_OUTPUT_IMAGE binary frame (event_type 100) and, for raw RGBA
/// payloads (format_tags 3/4), encode to JXL + a WebP/PNG display copy.
/// Shared by the Tauri, headless, and multi-GPU WebSocket handlers.
async fn process_output_image(data: &[u8]) -> Option<ProcessedOutputImage> {
    if data.len() < 8 {
        return None;
    }
    let format_tag = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let started = Instant::now();

    let (out_format, out_ext, bit_depth, image_bytes, display_bytes, display_fmt): (
        &'static str,
        &'static str,
        u8,
        Vec<u8>,
        Option<Vec<u8>>,
        &'static str,
    ) = match format_tag {
        3 | 4 => {
            // Raw RGBA pixels — encode to JXL + display copy
            if data.len() < 16 {
                log::warn!("MooshieSaveImage raw frame too small: len={}", data.len());
                return None;
            }
            let width = u16::from_be_bytes([data[8], data[9]]) as u32;
            let height = u16::from_be_bytes([data[10], data[11]]) as u32;
            let channels = data[12];
            let depth = data[13];
            if channels != 4 || !(depth == 8 || depth == 16) {
                log::warn!(
                    "MooshieSaveImage raw header rejected: ch={} depth={}",
                    channels,
                    depth
                );
                return None;
            }
            let pixels = data[16..].to_vec();
            let w = width;
            let h = height;
            let is_16 = depth == 16;
            let result = tokio::task::spawn_blocking(move || {
                let jxl = if is_16 {
                    crate::jxl::encode_rgba16_lossless(&pixels, w, h)
                } else {
                    crate::jxl::encode_rgba8_lossless(&pixels, w, h)
                };
                let (display, display_fmt): (Option<Vec<u8>>, &'static str) =
                    match crate::jxl::encode_rgba8_webp_from_raw(&pixels, w, h, is_16) {
                        Ok(webp) => (Some(webp), "webp"),
                        Err(e) => {
                            log::warn!("WebP encode failed, falling back to PNG: {}", e);
                            let png = if is_16 {
                                crate::jxl::encode_rgba16_png(&pixels, w, h)
                            } else {
                                crate::jxl::encode_rgba8_png(&pixels, w, h)
                            };
                            match png {
                                Ok(p) => (Some(p), "png"),
                                Err(e2) => {
                                    log::error!("PNG fallback also failed: {}", e2);
                                    (None, "none")
                                }
                            }
                        }
                    };
                jxl.map(|j| (j, display, display_fmt))
            })
            .await;
            let (jxl_bytes, display_opt, disp_fmt) = match result {
                Ok(Ok(triple)) => triple,
                Ok(Err(e)) => {
                    log::error!("JXL encode failed: {}", e);
                    return None;
                }
                Err(e) => {
                    log::error!("JXL encode task panicked: {}", e);
                    return None;
                }
            };
            (
                "jxl",
                "jxl",
                if is_16 { 16 } else { 8 },
                jxl_bytes,
                display_opt,
                disp_fmt,
            )
        }
        2 => ("png", "png", 16, data[8..].to_vec(), None, "png"),
        _ => ("png", "png", 8, data[8..].to_vec(), None, "png"),
    };

    let encode_ms = started.elapsed().as_millis() as u64;

    if bit_depth == 16 && encode_ms > 500 {
        log::warn!(
            "Slow output WS payload processing: format={} encode_ms={} bytes={}",
            out_format,
            encode_ms,
            image_bytes.len(),
        );
    }

    Some(ProcessedOutputImage {
        format: out_format,
        ext: out_ext,
        bit_depth,
        image_bytes,
        display_bytes,
        display_format: display_fmt,
        encode_ms,
    })
}

/// Save the processed output image to temp files and build the SSE event payload.
/// Shared by the headless and multi-GPU WebSocket handlers.
fn build_sse_payload(img: &ProcessedOutputImage, prompt_id: &str) -> serde_json::Value {
    let temp_filename = crate::temp_images::save(&img.image_bytes, img.ext);

    // For JXL: save the pre-computed display copy (WebP/PNG) so the browser
    // can show it directly without server-side transcoding.
    let display_temp_filename: Option<String> = if img.format == "jxl" {
        img.display_bytes.as_ref().and_then(|db| {
            let ext = if img.display_format == "webp" {
                "webp"
            } else {
                "png"
            };
            crate::temp_images::save(db, ext)
        })
    } else {
        None
    };

    log::info!(
        "output_image: format={} temp={:?} display_temp={:?} display_fmt={} bytes={} encode_ms={} prompt_id={}",
        img.format, temp_filename, display_temp_filename, img.display_format,
        img.image_bytes.len(), img.encode_ms, prompt_id,
    );

    if let Some(name) = temp_filename {
        let mut payload = serde_json::json!({
            "temp_filename": name,
            "format": img.format,
            "bit_depth": img.bit_depth,
            "image_bytes": img.image_bytes.len(),
            "encode_ms": img.encode_ms,
            "prompt_id": prompt_id,
        });
        if let Some(ref disp) = display_temp_filename {
            payload["display_temp_filename"] = serde_json::json!(disp);
            payload["display_format"] = serde_json::json!(img.display_format);
        }
        payload
    } else {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&img.image_bytes);
        serde_json::json!({
            "image": b64,
            "format": img.format,
            "bit_depth": img.bit_depth,
            "image_bytes": img.image_bytes.len(),
            "encode_ms": img.encode_ms,
            "prompt_id": prompt_id,
        })
    }
}

#[cfg(feature = "desktop")]
pub async fn connect_websocket(
    app_handle: AppHandle,
    state: Arc<AppState>,
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
    // Clone the Arc so the spawned task owns it (needed for queue cleanup).
    let ws_state = Arc::clone(&state);
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
                                        // Prompt completed — release GPU worker and clean up
                                        // the internal queue. In browser mode this is done by
                                        // the cleanup reactor in webserver.rs; in Tauri desktop
                                        // mode we must do it here so the worker becomes available
                                        // for the next generation.
                                        let resolved =
                                            ws_state.prompt_queue.resolve_alias(prompt_id);
                                        let wid = ws_state.prompt_queue.finish(&resolved);
                                        let alias_state = Arc::clone(&ws_state);
                                        let alias_pid = resolved.clone();
                                        tokio::spawn(async move {
                                            tokio::time::sleep(std::time::Duration::from_secs(5))
                                                .await;
                                            alias_state.prompt_queue.cleanup_alias(&alias_pid);
                                        });
                                        if let Some(worker_id) = wid {
                                            ws_state.gpu_manager.mark_worker_idle(worker_id).await;
                                        }
                                        ws_state.prompt_queue.drain_notify.notify_one();
                                        // Broadcast updated queue positions to the Tauri
                                        // frontend. broadcast_queue_positions() uses event_tx
                                        // (SSE-only); we must call app.emit() directly here.
                                        let updates: Vec<serde_json::Value> = {
                                            let queue = ws_state.prompt_queue.queue.read().unwrap();
                                            let total = queue.len();
                                            queue
                                                .iter()
                                                .enumerate()
                                                .map(|(pos, (pid, _))| {
                                                    serde_json::json!({
                                                        "prompt_id": pid,
                                                        "position": pos,
                                                        "total": total,
                                                    })
                                                })
                                                .collect()
                                        };
                                        if updates.is_empty() {
                                            emit(
                                                "mooshie:queue_update",
                                                serde_json::json!({ "total": 0_u32 }),
                                            );
                                        } else {
                                            for payload in updates {
                                                emit("mooshie:queue_update", payload);
                                            }
                                        }
                                    } else {
                                        current_prompt_id = Some(prompt_id.to_string());
                                    }
                                }
                                "execution_error" => {
                                    // Prompt failed — release GPU worker so the next generation
                                    // can proceed. Without this the worker stays in Running state
                                    // and submit_prompt blocks for 300 s before timing out.
                                    let resolved = ws_state.prompt_queue.resolve_alias(prompt_id);
                                    let wid = ws_state.prompt_queue.finish(&resolved);
                                    let alias_state = Arc::clone(&ws_state);
                                    let alias_pid = resolved.clone();
                                    tokio::spawn(async move {
                                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                        alias_state.prompt_queue.cleanup_alias(&alias_pid);
                                    });
                                    if let Some(worker_id) = wid {
                                        ws_state
                                            .gpu_manager
                                            .mark_worker_error_then_idle(worker_id)
                                            .await;
                                    }
                                    ws_state.prompt_queue.drain_notify.notify_one();
                                    // Signal frontend that queue has been cleared.
                                    emit(
                                        "mooshie:queue_update",
                                        serde_json::json!({ "total": 0_u32 }),
                                    );
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
                            // MOOSHIE_OUTPUT_IMAGE — use shared processing function
                            let prompt_id_str = current_prompt_id.as_deref().unwrap();
                            let img = match process_output_image(&data).await {
                                Some(img) => img,
                                None => continue,
                            };

                            // Save canonical image to temp dir.
                            let temp_filename = crate::temp_images::save(&img.image_bytes, img.ext);

                            // For JXL: save the display copy (WebP/PNG) as a second temp file.
                            let display_temp_filename: Option<String> = if img.format == "jxl" {
                                img.display_bytes.as_ref().and_then(|db| {
                                    let ext = if img.display_format == "webp" {
                                        "webp"
                                    } else {
                                        "png"
                                    };
                                    crate::temp_images::save(db, ext)
                                })
                            } else {
                                None
                            };

                            log::info!(
                                "output_image: format={} jxl_temp={:?} display_temp={:?} display_fmt={} bytes={} display_bytes={}",
                                img.format, temp_filename, display_temp_filename, img.display_format,
                                img.image_bytes.len(),
                                img.display_bytes.as_ref().map(|d| d.len()).unwrap_or(0),
                            );

                            // Tauri desktop: reference temp files only (no inline base64).
                            // app.emit() silently drops events exceeding ~1-2 MB.
                            let tauri_payload = if img.format == "jxl" {
                                match (temp_filename.as_ref(), display_temp_filename.as_ref()) {
                                    (Some(jxl_f), Some(disp_f)) => serde_json::json!({
                                        "temp_filename": jxl_f,
                                        "display_temp_filename": disp_f,
                                        "format": "jxl",
                                        "display_format": img.display_format,
                                        "bit_depth": img.bit_depth,
                                        "image_bytes": img.image_bytes.len(),
                                        "encode_ms": img.encode_ms,
                                        "prompt_id": prompt_id_str,
                                    }),
                                    (Some(jxl_f), None) => serde_json::json!({
                                        "temp_filename": jxl_f,
                                        "format": "jxl",
                                        "bit_depth": img.bit_depth,
                                        "image_bytes": img.image_bytes.len(),
                                        "encode_ms": img.encode_ms,
                                        "prompt_id": prompt_id_str,
                                    }),
                                    _ => {
                                        let b64 = base64::engine::general_purpose::STANDARD
                                            .encode(&img.image_bytes);
                                        serde_json::json!({
                                            "jxl_image": b64,
                                            "format": "jxl",
                                            "bit_depth": img.bit_depth,
                                            "image_bytes": img.image_bytes.len(),
                                            "encode_ms": img.encode_ms,
                                            "prompt_id": prompt_id_str,
                                        })
                                    }
                                }
                            } else if let Some(ref tf) = temp_filename {
                                serde_json::json!({
                                    "temp_filename": tf,
                                    "format": img.format,
                                    "bit_depth": img.bit_depth,
                                    "image_bytes": img.image_bytes.len(),
                                    "encode_ms": img.encode_ms,
                                    "prompt_id": prompt_id_str,
                                })
                            } else {
                                let b64 = base64::engine::general_purpose::STANDARD
                                    .encode(&img.image_bytes);
                                serde_json::json!({
                                    "image": b64,
                                    "format": img.format,
                                    "bit_depth": img.bit_depth,
                                    "image_bytes": img.image_bytes.len(),
                                    "encode_ms": img.encode_ms,
                                    "prompt_id": prompt_id_str,
                                })
                            };

                            // SSE payload: always use temp filename
                            let sse_payload = if let Some(name) = temp_filename {
                                serde_json::json!({
                                    "temp_filename": name,
                                    "format": img.format,
                                    "bit_depth": img.bit_depth,
                                    "image_bytes": img.image_bytes.len(),
                                    "encode_ms": img.encode_ms,
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
                            let prompt_id_str = current_prompt_id.as_deref().unwrap();
                            let img = match process_output_image(&data).await {
                                Some(img) => img,
                                None => continue,
                            };
                            let payload = build_sse_payload(&img, prompt_id_str);
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

/// Connect a WebSocket to a specific GPU worker's ComfyUI instance.
/// Events are broadcast to the shared event_tx channel.
/// The task handle is stored in the worker so it can be aborted on shutdown.
pub async fn connect_websocket_for_worker(
    state: &AppState,
    worker: &std::sync::Arc<super::gpu_manager::GpuWorker>,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
) -> Result<(), AppError> {
    // Disconnect existing WS for this worker
    {
        let mut handle = worker.ws_handle.lock().await;
        if let Some(h) = handle.take() {
            h.abort();
        }
    }

    let ws_url = worker
        .base_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws_url = format!("{}/ws?clientId={}", ws_url, state.client_id);

    let worker_id = worker.id;
    let tx = event_tx;

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
                log::error!("Worker {} WebSocket connection failed: {}", worker_id, e);
                emit(
                    "comfyui:connection",
                    serde_json::json!({"connected": false, "worker_id": worker_id}),
                );
                return;
            }
        };

        log::info!("Worker {} WebSocket connected", worker_id);
        emit(
            "comfyui:connection",
            serde_json::json!({"connected": true, "worker_id": worker_id}),
        );

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
                            let prompt_id_str = current_prompt_id.as_deref().unwrap();
                            let img = match process_output_image(&data).await {
                                Some(img) => img,
                                None => continue,
                            };
                            let payload = build_sse_payload(&img, prompt_id_str);
                            emit("comfyui:output_image", payload);
                        }
                        _ => {}
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                    log::warn!("Worker {} WebSocket closed", worker_id);
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false, "worker_id": worker_id}),
                    );
                    break;
                }
                Err(e) => {
                    log::error!("Worker {} WebSocket error: {}", worker_id, e);
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false, "worker_id": worker_id}),
                    );
                    break;
                }
                _ => {}
            }
        }
    });

    *worker.ws_handle.lock().await = Some(task);
    Ok(())
}
