//! Image Edit mode: instruction-driven editing of one or more reference images.
//!
//! Two paths, keyed on the resolved model family:
//!   * Qwen Image Edit / Edit Plus (`qwen_edit`, `qwen_edit_plus`) —
//!     `TextEncodeQwenImageEdit` / `TextEncodeQwenImageEditPlus` fold the reference
//!     image(s) into the conditioning (Qwen2.5-VL vision tokens + a reference latent
//!     when a VAE is wired). Sampling starts from an empty SD3 latent at the target
//!     resolution.
//!   * Flux.1 Kontext (`flux1kontext`) — the reference is scaled with
//!     `FluxKontextImageScale`, encoded with `VAEEncode`, and attached to the positive
//!     conditioning via `ReferenceLatent` + `FluxGuidance`. The KSampler denoises from
//!     the encoded reference latent (denoise 1.0) so the output matches Kontext's
//!     preferred scaled resolution.
//!
//!   * Anima ReStyler (`anima`) — character-consistent restyling. The reference is
//!     scaled to the target size, VAE-encoded, and applied through
//!     `ApplyCosmosReferenceLatent` (ComfyUI-Cosmos-Reference, auto-installed with
//!     the style-transfer packs), which temporally concatenates the reference
//!     latent inside the diffusion model on every step. With the Anima Edit LoRA
//!     loaded, sampling is otherwise plain txt2img from an empty latent at the
//!     same size (the temporal concat requires matching spatial dims).
//!
//! Qwen/Kontext use only core ComfyUI nodes; Anima additionally needs the
//! ComfyUI-Cosmos-Reference custom node package.

use serde_json::json;

use super::{build_scheduled_conditioning, insert_vae_decode, load_model_nodes, WorkflowResult};
use crate::comfyui::nodes::ANIMA_EDIT_LORA_FILENAME;
use crate::comfyui::types::GenerationParams;

pub fn build(params: &GenerationParams, seed: i64) -> WorkflowResult {
    match params.model_architecture.as_str() {
        "flux1kontext" => build_kontext(params, seed),
        "qwen_edit" | "qwen_edit_plus" => build_qwen(params, seed),
        "anima" => build_anima_restyler(params, seed),
        // Validation guarantees a supported edit family reaches here; fall back to
        // txt2img defensively rather than panicking on an unexpected architecture.
        _ => super::txt2img::build(params, seed),
    }
}

/// Non-empty reference-image filenames (ComfyUI input names), slot 0 first.
fn reference_images(params: &GenerationParams) -> Vec<&str> {
    params
        .edit_reference_images
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Qwen Image Edit / Edit Plus path.
fn build_qwen(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    // Edit Plus accepts up to three references; single Edit uses only slot 0.
    let is_plus = params.model_architecture == "qwen_edit_plus";
    let mut refs = reference_images(params);
    if refs.is_empty() {
        // Validation should prevent this; keep the workflow well-formed regardless.
        refs.push("");
    }
    if !is_plus {
        refs.truncate(1);
    }
    refs.truncate(3);

    // LoadImage per reference slot.
    let mut load_ids: Vec<String> = Vec::with_capacity(refs.len());
    for image_name in &refs {
        let load_id = next_id.to_string();
        workflow.insert(
            load_id.clone(),
            json!({
                "class_type": "LoadImage",
                "inputs": { "image": image_name }
            }),
        );
        load_ids.push(load_id);
        next_id += 1;
    }

    // Positive and negative conditioning both fold in the reference image(s) and VAE,
    // matching ComfyUI's bundled Qwen Image Edit templates.
    let (pos_source, nid) = encode_qwen_conditioning(
        &mut workflow,
        next_id,
        is_plus,
        &clip_source,
        &vae_source,
        &load_ids,
        &params.positive_prompt,
    );
    next_id = nid;

    let (neg_source, nid) = encode_qwen_conditioning(
        &mut workflow,
        next_id,
        is_plus,
        &clip_source,
        &vae_source,
        &load_ids,
        &params.negative_prompt,
    );
    next_id = nid;

    // Empty latent at the target resolution (Qwen uses the SD3 latent node).
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
        refiner_model_source: None,
    }
}

