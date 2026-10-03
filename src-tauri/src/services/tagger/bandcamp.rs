use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{CrateError, Result};
use crate::models::{TagCandidate, TagSearchQuery};

use super::TaggerProvider;

/// Bandcamp search provider.
///
/// Search uses the open autocomplete JSON endpoint, whose payload is sparse by
/// design (no release date / label / duration / genre). Those live on the track
/// page and are filled by [`TaggerProvider::extend`] when the user picks a
/// candidate.
pub(super) struct BandcampProvider;

#[async_trait]
impl TaggerProvider for BandcampProvider {
    fn id(&self) -> &'static str {
        "bandcamp"
    }

    async fn search(
        &self,
        client: &reqwest::Client,
        query: &TagSearchQuery,
        limit: usize,
    ) -> Result<Vec<TagCandidate>> {
        let body = serde_json::json!({
          "fan_id": null,
          "full_page": false,
          "search_filter": "t",
          "search_text": query.term(),
        });

        let response = client
            .post("https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic")
            .json(&body)
            .send()
            .await
            .map_err(|e| CrateError::Tagger(format!("Bandcamp request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(CrateError::Tagger(format!(
                "Bandcamp search returned HTTP {status}"
            )));
        }

        let text = response
            .text()
            .await
            .map_err(|e| CrateError::Tagger(format!("Bandcamp response read failed: {e}")))?;

        let mut candidates = parse_bandcamp_autocomplete(&text)?;
        candidates.truncate(limit);
        Ok(candidates)
    }

    /// Fetch the track page and enrich the candidate from its embedded metadata.
    ///
    /// The autocomplete payload carries no label, release date, duration or genre;
    /// the track page's `application/ld+json` `MusicRecording` block and its
    /// `data-tralbum` release JSON do. An empty `url` returns the candidate
    /// unchanged.
    async fn extend(
        &self,
        client: &reqwest::Client,
        candidate: &TagCandidate,
    ) -> Result<TagCandidate> {
        let url = candidate.url.trim();
        if url.is_empty() {
            return Ok(candidate.clone());
        }

        let response = client
            .get(url)
            .header(reqwest::header::ACCEPT, "text/html")
            .send()
            .await
            .map_err(|e| CrateError::Tagger(format!("Bandcamp detail request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(CrateError::Tagger(format!(
                "Bandcamp detail returned HTTP {status}"
            )));
        }

        let html = response
            .text()
            .await
            .map_err(|e| CrateError::Tagger(format!("Bandcamp detail read failed: {e}")))?;

        Ok(enrich_bandcamp_candidate(candidate, &html))
    }
}

#[derive(Debug, Deserialize)]
struct AutocompleteResponse {
    auto: Option<AutoSection>,
}

#[derive(Debug, Deserialize)]
struct AutoSection {
    results: Option<Vec<AutoResult>>,
}

#[derive(Debug, Deserialize)]
struct AutoResult {
    id: Option<Value>,
    name: Option<String>,
    band_name: Option<String>,
    album_name: Option<String>,
    album_id: Option<Value>,
    item_url_path: Option<String>,
    img: Option<String>,
}

/// Parse the Bandcamp autocomplete payload into candidates.
pub(super) fn parse_bandcamp_autocomplete(body: &str) -> Result<Vec<TagCandidate>> {
    let parsed: AutocompleteResponse = serde_json::from_str(body)
        .map_err(|e| CrateError::Tagger(format!("Failed to parse Bandcamp response: {e}")))?;

    let results = parsed
        .auto
        .and_then(|auto| auto.results)
        .unwrap_or_default();

    Ok(results
        .into_iter()
        .map(|result| TagCandidate {
            provider: "bandcamp".to_string(),
            title: result.name.unwrap_or_default(),
            version: None,
            artists: result.band_name.into_iter().collect(),
            album: result.album_name,
            label: None,
            catalog_number: None,
            genre: None,
            release_date: None,
            bpm: None,
            key: None,
            duration_ms: None,
            isrc: None,
            track_number: None,
            artwork_url: result.img,
            url: result.item_url_path.unwrap_or_default(),
            provider_track_id: result.id.and_then(value_to_string),
            provider_release_id: result.album_id.and_then(value_to_string),
        })
        .collect())
}

