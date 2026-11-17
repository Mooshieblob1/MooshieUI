//! Image Edit for Krea 2, driven by an edit LoRA. Three recipes, picked with
//! `krea2_edit_method`:
//!
//!   * `identity` (Krea 2 Identity Edit V2, conradlocke) and `general`
//!     (Krea 2 General Edit V2, same author). Both were trained with dual
//!     conditioning that stock nodes can't give: the reference goes in as
//!     clean VAE tokens next to the target (`Krea2EditModelPatch`) and into
//!     the Qwen3-VL encoder alongside the instruction
//!     (`Krea2EditGroundedEncode`). Both come from the comfyui-krea2edit pack
//!     (Apache-2.0, pure Python, pinned in `comfyui/nodes.rs`). One or two
//!     references: image 1 is the picture being edited (the scene), image 2
//!     an optional person to put into it. The graph follows the pack's own
//!     `workflows/krea2_identity_edit.json`.
//!   * `style` (Comfy-Org's `krea2_style_reference`, by ostris). Core nodes
//!     only, as in ComfyUI's bundled "Image Style Reference (Krea-2 Turbo)"
//!     template: `TextEncodeQwenImageEditPlus` reads the references, and
//!     `FluxKontextMultiReferenceLatentMethod` switches Krea 2's reference
//!     tokens on (`index_timestep_zero`; Krea 2 has no default method, so
//!     without it the references are ignored).
//!
//! The edit LoRA is loaded model-only after the user's own LoRA chain. Its
//! filename comes from the frontend (`krea2_edit_lora`), which picks whichever
//! size of the method's LoRA is installed; validation rejects an empty one.

use serde_json::json;

use super::{insert_vae_decode, load_model_nodes, strip_lora_tags, WorkflowResult};
use crate::comfyui::types::GenerationParams;

/// Most references the edit recipes take. The Identity/General nodes have a
/// scene and a subject input; the style LoRA was trained on one or two.
pub const KREA2_MAX_REFERENCES: usize = 2;

/// `Krea2EditGroundedEncode.grounding_px`: the longest side the text encoder
/// sees. 768 is the pack's default and the V2 model card's setting for every
/// recipe.
const GROUNDING_PX: u32 = 768;

/// `Krea2EditModelPatch.ref_position` values (the pack's `REF_POSITIONS`).
const POSITION_ALIGNED: &str = "aligned (trained)";
const POSITION_BESIDE: &str = "beside (right of target)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Identity,
    General,
    Style,
}

impl Method {
    /// Unknown or empty values fall back to General Edit, the broadest one.
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "identity" => Self::Identity,
            "style" => Self::Style,
            _ => Self::General,
        }
    }

    /// The V2 model card's `ref_boost`: 2 to restage one person, 4 for most
    /// general edits.
    pub fn default_ref_boost(self) -> f64 {
        match self {
            Self::Identity => 2.0,
            Self::General => 4.0,
            Self::Style => 1.0,
        }
    }

    /// Human name for error messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Identity => "Identity Edit",
            Self::General => "General Edit",
            Self::Style => "Style Reference",
        }
    }
}

pub fn method(params: &GenerationParams) -> Method {
    Method::parse(&params.krea2_edit_method)
}

/// `ref_boost` for the last (or only) reference, bounded to the range the
/// model card explores. `None` or a non-finite value uses the recipe default.
fn ref_boost(params: &GenerationParams, method: Method) -> f64 {
    params
        .krea2_edit_ref_boost
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| v.clamp(0.1, 8.0))
        .unwrap_or_else(|| method.default_ref_boost())
}

/// Node classes a Krea 2 Identity/General Edit needs from comfyui-krea2edit,
/// with the fix to show when the pack has not loaded (an external ComfyUI, or
/// a failed install). Empty for every other request: Krea 2 txt2img and the
/// Style Reference recipe use core nodes only.
pub fn required_node_classes(params: &GenerationParams) -> Vec<(&'static str, String)> {
    if params.mode != "image_edit"
        || params.model_architecture != "krea2"
        || method(params) == Method::Style
    {
        return Vec::new();
    }
    ["Krea2EditModelPatch", "Krea2EditGroundedEncode"]
        .into_iter()
        .map(|class| {
            (
                class,
                format!(
                    "Krea 2 {} needs the comfyui-krea2edit custom nodes, which have not loaded. Restart managed ComfyUI so MooshieUI can install them, or install https://github.com/lbouaraba/comfyui-krea2edit at {} on the remote server.",
                    method(params).label(),
                    crate::comfyui::nodes::KREA2_EDIT_PACK_REV
                ),
            )
        })
        .collect()
}