/// Insert a `TextEncodeQwenImageEdit` (single) or `TextEncodeQwenImageEditPlus`
/// (multi) node wiring the CLIP, VAE, prompt text, and the loaded reference
/// image(s). Returns `(conditioning_source, next_id)`.
#[allow(clippy::too_many_arguments)]
fn encode_qwen_conditioning(
    workflow: &mut serde_json::Map<String, serde_json::Value>,
    next_id: u32,
    is_plus: bool,
    clip_source: &(String, u32),
    vae_source: &(String, u32),
    load_ids: &[String],
    prompt: &str,
) -> ((String, u32), u32) {
    let node_id = next_id.to_string();
    let mut inputs = serde_json::Map::new();
    inputs.insert("clip".into(), json!([clip_source.0.clone(), clip_source.1]));
    inputs.insert("prompt".into(), json!(prompt));
    inputs.insert("vae".into(), json!([vae_source.0.clone(), vae_source.1]));

    let class_type = if is_plus {
        // image1..image3 are optional inputs — wire only the slots we have.
        for (idx, load_id) in load_ids.iter().take(3).enumerate() {
            inputs.insert(format!("image{}", idx + 1), json!([load_id.clone(), 0]));
        }
        "TextEncodeQwenImageEditPlus"
    } else {
        if let Some(load_id) = load_ids.first() {
            inputs.insert("image".into(), json!([load_id.clone(), 0]));
        }
        "TextEncodeQwenImageEdit"
    };

    workflow.insert(
        node_id.clone(),
        json!({ "class_type": class_type, "inputs": inputs }),
    );
    ((node_id, 0), next_id + 1)
}

/// Sampling size for the split-screen composite: ~1.4 MP total, the original
/// ReStyler recipe (Anima quality drops off above that). Returns
/// `(half_width, height)`; both are multiples of 16 so the mask boundary and
/// the output crop land on exact latent cells.
fn split_composite_dims(width: u32, height: u32) -> (u32, u32) {
    const SPLIT_TOTAL_PIXELS: f64 = 1_400_000.0;
    let scale = (SPLIT_TOTAL_PIXELS / ((width as f64) * 2.0 * (height as f64))).sqrt();
    let round16 = |v: f64| (((v / 16.0).round() as u32) * 16).max(16);
    (
        round16(width as f64 * scale),
        round16(height as f64 * scale),
    )
}

