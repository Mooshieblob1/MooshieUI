//! Eagle (eagle.cool) export through Eagle's local API.
//!
//! Eagle listens on `127.0.0.1:41595` while it is open. Saved gallery images are
//! handed over by path with `/api/item/addFromPath`, so Eagle copies the file
//! itself and nothing is re-encoded. Tags come from the prompt, and the full
//! prompt, negative and seed go in the item's annotation.
//!
//! Compiled in both builds; the Tauri wrappers live in `commands/api.rs`.

use crate::error::AppError;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

pub const EAGLE_API: &str = "http://127.0.0.1:41595";
/// Eagle answers locally in milliseconds; anything slower means it is not running.
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TAGS: usize = 40;
const MAX_TAG_LEN: usize = 64;

/// Eagle's version, or an error saying it is not reachable.
pub async fn status(client: &reqwest::Client) -> Result<String, AppError> {
    let response = client
        .get(format!("{EAGLE_API}/api/application/info"))
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|_| AppError::Other("Eagle is not running".into()))?;
    let body: Value = response
        .json()
        .await
        .map_err(|_| AppError::Other("Eagle sent an unexpected reply".into()))?;
    if body["status"] != "success" {
        return Err(AppError::Other("Eagle refused the request".into()));
    }
    Ok(body["data"]["version"].as_str().unwrap_or("").to_string())
}

/// The request body for one image. Pure, so it is tested without Eagle.
pub fn add_body(path: &Path, metadata: Option<&Value>, folder_id: Option<&str>) -> Value {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let field = |key: &str| {
        metadata
            .and_then(|m| m.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let positive = field("positive_prompt");
    let negative = field("negative_prompt");
    let seed = field("seed");
    let model = field("model");

    let mut tags = prompt_tags(&positive);
    if !model.is_empty() {
        let model_tag = Path::new(&model)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or(model.clone());
        tags.push(format!("model:{model_tag}"));
    }

    let mut annotation = positive.clone();
    if !negative.is_empty() {
        annotation.push_str(&format!("\n\nNegative: {negative}"));
    }
    if !seed.is_empty() {
        annotation.push_str(&format!("\nSeed: {seed}"));
    }

    let mut body = json!({
        "path": path.to_string_lossy(),
        "name": name,
        "tags": tags,
        "annotation": annotation.trim(),
        "website": "",
    });
    if let Some(folder) = folder_id.map(str::trim).filter(|f| !f.is_empty()) {
        body["folderId"] = json!(folder);
    }
    body
}

pub async fn add_from_path(
    client: &reqwest::Client,
    path: &Path,
    metadata: Option<&Value>,
    folder_id: Option<&str>,
) -> Result<(), AppError> {
    let body = add_body(path, metadata, folder_id);
    let response = client
        .post(format!("{EAGLE_API}/api/item/addFromPath"))
        .timeout(TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|_| AppError::Other("Eagle is not running".into()))?;
    let reply: Value = response.json().await.unwrap_or(Value::Null);
    if reply["status"] != "success" {
        return Err(AppError::Other(format!(
            "Eagle did not add the image: {}",
            reply["data"].as_str().unwrap_or("unknown error")
        )));
    }
    Ok(())
}

/// Send gallery files the owner picked, by bare file name. Prompt, negative,
/// seed and model come from the gallery index. Returns how many Eagle took;
/// the first failure is returned when none were added.
pub async fn send_gallery_files(
    client: &reqwest::Client,
    names: &[String],
    folder_id: Option<&str>,
) -> Result<usize, AppError> {
    let dir = crate::config::gallery_dir()
        .ok_or_else(|| AppError::Other("Cannot find gallery directory".into()))?;
    let mut paths = Vec::new();
    for name in names {
        if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
            return Err(AppError::Other(format!(
                "Invalid gallery file name: {name}"
            )));
        }
        let path = dir.join(name);
        if !path.is_file() {
            return Err(AppError::Other(format!("Gallery file not found: {name}")));
        }
        paths.push(path);
    }
    let rows = crate::gallery_index::rows_for(&paths);
    let mut sent = 0;
    let mut first_error = None;
    for path in &paths {
        let metadata = rows
            .get(path.to_string_lossy().as_ref())
            .map(indexed_metadata);
        match add_from_path(client, path, metadata.as_ref(), folder_id).await {
            Ok(()) => sent += 1,
            Err(e) => {
                log::warn!("Eagle export failed for {}: {}", path.display(), e);
                first_error.get_or_insert(e);
            }
        }
    }
    match first_error {
        Some(e) if sent == 0 => Err(e),
        _ => Ok(sent),
    }
}

fn indexed_metadata(row: &crate::gallery_index::IndexedImage) -> Value {
    json!({
        "positive_prompt": row.prompt.clone().unwrap_or_default(),
        "negative_prompt": row.negative_prompt.clone().unwrap_or_default(),
        "seed": row.seed.map(|s| s.to_string()).unwrap_or_default(),
        "model": row.model.clone().unwrap_or_default(),
    })
}

/// Read Eagle's `{ status, data }` envelope.
async fn get_data(client: &reqwest::Client, path_and_query: &str) -> Result<Value, AppError> {
    let response = client
        .get(format!("{EAGLE_API}{path_and_query}"))
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|_| AppError::Other("Eagle is not running".into()))?;
    let body: Value = response
        .json()
        .await
        .map_err(|_| AppError::Other("Eagle sent an unexpected reply".into()))?;
    if body["status"] != "success" {
        return Err(AppError::Other(format!(
            "Eagle refused the request: {}",
            body["data"].as_str().unwrap_or("unknown error")
        )));
    }
    Ok(body["data"].clone())
}

