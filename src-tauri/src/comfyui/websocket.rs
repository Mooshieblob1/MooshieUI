use base64::Engine;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Instant;
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::error::AppError;
use crate::state::AppState;

/// WebSocket config for ComfyUI connections.
///
/// `MooshieSaveImage` streams each finished image back as a single unfragmented
/// binary frame of raw RGBA pixels. A 4x-upscaled 16-bit result (e.g. face
/// detail + tiled upscale) is `w * h * 4ch * 2bytes`, which for large canvases
/// exceeds tungstenite's default 16 MiB `max_frame_size`. When that happens the
/// frame is rejected with a capacity error, the socket resets, and the output
/// image is lost — the UI is left stuck on the last preview frame. These frames
/// come from our own localhost/LAN ComfyUI, so we lift both limits to a generous
/// 1 GiB (covers up to ~8K 16-bit RGBA with headroom) while still bounding
/// allocation against a runaway length header.
fn comfyui_ws_config() -> WebSocketConfig {
    const LIMIT: usize = 1 << 30; // 1 GiB
    WebSocketConfig::default()
        .max_message_size(Some(LIMIT))
        .max_frame_size(Some(LIMIT))
}

/// Result of processing a MOOSHIE_OUTPUT_IMAGE (event_type 100) binary frame.
struct ProcessedOutputImage {
    format: &'static str, // "jxl", "webp", or "png"
    ext: &'static str,    // file extension for the canonical image
    bit_depth: u8,
    image_bytes: Vec<u8>,           // encoded JXL, WebP, or PNG bytes
    display_bytes: Option<Vec<u8>>, // WebP or PNG display copy (for JXL only)
    display_format: &'static str,   // "webp", "png", or "none"
    encode_ms: u64,
}

