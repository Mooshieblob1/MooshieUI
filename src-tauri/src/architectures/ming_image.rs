//! Ming-Image 0.1 Design (inclusionAI / Ant Group, MIT, ComfyUI core since
//! v0.38.0).
//!
//! A 6B DiT for design work (posters, UI screens, infographics, text-heavy
//! layouts) on a Ling-mini 2.0 MoE text encoder and a 4-channel (RGBA)
//! Wan 2.1-layout VAE, so it can return real transparent backgrounds. Two
//! checkpoints, registered as two architectures because they need different
//! text encoders and modes:
//!
//! - **Design** ([`SPEC`]): text to image, plus editing with up to 8
//!   references through core `TextEncodeMingImageEdit`.
//! - **Design-Layer** ([`LAYER_SPEC`]): splits one flattened design into N
//!   transparent layers. It runs on the Image Edit page: the input image plus a
//!   "Decompose this image into N layers" prompt.
//!
//! Graphs follow Comfy-Org's `image_ming_image_01_design_{t2i,image_edit}`
//! templates: 12 steps at CFG 1 (euler, simple) on `ModelSamplingFlux` with
//! max shift 1.15 / base shift 0.5. The layer graph has no Comfy template and
//! is built from ComfyUI's Ming code and inclusionAI's reference CLI: CFG 2,
//! a latent with one frame per layer plus a leading composite, each frame
//! decoded on its own.

use serde_json::{json, Map, Value};

use super::{
    ArchitectureSpec, Detect, EncoderSpec, FamilySpec, ModeSpec, ModelFile, PromptStyle,
    RecommendedModel, Sampling, VaeSpec,
};
use crate::comfyui::types::GenerationParams;
use crate::templates::{
    img2img, inpainting, insert_vae_decode, load_model_nodes, strip_lora_tags, txt2img,
    WorkflowResult,
};

/// References `TextEncodeMingImageEdit` takes (`image_1` .. `image_8`).
pub const MAX_REFERENCES: usize = 8;

/// `ModelSamplingFlux` shifts from Comfy-Org's Ming templates.
const MAX_SHIFT: f64 = 1.15;
const BASE_SHIFT: f64 = 0.5;

/// The model works on 16 px steps (8x VAE, 2x2 patches).
const SIZE_STEP: u32 = 16;

/// The fixed phrase inclusionAI's model card gives for a transparent
/// background, put in front of the prompt when the toggle is on.
pub const TRANSPARENT_PHRASE: &str = "RGBA, 4-channel, transparent background";

/// Layers asked for when the prompt does not say how many.
pub const DEFAULT_LAYERS: u32 = 4;
/// Upper bound on layers; each is a full latent frame.
pub const MAX_LAYERS: u32 = 16;

/// Working size of a decomposition (the 1024 bucket the model card
/// recommends); the input's aspect ratio is kept.
const LAYER_MEGAPIXELS: f64 = 1.048_576;

const VAE: ModelFile = ModelFile {
    filename: "ming_image_vae_bf16.safetensors",
    url: "https://huggingface.co/Comfy-Org/Ming-Image/resolve/main/vae/ming_image_vae_bf16.safetensors",
    category: "vae",
    size: Some("~0.25 GB"),
    sha256: Some("7f5bed402dc8c77dc2e0ab1929a85d4df433b7cf7b599dfa8c353da98db0b90a"),
};

const DESIGN_ENCODER: ModelFile = ModelFile {
    filename: "ming_image_0.1_ling_mini_2.0_w4a8.safetensors",
    url: "https://huggingface.co/Comfy-Org/Ming-Image/resolve/main/text_encoders/ming_image_0.1_ling_mini_2.0_w4a8.safetensors",
    category: "text_encoders",
    size: Some("~12.8 GB"),
    sha256: Some("91de4cd0718bec1452b74ff3b0df0dfde280493dab384f0eb4007c18bf932ccd"),
};

const LAYER_ENCODER: ModelFile = ModelFile {
    filename: "ming_image_0.1_ling_mini_2.0_layer_int8_convrot.safetensors",
    url: "https://huggingface.co/Comfy-Org/Ming-Image/resolve/main/text_encoders/ming_image_0.1_ling_mini_2.0_layer_int8_convrot.safetensors",
    category: "text_encoders",
    size: Some("~19.5 GB"),
    sha256: Some("086324b766a04844dc918866e48e40b5cede499d869ac3fa05ceaa87ad7c6850"),
};