fn query_value(v: &str) -> String {
    percent_encoding::utf8_percent_encode(v, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// What a library search asks for.
#[derive(Debug, Clone, Default)]
pub struct ItemQuery {
    pub keyword: Option<String>,
    pub tags: Vec<String>,
    pub folder_ids: Vec<String>,
    pub ext: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

/// The `/api/item/list` path for a query, newest first. Pure, for tests.
pub fn item_list_path(q: &ItemQuery) -> String {
    let mut parts = vec![
        format!("limit={}", q.limit.clamp(1, 200)),
        format!("offset={}", q.offset),
        "orderBy=-CREATEDATE".to_string(),
    ];
    if let Some(k) = q
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        parts.push(format!("keyword={}", query_value(k)));
    }
    if !q.tags.is_empty() {
        parts.push(format!("tags={}", query_value(&q.tags.join(","))));
    }
    if !q.folder_ids.is_empty() {
        parts.push(format!("folders={}", query_value(&q.folder_ids.join(","))));
    }
    if let Some(ext) = q.ext.as_deref().filter(|e| !e.is_empty()) {
        parts.push(format!("ext={}", query_value(ext)));
    }
    format!("/api/item/list?{}", parts.join("&"))
}

/// Items in the open Eagle library matching `q`.
pub async fn search_items(client: &reqwest::Client, q: &ItemQuery) -> Result<Vec<Value>, AppError> {
    let data = get_data(client, &item_list_path(q)).await?;
    Ok(data.as_array().cloned().unwrap_or_default())
}

/// Every folder in the library, flattened, as `(id, path name)` pairs where
/// the name holds the parent folders (`Art/Portraits`).
pub async fn folders(client: &reqwest::Client) -> Result<Vec<(String, String)>, AppError> {
    fn walk(nodes: &Value, prefix: &str, out: &mut Vec<(String, String)>) {
        for node in nodes.as_array().into_iter().flatten() {
            let (Some(id), Some(name)) = (node["id"].as_str(), node["name"].as_str()) else {
                continue;
            };
            let full = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}/{name}")
            };
            out.push((id.to_string(), full.clone()));
            walk(&node["children"], &full, out);
        }
    }
    let data = get_data(client, "/api/folder/list").await?;
    let mut out = Vec::new();
    walk(&data, "", &mut out);
    Ok(out)
}

/// One item's details.
pub async fn item_info(client: &reqwest::Client, id: &str) -> Result<Value, AppError> {
    validate_item_id(id)?;
    get_data(client, &format!("/api/item/info?id={}", query_value(id))).await
}

/// Eagle item ids are short alphanumeric strings.
fn validate_item_id(id: &str) -> Result<(), AppError> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(AppError::Other(format!("Invalid Eagle item id: {id}")));
    }
    Ok(())
}

/// The file behind an Eagle item, found next to the thumbnail Eagle reports
/// (`<library>/images/<id>.info/<name>.<ext>`). Only image files in that
/// item's own folder are returned.
pub async fn item_file(client: &reqwest::Client, id: &str) -> Result<std::path::PathBuf, AppError> {
    let info = item_info(client, id).await?;
    let thumb = get_data(
        client,
        &format!("/api/item/thumbnail?id={}", query_value(id)),
    )
    .await?;
    let thumb = thumb
        .as_str()
        .ok_or_else(|| AppError::Other("Eagle did not say where the item is".into()))?;
    let thumb = percent_encoding::percent_decode_str(thumb)
        .decode_utf8_lossy()
        .into_owned();
    let dir = Path::new(&thumb)
        .parent()
        .ok_or_else(|| AppError::Other("Eagle gave an unexpected item path".into()))?
        .to_path_buf();
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if dir_name != format!("{id}.info") {
        return Err(AppError::Other("Eagle gave an unexpected item path".into()));
    }
    let name = info["name"].as_str().unwrap_or("");
    let ext = info["ext"].as_str().unwrap_or("").to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "avif" | "jxl" | "bmp"
    ) {
        return Err(AppError::Other(format!(
            "Eagle item {id} is not an image ({ext})"
        )));
    }
    let original = dir.join(format!("{name}.{ext}"));
    if !name.is_empty() && !name.contains(['/', '\\']) && original.is_file() {
        return Ok(original);
    }
    // Some versions name the file differently; take the one image that is not the thumbnail.
    let found = std::fs::read_dir(&dir)
        .map_err(|e| AppError::Other(e.to_string()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(&ext))
                && !p
                    .file_stem()
                    .is_some_and(|s| s.to_string_lossy().ends_with("_thumbnail"))
        });
    found.ok_or_else(|| AppError::Other(format!("Eagle item {id} has no file on disk")))
}

