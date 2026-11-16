//! The owner's saved characters, artist styles and prompt chunks, read from
//! their synced prefs (`users/_admin/prefs.json`), and the rules for putting
//! them into an MCP request.
//!
//! The Generate page adds these in `toParams` before a request reaches Rust;
//! a setup has its prompt cleared, so nothing of them survives into MCP. This
//! mirrors what the page does: a character goes in as its own NovelAI
//! character prompt where the model has them, otherwise at the end of the
//! prompt; styles append weighted artist tags; chunks prepend or append their
//! text (or one random line); and every UC part goes to the negative prompt.
//!
//! Read only. Edits stay in the app, whose stores would overwrite a change
//! made here on their next sync.

use serde::Deserialize;
use serde_json::Value;

use crate::comfyui::types::GenerationParams;
use crate::novelai::params::{NovelAiCharacter, NovelAiCoord};

/// Every NovelAI model shares one bucket for saved characters.
pub const NOVELAI_ARCHITECTURE: &str = "novelai";

/// Families whose prompts are Danbooru-style tags (`TAG_PROMPT_FAMILIES`).
const TAG_FAMILIES: &[&str] = &[
    NOVELAI_ARCHITECTURE,
    "sdxl",
    "illustrious",
    "pony",
    "sd15",
    "anima",
];

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MemoryFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Character {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub negative: String,
    pub architecture: String,
    pub thumbnail: Option<String>,
    pub reference: Option<String>,
    pub parent_id: Option<String>,
    pub memory: Vec<MemoryFile>,
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StyleArtist {
    pub tag: String,
    pub weight: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Style {
    pub id: String,
    pub name: String,
    pub artists: Vec<StyleArtist>,
    pub overall_weight: Option<f64>,
    pub negative: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Chunk {
    pub id: String,
    pub name: String,
    pub content: String,
    pub negative: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ActiveChunk {
    id: String,
    mode: String,
}

#[derive(Debug, Clone, Default)]
pub struct Library {
    pub characters: Vec<Character>,
    pub styles: Vec<Style>,
    pub chunks: Vec<Chunk>,
    pub active_style_ids: Vec<String>,
    /// Active chunks and how the app applies them.
    pub active_chunks: Vec<(String, ChunkMode)>,
}

/// Where a chunk's text goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkMode {
    Prepend,
    Append,
    /// One random line of the chunk, appended.
    Random,
}

impl ChunkMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "prepend" | "start" | "before" => Some(Self::Prepend),
            "append" | "end" | "after" => Some(Self::Append),
            "random" | "wildcard" | "wildcard_ordered" | "pick_one" => Some(Self::Random),
            _ => None,
        }
    }
}

fn list<T: for<'de> Deserialize<'de>>(value: Option<&Value>) -> Vec<T> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

impl Library {
    /// Build from the stored prefs categories.
    pub fn from_prefs(prefs: &crate::user_prefs::UserPrefs) -> Self {
        let characters: Vec<Character> =
            list(prefs.characters.as_ref().and_then(|c| c.get("characters")));
        let styles: Vec<Style> = list(prefs.styles.as_ref().and_then(|s| s.get("styles")));
        let active_style_ids: Vec<String> =
            list(prefs.styles.as_ref().and_then(|s| s.get("activeIds")));
        let chunks: Vec<Chunk> = list(prefs.prompt_presets.as_ref().and_then(|p| p.get("presets")));
        let active: Vec<ActiveChunk> =
            list(prefs.prompt_presets.as_ref().and_then(|p| p.get("active")));
        Self {
            characters: characters
                .into_iter()
                .filter(|c| !c.name.trim().is_empty())
                .collect(),
            styles,
            chunks,
            active_style_ids,
            active_chunks: active
                .into_iter()
                .filter_map(|a| ChunkMode::parse(&a.mode).map(|m| (a.id, m)))
                .collect(),
        }
    }

    /// The owner's library, or an empty one when nothing is saved yet.
    pub async fn load() -> Self {
        crate::user_prefs::load(crate::user_prefs::OWNER_KEY)
            .await
            .map(|p| Self::from_prefs(&p))
            .unwrap_or_default()
    }
}

