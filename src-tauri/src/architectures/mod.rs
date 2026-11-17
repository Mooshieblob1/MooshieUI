//! Per-architecture registry.
//!
//! Each model architecture that needs more than the generic split-model graph
//! (its own detection, text encoder, VAE, ComfyUI nodes, custom node pack or
//! workflow) declares it once, in its own file in this folder, as a
//! [`ArchitectureSpec`], and is listed in [`ALL`]. Everything else reads the
//! registry instead of growing another per-family `match`:
//!
//! - detection: `commands::api` asks [`family_from_text`] before its legacy
//!   filename / CivitAI chain, and [`family_from_tensor_architecture`] /
//!   [`architecture_class`] for structural detection;
//! - text encoder and VAE pairing: [`recommended_clip`] / [`recommended_vae`];
//! - generation: [`validate`] (modes, strict encoder, INT8-Fast), the
//!   [`LoaderFn`] hook in `templates::load_model_nodes`, the per-mode
//!   [`BuildFn`] in `templates::build_image_stage`, and the [`TextEncodeFn`]
//!   rewrite of every `CLIPTextEncode` in the finished graph;
//! - custom nodes: [`NodePack`]s are installed at ComfyUI startup next to the
//!   other pinned packs and checked before generating ([`required_node_classes`]);
//! - frontend: `list_model_architectures` serialises the registry, so labels,
//!   sampling presets, Model Hub entries, encoder / VAE auto-download and
//!   Image Edit support need no TypeScript edits for a new family.
//!
//! Families that predate the registry keep their hand-written arms; a family
//! listed here always wins, because it is consulted first.
//!
//! Compiled in both the desktop and the server build: no `tauri` here.

pub(crate) mod wan;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::comfyui::types::GenerationParams;
use crate::templates::WorkflowResult;

mod flux2_klein;
mod kolors;
mod ming_image;
mod minimax_h3_image;
mod moss_image;
mod pixart;
mod qwen_image_21;
pub mod sd3;

/// Every registered architecture. Add one line per architecture file.
pub static ALL: &[&ArchitectureSpec] = &[
    &qwen_image_21::SPEC,
    &minimax_h3_image::SPEC,
    &moss_image::SPEC,
    &ming_image::SPEC,
    &ming_image::LAYER_SPEC,
    // PixArt Alpha before Sigma: its filename markers are the narrower ones.
    &pixart::ALPHA_SPEC,
    &pixart::SPEC,
    // Distilled before Base: a purely structural match picks the first.
    &flux2_klein::SPEC_9B,
    &flux2_klein::SPEC_9B_BASE,
    &flux2_klein::SPEC_4B,
    &flux2_klein::SPEC_4B_BASE,
    &wan::SPEC,
    &kolors::SPEC,
    &sd3::SPEC,
];

/// How prompts for a family are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptStyle {
    /// Danbooru-style tags (SDXL anime models, Anima).
    Tags,
    /// Natural-language sentences (Flux, Qwen, Krea, ...).
    Natural,
}

/// A downloadable model file.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelFile {
    pub filename: &'static str,
    pub url: &'static str,
    /// ComfyUI model folder: `diffusion_models`, `text_encoders`, `vae`, `loras`, ...
    pub category: &'static str,
    /// Display size, such as `"~7.3 GB"`.
    pub size: Option<&'static str>,
    /// Lowercase hex SHA-256, checked after download when present.
    pub sha256: Option<&'static str>,
}

/// Filename / CivitAI base-model detection for one family. Text is compared
/// lowercased.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Detect {
    /// The family matches when every marker of at least one group appears.
    pub all_of: &'static [&'static [&'static str]],
    /// No match when any of these appears, even if a group matched.
    pub none_of: &'static [&'static str],
    /// Exact CivitAI `baseModel` names (lowercase) that mean this family.
    pub base_models: &'static [&'static str],
}

impl Detect {
    pub const NONE: Detect = Detect {
        all_of: &[],
        none_of: &[],
        base_models: &[],
    };

    fn matches(&self, lower: &str) -> bool {
        if self.base_models.contains(&lower) {
            return true;
        }
        if self.none_of.iter().any(|marker| lower.contains(marker)) {
            return false;
        }
        self.all_of
            .iter()
            .any(|group| !group.is_empty() && group.iter().all(|marker| lower.contains(marker)))
    }
}

/// The text encoder a family loads through `CLIPLoader` (or a loader hook).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EncoderSpec {
    /// `CLIPLoader` `type` input.
    pub clip_type: &'static str,
    /// Lowercase filename markers of compatible encoders.
    pub markers: &'static [&'static str],
    /// When true no other encoder is ever substituted, and generating with a
    /// non-matching one is refused with the download hint.
    pub strict: bool,
    /// Offered and auto-downloaded when no compatible encoder is installed.
    pub download: Option<ModelFile>,
}

/// The VAE a family decodes with.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct VaeSpec {
    /// Preferred exact file names (case-insensitive, any subfolder).
    pub basenames: &'static [&'static str],
    /// Lowercase filename markers of compatible VAEs.
    pub markers: &'static [&'static str],
    /// When true no unrelated VAE is substituted.
    pub strict: bool,
    pub download: Option<ModelFile>,
}

