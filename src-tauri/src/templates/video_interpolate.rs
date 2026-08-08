//! Post-hoc frame interpolation: take a clip that is already in the gallery and
//! re-encode it with interpolated frames.
//!
//! Deliberately a real ComfyUI job rather than a local pass, so it inherits
//! queue position, progress events, cancellation, and gallery saving from the
//! normal generation path. The result is saved as a new entry; the source clip
//! is never touched.

use serde_json::{json, Map, Value};

use crate::templates::rife::RifeSettings;

/// `MooshieLoadVideoPath -> RIFE VFI -> CreateVideo -> MooshieSaveVideo`.
///
/// The loader reports `output_fps` (source rate x multiplier) as a FLOAT that
/// feeds `CreateVideo.fps`, so Rust never has to know the clip's real frame
/// rate. That is what makes a second pass over an already-interpolated clip
/// come out at 96 fps rather than snapping back to 48.
pub fn build(video_path: &str, settings: RifeSettings) -> Value {
    let mut workflow = Map::new();

    let load_id = "1".to_string();
    workflow.insert(
        load_id.clone(),
        json!({
            "class_type": "MooshieLoadVideoPath",
            "inputs": {
                "video_path": video_path,
                "fps_multiplier": settings.multiplier
            }
        }),
    );

    let rife_id = "2".to_string();
    workflow.insert(rife_id.clone(), settings.node(json!([load_id.as_str(), 0])));

    let create_id = "3".to_string();
    workflow.insert(
        create_id.clone(),
        json!({
            "class_type": "CreateVideo",
            "inputs": {
                "images": [rife_id.as_str(), 0],
                "audio": [load_id.as_str(), 1],
                "fps": [load_id.as_str(), 2]
            }
        }),
    );

    workflow.insert(
        "4".to_string(),
        json!({
            "class_type": "MooshieSaveVideo",
            "inputs": {
                "video": [create_id.as_str(), 0],
                "filename_prefix": "mooshie_video"
            }
        }),
    );

    Value::Object(workflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::rife::RifeSettings;
    use serde_json::{json, Value};

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
    fn graph_is_loader_rife_create_save() {
        let settings = RifeSettings::sanitized(3, 1.0, true, true);
        let workflow = build("D:\\gallery\\mooshie_video_00001_.mp4", settings);

        let (load_id, load) = node_of_class(&workflow, "MooshieLoadVideoPath");
        let (rife_id, rife) = node_of_class(&workflow, "RIFE VFI");
        let (create_id, create) = node_of_class(&workflow, "CreateVideo");
        let (_, save) = node_of_class(&workflow, "MooshieSaveVideo");

        assert_eq!(
            load["inputs"]["video_path"],
            json!("D:\\gallery\\mooshie_video_00001_.mp4")
        );
        assert_eq!(load["inputs"]["fps_multiplier"], json!(3));
        assert_eq!(rife["inputs"]["frames"], json!([load_id, 0]));
        assert_eq!(rife["inputs"]["multiplier"], json!(3));
        assert_eq!(create["inputs"]["images"], json!([rife_id, 0]));
        assert_eq!(create["inputs"]["audio"], json!([load_id, 1]));
        assert_eq!(create["inputs"]["fps"], json!([load_id, 2]));
        assert_eq!(save["inputs"]["video"], json!([create_id, 0]));
        assert_eq!(save["inputs"]["filename_prefix"], json!("mooshie_video"));
    }

    #[test]
    fn loader_multiplier_always_matches_the_rife_node() {
        // The loader decides playback rate and the node decides how many frames
        // exist. If they ever disagree the clip changes length, which is the one
        // thing this feature promises not to do.
        for requested in [1u32, 2, 4, 99] {
            let settings = RifeSettings::sanitized(requested, 1.0, true, true);
            let workflow = build("clip.mp4", settings);
            let (_, load) = node_of_class(&workflow, "MooshieLoadVideoPath");
            let (_, rife) = node_of_class(&workflow, "RIFE VFI");
            assert_eq!(
                load["inputs"]["fps_multiplier"],
                rife["inputs"]["multiplier"]
            );
        }
    }

    #[test]
    fn advanced_knobs_reach_the_node() {
        let workflow = build("clip.mp4", RifeSettings::sanitized(2, 0.5, false, false));
        let (_, rife) = node_of_class(&workflow, "RIFE VFI");
        assert_eq!(rife["inputs"]["scale_factor"], json!(0.5));
        assert_eq!(rife["inputs"]["fast_mode"], json!(false));
        assert_eq!(rife["inputs"]["ensemble"], json!(false));
    }
}
