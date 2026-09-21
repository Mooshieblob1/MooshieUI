//! Public recording lookup for reference-song style drafts. Fixed upstream hosts,
//! bounded responses, no audio downloads or LLM/provider credentials sent upstream.
use crate::{error::AppError, state::AppState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferenceSong {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub year: String,
    pub duration_seconds: Option<u64>,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferenceSource {
    pub title: String,
    pub url: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferenceContext {
    pub song: ReferenceSong,
    pub sources: Vec<ReferenceSource>,
}

#[derive(Default)]
pub struct ReferenceLookup {
    // Apple documents approximately 20 requests/minute. Shared by all clients.
    gate: Mutex<Option<Instant>>,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
}

fn invalid(message: &str) -> AppError {
    AppError::Other(message.into())
}
fn field(value: &Value, key: &str, limit: usize) -> String {
    value[key]
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect::<String>()
        .trim()
        .into()
}

fn songs(value: &Value) -> Vec<ReferenceSong> {
    let mut found = Vec::new();
    for track in value["results"].as_array().into_iter().flatten() {
        if track["kind"] != "song" {
            continue;
        }
        let Some(id) = track["trackId"].as_u64().filter(|id| *id > 0) else {
            continue;
        };
        let title = field(track, "trackName", 250);
        let artist = field(track, "artistName", 250);
        if title.is_empty()
            || artist.is_empty()
            || found.iter().any(|song: &ReferenceSong| song.id == id)
        {
            continue;
        }
        found.push(ReferenceSong {
            id,
            title,
            artist,
            album: field(track, "collectionName", 250),
            genre: field(track, "primaryGenreName", 100),
            year: field(track, "releaseDate", 4),
            duration_seconds: track["trackTimeMillis"].as_u64().map(|ms| ms / 1000),
            // Construct our own source URL rather than trusting a returned URL.
            url: format!("https://music.apple.com/us/song/{id}"),
        });
        if found.len() == 8 {
            break;
        }
    }
    found
}

async fn json(state: &AppState, url: &str, query: &[(&str, &str)]) -> Result<Value, AppError> {
    let mut response = state
        .http_client_no_redirect
        .get(url)
        .query(query)
        .header(
            "User-Agent",
            "MooshieUI/2.3 (https://github.com/Mooshieblob1/MooshieUI)",
        )
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(invalid("Song lookup is unavailable. Please retry shortly."));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(invalid("Song lookup response was too large."));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

async fn catalog(
    state: &AppState,
    key: String,
    endpoint: &str,
    query: &[(&str, &str)],
) -> Result<Value, AppError> {
    let lookup = &state.reference_lookup;
    // A bounded gate prevents a burst of remote users from exhausting the public API.
    let mut gate = tokio::time::timeout(Duration::from_secs(10), lookup.gate.lock())
        .await
        .map_err(|_| invalid("Song lookup is busy. Please retry shortly."))?;
    if let Some((saved, value)) = lookup.cache.lock().await.get(&key) {
        if saved.elapsed() < Duration::from_secs(600) {
            return Ok(value.clone());
        }
    }
    if let Some(previous) = *gate {
        tokio::time::sleep(Duration::from_millis(3100).saturating_sub(previous.elapsed())).await;
    }
    *gate = Some(Instant::now());
    let value = json(state, endpoint, query).await?;
    let mut cache = lookup.cache.lock().await;
    cache.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(600));
    if cache.len() >= 32 {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (at, _))| *at)
            .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(key, (Instant::now(), value.clone()));
    Ok(value)
}

pub(crate) async fn search(state: &AppState, query: &str) -> Result<Vec<ReferenceSong>, AppError> {
    let query = query.trim();
    if query.is_empty()
        || query.len() > 2400
        || query.chars().count() > 600
        || query.chars().any(char::is_control)
    {
        return Err(invalid(
            "Enter a song title and artist, up to 600 characters.",
        ));
    }
    let result = catalog(
        state,
        format!("search:{query}"),
        "https://itunes.apple.com/search",
        &[
            ("term", query),
            ("media", "music"),
            ("entity", "song"),
            ("limit", "8"),
            ("country", "US"),
        ],
    )
    .await?;
    Ok(songs(&result))
}

fn normalized(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn article(song: &ReferenceSong, page: &Value) -> Option<ReferenceSource> {
    let title = field(page, "title", 300);
    let raw = page["extract"].as_str()?;
    let wanted = normalized(&song.title);
    let name = normalized(&title);
    let artist = normalized(&song.artist);
    // Conservative matching: do not use artist profiles, similarly named songs,
    // or a different artist's recording just because it mentions a cover later.
    if ![
        wanted.clone(),
        format!("{wanted} song"),
        format!("{wanted} {artist} song"),
    ]
    .contains(&name)
    {
        return None;
    }
    let opening = normalized(
        raw.split(". ")
            .next()?
            .chars()
            .take(700)
            .collect::<String>()
            .as_str(),
    );
    if !format!(" {opening} ").contains(&format!(" {artist} "))
        || !(opening.contains("song") || opening.contains("single"))
    {
        return None;
    }
    let id = page["pageid"].as_u64()?;
    Some(ReferenceSource {
        title,
        url: format!("https://en.wikipedia.org/?curid={id}"),
        text: raw.chars().take(6000).collect(),
    })
}

pub(crate) async fn context(state: &AppState, song_id: u64) -> Result<ReferenceContext, AppError> {
    if song_id == 0 {
        return Err(invalid("Choose a recording first."));
    }
    let id = song_id.to_string();
    let value = catalog(
        state,
        format!("song:{id}"),
        "https://itunes.apple.com/lookup",
        &[("id", &id), ("entity", "song"), ("country", "US")],
    )
    .await?;
    let song = songs(&value)
        .into_iter()
        .find(|s| s.id == song_id)
        .ok_or_else(|| {
            invalid("This recording is no longer available in the catalog. Search again.")
        })?;
    let mut sources = vec![ReferenceSource {
        title: "Apple Music / iTunes catalog".into(),
        url: song.url.clone(),
        text: format!(
            "Title: {}\nArtist: {}\nRelease: {}\nCatalog genre: {}\nRelease year: {}",
            song.title, song.artist, song.album, song.genre, song.year
        ),
    }];
    let query = format!(
        "\"{}\" \"{}\" song",
        song.title.replace('"', " "),
        song.artist.replace('"', " ")
    );
    // Supplemental information is best-effort. No article is preferable to an
    // unrelated article; lack of one is surfaced by the returned source list.
    if let Ok(value) = json(
        state,
        "https://en.wikipedia.org/w/api.php",
        &[
            ("action", "query"),
            ("format", "json"),
            ("formatversion", "2"),
            ("generator", "search"),
            ("gsrsearch", &query),
            ("gsrlimit", "3"),
            ("prop", "extracts"),
            ("explaintext", "1"),
            ("exintro", "1"),
            ("exlimit", "3"),
            ("exchars", "6000"),
        ],
    )
    .await
    {
        if let Some(source) = value["query"]["pages"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|page| article(&song, page))
        {
            sources.push(source);
        }
    }
    Ok(ReferenceContext { song, sources })
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn search_music_reference(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    query: String,
) -> Result<Vec<ReferenceSong>, AppError> {
    search(&state, &query).await
}
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn get_music_reference(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    song_id: u64,
) -> Result<ReferenceContext, AppError> {
    context(&state, song_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn catalog_results_preserve_recording_identity_and_use_safe_links() {
        let value = json!({"results":[{"kind":"song","trackId":12,"trackName":"Halo (Live)","artistName":"Beyoncé","collectionName":"Live Album","primaryGenreName":"Pop","releaseDate":"2009-01-01","trackViewUrl":"javascript:bad","trackTimeMillis":240100},{"kind":"song","trackId":12,"trackName":"Duplicate","artistName":"A"},{"kind":"music-video","trackId":13}]});
        let result = songs(&value);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Halo (Live)");
        assert_eq!(result[0].url, "https://music.apple.com/us/song/12");
        assert_eq!(result[0].year, "2009");
        assert_eq!(result[0].duration_seconds, Some(240));
    }
    #[test]
    fn articles_must_match_song_and_primary_artist() {
        let song = songs(&json!({"results":[{"kind":"song","trackId":1,"trackName":"Halo","artistName":"Beyoncé"}]})).remove(0);
        let page = json!({"title":"Halo (Beyoncé song)","pageid":1,"extract":"Halo is a song by Beyoncé from her album. More context."});
        assert!(article(&song, &page).is_some());
        for page in [
            json!({"title":"Beyoncé","pageid":2,"extract":"Beyoncé performs the song Halo."}),
            json!({"title":"Halo (Other song)","pageid":3,"extract":"Halo is a song by Other. Beyoncé covered it."}),
            json!({"title":"Halo Wars","pageid":4,"extract":"A song by Beyoncé."}),
        ] {
            assert!(article(&song, &page).is_none());
        }
        let mut live = song.clone();
        live.title = "Halo (Live)".into();
        assert!(
            article(&live, &page).is_none(),
            "A studio article must not describe a live recording as verified"
        );
    }

    #[tokio::test]
    #[ignore = "Calls the public Apple/Wikipedia services; no LLM or audio download"]
    async fn live_catalog_and_song_article() {
        let state = AppState::new(crate::config::AppConfig::default());
        let found = search(&state, "Halo Beyoncé").await.unwrap();
        assert!(
            found.len() > 1,
            "Expected distinct recordings to choose from"
        );
        let song = found
            .iter()
            .find(|song| song.title == "Halo" && song.artist == "Beyoncé")
            .unwrap();
        let context = context(&state, song.id).await.unwrap();
        assert_eq!(context.song.id, song.id);
        assert!(
            context.sources.len() > 1,
            "Expected the matching song article for this smoke fixture"
        );
        assert!(context.sources[1].title.contains("Halo"));
        let cached = search(&state, "Halo Beyoncé").await.unwrap();
        assert_eq!(cached[0].id, found[0].id);
        println!(
            "PASS: {} catalog recordings; exact ID lookup; {} source links; cache reuse",
            found.len(),
            context.sources.len()
        );
    }
}
