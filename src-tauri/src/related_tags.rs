//! Related tag suggestions for one prompt tag, from two places:
//!
//! - Danbooru's related-tag statistics (`/related_tag.json`), ranked by
//!   Jaccard similarity so a generic tag such as `1girl` does not crowd out the
//!   ones that actually go with the query. Meta tags (`highres`, `commentary`)
//!   are dropped: they describe the post, not the picture. Only the tag itself is sent. Answers are
//!   cached for the session, so asking again costs Danbooru nothing.
//! - The person's own gallery: tags that appear with it in their saved
//!   prompts, counted locally from the gallery index. Works offline.
//!
//! Compiled in both builds; the Tauri wrapper lives in `commands/api.rs`.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const DANBOORU_RELATED: &str = "https://danbooru.donmai.us/related_tag.json";
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TAG_LEN: usize = 100;
const MAX_RESULTS: usize = 30;
/// Gallery prompts read per lookup. The index search is ranked, so these are
/// the best matches, not the first saved.
const GALLERY_SCAN: usize = 400;
const CACHE_CAP: usize = 500;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RelatedTag {
    /// Tag as written in a prompt: spaces, not underscores.
    pub name: String,
    /// Danbooru category (0 general, 1 artist, 3 copyright, 4 character,
    /// 5 meta). Gallery tags carry 0.
    pub category: u8,
    /// Jaccard similarity for Danbooru, times used together for the gallery.
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RelatedTags {
    /// The tag that was looked up, normalised.
    pub tag: String,
    pub danbooru: Vec<RelatedTag>,
    /// Why Danbooru gave nothing (offline, blocked), so the UI can say so.
    pub danbooru_error: Option<String>,
    pub gallery: Vec<RelatedTag>,
    /// How many gallery prompts held the tag.
    pub gallery_images: usize,
}

/// A prompt tag reduced to its plain form: weights, emphasis and NovelAI
/// numeric weights removed, lowercased, underscores as spaces. Empty when
/// nothing usable is left.
pub fn normalize_tag(raw: &str) -> String {
    // Escaped parentheses belong to the tag (`name_\(series\)`); park them so
    // the emphasis trimming below leaves them alone.
    let mut tag = raw.trim().replace("\\(", "\u{1}").replace("\\)", "\u{2}");
    if let Some(idx) = tag.find("::") {
        if tag[..idx].trim().parse::<f32>().is_ok() {
            tag = tag[idx + 2..].to_string();
        }
    }
    tag = tag.replace("::", "");
    if let Some(idx) = tag.rfind(':') {
        let suffix = tag[idx + 1..].trim_end_matches([')', ']', '}']).trim();
        if suffix.parse::<f32>().is_ok() {
            tag.truncate(idx);
        }
    }
    tag.trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | '{' | '}') || c.is_whitespace())
        .replace('_', " ")
        .replace('\u{1}', "(")
        .replace('\u{2}', ")")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Every tag in a prompt, normalised and de-duplicated, in order.
pub fn prompt_tag_set(prompt: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in prompt.lines() {
        if line.trim().to_ascii_lowercase().starts_with("text:") {
            break;
        }
        for raw in line.split(['|', ',']) {
            let tag = normalize_tag(raw);
            if tag.is_empty() || tag.len() > MAX_TAG_LEN || tag.split(' ').count() > 6 {
                continue;
            }
            if seen.insert(tag.clone()) {
                out.push(tag);
            }
        }
    }
    out
}

