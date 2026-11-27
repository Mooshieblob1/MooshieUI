//! The NovelAI model table and per-model capability flags.
//!
//! Capabilities are data, not branching logic, so enabling a feature when
//! NovelAI ships it is a one-line change here rather than a hunt through the
//! payload builder and the UI.

use serde::Serialize;

/// How a model expects Precise Reference (`director_reference_images`) inputs
/// to be sized before upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceCanvasMode {
    /// V4.5: fit inside 1024x1536, 1536x1024, or 1472x1472 on opaque black.
    Letterbox,
    /// V5: scale-to-cover and centre-crop onto the 97-entry aspect-ratio bucket table.
    Bucket,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct NovelAiModel {
    /// API model id.
    pub id: &'static str,
    /// Label shown in the model dropdown.
    pub label: &'static str,
    /// Model id used when `action` is `infill`. NovelAI exposes inpainting as a
    /// separate checkpoint rather than a flag.
    pub inpainting_id: Option<&'static str>,
    /// Uses the v4-style structured `v4_prompt` caption block.
    pub v4_prompt: bool,
    /// Supports Precise Reference (`director_reference_*`).
    pub precise_reference: bool,
    /// Supports Precise Reference during inpainting (`action: "infill"`).
    pub precise_reference_inpainting: bool,
    /// How Precise Reference images are fitted before encoding.
    pub precise_reference_canvas: ReferenceCanvasMode,
    /// Exposes the Fidelity slider (`director_reference_secondary_strength_values`).
    /// V5 removed Fidelity and pins secondary strength to 0 (fidelity = 1.0).
    pub precise_reference_fidelity: bool,
    /// Upper bound of the Precise Reference Strength slider in the UI (1.0 on V4.5, 5.0 on V5).
    pub precise_reference_strength_max: f64,
    /// Supports Outfit references (`"costume"`) and "Prefer same outfit" modes.
    pub outfit_references: bool,
    /// Supports vibe transfer.
    pub vibe_transfer: bool,
    /// Per-character negative prompts.
    pub character_negatives: bool,
    /// The VAE emits a real alpha channel, so transparency can be asked for.
    ///
    /// V5's custom VAE is the first to carry one. There is no request field for
    /// it: NovelAI ships the feature as a prompt tag, so this flag gates the
    /// tag rather than a parameter.
    pub alpha: bool,
    /// Quoted prompt text is auto-formatted into a trailing `Text:` block, the
    /// way NovelAI's own V5 frontend prepares text rendering. Like `alpha`,
    /// this is a prompt transform rather than a request field; see
    /// `payload::with_text_blocks`.
    pub auto_text: bool,
    /// Character slots accepted, mirroring NovelAI's own client. V5's free
    /// positioning was demonstrated with up to 22 characters; V4/V4.5 stop
    /// at 6.
    pub max_characters: usize,
    /// The distilled checkpoints NovelAI's "Medium" effort runs on, or `None`
    /// when the model has no effort toggle (everything but V5 Full).
    pub medium_effort: Option<MediumEffort>,
    /// `parameters.params_version` the model's endpoint expects. V5 speaks 4
    /// (per NovelAI's OpenAPI schema, mirrored by every V5-updated reference
    /// client); V4/V4.5 stay on 3.
    pub params_version: u8,
}

/// The model ids behind a model's Medium effort.
///
/// NovelAI ships effort as a separate checkpoint rather than a request field:
/// its client keeps "Effort: Medium / High" as a toggle next to the model and
/// swaps the id when it sends. Medium has its own inpainting checkpoint too.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MediumEffort {
    pub id: &'static str,
    pub inpainting_id: &'static str,
}

/// `NovelAiParams::effort` values.
pub const EFFORT_HIGH: &str = "high";
pub const EFFORT_MEDIUM: &str = "medium";

/// What Medium effort pins, copied from the `fixedSettings` NovelAI's client
/// applies to the medium checkpoints. The model was distilled for exactly
/// these, so the client hides the controls rather than letting them vary.
pub const MEDIUM_EFFORT_STEPS: u32 = 14;
pub const MEDIUM_EFFORT_SAMPLER: &str = "k_euler_ancestral";

