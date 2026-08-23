//! Free local post-process for images that were generated off-box.
//!
//! A NovelAI generation is already paid for by the time it lands, so running a
//! local pass over it costs nothing but GPU time. Rather than hand-assembling a
//! second graph, this module derives a `GenerationParams` describing a
//! refine-only pass and hands it to the ordinary [`super::build_workflow`],
//! which keeps the v-pred, cascade, rectified-flow and smart-guidance
//! injections in one place.
//!
//! `refine_only` means the NovelAI image is loaded and handed straight to the
//! upscale chain, with no base sampling pass of its own. A low-denoise img2img
//! round-trip was tried in between and made the result worse, so the pass is
//! back to being what it says on the tin: an upscale, then a face fix if that
//! is on. Everything it renders with therefore comes from the upscale and
//! face-fix panels, which is where the user can see and change it.
//!
//! The two settings those panels do not own are the sampler and its guidance.
//! In NovelAI mode the sampler panel is hidden, so `params.sampler_name` is
//! whatever ComfyUI value was left behind and `params.cfg` is NovelAI's own
//! guidance scale, tuned for a model that is not the one about to sample. The
//! frontend fills `local_sampler`, `local_scheduler` and `local_cfg` from the
//! picked model's recommendation to cover that gap.

use serde_json::Value;

use crate::comfyui::types::GenerationParams;

/// Does this request ask for a local pass that can actually run?
///
/// The local checkpoint is the hard requirement: `params.checkpoint` names a
/// NovelAI model here, and ComfyUI has no such file. Without one there is
/// nothing to sample with, so the NovelAI image is delivered untouched.
pub fn is_requested(params: &GenerationParams) -> bool {
    let Some(nai) = params.novelai.as_ref() else {
        return false;
    };
    nai.local_post_process
        && nai
            .local_checkpoint
            .as_deref()
            .is_some_and(|c| !c.trim().is_empty())
        && (params.upscale_enabled || params.facefix_enabled)
}