/// Decode a MOOSHIE_OUTPUT_IMAGE binary frame (event_type 100) and, for raw RGBA
/// payloads (format_tags 3/4/5), encode to JXL + a WebP/PNG display copy, or to
/// lossless WebP directly (tag 5).
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
                    crate::jxl::encode_rgba16_visually_lossless(&pixels, w, h)
                } else {
                    crate::jxl::encode_rgba8_visually_lossless(&pixels, w, h)
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
        5 => {
            // Raw 8-bit RGBA pixels destined for lossless WebP. The canonical
            // image is itself browser-displayable, so no separate display copy
            // is produced (unlike JXL, which browsers cannot render).
            if data.len() < 16 {
                log::warn!("MooshieSaveImage raw frame too small: len={}", data.len());
                return None;
            }
            let width = u16::from_be_bytes([data[8], data[9]]) as u32;
            let height = u16::from_be_bytes([data[10], data[11]]) as u32;
            let channels = data[12];
            let depth = data[13];
            // WebP has no 16-bit sample format, so the Python node always packs
            // 8-bit for this tag; anything else means a protocol mismatch.
            if channels != 4 || depth != 8 {
                log::warn!(
                    "MooshieSaveImage webp header rejected: ch={} depth={}",
                    channels,
                    depth
                );
                return None;
            }
            let pixels = data[16..].to_vec();
            let result = tokio::task::spawn_blocking(move || {
                crate::jxl::encode_rgba8_webp_from_raw(&pixels, width, height, false)
            })
            .await;
            let webp_bytes = match result {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(e)) => {
                    log::error!("WebP encode failed: {}", e);
                    return None;
                }
                Err(e) => {
                    log::error!("WebP encode task panicked: {}", e);
                    return None;
                }
            };
            ("webp", "webp", 8, webp_bytes, None, "none")
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

fn cache_temp_event(
    state: &Arc<AppState>,
    event: &str,
    prompt_id: &str,
    payload: &serde_json::Value,
) {
    let Some(temp_filename) = payload.get("temp_filename").and_then(|v| v.as_str()) else {
        return;
    };
    let ids = state.prompt_queue.related_ids(prompt_id);
    match event {
        "comfyui:preview" => {
            let mut previews = state.last_preview_by_prompt.write().unwrap();
            for id in ids {
                previews.insert(id, temp_filename.to_string());
            }
        }
        "comfyui:output_image" => {
            let mut outputs = state.output_image_cache.write().unwrap();
            for id in ids {
                let entry = outputs.entry(id).or_default();
                if !entry.iter().any(|existing| existing == temp_filename) {
                    entry.push(temp_filename.to_string());
                }
            }
        }
        _ => {}
    }
}

/// Handle a `MooshieSaveVideo` completion (binary WS event 102).
///
/// The payload after the 4-byte big-endian event id is UTF-8 JSON:
/// `{"video_path","poster_path","fps","frame_count","width","height"}` with
/// absolute paths inside ComfyUI's output directory. Moves the mp4 and its
/// poster into the owning user's gallery directory, indexes them, and returns
/// the payload to emit as `comfyui:output_video`. Any failure logs and
/// returns None -- video output must never crash the WS loop.
async fn handle_video_output(
    state: &std::sync::Arc<crate::state::AppState>,
    data: &[u8],
    prompt_id: &str,
) -> Option<serde_json::Value> {
    let payload: serde_json::Value = match serde_json::from_slice(data.get(4..)?) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[video] event 102 carried invalid JSON: {e}");
            return None;
        }
    };
    let video_path = std::path::PathBuf::from(payload.get("video_path")?.as_str()?);
    let poster_path = payload
        .get("poster_path")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    let fps = payload.get("fps").and_then(|v| v.as_f64()).unwrap_or(24.0);
    let frame_count = payload
        .get("frame_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let width = payload.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let height = payload.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    // bind_alias copies ownership onto the ComfyUI-side prompt id, so the
    // WS-side id resolves the owner directly. Desktop generations have no
    // owner and land in the root gallery.
    let owner = state.prompt_queue.owner_of(prompt_id);
    let Some(gallery_dir) = crate::webserver::user_gallery_dir(owner.as_deref()) else {
        log::warn!("[video] gallery unavailable, dropping video output for prompt {prompt_id}");
        return None;
    };

    let prompt_id_owned = prompt_id.to_string();
    let saved = tokio::task::spawn_blocking(move || {
        crate::commands::api::save_video_to_gallery(
            &video_path,
            poster_path.as_deref(),
            &gallery_dir,
            &prompt_id_owned,
            fps,
            frame_count,
            width,
            height,
        )
    })
    .await;
    let saved = match saved {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            log::warn!("[video] failed to move video into gallery: {e}");
            return None;
        }
        Err(e) => {
            log::warn!("[video] gallery move task failed: {e}");
            return None;
        }
    };

    let duration_seconds = if fps > 0.0 {
        frame_count as f64 / fps
    } else {
        0.0
    };
    Some(serde_json::json!({
        "type": "video",
        "prompt_id": prompt_id,
        "video_filename": saved.video_filename,
        "poster_filename": saved.poster_filename,
        "duration_seconds": duration_seconds,
        "fps": fps,
        "frame_count": frame_count,
        "width": width,
        "height": height,
    }))
}

