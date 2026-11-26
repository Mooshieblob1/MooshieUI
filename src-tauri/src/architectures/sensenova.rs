//! SenseNova U1.6 (8B MoT, ComfyUI core since v0.46.0).
//!
//! Unified 8B text-and-image checkpoint (`CheckpointLoaderSimple`) that runs
//! txt2img and image editing (up to 16 references via `HiDreamO1ReferenceImages`)
//! in pixel-space 3-channel latents (`EmptyHiDreamO1LatentImage`, step 32).
//!
//! When `params.self_correction` is enabled, builds an unrolled 5-pass lazy
//! review-and-refine graph using `SenseNovaConditioning(task = "self-correction")`
//! and `ComfySwitchNode`, decoding the final refined latent with `VAEDecode`.

use serde_json::{json, Map, Value};

use super::{
    ArchitectureSpec, Detect, FamilySpec, ModeSpec, ModelFile, PromptStyle, RecommendedModel,
    Sampling,
};
use crate::comfyui::types::GenerationParams;
use crate::templates::{insert_vae_decode, load_model_nodes, strip_lora_tags, WorkflowResult};

pub const CANVAS_MULTIPLE: u32 = 32;
pub const MAX_REFERENCES: usize = 16;
pub const SELF_CORRECTION_MAX_PASSES: usize = 5;

const EDIT_MIN_MEGAPIXELS: f64 = 1.0;
const EDIT_MAX_MEGAPIXELS: f64 = 4.2;

const HF_REPO_PIN: &str =
    "https://huggingface.co/Comfy-Org/SenseNova-U1/resolve/1945bcc8fbd56b9ed09aee3e207ea269ad64858d";

const NAME_MARKERS: &[&[&str]] = &[
    &["sensenova_u1.6"],
    &["sensenova-u1.6"],
    &["sensenova u1.6"],
    &["sensenova_u16"],
    &["sensenova-u16"],
    &["sensenova u16"],
];

const NOT_A_MODEL: &[&str] = &["lora", "_pe", "-pe", "vae", "clip"];

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "sensenova_u16",
    label: "SenseNova U1.6",
    families: &[FamilySpec {
        family: "sensenova_u16",
        label: "SenseNova U1.6",
        detect: Detect {
            all_of: NAME_MARKERS,
            none_of: NOT_A_MODEL,
            base_models: &["sensenova u1.6", "sensenova-u1.6", "sensenova u1"],
        },
        turbo: false,
        sampling: Some(Sampling {
            steps: 50,
            cfg: 4.0,
            sampler: "euler",
            scheduler: "simple",
            width: 2048,
            height: 2048,
        }),
        recommended: &[
            RecommendedModel {
                label: "SenseNova U1.6 (8B MoT INT8)",
                size: "~17.6 GB",
                diffusion_model: ModelFile {
                    filename: "sensenova_u1.6_8b_mot_int8_convrot.safetensors",
                    url: "https://huggingface.co/Comfy-Org/SenseNova-U1/resolve/1945bcc8fbd56b9ed09aee3e207ea269ad64858d/checkpoints/sensenova_u1.6_8b_mot_int8_convrot.safetensors",
                    category: "checkpoints",
                    size: Some("~17.6 GB"),
                    sha256: Some(
                        "f6e5dc3bfb2023c3d009a8134b8dea8481551afb21b4a03b0e40ce7c651b6f78",
                    ),
                },
                min_compute_capability: None,
            },
            RecommendedModel {
                label: "SenseNova U1.6 (8B MoT BF16)",
                size: "~35.1 GB",
                diffusion_model: ModelFile {
                    filename: "sensenova_u1.6_8b_mot_bf16.safetensors",
                    url: "https://huggingface.co/Comfy-Org/SenseNova-U1/resolve/1945bcc8fbd56b9ed09aee3e207ea269ad64858d/checkpoints/sensenova_u1.6_8b_mot_bf16.safetensors",
                    category: "checkpoints",
                    size: Some("~35.1 GB"),
                    sha256: Some(
                        "196861c46c63b1461dfb132ec8010d79ac80fe28e729cc5cceab7a282bb6af88",
                    ),
                },
                min_compute_capability: None,
            },
        ],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    split_only: false,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("sensenova_u16"),
    text_encoder: None,
    vae: None,
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &[
        "SenseNovaConditioning",
        "EmptyHiDreamO1LatentImage",
        "HiDreamO1ReferenceImages",
        "ComfySwitchNode",
    ],
    min_comfyui: Some("0.46.0"),
    int8_fast_model_type: None,
    modes: &[
        ModeSpec {
            mode: "txt2img",
            max_references: 0,
            required_files: &[],
            sampling: None,
            build: Some(build_txt2img),
        },
        ModeSpec {
            mode: "image_edit",
            max_references: MAX_REFERENCES as u32,
            required_files: &[],
            sampling: None,
            build: Some(build_edit),
        },
    ],
    negative_prompt: true,
    resolution_multiple: CANVAS_MULTIPLE,
    controlnet: false,
    loader: None,
    text_encode: None,
};

#[allow(dead_code)]
pub fn pinned_repo_base() -> &'static str {
    HF_REPO_PIN
}