/// Prompt tags without weights or emphasis: `(red hair:1.2)`, `{{smile}}`,
/// `1.2::rain::` and `[blurry]` all become their plain tag. Lines that read as
/// prose (NovelAI V5) and the `Text:` block are left out.
pub fn prompt_tags(prompt: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut tags = Vec::new();
    for line in prompt.lines() {
        let trimmed = line.trim();
        if trimmed.to_ascii_lowercase().starts_with("text:") {
            break;
        }
        for raw in trimmed.split(',') {
            let mut tag = raw.trim().to_string();
            // NovelAI numeric weight: `1.2::tag::` or `-1::tag::`.
            if let Some(idx) = tag.find("::") {
                if tag[..idx].trim().parse::<f32>().is_ok() {
                    tag = tag[idx + 2..].to_string();
                }
            }
            tag = tag.replace("::", "");
            // A1111 weight suffix `(tag:1.2)`.
            if let Some(idx) = tag.rfind(':') {
                let suffix = tag[idx + 1..].trim_end_matches([')', ']', '}']).trim();
                if suffix.parse::<f32>().is_ok() {
                    tag.truncate(idx);
                }
            }
            let tag = tag
                .trim_matches(|c: char| {
                    matches!(c, '(' | ')' | '[' | ']' | '{' | '}') || c.is_whitespace()
                })
                .trim()
                .to_string();
            if tag.is_empty() || tag.len() > MAX_TAG_LEN || tag.split_whitespace().count() > 5 {
                continue;
            }
            if tag.starts_with('<') || tag.contains("__") {
                continue;
            }
            if seen.insert(tag.to_lowercase()) {
                tags.push(tag);
                if tags.len() >= MAX_TAGS {
                    return tags;
                }
            }
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_list_queries_are_encoded() {
        let q = ItemQuery {
            keyword: Some("red hair".into()),
            tags: vec!["1girl".into(), "smile".into()],
            folder_ids: vec!["ABC".into()],
            limit: 500,
            ..Default::default()
        };
        assert_eq!(
            item_list_path(&q),
            "/api/item/list?limit=200&offset=0&orderBy=-CREATEDATE&keyword=red%20hair&tags=1girl%2Csmile&folders=ABC"
        );
        assert!(validate_item_id("KBJ8Z60O88VMG").is_ok());
        assert!(validate_item_id("../x").is_err());
    }

    #[test]
    fn tags_drop_weights_emphasis_and_prose() {
        let tags = prompt_tags(
            "1girl, (red hair:1.2), {{smile}}, 1.2::rain::, [blurry], 1girl, <lora:x:1>\nShe waits at the bus stop for the last train home tonight.\nText:\nHello",
        );
        assert_eq!(tags, vec!["1girl", "red hair", "smile", "rain", "blurry"]);
    }

    #[test]
    fn body_carries_tags_annotation_and_folder() {
        let metadata = json!({
            "positive_prompt": "1girl, smile",
            "negative_prompt": "lowres",
            "seed": "42",
            "model": "checkpoints/animagine-xl.safetensors",
        });
        let body = add_body(Path::new("/g/abc_0.png"), Some(&metadata), Some(" F1 "));
        assert_eq!(body["name"], "abc_0");
        assert_eq!(
            body["tags"],
            json!(["1girl", "smile", "model:animagine-xl"])
        );
        assert_eq!(
            body["annotation"],
            "1girl, smile\n\nNegative: lowres\nSeed: 42"
        );
        assert_eq!(body["folderId"], "F1");
    }

    #[test]
    fn body_without_metadata_or_folder_is_still_valid() {
        let body = add_body(Path::new("/g/x.webp"), None, Some("  "));
        assert_eq!(body["tags"], json!([]));
        assert_eq!(body["annotation"], "");
        assert!(body.get("folderId").is_none());
    }
}
