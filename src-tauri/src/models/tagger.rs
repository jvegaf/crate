use serde::{Deserialize, Serialize};

/// A single metadata candidate returned by a tagger provider search.
///
/// Fields are intentionally sparse: providers that do not expose a value in
/// their search payload leave it as `None` (the deferred `extend` step would
/// enrich them per ID).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TagCandidate {
  pub provider: String,
  pub title: String,
  pub version: Option<String>,
  pub artists: Vec<String>,
  pub album: Option<String>,
  pub label: Option<String>,
  pub catalog_number: Option<String>,
  pub genre: Option<String>,
  pub release_date: Option<String>,
  pub bpm: Option<f64>,
  pub key: Option<String>,
  pub duration_ms: Option<i64>,
  pub isrc: Option<String>,
  pub track_number: Option<i32>,
  pub artwork_url: Option<String>,
  pub url: String,
  pub provider_track_id: Option<String>,
  pub provider_release_id: Option<String>,
}

/// The free-text a tagger provider is searched with.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TagSearchQuery {
  pub artist: Option<String>,
  pub title: String,
}

impl TagSearchQuery {
  /// The provider search term: `"{artist} {title}"`, or just the trimmed title
  /// when no (non-empty) artist is set.
  pub fn term(&self) -> String {
    match self
      .artist
      .as_deref()
      .map(str::trim)
      .filter(|a| !a.is_empty())
    {
      Some(artist) => format!("{artist} {}", self.title.trim()),
      None => self.title.trim().to_string(),
    }
  }
}

/// The outcome of one provider within a multi-provider search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSearchResult {
  pub provider: String,
  pub candidates: Vec<TagCandidate>,
  /// Populated when this provider failed; the other providers still return candidates.
  pub error: Option<String>,
}
