//! Collect a local refiner result before continuing with NovelAI faces.
//!
//! This uses a private ComfyUI client session: the ordinary websocket must not
//! publish the intermediate image or finish the generation's queue entry.
use std::sync::Arc;
use std::time::Duration;

use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async_with_config, tungstenite::Message};

use super::EventSink;
use crate::comfyui::{gpu_manager::GpuWorker, websocket::comfyui_ws_config};
use crate::{error::AppError, state::AppState};

const WAIT_FOR_WORKER: Duration = Duration::from_secs(300);
const EXECUTION_TIMEOUT: Duration = Duration::from_secs(1800);
const CANCEL_INTERVAL: Duration = Duration::from_millis(250);

pub(super) async fn run(
    state: &Arc<AppState>,
    sink: &EventSink,
    prompt_id: &str,
    workflow: Value,
) -> Result<Vec<u8>, AppError> {
    sink.emit(
        "comfyui:progress",
        json!({
            "prompt_id": prompt_id, "node": "Refiner", "value": 0, "max": 1,
        }),
    );
    let worker = reserve_worker(state, prompt_id).await?;
    // Cancellation should target this worker while the local stage runs. No
    // alias is bound: history reconciliation must not mistake local completion
    // for completion of the entire NovelAI generation.
    state.prompt_queue.set_worker(prompt_id, worker.id);
    let result = run_reserved(state, sink, prompt_id, &worker, workflow).await;
    state.prompt_queue.clear_worker(prompt_id);
    state.gpu_manager.mark_worker_idle(worker.id).await;
    result
}

async fn reserve_worker(state: &AppState, prompt_id: &str) -> Result<Arc<GpuWorker>, AppError> {
    let deadline = tokio::time::Instant::now() + WAIT_FOR_WORKER;
    loop {
        if state.prompt_queue.is_cancelled(prompt_id) {
            return Err(AppError::Other("Local refiner cancelled".into()));
        }
        if let Some(worker) = state.gpu_manager.find_available().await {
            if worker.try_reserve() {
                return Ok(worker);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::Other(
                "Timed out waiting for a local refiner worker".into(),
            ));
        }
        tokio::time::sleep(CANCEL_INTERVAL).await;
    }
}

async fn run_reserved(
    state: &AppState,
    sink: &EventSink,
    prompt_id: &str,
    worker: &Arc<GpuWorker>,
    workflow: Value,
) -> Result<Vec<u8>, AppError> {
    let client_id = format!("{}-refine-{}", state.client_id, uuid::Uuid::new_v4());
    let ws_base = worker.base_url.replacen("http", "ws", 1);
    let url = format!("{ws_base}/ws?clientId={client_id}");
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(15),
        connect_async_with_config(url, Some(comfyui_ws_config()), false),
    )
    .await
    .map_err(|_| AppError::Other("Local refiner websocket connection timed out".into()))?
    .map_err(|err| AppError::WebSocketError(err.to_string()))?;
    if state.prompt_queue.is_cancelled(prompt_id) {
        return Err(AppError::Other("Local refiner cancelled".into()));
    }
    let (_, response) = state
        .gpu_manager
        .do_submit(worker, workflow, &client_id)
        .await?;
    let result = tokio::time::timeout(
        EXECUTION_TIMEOUT,
        collect_output(
            &mut socket,
            &response.prompt_id,
            || state.prompt_queue.is_cancelled(prompt_id),
            |event, mut payload| {
                payload["prompt_id"] = json!(prompt_id);
                sink.emit(event, payload);
            },
        ),
    )
    .await
    .unwrap_or_else(|_| Err(AppError::Other("Local refiner execution timed out".into())));
    if result.is_err() {
        // Delete only this private prompt, then target its running execution.
        // ComfyUI's prompt_id interrupt does not stop a neighbour's generation.
        for (path, body) in [
            ("queue", json!({"delete": [&response.prompt_id]})),
            ("interrupt", json!({"prompt_id": &response.prompt_id})),
        ] {
            if let Err(err) = state
                .http_client
                .post(format!("{}/{path}", worker.base_url))
                .json(&body)
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
                log::warn!("Local refiner cleanup {path} failed: {err}");
            }
        }
    }
    result
}