/// Every NovelAI model MooshieUI offers.
///
/// V5 has Precise Reference enabled (with bucketing, Outfit references, no
/// Fidelity slider, strength up to 5.0, and no inpainting support yet); Vibe
/// Transfer remains off on V5 until NovelAI ships it.
pub const MODELS: &[NovelAiModel] = &[
    NovelAiModel {
        id: "nai-diffusion-5-full",
        label: "NovelAI V5 Full",
        inpainting_id: Some("nai-diffusion-5-full-inpainting"),
        v4_prompt: true,
        precise_reference: true,
        precise_reference_inpainting: false,
        precise_reference_canvas: ReferenceCanvasMode::Bucket,
        precise_reference_fidelity: false,
        precise_reference_strength_max: 5.0,
        outfit_references: true,
        vibe_transfer: false,
        character_negatives: true,
        alpha: true,
        auto_text: true,
        max_characters: 22,
        medium_effort: Some(MediumEffort {
            id: "nai-diffusion-5-full-medium",
            inpainting_id: "nai-diffusion-5-full-medium-inpainting",
        }),
        params_version: 4,
    },
    NovelAiModel {
        id: "nai-diffusion-5-curated",
        label: "NovelAI V5 Curated",
        // V5 Curated's own inpainting model is still training upstream, and
        // NovelAI's client substitutes V4.5 Curated's in the meantime. Point
        // back at `nai-diffusion-5-curated-inpainting` once it ships.
        inpainting_id: Some("nai-diffusion-4-5-curated-inpainting"),
        v4_prompt: true,
        precise_reference: true,
        precise_reference_inpainting: false,
        precise_reference_canvas: ReferenceCanvasMode::Bucket,
        precise_reference_fidelity: false,
        precise_reference_strength_max: 5.0,
        outfit_references: true,
        vibe_transfer: false,
        character_negatives: true,
        alpha: true,
        auto_text: true,
        max_characters: 22,
        medium_effort: None,
        params_version: 4,
    },
    NovelAiModel {
        id: "nai-diffusion-4-5-full",
        label: "NovelAI V4.5 Full",
        inpainting_id: Some("nai-diffusion-4-5-full-inpainting"),
        v4_prompt: true,
        precise_reference: true,
        precise_reference_inpainting: true,
        precise_reference_canvas: ReferenceCanvasMode::Letterbox,
        precise_reference_fidelity: true,
        precise_reference_strength_max: 1.0,
        outfit_references: false,
        vibe_transfer: true,
        character_negatives: true,
        alpha: false,
        auto_text: false,
        max_characters: 6,
        medium_effort: None,
        params_version: 3,
    },
    NovelAiModel {
        id: "nai-diffusion-4-full",
        label: "NovelAI V4 Full",
        inpainting_id: Some("nai-diffusion-4-full-inpainting"),
        v4_prompt: true,
        precise_reference: false,
        precise_reference_inpainting: false,
        precise_reference_canvas: ReferenceCanvasMode::Letterbox,
        precise_reference_fidelity: false,
        precise_reference_strength_max: 1.0,
        outfit_references: false,
        vibe_transfer: true,
        character_negatives: true,
        alpha: false,
        auto_text: false,
        max_characters: 6,
        medium_effort: None,
        params_version: 3,
    },
];

/// V4.5 Curated inpainting specification used when V5 Curated routes `infill`
/// to `nai-diffusion-4-5-curated-inpainting` while V5 Curated's own inpainting
/// checkpoint trains upstream.
pub const V45_CURATED_INPAINTING: NovelAiModel = NovelAiModel {
    id: "nai-diffusion-4-5-curated-inpainting",
    label: "NovelAI V4.5 Curated Inpainting",
    inpainting_id: Some("nai-diffusion-4-5-curated-inpainting"),
    v4_prompt: true,
    precise_reference: false,
    precise_reference_inpainting: false,
    precise_reference_canvas: ReferenceCanvasMode::Letterbox,
    precise_reference_fidelity: true,
    precise_reference_strength_max: 1.0,
    outfit_references: false,
    vibe_transfer: false,
    character_negatives: true,
    alpha: false,
    auto_text: false,
    max_characters: 6,
    medium_effort: None,
    params_version: 3,
};

