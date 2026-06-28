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
    /// Canonical character names (underscored form, category 4) — tracked
    /// separately so the face-detailer auto-prompt can preserve named faces.
    pub characters: HashSet<String>,
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
        let mut characters = HashSet::new();
        let mut alias_to_canonical = HashMap::new();
        for t in raw {
            let canon = normalize(&t.n);
            match t.c {
                1 => {
                    artists.insert(canon.clone());
                }
                // general, copyright, character — all valid danbooru tags
                0 | 3 | 4 => {
                    if t.c == 4 {
                        characters.insert(canon.clone());
                    }
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
            characters,
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
/// `include_artists` relaxes the default artist restriction when the user opts
/// in, letting the assistant suggest well-known artists for style.
pub fn system_prompt(
    tag_only: bool,
    mode: GenMode,
    candidates: &[String],
    include_artists: bool,
) -> String {
    let cand = if candidates.is_empty() {
        String::new()
    } else {
        format!(
            "\nRelevant known tags you may draw from: {}.",
            candidates.join(", ")
        )
    };
    if tag_only {
        let body = match mode {
            GenMode::Enhance => {
                "Improve the user's danbooru tag list without changing its \
meaning. Keep every tag the user wrote, including any named character, and only ADD tags that \
reinforce what is already there (extra detail, clothing, pose, expression, setting, lighting, \
quality). When the prompt names a character, add tags that fit that character's canonical \
appearance and usual outfit; but if the user's tags already differ from those defaults (a \
different hair colour, a different outfit, and so on), keep the user's tags and do NOT add the \
conflicting default. Replace any made-up or misspelled tag with its real danbooru equivalent. \
Never change the subject or the number of characters: do not add a different or extra \
character (for example, never add 1boy to a 1girl prompt) and never add a tag that contradicts \
one already present."
            }
            GenMode::Compose => {
                "Convert the user's description into a danbooru tag list that \
matches it. Use concrete, well-known tags; do not invent subjects the description does not \
mention."
            }
        };
        let artist_rule = if include_artists {
            " You may add one or more well-known danbooru artist tags that fit the requested \
style; only use real artist names you are confident exist."
        } else {
            " Do not add artist tags."
        };
        format!(
            "You are a danbooru tag prompt writer for an anime image generator. {body}{artist_rule} \
Output ONLY a comma-separated list of lowercase danbooru tags. \
No sentences, no explanations, no quotes, no numbering. \
Prefer concrete, well-known tags.{cand}"
        )
    } else {
        // Anima: known tags first (one merged section), then a detailed NL sentence.
        let body = match mode {
            GenMode::Enhance => {
                "Improve the user's prompt without changing its meaning. Keep \
the existing subject, the number of characters, and any named character. When a character is \
named, add details that fit that character's canonical appearance and usual outfit; but if the \
user's tags already differ from those defaults (a different hair colour, a different outfit, \
and so on), keep the user's tags and never add the conflicting default. Do not add a different \
or extra character (for example, never add 1boy to a 1girl prompt) and never add anything that \
contradicts the prompt. Only add details that reinforce what is already there, and turn any \
made-up tag into a real one."
            }
            GenMode::Compose => "Write a prompt from the user's description.",
        };
        let artist_rule = if include_artists {
            "You may reference well-known artists as @name to match the requested style; \
only use real artist names you are confident exist, and never emit a literal placeholder."
        } else {
            "Only reference an artist that appears in the user's input, written as @name; \
never invent one or emit a literal placeholder."
        };
        format!(
            "You are a prompt writer for the Anima anime image model. {body} \
For every concept that has a known Gelbooru-style tag, use that tag; only fall back to \
natural language for ideas that have no matching tag. \
Put all the tags first as a single comma-separated section, then finish with one \
detailed, grammatically complete natural-language sentence describing the scene. \
Keep everything on one line, comma-separated, tags before the sentence. \
{artist_rule} \
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

/// Danbooru person-count tags, grouped girls / boys / others. Enhancement must not
/// change which of these the user supplied: adding "1boy" to a "1girl" prompt, or
/// escalating "1girl" to "2girls", changes the subject rather than improving it. The
/// user owns the character count; enhancement only adds supporting detail.
const COUNT_TAGS: &[&str] = &[
    "1girl",
    "2girls",
    "3girls",
    "4girls",
    "5girls",
    "6+girls",
    "multiple_girls",
    "1boy",
    "2boys",
    "3boys",
    "4boys",
    "5boys",
    "6+boys",
    "multiple_boys",
    "1other",
    "2others",
    "3others",
    "4others",
    "5others",
    "6+others",
    "multiple_others",
];

fn count_tag_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| COUNT_TAGS.iter().copied().collect())
}

/// Single-word atoms (danbooru canonical form, split on `_`) that mark a tag as
/// describing the face or head. Used by the optional face-detailer auto-prompt to
/// reduce a full prompt down to just what should condition a cropped face region,
/// dropping scene, pose, clothing, and background tags.
const FACE_ATOMS: &[&str] = &[
    // hair
    "hair",
    "bangs",
    "ahoge",
    "ponytail",
    "ponytails",
    "twintail",
    "twintails",
    "braid",
    "braids",
    "sidelocks",
    "sidelock",
    "forelock",
    "bun",
    "bob",
    "hime",
    "drill",
    "drills",
    "hairband",
    "hairclip",
    "hairpin",
    "hairpins",
    "scrunchie",
    "fringe",
    // eyes
    "eye",
    "eyes",
    "eyelashes",
    "eyebrow",
    "eyebrows",
    "eyeshadow",
    "eyeliner",
    "eyepatch",
    "eyewear",
    "heterochromia",
    "pupils",
    "pupil",
    "sclera",
    "iris",
    // mouth
    "mouth",
    "lips",
    "lip",
    "teeth",
    "fang",
    "fangs",
    "tongue",
    "saliva",
    "drool",
    "lipstick",
    // expression
    "smile",
    "grin",
    "smirk",
    "frown",
    "pout",
    "scowl",
    "blush",
    "blushing",
    "wink",
    "expression",
    "expressionless",
    "ahegao",
    "tears",
    "teary",
    "crying",
    // face features
    "face",
    "facial",
    "nose",
    "cheek",
    "cheeks",
    "chin",
    "jaw",
    "forehead",
    "freckles",
    "mole",
    "dimples",
    "makeup",
    "mascara",
    // glasses / facial hair
    "glasses",
    "sunglasses",
    "monocle",
    "beard",
    "mustache",
    "goatee",
    "stubble",
    "sideburns",
    // ears / head ornaments visible in a face crop
    "ears",
    "ear",
    "earrings",
    "horn",
    "horns",
    "antlers",
    "halo",
    "head",
    "headband",
    "headphones",
    "headdress",
    "headgear",
];

/// Danbooru emoticon/expression tags that carry no `_`-separated word a
/// [`FACE_ATOMS`] check could catch, yet clearly describe a facial expression.
const EMOTICON_FACE_TAGS: &[&str] = &[
    ":d", ":3", ":o", ":p", ":q", ":t", ":i", ":<", ":>", ":/", ":|", ";)", ";d", ";o", ";p", ";3",
    ";q", "xd", "x3", "d:", "o3o", "uwu", ">:)", ">:(", ">_<", "@_@", "+_+", "^_^", "^o^", "0_0",
    "o_o", "._.", "=_=", "qwq", "tot",
];

/// Extract the face/head-relevant subset of a comma-separated prompt, preserving
/// each kept item's original display text and order. Count tags (1girl/1boy),
/// named characters, and any tag whose canonical form ends in `_hair`/`_eyes` or
/// contains a [`FACE_ATOMS`] atom are kept; scene, pose, clothing, and background
/// tags are dropped. Returns an empty string when nothing face-relevant is found,
/// so the caller can fall back to conditioning on the full prompt.
pub fn extract_face_tags(prompt: &str) -> String {
    let c = corpus();
    let mut seen: HashSet<String> = HashSet::new();
    let mut kept: Vec<String> = Vec::new();
    for raw in prompt.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        // Match on a copy with ComfyUI/SD attention-weight syntax stripped
        // (`(blue hair:1.1)`, `(((smiling)))`) but keep the original `item` — weights
        // and all — in the output so the user's conditioning strength is preserved.
        // Only unwrap when the item starts with a bracket, so a tag that merely carries
        // a parenthesised qualifier (`hatsune miku (vocaloid)`) and emoticon tags
        // (`:3`, `:d`) are left intact.
        let mut clean = item;
        if clean.starts_with('(') || clean.starts_with('[') {
            clean = clean.trim_matches(|ch| matches!(ch, '(' | ')' | '[' | ']'));
            if let Some(idx) = clean.rfind(':') {
                if idx > 0 && clean[idx + 1..].trim().parse::<f32>().is_ok() {
                    clean = clean[..idx].trim_end();
                }
            }
        }
        let canon = normalize(clean);
        if canon.is_empty() {
            continue;
        }
        let is_face = canon.ends_with("_hair")
            || canon.ends_with("_eyes")
            || count_tag_set().contains(canon.as_str())
            || c.characters.contains(&canon)
            || EMOTICON_FACE_TAGS.contains(&canon.as_str())
            || canon.split('_').any(|atom| FACE_ATOMS.contains(&atom));
        if is_face && seen.insert(canon) {
            kept.push(item.to_string());
        }
    }
    kept.join(", ")
}

/// Danbooru hair/eye colours, used to detect the colour-attribute families. A
/// character's canonical hair or eye colour is one of these; enhancement must not
/// switch it when the user pinned a different one ("blue hair" must not gain "red
/// hair"). Hair *style* (long_hair, ponytail) and compound descriptors (two-tone,
/// multicoloured) are intentionally excluded — they coexist, so they are not policed.
const COLORS: &[&str] = &[
    "aqua",
    "black",
    "blonde",
    "blue",
    "brown",
    "green",
    "grey",
    "gray",
    "orange",
    "pink",
    "purple",
    "red",
    "white",
    "silver",
    "yellow",
    "light_blue",
    "dark_blue",
    "light_brown",
    "light_green",
    "light_purple",
    "dark_green",
    "light_blonde",
];

fn color_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| COLORS.iter().copied().collect())
}

/// A mutually-exclusive attribute family. When the user pins one of these,
/// enhancement may not introduce a *different* value in the same family: the person
/// count, the hair colour, and the eye colour the user chose are theirs to keep.
/// Open-ended traits (clothing, accessories, hair style) are not families — the
/// system prompt steers those instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Family {
    Count,
    HairColor,
    EyeColor,
}