/// `solo` contradicts the split-screen `multiple views` conditioning head-on:
/// the model resolves the conflict by leaving the masked panel untouched,
/// which decodes as a solid black image. Verified empirically: adding `solo`
/// to an otherwise working split prompt reliably blacks out the output, and
/// removing it from a failing prompt fixes it with no other change.
fn strip_split_conflict_tags(prompt: &str) -> String {
    prompt
        .split(',')
        .filter(|seg| {
            let t = seg.trim();
            // Match the bare tag and self-contained weighted forms like
            // "(solo)" or "(solo:1.2)". A "solo" inside a multi-tag weight
            // group is left alone so we never unbalance parentheses.
            let inner = t
                .strip_prefix('(')
                .and_then(|s| s.strip_suffix(')'))
                .map(|s| s.split(':').next().unwrap_or(s).trim())
                .unwrap_or(t);
            !(inner.eq_ignore_ascii_case("solo") && (t == inner || t.starts_with('(')))
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Anima ReStyler path (Cosmos reference latent + Anima Edit LoRA).
///
/// The reference image is scaled to the target size, VAE-encoded, and fed to
/// `ApplyCosmosReferenceLatent`, which patches the diffusion model to
/// concatenate the reference latent along the temporal dimension on every
/// step. The temporal concat requires the generation latent and the reference
/// to match exactly in spatial size, so the two modes are:
///
/// - Default (full-frame): plain txt2img from an empty latent at the target
///   size. Subtle restyles at full output resolution.
/// - `edit_split_screen` ("drastic restyle", the original ReStyler recipe):
///   the reference is stitched beside a solid black panel into a 2x-wide
///   composite, downscaled to ~1.4 MP total (Anima's happy zone), and sampled
///   through `InpaintModelConditioning` with the mask over the panel half and
///   a `(split screen, multiple views, split screen:1.2)` prompt prefix. The
///   same scaled composite is the Cosmos ref (the temporal concat requires ref
///   and generation latents to match spatially). What fills the panel decides
///   the result — the temporal-consistency conditioning reproduces whatever
///   the ref shows there: gray paints flat gray, a copy of the character locks
///   the output to the original style, and a white panel reads as an
///   intentionally blank comic panel on many seeds (solid white output). Black
///   with inpaint conditioning (pixels kept in the latent, noise on top) is
///   the original workflow's recipe and survives seeds that white+
///   VAEEncodeForInpaint did not. The right half is cropped and scaled back to
///   the requested output size.
fn build_anima_restyler(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    let mut model_source = ml.model_source.clone();

    // Split-screen mode leans on the model's "multiple views" prior: the
    // prompt hack makes it treat the reference half as one view and paint a
    // fresh view of the same character on the blank half. The tag is doubled
    // inside the weight group (the original workflow's phrasing): long
    // tag-dump prompts dilute a single mention until the model treats the
    // panel as intentionally empty.
    let positive_prompt = if params.edit_split_screen {
        format!(
            "(split screen, multiple views, split screen:1.2), {}",
            strip_split_conflict_tags(&params.positive_prompt)
        )
    } else {
        params.positive_prompt.clone()
    };

    let (pos_source, nid) =
        build_scheduled_conditioning(&mut workflow, next_id, &clip_source, &positive_prompt, &[]);
    next_id = nid;

    let (neg_source, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.negative_prompt,
        &[],
    );
    next_id = nid;

    let image_name = reference_images(params)
        .first()
        .copied()
        .unwrap_or_default()
        .to_string();

    let load_id = next_id.to_string();
    workflow.insert(
        load_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": { "image": image_name }
        }),
    );
    next_id += 1;

    // Normalize the reference to the target output size; the temporal concat
    // in the Cosmos patch requires ref and generation latents to match.
    let scale_id = next_id.to_string();
    workflow.insert(
        scale_id.clone(),
        json!({
            "class_type": "ImageScale",
            "inputs": {
                "image": [load_id, 0],
                "width": params.width,
                "height": params.height,
                "upscale_method": "lanczos",
                "crop": "disabled"
            }
        }),
    );
    next_id += 1;

    // Split-screen: stitch the reference beside a solid black panel into a
    // 2x-wide composite, then downscale the whole composite to ~1.4 MP total.
    // Both the Cosmos ref and the sampled pixels are this same scaled
    // composite — the temporal concat requires ref and generation latents to
    // match spatially, and Anima degrades noticeably above ~1.4 MP (the
    // original ReStyler samples there regardless of the requested size).
    // Dimensions are computed in Rust (multiples of 16, equal halves) so the
    // mask boundary and the final crop land on exact latent cells.
    let mut split_dims: Option<(u32, u32)> = None; // (half_w, comp_h)
    let ref_pixels = if params.edit_split_screen {
        let (half_w, comp_h) = split_composite_dims(params.width, params.height);
        split_dims = Some((half_w, comp_h));

        // Black start panel, the original recipe's default. The panel fill
        // decides the result: the temporal-consistency conditioning reproduces
        // whatever the ref shows there. Gray paints flat gray; a copy of the
        // character locks the output to the original style; white reads as an
        // intentionally blank comic panel on unlucky seeds (solid white
        // output). All verified live.
        let blank_id = next_id.to_string();
        workflow.insert(
            blank_id.clone(),
            json!({
                "class_type": "EmptyImage",
                "inputs": {
                    "width": params.width,
                    "height": params.height,
                    "batch_size": 1,
                    "color": 0
                }
            }),
        );
        next_id += 1;

        let stitch_id = next_id.to_string();
        workflow.insert(
            stitch_id.clone(),
            json!({
                "class_type": "ImageStitch",
                "inputs": {
                    "image1": [scale_id, 0],
                    "image2": [blank_id, 0],
                    "direction": "right",
                    "match_image_size": true,
                    "spacing_width": 0,
                    "spacing_color": "white"
                }
            }),
        );
        next_id += 1;

        let comp_scale_id = next_id.to_string();
        workflow.insert(
            comp_scale_id.clone(),
            json!({
                "class_type": "ImageScale",
                "inputs": {
                    "image": [stitch_id, 0],
                    "width": half_w * 2,
                    "height": comp_h,
                    "upscale_method": "lanczos",
                    "crop": "disabled"
                }
            }),
        );
        next_id += 1;
        comp_scale_id
    } else {
        scale_id
    };

    // Reference latent for the Cosmos model patch.
    let ref_encode_id = next_id.to_string();
    workflow.insert(
        ref_encode_id.clone(),
        json!({
            "class_type": "VAEEncode",
            "inputs": {
                "pixels": [ref_pixels.clone(), 0],
                "vae": [vae_source.0.clone(), vae_source.1]
            }
        }),
    );
    next_id += 1;

    // Reference strength: the Cosmos node has no strength input, so adherence
    // is controlled by scaling the reference latent itself. 1.0 keeps the
    // output close to the reference; lower values give the prompt/style more
    // room to reinterpret it (verified live: 0.6 visibly loosens the restyle).
    let mut ref_source = (ref_encode_id, 0u32);
    let ref_strength = params.edit_reference_strength.clamp(0.0, 1.0);
    if ref_strength < 1.0 {
        let mult_id = next_id.to_string();
        workflow.insert(
            mult_id.clone(),
            json!({
                "class_type": "LatentMultiply",
                "inputs": {
                    "samples": [ref_source.0.clone(), ref_source.1],
                    "multiplier": ref_strength
                }
            }),
        );
        ref_source = (mult_id, 0);
        next_id += 1;
    }

    // V3 Autogrow inputs serialize dot-prefixed in the API prompt
    // ("ref_latents.ref_latent_1"). A bare "ref_latent_1" never binds, and
    // because the slot's min is 0 the node silently applies no reference.
    let cosmos_id = next_id.to_string();
    workflow.insert(
        cosmos_id.clone(),
        json!({
            "class_type": "ApplyCosmosReferenceLatent",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "ref_latents.ref_latent_1": [ref_source.0, ref_source.1]
            }
        }),
    );
    model_source = (cosmos_id, 0);
    next_id += 1;

    // Anima Edit LoRA after the Cosmos patch (the original workflow's order),
    // after the user's own LoRA chain, and model-only so the text encoder
    // stays untouched. Loaded by constant filename — the frontend download
    // button installs it under the same name. Split-screen uses the original
    // recipe's 0.72: at 1.0 the LoRA over-locks the output to the reference
    // panel. The subtler full-frame mode keeps full strength.
    let edit_lora_id = next_id.to_string();
    workflow.insert(
        edit_lora_id.clone(),
        json!({
            "class_type": "LoraLoaderModelOnly",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "lora_name": ANIMA_EDIT_LORA_FILENAME,
                "strength_model": if params.edit_split_screen { 0.72 } else { 1.0 }
            }
        }),
    );
    model_source = (edit_lora_id, 0);
    next_id += 1;

    // Split-screen sampling state comes from InpaintModelConditioning: the
    // composite's pixels stay in the latent with noise added on top, and the
    // noise mask confines denoise-1.0 sampling to the panel half. (The
    // previous VAEEncodeForInpaint approach zeroed the masked region instead;
    // combined with the white panel it collapsed to blank outputs on many
    // seeds.)
    let mut sampler_positive = pos_source.clone();
    let mut sampler_negative = neg_source.clone();
    let latent_source: (String, u32) = if let Some((half_w, comp_h)) = split_dims {
        let base_mask_id = next_id.to_string();
        workflow.insert(
            base_mask_id.clone(),
            json!({
                "class_type": "SolidMask",
                "inputs": { "value": 0.0, "width": half_w * 2, "height": comp_h }
            }),
        );
        next_id += 1;

        let panel_mask_id = next_id.to_string();
        workflow.insert(
            panel_mask_id.clone(),
            json!({
                "class_type": "SolidMask",
                "inputs": { "value": 1.0, "width": half_w, "height": comp_h }
            }),
        );
        next_id += 1;

        let mask_id = next_id.to_string();
        workflow.insert(
            mask_id.clone(),
            json!({
                "class_type": "MaskComposite",
                "inputs": {
                    "destination": [base_mask_id, 0],
                    "source": [panel_mask_id, 0],
                    "x": half_w,
                    "y": 0,
                    "operation": "add"
                }
            }),
        );
        next_id += 1;

        let inpaint_id = next_id.to_string();
        workflow.insert(
            inpaint_id.clone(),
            json!({
                "class_type": "InpaintModelConditioning",
                "inputs": {
                    "positive": [pos_source.0.clone(), pos_source.1],
                    "negative": [neg_source.0.clone(), neg_source.1],
                    "vae": [vae_source.0.clone(), vae_source.1],
                    "pixels": [ref_pixels.clone(), 0],
                    "mask": [mask_id, 0],
                    "noise_mask": true
                }
            }),
        );
        next_id += 1;
        sampler_positive = (inpaint_id.clone(), 0);
        sampler_negative = (inpaint_id.clone(), 1);

        if params.batch_size > 1 {
            let repeat_id = next_id.to_string();
            workflow.insert(
                repeat_id.clone(),
                json!({
                    "class_type": "RepeatLatentBatch",
                    "inputs": {
                        "samples": [inpaint_id, 2],
                        "amount": params.batch_size
                    }
                }),
            );
            next_id += 1;
            (repeat_id, 0)
        } else {
            (inpaint_id, 2)
        }
    } else {
        let id = next_id.to_string();
        workflow.insert(
            id.clone(),
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
        (id, 0)
    };

    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [sampler_positive.0.clone(), sampler_positive.1],
                "negative": [sampler_negative.0.clone(), sampler_negative.1],
                "latent_image": [latent_source.0, latent_source.1],
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

    let (decode_id, mut next_id) =
        insert_vae_decode(&mut workflow, next_id, &sampler_id, &vae_source, params);

    // Split-screen: the composite's right half is the restyled output, scaled
    // back up from the 1.4 MP sampling size to the requested output size.
    let image_output = if let Some((half_w, comp_h)) = split_dims {
        let crop_id = next_id.to_string();
        workflow.insert(
            crop_id.clone(),
            json!({
                "class_type": "ImageCrop",
                "inputs": {
                    "image": [decode_id, 0],
                    "width": half_w,
                    "height": comp_h,
                    "x": half_w,
                    "y": 0
                }
            }),
        );
        next_id += 1;

        let out_scale_id = next_id.to_string();
        workflow.insert(
            out_scale_id.clone(),
            json!({
                "class_type": "ImageScale",
                "inputs": {
                    "image": [crop_id, 0],
                    "width": params.width,
                    "height": params.height,
                    "upscale_method": "lanczos",
                    "crop": "disabled"
                }
            }),
        );
        next_id += 1;
        (out_scale_id, 0)
    } else {
        (decode_id, 0)
    };

    WorkflowResult {
        workflow,
        next_id,
        image_output,
        model_source,
        clip_source,
        positive_source: pos_source,
        negative_source: neg_source,
        vae_source,
        sampler_id,
        // The Cosmos patch is size-locked to the base pass; refinement
        // samplers (upscale/facefix/segment) run at other resolutions and
        // must use the plain Anima model instead.
        refiner_model_source: Some(ml.model_source.clone()),
    }
}

