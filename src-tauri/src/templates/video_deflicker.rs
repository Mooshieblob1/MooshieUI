//! Post-hoc video deflicker: run ComfyUI v0.46.0+ core `OpticalFlowLoader` +
//! `VideoDeflicker` over a finished gallery clip and save the result as a new
//! gallery entry while preserving original FPS and audio.

use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeflickerSettings {
    pub strength: f64,
    pub direction: &'static str,
    pub threshold: f64,
}

impl DeflickerSettings {
    /// Clamp user inputs into ComfyUI `VideoDeflicker` bounds so NaNs or
    /// unexpected direction strings never reach the backend.
    pub fn sanitized(strength: f64, direction: &str, threshold: f64) -> Self {
        Self {
            strength: if strength.is_finite() {
                strength.clamp(0.0, 100.0)
            } else {
                1.0
            },
            direction: if direction == "forward" {
                "forward"
            } else {
                "bidirectional"
            },
            threshold: if threshold.is_finite() {
                threshold.clamp(0.0, 1.0)
            } else {
                0.1
            },
        }
    }
}

/// `MooshieLoadVideoPath(fps_multiplier=1) -> OpticalFlowLoader + VideoDeflicker -> CreateVideo -> MooshieSaveVideo`.
pub fn build(video_path: &str, optical_flow_model: &str, settings: DeflickerSettings) -> Value {
    let mut workflow = Map::new();

    workflow.insert(
        "1".to_string(),
        json!({
            "class_type": "MooshieLoadVideoPath",
            "inputs": {
                "video_path": video_path,
                "fps_multiplier": 1
            }
        }),
    );

    workflow.insert(
        "2".to_string(),
        json!({
            "class_type": "OpticalFlowLoader",
            "inputs": {
                "model_name": optical_flow_model
            }
        }),
    );

    workflow.insert(
        "3".to_string(),
        json!({
            "class_type": "VideoDeflicker",
            "inputs": {
                "optical_flow": ["2", 0],
                "images": ["1", 0],
                "strength": settings.strength,
                "direction": settings.direction,
                "threshold": settings.threshold
            }
        }),
    );

    workflow.insert(
        "4".to_string(),
        json!({
            "class_type": "CreateVideo",
            "inputs": {
                "images": ["3", 0],
                "audio": ["1", 1],
                "fps": ["1", 2]
            }
        }),
    );

    workflow.insert(
        "5".to_string(),
        json!({
            "class_type": "MooshieSaveVideo",
            "inputs": {
                "video": ["4", 0],
                "filename_prefix": "mooshie_video"
            }
        }),
    );

    Value::Object(workflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_of_class<'a>(workflow: &'a Value, class_type: &str) -> (String, &'a Value) {
        workflow
            .as_object()
            .expect("workflow is an object")
            .iter()
            .find(|(_, node)| node["class_type"] == class_type)
            .map(|(id, node)| (id.clone(), node))
            .unwrap_or_else(|| panic!("no {class_type} node in workflow"))
    }

    #[test]
    fn graph_wires_optical_flow_deflicker_with_preserved_fps_and_audio() {
        let settings = DeflickerSettings::sanitized(1.5, "forward", 0.25);
        let workflow = build(
            "D:\\gallery\\mooshie_video_00001_.mp4",
            "raft_large_C_T_SKHT_V2-ff5fadd5.safetensors",
            settings,
        );

        let (load_id, load) = node_of_class(&workflow, "MooshieLoadVideoPath");
        assert_eq!(load["inputs"]["fps_multiplier"], json!(1));
        assert_eq!(
            load["inputs"]["video_path"],
            json!("D:\\gallery\\mooshie_video_00001_.mp4")
        );

        let (flow_id, flow) = node_of_class(&workflow, "OpticalFlowLoader");
        assert_eq!(
            flow["inputs"]["model_name"],
            json!("raft_large_C_T_SKHT_V2-ff5fadd5.safetensors")
        );

        let (deflicker_id, deflicker) = node_of_class(&workflow, "VideoDeflicker");
        assert_eq!(deflicker["inputs"]["optical_flow"], json!([flow_id, 0]));
        assert_eq!(deflicker["inputs"]["images"], json!([load_id, 0]));
        assert_eq!(deflicker["inputs"]["strength"], json!(1.5));
        assert_eq!(deflicker["inputs"]["direction"], json!("forward"));
        assert_eq!(deflicker["inputs"]["threshold"], json!(0.25));

        let (create_id, create) = node_of_class(&workflow, "CreateVideo");
        assert_eq!(create["inputs"]["images"], json!([deflicker_id, 0]));
        assert_eq!(create["inputs"]["audio"], json!([load_id, 1]));
        assert_eq!(create["inputs"]["fps"], json!([load_id, 2]));

        let (_, save) = node_of_class(&workflow, "MooshieSaveVideo");
        assert_eq!(save["inputs"]["video"], json!([create_id, 0]));
        assert_eq!(save["inputs"]["filename_prefix"], json!("mooshie_video"));
    }

    #[test]
    fn sanitizes_bounds_nans_and_direction() {
        let clamped_high = DeflickerSettings::sanitized(250.0, "unknown", 2.5);
        assert_eq!(clamped_high.strength, 100.0);
        assert_eq!(clamped_high.direction, "bidirectional");
        assert_eq!(clamped_high.threshold, 1.0);

        let clamped_low = DeflickerSettings::sanitized(-4.0, "forward", -0.2);
        assert_eq!(clamped_low.strength, 0.0);
        assert_eq!(clamped_low.direction, "forward");
        assert_eq!(clamped_low.threshold, 0.0);

        let nans = DeflickerSettings::sanitized(f64::NAN, "bidirectional", f64::INFINITY);
        assert_eq!(nans.strength, 1.0);
        assert_eq!(nans.direction, "bidirectional");
        assert_eq!(nans.threshold, 0.1);
    }
}