/// Convert a JSON scalar to its string form (numbers become their decimal text).
fn value_to_string(value: Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

static LD_JSON_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script[^>]*type="application/ld\+json"[^>]*>(.*?)</script>"#).ok()
});
static DATA_TRALBUM_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"data-tralbum="([^"]*)""#).ok());
static ISO_DURATION_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^P(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?)?$").ok());

/// Enrich a Bandcamp candidate from its track-page HTML.
///
/// Reads the page's `application/ld+json` `MusicRecording` block and its
/// `data-tralbum` release JSON. A field the page does not expose stays as it was
/// on the input candidate, and `provider`, `provider_track_id`,
/// `provider_release_id` and a non-empty `url` are always preserved. Nothing is
/// guessed.
pub(super) fn enrich_bandcamp_candidate(candidate: &TagCandidate, html: &str) -> TagCandidate {
    let recording = find_music_recording(html);
    let tralbum = find_data_tralbum(html);

    let mut enriched = candidate.clone();

    if let Some(recording) = recording.as_ref() {
        if let Some(name) = json_string(recording, "name") {
            enriched.title = name;
        }
        if let Some(artist) = json_nested_name(recording, "byArtist") {
            enriched.artists = vec![artist];
        }
        fill(&mut enriched.album, json_nested_name(recording, "inAlbum"));
        fill(
            &mut enriched.duration_ms,
            json_string(recording, "duration").and_then(|value| parse_iso8601_duration(&value)),
        );
        fill(&mut enriched.artwork_url, json_image(recording));
    }

    if enriched.url.trim().is_empty() {
        if let Some(url) = recording.as_ref().and_then(json_url) {
            enriched.url = url;
        }
    }

    let recording_date = recording
        .as_ref()
        .and_then(|recording| json_string(recording, "datePublished"))
        .and_then(|value| normalize_date(&value));
    let tralbum_date = tralbum
        .as_ref()
        .and_then(|tralbum| json_string(tralbum, "album_release_date"))
        .and_then(|value| normalize_date(&value));
    fill(&mut enriched.release_date, recording_date.or(tralbum_date));

    if let Some(tralbum) = tralbum.as_ref() {
        fill(&mut enriched.label, json_string(tralbum, "label"));
        fill(&mut enriched.genre, json_string(tralbum, "genre"));
    }

    enriched
}

/// Parse an ISO-8601 duration (`PT5M41S`, `PT1H2M3S`, `P1DT2H`) into
/// milliseconds. Returns `None` for anything that is not a non-zero, well-formed
/// duration.
pub(super) fn parse_iso8601_duration(raw: &str) -> Option<i64> {
    let captures = ISO_DURATION_RE.as_ref()?.captures(raw.trim())?;

    let days = duration_part(&captures, 1)?;
    let hours = duration_part(&captures, 2)?;
    let minutes = duration_part(&captures, 3)?;
    let seconds = duration_part(&captures, 4)?;

    let total_seconds = days
        .checked_mul(86_400)?
        .checked_add(hours.checked_mul(3_600)?)?
        .checked_add(minutes.checked_mul(60)?)?
        .checked_add(seconds)?;

    if total_seconds == 0 {
        return None;
    }

    total_seconds.checked_mul(1_000)
}

/// One numeric group of the ISO-8601 duration: `Some(0)` when the group is
/// absent, `None` when it is present but cannot be parsed (so a malformed value
/// is not silently read as zero).
fn duration_part(captures: &regex::Captures<'_>, index: usize) -> Option<i64> {
    match captures.get(index) {
        None => Some(0),
        Some(value) => value.as_str().parse::<i64>().ok(),
    }
}

/// Return the first `application/ld+json` block whose `@type` is
/// `MusicRecording`, decoded as JSON.
fn find_music_recording(html: &str) -> Option<Value> {
    let regex = LD_JSON_RE.as_ref()?;
    for captures in regex.captures_iter(html) {
        let Some(raw) = captures.get(1) else { continue };
        let Ok(value) = serde_json::from_str::<Value>(raw.as_str().trim()) else {
            continue;
        };
        if is_music_recording(&value) {
            return Some(value);
        }
    }
    None
}

/// Decode the page's `data-tralbum` attribute (HTML-entity-encoded JSON).
fn find_data_tralbum(html: &str) -> Option<Value> {
    let captures = DATA_TRALBUM_RE.as_ref()?.captures(html)?;
    let raw = decode_entities(captures.get(1)?.as_str());
    serde_json::from_str::<Value>(&raw).ok()
}

fn is_music_recording(value: &Value) -> bool {
    match value.get("@type") {
        Some(Value::String(kind)) => kind == "MusicRecording",
        Some(Value::Array(kinds)) => kinds
            .iter()
            .any(|kind| kind.as_str() == Some("MusicRecording")),
        _ => false,
    }
}

/// The trimmed, non-empty string at `value[key]`, if any.
fn json_string(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// The first non-empty `name` under `value[key]`, accepting an object or an
/// array of objects (schema.org `byArtist` may be either).
fn json_nested_name(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::Array(items) => items.iter().find_map(|item| json_string(item, "name")),
        single => json_string(single, "name"),
    }
}

/// The `image` URL, accepting either a bare string or an `ImageObject`.
fn json_image(value: &Value) -> Option<String> {
    match value.get("image")? {
        Value::String(url) => {
            let trimmed = url.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        image => json_string(image, "url"),
    }
}

/// The `url`, falling back to `@id`.
fn json_url(value: &Value) -> Option<String> {
    json_string(value, "url").or_else(|| json_string(value, "@id"))
}

/// Normalize a provider date to its `YYYY-MM-DD` prefix. Anything that is not an
/// ISO-8601 date is left to the caller as `None` rather than guessed.
fn normalize_date(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let prefix = trimmed.get(..10)?;
    if is_iso_date(prefix) {
        Some(prefix.to_string())
    } else {
        None
    }
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

/// Replace `slot` only when the parsed `value` is non-empty, so a detail parse
/// never clears a field the search already populated.
fn fill<T>(slot: &mut Option<T>, value: Option<T>) {
    if value.is_some() {
        *slot = value;
    }
}

/// Decode the HTML entities Bandcamp uses inside the `data-tralbum` attribute.
/// `&amp;` is decoded last so a double-encoded entity is not over-decoded.
fn decode_entities(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}