#[cfg(feature = "desktop")]
pub async fn connect_websocket(
    app_handle: AppHandle,
    state: Arc<AppState>,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
) -> Result<(), AppError> {
    {
        let mut handle = state.ws_handle.lock().await;
        if handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false) {
            log::debug!("ComfyUI WebSocket already connected; skipping reconnect");
            return Ok(());
        }
        if let Some(h) = handle.take() {
            h.abort();
        }
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
                if let Some(prompt_id) = sse_payload.get("prompt_id").and_then(|v| v.as_str()) {
                    cache_temp_event(&ws_state, event, prompt_id, &sse_payload);
                }
                let _ = app.emit(event, tauri_payload);
                let _ = tx.send(crate::state::BroadcastEvent {
                    event: event.to_string(),
                    payload: sse_payload,
                });
            };
        // Persist across reconnects so we can detect prompts that completed
        // while the WebSocket was down and emit a synthetic completion event.
        let mut current_prompt_id: Option<String> = None;
        let mut backoff_ms: u64 = 0;

        'reconnect: loop {
            if backoff_ms > 0 {
                log::info!("WebSocket reconnecting in {} ms", backoff_ms);
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;

                // If a prompt was mid-execution when we disconnected, query
                // /history/{prompt_id} — if it landed there, it completed
                // during the gap and we lost the terminal `executing { node: null }`
                // event. Emit a synthetic one so the frontend can finalize.
                if let Some(pid) = current_prompt_id.clone() {
                    match ws_state.get_history_for(&pid).await {
                        Ok(history) => {
                            let completed =
                                history.get(&pid).map(|v| !v.is_null()).unwrap_or(false);
                            if completed {
                                log::warn!(
                                    "Prompt {} completed during WS disconnect — emitting synthetic completion",
                                    pid
                                );
                                emit(
                                    "comfyui:executing",
                                    serde_json::json!({"node": null, "prompt_id": pid}),
                                );
                                current_prompt_id = None;
                            }
                        }
                        Err(e) => log::warn!("History query for {} failed: {}", pid, e),
                    }
                }
            }

            let result = connect_async_with_config(&ws_url, Some(comfyui_ws_config()), false).await;
            let (ws_stream, _) = match result {
                Ok(s) => {
                    backoff_ms = 0;
                    s
                }
                Err(e) => {
                    log::error!("WebSocket connection failed: {}", e);
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false}),
                    );
                    backoff_ms = (backoff_ms.max(500) * 2).min(30_000);
                    continue 'reconnect;
                }
            };

            emit("comfyui:connection", serde_json::json!({"connected": true}));

            let (_, mut read) = ws_stream.split();

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
                                                tokio::time::sleep(std::time::Duration::from_secs(
                                                    5,
                                                ))
                                                .await;
                                                alias_state.prompt_queue.cleanup_alias(&alias_pid);
                                            });
                                            if let Some(worker_id) = wid {
                                                ws_state
                                                    .gpu_manager
                                                    .mark_worker_idle(worker_id)
                                                    .await;
                                            }
                                            ws_state.prompt_queue.drain_notify.notify_one();
                                            // Broadcast updated queue positions to the Tauri
                                            // frontend. broadcast_queue_positions() uses event_tx
                                            // (SSE-only); we must call app.emit() directly here.
                                            let updates: Vec<serde_json::Value> = {
                                                let queue =
                                                    ws_state.prompt_queue.queue.read().unwrap();
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
                                            ws_state
                                                .gpu_manager
                                                .mark_worker_error_then_idle(worker_id)
                                                .await;
                                        }
                                        ws_state.prompt_queue.drain_notify.notify_one();
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
                        if current_prompt_id.is_none()
                            && matches!(event_type, 1 | 2 | 4 | 100 | 101 | 102)
                        {
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
                                let b64 =
                                    base64::engine::general_purpose::STANDARD.encode(image_data);
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
                                    u32::from_be_bytes([data[4], data[5], data[6], data[7]])
                                        as usize;
                                let image_start = 8 + meta_len;
                                if image_start < data.len() {
                                    let image_data = &data[image_start..];
                                    let prompt_id_str = current_prompt_id.as_deref().unwrap();

                                    let b64 = base64::engine::general_purpose::STANDARD
                                        .encode(image_data);
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
                            100 | 101 => {
                                // MOOSHIE_OUTPUT_IMAGE — use shared processing function
                                let frontend_event = if event_type == 101 {
                                    "comfyui:controlnet_preprocessor"
                                } else {
                                    "comfyui:output_image"
                                };
                                let prompt_id_str = current_prompt_id.as_deref().unwrap();
                                let img = match process_output_image(&data).await {
                                    Some(img) => img,
                                    None => continue,
                                };

                                // Save canonical image to temp dir.
                                let temp_filename =
                                    crate::temp_images::save(&img.image_bytes, img.ext);

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

                                // SSE payload: always use temp filenames, including the
                                // browser-display copy for JXL.
                                let sse_payload = if let Some(name) = temp_filename {
                                    let mut payload = serde_json::json!({
                                        "temp_filename": name,
                                        "format": img.format,
                                        "bit_depth": img.bit_depth,
                                        "image_bytes": img.image_bytes.len(),
                                        "encode_ms": img.encode_ms,
                                        "prompt_id": prompt_id_str,
                                    });
                                    if img.format == "jxl" {
                                        if let Some(ref display_name) = display_temp_filename {
                                            payload["display_temp_filename"] =
                                                serde_json::json!(display_name);
                                            payload["display_format"] =
                                                serde_json::json!(img.display_format);
                                        }
                                    }
                                    payload
                                } else {
                                    tauri_payload.clone()
                                };

                                emit_split(frontend_event, tauri_payload, sse_payload);
                            }
                            102 => {
                                let prompt_id_str = current_prompt_id.as_deref().unwrap();
                                if let Some(payload) =
                                    handle_video_output(&ws_state, &data, prompt_id_str).await
                                {
                                    emit("comfyui:output_video", payload);
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                        log::warn!("WebSocket closed by server — will reconnect");
                        break;
                    }
                    Err(e) => {
                        log::error!("WebSocket error: {} — will reconnect", e);
                        break;
                    }
                    _ => {}
                }
            }

            emit(
                "comfyui:connection",
                serde_json::json!({"connected": false}),
            );
            backoff_ms = (backoff_ms.max(500) * 2).min(30_000);
        }
    });

    *state.ws_handle.lock().await = Some(task);
    Ok(())
}

