use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

/// Raw corpus entry from anima-tags.json.
#[derive(Debug, Deserialize)]
struct RawTag {
    n: String,
    c: i8,
    #[serde(default)]
    a: Vec<String>,
}

pub struct Corpus {
    /// Canonical general/character/copyright tag names (underscored form).
    pub tags: HashSet<String>,
    /// Canonical artist names (underscored form, category 1).
    pub artists: HashSet<String>,
    /// alias (underscored) → canonical name, for snapping near-misses.
    pub alias_to_canonical: HashMap<String, String>,
}

static CORPUS: OnceLock<Corpus> = OnceLock::new();

// Baked into the binary so it works identically in desktop, browser, and server modes.
const ANIMA_TAGS_JSON: &str = include_str!("../../../src/lib/assets/anima-tags.json");

pub fn corpus() -> &'static Corpus {
    CORPUS.get_or_init(|| {
        let raw: Vec<RawTag> = serde_json::from_str(ANIMA_TAGS_JSON).unwrap_or_default();
        let mut tags = HashSet::new();
        let mut artists = HashSet::new();
        let mut alias_to_canonical = HashMap::new();
        for t in raw {
            let canon = normalize(&t.n);
            match t.c {
                1 => {
                    artists.insert(canon.clone());
                }
                // general, copyright, character — all valid danbooru tags
                0 | 3 | 4 => {
                    tags.insert(canon.clone());
                }
                _ => {} // meta (5), unknown (-1), etc.
            }
            for alias in t.a {
                alias_to_canonical.insert(normalize(&alias), canon.clone());
            }
        }
        Corpus {
            tags,
            artists,
            alias_to_canonical,
        }
    })
}