/// Return the effective model specification for `action`.
///
/// While `nai-diffusion-5-curated` routes `action: "infill"` to V4.5 Curated
/// (`nai-diffusion-4-5-curated-inpainting`), payload formatting, character caps,
/// alpha/text features, and presets must match V4.5 Curated rather than V5.
pub fn effective_for_action<'a>(model: &'a NovelAiModel, action: &str) -> &'a NovelAiModel {
    if action == "infill" && model.inpainting_id == Some(V45_CURATED_INPAINTING.id) {
        &V45_CURATED_INPAINTING
    } else {
        model
    }
}

/// Look up a model by API id.
pub fn find(id: &str) -> Option<&'static NovelAiModel> {
    MODELS.iter().find(|m| m.id == id)
}

/// True when the given checkpoint string selects the NovelAI backend.
///
/// The model dropdown entry *is* the backend switch, so this is the single
/// predicate that decides which pipeline a generation takes.
pub fn is_novelai_model(id: &str) -> bool {
    find(id).is_some()
}

/// True when this request runs on the model's Medium effort checkpoint.
pub fn uses_medium_effort(model: &NovelAiModel, effort: &str) -> bool {
    model.medium_effort.is_some() && effort.trim().eq_ignore_ascii_case(EFFORT_MEDIUM)
}