fn reference_images(params: &GenerationParams) -> Vec<&str> {
    let mut refs: Vec<&str> = params
        .edit_reference_images
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .take(KREA2_MAX_REFERENCES)
        .collect();
    if refs.is_empty() {
        // Validation should prevent this; keep the workflow well-formed regardless.
        refs.push("");
    }
    refs
}

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let method = method(params);
    let mut workflow = serde_json::Map::new();
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut next_id = ml.next_id;
    let clip_source = ml.clip_source.clone();
    let vae_source = ml.vae_source.clone();

    // Edit LoRA, model-only, after the user's LoRAs.
    let lora_id = next_id.to_string();
    workflow.insert(
        lora_id.clone(),
        json!({
            "class_type": "LoraLoaderModelOnly",
            "inputs": {
                "model": [ml.model_source.0.clone(), ml.model_source.1],
                "lora_name": params.krea2_edit_lora.trim(),
                "strength_model": 1.0
            }
        }),
    );
    next_id += 1;
    let mut model_source = (lora_id, 0u32);

    let mut load_ids = Vec::new();
    for image in reference_images(params) {
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
            json!({ "class_type": "LoadImage", "inputs": { "image": image } }),
        );
        load_ids.push(id);
        next_id += 1;
    }

    let latent_id = next_id.to_string();
    workflow.insert(
        latent_id.clone(),
        json!({
            "class_type": "EmptySD3LatentImage",
            "inputs": {
                "width": params.width,
                "height": params.height,
                "batch_size": params.batch_size
            }
        }),
    );
    next_id += 1;

    let (pos_source, neg_source) = match method {
        Method::Identity | Method::General => {
            let boost = ref_boost(params, method);
            let mut patch = serde_json::Map::new();
            patch.insert(
                "model".into(),
                json!([model_source.0.clone(), model_source.1]),
            );
            patch.insert("vae".into(), json!([vae_source.0.clone(), vae_source.1]));
            patch.insert("target_latent".into(), json!([latent_id.clone(), 0]));
            patch.insert("fit_mode".into(), json!("fit"));
            // The pack's `ref_boost` / `ref_position` act on the LAST
            // reference (the person when there are two, else the only image);
            // the `_a` pair acts on the first (the scene), which the card
            // keeps aligned at boost 1.
            patch.insert("ref_boost".into(), json!(boost));
            patch.insert("ref_boost_a".into(), json!(1.0));
            let last = if params.krea2_edit_beside {
                POSITION_BESIDE
            } else {
                POSITION_ALIGNED
            };
            patch.insert("ref_position".into(), json!(last));
            patch.insert("ref_position_a".into(), json!(POSITION_ALIGNED));
            for (idx, load_id) in load_ids.iter().enumerate() {
                // `source_latent` is a required input even on the pixel path,
                // which re-encodes `source_image` at the target size.
                let encode_id = next_id.to_string();
                workflow.insert(
                    encode_id.clone(),
                    json!({
                        "class_type": "VAEEncode",
                        "inputs": {
                            "pixels": [load_id.clone(), 0],
                            "vae": [vae_source.0.clone(), vae_source.1]
                        }
                    }),
                );
                next_id += 1;
                let suffix = if idx == 0 { "" } else { "_b" };
                patch.insert(format!("source_latent{suffix}"), json!([encode_id, 0]));
                patch.insert(format!("source_image{suffix}"), json!([load_id.clone(), 0]));
            }
            let patch_id = next_id.to_string();
            workflow.insert(
                patch_id.clone(),
                json!({ "class_type": "Krea2EditModelPatch", "inputs": patch }),
            );
            next_id += 1;
            model_source = (patch_id, 0);

            let grounded = |workflow: &mut serde_json::Map<String, serde_json::Value>,
                            id: u32,
                            prompt: String| {
                let mut inputs = serde_json::Map::new();
                inputs.insert("clip".into(), json!([clip_source.0.clone(), clip_source.1]));
                inputs.insert("prompt".into(), json!(prompt));
                inputs.insert("grounding_px".into(), json!(GROUNDING_PX));
                for (idx, load_id) in load_ids.iter().enumerate() {
                    let key = if idx == 0 { "image" } else { "image_b" };
                    inputs.insert(key.into(), json!([load_id.clone(), 0]));
                }
                workflow.insert(
                    id.to_string(),
                    json!({ "class_type": "Krea2EditGroundedEncode", "inputs": inputs }),
                );
                (id.to_string(), 0u32)
            };
            let pos = grounded(
                &mut workflow,
                next_id,
                strip_lora_tags(&params.positive_prompt),
            );
            next_id += 1;
            // Above CFG 1 the trained unconditional is an empty instruction
            // grounded on the same images (model card: "ground the negative
            // too"). At CFG 1 the sampler never evaluates it.
            let neg = if params.cfg > 1.0 {
                let neg = grounded(&mut workflow, next_id, String::new());
                next_id += 1;
                neg
            } else {
                let id = next_id.to_string();
                workflow.insert(
                    id.clone(),
                    json!({
                        "class_type": "ConditioningZeroOut",
                        "inputs": { "conditioning": [pos.0.clone(), pos.1] }
                    }),
                );
                next_id += 1;
                (id, 0)
            };
            (pos, neg)
        }
        Method::Style => {
            let encode = |workflow: &mut serde_json::Map<String, serde_json::Value>,
                          id: u32,
                          prompt: &str| {
                let mut inputs = serde_json::Map::new();
                inputs.insert("clip".into(), json!([clip_source.0.clone(), clip_source.1]));
                inputs.insert("prompt".into(), json!(strip_lora_tags(prompt)));
                inputs.insert("vae".into(), json!([vae_source.0.clone(), vae_source.1]));
                for (idx, load_id) in load_ids.iter().enumerate() {
                    inputs.insert(format!("image{}", idx + 1), json!([load_id.clone(), 0]));
                }
                workflow.insert(
                    id.to_string(),
                    json!({ "class_type": "TextEncodeQwenImageEditPlus", "inputs": inputs }),
                );
                workflow.insert(
                    (id + 1).to_string(),
                    json!({
                        "class_type": "FluxKontextMultiReferenceLatentMethod",
                        "inputs": {
                            "conditioning": [id.to_string(), 0],
                            "reference_latents_method": "index_timestep_zero"
                        }
                    }),
                );
                ((id + 1).to_string(), 0u32)
            };
            let pos = encode(&mut workflow, next_id, &params.positive_prompt);
            next_id += 2;
            let neg = if params.cfg > 1.0 {
                let neg = encode(&mut workflow, next_id, &params.negative_prompt);
                next_id += 2;
                neg
            } else {
                let id = next_id.to_string();
                workflow.insert(
                    id.clone(),
                    json!({
                        "class_type": "ConditioningZeroOut",
                        "inputs": { "conditioning": [pos.0.clone(), pos.1] }
                    }),
                );
                next_id += 1;
                (id, 0)
            };

            // The bundled template's resolution-scaled shift (1.15 at 1 MP,
            // Krea 2's own default there).
            let shift_id = next_id.to_string();
            workflow.insert(
                shift_id.clone(),
                json!({
                    "class_type": "ModelSamplingFlux",
                    "inputs": {
                        "model": [model_source.0.clone(), model_source.1],
                        "max_shift": 1.15,
                        "base_shift": 0.5,
                        "width": params.width,
                        "height": params.height
                    }
                }),
            );
            next_id += 1;
            model_source = (shift_id, 0);
            (pos, neg)
        }
    };

    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [latent_id, 0],
                "seed": seed,
                "steps": params.steps,
                "cfg": params.cfg,
                "sampler_name": params.sampler_name,
                "scheduler": params.scheduler,
                "denoise": 1.0
            }
        }),
    );
    next_id += 1;

    let (decode_id, next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        model_source,
        clip_source,
        positive_source: pos_source,
        negative_source: neg_source,
        vae_source,
        sampler_id,
        // Upscale and face-fix passes run at other sizes on plain Krea 2: the
        // reference patch and the edit LoRA belong to the base pass only.
        refiner_model_source: Some(ml.model_source),
        base_sources: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(method: &str, refs: usize, cfg: f64) -> GenerationParams {
        serde_json::from_value(serde_json::json!({
            "mode": "image_edit",
            "model_architecture": "krea2",
            "positive_prompt": "put her on a misty ridge in hiking boots <lora:extra:0.5>",
            "negative_prompt": "blurry",
            "checkpoint": "",
            "use_split_model": true,
            "diffusion_model": "krea2_turbo_int8_convrot.safetensors",
            "clip_model": "qwen3vl_4b_fp8_scaled.safetensors",
            "clip_type": "krea2",
            "vae": "qwen_image_vae.safetensors",
            "loras": [],
            "sampler_name": "euler",
            "scheduler": "simple",
            "steps": 8,
            "cfg": cfg,
            "seed": "42",
            "width": 1024,
            "height": 1024,
            "batch_size": 1,
            "denoise": 1.0,
            "upscale_enabled": false,
            "upscale_method": "algorithmic",
            "upscale_scale": 2.0,
            "upscale_denoise": 0.4,
            "upscale_steps": 20,
            "upscale_tile_size": 1024,
            "upscale_tiling": false,
            "edit_reference_images": (0..refs).map(|i| format!("ref{i}.png")).collect::<Vec<_>>(),
            "krea2_edit_method": method,
            "krea2_edit_lora": "krea2_identity_edit_v2_r128.safetensors",
        }))
        .expect("params")
    }

    fn nodes<'a>(result: &'a WorkflowResult, class: &str) -> Vec<&'a serde_json::Value> {
        result
            .workflow
            .values()
            .filter(|n| n["class_type"] == class)
            .collect()
    }

    fn link(value: &serde_json::Value) -> String {
        value[0].as_str().expect("link").to_string()
    }

    #[test]
    fn identity_edit_patches_the_model_and_grounds_the_prompt() {
        let mut p = params("identity", 1, 1.0);
        p.krea2_edit_beside = true;
        let r = build(&p, 7);

        let lora = nodes(&r, "LoraLoaderModelOnly");
        assert_eq!(lora.len(), 1);
        assert_eq!(
            lora[0]["inputs"]["lora_name"],
            "krea2_identity_edit_v2_r128.safetensors"
        );

        let patch = nodes(&r, "Krea2EditModelPatch");
        assert_eq!(patch.len(), 1);
        let pi = &patch[0]["inputs"];
        assert_eq!(pi["ref_boost"], 2.0);
        assert_eq!(pi["ref_position"], POSITION_BESIDE);
        assert_eq!(pi["fit_mode"], "fit");
        assert!(pi.get("source_latent_b").is_none());
        assert_eq!(
            r.workflow[&link(&pi["source_latent"])]["class_type"],
            "VAEEncode"
        );
        assert_eq!(
            r.workflow[&link(&pi["source_image"])]["class_type"],
            "LoadImage"
        );
        // The patch sees the same latent the sampler starts from.
        let sampler = &r.workflow[&r.sampler_id]["inputs"];
        assert_eq!(pi["target_latent"], sampler["latent_image"]);
        assert_eq!(link(&sampler["model"]), r.model_source.0);
        assert_eq!(
            r.workflow[&r.model_source.0]["class_type"],
            "Krea2EditModelPatch"
        );

        let enc = nodes(&r, "Krea2EditGroundedEncode");
        assert_eq!(enc.len(), 1, "CFG 1 needs no grounded negative");
        assert_eq!(enc[0]["inputs"]["grounding_px"], 768);
        assert_eq!(
            enc[0]["inputs"]["prompt"],
            "put her on a misty ridge in hiking boots"
        );
        assert_eq!(
            r.workflow[&r.negative_source.0]["class_type"],
            "ConditioningZeroOut"
        );

        // Refiners skip the patch and the edit LoRA.
        let refiner = r.refiner_model_source.clone().expect("refiner");
        assert_eq!(r.workflow[&refiner.0]["class_type"], "UNETLoader");
    }

    #[test]
    fn two_references_put_the_person_second_and_keep_the_scene_aligned() {
        let r = build(&params("general", 3, 3.0), 7);
        let pi = &nodes(&r, "Krea2EditModelPatch")[0]["inputs"];
        assert_eq!(pi["ref_boost"], 4.0);
        assert_eq!(pi["ref_boost_a"], 1.0);
        assert_eq!(pi["ref_position"], POSITION_ALIGNED);
        assert_eq!(pi["ref_position_a"], POSITION_ALIGNED);
        let scene = link(&pi["source_image"]);
        let person = link(&pi["source_image_b"]);
        assert_eq!(r.workflow[&scene]["inputs"]["image"], "ref0.png");
        assert_eq!(r.workflow[&person]["inputs"]["image"], "ref1.png");
        assert_eq!(nodes(&r, "LoadImage").len(), 2, "capped at two references");

        // CFG 3: the negative is an empty instruction grounded on both images.
        let neg = &r.workflow[&r.negative_source.0];
        assert_eq!(neg["class_type"], "Krea2EditGroundedEncode");
        assert_eq!(neg["inputs"]["prompt"], "");
        assert_eq!(link(&neg["inputs"]["image_b"]), person);
    }

    #[test]
    fn ref_boost_override_is_bounded() {
        let mut p = params("identity", 1, 1.0);
        p.krea2_edit_ref_boost = Some(3.0);
        let r = build(&p, 1);
        assert_eq!(
            nodes(&r, "Krea2EditModelPatch")[0]["inputs"]["ref_boost"],
            3.0
        );
        p.krea2_edit_ref_boost = Some(500.0);
        let r = build(&p, 1);
        assert_eq!(
            nodes(&r, "Krea2EditModelPatch")[0]["inputs"]["ref_boost"],
            8.0
        );
        p.krea2_edit_ref_boost = Some(f64::NAN);
        let r = build(&p, 1);
        assert_eq!(
            nodes(&r, "Krea2EditModelPatch")[0]["inputs"]["ref_boost"],
            2.0
        );
    }

    #[test]
    fn style_reference_uses_core_nodes_with_the_reference_method_set() {
        let r = build(&params("style", 2, 1.0), 7);
        assert!(nodes(&r, "Krea2EditModelPatch").is_empty());
        assert!(nodes(&r, "Krea2EditGroundedEncode").is_empty());
        let enc = nodes(&r, "TextEncodeQwenImageEditPlus");
        assert_eq!(enc.len(), 1);
        assert!(enc[0]["inputs"].get("image2").is_some());
        assert!(enc[0]["inputs"].get("image3").is_none());
        let pos = &r.workflow[&r.positive_source.0];
        assert_eq!(pos["class_type"], "FluxKontextMultiReferenceLatentMethod");
        assert_eq!(
            pos["inputs"]["reference_latents_method"],
            "index_timestep_zero"
        );
        assert_eq!(
            r.workflow[&r.negative_source.0]["class_type"],
            "ConditioningZeroOut"
        );
        assert_eq!(
            r.workflow[&r.model_source.0]["class_type"],
            "ModelSamplingFlux"
        );
    }

    #[test]
    fn style_reference_above_cfg_one_encodes_the_negative_prompt() {
        let r = build(&params("style", 1, 4.0), 7);
        let enc = nodes(&r, "TextEncodeQwenImageEditPlus");
        assert_eq!(enc.len(), 2);
        let neg = &r.workflow[&r.negative_source.0];
        assert_eq!(neg["class_type"], "FluxKontextMultiReferenceLatentMethod");
        let neg_text = &r.workflow[&link(&neg["inputs"]["conditioning"])];
        assert_eq!(neg_text["inputs"]["prompt"], "blurry");
    }

    #[test]
    fn only_identity_and_general_edit_need_the_pack() {
        assert_eq!(required_node_classes(&params("identity", 1, 1.0)).len(), 2);
        assert_eq!(required_node_classes(&params("general", 1, 1.0)).len(), 2);
        assert!(required_node_classes(&params("style", 1, 1.0)).is_empty());
        let mut txt = params("identity", 1, 1.0);
        txt.mode = "txt2img".into();
        assert!(required_node_classes(&txt).is_empty());
    }

    #[test]
    fn unknown_method_falls_back_to_general_edit() {
        assert_eq!(Method::parse(""), Method::General);
        assert_eq!(Method::parse("bogus"), Method::General);
        assert_eq!(Method::parse("identity"), Method::Identity);
        assert_eq!(Method::parse("style"), Method::Style);
    }
}