/// Count the tags that share a prompt with `tag`. Returns the ranking and how
/// many prompts held the tag.
pub fn co_occurrence<'a>(
    tag: &str,
    prompts: impl IntoIterator<Item = &'a str>,
) -> (Vec<RelatedTag>, usize) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut holding = 0;
    for prompt in prompts {
        let tags = prompt_tag_set(prompt);
        if !tags.iter().any(|t| t == tag) {
            continue;
        }
        holding += 1;
        for other in tags {
            if other != tag {
                *counts.entry(other).or_default() += 1;
            }
        }
    }
    let mut ranked: Vec<RelatedTag> = counts
        .into_iter()
        .map(|(name, count)| RelatedTag {
            name,
            category: 0,
            score: count as f64,
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    ranked.truncate(MAX_RESULTS);
    (ranked, holding)
}

/// Danbooru's answer, ranked by Jaccard similarity, without the tag itself,
/// deprecated tags or meta tags.
pub fn parse_danbooru(tag: &str, body: &Value) -> Vec<RelatedTag> {
    let mut out: Vec<RelatedTag> = body["related_tags"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let name = row["tag"]["name"].as_str()?.replace('_', " ");
                    let category = row["tag"]["category"].as_u64().unwrap_or(0).min(255) as u8;
                    if name == tag
                        || category == 5
                        || row["tag"]["is_deprecated"].as_bool() == Some(true)
                    {
                        return None;
                    }
                    Some(RelatedTag {
                        name,
                        category,
                        score: row["jaccard_similarity"].as_f64().unwrap_or(0.0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(MAX_RESULTS);
    out
}

fn cache() -> &'static Mutex<HashMap<String, Vec<RelatedTag>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<RelatedTag>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn danbooru(client: &reqwest::Client, tag: &str) -> Result<Vec<RelatedTag>, String> {
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(tag).cloned()) {
        return Ok(hit);
    }
    let query = tag.replace(' ', "_");
    let response = client
        .get(DANBOORU_RELATED)
        .query(&[
            ("query", query.as_str()),
            ("order", "Jaccard"),
            ("limit", "100"),
        ])
        .header(reqwest::header::USER_AGENT, "MooshieUI")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|_| "Danbooru is not reachable".to_string())?;
    if !response.status().is_success() {
        return Err(format!("Danbooru answered {}", response.status().as_u16()));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| "Danbooru sent an unexpected reply".to_string())?;
    let tags = parse_danbooru(tag, &body);
    if let Ok(mut c) = cache().lock() {
        if c.len() >= CACHE_CAP {
            c.clear();
        }
        c.insert(tag.to_string(), tags.clone());
    }
    Ok(tags)
}

/// Look up `raw_tag` in Danbooru and in the gallery at `gallery_dir`.
pub async fn lookup(
    client: &reqwest::Client,
    raw_tag: &str,
    gallery_dir: Option<&Path>,
) -> Result<RelatedTags, String> {
    let tag = normalize_tag(raw_tag);
    if tag.is_empty() {
        return Err("Select a tag to find related tags".into());
    }
    if tag.len() > MAX_TAG_LEN {
        return Err("That tag is too long to look up".into());
    }

    let (gallery, gallery_images) = match gallery_dir {
        Some(dir) => {
            let dir = dir.to_path_buf();
            let needle = tag.clone();
            tokio::task::spawn_blocking(move || {
                let rows = crate::gallery_index::search(&dir, &needle, GALLERY_SCAN);
                co_occurrence(&needle, rows.iter().filter_map(|r| r.prompt.as_deref()))
            })
            .await
            .unwrap_or_default()
        }
        None => (Vec::new(), 0),
    };

    let (danbooru, danbooru_error) = match danbooru(client, &tag).await {
        Ok(list) => (list, None),
        Err(e) => (Vec::new(), Some(e)),
    };

    Ok(RelatedTags {
        tag,
        danbooru,
        danbooru_error,
        gallery,
        gallery_images,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_strips_weights_and_underscores() {
        assert_eq!(normalize_tag(" (Red_Hair:1.2) "), "red hair");
        assert_eq!(normalize_tag("{{smile}}"), "smile");
        assert_eq!(normalize_tag("1.3::long hair::"), "long hair");
        assert_eq!(normalize_tag("[blurry]"), "blurry");
        assert_eq!(
            normalize_tag("hatsune_miku_\\(append\\)"),
            "hatsune miku (append)"
        );
        assert_eq!(normalize_tag("  ( ) "), "");
    }

    #[test]
    fn co_occurrence_counts_only_prompts_with_the_tag() {
        let prompts = [
            "1girl, red hair, smile",
            "1girl, (red_hair:1.1), green eyes",
            "1boy, blue hair, smile",
            "red hair, smile\nText: red hair everywhere",
        ];
        let (ranked, holding) = co_occurrence("red hair", prompts.iter().copied());
        assert_eq!(holding, 3);
        assert_eq!(ranked[0].name, "1girl");
        assert_eq!(ranked[0].score, 2.0);
        assert_eq!(ranked[1].name, "smile");
        assert!(ranked
            .iter()
            .all(|t| t.name != "red hair" && t.name != "1boy"));
    }

    #[test]
    fn danbooru_reply_ranked_without_query_meta_or_deprecated() {
        let body = json!({ "related_tags": [
            { "tag": { "name": "red_hair", "category": 0 }, "jaccard_similarity": 1.0 },
            { "tag": { "name": "1girl", "category": 0 }, "jaccard_similarity": 0.05 },
            { "tag": { "name": "kurenai_yuuhi", "category": 4 }, "jaccard_similarity": 0.2 },
            { "tag": { "name": "highres", "category": 5 }, "jaccard_similarity": 0.3 },
            { "tag": { "name": "old_tag", "category": 0, "is_deprecated": true }, "jaccard_similarity": 0.9 },
        ]});
        let tags = parse_danbooru("red hair", &body);
        let names: Vec<_> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["kurenai yuuhi", "1girl"]);
        assert_eq!(tags[0].category, 4);
        assert!(parse_danbooru("x", &json!({})).is_empty());
    }
}