fn take_id(next_id: &mut u32) -> String {
    let id = next_id.to_string();
    *next_id += 1;
    id
}

/// Round to a multiple of 32 within `EmptyHiDreamO1LatentImage` bounds (`64..=4096`).
pub fn snap_side(side: u32) -> u32 {
    (((side.max(CANVAS_MULTIPLE) + CANVAS_MULTIPLE / 2) / CANVAS_MULTIPLE) * CANVAS_MULTIPLE)
        .clamp(64, 4096)
}

pub fn self_correction_active(params: &GenerationParams) -> bool {
    params.self_correction && params.model_architecture == "sensenova_u16" && params.stage.is_none()
}

/// Keep every pass's `KSampler` pointing at the final patched `model_source`
/// (after Smart Guidance or post-build hooks run in `templates::mod`).
pub fn sync_self_correction_samplers(result: &mut WorkflowResult) {
    let model = json!([result.model_source.0, result.model_source.1]);
    for node in result.workflow.values_mut() {
        if node.get("class_type").and_then(Value::as_str) == Some("KSampler") {
            if let Some(inputs) = node.get_mut("inputs").and_then(Value::as_object_mut) {
                inputs.insert("model".into(), model.clone());
            }
        }
    }
}

fn edit_megapixels(params: &GenerationParams) -> f64 {
    let area = f64::from(params.width) * f64::from(params.height) / (1024.0 * 1024.0);
    let mp = if area > 0.0 { area } else { 4.0 };
    (mp.clamp(EDIT_MIN_MEGAPIXELS, EDIT_MAX_MEGAPIXELS) * 100.0).round() / 100.0
}

fn references(params: &GenerationParams) -> Vec<&str> {
    let mut refs: Vec<&str> = params
        .edit_reference_images
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if refs.is_empty() {
        if let Some(input) = params.input_image.as_deref().map(str::trim) {
            if !input.is_empty() {
                refs.push(input);
            }
        }
    }
    refs.truncate(MAX_REFERENCES);
    refs
}

fn load_references(
    workflow: &mut Map<String, Value>,
    next_id: &mut u32,
    refs: &[&str],
    megapixels: f64,
) -> (Vec<(String, u32)>, String) {
    let mut links = Vec::new();
    let mut size_id = String::new();
    for (index, image) in refs.iter().enumerate() {
        let load_id = take_id(next_id);
        workflow.insert(
            load_id.clone(),
            json!({ "class_type": "LoadImage", "inputs": { "image": image } }),
        );
        if index == 0 {
            let scale_id = take_id(next_id);
            workflow.insert(
                scale_id.clone(),
                json!({
                    "class_type": "ImageScaleToTotalPixels",
                    "inputs": {
                        "image": [load_id, 0],
                        "upscale_method": "area",
                        "megapixels": megapixels,
                        "resolution_steps": CANVAS_MULTIPLE
                    }
                }),
            );
            size_id = take_id(next_id);
            workflow.insert(
                size_id.clone(),
                json!({ "class_type": "GetImageSize", "inputs": { "image": [scale_id, 0] } }),
            );
            links.push((scale_id, 0));
        } else {
            links.push((load_id, 0));
        }
    }
    (links, size_id)
}

fn build_txt2img(params: &GenerationParams, seed: i64) -> WorkflowResult {
    build_graph(params, seed, false)
}

fn build_edit(params: &GenerationParams, seed: i64) -> WorkflowResult {
    build_graph(params, seed, true)
}