/// Connect the WebSocket to ComfyUI without requiring an AppHandle.
/// Events are only sent to the broadcast channel (SSE clients).
/// Handles prompt queue cleanup on completion/error for multi-user isolation.
pub async fn connect_websocket_headless(
    state: &Arc<AppState>,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
) -> Result<(), AppError> {
    {
        let mut handle = state.ws_handle.lock().await;
        if handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false) {
            log::debug!("ComfyUI WebSocket (headless) already connected; skipping reconnect");
            return Ok(());
        }
        if let Some(h) = handle.take() {
            h.abort();
        }
    }

    let base_url = state.base_url().await;
    let client_id = state.client_id.clone();
    let ws_url = base_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws_url = format!("{}/ws?clientId={}", ws_url, client_id);

    let tx = event_tx.clone();
    let ws_state = Arc::clone(state);

    let task = tokio::spawn(async move {
        let emit = |event: &str, payload: serde_json::Value| {
            if let Some(prompt_id) = payload.get("prompt_id").and_then(|v| v.as_str()) {
                cache_temp_event(&ws_state, event, prompt_id, &payload);
            }
            let _ = tx.send(crate::state::BroadcastEvent {
                event: event.to_string(),
                payload,
            });
        };
        let mut current_prompt_id: Option<String> = None;
        let mut backoff_ms: u64 = 0;

        'reconnect: loop {
            if backoff_ms > 0 {
                log::info!("WebSocket (headless) reconnecting in {} ms", backoff_ms);
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;

                if let Some(pid) = current_prompt_id.clone() {
                    match ws_state.get_history_for(&pid).await {
                        Ok(history) => {
                            let completed =
                                history.get(&pid).map(|v| !v.is_null()).unwrap_or(false);
                            if completed {
                                log::warn!(
                                    "Prompt {} completed during WS disconnect (headless) — emitting synthetic completion",
                                    pid
                                );
                                emit(
                                    "comfyui:executing",
                                    serde_json::json!({"node": null, "prompt_id": pid}),
                                );
                                current_prompt_id = None;
                            }
                        }
                        Err(e) => log::warn!("History query for {} failed: {}", pid, e),
                    }
                }
            }

            let result = connect_async_with_config(&ws_url, Some(comfyui_ws_config()), false).await;
            let (ws_stream, _) = match result {
                Ok(s) => {
                    backoff_ms = 0;
                    s
                }
                Err(e) => {
                    log::error!("WebSocket connection failed (headless): {}", e);
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false}),
                    );
                    backoff_ms = (backoff_ms.max(500) * 2).min(30_000);
                    continue 'reconnect;
                }
            };

            emit("comfyui:connection", serde_json::json!({"connected": true}));

            let (_, mut read) = ws_stream.split();

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
                        if current_prompt_id.is_none()
                            && matches!(event_type, 1 | 2 | 4 | 100 | 101 | 102)
                        {
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
                                    let b64 = base64::engine::general_purpose::STANDARD
                                        .encode(image_data);
                                    serde_json::json!({ "image": b64, "format": format, "prompt_id": prompt_id_str })
                                };
                                emit("comfyui:preview", payload);
                            }
                            4 => {
                                if data.len() < 8 {
                                    continue;
                                }
                                let meta_len =
                                    u32::from_be_bytes([data[4], data[5], data[6], data[7]])
                                        as usize;
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
                            100 | 101 => {
                                let prompt_id_str = current_prompt_id.as_deref().unwrap();
                                let img = match process_output_image(&data).await {
                                    Some(img) => img,
                                    None => continue,
                                };
                                let payload = build_sse_payload(&img, prompt_id_str);
                                let frontend_event = if event_type == 101 {
                                    "comfyui:controlnet_preprocessor"
                                } else {
                                    "comfyui:output_image"
                                };
                                emit(frontend_event, payload);
                            }
                            102 => {
                                let prompt_id_str = current_prompt_id.as_deref().unwrap();
                                if let Some(payload) =
                                    handle_video_output(&ws_state, &data, prompt_id_str).await
                                {
                                    emit("comfyui:output_video", payload);
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                        log::warn!("WebSocket closed by server (headless) — will reconnect");
                        break;
                    }
                    Err(e) => {
                        log::error!("WebSocket error (headless): {} — will reconnect", e);
                        break;
                    }
                    _ => {}
                }
            }

            emit(
                "comfyui:connection",
                serde_json::json!({"connected": false}),
            );
            backoff_ms = (backoff_ms.max(500) * 2).min(30_000);
        }
    });

    *state.ws_handle.lock().await = Some(task);
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
    state: &Arc<AppState>,
    worker: &std::sync::Arc<super::gpu_manager::GpuWorker>,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
) -> Result<(), AppError> {
    connect_websocket_for_worker_inner(
        #[cfg(feature = "desktop")]
        None,
        state,
        worker,
        event_tx,
    )
    .await
}