/// Find one item by name: exact (case-insensitive), then a unique substring.
fn find_by_name<'a, T>(
    items: &'a [T],
    name: &str,
    name_of: impl Fn(&T) -> &str,
    what: &str,
) -> Result<&'a T, String> {
    let wanted = name.trim().to_lowercase();
    if wanted.is_empty() {
        return Err(format!("Empty {what} name"));
    }
    if let Some(hit) = items
        .iter()
        .find(|i| name_of(i).trim().to_lowercase() == wanted)
    {
        return Ok(hit);
    }
    let partial: Vec<&T> = items
        .iter()
        .filter(|i| name_of(i).to_lowercase().contains(&wanted))
        .collect();
    match partial.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "No saved {what} called '{name}'. Saved: {}.",
            names(items.iter().map(&name_of))
        )),
        many => Err(format!(
            "'{name}' matches several {what}s: {}. Use the full name.",
            names(many.iter().map(|i| name_of(i)))
        )),
    }
}

fn names<'a>(it: impl Iterator<Item = &'a str>) -> String {
    let mut all: Vec<&str> = it.collect();
    all.dedup();
    if all.is_empty() {
        "none".into()
    } else {
        all.join(", ")
    }
}

fn is_tag_family(arch: &str) -> bool {
    TAG_FAMILIES.contains(&arch)
}

/// The architecture bucket a request's characters come from.
pub fn architecture_of(params: &GenerationParams) -> String {
    if params.novelai.is_some() {
        NOVELAI_ARCHITECTURE.into()
    } else {
        params.model_architecture.clone()
    }
}

/// The card for `name` that suits `arch`: one saved under that architecture,
/// else one written the same way (tags or sentences), else any.
pub fn find_character<'a>(
    lib: &'a Library,
    name: &str,
    arch: &str,
) -> Result<&'a Character, String> {
    let first = find_by_name(&lib.characters, name, |c| &c.name, "character")?;
    let wanted = first.name.trim().to_lowercase();
    let same_name: Vec<&Character> = lib
        .characters
        .iter()
        .filter(|c| c.name.trim().to_lowercase() == wanted)
        .collect();
    let pick = same_name
        .iter()
        .find(|c| c.architecture == arch)
        .or_else(|| {
            same_name
                .iter()
                .find(|c| is_tag_family(&c.architecture) == is_tag_family(arch))
        })
        .copied()
        .unwrap_or(first);
    Ok(pick)
}

/// Append `text` to a comma-separated prompt.
fn join_prompt(current: &str, text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return current.to_string();
    }
    let base = current.trim().trim_end_matches(',').trim_end();
    if base.is_empty() {
        text.to_string()
    } else {
        format!("{base}, {text}")
    }
}

fn prepend_prompt(current: &str, text: &str) -> String {
    let text = text.trim().trim_end_matches(',').trim_end();
    if text.is_empty() {
        return current.to_string();
    }
    let rest = current.trim();
    if rest.is_empty() {
        text.to_string()
    } else {
        format!("{text}, {rest}")
    }
}

