use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

/// Raw corpus entry from anima-tags.json.
#[derive(Debug, Deserialize)]
struct RawTag {
    n: String,
    c: i8,
    /// Post count — proxy for how well-known the tag is to the Anima model.
    #[serde(default)]
    p: i64,
    #[serde(default)]
    a: Vec<String>,
}

pub struct Corpus {
    /// Canonical general/character/copyright tag names (underscored form) →
    /// post count, used to rank candidates by Anima familiarity.
    pub tags: HashMap<String, i64>,
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
        let mut tags = HashMap::new();
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
                    tags.insert(canon.clone(), t.p);
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
    if c.tags.contains_key(&n) {
        Some(n)
    } else {
        c.alias_to_canonical
            .get(&n)
            .filter(|canon| c.tags.contains_key(*canon))
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
/// to seed the system prompt (lexical grounding). Candidates are ranked by post
/// count so the most well-known Anima tags win the limited budget; the name is a
/// tie-breaker so the selection is deterministic regardless of map iteration order.
pub fn retrieve_candidates(input: &str, limit: usize) -> Vec<String> {
    let c = corpus();
    let input_tokens: HashSet<String> = input
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect();
    let mut matches: Vec<(&String, i64)> = c
        .tags
        .iter()
        .filter(|(tag, _)| {
            tag.split('_')
                .any(|part| part.len() > 2 && input_tokens.contains(part))
        })
        .map(|(tag, count)| (tag, *count))
        .collect();
    // Most popular (best-known to the model) first; name breaks ties deterministically.
    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    matches.truncate(limit);
    matches.iter().map(|(tag, _)| to_display(tag)).collect()
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
        // Anima: known tags first (one merged section), then a detailed NL sentence.
        let verb = match mode {
            GenMode::Enhance => "Enhance the user's prompt",
            GenMode::Compose => "Write a prompt from the user's description",
        };
        format!(
            "You are a prompt writer for the Anima anime image model. {verb}. \
For every concept that has a known Gelbooru-style tag, use that tag; only fall back to \
natural language for ideas that have no matching tag. \
Put all the tags first as a single comma-separated section, then finish with one \
detailed, grammatically complete natural-language sentence describing the scene. \
Keep everything on one line, comma-separated, tags before the sentence. \
Only reference an artist that appears in the user's input, written as @name; \
never invent one or emit a literal placeholder. \
Do not repeat tags inside the sentence. \
Do not add headings, labels like 'tags:', or em dashes. No explanations or quotes.{cand}"
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
    let pre = presanitize(raw);
    if tag_only {
        repair_tag_only(&pre)
    } else {
        repair_anima(&pre)
    }
}

/// Strip artifacts models add despite instructions, before per-mode repair:
/// - a trailing redundant labelled block ("tags:", "tag:", "prompt:") that begins
///   its own line (a leading "Prompt:" prefix is left intact so we never empty the
///   result);
/// - em/en dashes, rewritten to commas so dash-joined flourishes (e.g.
///   "elegance — @artist_name") split into separate tokens the repair pass can drop.
fn presanitize(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let mut cut = raw.len();
    for m in ["tags:", "tag:", "prompt:"] {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(m) {
            let idx = from + rel;
            let before_ok = idx == 0 || !lower.as_bytes()[idx - 1].is_ascii_alphanumeric();
            let starts_line = lower[..idx]
                .trim_end_matches(|c| c == ' ' || c == '\t')
                .ends_with('\n');
            if before_ok && starts_line {
                cut = cut.min(idx);
                break;
            }
            from = idx + m.len();
        }
    }
    raw[..cut]
        .replace('\u{2014}', ", ")
        .replace('\u{2013}', ", ")
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

/// Anima: split the output into a leading tag section and a trailing
/// natural-language section. Every standalone comma item that resolves to a known
/// tag/artist is hoisted into the merged tag section (deduped, artists as @name);
/// everything else is kept verbatim and in order as the NL description, which is
/// emitted last. Multi-word prose clauses never resolve to a tag, so sentences are
/// never fragmented — only short tag-like items get reordered.
fn repair_anima(raw: &str) -> String {
    let mut tags: Vec<String> = Vec::new();
    let mut seen_tags = HashSet::new();
    let mut prose: Vec<String> = Vec::new();
    let mut seen_prose = HashSet::new();
    for chunk in raw.split(',') {
        let token = chunk.trim();
        if token.is_empty() {
            continue;
        }
        // An @artist reference — validate it; drop unrecognized ones (e.g. a
        // literal "@artist_name" placeholder) rather than leaking them into prose.
        if let Some(rest) = token.strip_prefix('@') {
            if let Some(canon) = resolve_artist(rest) {
                let formatted = format!("@{}", to_display(&canon).replace(' ', "_"));
                if seen_tags.insert(formatted.clone()) {
                    tags.push(formatted);
                }
            }
            continue;
        }
        // A bare recognized artist → promote to @artist (tag section).
        if let Some(canon) = resolve_artist(token) {
            let formatted = format!("@{}", to_display(&canon).replace(' ', "_"));
            if seen_tags.insert(formatted.clone()) {
                tags.push(formatted);
            }
            continue;
        }
        // A recognized Anima tag → tag section (display form).
        if let Some(canon) = resolve_tag(token) {
            let display = to_display(&canon);
            if seen_tags.insert(display.clone()) {
                tags.push(display);
            }
            continue;
        }
        // Otherwise it's natural language — keep verbatim, in order, for the tail.
        let cleaned = token.trim_matches('"').trim().to_string();
        if !cleaned.is_empty() && seen_prose.insert(cleaned.clone()) {
            prose.push(cleaned);
        }
    }
    let mut sections: Vec<String> = Vec::new();
    if !tags.is_empty() {
        sections.push(tags.join(", "));
    }
    if !prose.is_empty() {
        sections.push(prose.join(", "));
    }
    sections.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_loads_known_tags() {
        let c = corpus();
        assert!(c.tags.contains_key("1girl"), "expected 1girl in corpus");
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
    fn anima_merges_tags_first_prose_last() {
        // Known tags (1girl, long_hair) are hoisted into one leading section; the
        // multi-word prose clause stays intact and is emitted last.
        let out = repair_anima("a girl stands in a misty forest, 1girl, long_hair");
        let tag_pos = out.find("1girl").expect("1girl present");
        let prose_pos = out
            .find("a girl stands in a misty forest")
            .expect("prose present");
        assert!(tag_pos < prose_pos, "tags must precede prose: {out}");
        assert!(out.contains("long hair"), "long_hair tag hoisted: {out}");
        // The sentence is not fragmented across the tag section.
        assert!(
            out.contains("a girl stands in a misty forest"),
            "got: {out}"
        );
    }

    #[test]
    fn anima_strips_emdash_placeholder_and_tag_dump() {
        // Mirrors a real bad generation: an em-dash flourish ending in the literal
        // @artist_name placeholder, plus a redundant trailing "tags:" block.
        let raw = "1girl, wearing a red dress, subtle elegance — @artist_name\n\n\
tags: 1girl, red dress, hair bun";
        let out = repair(raw, false);
        assert!(!out.contains('\u{2014}'), "em dash should be gone: {out}");
        assert!(
            !out.to_ascii_lowercase().contains("artist_name"),
            "placeholder artist should be dropped: {out}"
        );
        assert!(
            !out.to_ascii_lowercase().contains("tags:"),
            "trailing tag dump should be cut: {out}"
        );
        assert!(out.contains("1girl"));
        assert!(out.contains("wearing a red dress"));
    }

    #[test]
    fn presanitize_keeps_leading_label_prefix() {
        // A label at the very start is a prefix, not a redundant trailing block —
        // cutting there would empty the result, so it must be preserved.
        let out = presanitize("prompt: 1girl, solo");
        assert!(out.contains("1girl"), "leading prefix must survive: {out}");
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
    }

    #[test]
    fn retrieve_candidates_ranks_by_popularity() {
        // "long_hair" is one of the most common danbooru/Anima tags. Popularity
        // ranking must surface it within a realistic budget — the old alphabetical
        // sort buried it behind dozens of obscure "a…"/"b…" hair tags and
        // truncated it out entirely.
        let out = retrieve_candidates("long hair portrait", 40);
        assert!(out.contains(&"long hair".to_string()), "got: {out:?}");
    }
}