/// Spellings of "Ming-Image" in Comfy-Org, inclusionAI and community names.
const NAME_MARKERS: &[&[&str]] = &[
    &["ming_image"],
    &["ming-image"],
    &["ming image"],
    &["mingimage"],
];

const LAYER_NAME_MARKERS: &[&[&str]] = &[
    &["ming_image", "layer"],
    &["ming-image", "layer"],
    &["ming image", "layer"],
    &["mingimage", "layer"],
];

/// Not a diffusion model: the encoders and the VAE carry the name too.
const NOT_A_MODEL: &[&str] = &["ling_mini", "ling-mini", "_vae", "-vae", "lora"];
const NOT_DESIGN: &[&str] = &["ling_mini", "ling-mini", "_vae", "-vae", "lora", "layer"];

/// Encoder markers. A Layer encoder is never a Design one and vice versa:
/// the two are fine-tuned apart and fail on the other checkpoint.
const DESIGN_ENCODER_MARKERS: &[&str] = &[
    "ming_image_0.1_ling_mini_2.0_bf16",
    "ming_image_0.1_ling_mini_2.0_int8",
    "ming_image_0.1_ling_mini_2.0_w4a8",
];
const LAYER_ENCODER_MARKERS: &[&str] = &["ling_mini_2.0_layer", "ling-mini-2.0-layer"];

const VAE_SPEC: VaeSpec = VaeSpec {
    basenames: &["ming_image_vae_bf16.safetensors"],
    markers: &["ming_image_vae", "ming-image-vae", "mingimage_vae"],
    strict: true,
    download: Some(VAE),
};

pub static SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "ming_image",
    label: "Ming-Image",
    families: &[FamilySpec {
        family: "mingimage",
        label: "Ming-Image Design",
        detect: Detect {
            all_of: NAME_MARKERS,
            none_of: NOT_DESIGN,
            base_models: &["ming-image", "ming image", "ming-image design"],
        },
        turbo: false,
        sampling: Some(Sampling {
            steps: 12,
            cfg: 1.0,
            sampler: "euler",
            scheduler: "simple",
            width: 2048,
            height: 2048,
        }),
        recommended: &[RecommendedModel {
            label: "Ming-Image 0.1 Design (INT8)",
            size: "~19.3 GB",
            diffusion_model: ModelFile {
                filename: "ming_image_0.1_design_int8_convrot.safetensors",
                url: "https://huggingface.co/Comfy-Org/Ming-Image/resolve/main/diffusion_models/ming_image_0.1_design_int8_convrot.safetensors",
                category: "diffusion_models",
                size: Some("~6.2 GB"),
                sha256: Some("0d3f5bcc6d2cb830578ea6d0925b30b9fb66058bcb651cc8633e60760dceb1d0"),
            },
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("ming_image"),
    text_encoder: Some(EncoderSpec {
        // ComfyUI picks the Ming encoder from the weights; the template loads
        // it with this type.
        clip_type: "qwen_image",
        markers: DESIGN_ENCODER_MARKERS,
        strict: true,
        download: Some(DESIGN_ENCODER),
    }),
    vae: Some(VAE_SPEC),
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &["TextEncodeMingImageEdit", "ModelSamplingFlux"],
    min_comfyui: Some("0.38.0"),
    // ComfyUI loads the INT8 ConvRot files natively.
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
            mode: "img2img",
            max_references: 0,
            required_files: &[],
            sampling: None,
            build: Some(build_img2img),
        },
        ModeSpec {
            mode: "inpainting",
            max_references: 0,
            required_files: &[],
            sampling: None,
            build: Some(build_inpainting),
        },
        ModeSpec {
            mode: "image_edit",
            max_references: MAX_REFERENCES as u32,
            required_files: &[],
            sampling: None,
            build: Some(build_edit),
        },
    ],
    // CFG 1, positive conditioning only.
    negative_prompt: false,
    resolution_multiple: SIZE_STEP,
    loader: None,
    text_encode: None,
};

pub static LAYER_SPEC: ArchitectureSpec = ArchitectureSpec {
    id: "ming_image_layer",
    label: "Ming-Image Layer",
    families: &[FamilySpec {
        family: "mingimagelayer",
        label: "Ming-Image Layer",
        detect: Detect {
            all_of: LAYER_NAME_MARKERS,
            none_of: NOT_A_MODEL,
            base_models: &["ming-image layer", "ming-image design layer"],
        },
        turbo: false,
        sampling: Some(Sampling {
            steps: 12,
            cfg: 2.0,
            sampler: "euler",
            scheduler: "simple",
            width: 1024,
            height: 1024,
        }),
        recommended: &[RecommendedModel {
            label: "Ming-Image 0.1 Design Layer (INT8)",
            size: "~26 GB",
            diffusion_model: ModelFile {
                filename: "ming_image_0.1_design_layer_int8_convrot.safetensors",
                url: "https://huggingface.co/Comfy-Org/Ming-Image/resolve/main/diffusion_models/ming_image_0.1_design_layer_int8_convrot.safetensors",
                category: "diffusion_models",
                size: Some("~6.2 GB"),
                sha256: Some("ef3f306c7981ac477f1fc28693144338ea35fb3f17cb0954c570553f53727a92"),
            },
            min_compute_capability: None,
        }],
        text_encoder: None,
        vae: None,
        manual_sigmas: None,
    }],
    split_only: true,
    prompt_style: PromptStyle::Natural,
    tensor_architecture: Some("ming_image_layer"),
    text_encoder: Some(EncoderSpec {
        clip_type: "qwen_image",
        markers: LAYER_ENCODER_MARKERS,
        strict: true,
        download: Some(LAYER_ENCODER),
    }),
    vae: Some(VAE_SPEC),
    model_slots: &[],
    custom_nodes: &[],
    core_nodes: &[
        "TextEncodeMingImageEdit",
        "ModelSamplingFlux",
        "LatentCutToBatch",
    ],
    min_comfyui: Some("0.38.0"),
    int8_fast_model_type: None,
    modes: &[ModeSpec {
        mode: "image_edit",
        max_references: 1,
        required_files: &[],
        sampling: None,
        build: Some(build_layers),
    }],
    // The unconditional side is the image with an empty prompt.
    negative_prompt: false,
    resolution_multiple: SIZE_STEP,
    loader: None,
    text_encode: None,
};

// ── Shared pieces ───────────────────────────────────────────────────────────

/// The request with the transparent-background phrase in front of the prompt
/// when the toggle is on.
fn with_prompt_prefix(params: &GenerationParams) -> std::borrow::Cow<'_, GenerationParams> {
    if !params.transparent_background {
        return std::borrow::Cow::Borrowed(params);
    }
    let mut p = params.clone();
    p.positive_prompt = prefixed_prompt(&params.positive_prompt);
    std::borrow::Cow::Owned(p)
}