/// Derive the refine-only parameters for the local pass.
///
/// `input_filename` is the name the NovelAI PNG was uploaded under in
/// ComfyUI's input directory, i.e. what `LoadImage` will resolve.
///
/// Returns `None` when [`is_requested`] would be false, so callers can treat
/// "no post-process" and "post-process not possible" identically.
pub fn build_params(params: &GenerationParams, input_filename: &str) -> Option<GenerationParams> {
    if !is_requested(params) {
        return None;
    }
    let nai = params.novelai.as_ref()?;

    let mut out = params.clone();

    out.mode = "img2img".to_string();
    // Refine-only: load the NovelAI image and hand it straight to the upscale
    // chain, with no base sampling pass. `out.denoise` and `out.steps` are
    // therefore never read; the upscale panel's own denoise and step count are.
    out.refine_only = true;
    out.input_image = Some(input_filename.to_string());
    // The NovelAI mask (if any) applied to NovelAI's own infill pass. The local
    // pass works on the finished image and must not be masked by it.
    out.mask_image = None;
    out.grow_mask_by = None;
    // One image, one refine pass, and no batch to spread over: MultiDiffusion
    // and the tiled VAE buy nothing here and cost tile seams. Fast refine is
    // the flag that turns both off, including for a split-file local model,
    // where the tiling gate would otherwise force them back on. The upscale
    // panel hides all three tiling controls in NovelAI mode to match.
    out.upscale_tiling = false;
    out.upscale_fast_refine = true;

    // Local model identity. `model_architecture` and `is_vpred_model` describe
    // the *sampling* model, so they have to follow the checkpoint swap or the
    // injections would be applied on the strength of the NovelAI model's
    // metadata.
    out.checkpoint = nai.local_checkpoint.clone()?;
    out.model_architecture = nai.local_architecture.clone().unwrap_or_default();
    out.is_vpred_model = nai.local_is_vpred;
    out.is_sdxl_like = matches!(
        out.model_architecture.as_str(),
        "sdxl" | "illustrious" | "noobai" | "pony" | "anima"
    );
    // Loader mode follows the file the user picked. A split-file model (Anima,
    // Flux, Chroma, ...) has no text encoder or VAE baked in, so it needs
    // UNETLoader + CLIPLoader + VAELoader; the frontend resolves the companion
    // files when the model is chosen.
    out.use_split_model = nai.local_use_split_model;
    if out.use_split_model {
        out.diffusion_model = Some(out.checkpoint.clone());
        out.clip_model = nai.local_clip_model.clone();
        out.clip_type = nai.local_clip_type.clone();
    } else {
        out.diffusion_model = None;
        out.clip_model = None;
        out.clip_type = None;
    }
    out.vae = nai.local_vae.clone().filter(|v| !v.trim().is_empty());
    // Misplaced file: a split-file model sitting in checkpoints/, or a full
    // checkpoint sitting in diffusion_models/. ComfyUI's stock loaders validate
    // the name against their own folder listing, so those load by absolute path
    // instead. `resolved_model_path` is filled in by the caller, which has the
    // config the lookup needs.
    let category = nai
        .local_model_category
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());
    out.model_source_category = match category {
        Some("checkpoints") if out.use_split_model => Some("checkpoints".to_string()),
        Some("diffusion_models") if !out.use_split_model => Some("diffusion_models".to_string()),
        _ => None,
    };
    out.resolved_model_path = None;

    // Sampler and guidance follow the local model, because nothing in the UI
    // sets them in NovelAI mode: the sampler panel is hidden, so these are
    // still the NovelAI request's values. Steps and denoise are deliberately
    // not overridden, since the upscale and face-fix panels own those and the
    // user can see them. `None` means no recommendation was known, so the
    // top-level value is left in place rather than guessed at.
    if let Some(sampler) = nai
        .local_sampler
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.sampler_name = sampler.to_string();
    }
    if let Some(scheduler) = nai
        .local_scheduler
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.scheduler = scheduler.to_string();
    }
    if let Some(cfg) = nai.local_cfg.filter(|c| *c > 0.0) {
        out.cfg = cfg;
    }

    // LoRAs, ControlNet, style transfer and `<segment:...>` refinement all
    // belong to the NovelAI request the user was composing. Carrying them into
    // the local pass would apply weights the user never asked for here, and
    // would fail outright when a LoRA does not match the refiner.
    out.loras = Vec::new();
    out.controlnet = None;
    out.style_transfer_enabled = false;
    out.detail_segments = Vec::new();
    out.positive_regions = Vec::new();
    out.positive_segments = Vec::new();
    out.negative_segments = Vec::new();

    // Optional overrides. The NovelAI syntax rewrite happens while the request
    // body is built and never touches `params`, so the top-level prompt is
    // already the ComfyUI-syntax one this pass wants. These exist only for a
    // caller that wants the local pass to run on different text.
    if let Some(pos) = nai.local_positive_prompt.clone() {
        out.positive_prompt = pos;
    }
    if let Some(neg) = nai.local_negative_prompt.clone() {
        out.negative_prompt = neg;
    }

    // One image in, one image out. The NovelAI batch has already been split
    // into individual deliveries by the time this runs.
    out.batch_size = 1;
    // The caller delivers the NovelAI image itself, before this pass is even
    // submitted, so that paid work is not lost if the local pass fails. Saving
    // the pre-upscale image here would only add a third, near-duplicate frame
    // to the gallery.
    out.save_pre_upscale_image = false;

    // Nothing downstream should re-enter the NovelAI path from these params.
    out.novelai = None;

    Some(out)
}

