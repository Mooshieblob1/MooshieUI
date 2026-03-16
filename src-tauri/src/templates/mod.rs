pub mod img2img;
pub mod inpainting;
pub mod txt2img;
pub mod upscale;

use serde_json::{json, Value};

use crate::comfyui::types::GenerationParams;

pub struct WorkflowResult {
    pub workflow: serde_json::Map<String, Value>,
    pub next_id: u32,
    pub image_output: (String, u32),
    pub model_source: (String, u32),
    pub positive_id: String,
    pub negative_id: String,
    pub vae_source: (String, u32),
}

pub fn build_workflow(params: &GenerationParams, seed: i64) -> Value {
    let mut result = match params.mode.as_str() {
        "img2img" => img2img::build(params, seed),
        "inpainting" => inpainting::build(params, seed),
        _ => txt2img::build(params, seed),
    };

    let final_image = if params.upscale_enabled {
        upscale::append_upscale_chain(&mut result, params, seed)
    } else {
        result.image_output.clone()
    };

    let save_id = result.next_id.to_string();
    result.workflow.insert(
        save_id,
        json!({
            "class_type": "SaveImage",
            "inputs": {
                "images": [final_image.0, final_image.1],
                "filename_prefix": "ComfyUI"
            }
        }),
    );

    Value::Object(result.workflow)
}