/// Relays only progress/previews. The final PNG stays private until all face
/// requests finish, and a local terminal event never escapes to the frontend.
async fn collect_output<S, E>(
    socket: &mut S,
    local_prompt_id: &str,
    cancelled: impl Fn() -> bool,
    mut emit: impl FnMut(&str, Value),
) -> Result<Vec<u8>, AppError>
where
    S: Stream<Item = Result<Message, E>> + Unpin,
    E: std::fmt::Display,
{
    let mut output = None;
    let mut active = false;
    let mut cancellation = tokio::time::interval(CANCEL_INTERVAL);
    loop {
        let message = tokio::select! {
            _ = cancellation.tick() => {
                if cancelled() { return Err(AppError::Other("Local refiner cancelled".into())); }
                continue;
            }
            message = socket.next() => message,
        };
        if cancelled() {
            return Err(AppError::Other("Local refiner cancelled".into()));
        }
        match message {
            Some(Ok(Message::Text(text))) => {
                let Ok(event) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let data = &event["data"];
                if data["prompt_id"].as_str() != Some(local_prompt_id) {
                    continue;
                }
                match event["type"].as_str() {
                    Some("execution_start") => active = true,
                    Some("executing") if data["node"].is_string() => {
                        active = true;
                        emit("comfyui:executing", json!({"node": "Refiner"}));
                    }
                    Some("progress") => {
                        let mut progress = data.clone();
                        progress["node"] = json!("Refiner");
                        emit("comfyui:progress", progress);
                    }
                    Some("executing") if data.get("node").is_some_and(Value::is_null) => {
                        return output.ok_or_else(|| {
                            AppError::Other("Local refiner completed without an image".into())
                        });
                    }
                    Some("execution_error" | "execution_interrupted") => {
                        return Err(AppError::Other(format!(
                            "Local refiner failed: {}",
                            data["exception_message"]
                                .as_str()
                                .unwrap_or("execution interrupted")
                        )));
                    }
                    _ => {}
                }
            }
            Some(Ok(Message::Binary(data))) if active && data.len() >= 8 => {
                let event = u32::from_be_bytes(data[..4].try_into().unwrap());
                let format = u32::from_be_bytes(data[4..8].try_into().unwrap());
                let preview = match event {
                    // The caller requests lossless 8-bit PNG for the NAI stage.
                    100 if format == 1 && data[8..].starts_with(b"\x89PNG\r\n\x1a\n") => {
                        output = Some(data[8..].to_vec());
                        Some((&data[8..], "png"))
                    }
                    1 | 2 => Some((&data[8..], if format == 2 { "png" } else { "jpg" })),
                    4 => data
                        .get(8usize.saturating_add(format as usize)..)
                        .map(|bytes| (bytes, "jpg")),
                    _ => None,
                };
                if let Some((bytes, ext)) = preview {
                    if let Some(filename) = crate::temp_images::save(bytes, ext) {
                        emit(
                            "comfyui:preview",
                            json!({"temp_filename": filename, "format": if ext == "jpg" { "jpeg" } else { ext }}),
                        );
                    }
                }
            }
            Some(Err(err)) => return Err(AppError::WebSocketError(err.to_string())),
            None | Some(Ok(Message::Close(_))) => {
                return Err(AppError::Other(
                    "Local refiner connection closed before completion".into(),
                ))
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(event: &str, data: Value) -> Result<Message, String> {
        Ok(Message::Text(
            json!({"type": event, "data": data}).to_string().into(),
        ))
    }

    fn png_frame() -> (Vec<u8>, Result<Message, String>) {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([12, 34, 56, 255]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut frame = Vec::new();
        frame.extend_from_slice(&100u32.to_be_bytes());
        frame.extend_from_slice(&1u32.to_be_bytes());
        frame.extend_from_slice(&png);
        (png, Ok(Message::Binary(frame.into())))
    }

    // On Windows the desktop AppState pulls in Tauri's common-controls imports,
    // but Rust's test executable has no Tauri application manifest. Exercise
    // the full AppState/worker path in the server build there; the transport
    // collector tests below run in both builds.
    #[cfg(not(all(target_os = "windows", feature = "desktop")))]
    #[tokio::test]
    async fn immediate_comfy_result_keeps_the_novelai_queue_alive_and_releases_the_worker() {
        use futures_util::SinkExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (png, frame) = png_frame();
        let server = tokio::spawn(async move {
            // The result listener must connect before POST /prompt. Send all
            // output before the HTTP response to exercise the fast-job race.
            let (connection, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(connection).await.unwrap();
            let (mut http, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let body_start = loop {
                assert!(http.read_buf(&mut request).await.unwrap() > 0);
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..body_start]).to_ascii_lowercase();
            assert!(headers.starts_with("post /prompt "));
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap()
                .parse()
                .unwrap();
            while request.len() < body_start + length {
                assert!(http.read_buf(&mut request).await.unwrap() > 0);
            }
            let body: Value =
                serde_json::from_slice(&request[body_start..body_start + length]).unwrap();
            ws.send(text("execution_start", json!({"prompt_id": "local"})).unwrap())
                .await
                .unwrap();
            ws.send(frame.unwrap()).await.unwrap();
            ws.send(text("executing", json!({"prompt_id": "local", "node": null})).unwrap())
                .await
                .unwrap();
            let response =
                json!({"prompt_id": "local", "number": 1, "node_errors": {}}).to_string();
            http.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
            body
        });
        let state = Arc::new(AppState::new(crate::config::AppConfig {
            server_port: port,
            ..Default::default()
        }));
        *state.gpu_manager.workers[0].status.write().await =
            crate::comfyui::gpu_manager::WorkerStatus::Idle;
        state.prompt_queue.insert("nai-test", Some("alice".into()));
        let sink = EventSink::new(
            Arc::clone(&state),
            #[cfg(feature = "desktop")]
            None,
        );
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            run(&state, &sink, "nai-test", json!({"test": "workflow"})),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result, png);
        assert_eq!(state.prompt_queue.len(), 1);
        assert!(state
            .prompt_queue
            .is_owned_by("nai-test", &Some("alice".into())));
        assert_eq!(state.prompt_queue.related_ids("nai-test"), vec!["nai-test"]);
        assert_eq!(state.prompt_queue.worker_of("nai-test"), None);
        assert!(state.gpu_manager.workers[0].is_available().await);
        let submitted = server.await.unwrap();
        assert_eq!(submitted["prompt"], json!({"test": "workflow"}));
        assert_ne!(
            submitted["client_id"].as_str(),
            Some(state.client_id.as_str())
        );
    }

    #[cfg(not(all(target_os = "windows", feature = "desktop")))]
    #[tokio::test]
    async fn connection_failure_releases_the_reserved_worker_without_finishing_the_generation() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            drop(connection); // Refuse the websocket handshake.
        });
        let state = Arc::new(AppState::new(crate::config::AppConfig {
            server_port: port,
            ..Default::default()
        }));
        *state.gpu_manager.workers[0].status.write().await =
            crate::comfyui::gpu_manager::WorkerStatus::Idle;
        state.prompt_queue.insert("nai-test", None);
        let sink = EventSink::new(
            Arc::clone(&state),
            #[cfg(feature = "desktop")]
            None,
        );
        assert!(run(&state, &sink, "nai-test", json!({})).await.is_err());
        assert!(state.gpu_manager.workers[0].is_available().await);
        assert_eq!(state.prompt_queue.worker_of("nai-test"), None);
        assert_eq!(state.prompt_queue.len(), 1);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn refined_png_returns_privately_without_a_generation_completion() {
        let (png, frame) = png_frame();
        let mut socket = futures_util::stream::iter(vec![
            text(
                "executing",
                json!({"prompt_id": "someone-else", "node": null}),
            ),
            text("execution_start", json!({"prompt_id": "local"})),
            text("executing", json!({"prompt_id": "local", "node": "12"})),
            text(
                "progress",
                json!({"prompt_id": "local", "value": 5, "max": 20}),
            ),
            frame,
            text("executing", json!({"prompt_id": "local", "node": null})),
        ]);
        let mut events = Vec::new();
        let result = collect_output(
            &mut socket,
            "local",
            || false,
            |event, payload| events.push((event.to_string(), payload)),
        )
        .await
        .unwrap();
        assert_eq!(result, png);
        assert!(events
            .iter()
            .any(|(event, data)| event == "comfyui:progress" && data["value"] == 5));
        assert!(events
            .iter()
            .all(|(event, data)| event != "comfyui:output_image"
                && !(event == "comfyui:executing" && data["node"].is_null())));
    }

    #[tokio::test]
    async fn unrelated_binary_output_cannot_become_the_refiner_result() {
        let (_, frame) = png_frame();
        let mut socket = futures_util::stream::iter(vec![
            frame,
            text("execution_start", json!({"prompt_id": "local"})),
            text("executing", json!({"prompt_id": "local", "node": null})),
        ]);
        let err = collect_output(&mut socket, "local", || false, |_, _| {})
            .await
            .unwrap_err();
        assert!(err.to_string().contains("without an image"));
    }

    #[tokio::test]
    async fn execution_error_keeps_the_caller_on_its_original_image() {
        let (_, frame) = png_frame();
        let mut socket = futures_util::stream::iter(vec![
            text("execution_start", json!({"prompt_id": "local"})),
            frame,
            text(
                "execution_error",
                json!({"prompt_id": "local", "exception_message": "out of memory"}),
            ),
        ]);
        let err = collect_output(&mut socket, "local", || false, |_, _| {})
            .await
            .unwrap_err();
        assert!(err.to_string().contains("out of memory"));
    }

    #[tokio::test]
    async fn cancelled_refinement_never_publishes_or_returns_a_result() {
        let (_, frame) = png_frame();
        let mut socket = futures_util::stream::iter(vec![frame]);
        let err = collect_output(
            &mut socket,
            "local",
            || true,
            |_, _| panic!("cancelled prompt emitted output"),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("cancelled"));
    }

    #[tokio::test]
    async fn cancellation_is_checked_even_when_the_socket_is_silent() {
        let mut socket = futures_util::stream::pending::<Result<Message, String>>();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            collect_output(&mut socket, "local", || true, |_, _| {}),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().to_string().contains("cancelled"));
    }

    #[tokio::test]
    async fn closed_socket_is_a_failure_even_after_an_image_frame() {
        let (_, frame) = png_frame();
        let mut socket = futures_util::stream::iter(vec![
            text("execution_start", json!({"prompt_id": "local"})),
            frame,
        ]);
        assert!(collect_output(&mut socket, "local", || false, |_, _| {})
            .await
            .unwrap_err()
            .to_string()
            .contains("before completion"));
    }
}
