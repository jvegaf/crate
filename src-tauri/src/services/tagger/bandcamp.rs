use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{CrateError, Result};
use crate::models::{TagCandidate, TagSearchQuery};

use super::TaggerProvider;

/// Bandcamp search provider.
///
/// Uses the open autocomplete JSON endpoint. The payload is sparse by design
/// (no release date / label / duration / bpm / key); that enrichment is the
/// deferred `extend` step and is deliberately not performed here.
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

  Ok(
    results
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
      .collect(),
  )
}

/// Convert a JSON scalar to its string form (numbers become their decimal text).
fn value_to_string(value: Value) -> Option<String> {
  match value {
    Value::String(s) => Some(s),
    Value::Number(n) => Some(n.to_string()),
    _ => None,
  }
}