/// Lowercase, trim, collapse whitespace to single underscores (danbooru canonical form).
pub fn normalize(s: &str) -> String {
    s.trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

/// Convert a canonical underscored tag to display form (spaces, escaped parens
/// left intact for prompt usage).
fn to_display(tag: &str) -> String {
    tag.replace('_', " ")
}

/// Resolve a single raw token to a canonical tag if recognized (exact or alias).
fn resolve_tag(token: &str) -> Option<String> {
    let n = normalize(token);
    let c = corpus();
    if c.tags.contains(&n) {
        Some(n)
    } else {
        c.alias_to_canonical
            .get(&n)
            .filter(|canon| c.tags.contains(*canon))
            .cloned()
    }
}

/// Resolve a token to a canonical artist if recognized.
fn resolve_artist(token: &str) -> Option<String> {
    let n = normalize(token.trim_start_matches('@'));
    let c = corpus();
    if c.artists.contains(&n) {
        Some(n)
    } else {
        c.alias_to_canonical
            .get(&n)
            .filter(|canon| c.artists.contains(*canon))
            .cloned()
    }
}

/// Retrieve up to `limit` candidate tags that share a token with the input,
/// to seed the system prompt (lexical grounding). Results are sorted so the
/// selection is deterministic regardless of HashSet iteration order.
pub fn retrieve_candidates(input: &str, limit: usize) -> Vec<String> {
    let c = corpus();
    let input_tokens: HashSet<String> = input
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect();
    let mut matches: Vec<&String> = c
        .tags
        .iter()
        .filter(|tag| {
            tag.split('_')
                .any(|part| part.len() > 2 && input_tokens.contains(part))
        })
        .collect();
    matches.sort();
    matches.truncate(limit);
    matches.iter().map(|tag| to_display(tag)).collect()
}

/// Whether a family uses tag-only prompting (vs Anima natural language).
pub fn is_tag_only_family(family: &str) -> bool {
    !matches!(family, "anima")
}

/// Whether grounding should run in tag-only mode. A purpose-built tag upsampler
/// (e.g. DanTagGen) is always tag-only regardless of family; otherwise the
/// family decides (everything except Anima is tag-only).
pub fn is_tag_only(purpose: &str, family: &str) -> bool {
    purpose == "tag_upsampler" || is_tag_only_family(family)
}

/// Build the system prompt, seeded with grounding candidates. `tag_only`
/// selects between danbooru-tag and Anima natural-language conventions.
pub fn system_prompt(tag_only: bool, mode: GenMode, candidates: &[String]) -> String {
    let cand = if candidates.is_empty() {
        String::new()
    } else {
        format!(
            "\nRelevant known tags you may draw from: {}.",
            candidates.join(", ")
        )
    };
    if tag_only {
        let verb = match mode {
            GenMode::Enhance => "Expand and enrich the user's danbooru tag list",
            GenMode::Compose => "Convert the user's description into a danbooru tag list",
        };
        format!(
            "You are a danbooru tag prompt writer for an anime image generator. \
{verb}. Output ONLY a comma-separated list of lowercase danbooru tags. \
No sentences, no explanations, no quotes, no numbering. Keep existing tags. \
Prefer concrete, well-known tags.{cand}"
        )
    } else {
        // Anima: natural language + Gelbooru tags + @artist
        let verb = match mode {
            GenMode::Enhance => "Enhance the user's prompt",
            GenMode::Compose => "Write a prompt from the user's description",
        };
        format!(
            "You are a prompt writer for the Anima anime image model. {verb}. \
Use a short natural-language description followed by relevant Gelbooru-style tags. \
Reference known artists only as @artist_name. No explanations or quotes.{cand}"
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenMode {
    Enhance,
    Compose,
}

/// Post-filter repair of raw model output. Validates/repairs against the corpus
/// and enforces the active conventions. Returns a cleaned prompt string (possibly
/// empty if nothing survived — caller keeps the original prompt in that case).
/// `tag_only` selects danbooru-tag vs Anima natural-language repair.
pub fn repair(raw: &str, tag_only: bool) -> String {
    if tag_only {
        repair_tag_only(raw)
    } else {
        repair_anima(raw)
    }
}

/// Tag-only: split on commas, drop prose, validate/snap each tag, dedupe.
fn repair_tag_only(raw: &str) -> String {
    let mut seen = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for chunk in raw.split(',') {
        let token = chunk
            .trim()
            .trim_matches(|c| c == '.' || c == '"' || c == '\'');
        if token.is_empty() {
            continue;
        }
        // Drop obvious prose: a chunk with >4 words is a sentence, not a tag.
        if token.split_whitespace().count() > 4 {
            continue;
        }
        if let Some(canon) = resolve_tag(token) {
            let display = to_display(&canon);
            if seen.insert(display.clone()) {
                out.push(display);
            }
        }
        // Unrecognized tokens are dropped (hallucination guard).
    }
    out.join(", ")
}

/// Anima: keep natural-language clauses + Gelbooru tags; force recognized
/// artists into @artist form.
fn repair_anima(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for chunk in raw.split(',') {
        let token = chunk.trim();
        if token.is_empty() {
            continue;
        }
        // Already an @artist reference — validate it.
        if let Some(rest) = token.strip_prefix('@') {
            if let Some(canon) = resolve_artist(rest) {
                let formatted = format!("@{}", to_display(&canon).replace(' ', "_"));
                if seen.insert(formatted.clone()) {
                    out.push(formatted);
                }
            }
            continue;
        }
        // A bare recognized artist → promote to @artist.
        if let Some(canon) = resolve_artist(token) {
            let formatted = format!("@{}", to_display(&canon).replace(' ', "_"));
            if seen.insert(formatted.clone()) {
                out.push(formatted);
            }
            continue;
        }
        // Otherwise keep the clause/tag as-is (natural language allowed).
        let cleaned = token.trim_matches('"').trim().to_string();
        if !cleaned.is_empty() && seen.insert(cleaned.clone()) {
            out.push(cleaned);
        }
    }
    out.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_loads_known_tags() {
        let c = corpus();
        assert!(c.tags.contains("1girl"), "expected 1girl in corpus");
        assert!(!c.tags.is_empty());
    }

    #[test]
    fn tag_only_drops_prose_and_unknowns() {
        // "1girl" valid; the sentence is prose (>4 words) → dropped;
        // "zzzznotarealtag" unknown → dropped.
        let out = repair_tag_only("1girl, this is clearly a long sentence, zzzznotarealtag, solo");
        assert_eq!(out, "1girl, solo");
    }

    #[test]
    fn tag_only_snaps_alias() {
        // "1_girl" is an alias of "1girl".
        let out = repair_tag_only("1_girl");
        assert_eq!(out, "1girl");
    }

    #[test]
    fn tag_only_dedupes() {
        let out = repair_tag_only("solo, solo, 1girl");
        assert_eq!(out, "solo, 1girl");
    }

    #[test]
    fn anima_keeps_prose_clauses() {
        let out = repair_anima("a serene forest at dawn, 1girl, soft lighting");
        assert!(out.contains("a serene forest at dawn"));
        assert!(out.contains("1girl"));
    }

    #[test]
    fn is_tag_only_routes_by_purpose_and_family() {
        // Tag upsampler is always tag-only, even on Anima.
        assert!(is_tag_only("tag_upsampler", "anima"));
        // Natural-language model on Anima uses prose mode.
        assert!(!is_tag_only("natural_language", "anima"));
        // Natural-language model on a non-Anima family stays tag-only.
        assert!(is_tag_only("natural_language", "illustrious"));
    }

    #[test]
    fn retrieve_candidates_is_deterministic() {
        let a = retrieve_candidates("1girl solo", 10);
        let b = retrieve_candidates("1girl solo", 10);
        assert_eq!(a, b);
        // Sorted ascending.
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(a, sorted);
    }
}