fn build_graph(params: &GenerationParams, seed: i64, is_edit: bool) -> WorkflowResult {
    let mut workflow: Map<String, Value> = Map::new();
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    let raw_pos_id = take_id(&mut next_id);
    workflow.insert(
        raw_pos_id.clone(),
        json!({
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": [clip_source.0, clip_source.1],
                "text": strip_lora_tags(&params.positive_prompt)
            }
        }),
    );

    let raw_neg_id = take_id(&mut next_id);
    workflow.insert(
        raw_neg_id.clone(),
        json!({
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": [clip_source.0, clip_source.1],
                "text": strip_lora_tags(&params.negative_prompt)
            }
        }),
    );

    let (cond_pos, cond_neg, size_id) = if is_edit {
        let refs = references(params);
        let (images, size_id) =
            load_references(&mut workflow, &mut next_id, &refs, edit_megapixels(params));
        let ref_node_id = take_id(&mut next_id);
        let mut inputs = Map::new();
        inputs.insert("positive".into(), json!([raw_pos_id, 0]));
        inputs.insert("negative".into(), json!([raw_neg_id, 0]));
        for (index, img) in images.iter().enumerate() {
            inputs.insert(format!("images.image_{}", index + 1), json!([img.0, img.1]));
        }
        workflow.insert(
            ref_node_id.clone(),
            json!({ "class_type": "HiDreamO1ReferenceImages", "inputs": Value::Object(inputs) }),
        );
        ((ref_node_id.clone(), 0), (ref_node_id, 1), size_id)
    } else {
        ((raw_pos_id, 0), (raw_neg_id, 0), String::new())
    };

    let (width, height) = if size_id.is_empty() {
        (
            json!(snap_side(params.width)),
            json!(snap_side(params.height)),
        )
    } else {
        (json!([size_id, 0]), json!([size_id, 1]))
    };

    let empty_latent_id = take_id(&mut next_id);
    workflow.insert(
        empty_latent_id.clone(),
        json!({
            "class_type": "EmptyHiDreamO1LatentImage",
            "inputs": {
                "width": width,
                "height": height,
                "batch_size": params.batch_size.max(1)
            }
        }),
    );

    if params.self_correction {
        let mut prev_sc_id = String::new();
        let mut prev_latent_id = empty_latent_id.clone();
        let mut first_sampler_id = String::new();

        for pass in 0..SELF_CORRECTION_MAX_PASSES {
            let sc_id = take_id(&mut next_id);
            let mut sc_inputs = Map::new();
            sc_inputs.insert("clip".into(), json!([clip_source.0, clip_source.1]));
            if pass == 0 {
                sc_inputs.insert("conditioning".into(), json!([cond_pos.0, cond_pos.1]));
                sc_inputs.insert("negative".into(), json!([cond_neg.0, cond_neg.1]));
            } else {
                sc_inputs.insert("conditioning".into(), json!([prev_sc_id, 0]));
                sc_inputs.insert("negative".into(), json!([prev_sc_id, 2]));
                sc_inputs.insert("task.samples".into(), json!([prev_latent_id, 0]));
            }
            sc_inputs.insert("task".into(), json!("self-correction"));
            sc_inputs.insert("max_tokens".into(), json!(1024));
            workflow.insert(
                sc_id.clone(),
                json!({ "class_type": "SenseNovaConditioning", "inputs": Value::Object(sc_inputs) }),
            );

            let ks_id = take_id(&mut next_id);
            let pass_seed = seed.saturating_add(pass as i64);
            workflow.insert(
                ks_id.clone(),
                json!({
                    "class_type": "KSampler",
                    "inputs": {
                        "model": [model_source.0, model_source.1],
                        "positive": [sc_id, 0],
                        "negative": [sc_id, 2],
                        "latent_image": [empty_latent_id, 0],
                        "seed": pass_seed,
                        "steps": params.steps,
                        "cfg": params.cfg,
                        "sampler_name": params.sampler_name,
                        "scheduler": params.scheduler,
                        "denoise": 1.0
                    }
                }),
            );
            if pass == 0 {
                first_sampler_id = ks_id.clone();
            }

            let switch_id = take_id(&mut next_id);
            workflow.insert(
                switch_id.clone(),
                json!({
                    "class_type": "ComfySwitchNode",
                    "inputs": {
                        "switch": [sc_id, 3],
                        "on_false": [prev_latent_id, 0],
                        "on_true": [ks_id, 0]
                    }
                }),
            );

            prev_sc_id = sc_id;
            prev_latent_id = switch_id;
        }

        let (decode_id, next_id) =
            insert_vae_decode(&mut workflow, next_id, &prev_latent_id, &vae_source, params);

        WorkflowResult {
            workflow,
            next_id,
            image_output: (decode_id, 0),
            model_source,
            clip_source,
            positive_source: cond_pos,
            negative_source: cond_neg,
            vae_source,
            sampler_id: first_sampler_id,
            refiner_model_source: None,
            base_sources: None,
        }
    } else {
        let sc_id = take_id(&mut next_id);
        workflow.insert(
            sc_id.clone(),
            json!({
                "class_type": "SenseNovaConditioning",
                "inputs": {
                    "clip": [clip_source.0, clip_source.1],
                    "conditioning": [cond_pos.0, cond_pos.1],
                    "negative": [cond_neg.0, cond_neg.1],
                    "task": "image",
                    "max_tokens": 1024
                }
            }),
        );

        let sampler_id = take_id(&mut next_id);
        workflow.insert(
            sampler_id.clone(),
            json!({
                "class_type": "KSampler",
                "inputs": {
                    "model": [model_source.0, model_source.1],
                    "positive": [sc_id, 0],
                    "negative": [sc_id, 2],
                    "latent_image": [empty_latent_id, 0],
                    "seed": seed,
                    "steps": params.steps,
                    "cfg": params.cfg,
                    "sampler_name": params.sampler_name,
                    "scheduler": params.scheduler,
                    "denoise": 1.0
                }
            }),
        );

        let (decode_id, next_id) =
            insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

        WorkflowResult {
            workflow,
            next_id,
            image_output: (decode_id, 0),
            model_source,
            clip_source,
            positive_source: (sc_id.clone(), 0),
            negative_source: (sc_id, 2),
            vae_source,
            sampler_id,
            refiner_model_source: None,
            base_sources: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architectures;

    fn base_params(mode: &str) -> GenerationParams {
        GenerationParams {
            mode: mode.into(),
            checkpoint: "sensenova_u1.6_8b_mot_int8_convrot.safetensors".into(),
            model_architecture: "sensenova_u16".into(),
            positive_prompt: "a botanical illustration of a fern".into(),
            negative_prompt: "blurry".into(),
            steps: 50,
            cfg: 4.0,
            sampler_name: "euler".into(),
            scheduler: "simple".into(),
            width: 2048,
            height: 2048,
            ..Default::default()
        }
    }

    #[test]
    fn resolves_family_from_tensor_architecture_and_snaps_dimensions() {
        assert_eq!(
            architectures::family_from_tensor_architecture("sensenova_u16"),
            Some("sensenova_u16")
        );
        assert_eq!(snap_side(1024), 1024);
        assert_eq!(snap_side(1000), 992);
        assert_eq!(snap_side(16), 64);
        assert_eq!(snap_side(5000), 4096);
    }

    #[test]
    fn txt2img_standard_graph_uses_image_conditioning_and_latent() {
        let params = base_params("txt2img");
        let res = build_txt2img(&params, 42);
        let nodes = &res.workflow;
        assert!(nodes
            .values()
            .any(|n| n["class_type"] == "EmptyHiDreamO1LatentImage"));
        let sc: Vec<_> = nodes
            .values()
            .filter(|n| n["class_type"] == "SenseNovaConditioning")
            .collect();
        assert_eq!(sc.len(), 1);
        assert_eq!(sc[0]["inputs"]["task"], "image");
        assert!(sc[0]["inputs"].get("task.samples").is_none());
        assert_eq!(nodes[&res.sampler_id]["inputs"]["seed"], 42);
    }

    #[test]
    fn image_edit_graph_wires_hidream_references() {
        let mut params = base_params("image_edit");
        params.edit_reference_images = vec!["first.png".into(), "second.png".into()];
        let res = build_edit(&params, 7);
        let ref_nodes: Vec<_> = res
            .workflow
            .values()
            .filter(|n| n["class_type"] == "HiDreamO1ReferenceImages")
            .collect();
        assert_eq!(ref_nodes.len(), 1);
        assert!(ref_nodes[0]["inputs"].get("images.image_1").is_some());
        assert!(ref_nodes[0]["inputs"].get("images.image_2").is_some());
    }

    #[test]
    fn self_correction_unrolls_five_passes_with_samples_only_after_pass_one() {
        let mut params = base_params("txt2img");
        params.self_correction = true;
        let mut res = build_txt2img(&params, 100);
        let mut sc_nodes: Vec<(u32, &Value)> = res
            .workflow
            .iter()
            .filter(|(_, n)| n["class_type"] == "SenseNovaConditioning")
            .map(|(k, v)| (k.parse::<u32>().unwrap(), v))
            .collect();
        sc_nodes.sort_by_key(|(k, _)| *k);
        assert_eq!(sc_nodes.len(), SELF_CORRECTION_MAX_PASSES);
        for (idx, (_, node)) in sc_nodes.iter().enumerate() {
            assert_eq!(node["inputs"]["task"], "self-correction");
            if idx == 0 {
                assert!(node["inputs"].get("task.samples").is_none());
            } else {
                assert!(node["inputs"].get("task.samples").is_some());
            }
        }
        let switches = res
            .workflow
            .values()
            .filter(|n| n["class_type"] == "ComfySwitchNode")
            .count();
        assert_eq!(switches, SELF_CORRECTION_MAX_PASSES);

        // Model patch propagation reaches all samplers.
        res.model_source = ("999".into(), 0);
        sync_self_correction_samplers(&mut res);
        for node in res
            .workflow
            .values()
            .filter(|n| n["class_type"] == "KSampler")
        {
            assert_eq!(node["inputs"]["model"], json!(["999", 0]));
        }
    }
}