/// Resolve the model id to send for a given action and effort, swapping in the
/// inpainting checkpoint when infilling and the distilled one on Medium.
pub fn resolve_id(model: &NovelAiModel, action: &str, effort: &str) -> String {
    let infill = action == "infill";
    if let Some(medium) = model
        .medium_effort
        .filter(|_| uses_medium_effort(model, effort))
    {
        return if infill {
            medium.inpainting_id
        } else {
            medium.id
        }
        .to_string();
    }
    if infill {
        model.inpainting_id.unwrap_or(model.id).to_string()
    } else {
        model.id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_is_findable_by_id() {
        for m in MODELS {
            assert_eq!(find(m.id).map(|f| f.id), Some(m.id));
        }
    }

    #[test]
    fn local_checkpoints_are_not_novelai() {
        assert!(!is_novelai_model("animaPencilXL_v500.safetensors"));
        assert!(!is_novelai_model(""));
        assert!(is_novelai_model("nai-diffusion-4-5-full"));
    }

    #[test]
    fn infill_swaps_to_the_inpainting_checkpoint() {
        let m = find("nai-diffusion-4-5-full").unwrap();
        assert_eq!(
            resolve_id(m, "generate", EFFORT_HIGH),
            "nai-diffusion-4-5-full"
        );
        assert_eq!(
            resolve_id(m, "img2img", EFFORT_HIGH),
            "nai-diffusion-4-5-full"
        );
        assert_eq!(
            resolve_id(m, "infill", EFFORT_HIGH),
            "nai-diffusion-4-5-full-inpainting"
        );
    }

    #[test]
    fn medium_effort_swaps_to_the_distilled_checkpoints() {
        let m = find("nai-diffusion-5-full").unwrap();
        assert_eq!(
            resolve_id(m, "generate", EFFORT_MEDIUM),
            "nai-diffusion-5-full-medium"
        );
        assert_eq!(
            resolve_id(m, "img2img", "Medium"),
            "nai-diffusion-5-full-medium"
        );
        assert_eq!(
            resolve_id(m, "infill", EFFORT_MEDIUM),
            "nai-diffusion-5-full-medium-inpainting"
        );
        // High, and an empty value from a client that predates the toggle.
        assert_eq!(resolve_id(m, "generate", ""), "nai-diffusion-5-full");
        assert_eq!(
            resolve_id(m, "infill", EFFORT_HIGH),
            "nai-diffusion-5-full-inpainting"
        );
    }

    #[test]
    fn only_v5_full_has_medium_effort() {
        for m in MODELS {
            let full = m.id == "nai-diffusion-5-full";
            assert_eq!(m.medium_effort.is_some(), full, "{}", m.id);
            assert_eq!(uses_medium_effort(m, EFFORT_MEDIUM), full, "{}", m.id);
            // A stale Medium on another model is ignored, not sent.
            assert_eq!(
                resolve_id(m, "generate", EFFORT_MEDIUM) == m.id,
                !full,
                "{}",
                m.id
            );
        }
    }

    #[test]
    fn v5_enables_precise_reference_while_vibe_remains_off() {
        for id in ["nai-diffusion-5-full", "nai-diffusion-5-curated"] {
            let v5 = find(id).unwrap();
            assert!(v5.precise_reference, "{id}");
            assert!(!v5.precise_reference_inpainting, "{id}");
            assert_eq!(
                v5.precise_reference_canvas,
                ReferenceCanvasMode::Bucket,
                "{id}"
            );
            assert!(!v5.precise_reference_fidelity, "{id}");
            assert_eq!(v5.precise_reference_strength_max, 5.0, "{id}");
            assert!(v5.outfit_references, "{id}");
            assert!(!v5.vibe_transfer, "{id}");
        }
        let v45 = find("nai-diffusion-4-5-full").unwrap();
        assert!(v45.precise_reference);
        assert!(v45.precise_reference_inpainting);
        assert_eq!(v45.precise_reference_canvas, ReferenceCanvasMode::Letterbox);
        assert!(v45.precise_reference_fidelity);
        assert_eq!(v45.precise_reference_strength_max, 1.0);
        assert!(!v45.outfit_references);
        assert!(v45.vibe_transfer);
    }

    #[test]
    fn only_v5_carries_an_alpha_channel() {
        assert!(find("nai-diffusion-5-full").unwrap().alpha);
        assert!(find("nai-diffusion-5-curated").unwrap().alpha);
        assert!(!find("nai-diffusion-4-5-full").unwrap().alpha);
        assert!(!find("nai-diffusion-4-full").unwrap().alpha);
    }

    #[test]
    fn v5_curated_infill_borrows_v45_curated_inpainting() {
        // Upstream substitution while V5 Curated's inpainting model trains.
        let m = find("nai-diffusion-5-curated").unwrap();
        assert_eq!(
            resolve_id(m, "generate", EFFORT_HIGH),
            "nai-diffusion-5-curated"
        );
        assert_eq!(
            resolve_id(m, "infill", EFFORT_HIGH),
            "nai-diffusion-4-5-curated-inpainting"
        );
    }

    #[test]
    fn only_v5_formats_text_blocks_and_seats_22() {
        for m in MODELS {
            let v5 = m.id.starts_with("nai-diffusion-5-");
            assert_eq!(m.auto_text, v5, "{}", m.id);
            assert_eq!(m.max_characters, if v5 { 22 } else { 6 }, "{}", m.id);
        }
    }

    #[test]
    fn v5_speaks_params_version_4() {
        for m in MODELS {
            let v5 = m.id.starts_with("nai-diffusion-5-");
            assert_eq!(m.params_version, if v5 { 4 } else { 3 }, "{}", m.id);
        }
    }

    #[test]
    fn v4_has_vibe_but_not_precise_reference() {
        let v4 = find("nai-diffusion-4-full").unwrap();
        assert!(v4.vibe_transfer);
        assert!(!v4.precise_reference);
    }

    #[test]
    fn effective_for_action_downgrades_v5_curated_during_infill_only() {
        let v5_curated = find("nai-diffusion-5-curated").unwrap();
        assert_eq!(
            effective_for_action(v5_curated, "generate").id,
            "nai-diffusion-5-curated"
        );
        assert_eq!(
            effective_for_action(v5_curated, "generate").params_version,
            4
        );
        let infill_spec = effective_for_action(v5_curated, "infill");
        assert_eq!(infill_spec.id, "nai-diffusion-4-5-curated-inpainting");
        assert_eq!(infill_spec.params_version, 3);
        assert_eq!(infill_spec.max_characters, 6);
        assert!(!infill_spec.alpha);
        assert!(!infill_spec.auto_text);
        assert!(!infill_spec.precise_reference_inpainting);
        assert!(!infill_spec.vibe_transfer);

        let v5_full = find("nai-diffusion-5-full").unwrap();
        assert_eq!(
            effective_for_action(v5_full, "infill").id,
            "nai-diffusion-5-full"
        );
        assert_eq!(effective_for_action(v5_full, "infill").params_version, 4);
    }
}