/// A further model file a family loads beside the diffusion model, text
/// encoder and VAE: a second text encoder, a second expert, a stage B, ...
/// The frontend picks or downloads one per slot and sends it in
/// `GenerationParams.architecture_files[key]`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelSlot {
    pub key: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub markers: &'static [&'static str],
    /// When true, generating without a matching file is refused.
    pub strict: bool,
    pub download: Option<ModelFile>,
}

/// Recommended sampler settings and base size for a family.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Sampling {
    pub steps: u32,
    pub cfg: f64,
    pub sampler: &'static str,
    pub scheduler: &'static str,
    pub width: u32,
    pub height: u32,
}

/// A Model Hub entry: the diffusion model plus the architecture's encoder,
/// VAE and slot downloads.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RecommendedModel {
    pub label: &'static str,
    pub size: &'static str,
    pub diffusion_model: ModelFile,
    /// Minimum NVIDIA compute capability, as on hand-written entries.
    pub min_compute_capability: Option<f64>,
}

/// One family (variant) of an architecture.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct FamilySpec {
    /// The `model_architecture` / model family id, unique across the app.
    pub family: &'static str,
    pub label: &'static str,
    pub detect: Detect,
    /// A distilled few-step variant.
    pub turbo: bool,
    pub sampling: Option<Sampling>,
    pub recommended: &'static [RecommendedModel],
    /// Per-family override of [`ArchitectureSpec::text_encoder`].
    pub text_encoder: Option<EncoderSpec>,
    /// Per-family override of [`ArchitectureSpec::vae`].
    pub vae: Option<VaeSpec>,
    /// A fixed sigma schedule (`ManualSigmas` text, high to low) the family
    /// was distilled for. A full-denoise main sampler then runs as
    /// `SamplerCustomAdvanced` on these sigmas instead of steps + scheduler.
    pub manual_sigmas: Option<&'static str>,
}

/// A third-party ComfyUI custom node pack, installed pinned at ComfyUI startup.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct NodePack {
    pub name: &'static str,
    pub git_url: &'static str,
    /// Full 40-character commit SHA.
    pub git_rev: &'static str,
    pub source_subdir: Option<&'static str>,
    /// Node classes that prove the pack loaded.
    pub verify_nodes: &'static [&'static str],
    pub requirements_file: &'static str,
}

/// Builds a complete dedicated graph for one mode. `finish_workflow` still
/// appends upscale, face fix, segment detail and the save node.
pub type BuildFn = fn(&GenerationParams, i64) -> WorkflowResult;

/// Model loader outputs before the LoRA chain.
pub struct LoaderOutput {
    pub model: (String, u32),
    pub clip: (String, u32),
    pub vae: (String, u32),
    pub next_id: u32,
}

/// Emits the model / text encoder / VAE loaders for a split model in place of
/// `UNETLoader` + `CLIPLoader` + `VAELoader`. The LoRA chain runs on what it
/// returns.
pub type LoaderFn = fn(&mut Map<String, Value>, u32, &GenerationParams) -> LoaderOutput;

/// Returns the node that replaces a `CLIPTextEncode` with this `clip` link and
/// prompt `text`. Its output 0 must be the conditioning.
pub type TextEncodeFn = fn(&GenerationParams, &Value, &str) -> Value;

/// A generation mode an architecture supports.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModeSpec {
    /// `txt2img`, `img2img`, `inpainting`, `image_edit`, ...
    pub mode: &'static str,
    /// Reference images the mode takes (Image Edit slots). 0 for none.
    pub max_references: u32,
    /// Files the mode loads by fixed name (an edit LoRA, ...), with downloads.
    pub required_files: &'static [ModelFile],
    /// Sampling defaults for this mode, over the family's (an edit LoRA that
    /// wants other settings than txt2img, ...).
    pub sampling: Option<Sampling>,
    /// Dedicated graph for this mode; `None` uses the generic template.
    #[serde(skip)]
    pub build: Option<BuildFn>,
}

impl ModeSpec {
    /// A mode served by the generic template.
    pub const fn generic(mode: &'static str) -> ModeSpec {
        ModeSpec {
            mode,
            max_references: 0,
            required_files: &[],
            sampling: None,
            build: None,
        }
    }
}

/// Everything MooshieUI needs to know about one architecture.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ArchitectureSpec {
    pub id: &'static str,
    /// Display name of the architecture as a whole.
    pub label: &'static str,
    pub families: &'static [FamilySpec],
    /// Never ships as a single-file checkpoint with a baked text encoder.
    pub split_only: bool,
    pub prompt_style: PromptStyle,
    /// Architecture key from structural (tensor / metadata) detection that
    /// means this architecture; it resolves to the first family.
    pub tensor_architecture: Option<&'static str>,
    pub text_encoder: Option<EncoderSpec>,
    pub vae: Option<VaeSpec>,
    pub model_slots: &'static [ModelSlot],
    pub custom_nodes: &'static [NodePack],
    /// Core ComfyUI node classes the graph needs; missing ones mean ComfyUI
    /// is older than `min_comfyui`.
    pub core_nodes: &'static [&'static str],
    pub min_comfyui: Option<&'static str>,
    /// `OTUNetLoaderW8A8` `model_type`, or `None` when INT8-Fast cannot load it.
    pub int8_fast_model_type: Option<&'static str>,
    /// Supported modes. Any other mode is refused before building.
    pub modes: &'static [ModeSpec],
    /// False when the model ignores the negative prompt (CFG 1 distilled, or a
    /// graph that zeroes it out); the prompt panel then hides that box.
    pub negative_prompt: bool,
    /// Width and height the model needs to be multiples of (8 for most
    /// models). Served to the frontend; a build hook rounds to it as well,
    /// since the size controls do not snap to it yet.
    pub resolution_multiple: u32,
    /// False when no ControlNet fits the model; requests with ControlNet on
    /// are then refused.
    pub controlnet: bool,
    #[serde(skip)]
    pub loader: Option<LoaderFn>,
    #[serde(skip)]
    pub text_encode: Option<TextEncodeFn>,
}