/// A UC field with its lines turned into commas.
fn uc_text(negative: &str) -> String {
    negative
        .lines()
        .map(|l| l.trim().trim_matches(',').trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Put a character into the request: its own NovelAI character prompt on
/// models that have them (with its UC in that box where the model takes
/// per-character UC), otherwise at the end of the prompt.
pub fn insert_character(params: &mut GenerationParams, c: &Character) -> Result<(), String> {
    let prompt = c.prompt.trim();
    let negative = uc_text(&c.negative);
    let model = params.novelai.as_ref().and_then(|nai| {
        let id = if crate::novelai::models::is_novelai_model(&params.checkpoint) {
            params.checkpoint.clone()
        } else {
            nai.model.clone()
        };
        crate::novelai::models::find(&id)
    });
    if let (Some(model), Some(nai)) = (model.filter(|m| m.v4_prompt), params.novelai.as_mut()) {
        if nai.characters.len() >= model.max_characters {
            return Err(format!(
                "{} takes {} characters at most.",
                model.label, model.max_characters
            ));
        }
        nai.characters.push(NovelAiCharacter {
            prompt: prompt.to_string(),
            negative_prompt: if model.character_negatives {
                negative.clone()
            } else {
                String::new()
            },
            center: NovelAiCoord { x: 0.5, y: 0.5 },
            enabled: true,
        });
        if !model.character_negatives {
            params.negative_prompt = join_prompt(&params.negative_prompt, &negative);
        }
        return Ok(());
    }
    params.positive_prompt = join_prompt(&params.positive_prompt, prompt);
    params.negative_prompt = join_prompt(&params.negative_prompt, &negative);
    Ok(())
}

fn round2(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

fn clamp_weight(w: Option<f64>) -> f64 {
    w.filter(|w| w.is_finite()).unwrap_or(1.0).clamp(0.0, 3.0)
}

/// Weighted artist tags for `styles`, the way `fragmentForStyles` writes
/// them: deduped, weight 1 bare, `(tag:w)` otherwise (NovelAI's weight syntax
/// is produced from that when the request is built), `@` dropped on NovelAI.
pub fn style_fragment(styles: &[&Style], strip_sigil: bool) -> String {
    let mut parts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for style in styles {
        for a in &style.artists {
            let raw = a.tag.trim();
            let tag = if strip_sigil {
                raw.trim_start_matches('@')
            } else {
                raw
            };
            if tag.is_empty() || !seen.insert(tag.to_lowercase()) {
                continue;
            }
            let w = round2(clamp_weight(a.weight) * clamp_weight(style.overall_weight));
            if w <= 0.0 {
                continue;
            }
            let mut safe = String::with_capacity(tag.len());
            for ch in tag.chars() {
                if matches!(ch, '(' | ')' | '[' | ']') {
                    safe.push('\\');
                }
                safe.push(ch);
            }
            if (w - 1.0).abs() < f64::EPSILON {
                parts.push(safe);
            } else {
                parts.push(format!("({safe}:{w})"));
            }
        }
    }
    parts.join(", ")
}

/// What an agent asked to add.
#[derive(Debug, Clone, Default)]
pub struct Additions {
    pub characters: Vec<String>,
    pub styles: Vec<String>,
    pub chunks: Vec<(String, Option<ChunkMode>)>,
    /// Also add the owner's currently active styles and chunks.
    pub use_active: bool,
}

impl Additions {
    pub fn is_empty(&self) -> bool {
        self.characters.is_empty()
            && self.styles.is_empty()
            && self.chunks.is_empty()
            && !self.use_active
    }

    /// Read `characters`, `styles`, `chunks` and `use_active_styles` from a
    /// tool call. Chunks are names, or `{ "name", "mode" }` objects.
    pub fn from_args(args: &Value) -> Result<Self, String> {
        let strings = |key: &str| -> Result<Vec<String>, String> {
            match args.get(key) {
                None | Some(Value::Null) => Ok(Vec::new()),
                Some(Value::String(s)) => Ok(vec![s.clone()]),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| format!("{key} must be a list of names"))
                    })
                    .collect(),
                _ => Err(format!("{key} must be a list of names")),
            }
        };
        let mut chunks = Vec::new();
        match args.get("chunks") {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => {
                for item in items {
                    match item {
                        Value::String(s) => chunks.push((s.clone(), None)),
                        Value::Object(o) => {
                            let name = o
                                .get("name")
                                .and_then(Value::as_str)
                                .ok_or("every chunk needs a name")?;
                            let mode = match o.get("mode").and_then(Value::as_str) {
                                Some(m) => Some(ChunkMode::parse(m).ok_or_else(|| {
                                    format!(
                                        "chunk mode must be prepend, append or random, not '{m}'"
                                    )
                                })?),
                                None => None,
                            };
                            chunks.push((name.to_string(), mode));
                        }
                        _ => return Err("chunks must be names or { name, mode } objects".into()),
                    }
                }
            }
            Some(Value::String(s)) => chunks.push((s.clone(), None)),
            _ => return Err("chunks must be a list".into()),
        }
        let out = Self {
            characters: strings("characters")?,
            styles: strings("styles")?,
            chunks,
            use_active: args
                .get("use_active_styles")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        if out.characters.len() > 22 || out.styles.len() > 20 || out.chunks.len() > 30 {
            return Err("Too many characters, styles or chunks in one request".into());
        }
        Ok(out)
    }
}

/// One random non-empty line of a chunk.
fn random_line(content: &str, rng_value: u32) -> String {
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    lines[rng_value as usize % lines.len()].to_string()
}

/// Apply `add` to `params`. `seed` picks the random lines of random chunks.
/// Returns the names actually used, for the job record.
pub fn apply(
    params: &mut GenerationParams,
    lib: &Library,
    add: &Additions,
    seed: u32,
) -> Result<Vec<String>, String> {
    let mut used = Vec::new();
    let arch = architecture_of(params);
    let nai = params.novelai.is_some();

    for name in &add.characters {
        let c = find_character(lib, name, &arch)?;
        insert_character(params, c)?;
        used.push(format!("character:{}", c.name));
    }

    let mut style_refs: Vec<&Style> = Vec::new();
    if add.use_active {
        for id in &lib.active_style_ids {
            if let Some(s) = lib.styles.iter().find(|s| &s.id == id) {
                style_refs.push(s);
            }
        }
    }
    for name in &add.styles {
        let s = find_by_name(&lib.styles, name, |s| &s.name, "style")?;
        if !style_refs.iter().any(|r| r.id == s.id) {
            style_refs.push(s);
        }
    }
    let fragment = style_fragment(&style_refs, nai);
    params.positive_prompt = join_prompt(&params.positive_prompt, &fragment);
    let mut uc_parts: Vec<String> = Vec::new();
    for s in &style_refs {
        used.push(format!("style:{}", s.name));
        let uc = uc_text(&s.negative);
        if !uc.is_empty() && !uc_parts.iter().any(|p| p.eq_ignore_ascii_case(&uc)) {
            uc_parts.push(uc);
        }
    }

    let mut chunk_refs: Vec<(&Chunk, ChunkMode)> = Vec::new();
    if add.use_active {
        for (id, mode) in &lib.active_chunks {
            if let Some(c) = lib.chunks.iter().find(|c| &c.id == id) {
                chunk_refs.push((c, *mode));
            }
        }
    }
    for (name, mode) in &add.chunks {
        let c = find_by_name(&lib.chunks, name, |c| &c.name, "chunk")?;
        let mode = mode
            .or_else(|| {
                lib.active_chunks
                    .iter()
                    .find(|(id, _)| id == &c.id)
                    .map(|(_, m)| *m)
            })
            .unwrap_or(ChunkMode::Append);
        chunk_refs.retain(|(r, _)| r.id != c.id);
        chunk_refs.push((c, mode));
    }
    let mut pick = seed;
    for (c, mode) in &chunk_refs {
        match mode {
            ChunkMode::Prepend => {
                params.positive_prompt = prepend_prompt(&params.positive_prompt, &c.content);
            }
            ChunkMode::Append => {
                params.positive_prompt = join_prompt(&params.positive_prompt, &c.content);
            }
            ChunkMode::Random => {
                pick = pick.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let line = random_line(&c.content, pick >> 8);
                params.positive_prompt = join_prompt(&params.positive_prompt, &line);
            }
        }
        used.push(format!("chunk:{}", c.name));
        let uc = uc_text(&c.negative);
        if !uc.is_empty() && !uc_parts.iter().any(|p| p.eq_ignore_ascii_case(&uc)) {
            uc_parts.push(uc);
        }
    }
    if !uc_parts.is_empty() {
        params.negative_prompt = join_prompt(&params.negative_prompt, &uc_parts.join(", "));
    }
    Ok(used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::novelai::params::NovelAiParams;
    use serde_json::json;

    fn lib() -> Library {
        let prefs: crate::user_prefs::UserPrefs = serde_json::from_value(json!({
            "characters": { "characters": [
                { "id": "1", "name": "Yue", "prompt": "yue, silver hair", "negative": "short hair", "architecture": "illustrious" },
                { "id": "2", "name": "Yue", "prompt": "yue (character), silver hair", "negative": "short hair", "architecture": "novelai" },
                { "id": "3", "name": "Mira", "prompt": "A woman with red hair", "negative": "", "architecture": "flux" }
            ]},
            "styles": { "styles": [
                { "id": "s1", "name": "Soft", "artists": [ { "tag": "@ciloranko", "weight": 1.0 }, { "tag": "wlop (artist)", "weight": 0.5 } ], "overallWeight": 1.2, "negative": "sketch\nlineart" }
            ], "activeIds": ["s1"] },
            "prompt_presets": { "presets": [
                { "id": "c1", "name": "Quality", "content": "masterpiece, best quality", "negative": "worst quality" },
                { "id": "c2", "name": "Weather", "content": "rain\nsnow\nfog", "negative": "" }
            ], "active": [ { "id": "c1", "mode": "prepend" } ] }
        }))
        .unwrap();
        Library::from_prefs(&prefs)
    }

    fn local() -> GenerationParams {
        GenerationParams {
            mode: "txt2img".into(),
            positive_prompt: "1girl".into(),
            negative_prompt: "lowres".into(),
            model_architecture: "illustrious".into(),
            ..Default::default()
        }
    }

    fn novelai(model: &str) -> GenerationParams {
        GenerationParams {
            checkpoint: model.into(),
            novelai: Some(NovelAiParams {
                model: model.into(),
                ..Default::default()
            }),
            ..local()
        }
    }

    #[test]
    fn characters_pick_the_card_for_the_model() {
        let l = lib();
        assert_eq!(find_character(&l, "yue", "illustrious").unwrap().id, "1");
        assert_eq!(find_character(&l, "yue", "novelai").unwrap().id, "2");
        // Pony has no card of its own: a tag card is the next best thing.
        assert!(matches!(
            find_character(&l, "Yue", "pony").unwrap().id.as_str(),
            "1" | "2"
        ));
        assert_eq!(find_character(&l, "mir", "flux").unwrap().id, "3");
        assert!(find_character(&l, "nobody", "flux").is_err());
    }

    #[test]
    fn a_local_character_goes_to_the_end_of_the_prompt_with_its_uc() {
        let mut p = local();
        let add = Additions {
            characters: vec!["Yue".into()],
            ..Default::default()
        };
        apply(&mut p, &lib(), &add, 1).unwrap();
        assert_eq!(p.positive_prompt, "1girl, yue, silver hair");
        assert_eq!(p.negative_prompt, "lowres, short hair");
    }

    #[test]
    fn a_novelai_character_gets_its_own_character_prompt() {
        let mut p = novelai("nai-diffusion-4-5-full");
        let add = Additions {
            characters: vec!["Yue".into()],
            ..Default::default()
        };
        apply(&mut p, &lib(), &add, 1).unwrap();
        let nai = p.novelai.unwrap();
        assert_eq!(nai.characters.len(), 1);
        assert_eq!(nai.characters[0].prompt, "yue (character), silver hair");
        assert_eq!(p.positive_prompt, "1girl");
        // V4.5 takes per-character UC, so it stays in the box.
        assert_eq!(nai.characters[0].negative_prompt, "short hair");
        assert_eq!(p.negative_prompt, "lowres");
    }

    #[test]
    fn styles_write_weighted_tags_and_add_their_uc() {
        let mut p = local();
        let add = Additions {
            styles: vec!["soft".into()],
            ..Default::default()
        };
        apply(&mut p, &lib(), &add, 1).unwrap();
        assert_eq!(
            p.positive_prompt,
            "1girl, (@ciloranko:1.2), (wlop \\(artist\\):0.6)"
        );
        assert_eq!(p.negative_prompt, "lowres, sketch, lineart");

        let mut n = novelai("nai-diffusion-4-5-full");
        apply(&mut n, &lib(), &add, 1).unwrap();
        assert!(n.positive_prompt.contains("(ciloranko:1.2)"));
    }

    #[test]
    fn chunks_follow_their_mode() {
        let mut p = local();
        let add = Additions {
            chunks: vec![
                ("quality".into(), None),
                ("weather".into(), Some(ChunkMode::Random)),
            ],
            ..Default::default()
        };
        apply(&mut p, &lib(), &add, 7).unwrap();
        // Quality is active as prepend, so it keeps that mode.
        assert!(p
            .positive_prompt
            .starts_with("masterpiece, best quality, 1girl, "));
        let last = p.positive_prompt.rsplit(", ").next().unwrap();
        assert!(
            ["rain", "snow", "fog"].contains(&last),
            "{}",
            p.positive_prompt
        );
        assert_eq!(p.negative_prompt, "lowres, worst quality");
    }

    #[test]
    fn active_styles_and_chunks_on_request() {
        let mut p = local();
        let add = Additions {
            use_active: true,
            ..Default::default()
        };
        let used = apply(&mut p, &lib(), &add, 1).unwrap();
        assert_eq!(used, vec!["style:Soft", "chunk:Quality"]);
        assert!(p.positive_prompt.starts_with("masterpiece"));
        assert!(p.positive_prompt.contains("ciloranko"));
    }

    #[test]
    fn arguments_accept_names_and_chunk_objects() {
        let add = Additions::from_args(&json!({
            "characters": "Yue",
            "styles": ["Soft"],
            "chunks": ["Quality", { "name": "Weather", "mode": "random" }],
        }))
        .unwrap();
        assert_eq!(add.characters, vec!["Yue"]);
        assert_eq!(add.chunks[1].1, Some(ChunkMode::Random));
        assert!(
            Additions::from_args(&json!({ "chunks": [{ "name": "x", "mode": "sideways" }] }))
                .is_err()
        );
        assert!(Additions::from_args(&json!({})).unwrap().is_empty());
    }
}
