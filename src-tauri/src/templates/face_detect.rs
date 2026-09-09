//! A detect-only ComfyUI graph for the NovelAI face detailer.
//!
//! The NovelAI face pass keeps detection local but sends every crop back to
//! NovelAI, so all this graph does is run YOLO and report boxes. It loads no
//! checkpoint, no CLIP and no VAE, which is what makes it usable on an install
//! that has ComfyUI for its detector alone.
//!
//! The boxes come back through the history endpoint rather than the websocket:
//! `MooshieFaceDetect` is an output node whose `ui.text` payload is a single
//! JSON string, which [`parse_boxes`] decodes.

use serde_json::{json, Value};

/// The graph's only node. Also the key the history's `outputs` map uses.
pub const DETECT_NODE_ID: &str = "2";
const LOAD_NODE_ID: &str = "1";

/// One face, in the source image's pixel space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceBox {
    pub x1: i64,
    pub y1: i64,
    pub x2: i64,
    pub y2: i64,
    pub confidence: f64,
}

impl FaceBox {
    pub fn width(&self) -> i64 {
        self.x2 - self.x1
    }

    pub fn height(&self) -> i64 {
        self.y2 - self.y1
    }
}

/// Build `LoadImage -> MooshieFaceDetect`.
///
/// `input_filename` is the name the image was uploaded under in ComfyUI's
/// input directory. `max_faces` of 0 means every detection above the
/// threshold; the node sorts by confidence before it truncates, so a cap keeps
/// the strongest faces rather than the first ones scanned.
pub fn build_workflow(
    input_filename: &str,
    detector_model: &str,
    threshold: f64,
    max_faces: u32,
) -> Value {
    json!({
        LOAD_NODE_ID: {
            "class_type": "LoadImage",
            "inputs": { "image": input_filename, "upload": "image" },
        },
        DETECT_NODE_ID: {
            "class_type": "MooshieFaceDetect",
            "inputs": {
                "image": [LOAD_NODE_ID, 0],
                "detector_model": detector_model,
                "bbox_threshold": threshold.clamp(0.0, 1.0),
                "max_faces": max_faces,
            },
        },
    })
}

/// Read the boxes out of the node's `ui.text` payload.
///
/// ComfyUI wraps a `ui` dict as `outputs[<node>][<key>] = [values]`, so the
/// JSON this node produced arrives as the first element of a `text` array.
/// Boxes with no area are dropped: a zero-width detection cannot be cropped
/// and would only fail further down.
pub fn parse_boxes(outputs: &Value) -> Result<Vec<FaceBox>, String> {
    let text = outputs
        .get(DETECT_NODE_ID)
        .and_then(|node| node.get("text"))
        .and_then(|text| text.get(0))
        .and_then(Value::as_str)
        .ok_or_else(|| "face detection returned no result".to_string())?;

    let payload: Value =
        serde_json::from_str(text).map_err(|e| format!("face detection returned bad JSON: {e}"))?;

    // The node reports its own failures in-band rather than raising, so a
    // missing detector weight reaches the user as a message instead of a
    // ComfyUI stack trace.
    if let Some(error) = payload.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }

    let boxes = payload
        .get("boxes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    Ok(boxes
        .iter()
        .filter_map(|b| {
            let read = |key: &str| b.get(key).and_then(Value::as_i64);
            let face = FaceBox {
                x1: read("x1")?,
                y1: read("y1")?,
                x2: read("x2")?,
                y2: read("y2")?,
                confidence: b.get("confidence").and_then(Value::as_f64).unwrap_or(0.0),
            };
            (face.width() > 0 && face.height() > 0).then_some(face)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_wires_the_loader_into_the_detector() {
        let wf = build_workflow("nai.png", "face.pt", 0.5, 3);
        assert_eq!(wf[LOAD_NODE_ID]["inputs"]["image"], "nai.png");
        assert_eq!(wf[DETECT_NODE_ID]["class_type"], "MooshieFaceDetect");
        assert_eq!(wf[DETECT_NODE_ID]["inputs"]["image"][0], LOAD_NODE_ID);
        assert_eq!(wf[DETECT_NODE_ID]["inputs"]["detector_model"], "face.pt");
        assert_eq!(wf[DETECT_NODE_ID]["inputs"]["max_faces"], 3);
    }

    #[test]
    fn workflow_clamps_an_out_of_range_threshold() {
        let wf = build_workflow("nai.png", "face.pt", 4.2, 0);
        assert_eq!(wf[DETECT_NODE_ID]["inputs"]["bbox_threshold"], 1.0);
    }

    #[test]
    fn parses_boxes_and_drops_empty_ones() {
        let payload = json!({
            "width": 832, "height": 1216,
            "boxes": [
                { "x1": 10, "y1": 20, "x2": 110, "y2": 140, "confidence": 0.91 },
                { "x1": 5, "y1": 5, "x2": 5, "y2": 60, "confidence": 0.4 },
            ],
        })
        .to_string();
        let outputs = json!({ DETECT_NODE_ID: { "text": [payload] } });

        let boxes = parse_boxes(&outputs).expect("valid payload");
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].width(), 100);
        assert_eq!(boxes[0].height(), 120);
        assert!((boxes[0].confidence - 0.91).abs() < 1e-9);
    }

    #[test]
    fn no_detections_is_success_not_an_error() {
        let payload = json!({ "width": 832, "height": 1216, "boxes": [] }).to_string();
        let outputs = json!({ DETECT_NODE_ID: { "text": [payload] } });
        assert!(parse_boxes(&outputs).expect("valid payload").is_empty());
    }

    #[test]
    fn in_band_error_surfaces_as_an_error() {
        let payload = json!({ "boxes": [], "error": "detector model not found: face.pt" });
        let outputs = json!({ DETECT_NODE_ID: { "text": [payload.to_string()] } });
        assert!(parse_boxes(&outputs)
            .unwrap_err()
            .contains("detector model not found"));
    }

    #[test]
    fn missing_output_is_an_error_rather_than_zero_faces() {
        assert!(parse_boxes(&json!({})).is_err());
    }
}