impl ArchitectureSpec {
    pub fn mode(&self, mode: &str) -> Option<&ModeSpec> {
        self.modes.iter().find(|m| m.mode == mode)
    }

    pub fn family(&self, family: &str) -> Option<&FamilySpec> {
        self.families.iter().find(|f| f.family == family)
    }

    fn encoder_for(&self, family: &FamilySpec) -> Option<EncoderSpec> {
        family.text_encoder.or(self.text_encoder)
    }

    fn vae_for(&self, family: &FamilySpec) -> Option<VaeSpec> {
        family.vae.or(self.vae)
    }
}

// ── Lookups (each has a registry-parameterised twin for tests) ──────────────

fn find_in<'a>(
    registry: &[&'a ArchitectureSpec],
    family: &str,
) -> Option<(&'a ArchitectureSpec, &'a FamilySpec)> {
    registry.iter().find_map(|arch| {
        arch.families
            .iter()
            .find(|f| f.family == family)
            .map(|f| (*arch, f))
    })
}

/// The architecture and family spec of a registered family.
pub fn find(family: &str) -> Option<(&'static ArchitectureSpec, &'static FamilySpec)> {
    find_in(ALL, family)
}

/// True when `family` is registered.
pub fn is_registered(family: &str) -> bool {
    find(family).is_some()
}

fn family_from_text_in(registry: &[&ArchitectureSpec], text: &str) -> Option<&'static str> {
    let lower = text.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    registry
        .iter()
        .flat_map(|arch| arch.families.iter())
        .find(|f| f.detect.matches(&lower))
        .map(|f| f.family)
}

/// Family named by a model filename or CivitAI base-model name.
pub fn family_from_text(text: &str) -> Option<&'static str> {
    family_from_text_in(ALL, text)
}

fn family_from_tensor_architecture_in(
    registry: &[&ArchitectureSpec],
    architecture: &str,
) -> Option<&'static str> {
    registry
        .iter()
        .find(|arch| arch.tensor_architecture == Some(architecture))
        .and_then(|arch| arch.families.first())
        .map(|f| f.family)
}

/// Family a structural detection result means.
pub fn family_from_tensor_architecture(architecture: &str) -> Option<&'static str> {
    family_from_tensor_architecture_in(ALL, architecture)
}

/// Structural class of a registered family (its tensor architecture, else its
/// architecture id). Families of one architecture share a class.
pub fn architecture_class(family: &str) -> Option<&'static str> {
    find(family).map(|(arch, _)| arch.tensor_architecture.unwrap_or(arch.id))
}

/// `Some(split_only)` for a registered family.
pub fn requires_separate_clip(family: &str) -> Option<bool> {
    find(family).map(|(arch, _)| arch.split_only)
}

/// `Some(prompt style)` for a registered family.
pub fn prompt_style(family: &str) -> Option<PromptStyle> {
    find(family).map(|(arch, _)| arch.prompt_style)
}

/// `Some(INT8-Fast model_type)` for a registered family; the inner `None`
/// means INT8-Fast cannot load it.
pub fn int8_fast_model_type(family: &str) -> Option<Option<&'static str>> {
    find(family).map(|(arch, _)| arch.int8_fast_model_type)
}

/// The text encoder spec of a registered family.
pub fn text_encoder(family: &str) -> Option<EncoderSpec> {
    find(family).and_then(|(arch, f)| arch.encoder_for(f))
}

fn lower_basename(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase()
}

fn encoder_matches(spec: &EncoderSpec, filename: &str) -> bool {
    let lower = filename.to_lowercase();
    spec.markers.iter().any(|marker| lower.contains(marker))
        || spec
            .download
            .is_some_and(|d| lower_basename(filename) == d.filename.to_lowercase())
}

fn vae_matches(spec: &VaeSpec, filename: &str) -> bool {
    let base = lower_basename(filename);
    let lower = filename.to_lowercase();
    spec.basenames.iter().any(|b| base == b.to_lowercase())
        || spec.markers.iter().any(|marker| lower.contains(marker))
        || spec
            .download
            .is_some_and(|d| base == d.filename.to_lowercase())
}