/// `TRANSPARENT_PHRASE` in front of `prompt`, unless the prompt already
/// starts with it.
pub fn prefixed_prompt(prompt: &str) -> String {
    let trimmed = prompt.trim_start();
    if trimmed
        .to_lowercase()
        .starts_with(&TRANSPARENT_PHRASE.to_lowercase())
    {
        return prompt.to_string();
    }
    if trimmed.is_empty() {
        TRANSPARENT_PHRASE.to_string()
    } else {
        format!("{TRANSPARENT_PHRASE}. {trimmed}")
    }
}

fn take_id(next_id: &mut u32) -> String {
    let id = next_id.to_string();
    *next_id += 1;
    id
}

/// Put `ModelSamplingFlux` (Ming's resolution-dependent shift) on the model
/// that `sampler_id` samples with, and make it the refiners' model too.
/// `width` / `height` are numbers or links.
fn patch_model_sampling(result: &mut WorkflowResult, width: Value, height: Value) {
    let id = take_id(&mut result.next_id);
    result.workflow.insert(
        id.clone(),
        json!({"class_type": "ModelSamplingFlux", "inputs": {
            "model": [result.model_source.0, result.model_source.1],
            "max_shift": MAX_SHIFT,
            "base_shift": BASE_SHIFT,
            "width": width,
            "height": height
        }}),
    );
    if let Some(inputs) = result
        .workflow
        .get_mut(&result.sampler_id)
        .and_then(|node| node.get_mut("inputs"))
    {
        inputs["model"] = json!([id, 0]);
    }
    result.model_source = (id, 0);
}

/// True when chains that expect RGB images (upscale models, face fix,
/// segment detail) run after this graph.
fn post_chains_active(params: &GenerationParams) -> bool {
    params.upscale_enabled || params.facefix_enabled || !params.detail_segments.is_empty()
}

