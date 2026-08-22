//! The NovelAI model table and per-model capability flags.
//!
//! Capabilities are data, not branching logic, so enabling a feature when
//! NovelAI ships it is a one-line change here rather than a hunt through the
//! payload builder and the UI.

use serde::Serialize;

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
    /// Supports vibe transfer.
    pub vibe_transfer: bool,
    /// Per-character negative prompts.
    pub character_negatives: bool,
}

/// Every NovelAI model MooshieUI offers.
///
/// V5 capability flags are false because NovelAI has not shipped Precise
/// Reference or vibe transfer for V5 yet. The UI still builds both panels and
/// renders them disabled, so turning them on is a two-boolean change.
pub const MODELS: &[NovelAiModel] = &[
    NovelAiModel {
        id: "nai-diffusion-5-full",
        label: "NovelAI V5 Full",
        inpainting_id: Some("nai-diffusion-5-full-inpainting"),
        v4_prompt: true,
        precise_reference: false,
        vibe_transfer: false,
        character_negatives: true,
    },
    NovelAiModel {
        id: "nai-diffusion-5-curated",
        label: "NovelAI V5 Curated",
        inpainting_id: Some("nai-diffusion-5-curated-inpainting"),
        v4_prompt: true,
        precise_reference: false,
        vibe_transfer: false,
        character_negatives: true,
    },
    NovelAiModel {
        id: "nai-diffusion-4-5-full",
        label: "NovelAI V4.5 Full",
        inpainting_id: Some("nai-diffusion-4-5-full-inpainting"),
        v4_prompt: true,
        precise_reference: true,
        vibe_transfer: true,
        character_negatives: true,
    },
    NovelAiModel {
        id: "nai-diffusion-4-full",
        label: "NovelAI V4 Full",
        inpainting_id: Some("nai-diffusion-4-full-inpainting"),
        v4_prompt: true,
        precise_reference: false,
        vibe_transfer: true,
        character_negatives: true,
    },
];

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

/// Resolve the model id to send for a given action, swapping in the inpainting
/// checkpoint when infilling.
pub fn resolve_id(model: &NovelAiModel, action: &str) -> String {
    if action == "infill" {
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
        assert_eq!(resolve_id(m, "generate"), "nai-diffusion-4-5-full");
        assert_eq!(resolve_id(m, "img2img"), "nai-diffusion-4-5-full");
        assert_eq!(resolve_id(m, "infill"), "nai-diffusion-4-5-full-inpainting");
    }

    #[test]
    fn v5_has_reference_features_wired_but_off() {
        let v5 = find("nai-diffusion-5-full").unwrap();
        assert!(!v5.precise_reference);
        assert!(!v5.vibe_transfer);
        let v45 = find("nai-diffusion-4-5-full").unwrap();
        assert!(v45.precise_reference);
        assert!(v45.vibe_transfer);
    }

    #[test]
    fn v4_has_vibe_but_not_precise_reference() {
        let v4 = find("nai-diffusion-4-full").unwrap();
        assert!(v4.vibe_transfer);
        assert!(!v4.precise_reference);
    }
}