fn recommended_clip_in(
    registry: &[&ArchitectureSpec],
    family: &str,
    encoders: &[String],
) -> Option<(Option<String>, &'static str)> {
    let (arch, f) = find_in(registry, family)?;
    let spec = arch.encoder_for(f)?;
    let matching = encoders
        .iter()
        .filter(|e| {
            let lower = e.to_lowercase();
            lower.ends_with(".safetensors") || lower.ends_with(".gguf")
        })
        .find(|e| encoder_matches(&spec, e))
        .cloned();
    let pick = if spec.strict {
        matching
    } else {
        matching.or_else(|| encoders.first().cloned())
    };
    Some((pick, spec.clip_type))
}

/// `(recommended encoder, CLIP type)` for a registered family, in the shape of
/// `recommended_clip_from_available`: a `None` encoder means none installed is
/// compatible and nothing must be substituted.
pub fn recommended_clip(
    family: &str,
    encoders: &[String],
) -> Option<(Option<String>, &'static str)> {
    recommended_clip_in(ALL, family, encoders)
}

fn recommended_vae_in(
    registry: &[&ArchitectureSpec],
    family: &str,
    vaes: &[String],
) -> Option<Option<String>> {
    let (arch, f) = find_in(registry, family)?;
    let spec = arch.vae_for(f)?;
    let by_basename = spec.basenames.iter().find_map(|b| {
        vaes.iter()
            .find(|v| lower_basename(v) == b.to_lowercase())
            .cloned()
    });
    let matching = by_basename.or_else(|| vaes.iter().find(|v| vae_matches(&spec, v)).cloned());
    Some(if spec.strict {
        matching
    } else {
        matching.or_else(|| vaes.first().cloned())
    })
}

/// `Some(recommended VAE)` for a registered family that declares a VAE.
pub fn recommended_vae(family: &str, vaes: &[String]) -> Option<Option<String>> {
    recommended_vae_in(ALL, family, vaes)
}

// ── Generation ──────────────────────────────────────────────────────────────

fn validate_in(registry: &[&ArchitectureSpec], params: &GenerationParams) -> Result<(), String> {
    let Some((arch, family)) = find_in(registry, &params.model_architecture) else {
        return Ok(());
    };
    // Video and the standalone tools build their own graphs around no model.
    let checked_mode = !matches!(params.mode.as_str(), "video" | "upscale" | "music");
    if checked_mode && !arch.modes.is_empty() && arch.mode(&params.mode).is_none() {
        let modes: Vec<&str> = arch.modes.iter().map(|m| m.mode).collect();
        return Err(format!(
            "{} does not support {} mode. Supported: {}.",
            family.label,
            params.mode,
            modes.join(", ")
        ));
    }
    // A family with its own loader node cannot load through the checkpoint
    // loader at all. Without one, a single-file checkpoint may still work
    // (an all-in-one repack), so it is left to ComfyUI.
    if checked_mode && arch.split_only && arch.loader.is_some() && !params.use_split_model {
        return Err(format!(
            "{} loads as separate files, not as a checkpoint: put the model in models/diffusion_models and pick it there with its text encoder. The Model Hub entry downloads all of them.",
            family.label
        ));
    }
    if checked_mode && !arch.controlnet && params.controlnet.as_ref().is_some_and(|cn| cn.enabled) {
        return Err(format!(
            "ControlNet is not available for {}. Turn ControlNet off to generate with it.",
            family.label
        ));
    }
    if params.use_split_model {
        if let Some(spec) = arch.encoder_for(family) {
            if spec.strict {
                if params.clip_type.as_deref() != Some(spec.clip_type) {
                    return Err(format!(
                        "{} requires the CLIP loader type \"{}\"{}. Re-select the model so the text encoder settings refresh, or update ComfyUI.",
                        family.label,
                        spec.clip_type,
                        arch.min_comfyui
                            .map(|v| format!(" (ComfyUI {v} or newer)"))
                            .unwrap_or_default()
                    ));
                }
                let clip = params.clip_model.as_deref().map(str::trim).unwrap_or("");
                if clip.is_empty() || !encoder_matches(&spec, clip) {
                    let hint = spec
                        .download
                        .map(|d| format!(" Open the model panel to download it ({}).", d.filename))
                        .unwrap_or_default();
                    return Err(format!(
                        "{} needs its own text encoder, but {} is selected.{}",
                        family.label,
                        if clip.is_empty() {
                            "none".to_string()
                        } else {
                            format!("\"{clip}\"")
                        },
                        hint
                    ));
                }
            }
        }
        for slot in arch.model_slots.iter().filter(|s| s.strict) {
            let file = params
                .architecture_files
                .get(slot.key)
                .map(|f| f.trim())
                .unwrap_or("");
            if file.is_empty() {
                return Err(format!(
                    "{} needs a {} file. Open the model panel to pick or download one.",
                    family.label, slot.label
                ));
            }
        }
    }
    if params.int8_fast_enabled && params.use_split_model && arch.int8_fast_model_type.is_none() {
        return Err(format!(
            "{} loads without the INT8-Fast loader. Turn the INT8-Fast toggle off in the model panel and generate again.",
            family.label
        ));
    }
    if params.mode == "image_edit" {
        if let Some(mode) = arch.mode("image_edit") {
            // Slot 0 of `edit_reference_images` is the edit source itself.
            let listed = params
                .edit_reference_images
                .iter()
                .filter(|r| !r.trim().is_empty())
                .count();
            let has_input = !params
                .input_image
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty();
            let refs = if listed == 0 {
                usize::from(has_input)
            } else {
                listed
            };
            if mode.max_references > 0 && refs > mode.max_references as usize {
                return Err(format!(
                    "{} takes up to {} reference images, but {} were given.",
                    family.label, mode.max_references, refs
                ));
            }
        }
    }
    Ok(())
}

