//! Running the local YOLO detector for the NovelAI face pass.
//!
//! Detection is the one part of this feature that stays on the user's machine.
//! It goes through ComfyUI because that is where Ultralytics and the detector
//! weights already live, but the graph loads no checkpoint, CLIP or VAE, so it
//! works on an install that has ComfyUI for its detector alone.
//!
//! The boxes come back over HTTP rather than the websocket. The app's
//! websocket handler keys off prompt ids it knows about, so this submits under
//! its own client id: ComfyUI addresses `executing` / `progress` / `executed`
//! to the session matching the prompt's `client_id`, and a detect prompt the
//! progress store has never heard of would otherwise look like a stray
//! generation.

use std::time::Duration;

use serde_json::Value;

use crate::error::AppError;
use crate::state::AppState;
use crate::templates::face_detect::{self, FaceBox};

/// How long to wait for a free GPU worker.
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(120);
/// How long to wait for the detect prompt to finish once it is queued.
const POLL_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Detect faces in `image_bytes` (PNG) and return their boxes, strongest first.
///
/// An empty vector means the detector ran and found nothing, which is a normal
/// outcome; an `Err` means the detector could not run at all.
pub async fn detect_faces(
    state: &AppState,
    image_bytes: Vec<u8>,
    detector_model: &str,
    threshold: f64,
    max_faces: u32,
) -> Result<Vec<FaceBox>, AppError> {
    if detector_model.trim().is_empty() {
        return Err(AppError::Other(
            "No face detector model is selected".to_string(),
        ));
    }

    let filename = format!("mooshie-facedetect-{}.png", uuid::Uuid::new_v4());
    let upload = state.upload_image_from_bytes(image_bytes, filename).await?;
    let input_name = if upload.subfolder.is_empty() {
        upload.name.clone()
    } else {
        format!("{}/{}", upload.subfolder, upload.name)
    };

    let workflow = face_detect::build_workflow(&input_name, detector_model, threshold, max_faces);

    let client_id = format!("{}-facedetect", state.client_id);
    let (worker_id, response) = state
        .gpu_manager
        .submit_prompt(workflow, &client_id, SUBMIT_TIMEOUT)
        .await?;

    let result = poll_for_boxes(state, worker_id, &response.prompt_id).await;

    // `submit_prompt` may have reserved the worker for us, and nothing else
    // releases it: the websocket handler that normally does never sees this
    // prompt, because it ran under a different client id. The name reads
    // wrong for a success path, but this is the only public API that calls
    // `release()` and puts the worker back to Idle.
    state
        .gpu_manager
        .mark_worker_error_then_idle(worker_id)
        .await;

    result
}

async fn poll_for_boxes(
    state: &AppState,
    worker_id: u32,
    prompt_id: &str,
) -> Result<Vec<FaceBox>, AppError> {
    let deadline = tokio::time::Instant::now() + POLL_TIMEOUT;
    loop {
        // Read from the worker that actually ran the prompt: in a multi-GPU
        // deployment the generic history helper would ask whichever worker is
        // ready first, and that one knows nothing about this prompt.
        let history = state
            .gpu_manager
            .get_history_from_worker(worker_id, prompt_id)
            .await?;

        if let Some(outcome) = read_history_entry(&history, prompt_id) {
            return outcome.map_err(AppError::Other);
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::Other(
                "Face detection timed out waiting for ComfyUI".to_string(),
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Interpret one `/history/{prompt_id}` response.
///
/// `None` means the prompt has not finished yet, which is the only reason to
/// keep polling. Anything else is terminal, success or failure.
fn read_history_entry(history: &Value, prompt_id: &str) -> Option<Result<Vec<FaceBox>, String>> {
    let entry = history.get(prompt_id)?;
    let status = entry.get("status");

    let status_str = status
        .and_then(|s| s.get("status_str"))
        .and_then(Value::as_str);
    if status_str == Some("error") {
        return Some(Err(format!(
            "Face detection failed in ComfyUI: {}",
            status.map(describe_error).unwrap_or_default()
        )));
    }

    let completed = status
        .and_then(|s| s.get("completed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let outputs = entry.get("outputs");
    let has_outputs = outputs
        .and_then(Value::as_object)
        .is_some_and(|o| !o.is_empty());

    if !completed && !has_outputs {
        return None;
    }

    // `parse_boxes` reports a missing output itself, which is the right answer
    // for a prompt that completed with nothing to show.
    Some(face_detect::parse_boxes(outputs.unwrap_or(&Value::Null)))
}

/// Pull something readable out of ComfyUI's `status.messages` array.
///
/// Messages are `[name, payload]` pairs; the useful one is the execution error
/// and its `exception_message`.
fn describe_error(status: &Value) -> String {
    let messages = status
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    for message in messages {
        let payload = message.get(1);
        if let Some(text) = payload
            .and_then(|p| p.get("exception_message"))
            .and_then(Value::as_str)
        {
            return text.to_string();
        }
    }
    "no error detail reported".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn detect_output(boxes: Value) -> Value {
        let payload = json!({ "width": 832, "height": 1216, "boxes": boxes }).to_string();
        json!({ face_detect::DETECT_NODE_ID: { "text": [payload] } })
    }

    #[test]
    fn an_unknown_prompt_keeps_polling() {
        assert!(read_history_entry(&json!({}), "p1").is_none());
    }

    #[test]
    fn a_queued_prompt_with_no_outputs_keeps_polling() {
        let history = json!({
            "p1": { "status": { "status_str": "success", "completed": false }, "outputs": {} },
        });
        assert!(read_history_entry(&history, "p1").is_none());
    }

    #[test]
    fn a_completed_prompt_yields_boxes() {
        let history = json!({
            "p1": {
                "status": { "status_str": "success", "completed": true },
                "outputs": detect_output(json!([
                    { "x1": 10, "y1": 20, "x2": 110, "y2": 140, "confidence": 0.9 },
                ])),
            },
        });
        let boxes = read_history_entry(&history, "p1")
            .expect("terminal")
            .expect("parsed");
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].width(), 100);
    }

    #[test]
    fn a_failed_prompt_surfaces_its_exception_message() {
        let history = json!({
            "p1": {
                "status": {
                    "status_str": "error",
                    "completed": false,
                    "messages": [
                        ["execution_start", {}],
                        ["execution_error", { "exception_message": "No module named 'ultralytics'" }],
                    ],
                },
                "outputs": {},
            },
        });
        let err = read_history_entry(&history, "p1")
            .expect("terminal")
            .unwrap_err();
        assert!(err.contains("ultralytics"), "got: {err}");
    }

    #[test]
    fn a_completed_prompt_with_no_output_node_is_an_error() {
        let history = json!({
            "p1": { "status": { "status_str": "success", "completed": true }, "outputs": {} },
        });
        assert!(read_history_entry(&history, "p1")
            .expect("terminal")
            .is_err());
    }
}