/// Desktop variant that also mirrors worker events to Tauri listeners.
#[cfg(feature = "desktop")]
pub async fn connect_websocket_for_worker_desktop(
    app_handle: AppHandle,
    state: &Arc<AppState>,
    worker: &std::sync::Arc<super::gpu_manager::GpuWorker>,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
) -> Result<(), AppError> {
    connect_websocket_for_worker_inner(Some(app_handle), state, worker, event_tx).await
}

async fn connect_websocket_for_worker_inner(
    #[cfg(feature = "desktop")] app_handle: Option<AppHandle>,
    state: &Arc<AppState>,
    worker: &std::sync::Arc<super::gpu_manager::GpuWorker>,
    event_tx: tokio::sync::broadcast::Sender<crate::state::BroadcastEvent>,
) -> Result<(), AppError> {
    {
        let mut handle = worker.ws_handle.lock().await;
        if handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false) {
            log::debug!(
                "Worker {} WebSocket already connected; skipping reconnect",
                worker.id
            );
            return Ok(());
        }
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
    #[cfg(feature = "desktop")]
    let app = app_handle;
    let ws_state = Arc::clone(state);

    let task = tokio::spawn(async move {
        let emit = |event: &str, payload: serde_json::Value| {
            #[cfg(feature = "desktop")]
            if let Some(app) = &app {
                let _ = app.emit(event, payload.clone());
            }
            if let Some(prompt_id) = payload.get("prompt_id").and_then(|v| v.as_str()) {
                cache_temp_event(&ws_state, event, prompt_id, &payload);
            }
            let _ = tx.send(crate::state::BroadcastEvent {
                event: event.to_string(),
                payload,
            });
        };
        let mut current_prompt_id: Option<String> = None;
        let mut backoff_ms: u64 = 0;

        'reconnect: loop {
            if backoff_ms > 0 {
                log::info!(
                    "Worker {} WebSocket reconnecting in {} ms",
                    worker_id,
                    backoff_ms
                );
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;

                if let Some(pid) = current_prompt_id.clone() {
                    match ws_state.get_history_for(&pid).await {
                        Ok(history) => {
                            let completed =
                                history.get(&pid).map(|v| !v.is_null()).unwrap_or(false);
                            if completed {
                                log::warn!(
                                    "Worker {}: prompt {} completed during WS disconnect — emitting synthetic completion",
                                    worker_id, pid
                                );
                                emit(
                                    "comfyui:executing",
                                    serde_json::json!({"node": null, "prompt_id": pid}),
                                );
                                current_prompt_id = None;
                            }
                        }
                        Err(e) => log::warn!("History query for {} failed: {}", pid, e),
                    }
                }
            }

            let result = connect_async_with_config(&ws_url, Some(comfyui_ws_config()), false).await;
            let (ws_stream, _) = match result {
                Ok(s) => {
                    backoff_ms = 0;
                    s
                }
                Err(e) => {
                    log::error!("Worker {} WebSocket connection failed: {}", worker_id, e);
                    emit(
                        "comfyui:connection",
                        serde_json::json!({"connected": false, "worker_id": worker_id}),
                    );
                    backoff_ms = (backoff_ms.max(500) * 2).min(30_000);
                    continue 'reconnect;
                }
            };

            log::info!("Worker {} WebSocket connected", worker_id);
            emit(
                "comfyui:connection",
                serde_json::json!({"connected": true, "worker_id": worker_id}),
            );

            let (_, mut read) = ws_stream.split();

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
                        if current_prompt_id.is_none()
                            && matches!(event_type, 1 | 2 | 4 | 100 | 101 | 102)
                        {
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
                                    let b64 = base64::engine::general_purpose::STANDARD
                                        .encode(image_data);
                                    serde_json::json!({ "image": b64, "format": format, "prompt_id": prompt_id_str })
                                };
                                emit("comfyui:preview", payload);
                            }
                            4 => {
                                if data.len() < 8 {
                                    continue;
                                }
                                let meta_len =
                                    u32::from_be_bytes([data[4], data[5], data[6], data[7]])
                                        as usize;
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
                            100 | 101 => {
                                let prompt_id_str = current_prompt_id.as_deref().unwrap();
                                let img = match process_output_image(&data).await {
                                    Some(img) => img,
                                    None => continue,
                                };
                                let payload = build_sse_payload(&img, prompt_id_str);
                                let frontend_event = if event_type == 101 {
                                    "comfyui:controlnet_preprocessor"
                                } else {
                                    "comfyui:output_image"
                                };
                                emit(frontend_event, payload);
                            }
                            102 => {
                                let prompt_id_str = current_prompt_id.as_deref().unwrap();
                                if let Some(payload) =
                                    handle_video_output(&ws_state, &data, prompt_id_str).await
                                {
                                    emit("comfyui:output_video", payload);
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                        log::warn!("Worker {} WebSocket closed — will reconnect", worker_id);
                        break;
                    }
                    Err(e) => {
                        log::error!(
                            "Worker {} WebSocket error: {} — will reconnect",
                            worker_id,
                            e
                        );
                        break;
                    }
                    _ => {}
                }
            }

            emit(
                "comfyui:connection",
                serde_json::json!({"connected": false, "worker_id": worker_id}),
            );
            backoff_ms = (backoff_ms.max(500) * 2).min(30_000);
        }
    });

    *worker.ws_handle.lock().await = Some(task);
    Ok(())
}