/// Build the ComfyUI workflow for the local pass, or `None` if it is not
/// applicable.
pub fn build(params: &GenerationParams, input_filename: &str, seed: i64) -> Option<Value> {
    let derived = build_params(params, input_filename)?;
    Some(super::build_workflow(&derived, seed, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::params::NovelAiParams;

    fn base() -> GenerationParams {
        let json = serde_json::json!({
            "mode": "txt2img",
            "positive_prompt": "1.3::masterpiece::, girl",
            "negative_prompt": "1.1::bad::",
            "checkpoint": "nai-diffusion-4-5-full",
            "vae": null,
            "loras": [],
            "sampler_name": "k_euler_ancestral",
            "scheduler": "karras",
            "steps": 23,
            "cfg": 7.0,
            "seed": "12345",
            "width": 1024,
            "height": 1024,
            "batch_size": 4,
            "denoise": 1.0,
            "input_image": null,
            "mask_image": null,
            "grow_mask_by": null,
            "upscale_enabled": true,
            "upscale_method": "model",
            "upscale_model": "4x-UltraSharp.pth",
            "upscale_scale": 2.0,
            "upscale_denoise": 0.18,
            "upscale_steps": 20,
            "upscale_tile_size": 1024,
            "upscale_tiling": true,
        });
        serde_json::from_value(json).expect("base params")
    }

    fn with_nai(nai: NovelAiParams) -> GenerationParams {
        let mut p = base();
        p.novelai = Some(nai);
        p
    }

    fn local_nai() -> NovelAiParams {
        NovelAiParams {
            model: "nai-diffusion-4-5-full".into(),
            local_post_process: true,
            local_checkpoint: Some("animaPencilXL.safetensors".into()),
            local_architecture: Some("anima".into()),
            local_sampler: Some("er_sde".into()),
            local_scheduler: Some("sgm_uniform".into()),
            local_cfg: Some(4.0),
            local_positive_prompt: Some("(masterpiece:1.3), girl".into()),
            local_negative_prompt: Some("(bad:1.1)".into()),
            ..Default::default()
        }
    }

    #[test]
    fn no_novelai_block_means_no_local_pass() {
        assert!(!is_requested(&base()));
        assert!(build_params(&base(), "nai.png").is_none());
    }

    #[test]
    fn a_local_pass_without_a_checkpoint_is_skipped() {
        // Otherwise the graph would ask ComfyUI to load "nai-diffusion-4-5-full"
        // and the user would lose the post-process to a confusing load error.
        let nai = NovelAiParams {
            local_checkpoint: None,
            ..local_nai()
        };
        assert!(!is_requested(&with_nai(nai)));
    }

    #[test]
    fn a_local_pass_with_nothing_to_do_is_skipped() {
        let mut p = with_nai(local_nai());
        p.upscale_enabled = false;
        p.facefix_enabled = false;
        assert!(!is_requested(&p));
    }

    #[test]
    fn facefix_alone_is_enough_to_run() {
        let mut p = with_nai(local_nai());
        p.upscale_enabled = false;
        p.facefix_enabled = true;
        assert!(is_requested(&p));
    }

    #[test]
    fn derived_params_describe_a_refine_only_img2img() {
        let p = with_nai(local_nai());
        let out = build_params(&p, "nai-abc.png").expect("derived");
        assert_eq!(out.mode, "img2img");
        // No base sampling pass of its own: the loaded image goes straight to
        // the upscale chain, which brings its own denoise and step count.
        assert!(out.refine_only);
        assert_eq!(out.input_image.as_deref(), Some("nai-abc.png"));
        assert_eq!(out.batch_size, 1);
    }

    #[test]
    fn derived_params_sample_with_the_local_models_recommendation() {
        // The base params carry NovelAI's own sampler and guidance, and neither
        // is a thing a ComfyUI KSampler can be handed. Step count is absent on
        // purpose: the upscale panel owns that one.
        let p = with_nai(local_nai());
        assert_eq!(p.sampler_name, "k_euler_ancestral");
        let out = build_params(&p, "nai-abc.png").expect("derived");
        assert_eq!(out.sampler_name, "er_sde");
        assert_eq!(out.scheduler, "sgm_uniform");
        assert!((out.cfg - 4.0).abs() < 1e-9);
        assert_eq!(out.steps, p.steps);
    }

    #[test]
    fn an_unknown_local_model_keeps_the_top_level_sampling() {
        // No recommendation to give: guessing would be worse than leaving the
        // settings the user can see in the sampler panel alone.
        let nai = NovelAiParams {
            local_sampler: None,
            local_scheduler: None,
            local_cfg: None,
            ..local_nai()
        };
        let p = with_nai(nai);
        let out = build_params(&p, "nai-abc.png").expect("derived");
        assert_eq!(out.sampler_name, p.sampler_name);
        assert_eq!(out.scheduler, p.scheduler);
        assert!((out.cfg - p.cfg).abs() < 1e-9);
    }

    #[test]
    fn derived_params_swap_in_the_local_model_identity() {
        let p = with_nai(local_nai());
        let out = build_params(&p, "nai-abc.png").expect("derived");
        assert_eq!(out.checkpoint, "animaPencilXL.safetensors");
        assert_eq!(out.model_architecture, "anima");
        assert!(out.is_sdxl_like);
        assert!(!out.use_split_model);
        assert!(out.novelai.is_none());
    }

    fn split_nai() -> NovelAiParams {
        NovelAiParams {
            local_checkpoint: Some("anima_pencil.safetensors".into()),
            local_model_category: Some("diffusion_models".into()),
            local_use_split_model: true,
            local_clip_model: Some("clip_l.safetensors".into()),
            local_clip_type: Some("sdxl".into()),
            local_vae: Some("sdxl_vae.safetensors".into()),
            ..local_nai()
        }
    }

    #[test]
    fn a_split_file_model_keeps_its_own_loaders() {
        let out = build_params(&with_nai(split_nai()), "nai-abc.png").expect("derived");
        assert!(out.use_split_model);
        assert_eq!(
            out.diffusion_model.as_deref(),
            Some("anima_pencil.safetensors")
        );
        assert_eq!(out.clip_model.as_deref(), Some("clip_l.safetensors"));
        assert_eq!(out.clip_type.as_deref(), Some("sdxl"));
        assert_eq!(out.vae.as_deref(), Some("sdxl_vae.safetensors"));
        // Filed where its loader expects it, so no absolute-path fallback.
        assert!(out.model_source_category.is_none());
    }

    #[test]
    fn the_split_graph_loads_the_unet_and_the_text_encoder() {
        let wf = build(&with_nai(split_nai()), "nai-abc.png", 12345).expect("workflow");
        let nodes = wf.as_object().expect("object");
        for class in ["UNETLoader", "CLIPLoader", "VAELoader"] {
            assert!(
                nodes.values().any(|n| n["class_type"] == class),
                "{class} missing from the split local pass"
            );
        }
        assert!(!nodes
            .values()
            .any(|n| n["class_type"] == "CheckpointLoaderSimple"));
    }

    #[test]
    fn a_misplaced_local_model_is_flagged_for_path_loading() {
        // A split file dropped into models/checkpoints/: the stock UNETLoader
        // validates names against its own folder listing and would reject it.
        let nai = NovelAiParams {
            local_model_category: Some("checkpoints".into()),
            ..split_nai()
        };
        let out = build_params(&with_nai(nai), "nai-abc.png").expect("derived");
        assert_eq!(out.model_source_category.as_deref(), Some("checkpoints"));
        // The caller fills this in; nothing stale may survive the clone.
        assert!(out.resolved_model_path.is_none());

        // ...and the mirror case, a full checkpoint filed under diffusion_models/.
        let nai = NovelAiParams {
            local_model_category: Some("diffusion_models".into()),
            local_use_split_model: false,
            ..split_nai()
        };
        let out = build_params(&with_nai(nai), "nai-abc.png").expect("derived");
        assert_eq!(
            out.model_source_category.as_deref(),
            Some("diffusion_models")
        );
    }

    #[test]
    fn a_plain_checkpoint_drops_the_split_companions() {
        // Switching back from a split file must not leave its text encoder
        // pointed at a checkpoint that bakes its own in.
        let nai = NovelAiParams {
            local_use_split_model: false,
            local_model_category: Some("checkpoints".into()),
            ..split_nai()
        };
        let out = build_params(&with_nai(nai), "nai-abc.png").expect("derived");
        assert!(out.diffusion_model.is_none());
        assert!(out.clip_model.is_none());
        assert!(out.clip_type.is_none());
        assert!(out.model_source_category.is_none());
    }

    #[test]
    fn derived_params_prefer_the_local_prompt_overrides() {
        // When supplied, the overrides win over the top-level prompt.
        let p = with_nai(local_nai());
        let out = build_params(&p, "nai-abc.png").expect("derived");
        assert_eq!(out.positive_prompt, "(masterpiece:1.3), girl");
        assert_eq!(out.negative_prompt, "(bad:1.1)");
    }

    #[test]
    fn derived_params_fall_back_to_the_top_level_prompt() {
        let nai = NovelAiParams {
            local_positive_prompt: None,
            local_negative_prompt: None,
            ..local_nai()
        };
        let p = with_nai(nai);
        let out = build_params(&p, "nai-abc.png").expect("derived");
        assert_eq!(out.positive_prompt, p.positive_prompt);
    }

    #[test]
    fn derived_params_drop_the_novelai_side_of_the_request() {
        let mut p = with_nai(local_nai());
        p.mask_image = Some("mask.png".into());
        p.loras = serde_json::from_value(serde_json::json!([
            { "name": "style.safetensors", "strength_model": 1.0, "strength_clip": 1.0 }
        ]))
        .expect("loras");
        let out = build_params(&p, "nai-abc.png").expect("derived");
        assert!(out.mask_image.is_none());
        assert!(out.loras.is_empty());
        assert!(out.controlnet.is_none());
        assert!(!out.style_transfer_enabled);
        assert!(out.detail_segments.is_empty());
    }

    #[test]
    fn the_graph_loads_the_uploaded_image_and_never_samples_it_twice() {
        let p = with_nai(local_nai());
        let wf = build(&p, "nai-abc.png", 12345).expect("workflow");
        let nodes = wf.as_object().expect("object");

        let load = nodes
            .values()
            .find(|n| n["class_type"] == "LoadImage")
            .expect("LoadImage present");
        assert_eq!(load["inputs"]["image"], "nai-abc.png");

        // Refine-only means exactly one sampling pass (the upscale refiner). A
        // second KSampler would mean the base img2img round-trip crept back in
        // and the paid image got re-denoised at full strength. Counting
        // VAEEncode nodes cannot say this: the upscale chain encodes the
        // upscaled pixels itself, so one is expected either way.
        let samplers = nodes
            .values()
            .filter(|n| n["class_type"] == "KSampler")
            .count();
        assert_eq!(samplers, 1, "refine-only must sample exactly once");

        // MultiDiffusion and the tiled VAE are off for the local pass: one
        // image, one refine, nothing to gain and tile seams to lose.
        for class in ["ApplyTiledDiffusion", "VAEEncodeTiled", "VAEDecodeTiled"] {
            assert!(
                !nodes.values().any(|n| n["class_type"] == class),
                "{class} must not appear in the local pass"
            );
        }

        assert!(nodes
            .values()
            .any(|n| n["class_type"] == "MooshieSaveImage"));
    }

    #[test]
    fn the_refine_keeps_the_models_full_cfg() {
        // The upscale chain normally halves CFG because the base sampling pass
        // already applied the model's full guidance. Refine-only has no base
        // pass, so this KSampler is the only guidance the image ever gets, and
        // halving it under-guides the refine and leaves the upscaler's noise in.
        let p = with_nai(local_nai());
        let wf = build(&p, "nai-abc.png", 12345).expect("workflow");
        let nodes = wf.as_object().expect("object");

        let samplers: Vec<_> = nodes
            .values()
            .filter(|n| n["class_type"] == "KSampler")
            .collect();
        assert_eq!(samplers.len(), 1, "refine-only samples exactly once");
        let cfg = samplers[0]["inputs"]["cfg"].as_f64().expect("cfg");
        assert!(
            (cfg - 4.0).abs() < 1e-9,
            "expected Anima's own CFG, got {cfg}"
        );
    }

    #[test]
    fn a_request_with_no_local_pass_builds_no_graph() {
        assert!(build(&base(), "nai-abc.png", 1).is_none());
    }
}