/// Ming decodes RGBA. Upscale models and the detailers take RGB only, so when
/// they run the alpha channel is dropped first; otherwise the transparent
/// image goes to the save node as it is.
fn rgb_for_post_chains(result: &mut WorkflowResult, params: &GenerationParams) {
    if !post_chains_active(params) {
        return;
    }
    let id = take_id(&mut result.next_id);
    result.workflow.insert(
        id.clone(),
        json!({"class_type": "SplitImageWithAlpha", "inputs": {
            "image": [result.image_output.0, result.image_output.1]
        }}),
    );
    result.image_output = (id, 0);
}

/// Generic template, then Ming's model sampling and the RGBA handling.
fn finish_generic(mut result: WorkflowResult, params: &GenerationParams) -> WorkflowResult {
    patch_model_sampling(&mut result, json!(params.width), json!(params.height));
    rgb_for_post_chains(&mut result, params);
    result
}

fn build_txt2img(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let p = with_prompt_prefix(params);
    finish_generic(txt2img::build(&p, seed), &p)
}

fn build_img2img(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let p = with_prompt_prefix(params);
    finish_generic(img2img::build(&p, seed), &p)
}

fn build_inpainting(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let p = with_prompt_prefix(params);
    finish_generic(inpainting::build(&p, seed), &p)
}

/// Non-empty reference images, the edit source (slot 0) first.
fn references(params: &GenerationParams, max: usize) -> Vec<&str> {
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
    refs.truncate(max);
    refs
}

/// Megapixels for the edit canvas: the requested size's area, within what
/// the model was trained on (about 0.25 to 4.2 MP).
fn edit_megapixels(params: &GenerationParams) -> f64 {
    let mp = f64::from(params.width) * f64::from(params.height) / 1_000_000.0;
    (mp.clamp(0.25, 4.2) * 100.0).round() / 100.0
}

/// Load each reference, the first scaled to `megapixels` on 16 px steps (the
/// encoder resizes the rest to it). Returns the node links and the first
/// image's `GetImageSize`.
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
            json!({"class_type": "LoadImage", "inputs": {"image": image}}),
        );
        if index == 0 {
            let scale_id = take_id(next_id);
            workflow.insert(
                scale_id.clone(),
                json!({"class_type": "ImageScaleToTotalPixels", "inputs": {
                    "image": [load_id, 0],
                    "upscale_method": "area",
                    "megapixels": megapixels,
                    "resolution_steps": SIZE_STEP
                }}),
            );
            size_id = take_id(next_id);
            workflow.insert(
                size_id.clone(),
                json!({"class_type": "GetImageSize", "inputs": {"image": [scale_id, 0]}}),
            );
            links.push((scale_id, 0));
        } else {
            links.push((load_id, 0));
        }
    }
    (links, size_id)
}

fn edit_encode(
    workflow: &mut Map<String, Value>,
    next_id: &mut u32,
    clip: &(String, u32),
    vae: &(String, u32),
    prompt: &str,
    images: &[(String, u32)],
) -> String {
    let mut inputs = Map::new();
    inputs.insert("clip".into(), json!([clip.0, clip.1]));
    inputs.insert("vae".into(), json!([vae.0, vae.1]));
    inputs.insert("prompt".into(), json!(strip_lora_tags(prompt)));
    for (index, image) in images.iter().enumerate() {
        inputs.insert(
            format!("images.image_{}", index + 1),
            json!([image.0, image.1]),
        );
    }
    let id = take_id(next_id);
    workflow.insert(
        id.clone(),
        json!({"class_type": "TextEncodeMingImageEdit", "inputs": Value::Object(inputs)}),
    );
    id
}

