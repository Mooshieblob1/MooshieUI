use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LlmVariant {
    /// Weight format. Currently always "gguf".
    pub format: String,
    /// Quant label for GGUF (e.g. "Q4_K_M").
    pub quant: Option<String>,
    /// On-disk size estimate (MB).
    pub size_mb: u64,
    /// VRAM needed to run fully offloaded (MB). Used for fit/dimming.
    pub vram_mb: u64,
    /// HuggingFace repo id.
    pub repo: String,
    /// File name within the repo.
    pub file: String,
    /// The vision projector a model that can see images loads beside `file`,
    /// from the same repo. `None` for text-only models.
    pub mmproj: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmCatalogEntry {
    pub id: String,
    pub name: String,
    /// "tag_upsampler", "natural_language", or "captioner" (describes images
    /// and nothing else, so it never serves as the prompt assistant model).
    pub purpose: String,
    /// Model families this entry serves well (matches generation.modelFamily values),
    /// or ["*"] for any family.
    pub families: Vec<String>,
    pub variants: Vec<LlmVariant>,
    pub pros: String,
    pub cons: String,
    pub best_for: String,
}

fn gguf(quant: &str, size_mb: u64, vram_mb: u64, repo: &str, file: &str) -> LlmVariant {
    LlmVariant {
        format: "gguf".into(),
        quant: Some(quant.into()),
        size_mb,
        vram_mb,
        repo: repo.into(),
        file: file.into(),
        mmproj: None,
    }
}

/// A GGUF variant that sees images through `mmproj`. `size_mb` and `vram_mb`
/// cover both files.
fn gguf_vision(
    quant: &str,
    size_mb: u64,
    vram_mb: u64,
    repo: &str,
    file: &str,
    mmproj: &str,
) -> LlmVariant {
    LlmVariant {
        mmproj: Some(mmproj.into()),
        ..gguf(quant, size_mb, vram_mb, repo, file)
    }
}

impl LlmCatalogEntry {
    /// Whether the model reads images (every variant ships a projector).
    pub fn has_vision(&self) -> bool {
        !self.variants.is_empty() && self.variants.iter().all(|v| v.mmproj.is_some())
    }
}

/// The curated v1 catalog. Repos/files are PINNED here — update this list to
/// change available models. Sizes/vram are conservative estimates for fit logic.
pub fn catalog() -> Vec<LlmCatalogEntry> {
    vec![
        // Tiny — purpose-built danbooru tag upsampler. CPU-friendly.
        LlmCatalogEntry {
            id: "dantaggen-l".into(),
            name: "DanTagGen-delta (Large)".into(),
            purpose: "tag_upsampler".into(),
            families: vec![
                "illustrious".into(),
                "pony".into(),
                "nanosaur".into(),
                "anima".into(),
            ],
            variants: vec![gguf(
                "Q8",
                420,
                700,
                "KBlueLeaf/DanTagGen-delta",
                "ggml-model-Q8_0.gguf",
            )],
            pros: "Tiny, fast, purpose-built for danbooru tags; runs on CPU.".into(),
            cons: "Tags only — cannot write natural-language prose.".into(),
            best_for: "Expanding a few tags into a fuller tag prompt.".into(),
        },
        // Small — modern non-thinking instruct, natural language + tags. Laptop default.
        LlmCatalogEntry {
            id: "qwen3-4b-instruct".into(),
            name: "Qwen3 4B Instruct".into(),
            purpose: "natural_language".into(),
            families: vec!["*".into()],
            variants: vec![
                // bartowski hosts single-file GGUFs of Qwen3-4B-Instruct-2507 (the
                // non-thinking "2507" instruct — no <think> blocks, ideal for prompt
                // generation). Qwen's own repo shards these into multi-part files.
                gguf(
                    "Q4_K_M",
                    2500,
                    3600,
                    "bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF",
                    "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
                ),
                gguf(
                    "Q5_K_M",
                    2900,
                    4200,
                    "bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF",
                    "Qwen_Qwen3-4B-Instruct-2507-Q5_K_M.gguf",
                ),
                // Q8_0 — near-lossless 8-bit (the highest-fidelity GGUF tier;
                // llama.cpp has no true FP8 inference type, so Q8_0 is the 8-bit option).
                gguf(
                    "Q8_0",
                    4100,
                    5500,
                    "bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF",
                    "Qwen_Qwen3-4B-Instruct-2507-Q8_0.gguf",
                ),
            ],
            pros: "Modern non-thinking instruct; strong natural language + tags; 4-6 GB VRAM friendly."
                .into(),
            cons: "Less nuanced than 7B+ models.".into(),
            best_for: "Laptops / 6-8 GB GPUs; Anima natural-language prompts.".into(),
        },
        // Medium — higher quality natural-language compose/enhance.
        LlmCatalogEntry {
            id: "qwen25-7b-instruct".into(),
            name: "Qwen2.5 7B Instruct".into(),
            purpose: "natural_language".into(),
            families: vec!["*".into()],
            variants: vec![
                // Qwen's own 7B GGUF repo shards Q4_K_M/Q5_K_M into multi-part
                // files, which the single-file downloader cannot fetch (404).
                // bartowski hosts single-file builds of the same quants.
                gguf(
                    "Q4_K_M",
                    4700,
                    6500,
                    "bartowski/Qwen2.5-7B-Instruct-GGUF",
                    "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
                ),
                gguf(
                    "Q5_K_M",
                    5400,
                    7300,
                    "bartowski/Qwen2.5-7B-Instruct-GGUF",
                    "Qwen2.5-7B-Instruct-Q5_K_M.gguf",
                ),
                // Q8_0 — near-lossless 8-bit, for >=12 GB GPUs that want top quality.
                gguf(
                    "Q8_0",
                    7800,
                    9700,
                    "bartowski/Qwen2.5-7B-Instruct-GGUF",
                    "Qwen2.5-7B-Instruct-Q8_0.gguf",
                ),
            ],
            pros: "High-quality compose/enhance; best prose of the GGUF lineup.".into(),
            cons: "Needs ~6-7 GB VRAM; slow on CPU.".into(),
            best_for: ">=12 GB GPUs; best compose quality.".into(),
        },
        // Small vision model: writes prompts like Qwen3 4B and also reads
        // images, so Describe in Interrogate and reference images in Enhance
        // work without an API. Qwen's own single-file GGUFs plus projector.
        LlmCatalogEntry {
            id: "qwen3-vl-4b-instruct".into(),
            name: "Qwen3-VL 4B Instruct".into(),
            purpose: "natural_language".into(),
            families: vec!["*".into()],
            variants: vec![
                gguf_vision(
                    "Q4_K_M",
                    2950,
                    4300,
                    "Qwen/Qwen3-VL-4B-Instruct-GGUF",
                    "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
                    "mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf",
                ),
                gguf_vision(
                    "Q8_0",
                    4750,
                    6100,
                    "Qwen/Qwen3-VL-4B-Instruct-GGUF",
                    "Qwen3VL-4B-Instruct-Q8_0.gguf",
                    "mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf",
                ),
            ],
            pros: "Sees images: describes them in Interrogate and reads reference images in Enhance."
                .into(),
            cons: "Prose a little behind Qwen2.5 7B; may tone down explicit images.".into(),
            best_for: "Describing images and prompting from references without an API.".into(),
        },
        // Captioner only: trained to caption images for diffusion training,
        // including explicit ones. Never offered as the prompt assistant model.
        LlmCatalogEntry {
            id: "joycaption-beta-one".into(),
            name: "JoyCaption Beta One".into(),
            purpose: "captioner".into(),
            families: vec!["*".into()],
            variants: vec![gguf_vision(
                "Q4_K",
                5800,
                7200,
                "concedo/llama-joycaption-beta-one-hf-llava-mmproj-gguf",
                "Llama-Joycaption-Beta-One-Hf-Llava-Q4_K.gguf",
                "llama-joycaption-beta-one-llava-mmproj-model-f16.gguf",
            )],
            pros: "Made for captioning images for diffusion models; detailed and uncensored."
                .into(),
            cons: "Only describes images, it can't enhance prompts; ~6 GB download.".into(),
            best_for: "The most faithful image descriptions.".into(),
        },
    ]
}

/// Look up a catalog entry by id.
pub fn entry(id: &str) -> Option<LlmCatalogEntry> {
    catalog().into_iter().find(|e| e.id == id)
}

/// Pick the largest-footprint GGUF variant a host can run (best quality that
/// fits). Returns None if no variant fits.
/// The context every `vram_mb` above was measured at (llama.cpp's default).
const MEASURED_CONTEXT_TOKENS: u64 = 4096;

/// VRAM the f16 KV cache needs beyond what `vram_mb` already covers, when the
/// server runs with `context_tokens` instead of the measured 4096.
///
/// Per-token cost is 2 (K and V) x layers x KV heads x head dim x 2 bytes.
/// Unknown models get the larger figure: underestimating here is what pushes
/// ComfyUI's weights out of VRAM.
pub fn context_overhead_mb(model_id: &str, context_tokens: u32) -> u64 {
    let bytes_per_token: u64 = match model_id {
        "qwen25-7b-instruct" => 2 * 28 * 4 * 128 * 2,
        "joycaption-beta-one" => 2 * 32 * 8 * 128 * 2, // Llama 3.1 8B
        _ => 2 * 36 * 8 * 128 * 2,                     // Qwen3-4B, and the fallback
    };
    let extra_tokens = u64::from(context_tokens).saturating_sub(MEASURED_CONTEXT_TOKENS);
    bytes_per_token * extra_tokens / (1024 * 1024)
}

pub fn best_variant_for(entry: &LlmCatalogEntry, available_vram_mb: u64) -> Option<&LlmVariant> {
    // Prefer the largest variant that fits with the session context's KV
    // cache, the budget `ensure_running` gates GPU offload on. When none does,
    // keep the old answer: that model still runs, on CPU, and a natural
    // language model on CPU beats falling back to the tag-only upsampler.
    let overhead = context_overhead_mb(&entry.id, super::server::LOCAL_CONTEXT_TOKENS);
    let largest_within = |budget: u64| {
        entry
            .variants
            .iter()
            .filter(|v| v.format == "gguf" && v.vram_mb <= budget)
            .max_by_key(|v| v.vram_mb)
    };
    largest_within(available_vram_mb.saturating_sub(overhead))
        .or_else(|| largest_within(available_vram_mb))
}

/// Recommend the best catalog model id for the detected hardware:
/// the largest natural-language model that fits comfortably, else the tiny
/// tag upsampler (always runnable on CPU/RAM).
///
/// "available" is GPU VRAM when a GPU is present, otherwise a fraction of system RAM.
pub fn recommend_model_id(total_vram_mb: u64, system_ram_mb: u64) -> String {
    // CPU path: use ~60% of system RAM as a safe working budget.
    let available = if total_vram_mb >= 2000 {
        total_vram_mb
    } else {
        (system_ram_mb as f64 * 0.6) as u64
    };

    let cat = catalog();
    // Prefer natural-language models, largest that fits.
    let nl_pick = cat
        .iter()
        .filter(|e| e.purpose == "natural_language")
        // A vision model is a deliberate pick for its images, not a default:
        // its projector costs VRAM a text-only prompt never uses.
        .filter(|e| !e.has_vision())
        .filter(|e| best_variant_for(e, available).is_some())
        .max_by_key(|e| {
            best_variant_for(e, available)
                .map(|v| v.vram_mb)
                .unwrap_or(0)
        });
    if let Some(e) = nl_pick {
        return e.id.clone();
    }
    // Fallback: the tiny tag upsampler (id known-present in the catalog).
    "dantaggen-l".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_vram_picks_7b_gguf() {
        // High-VRAM host: pick the best-fitting GGUF of the largest NL model.
        let id = recommend_model_id(32000, 65536);
        assert_eq!(id, "qwen25-7b-instruct");
        let e = entry(&id).unwrap();
        let v = best_variant_for(&e, 32000).unwrap();
        assert_eq!(v.format, "gguf");
        // Q8_0 is the highest tier and fits comfortably here.
        assert_eq!(v.quant.as_deref(), Some("Q8_0"));
    }

    #[test]
    fn midrange_gpu_picks_largest_fitting_gguf() {
        // 8 GB GPU → 7B Q5 needs 7.3 GB → fits; should pick 7B.
        let id = recommend_model_id(8000, 32768);
        assert_eq!(id, "qwen25-7b-instruct");
    }

    #[test]
    fn small_gpu_picks_4b() {
        // 4 GB GPU → 7B does not fit, Qwen3-4B Q4 (3.6 GB) fits.
        let id = recommend_model_id(4000, 16384);
        assert_eq!(id, "qwen3-4b-instruct");
    }

    #[test]
    fn cpu_only_low_ram_falls_back_to_tiny() {
        // No GPU, 4 GB RAM → 60% = 2.4 GB, Qwen3-4B Q4 needs 3.6 GB → no NL fits → tiny.
        let id = recommend_model_id(0, 4096);
        assert_eq!(id, "dantaggen-l");
    }

    #[test]
    fn recommendations_leave_room_for_the_larger_context() {
        // 6 GB card: Q8_0 (5500) only fit before the 16k context's extra
        // 1728 MB; Q5_K_M (4200 + 1728) is the largest that still fits.
        let e = entry("qwen3-4b-instruct").unwrap();
        let v = best_variant_for(&e, 6000).unwrap();
        assert_eq!(v.quant.as_deref(), Some("Q5_K_M"));
    }

    #[test]
    fn context_overhead_counts_the_cache_beyond_the_measured_4k() {
        // f16 KV cache: Qwen3-4B is 144 KiB/token (576 MiB at 4096, as logged),
        // so 16384 costs three more 4096 slices: 1728 MiB.
        assert_eq!(context_overhead_mb("qwen3-4b-instruct", 16384), 1728);
        // Qwen2.5-7B: 56 KiB/token, 224 MiB at 4096.
        assert_eq!(context_overhead_mb("qwen25-7b-instruct", 16384), 672);
        // No overhead at the context the catalog sizes were measured at.
        assert_eq!(context_overhead_mb("qwen3-4b-instruct", 4096), 0);
        // Unknown models get the larger per-token cost, never a smaller one.
        assert_eq!(context_overhead_mb("something-new", 16384), 1728);
    }

    #[test]
    fn vision_models_ship_a_projector_and_are_never_the_default() {
        let vision: Vec<_> = catalog().into_iter().filter(|e| e.has_vision()).collect();
        assert!(vision.iter().any(|e| e.id == "qwen3-vl-4b-instruct"));
        assert!(vision.iter().any(|e| e.id == "joycaption-beta-one"));
        for e in &vision {
            assert!(e.variants.iter().all(|v| v.mmproj.is_some()), "{}", e.id);
        }
        for (vram, ram) in [
            (4000, 16000),
            (6000, 32000),
            (8000, 32768),
            (32000, 65536),
            (0, 8000),
        ] {
            let id = recommend_model_id(vram, ram);
            assert!(
                !entry(&id).unwrap().has_vision(),
                "{vram}/{ram} picked {id}"
            );
        }
        // Text-only models carry no projector.
        assert!(!entry("qwen3-4b-instruct").unwrap().has_vision());
    }
}