/// Registry checks for a request: supported mode, strict encoder and slots,
/// INT8-Fast and the reference count. Unregistered families pass.
pub fn validate(params: &GenerationParams) -> Result<(), String> {
    validate_in(ALL, params)
}

/// True when a registered family can run `mode` (`None` when unregistered).
pub fn supports_mode(family: &str, mode: &str) -> Option<bool> {
    find(family).map(|(arch, _)| arch.mode(mode).is_some())
}

/// The dedicated builder for this request's family and mode, if any.
pub fn builder(params: &GenerationParams) -> Option<BuildFn> {
    find(&params.model_architecture)
        .and_then(|(arch, _)| arch.mode(&params.mode))
        .and_then(|mode| mode.build)
}

/// The loader hook for this request's family, if any.
pub fn loader(params: &GenerationParams) -> Option<LoaderFn> {
    find(&params.model_architecture).and_then(|(arch, _)| arch.loader)
}

fn apply_text_encode_with(
    workflow: &mut Map<String, Value>,
    params: &GenerationParams,
    encode: TextEncodeFn,
) {
    for node in workflow.values_mut() {
        if node.get("class_type").and_then(Value::as_str) != Some("CLIPTextEncode") {
            continue;
        }
        let inputs = node.get("inputs").cloned().unwrap_or_else(|| json!({}));
        let clip = inputs.get("clip").cloned().unwrap_or(Value::Null);
        let text = inputs
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        *node = encode(params, &clip, &text);
    }
}

/// Rewrite every `CLIPTextEncode` in a finished graph through the family's
/// [`TextEncodeFn`], so every template (refiners and detailers included)
/// encodes prompts the way the architecture needs.
pub fn apply_text_encode(workflow: &mut Map<String, Value>, params: &GenerationParams) {
    if let Some(encode) = find(&params.model_architecture).and_then(|(arch, _)| arch.text_encode) {
        apply_text_encode_with(workflow, params, encode);
    }
}

/// Run the main sampler on the family's fixed sigma schedule
/// ([`FamilySpec::manual_sigmas`]). Called by `templates::build_image_stage`
/// after every injection has wired the sampler, so the model, conditioning,
/// latent and seed it ends up with carry over unchanged. Only a full-denoise
/// `KSampler` is converted: a partial denoise (img2img, inpaint) cannot keep
/// its strength on a fixed schedule, so it stays on steps + scheduler.
pub fn apply_manual_sigmas(result: &mut WorkflowResult, params: &GenerationParams) {
    let Some(sigmas) = find(&params.model_architecture).and_then(|(_, f)| f.manual_sigmas) else {
        return;
    };
    if params.stage.is_some() {
        return;
    }
    convert_to_manual_sigmas(result, sigmas);
}

fn convert_to_manual_sigmas(result: &mut WorkflowResult, sigmas: &str) {
    let Some(sampler) = result.workflow.get(&result.sampler_id) else {
        return;
    };
    if sampler.get("class_type").and_then(Value::as_str) != Some("KSampler") {
        return;
    }
    let inputs = sampler.get("inputs").cloned().unwrap_or_else(|| json!({}));
    let denoise = inputs.get("denoise").and_then(Value::as_f64).unwrap_or(1.0);
    if denoise < 0.999 {
        return;
    }

    let add = |result: &mut WorkflowResult, node: Value| -> String {
        let id = result.next_id.to_string();
        result.next_id += 1;
        result.workflow.insert(id.clone(), node);
        id
    };
    let noise = add(
        result,
        json!({"class_type": "RandomNoise", "inputs": {"noise_seed": inputs["seed"]}}),
    );
    let guider = add(
        result,
        json!({"class_type": "CFGGuider", "inputs": {
            "model": inputs["model"],
            "positive": inputs["positive"],
            "negative": inputs["negative"],
            "cfg": inputs["cfg"]
        }}),
    );
    let select = add(
        result,
        json!({"class_type": "KSamplerSelect", "inputs": {"sampler_name": inputs["sampler_name"]}}),
    );
    let schedule = add(
        result,
        json!({"class_type": "ManualSigmas", "inputs": {"sigmas": sigmas}}),
    );
    // Same node id, so everything reading the sampler's output 0 (the
    // latent) keeps working.
    result.workflow.insert(
        result.sampler_id.clone(),
        json!({"class_type": "SamplerCustomAdvanced", "inputs": {
            "noise": [noise, 0],
            "guider": [guider, 0],
            "sampler": [select, 0],
            "sigmas": [schedule, 0],
            "latent_image": inputs["latent_image"]
        }}),
    );
}

// ── Custom nodes ────────────────────────────────────────────────────────────