/// The family a canonical tag belongs to, if any.
fn family_of(canon: &str) -> Option<Family> {
    if count_tag_set().contains(canon) {
        return Some(Family::Count);
    }
    if let Some(color) = canon.strip_suffix("_hair") {
        if color_set().contains(color) {
            return Some(Family::HairColor);
        }
    }
    if let Some(color) = canon.strip_suffix("_eyes") {
        if color_set().contains(color) {
            return Some(Family::EyeColor);
        }
    }
    None
}

/// Canonical identity of a prompt token for reconciliation: snap aliases to their
/// canonical danbooru form when known, else fall back to the normalized (underscored)
/// text so out-of-corpus tokens (niche character names) still compare consistently.
fn reconcile_canon(token: &str) -> String {
    resolve_tag(token).unwrap_or_else(|| normalize(token))
}

/// One authoritative tag from the user's prompt (one per comma item).
struct UserTag {
    /// Canonical key of the whole comma item — used for survival matching.
    canon: String,
    /// Verbatim text, re-injected unchanged if the model drops it.
    display: String,
    /// Canonical keys of the item's individual words. Lets a comma item that is really
    /// several tags run together ("1girl solo masterpiece") pin families and count as
    /// covered word by word, without ever tearing a genuine multi-word tag or character
    /// name ("ganyu (genshin impact)") apart.
    atoms: Vec<String>,
}

