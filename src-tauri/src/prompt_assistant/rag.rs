//! Local + optional opt-in online Danbooru RAG grounding for prompt enhancement
//! and character editing.
//!
//! Offline retrieval draws from the baked-in Danbooru/Anima corpus (`grounding::corpus`)
//! to resolve canonical character, series, artist, and general tags without adding
//! extra bundled datasets. When the user explicitly enables online lookup, up to two
//! unknown character candidates per request are queried against Danbooru's read-only
//! tag, wiki, and related-tag endpoints and cached in a local SQLite database for
//! repeat instant offline hits.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::grounding;

const DANBOORU_BASE: &str = "https://danbooru.donmai.us";
const PER_REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const TOTAL_LOOKUP_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_HTTP_BYTES: usize = 64 * 1024;
const CACHE_TTL_SECS: i64 = 30 * 24 * 60 * 60;
const MISS_TTL_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RagQuery {
    pub query: String,
    #[serde(default)]
    pub existing: Option<String>,
    #[serde(default)]
    pub saved_character_names: Vec<String>,
    #[serde(default)]
    pub include_artists: bool,
    #[serde(default)]
    pub max_tags: Option<usize>,
    #[serde(default)]
    pub tier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RagCanonEntry {
    pub tag: String,
    pub series: Option<String>,
    pub summary: String,
    pub traits: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RagResult {
    pub prompt_block: String,
    pub characters: Vec<String>,
    pub copyrights: Vec<String>,
    pub artists: Vec<String>,
    pub tags: Vec<String>,
    pub canon: Vec<RagCanonEntry>,
    pub online_used: bool,
}

#[derive(Debug, Clone, Copy)]
struct TierLimits {
    max_general: usize,
    max_characters: usize,
    max_copyrights: usize,
    max_canon: usize,
    block_chars: usize,
}

fn tier_limits(tier: Option<&str>, max_tags: Option<usize>) -> TierLimits {
    let mut base = match tier.unwrap_or("full").trim().to_ascii_lowercase().as_str() {
        "compact" => TierLimits {
            max_general: 10,
            max_characters: 4,
            max_copyrights: 3,
            max_canon: 2,
            block_chars: 1200,
        },
        "standard" => TierLimits {
            max_general: 18,
            max_characters: 6,
            max_copyrights: 4,
            max_canon: 3,
            block_chars: 2200,
        },
        _ => TierLimits {
            max_general: 24,
            max_characters: 8,
            max_copyrights: 6,
            max_canon: 4,
            block_chars: 3000,
        },
    };
    if let Some(m) = max_tags {
        base.max_general = m.min(base.max_general);
    }
    base
}

fn stopword(w: &str) -> bool {
    matches!(
        w,
        "the"
            | "and"
            | "with"
            | "from"
            | "that"
            | "this"
            | "for"
            | "are"
            | "was"
            | "were"
            | "her"
            | "his"
            | "she"
            | "him"
            | "they"
            | "them"
            | "their"
            | "you"
            | "your"
            | "into"
            | "onto"
            | "over"
            | "under"
            | "near"
            | "next"
            | "wearing"
            | "holding"
            | "standing"
            | "sitting"
            | "looking"
            | "make"
            | "draw"
            | "create"
            | "change"
            | "update"
            | "edit"
            | "add"
            | "remove"
            | "keep"
            | "please"
            | "want"
            | "like"
            | "style"
            | "image"
            | "prompt"
            | "scene"
            | "character"
            | "girl"
            | "boy"
            | "woman"
            | "man"
            | "person"
            | "portrait"
            | "solo"
            | "one"
            | "two"
    )
}

fn tokenize_words(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != ':')
        .map(|s| s.trim_matches(':').to_string())
        .filter(|s| s.len() >= 2 && !stopword(s))
        .collect()
}

fn display_tag(canon: &str) -> String {
    canon.replace('_', " ")
}

fn series_from_parens(canon: &str) -> Option<&str> {
    let start = canon.rfind("_(")?;
    let end = canon.rfind(')')?;
    if end > start + 2 {
        Some(&canon[start + 2..end])
    } else {
        None
    }
}

fn strip_parens_display(tag: &str) -> String {
    tag.split(" (").next().unwrap_or(tag).trim().to_string()
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn default_cache_path() -> Option<PathBuf> {
    crate::config::app_data_dir().map(|d| d.join("prompt-assistant").join("rag_cache.sqlite"))
}

/// Resolve local verified tags, characters, series, and optional artists from the
/// baked-in corpus using exact phrase/alias matches plus token-overlap ranking.
pub fn retrieve_local(query: &RagQuery) -> RagResult {
    let limits = tier_limits(query.tier.as_deref(), query.max_tags);
    let corpus = grounding::corpus();

    let combined = match &query.existing {
        Some(ex) if !ex.trim().is_empty() => format!("{} {}", query.query, ex),
        _ => query.query.clone(),
    };
    let words = tokenize_words(&combined);
    let word_set: HashSet<String> = words.iter().cloned().collect();

    // Generate normalized 1..=3 word contiguous phrases for exact canonical / alias lookups.
    let mut phrases: Vec<String> = Vec::new();
    for width in [3usize, 2, 1] {
        if words.len() >= width {
            for win in words.windows(width) {
                phrases.push(win.join("_"));
            }
        }
    }
    // Also add raw comma-delimited pieces (handles tags with parentheses cleanly).
    for part in combined.split([',', '\n']) {
        let norm = grounding::normalize(part);
        if norm.len() >= 2 {
            phrases.push(norm);
        }
    }

    let mut char_scores: HashMap<String, i64> = HashMap::new();
    let mut copyright_scores: HashMap<String, i64> = HashMap::new();
    let mut artist_hits: Vec<String> = Vec::new();

    for phrase in &phrases {
        if phrase.len() < 2 {
            continue;
        }
        let canon = if corpus.tags.contains_key(phrase) || corpus.artists.contains(phrase) {
            phrase.clone()
        } else if let Some(mapped) = corpus.alias_to_canonical.get(phrase) {
            mapped.clone()
        } else {
            continue;
        };
        let count = corpus.tags.get(&canon).copied().unwrap_or(0);
        if corpus.characters.contains(&canon) {
            let entry = char_scores.entry(canon.clone()).or_insert(0);
            *entry = (*entry).max(count.saturating_add(1_000_000_000));
            if let Some(series_raw) = series_from_parens(&canon) {
                let series_canon = if corpus.copyrights.contains(series_raw) {
                    Some(series_raw.to_string())
                } else {
                    corpus
                        .alias_to_canonical
                        .get(series_raw)
                        .filter(|c| corpus.copyrights.contains(*c))
                        .cloned()
                };
                if let Some(sc) = series_canon {
                    let s_count = corpus.tags.get(&sc).copied().unwrap_or(0);
                    let e = copyright_scores.entry(sc).or_insert(0);
                    *e = (*e).max(s_count.saturating_add(500_000_000));
                }
            }
        } else if corpus.copyrights.contains(&canon) {
            let entry = copyright_scores.entry(canon).or_insert(0);
            *entry = (*entry).max(count.saturating_add(500_000_000));
        } else if corpus.artists.contains(&canon)
            && (query.include_artists
                || combined.contains('@')
                || combined.to_ascii_lowercase().contains("artist:"))
            && !artist_hits.contains(&display_tag(&canon))
        {
            artist_hits.push(display_tag(&canon));
        }
    }

    // Also match `@artist` / `artist:name` explicitly even without include_artists.
    for raw_tok in combined.split(|ch: char| ch == ',' || ch.is_whitespace()) {
        let trimmed = raw_tok.trim();
        let candidate = trimmed
            .strip_prefix('@')
            .or_else(|| trimmed.strip_prefix("artist:"))
            .or_else(|| trimmed.strip_prefix("ARTIST:"));
        if let Some(name) = candidate {
            let norm = grounding::normalize(name);
            let canon = if corpus.artists.contains(&norm) {
                Some(norm)
            } else {
                corpus
                    .alias_to_canonical
                    .get(&norm)
                    .filter(|c| corpus.artists.contains(*c))
                    .cloned()
            };
            if let Some(c) = canon {
                let disp = display_tag(&c);
                if !artist_hits.contains(&disp) {
                    artist_hits.push(disp);
                }
            }
        }
    }

    // Token-level lookup for disambiguated characters when exact tag wasn't typed
    // (e.g. "anis" matching "anis_(nikke)" via leading token).
    let meaningful_tokens: HashSet<String> =
        word_set.iter().filter(|w| w.len() >= 3).cloned().collect();
    if !meaningful_tokens.is_empty() {
        for canon in &corpus.characters {
            if char_scores.contains_key(canon) {
                continue;
            }
            let base = canon.split("_(").next().unwrap_or(canon);
            let mut matched_parts = 0usize;
            for part in base.split('_') {
                if part.len() >= 3 && meaningful_tokens.contains(part) {
                    matched_parts += 1;
                }
            }
            let base_parts = base.split('_').filter(|p| p.len() >= 3).count().max(1);
            if matched_parts > 0 && matched_parts * 2 >= base_parts {
                let count = corpus.tags.get(canon).copied().unwrap_or(0);
                let series_bonus = series_from_parens(canon)
                    .map(|s| {
                        s.split(|c: char| !c.is_ascii_alphanumeric())
                            .any(|p| p.len() >= 3 && meaningful_tokens.contains(p))
                    })
                    .unwrap_or(false);
                if matched_parts == base_parts || series_bonus {
                    let bonus = if series_bonus { 250_000_000 } else { 0 };
                    char_scores.insert(canon.clone(), count.saturating_add(bonus));
                    if series_bonus {
                        if let Some(series_raw) = series_from_parens(canon) {
                            if corpus.copyrights.contains(series_raw) {
                                let sc = corpus.tags.get(series_raw).copied().unwrap_or(0);
                                copyright_scores.entry(series_raw.to_string()).or_insert(sc);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut chars_sorted: Vec<(String, i64)> = char_scores.into_iter().collect();
    chars_sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    chars_sorted.truncate(limits.max_characters);
    let characters: Vec<String> = chars_sorted.iter().map(|(k, _)| display_tag(k)).collect();

    let mut cr_sorted: Vec<(String, i64)> = copyright_scores.into_iter().collect();
    cr_sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    cr_sorted.truncate(limits.max_copyrights);
    let copyrights: Vec<String> = cr_sorted.iter().map(|(k, _)| display_tag(k)).collect();

    // General tags only (exclude categories 3 and 4 so they stay cleanly separated).
    let mut general_matches: Vec<(String, i64)> = Vec::new();
    let long_tokens: HashSet<&str> = meaningful_tokens.iter().map(|s| s.as_str()).collect();
    if !long_tokens.is_empty() {
        for (tag, count) in &corpus.tags {
            if corpus.characters.contains(tag) || corpus.copyrights.contains(tag) {
                continue;
            }
            if tag
                .split('_')
                .any(|part| part.len() > 2 && long_tokens.contains(part))
            {
                let exact_bonus = if phrases.contains(tag) {
                    500_000_000
                } else {
                    0
                };
                general_matches.push((tag.clone(), count.saturating_add(exact_bonus)));
            }
        }
    }
    general_matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    general_matches.truncate(limits.max_general);
    let tags: Vec<String> = general_matches
        .iter()
        .map(|(k, _)| display_tag(k))
        .collect();

    artist_hits.truncate(6);

    let mut out = RagResult {
        prompt_block: String::new(),
        characters,
        copyrights,
        artists: artist_hits,
        tags,
        canon: Vec::new(),
        online_used: false,
    };
    out.prompt_block = format_rag_block(&out, limits.block_chars);
    out
}

/// Extract up to two sanitized 1-4 word candidate character queries for optional
/// Danbooru lookup, omitting anything already in `saved_character_names` and never
/// sending raw prose sentences over the network.
pub fn extract_online_candidates(
    query: &str,
    local_characters: &[String],
    saved_character_names: &[String],
) -> Vec<String> {
    let saved_lower: HashSet<String> = saved_character_names
        .iter()
        .map(|s| strip_parens_display(&s.to_ascii_lowercase()))
        .filter(|s| !s.is_empty())
        .collect();

    let mut out: Vec<String> = Vec::new();
    let mut push_candidate = |raw: &str| {
        if out.len() >= 2 {
            return;
        }
        let cleaned = raw
            .trim()
            .replace('_', " ")
            .chars()
            .filter(|c| {
                c.is_ascii_alphanumeric() || matches!(c, ' ' | '(' | ')' | ':' | '\'' | '-')
            })
            .collect::<String>();
        let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        let words_count = collapsed.split_whitespace().count();
        if collapsed.len() < 2 || collapsed.len() > 48 || words_count == 0 || words_count > 4 {
            return;
        }
        let base_lower = strip_parens_display(&collapsed.to_ascii_lowercase());
        if stopword(&base_lower) || saved_lower.contains(&base_lower) {
            return;
        }
        if !out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&collapsed))
        {
            out.push(collapsed);
        }
    };

    for ch in local_characters {
        push_candidate(ch);
    }

    // Capitalized name spans (e.g. "Hatsune Miku", "Frieren") not yet covered.
    let mut run: Vec<String> = Vec::new();
    let flush_run = |buf: &mut Vec<String>, push: &mut dyn FnMut(&str)| {
        if !buf.is_empty() {
            let joined = buf.join(" ");
            buf.clear();
            push(&joined);
        }
    };
    for raw_tok in query.split_whitespace() {
        let clean =
            raw_tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '\'' && c != '-');
        let starts_upper = clean.chars().next().is_some_and(|c| c.is_ascii_uppercase());
        if starts_upper && !stopword(&clean.to_ascii_lowercase()) && clean.len() >= 2 {
            run.push(clean.to_string());
        } else {
            flush_run(&mut run, &mut push_candidate);
        }
    }
    flush_run(&mut run, &mut push_candidate);

    out
}

/// Strip Danbooru DText markup (`[[tag|label]]`, `[[tag]]`, headings, links, bold/expand)
/// and condense into 1-2 clean ASCII-safe descriptive sentences up to `max_chars`.
pub fn condense_dtext_summary(raw: &str, max_chars: usize) -> String {
    let mut text = raw.replace("\r\n", "\n");

    // Replace [[target|label]] -> label, and [[target]] -> target
    while let Some(start) = text.find("[[") {
        let Some(rel_end) = text[start + 2..].find("]]") else {
            break;
        };
        let inner = &text[start + 2..start + 2 + rel_end];
        let replacement = inner
            .split_once('|')
            .map(|(_, label)| label)
            .unwrap_or(inner)
            .to_string();
        text.replace_range(start..start + 4 + rel_end, &replacement);
    }

    // Strip simple DText block/inline tags.
    for marker in [
        "[b]",
        "[/b]",
        "[i]",
        "[/i]",
        "[u]",
        "[/u]",
        "[s]",
        "[/s]",
        "[spoiler]",
        "[/spoiler]",
        "[expand]",
        "[/expand]",
    ] {
        text = text.replace(marker, "");
    }

    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("h1.")
            || trimmed.starts_with("h2.")
            || trimmed.starts_with("h3.")
            || trimmed.starts_with("h4.")
            || trimmed.starts_with("h5.")
            || trimmed.starts_with("h6.")
            || trimmed.starts_with("[expand")
            || trimmed.starts_with('*')
            || trimmed.starts_with("See also")
        {
            continue;
        }
        // Replace `"Label":https://...` links with `Label`.
        let mut clean_line = trimmed.to_string();
        while let Some(q1) = clean_line.find('"') {
            let Some(rel_q2) = clean_line[q1 + 1..].find("\":http") else {
                break;
            };
            let q2 = q1 + 1 + rel_q2;
            let mut url_end = clean_line[q2 + 2..]
                .find(|c: char| c.is_whitespace() || c == ')' || c == ',')
                .map(|i| q2 + 2 + i)
                .unwrap_or(clean_line.len());
            while url_end > q2 + 2
                && matches!(
                    clean_line.as_bytes()[url_end - 1],
                    b'.' | b'!' | b'?' | b';' | b':'
                )
            {
                url_end -= 1;
            }
            let label = clean_line[q1 + 1..q2].to_string();
            clean_line.replace_range(q1..url_end, &label);
        }
        let normalized = clean_line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !normalized.is_empty() {
            lines.push(normalized);
        }
        if lines.join(" ").len() >= max_chars {
            break;
        }
    }

    let joined = lines.join(" ");
    if joined.len() <= max_chars {
        return joined;
    }
    let slice: String = joined.chars().take(max_chars).collect();
    if let Some(pos) = slice.rfind(". ") {
        if pos >= max_chars / 3 {
            return slice[..=pos].to_string();
        }
    }
    if let Some(pos) = slice.rfind(' ') {
        return format!("{}...", &slice[..pos]);
    }
    slice
}

fn is_meta_related_tag(name: &str) -> bool {
    matches!(
        name,
        "1girl"
            | "1boy"
            | "1other"
            | "solo"
            | "highres"
            | "absurdres"
            | "commentary_request"
            | "translated"
            | "translation_request"
            | "english_text"
            | "japanese_text"
            | "simple_background"
            | "white_background"
            | "looking_at_viewer"
            | "upper_body"
            | "full_body"
            | "portrait"
            | "multiple_girls"
            | "multiple_boys"
    ) || name.starts_with("rating:")
        || name.starts_with("meta:")
}

fn open_cache(path: &Path) -> Option<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let conn = Connection::open(path).ok()?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS danbooru_cache (
            query_key TEXT PRIMARY KEY,
            payload_json TEXT NOT NULL,
            fetched_at INTEGER NOT NULL
        )",
        [],
    )
    .ok()?;
    Some(conn)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedPayload {
    entry: Option<RagCanonEntry>,
}

fn read_cache(path: &Path, key: &str, now: i64) -> Option<Option<RagCanonEntry>> {
    let conn = open_cache(path)?;
    let (json_str, fetched_at): (String, i64) = conn
        .query_row(
            "SELECT payload_json, fetched_at FROM danbooru_cache WHERE query_key = ?1",
            params![key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()?;
    let payload: CachedPayload = serde_json::from_str(&json_str).ok()?;
    let ttl = if payload.entry.is_some() {
        CACHE_TTL_SECS
    } else {
        MISS_TTL_SECS
    };
    if now - fetched_at > ttl {
        return None;
    }
    Some(payload.entry)
}

fn write_cache(path: &Path, key: &str, entry: &Option<RagCanonEntry>, now: i64) {
    let Some(conn) = open_cache(path) else {
        return;
    };
    let Ok(payload_json) = serde_json::to_string(&CachedPayload {
        entry: entry.clone(),
    }) else {
        return;
    };
    let _ = conn.execute(
        "INSERT OR REPLACE INTO danbooru_cache (query_key, payload_json, fetched_at) VALUES (?1, ?2, ?3)",
        params![key, payload_json, now],
    );
}

fn encode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push('_'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

async fn fetch_limited_json(client: &reqwest::Client, url: &str) -> Option<serde_json::Value> {
    let resp = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            concat!("MooshieUI/", env!("CARGO_PKG_VERSION")),
        )
        .timeout(PER_REQUEST_TIMEOUT)
        .send()
        .await
        .ok()?;
    let status = resp.status();
    let bytes = resp.bytes().await.ok()?;
    let slice = &bytes[..bytes.len().min(MAX_HTTP_BYTES)];
    let text = String::from_utf8_lossy(slice);
    if !status.is_success() {
        let msg = crate::http_error::readable_body("Danbooru", status.as_u16(), &text);
        log::debug!("[prompt-assistant:rag] {url} returned HTTP {status}: {msg}");
        return None;
    }
    serde_json::from_str(&text).ok()
}

#[derive(Debug, Deserialize)]
struct DanbooruTagRow {
    name: String,
    #[serde(default)]
    post_count: i64,
}

#[derive(Debug, Deserialize)]
struct DanbooruWikiRow {
    #[serde(default)]
    body: String,
}

async fn fetch_danbooru_entry(client: &reqwest::Client, candidate: &str) -> Option<RagCanonEntry> {
    let q = encode_query_component(&candidate.to_ascii_lowercase());
    let tags_url = format!(
        "{DANBOORU_BASE}/tags.json?search[name_matches]={q}*&search[category]=4&search[order]=count&limit=3"
    );
    let tags_val = fetch_limited_json(client, &tags_url).await?;
    let rows: Vec<DanbooruTagRow> = serde_json::from_value(tags_val).ok()?;
    let best = rows.into_iter().find(|r| r.post_count > 0)?;
    let canon = best.name;

    let enc_canon = encode_query_component(&canon);
    let wiki_url = format!("{DANBOORU_BASE}/wiki_pages.json?search[title]={enc_canon}&limit=1");
    let related_url =
        format!("{DANBOORU_BASE}/related_tag.json?query={enc_canon}&category=0&limit=15");

    let (wiki_val, related_val) = tokio::join!(
        fetch_limited_json(client, &wiki_url),
        fetch_limited_json(client, &related_url)
    );

    let summary = wiki_val
        .and_then(|v| serde_json::from_value::<Vec<DanbooruWikiRow>>(v).ok())
        .and_then(|mut list| list.pop())
        .map(|w| condense_dtext_summary(&w.body, 220))
        .unwrap_or_default();

    let mut traits: Vec<String> = Vec::new();
    if let Some(rv) = related_val {
        if let Some(arr) = rv.get("related_tags").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(name) = item.pointer("/tag/name").and_then(|v| v.as_str()) {
                    if !is_meta_related_tag(name) && !traits.contains(&display_tag(name)) {
                        traits.push(display_tag(name));
                    }
                }
            }
        } else if let Some(arr) = rv.get("tags").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(name) = item
                    .get(0)
                    .and_then(|v| v.as_str())
                    .or_else(|| item.as_str())
                {
                    if !is_meta_related_tag(name) && !traits.contains(&display_tag(name)) {
                        traits.push(display_tag(name));
                    }
                }
            }
        }
    }
    traits.truncate(8);

    let corpus = grounding::corpus();
    let series = series_from_parens(&canon).map(|raw| {
        if corpus.copyrights.contains(raw) {
            display_tag(raw)
        } else if let Some(mapped) = corpus.alias_to_canonical.get(raw) {
            display_tag(mapped)
        } else {
            display_tag(raw)
        }
    });

    Some(RagCanonEntry {
        tag: display_tag(&canon),
        series,
        summary,
        traits,
        source: "danbooru".into(),
    })
}

/// Format resolved RAG facts into a bounded `GROUNDED REFERENCE` block.
pub fn format_rag_block(result: &RagResult, char_cap: usize) -> String {
    let mut lines: Vec<String> = Vec::new();

    if !result.characters.is_empty() || !result.copyrights.is_empty() {
        let mut char_parts: Vec<String> = Vec::new();
        for ch in &result.characters {
            let matching_series = result
                .canon
                .iter()
                .find(|c| c.tag.eq_ignore_ascii_case(ch))
                .and_then(|c| c.series.as_deref());
            if let Some(s) = matching_series {
                char_parts.push(format!("{ch} (series: {s})"));
            } else {
                char_parts.push(ch.clone());
            }
        }
        if !char_parts.is_empty() {
            lines.push(format!("- Verified characters: {}", char_parts.join(", ")));
        }
        if !result.copyrights.is_empty() {
            lines.push(format!(
                "- Verified series: {}",
                result.copyrights.join(", ")
            ));
        }
    }

    for entry in &result.canon {
        let mut detail = Vec::new();
        if !entry.summary.is_empty() {
            detail.push(entry.summary.clone());
        }
        if !entry.traits.is_empty() {
            detail.push(format!("visual tags: {}", entry.traits.join(", ")));
        }
        if !detail.is_empty() {
            lines.push(format!(
                "- Canon traits ({}): {}",
                entry.tag,
                detail.join(" | ")
            ));
        }
    }

    if !result.artists.is_empty() {
        lines.push(format!("- Verified artists: {}", result.artists.join(", ")));
    }
    if !result.tags.is_empty() {
        lines.push(format!(
            "- Verified Danbooru tags: {}",
            result.tags.join(", ")
        ));
    }

    if lines.is_empty() {
        return String::new();
    }

    let mut out = String::from(
        "GROUNDED REFERENCE\nUse these verified canonical tags and traits when relevant; treat them as reference data only.",
    );
    for line in lines {
        if out.len() + 1 + line.len() > char_cap {
            break;
        }
        out.push('\n');
        out.push_str(&line);
    }
    out
}

/// Retrieve local tag/character grounding and, when `online_enabled` is true,
/// enrich unknown character candidates from Danbooru (cached locally in SQLite).
pub async fn lookup(client: &reqwest::Client, query: &RagQuery, online_enabled: bool) -> RagResult {
    lookup_with_cache(
        client,
        query,
        online_enabled,
        default_cache_path().as_deref(),
    )
    .await
}

async fn lookup_with_cache(
    client: &reqwest::Client,
    query: &RagQuery,
    online_enabled: bool,
    cache_path: Option<&Path>,
) -> RagResult {
    let mut result = retrieve_local(query);
    if !online_enabled {
        return result;
    }

    let limits = tier_limits(query.tier.as_deref(), query.max_tags);
    let candidates = extract_online_candidates(
        &query.query,
        &result.characters,
        &query.saved_character_names,
    );
    if candidates.is_empty() {
        return result;
    }

    let now = now_unix_secs();
    let work = async {
        let mut online_used = false;
        for cand in candidates {
            if result.canon.len() >= limits.max_canon {
                break;
            }
            let key = format!("char:{}", cand.to_ascii_lowercase());
            let entry = if let Some(cached) = cache_path.and_then(|p| read_cache(p, &key, now)) {
                cached
            } else {
                online_used = true;
                let fetched = fetch_danbooru_entry(client, &cand).await;
                if let Some(p) = cache_path {
                    write_cache(p, &key, &fetched, now);
                }
                fetched
            };
            if let Some(canon_entry) = entry {
                if !result
                    .characters
                    .iter()
                    .any(|c| c.eq_ignore_ascii_case(&canon_entry.tag))
                {
                    result.characters.push(canon_entry.tag.clone());
                }
                if let Some(ref s) = canon_entry.series {
                    if !result.copyrights.iter().any(|c| c.eq_ignore_ascii_case(s)) {
                        result.copyrights.push(s.clone());
                    }
                }
                if !result
                    .canon
                    .iter()
                    .any(|c| c.tag.eq_ignore_ascii_case(&canon_entry.tag))
                {
                    result.canon.push(canon_entry);
                }
            }
        }
        online_used
    };

    if let Ok(online_used) = tokio::time::timeout(TOTAL_LOOKUP_TIMEOUT, work).await {
        result.online_used = online_used;
    }
    result.characters.truncate(limits.max_characters);
    result.copyrights.truncate(limits.max_copyrights);
    result.canon.truncate(limits.max_canon);
    result.prompt_block = format_rag_block(&result, limits.block_chars);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_local_tags_and_clamps_by_tier() {
        let compact = retrieve_local(&RagQuery {
            query:
                "hatsune miku in a school uniform holding blue flowers under moonlight on a rooftop"
                    .into(),
            tier: Some("compact".into()),
            ..Default::default()
        });
        let full = retrieve_local(&RagQuery {
            query:
                "hatsune miku in a school uniform holding blue flowers under moonlight on a rooftop"
                    .into(),
            tier: Some("full".into()),
            ..Default::default()
        });
        assert!(compact.tags.len() <= 10);
        assert!(compact.prompt_block.len() <= 1200);
        assert!(full.tags.len() <= 24);
        assert!(!compact.prompt_block.is_empty());
    }

    #[test]
    fn online_candidate_extraction_skips_saved_names_and_never_leaks_prose() {
        let cands = extract_online_candidates(
            "Draw Frieren and Yue sitting inside a wooden tavern at sunset wearing a blue cloak",
            &["frieren".into(), "yue (arifureta)".into()],
            &["Yue".into()],
        );
        assert!(cands.iter().any(|c| c.eq_ignore_ascii_case("frieren")));
        assert!(!cands.iter().any(|c| c.to_ascii_lowercase().contains("yue")));
        assert!(cands.iter().all(|c| c.split_whitespace().count() <= 4));
    }

    #[test]
    fn dtext_condense_strips_links_headers_and_brackets() {
        let sample = "h2. Overview\n[[Frieren]] is an [[elven_mage|elven mage]] with [b]silver hair[/b] and \"official site\":https://example.com/bio. See also [[Fern]].\n* list item";
        let out = condense_dtext_summary(sample, 160);
        assert_eq!(
            out,
            "Frieren is an elven mage with silver hair and official site. See also Fern."
        );
    }

    #[test]
    fn sqlite_cache_roundtrip_and_expiry() {
        let tmp =
            std::env::temp_dir().join(format!("mooshie_rag_test_{}.sqlite", uuid::Uuid::new_v4()));
        let entry = Some(RagCanonEntry {
            tag: "frieren".into(),
            series: Some("sousou no frieren".into()),
            summary: "Elven mage with twin-tailed white hair.".into(),
            traits: vec!["white hair".into(), "twintails".into()],
            source: "danbooru".into(),
        });
        let now = 1_700_000_000;
        write_cache(&tmp, "char:frieren", &entry, now);
        assert_eq!(read_cache(&tmp, "char:frieren", now + 60).unwrap(), entry);
        assert!(read_cache(&tmp, "char:frieren", now + CACHE_TTL_SECS + 5).is_none());
        let _ = std::fs::remove_file(tmp);
    }

    #[tokio::test]
    async fn offline_lookup_skips_network_even_with_unknown_names() {
        let client = reqwest::Client::new();
        let res = lookup_with_cache(
            &client,
            &RagQuery {
                query: "Draw TotallyUnknownCharacterName with white hair".into(),
                tier: Some("compact".into()),
                ..Default::default()
            },
            false,
            None,
        )
        .await;
        assert!(!res.online_used);
        assert!(res.canon.is_empty());
    }
}