/// Every node pack the registered architectures need, deduplicated by URL.
pub fn node_packs() -> Vec<&'static NodePack> {
    let mut packs: Vec<&'static NodePack> = Vec::new();
    for arch in ALL {
        for pack in arch.custom_nodes {
            if !packs.iter().any(|p| p.git_url == pack.git_url) {
                packs.push(pack);
            }
        }
    }
    packs
}

/// Node classes a request needs ComfyUI to have: its packs' verify nodes and
/// its core nodes, paired with the fix to show when one is missing.
pub fn required_node_classes(params: &GenerationParams) -> Vec<(&'static str, String)> {
    let Some((arch, family)) = find(&params.model_architecture) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pack in arch.custom_nodes {
        for class in pack.verify_nodes {
            out.push((
                *class,
                format!(
                    "{} needs the {} custom node pack, which has not loaded. Restart managed ComfyUI so MooshieUI can install it, or install {} at {} on the remote server.",
                    family.label, pack.name, pack.git_url, pack.git_rev
                ),
            ));
        }
    }
    for class in arch.core_nodes {
        out.push((
            *class,
            format!(
                "{} needs ComfyUI {} or newer (node {} is missing). Update ComfyUI and restart it.",
                family.label,
                arch.min_comfyui.unwrap_or("a newer version"),
                class
            ),
        ));
    }
    out
}