/// Image Edit: the references go into `TextEncodeMingImageEdit` with the VAE
/// (clean reference frames); the latent matches the first reference, scaled
/// to the requested area. No negative: CFG 1, so an empty one stands in.
fn build_edit(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let p = with_prompt_prefix(params);
    let params = p.as_ref();
    let mut workflow: Map<String, Value> = Map::new();
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    let refs = references(params, MAX_REFERENCES);
    let (images, size_id) =
        load_references(&mut workflow, &mut next_id, &refs, edit_megapixels(params));
    let positive_id = edit_encode(
        &mut workflow,
        &mut next_id,
        &clip_source,
        &vae_source,
        &params.positive_prompt,
        &images,
    );
    let negative_id = take_id(&mut next_id);
    workflow.insert(
        negative_id.clone(),
        json!({"class_type": "ConditioningZeroOut", "inputs": {
            "conditioning": [positive_id, 0]
        }}),
    );

    // Without a reference (validation normally stops this) the requested size.
    let (width, height) = if size_id.is_empty() {
        (json!(params.width), json!(params.height))
    } else {
        (json!([size_id, 0]), json!([size_id, 1]))
    };
    let latent_id = take_id(&mut next_id);
    workflow.insert(
        latent_id.clone(),
        json!({"class_type": "EmptyLatentImage", "inputs": {
            "width": width,
            "height": height,
            "batch_size": params.batch_size.max(1)
        }}),
    );

    let sampler_id = take_id(&mut next_id);
    workflow.insert(
        sampler_id.clone(),
        json!({"class_type": "KSampler", "inputs": {
            "model": [model_source.0, model_source.1],
            "positive": [positive_id, 0],
            "negative": [negative_id, 0],
            "latent_image": [latent_id, 0],
            "seed": seed,
            "steps": params.steps,
            "cfg": params.cfg,
            "sampler_name": params.sampler_name,
            "scheduler": params.scheduler,
            "denoise": 1.0
        }}),
    );

    let (decode_id, next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    let mut result = WorkflowResult {
        workflow,
        next_id,
        image_output: (decode_id, 0),
        model_source,
        clip_source,
        positive_source: (positive_id, 0),
        negative_source: (negative_id, 0),
        vae_source,
        sampler_id,
        refiner_model_source: None,
        base_sources: None,
    };
    patch_model_sampling(&mut result, width, height);
    rgb_for_post_chains(&mut result, params);
    result
}

// ── Layer decomposition ─────────────────────────────────────────────────────

/// The layer count a prompt asks for ("Decompose this image into N layers"
/// or "Number of layers: N", as inclusionAI's CLI reads it), clamped to
/// `1..=MAX_LAYERS`.
pub fn requested_layers(prompt: &str) -> Option<u32> {
    let lower = prompt.to_lowercase();
    let number_after = |marker: &str| -> Option<u32> {
        let rest = &lower[lower.find(marker)? + marker.len()..];
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    let before_layers = || -> Option<u32> {
        let at = lower.find("into ")? + "into ".len();
        let digits: String = lower[at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let after = lower[at + digits.len()..].trim_start();
        if after.starts_with("layer") {
            digits.parse().ok()
        } else {
            None
        }
    };
    number_after("number of layers:")
        .or_else(before_layers)
        .map(|n| n.clamp(1, MAX_LAYERS))
}

/// The decomposition prompt: the user's spec, or the CLI's default request
/// in front of what they wrote when it names no count.
pub fn layer_prompt(prompt: &str) -> (String, u32) {
    let text = strip_lora_tags(prompt);
    match requested_layers(&text) {
        Some(n) => (text.trim().to_string(), n),
        None => {
            let request = format!("Decompose this image into {DEFAULT_LAYERS} layers.");
            let trimmed = text.trim();
            if trimmed.is_empty() {
                (request, DEFAULT_LAYERS)
            } else {
                (format!("{request}\n{trimmed}"), DEFAULT_LAYERS)
            }
        }
    }
}

/// Layer decomposition: the input image conditions both sides (the prompt on
/// the positive, an empty prompt on the negative, CFG 2), the latent carries
/// a leading composite frame plus one frame per layer, and every frame is
/// decoded on its own so each layer comes out as its own RGBA image. The
/// composite is dropped, as inclusionAI's CLI does.
fn build_layers(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow: Map<String, Value> = Map::new();
    let ml = load_model_nodes(&mut workflow, 1, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    let refs = references(params, 1);
    let (images, size_id) = load_references(&mut workflow, &mut next_id, &refs, LAYER_MEGAPIXELS);
    let (prompt, layers) = layer_prompt(&params.positive_prompt);
    let positive_id = edit_encode(
        &mut workflow,
        &mut next_id,
        &clip_source,
        &vae_source,
        &prompt,
        &images,
    );
    let negative_id = edit_encode(
        &mut workflow,
        &mut next_id,
        &clip_source,
        &vae_source,
        "",
        &images,
    );

    let (width, height) = if size_id.is_empty() {
        (json!(1024), json!(1024))
    } else {
        (json!([size_id, 0]), json!([size_id, 1]))
    };
    // 16 channels, ((length - 1) / 4) + 1 frames: the composite plus `layers`.
    let latent_id = take_id(&mut next_id);
    workflow.insert(
        latent_id.clone(),
        json!({"class_type": "EmptyHunyuanLatentVideo", "inputs": {
            "width": width,
            "height": height,
            "length": 4 * layers + 1,
            "batch_size": 1
        }}),
    );

    let sampler_id = take_id(&mut next_id);
    workflow.insert(
        sampler_id.clone(),
        json!({"class_type": "KSampler", "inputs": {
            "model": [model_source.0, model_source.1],
            "positive": [positive_id, 0],
            "negative": [negative_id, 0],
            "latent_image": [latent_id, 0],
            "seed": seed,
            "steps": params.steps,
            "cfg": params.cfg,
            "sampler_name": params.sampler_name,
            "scheduler": params.scheduler,
            "denoise": 1.0
        }}),
    );

    // One latent frame per batch entry, so the video VAE decodes each as a
    // still instead of expanding later frames in time.
    let split_id = take_id(&mut next_id);
    workflow.insert(
        split_id.clone(),
        json!({"class_type": "LatentCutToBatch", "inputs": {
            "samples": [sampler_id, 0],
            "dim": "t",
            "slice_size": 1
        }}),
    );
    let decode_id = take_id(&mut next_id);
    workflow.insert(
        decode_id.clone(),
        json!({"class_type": "VAEDecode", "inputs": {
            "samples": [split_id, 0],
            "vae": [vae_source.0, vae_source.1]
        }}),
    );
    let layers_id = take_id(&mut next_id);
    workflow.insert(
        layers_id.clone(),
        json!({"class_type": "ImageFromBatch", "inputs": {
            "image": [decode_id, 0],
            "batch_index": 1,
            "length": layers
        }}),
    );

    let mut result = WorkflowResult {
        workflow,
        next_id,
        image_output: (layers_id, 0),
        model_source,
        clip_source,
        positive_source: (positive_id, 0),
        negative_source: (negative_id, 0),
        vae_source,
        sampler_id,
        refiner_model_source: None,
        base_sources: None,
    };
    patch_model_sampling(&mut result, width, height);
    rgb_for_post_chains(&mut result, params);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architectures;

    fn params(mode: &str, family: &str) -> GenerationParams {
        let layer = family == "mingimagelayer";
        GenerationParams {
            mode: mode.into(),
            model_architecture: family.into(),
            use_split_model: true,
            diffusion_model: Some(if layer {
                "ming_image_0.1_design_layer_int8_convrot.safetensors".into()
            } else {
                "ming_image_0.1_design_int8_convrot.safetensors".into()
            }),
            clip_model: Some(if layer {
                "ming_image_0.1_ling_mini_2.0_layer_int8_convrot.safetensors".into()
            } else {
                "ming_image_0.1_ling_mini_2.0_w4a8.safetensors".into()
            }),
            clip_type: Some("qwen_image".into()),
            vae: Some("ming_image_vae_bf16.safetensors".into()),
            positive_prompt: "A poster that reads \"OPEN LATE\"".into(),
            sampler_name: "euler".into(),
            scheduler: "simple".into(),
            steps: 12,
            cfg: if layer { 2.0 } else { 1.0 },
            width: 2048,
            height: 2048,
            batch_size: 1,
            denoise: 1.0,
            output_format: "png".into(),
            output_bit_depth: "8bit".into(),
            ..Default::default()
        }
    }

    fn nodes<'a>(workflow: &'a Value, class: &str) -> Vec<&'a Value> {
        workflow
            .as_object()
            .unwrap()
            .values()
            .filter(|n| n["class_type"] == class)
            .collect()
    }

    fn node<'a>(workflow: &'a Value, link: &Value) -> &'a Value {
        &workflow[link[0].as_str().unwrap()]
    }

    #[test]
    fn detection_tells_design_from_layer_and_skips_encoders() {
        for (name, family) in [
            (
                "ming_image_0.1_design_int8_convrot.safetensors",
                Some("mingimage"),
            ),
            ("ming_image_0.1_design_bf16.safetensors", Some("mingimage")),
            (
                "ming_image_0.1_design_layer_int8_convrot.safetensors",
                Some("mingimagelayer"),
            ),
            (
                "ming_image_0.1_design_layer_bf16.safetensors",
                Some("mingimagelayer"),
            ),
            ("Ming-Image-0.1-Design-Q8_0.gguf", Some("mingimage")),
            ("Ming-Image", Some("mingimage")),
            ("ming_image_0.1_ling_mini_2.0_w4a8.safetensors", None),
            (
                "ming_image_0.1_ling_mini_2.0_layer_int8_convrot.safetensors",
                None,
            ),
            ("ming_image_vae_bf16.safetensors", None),
            ("z_image_turbo_bf16.safetensors", None),
        ] {
            assert_eq!(architectures::family_from_text(name), family, "{name}");
        }
        assert_eq!(
            architectures::family_from_tensor_architecture("ming_image"),
            Some("mingimage")
        );
        assert_eq!(
            architectures::family_from_tensor_architecture("ming_image_layer"),
            Some("mingimagelayer")
        );
    }

    #[test]
    fn each_checkpoint_pairs_only_its_own_encoder() {
        let encoders = [
            "qwen_3_4b.safetensors".to_string(),
            "ming_image_0.1_ling_mini_2.0_layer_int8_convrot.safetensors".to_string(),
            "ming_image_0.1_ling_mini_2.0_w4a8.safetensors".to_string(),
        ];
        assert_eq!(
            architectures::recommended_clip("mingimage", &encoders),
            Some((
                Some("ming_image_0.1_ling_mini_2.0_w4a8.safetensors".to_string()),
                "qwen_image"
            ))
        );
        assert_eq!(
            architectures::recommended_clip("mingimagelayer", &encoders),
            Some((
                Some("ming_image_0.1_ling_mini_2.0_layer_int8_convrot.safetensors".to_string()),
                "qwen_image"
            ))
        );
        assert_eq!(
            architectures::recommended_clip("mingimage", &encoders[..2]),
            Some((None, "qwen_image"))
        );
        let mut p = params("txt2img", "mingimage");
        p.clip_model = Some(encoders[1].clone());
        assert!(architectures::validate(&p).is_err());
    }

    #[test]
    fn layer_checkpoint_only_decomposes() {
        assert!(architectures::validate(&params("txt2img", "mingimagelayer")).is_err());
        let mut p = params("image_edit", "mingimagelayer");
        p.input_image = Some("card.png".into());
        assert!(architectures::validate(&p).is_ok());
        p.edit_reference_images = vec!["card.png".into(), "other.png".into()];
        assert!(architectures::validate(&p).is_err());
    }

    #[test]
    fn txt2img_samples_on_ming_shift_and_keeps_alpha() {
        let wf = crate::templates::build_workflow(&params("txt2img", "mingimage"), 7, false);
        let shift = nodes(&wf, "ModelSamplingFlux");
        assert_eq!(shift.len(), 1);
        assert_eq!(shift[0]["inputs"]["max_shift"], 1.15);
        assert_eq!(shift[0]["inputs"]["base_shift"], 0.5);
        assert_eq!(shift[0]["inputs"]["width"], 2048);
        let sampler = nodes(&wf, "KSampler")[0];
        assert_eq!(
            node(&wf, &sampler["inputs"]["model"])["class_type"],
            "ModelSamplingFlux"
        );
        assert!(nodes(&wf, "SplitImageWithAlpha").is_empty());
        let save = nodes(&wf, "MooshieSaveImage")[0];
        assert_eq!(
            node(&wf, &save["inputs"]["images"])["class_type"],
            "VAEDecode"
        );
        let clip = nodes(&wf, "CLIPLoader")[0];
        assert_eq!(clip["inputs"]["type"], "qwen_image");
    }

    #[test]
    fn transparent_toggle_puts_the_rgba_phrase_first() {
        let mut p = params("txt2img", "mingimage");
        p.transparent_background = true;
        let wf = crate::templates::build_workflow(&p, 7, false);
        let texts: Vec<&str> = nodes(&wf, "CLIPTextEncode")
            .iter()
            .filter_map(|n| n["inputs"]["text"].as_str())
            .collect();
        assert!(texts
            .iter()
            .any(|t| t.starts_with("RGBA, 4-channel, transparent background. A poster")));
        assert_eq!(
            prefixed_prompt("RGBA, 4-channel, transparent background, a cat"),
            "RGBA, 4-channel, transparent background, a cat"
        );
        assert_eq!(prefixed_prompt("  "), TRANSPARENT_PHRASE);
    }

    #[test]
    fn rgb_only_chains_get_the_alpha_dropped_first() {
        let mut p = params("txt2img", "mingimage");
        p.upscale_enabled = true;
        p.upscale_model = Some("4x-UltraSharp.pth".into());
        let wf = crate::templates::build_workflow(&p, 7, false);
        let split = nodes(&wf, "SplitImageWithAlpha");
        assert_eq!(split.len(), 1);
        assert_eq!(
            node(&wf, &split[0]["inputs"]["image"])["class_type"],
            "VAEDecode"
        );
    }

    #[test]
    fn edit_conditions_on_every_reference_at_the_first_ones_size() {
        let mut p = params("image_edit", "mingimage");
        p.width = 1024;
        p.height = 1024;
        p.edit_reference_images = vec!["a.png".into(), "".into(), "b.png".into()];
        let wf = crate::templates::build_workflow(&p, 7, false);
        let encode = nodes(&wf, "TextEncodeMingImageEdit");
        assert_eq!(encode.len(), 1);
        let inputs = &encode[0]["inputs"];
        assert!(inputs["vae"].is_array());
        assert_eq!(
            node(&wf, &inputs["images.image_1"])["class_type"],
            "ImageScaleToTotalPixels"
        );
        assert_eq!(
            node(&wf, &inputs["images.image_2"])["class_type"],
            "LoadImage"
        );
        assert!(inputs.get("images.image_3").is_none());
        let scale = node(&wf, &inputs["images.image_1"]);
        assert_eq!(scale["inputs"]["megapixels"], 1.05);
        assert_eq!(scale["inputs"]["resolution_steps"], 16);
        let latent = nodes(&wf, "EmptyLatentImage")[0];
        assert_eq!(
            node(&wf, &latent["inputs"]["width"])["class_type"],
            "GetImageSize"
        );
        let sampler = nodes(&wf, "KSampler")[0];
        assert_eq!(
            node(&wf, &sampler["inputs"]["negative"])["class_type"],
            "ConditioningZeroOut"
        );
        let shift = nodes(&wf, "ModelSamplingFlux")[0];
        assert_eq!(shift["inputs"]["width"], latent["inputs"]["width"]);
    }

    #[test]
    fn layer_count_comes_from_the_prompt() {
        assert_eq!(
            requested_layers("Decompose this image into 6 layers with the following"),
            Some(6)
        );
        assert_eq!(
            requested_layers("Number of layers: 3\nLayer 1: text"),
            Some(3)
        );
        assert_eq!(
            requested_layers("split it into 40 layers"),
            Some(MAX_LAYERS)
        );
        assert_eq!(requested_layers("a poster with layers"), None);
        assert_eq!(
            layer_prompt("title on top"),
            (
                "Decompose this image into 4 layers.\ntitle on top".to_string(),
                4
            )
        );
        assert_eq!(
            layer_prompt(""),
            ("Decompose this image into 4 layers.".to_string(), 4)
        );
    }

    #[test]
    fn layers_decode_each_frame_and_drop_the_composite() {
        let mut p = params("image_edit", "mingimagelayer");
        p.input_image = Some("card.png".into());
        p.positive_prompt = "Decompose this image into 3 layers with the following specifications:\nNumber of layers: 3".into();
        let wf = crate::templates::build_workflow(&p, 7, false);
        let latent = nodes(&wf, "EmptyHunyuanLatentVideo")[0];
        assert_eq!(latent["inputs"]["length"], 13);
        let encodes = nodes(&wf, "TextEncodeMingImageEdit");
        assert_eq!(encodes.len(), 2);
        assert!(encodes.iter().any(|n| n["inputs"]["prompt"] == ""));
        let sampler = nodes(&wf, "KSampler")[0];
        assert_eq!(sampler["inputs"]["cfg"], 2.0);
        let save = nodes(&wf, "MooshieSaveImage")[0];
        let pick = node(&wf, &save["inputs"]["images"]);
        assert_eq!(pick["class_type"], "ImageFromBatch");
        assert_eq!(pick["inputs"]["batch_index"], 1);
        assert_eq!(pick["inputs"]["length"], 3);
        let decode = node(&wf, &pick["inputs"]["image"]);
        assert_eq!(decode["class_type"], "VAEDecode");
        let cut = node(&wf, &decode["inputs"]["samples"]);
        assert_eq!(cut["class_type"], "LatentCutToBatch");
        assert_eq!(cut["inputs"]["dim"], "t");
        let scale = nodes(&wf, "ImageScaleToTotalPixels")[0];
        assert_eq!(scale["inputs"]["megapixels"], LAYER_MEGAPIXELS);
    }
}