/// Flux.1 Kontext path.
fn build_kontext(params: &GenerationParams, seed: i64) -> WorkflowResult {
    let mut workflow = serde_json::Map::new();
    let next_id: u32 = 1;

    let ml = load_model_nodes(&mut workflow, next_id, params);
    let mut next_id = ml.next_id;
    let model_source = ml.model_source;
    let clip_source = ml.clip_source;
    let vae_source = ml.vae_source;

    // Base positive conditioning (plain text encode; no schedule segments in edit mode).
    let (pos_base, nid) = build_scheduled_conditioning(
        &mut workflow,
        next_id,
        &clip_source,
        &params.positive_prompt,
        &[],
    );
    next_id = nid;

    let image_name = reference_images(params)
        .first()
        .copied()
        .unwrap_or_default()
        .to_string();

    let load_id = next_id.to_string();
    workflow.insert(
        load_id.clone(),
        json!({
            "class_type": "LoadImage",
            "inputs": { "image": image_name }
        }),
    );
    next_id += 1;

    // Snap the reference to a Kontext-preferred resolution before encoding.
    let scale_id = next_id.to_string();
    workflow.insert(
        scale_id.clone(),
        json!({
            "class_type": "FluxKontextImageScale",
            "inputs": { "image": [load_id, 0] }
        }),
    );
    next_id += 1;

    let vae_encode_id = next_id.to_string();
    workflow.insert(
        vae_encode_id.clone(),
        json!({
            "class_type": "VAEEncode",
            "inputs": {
                "pixels": [scale_id, 0],
                "vae": [vae_source.0.clone(), vae_source.1]
            }
        }),
    );
    next_id += 1;

    // Attach the reference latent to the positive conditioning.
    let ref_id = next_id.to_string();
    workflow.insert(
        ref_id.clone(),
        json!({
            "class_type": "ReferenceLatent",
            "inputs": {
                "conditioning": [pos_base.0.clone(), pos_base.1],
                "latent": [vae_encode_id.clone(), 0]
            }
        }),
    );
    next_id += 1;

    // Kontext is guidance-distilled — apply FluxGuidance to the referenced positive.
    let guidance_id = next_id.to_string();
    workflow.insert(
        guidance_id.clone(),
        json!({
            "class_type": "FluxGuidance",
            "inputs": {
                "conditioning": [ref_id, 0],
                "guidance": params.flux_guidance
            }
        }),
    );
    let pos_source = (guidance_id, 0u32);
    next_id += 1;

    // Negative is a zeroed copy of the base text conditioning (KSampler ignores it at
    // cfg 1.0 but still requires a valid, shape-compatible input).
    let neg_id = next_id.to_string();
    workflow.insert(
        neg_id.clone(),
        json!({
            "class_type": "ConditioningZeroOut",
            "inputs": { "conditioning": [pos_base.0.clone(), pos_base.1] }
        }),
    );
    let neg_source = (neg_id, 0u32);
    next_id += 1;

    // Denoise the encoded reference latent (denoise 1.0 replaces its content with noise
    // while keeping Kontext's scaled output resolution).
    let sampler_id = next_id.to_string();
    workflow.insert(
        sampler_id.clone(),
        json!({
            "class_type": "KSampler",
            "inputs": {
                "model": [model_source.0.clone(), model_source.1],
                "positive": [pos_source.0.clone(), pos_source.1],
                "negative": [neg_source.0.clone(), neg_source.1],
                "latent_image": [vae_encode_id, 0],
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
        refiner_model_source: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anima_params() -> GenerationParams {
        serde_json::from_value(serde_json::json!({
            "mode": "image_edit",
            "model_architecture": "anima",
            "positive_prompt": "1girl, watercolor style",
            "negative_prompt": "lowres",
            "checkpoint": "anima.safetensors",
            "loras": [],
            "sampler_name": "er_sde",
            "scheduler": "simple",
            "steps": 30,
            "cfg": 4.0,
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
            "edit_reference_images": ["ref.png"],
        }))
        .expect("params")
    }

    fn nodes_of_type<'a>(
        result: &'a WorkflowResult,
        class_type: &str,
    ) -> Vec<&'a serde_json::Value> {
        result
            .workflow
            .values()
            .filter(|n| n["class_type"] == class_type)
            .collect()
    }

    #[test]
    fn anima_restyler_builds_the_cosmos_reference_chain() {
        let result = build(&anima_params(), 42);

        for class in [
            "LoadImage",
            "ImageScale",
            "LoraLoaderModelOnly",
            "VAEEncode",
            "ApplyCosmosReferenceLatent",
            "EmptySD3LatentImage",
            "KSampler",
        ] {
            assert_eq!(nodes_of_type(&result, class).len(), 1, "missing {class}");
        }
        // The split-screen inpaint chain only appears when `edit_split_screen`
        // is on; the default full-frame path must not carry it.
        for class in [
            "InpaintModelConditioning",
            "SolidMask",
            "MaskComposite",
            "ImageCrop",
            "ImageStitch",
            "EmptyImage",
        ] {
            assert_eq!(nodes_of_type(&result, class).len(), 0, "stale {class}");
        }

        // Edit LoRA loaded by the constant filename the download button uses,
        // at full strength in full-frame mode.
        let lora = nodes_of_type(&result, "LoraLoaderModelOnly")[0];
        assert_eq!(lora["inputs"]["lora_name"], ANIMA_EDIT_LORA_FILENAME);
        assert_eq!(lora["inputs"]["strength_model"], 1.0);

        // Model chain into the sampler: Cosmos patch → edit LoRA (the original
        // workflow's order). The Cosmos node takes the reference latent on the
        // V3 Autogrow slot, which serializes dot-prefixed in the API prompt. A
        // bare `ref_latent_1` never binds (min=0 makes it a silent no-op) —
        // that was the black-image bug.
        let cosmos = nodes_of_type(&result, "ApplyCosmosReferenceLatent")[0];
        assert!(cosmos["inputs"]["ref_latents.ref_latent_1"].is_array());
        assert!(cosmos["inputs"].get("ref_latent_1").is_none());
        let sampler = &result.workflow[&result.sampler_id];
        let sampler_model = sampler["inputs"]["model"][0].as_str().unwrap();
        assert_eq!(
            result.workflow[sampler_model]["class_type"],
            "LoraLoaderModelOnly"
        );
        let lora_model = result.workflow[sampler_model]["inputs"]["model"][0]
            .as_str()
            .unwrap();
        assert_eq!(
            result.workflow[lora_model]["class_type"],
            "ApplyCosmosReferenceLatent"
        );

        // The user's prompt passes through untouched (no split-screen trigger).
        let pos = &result.workflow[&result.positive_source.0];
        let text = pos["inputs"]["text"].as_str().unwrap();
        assert!(!text.contains("split screen"));
        assert!(text.contains("watercolor style"));

        // Generation latent matches the reference size exactly, sampled at
        // full denoise from an empty latent.
        let latent = nodes_of_type(&result, "EmptySD3LatentImage")[0];
        assert_eq!(latent["inputs"]["width"], 1024);
        assert_eq!(latent["inputs"]["height"], 1024);
        let scale = nodes_of_type(&result, "ImageScale")[0];
        assert_eq!(scale["inputs"]["width"], 1024);
        assert_eq!(scale["inputs"]["height"], 1024);
        assert_eq!(sampler["inputs"]["denoise"], 1.0);
        let latent_ref = sampler["inputs"]["latent_image"][0].as_str().unwrap();
        assert_eq!(
            result.workflow[latent_ref]["class_type"],
            "EmptySD3LatentImage"
        );

        // Output is the decoded full-frame image.
        assert_eq!(
            result.workflow[&result.image_output.0]["class_type"],
            "VAEDecode"
        );

        // At the default reference strength (1.0) the ref latent goes straight
        // into the Cosmos node with no LatentMultiply in between.
        assert_eq!(nodes_of_type(&result, "LatentMultiply").len(), 0);
        let ref_node = cosmos["inputs"]["ref_latents.ref_latent_1"][0]
            .as_str()
            .unwrap();
        assert_eq!(result.workflow[ref_node]["class_type"], "VAEEncode");
    }

    #[test]
    fn anima_restyler_scales_the_reference_latent_below_full_strength() {
        let mut params = anima_params();
        params.edit_reference_strength = 0.6;
        let result = build(&params, 42);

        let mults = nodes_of_type(&result, "LatentMultiply");
        assert_eq!(mults.len(), 1);
        assert_eq!(mults[0]["inputs"]["multiplier"], 0.6);

        // The Cosmos ref slot reads the scaled latent, which reads the encode.
        let cosmos = nodes_of_type(&result, "ApplyCosmosReferenceLatent")[0];
        let ref_node = cosmos["inputs"]["ref_latents.ref_latent_1"][0]
            .as_str()
            .unwrap();
        assert_eq!(result.workflow[ref_node]["class_type"], "LatentMultiply");
        let mult_src = result.workflow[ref_node]["inputs"]["samples"][0]
            .as_str()
            .unwrap();
        assert_eq!(result.workflow[mult_src]["class_type"], "VAEEncode");
    }

    #[test]
    fn anima_restyler_split_screen_builds_the_inpaint_composite() {
        let mut params = anima_params();
        params.edit_split_screen = true;
        let result = build(&params, 42);

        // 1024x1024 target → 2048x1024 composite → scaled to ~1.4 MP as
        // 1664x832 (halves of 832, multiples of 16).
        let (half_w, comp_h) = split_composite_dims(1024, 1024);
        assert_eq!((half_w, comp_h), (832, 832));

        // Composite: reference (scaled to target size) stitched beside a
        // solid BLACK panel — white reads as an intentionally blank comic
        // panel on unlucky seeds — then downscaled to the sampling size.
        let stitches = nodes_of_type(&result, "ImageStitch");
        assert_eq!(stitches.len(), 1);
        assert_eq!(stitches[0]["inputs"]["direction"], "right");
        assert_eq!(
            result.workflow[stitches[0]["inputs"]["image1"][0].as_str().unwrap()]["class_type"],
            "ImageScale"
        );
        let blank_id = stitches[0]["inputs"]["image2"][0].as_str().unwrap();
        let blank = &result.workflow[blank_id];
        assert_eq!(blank["class_type"], "EmptyImage");
        assert_eq!(blank["inputs"]["color"], 0);
        assert_eq!(blank["inputs"]["width"], 1024);
        assert_eq!(blank["inputs"]["height"], 1024);

        // Sampling state comes from InpaintModelConditioning over the scaled
        // composite (pixels kept in the latent, noise mask over the panel
        // half), at full denoise; no empty latent in this mode.
        assert_eq!(nodes_of_type(&result, "EmptySD3LatentImage").len(), 0);
        let inpaints = nodes_of_type(&result, "InpaintModelConditioning");
        assert_eq!(inpaints.len(), 1);
        assert_eq!(inpaints[0]["inputs"]["noise_mask"], true);
        let comp_scale_id = inpaints[0]["inputs"]["pixels"][0].as_str().unwrap();
        let comp_scale = &result.workflow[comp_scale_id];
        assert_eq!(comp_scale["class_type"], "ImageScale");
        assert_eq!(comp_scale["inputs"]["width"], 2 * half_w);
        assert_eq!(comp_scale["inputs"]["height"], comp_h);
        let stitch_id = result
            .workflow
            .iter()
            .find(|(_, n)| n["class_type"] == "ImageStitch")
            .map(|(id, _)| id.clone())
            .unwrap();
        assert_eq!(comp_scale["inputs"]["image"][0], json!(stitch_id));

        // Mask covers exactly the right half of the sampling composite.
        let mask_id = inpaints[0]["inputs"]["mask"][0].as_str().unwrap();
        let mask = &result.workflow[mask_id];
        assert_eq!(mask["class_type"], "MaskComposite");
        assert_eq!(mask["inputs"]["x"], half_w);
        assert_eq!(mask["inputs"]["y"], 0);
        let base_mask = &result.workflow[mask["inputs"]["destination"][0].as_str().unwrap()];
        assert_eq!(base_mask["inputs"]["value"], 0.0);
        assert_eq!(base_mask["inputs"]["width"], 2 * half_w);
        assert_eq!(base_mask["inputs"]["height"], comp_h);
        let panel_mask = &result.workflow[mask["inputs"]["source"][0].as_str().unwrap()];
        assert_eq!(panel_mask["inputs"]["value"], 1.0);
        assert_eq!(panel_mask["inputs"]["width"], half_w);

        // Sampler takes latent AND conditioning from the inpaint node.
        let sampler = &result.workflow[&result.sampler_id];
        assert_eq!(sampler["inputs"]["denoise"], 1.0);
        let inpaint_id = result
            .workflow
            .iter()
            .find(|(_, n)| n["class_type"] == "InpaintModelConditioning")
            .map(|(id, _)| id.clone())
            .unwrap();
        assert_eq!(sampler["inputs"]["latent_image"][0], json!(inpaint_id));
        assert_eq!(sampler["inputs"]["latent_image"][1], 2);
        assert_eq!(sampler["inputs"]["positive"][0], json!(inpaint_id));
        assert_eq!(sampler["inputs"]["positive"][1], 0);
        assert_eq!(sampler["inputs"]["negative"][0], json!(inpaint_id));
        assert_eq!(sampler["inputs"]["negative"][1], 1);

        // The Cosmos ref encodes the SAME scaled composite the sampler sees,
        // so ref and generation latents match spatially.
        let cosmos = nodes_of_type(&result, "ApplyCosmosReferenceLatent")[0];
        let ref_node = cosmos["inputs"]["ref_latents.ref_latent_1"][0]
            .as_str()
            .unwrap();
        let ref_encode = &result.workflow[ref_node];
        assert_eq!(ref_encode["class_type"], "VAEEncode");
        assert_eq!(
            ref_encode["inputs"]["pixels"][0].as_str().unwrap(),
            comp_scale_id
        );

        // Split mode runs the edit LoRA at the original recipe's 0.72.
        let lora = nodes_of_type(&result, "LoraLoaderModelOnly")[0];
        assert_eq!(lora["inputs"]["strength_model"], 0.72);

        // The prompt hack that makes the model paint a second view; the tag
        // is doubled inside the weight group so long tag dumps can't dilute
        // it away.
        let pos = &result.workflow[&result.positive_source.0];
        let text = pos["inputs"]["text"].as_str().unwrap();
        assert!(text.starts_with("(split screen, multiple views, split screen:1.2), "));
        assert!(text.contains("watercolor style"));

        // Output: crop the right half of the sampled composite, then scale it
        // back up to the requested output size.
        let out = &result.workflow[&result.image_output.0];
        assert_eq!(out["class_type"], "ImageScale");
        assert_eq!(out["inputs"]["width"], 1024);
        assert_eq!(out["inputs"]["height"], 1024);
        let crop = &result.workflow[out["inputs"]["image"][0].as_str().unwrap()];
        assert_eq!(crop["class_type"], "ImageCrop");
        assert_eq!(crop["inputs"]["x"], half_w);
        assert_eq!(crop["inputs"]["y"], 0);
        assert_eq!(crop["inputs"]["width"], half_w);
        assert_eq!(crop["inputs"]["height"], comp_h);
        assert_eq!(
            result.workflow[crop["inputs"]["image"][0].as_str().unwrap()]["class_type"],
            "VAEDecode"
        );

        // Single image: no batch repeater.
        assert_eq!(nodes_of_type(&result, "RepeatLatentBatch").len(), 0);
    }

    #[test]
    fn split_conflict_tag_stripping() {
        // "solo" contradicts the split-screen "multiple views" conditioning
        // and reliably produces a black output, so split mode drops it.
        assert_eq!(
            strip_split_conflict_tags("1girl, looking at viewer, solo, @asanagi"),
            "1girl, looking at viewer, @asanagi"
        );
        assert_eq!(strip_split_conflict_tags("solo, 1girl"), "1girl");
        assert_eq!(strip_split_conflict_tags("SOLO"), "");
        assert_eq!(strip_split_conflict_tags("1girl, (solo:1.2)"), "1girl");
        assert_eq!(strip_split_conflict_tags("1girl, (solo)"), "1girl");
        // Not stripped: tags that merely contain the word, and "solo" inside
        // a multi-tag weight group (stripping would unbalance the parens).
        assert_eq!(strip_split_conflict_tags("solo focus"), "solo focus");
        assert_eq!(
            strip_split_conflict_tags("(solo, standing:1.1)"),
            "(solo, standing:1.1)"
        );
    }

    #[test]
    fn anima_restyler_split_screen_strips_solo_from_the_prompt() {
        let mut params = anima_params();
        params.edit_split_screen = true;
        params.positive_prompt = "1girl, looking at viewer, solo, @asanagi".into();
        let result = build(&params, 42);

        let pos = &result.workflow[&result.positive_source.0];
        let text = pos["inputs"]["text"].as_str().unwrap();
        assert!(!text.contains("solo"));
        assert!(text.contains("@asanagi"));

        // Non-split mode leaves the prompt alone.
        params.edit_split_screen = false;
        let result = build(&params, 42);
        let pos = &result.workflow[&result.positive_source.0];
        assert!(pos["inputs"]["text"].as_str().unwrap().contains("solo"));
    }

    #[test]
    fn anima_restyler_split_screen_repeats_the_latent_for_batches() {
        let mut params = anima_params();
        params.edit_split_screen = true;
        params.batch_size = 4;
        let result = build(&params, 42);

        let repeats = nodes_of_type(&result, "RepeatLatentBatch");
        assert_eq!(repeats.len(), 1);
        assert_eq!(repeats[0]["inputs"]["amount"], 4);
        // The repeater reads the inpaint node's latent output (port 2), which
        // carries the noise mask along with the samples.
        let repeat_src = repeats[0]["inputs"]["samples"][0].as_str().unwrap();
        assert_eq!(
            result.workflow[repeat_src]["class_type"],
            "InpaintModelConditioning"
        );
        assert_eq!(repeats[0]["inputs"]["samples"][1], 2);
        let sampler = &result.workflow[&result.sampler_id];
        let latent_ref = sampler["inputs"]["latent_image"][0].as_str().unwrap();
        assert_eq!(
            result.workflow[latent_ref]["class_type"],
            "RepeatLatentBatch"
        );
    }

    #[test]
    fn anima_restyler_refiner_model_bypasses_the_cosmos_patch() {
        let result = build(&anima_params(), 42);

        // The Cosmos ref latent is size-locked to the base pass, so the
        // upscale/facefix/segment samplers must get a model chain without it
        // (and without the edit LoRA) or they crash on any other resolution.
        let refiner = result.refiner_model();
        assert_ne!(refiner, result.model_source);

        let mut node_id = refiner.0;
        loop {
            let node = &result.workflow[&node_id];
            assert_ne!(node["class_type"], "ApplyCosmosReferenceLatent");
            if node["class_type"] == "LoraLoaderModelOnly" {
                assert_ne!(node["inputs"]["lora_name"], ANIMA_EDIT_LORA_FILENAME);
            }
            match node["inputs"]["model"][0].as_str() {
                Some(upstream) => node_id = upstream.to_string(),
                None => break,
            }
        }
    }
}