/// The registry as the frontend reads it.
pub fn list() -> Vec<&'static ArchitectureSpec> {
    ALL.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_build(_: &GenerationParams, _: i64) -> WorkflowResult {
        unreachable!("not called in these tests")
    }

    fn fixture_loader(_: &mut Map<String, Value>, _: u32, _: &GenerationParams) -> LoaderOutput {
        unreachable!("not called in these tests")
    }

    fn fixture_encode(params: &GenerationParams, clip: &Value, text: &str) -> Value {
        json!({
            "class_type": "FixtureEncode",
            "inputs": { "clip": clip, "prompt": text, "width": params.width }
        })
    }

    static FIXTURE: ArchitectureSpec = ArchitectureSpec {
        id: "fixture",
        label: "Fixture",
        families: &[
            FamilySpec {
                family: "fixtureturbo",
                label: "Fixture Turbo",
                detect: Detect {
                    all_of: &[&["fixture", "turbo"]],
                    none_of: &[],
                    base_models: &[],
                },
                turbo: true,
                sampling: None,
                recommended: &[],
                text_encoder: None,
                vae: None,
                manual_sigmas: None,
            },
            FamilySpec {
                family: "fixture",
                label: "Fixture",
                detect: Detect {
                    all_of: &[&["fixture_image"], &["fixture-image"]],
                    none_of: &["notfixture"],
                    base_models: &["fixture 1.0"],
                },
                turbo: false,
                sampling: None,
                recommended: &[],
                text_encoder: None,
                vae: None,
                manual_sigmas: None,
            },
        ],
        split_only: true,
        prompt_style: PromptStyle::Natural,
        tensor_architecture: Some("fixture_dit"),
        text_encoder: Some(EncoderSpec {
            clip_type: "fixture_clip",
            markers: &["fixtenc"],
            strict: true,
            download: Some(ModelFile {
                filename: "fixtenc_8b.safetensors",
                url: "https://example.invalid/fixtenc_8b.safetensors",
                category: "text_encoders",
                size: None,
                sha256: None,
            }),
        }),
        vae: Some(VaeSpec {
            basenames: &["fixture_vae.safetensors"],
            markers: &["fixture"],
            strict: true,
            download: None,
        }),
        model_slots: &[ModelSlot {
            key: "second",
            label: "second expert",
            category: "diffusion_models",
            markers: &["second"],
            strict: true,
            download: None,
        }],
        custom_nodes: &[NodePack {
            name: "fixture-pack",
            git_url: "https://example.invalid/fixture-pack.git",
            git_rev: "0123456789abcdef0123456789abcdef01234567",
            source_subdir: None,
            verify_nodes: &["FixtureNode"],
            requirements_file: "requirements.txt",
        }],
        core_nodes: &["FixtureCore"],
        min_comfyui: Some("0.99.0"),
        int8_fast_model_type: None,
        negative_prompt: true,
        resolution_multiple: 16,
        controlnet: false,
        modes: &[
            ModeSpec::generic("txt2img"),
            ModeSpec {
                mode: "image_edit",
                max_references: 2,
                required_files: &[],
                sampling: None,
                build: Some(fixture_build),
            },
        ],
        loader: None,
        text_encode: Some(fixture_encode),
    };

    fn registry() -> [&'static ArchitectureSpec; 1] {
        [&FIXTURE]
    }

    fn params(family: &str, mode: &str) -> GenerationParams {
        let mut p = GenerationParams {
            mode: mode.to_string(),
            width: 1024,
            height: 768,
            ..Default::default()
        };
        p.model_architecture = family.to_string();
        p.use_split_model = true;
        p.clip_type = Some("fixture_clip".into());
        p.clip_model = Some("fixtenc_8b.safetensors".into());
        p.architecture_files
            .insert("second".into(), "second_expert.safetensors".into());
        p
    }

    #[test]
    fn detection_reads_groups_exclusions_and_base_models() {
        let r = registry();
        assert_eq!(
            family_from_text_in(&r, "Fixture_Image_Turbo_int8.safetensors"),
            Some("fixtureturbo")
        );
        assert_eq!(
            family_from_text_in(&r, "fixture-image-bf16.safetensors"),
            Some("fixture")
        );
        assert_eq!(family_from_text_in(&r, "Fixture 1.0"), Some("fixture"));
        assert_eq!(
            family_from_text_in(&r, "notfixture_image.safetensors"),
            None
        );
        assert_eq!(family_from_text_in(&r, "something_else.safetensors"), None);
        assert_eq!(family_from_text_in(&r, "  "), None);
        assert_eq!(
            family_from_tensor_architecture_in(&r, "fixture_dit"),
            Some("fixtureturbo")
        );
        assert_eq!(family_from_tensor_architecture_in(&r, "other"), None);
    }

    #[test]
    fn strict_encoder_and_vae_never_substitute() {
        let r = registry();
        let encoders = vec!["umt5.safetensors".to_string()];
        assert_eq!(
            recommended_clip_in(&r, "fixture", &encoders),
            Some((None, "fixture_clip"))
        );
        let encoders = vec![
            "umt5.safetensors".to_string(),
            "sub/FixtEnc_int8.safetensors".to_string(),
        ];
        assert_eq!(
            recommended_clip_in(&r, "fixture", &encoders),
            Some((Some("sub/FixtEnc_int8.safetensors".into()), "fixture_clip"))
        );
        let vaes = vec!["sdxl_vae.safetensors".to_string()];
        assert_eq!(recommended_vae_in(&r, "fixture", &vaes), Some(None));
        let vaes = vec![
            "fixture_sharp.safetensors".to_string(),
            "x/Fixture_VAE.safetensors".to_string(),
        ];
        assert_eq!(
            recommended_vae_in(&r, "fixture", &vaes),
            Some(Some("x/Fixture_VAE.safetensors".into()))
        );
        assert_eq!(recommended_vae_in(&r, "unregistered", &vaes), None);
    }

    #[test]
    fn validate_checks_mode_encoder_slots_int8_and_references() {
        let r = registry();
        assert!(validate_in(&r, &params("fixture", "txt2img")).is_ok());
        assert!(validate_in(&r, &params("unregistered", "img2img")).is_ok());

        let err = validate_in(&r, &params("fixture", "img2img")).unwrap_err();
        assert!(err.contains("does not support img2img"), "{err}");

        let mut p = params("fixture", "txt2img");
        p.clip_type = Some("wan".into());
        assert!(validate_in(&r, &p)
            .unwrap_err()
            .contains("\"fixture_clip\""));

        let mut p = params("fixture", "txt2img");
        p.clip_model = Some("umt5.safetensors".into());
        let err = validate_in(&r, &p).unwrap_err();
        assert!(err.contains("fixtenc_8b.safetensors"), "{err}");

        let mut p = params("fixture", "txt2img");
        p.architecture_files.clear();
        assert!(validate_in(&r, &p).unwrap_err().contains("second expert"));

        let mut p = params("fixture", "txt2img");
        p.use_split_model = false;
        p.architecture_files.clear();
        assert!(
            validate_in(&r, &p).is_ok(),
            "no loader hook: a checkpoint may work"
        );
        let with_loader: &'static ArchitectureSpec = Box::leak(Box::new(ArchitectureSpec {
            loader: Some(fixture_loader),
            ..FIXTURE
        }));
        assert!(validate_in(&[with_loader], &p)
            .unwrap_err()
            .contains("separate files"));

        let mut p = params("fixture", "txt2img");
        p.controlnet = Some(
            serde_json::from_value(json!({"enabled": true, "controlnet_model": null, "image": null, "preprocessor": null}))
                .unwrap(),
        );
        assert!(validate_in(&r, &p).unwrap_err().contains("ControlNet"));

        let mut p = params("fixture", "txt2img");
        p.int8_fast_enabled = true;
        assert!(validate_in(&r, &p).unwrap_err().contains("INT8-Fast"));

        let mut p = params("fixture", "image_edit");
        p.input_image = Some("a.png".into());
        p.edit_reference_images = vec!["a.png".into(), "b.png".into(), "c.png".into()];
        assert!(validate_in(&r, &p).unwrap_err().contains("up to 2"));
        p.edit_reference_images.pop();
        assert!(validate_in(&r, &p).is_ok());
    }

    #[test]
    fn text_encode_rewrites_every_clip_text_encode() {
        let p = params("fixture", "txt2img");
        let mut workflow = Map::new();
        workflow.insert(
            "1".into(),
            json!({"class_type": "CLIPTextEncode", "inputs": {"clip": ["9", 0], "text": "a cat"}}),
        );
        workflow.insert(
            "2".into(),
            json!({"class_type": "CLIPTextEncode", "inputs": {"clip": ["8", 1], "text": ""}}),
        );
        workflow.insert("3".into(), json!({"class_type": "KSampler", "inputs": {}}));
        apply_text_encode_with(&mut workflow, &p, fixture_encode);
        assert_eq!(workflow["1"]["class_type"], "FixtureEncode");
        assert_eq!(workflow["1"]["inputs"]["prompt"], "a cat");
        assert_eq!(workflow["1"]["inputs"]["clip"], json!(["9", 0]));
        assert_eq!(workflow["2"]["inputs"]["clip"], json!(["8", 1]));
        assert_eq!(workflow["2"]["inputs"]["width"], 1024);
        assert_eq!(workflow["3"]["class_type"], "KSampler");
    }

    #[test]
    fn manual_sigmas_replace_a_full_denoise_ksampler_in_place() {
        let sampler = json!({"class_type": "KSampler", "inputs": {
            "model": ["1", 0], "positive": ["2", 0], "negative": ["3", 0],
            "latent_image": ["4", 0], "seed": 7, "steps": 8, "cfg": 1.0,
            "sampler_name": "euler", "scheduler": "simple", "denoise": 1.0
        }});
        let mut workflow = Map::new();
        workflow.insert("5".into(), sampler.clone());
        let mut result = WorkflowResult {
            workflow,
            next_id: 6,
            image_output: ("6".into(), 0),
            model_source: ("1".into(), 0),
            clip_source: ("1".into(), 1),
            positive_source: ("2".into(), 0),
            negative_source: ("3".into(), 0),
            vae_source: ("1".into(), 2),
            sampler_id: "5".into(),
            refiner_model_source: None,
            base_sources: None,
        };
        convert_to_manual_sigmas(&mut result, "1.0, 0.5, 0");
        let wf = &result.workflow;
        assert_eq!(wf["5"]["class_type"], "SamplerCustomAdvanced");
        assert_eq!(wf["5"]["inputs"]["latent_image"], json!(["4", 0]));
        let guider = wf["5"]["inputs"]["guider"][0].as_str().unwrap();
        assert_eq!(wf[guider]["inputs"]["positive"], json!(["2", 0]));
        assert_eq!(wf[guider]["inputs"]["model"], json!(["1", 0]));
        let noise = wf["5"]["inputs"]["noise"][0].as_str().unwrap();
        assert_eq!(wf[noise]["inputs"]["noise_seed"], 7);
        let sigmas = wf["5"]["inputs"]["sigmas"][0].as_str().unwrap();
        assert_eq!(wf[sigmas]["inputs"]["sigmas"], "1.0, 0.5, 0");

        // A partial denoise keeps its KSampler.
        let mut partial = sampler;
        partial["inputs"]["denoise"] = json!(0.6);
        result.workflow.insert("5".into(), partial);
        convert_to_manual_sigmas(&mut result, "1.0, 0.5, 0");
        assert_eq!(result.workflow["5"]["class_type"], "KSampler");
    }

    #[test]
    fn spec_serialises_without_hooks() {
        let value = serde_json::to_value(FIXTURE).unwrap();
        assert_eq!(value["families"][0]["family"], "fixtureturbo");
        assert_eq!(value["prompt_style"], "natural");
        assert_eq!(value["modes"][1]["max_references"], 2);
        assert!(value["modes"][1].get("build").is_none());
        assert!(value.get("loader").is_none());
        assert!(value.get("text_encode").is_none());
    }

    /// The hand-written family list must stay the TypeScript one.
    #[test]
    fn legacy_families_mirror_typescript() {
        const TS: &str = include_str!("../../../src/lib/utils/modelFamily.ts");
        let start = TS
            .find("MODEL_FAMILIES = [")
            .expect("MODEL_FAMILIES in modelFamily.ts");
        let end = start
            + TS[start..]
                .find("] as const")
                .expect("end of MODEL_FAMILIES");
        let ts: Vec<&str> = TS[start..end].split('"').skip(1).step_by(2).collect();
        assert_eq!(ts, crate::commands::api::LEGACY_FAMILIES.to_vec());
    }

    /// Guards the real registry: unique family ids that never collide with a
    /// hand-written family, full-SHA pins, and https downloads.
    #[test]
    fn registered_architectures_are_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for arch in ALL {
            assert!(!arch.families.is_empty(), "{} has no families", arch.id);
            assert!(!arch.modes.is_empty(), "{} declares no modes", arch.id);
            for family in arch.families {
                assert!(
                    seen.insert(family.family),
                    "duplicate family {}",
                    family.family
                );
                assert!(
                    !crate::commands::api::is_legacy_family(family.family),
                    "{} is also a hand-written family: when moving one into the registry, drop it from LEGACY_FAMILIES in commands/api.rs",
                    family.family
                );
                for model in family.recommended {
                    assert!(model.diffusion_model.url.starts_with("https://"));
                }
            }
            for pack in arch.custom_nodes {
                assert_eq!(pack.git_rev.len(), 40, "{} rev", pack.name);
                assert!(pack.git_rev.chars().all(|c| c.is_ascii_hexdigit()));
            }
            let downloads = arch
                .text_encoder
                .and_then(|e| e.download)
                .into_iter()
                .chain(arch.vae.and_then(|v| v.download))
                .chain(arch.model_slots.iter().filter_map(|s| s.download))
                .chain(
                    arch.modes
                        .iter()
                        .flat_map(|m| m.required_files.iter().copied()),
                );
            for file in downloads {
                assert!(file.url.starts_with("https://"), "{}", file.filename);
            }
        }
    }
}