/// Parse the user's prompt into ordered, de-duplicated authoritative tags, one per comma
/// item. Comma items are the natural unit, so multi-word tags and character names stay
/// intact; each item also records its word-level canonical keys for family pinning and
/// coverage.
fn parse_user_tags(input: &str) -> Vec<UserTag> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<UserTag> = Vec::new();
    for chunk in input.split(',') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let canon = reconcile_canon(chunk);
        if !seen.insert(canon.clone()) {
            continue;
        }
        let atoms: Vec<String> = chunk.split_whitespace().map(reconcile_canon).collect();
        out.push(UserTag {
            canon,
            display: chunk.to_string(),
            atoms,
        });
    }
    out
}

/// Enhance-mode reconciliation. The user's prompt is authoritative and the model may
/// only ADD to it, so this enforces two guarantees on the post-repair output:
///
///   1. Conflict guard — for each mutually-exclusive family the user pinned (person
///      count, hair colour, eye colour), drop any *different* value the model
///      introduced. A 1girl prompt never gains 1boy; a "blue hair" prompt never gains
///      "red hair". Families the user left open are untouched, so the model stays free
///      to fill in a named character's canonical traits ("play into the character").
///   2. Survival — every tag the user wrote is guaranteed to appear, re-injected in
///      input order at the front if the model dropped it. A named character (or any
///      explicit tag) is therefore never lost during enhancement. An item the model
///      already echoed as separate words (a space-run like "1girl solo") counts as
///      covered and is not duplicated as a blob.
///
/// Compose invents its subject from prose, so it has nothing to anchor to and passes
/// through unchanged. Works on the comma-joined string for both tag-only and Anima.
pub fn reconcile_enhance(input: &str, output: &str, mode: GenMode) -> String {
    if mode != GenMode::Enhance {
        return output.to_string();
    }
    let user_tags = parse_user_tags(input);

    // Pin every family the user fixed, scanning each item whole *and* word by word so a
    // space-run ("1girl solo masterpiece") still pins Count via "1girl" even when a
    // neighbouring word ("masterpiece") is out of corpus.
    let mut pinned: HashMap<Family, HashSet<String>> = HashMap::new();
    for ut in &user_tags {
        for key in std::iter::once(&ut.canon).chain(ut.atoms.iter()) {
            if let Some(fam) = family_of(key) {
                pinned.entry(fam).or_default().insert(key.clone());
            }
        }
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut kept: Vec<String> = Vec::new();
    for chunk in output.split(',') {
        let token = chunk.trim();
        if token.is_empty() {
            continue;
        }
        let canon = reconcile_canon(token);
        // Drop a model-added value that switches a family the user pinned.
        if let Some(fam) = family_of(&canon) {
            if let Some(vals) = pinned.get(&fam) {
                if !vals.contains(&canon) {
                    continue;
                }
            }
        }
        if seen.insert(canon) {
            kept.push(token.to_string());
        }
    }

    // Re-inject any user tag the model failed to echo, in input order, at the front so
    // the subject and named characters still lead the prompt. Skip an item the model
    // already emitted as separate words (every atom present), so a space-run is not
    // duplicated as a blob.
    let mut missing: Vec<String> = Vec::new();
    for ut in &user_tags {
        if seen.contains(&ut.canon) {
            continue;
        }
        if ut.atoms.len() > 1 && ut.atoms.iter().all(|a| seen.contains(a)) {
            continue;
        }
        seen.insert(ut.canon.clone());
        missing.push(ut.display.clone());
    }
    if missing.is_empty() {
        kept.join(", ")
    } else {
        missing.extend(kept);
        missing.join(", ")
    }
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

    #[test]
    fn enhance_drops_added_opposite_gender() {
        // The reported bug: a 1girl prompt comes back with a random 1boy.
        let out = reconcile_enhance(
            "1girl, solo",
            "1girl, solo, 1boy, detailed background",
            GenMode::Enhance,
        );
        assert!(out.contains("1girl"), "got: {out}");
        assert!(out.contains("solo"), "got: {out}");
        assert!(out.contains("detailed background"), "got: {out}");
        assert!(!out.contains("1boy"), "added 1boy must be dropped: {out}");
    }

    #[test]
    fn enhance_blocks_count_escalation_and_restores_input() {
        // Model dropped the user's 1girl and escalated to 2girls: drop 2girls, restore 1girl.
        let out = reconcile_enhance("1girl", "2girls, long hair", GenMode::Enhance);
        assert!(out.contains("1girl"), "input count restored: {out}");
        assert!(!out.contains("2girls"), "escalated count dropped: {out}");
        assert!(out.contains("long hair"), "got: {out}");
    }

    #[test]
    fn enhance_keeps_user_supplied_multi_subject() {
        // If the user themselves asked for 1girl AND 1boy, both survive.
        let out = reconcile_enhance(
            "1girl, 1boy",
            "1girl, 1boy, holding hands",
            GenMode::Enhance,
        );
        assert!(out.contains("1girl") && out.contains("1boy"), "got: {out}");
        assert!(out.contains("holding hands"), "got: {out}");
    }

    #[test]
    fn enhance_handles_space_separated_input() {
        // Space-separated prompt: a 1boy added to "1girl solo masterpiece" is still dropped.
        let out = reconcile_enhance(
            "1girl solo masterpiece",
            "1girl, solo, masterpiece, 1boy",
            GenMode::Enhance,
        );
        assert!(!out.contains("1boy"), "got: {out}");
        assert!(out.contains("1girl") && out.contains("solo"), "got: {out}");
    }

    #[test]
    fn compose_leaves_counts_untouched() {
        // Compose invents the subject from prose, so nothing is locked.
        let out = reconcile_enhance("a girl and a boy", "1girl, 1boy, park", GenMode::Compose);
        assert_eq!(out, "1girl, 1boy, park");
    }

    #[test]
    fn enhance_preserves_named_character() {
        // The reported regression: enhancement dropped a named character. Even when the
        // model omits it, survival must re-inject the user's character tag.
        let out = reconcile_enhance(
            "1girl, hatsune miku, blue hair",
            "1girl, blue hair, stage lights",
            GenMode::Enhance,
        );
        assert!(
            out.contains("hatsune miku"),
            "character must survive: {out}"
        );
        assert!(
            out.contains("1girl") && out.contains("blue hair"),
            "got: {out}"
        );
    }

    #[test]
    fn enhance_user_hair_overrides_character_default() {
        // User pinned "blue hair"; the model added the character's canonical "red hair".
        // The user's colour wins: red hair is dropped, blue hair restored.
        let out = reconcile_enhance(
            "1girl, hatsune miku, blue hair",
            "1girl, hatsune miku, red hair, stage",
            GenMode::Enhance,
        );
        assert!(out.contains("blue hair"), "user colour kept: {out}");
        assert!(
            !out.contains("red hair"),
            "conflicting default dropped: {out}"
        );
        assert!(out.contains("hatsune miku"), "character kept: {out}");
    }

    #[test]
    fn enhance_plays_into_character_when_attribute_unspecified() {
        // The user did not pin a hair colour, so the model is free to add the
        // character's canonical one — "play into the character".
        let out = reconcile_enhance(
            "1girl, hatsune miku",
            "1girl, hatsune miku, aqua hair, detailed background",
            GenMode::Enhance,
        );
        assert!(out.contains("aqua hair"), "character trait allowed: {out}");
        assert!(out.contains("hatsune miku"), "got: {out}");
    }

    #[test]
    fn enhance_user_eye_color_overrides() {
        // Eye colour is a pinned family too: a "red eyes" prompt must not gain "blue eyes".
        let out = reconcile_enhance(
            "1girl, red eyes",
            "1girl, blue eyes, smile",
            GenMode::Enhance,
        );
        assert!(out.contains("red eyes"), "user eye colour kept: {out}");
        assert!(
            !out.contains("blue eyes"),
            "conflicting eye colour dropped: {out}"
        );
        assert!(out.contains("smile"), "supporting tag kept: {out}");
    }
}
