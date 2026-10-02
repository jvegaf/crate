//! Automatic song tagger — metadata providers (Beatport, TraxSource, Bandcamp).
//!
//! A search returns candidate tracks for an artist + title; `extend` enriches one
//! chosen candidate with its per-ID detail. Network-only and mostly mobile-safe:
//! Beatport and Bandcamp run everywhere, TraxSource is desktop-only because it
//! shells out to the system `curl` binary.
//!
//! The Beatport recommendations path (`recommendations`) is not a tagger provider:
//! it is a separate command that reuses this module's HTTP client and shared
//! Beatport OAuth token cache.

mod bandcamp;
mod beatport;
mod http;
mod recommendations;
mod scoring;
#[cfg(test)]
mod tests;
#[cfg(feature = "desktop")]
mod traxsource;

use scoring::{rank_candidates, UnifiedScorer};
#[cfg(feature = "desktop")]
use traxsource::TraxSourceProvider;

use async_trait::async_trait;

use crate::error::{CrateError, Result};
use crate::models::{
  ProviderError, ProviderSearchResult, RankedSearchResult, ScoredTagCandidate, TagCandidate,
  TagSearchQuery,
};

/// Shared HTTP client plus the persistent provider instances.
///
/// Providers are owned by the service (not rebuilt per call) so Beatport's OAuth
/// token cache survives across searches and recommendations.
pub struct TaggerService {
  client: reqwest::Client,
  providers: Vec<Box<dyn TaggerProvider>>,
  /// The one Beatport token cache, shared with the Beatport provider inside
  /// `providers` so both features mint a single token per expiry window.
  beatport_tokens: beatport::BeatportTokenProvider,
}

impl TaggerService {
  /// Build the service. The only failure mode is an HTTP client that cannot be built.
  pub fn new() -> Result<Self> {
    let beatport_tokens = beatport::BeatportTokenProvider::new();
    let mut providers: Vec<Box<dyn TaggerProvider>> = vec![Box::new(
      beatport::BeatportProvider::new(beatport_tokens.clone()),
    )];
    #[cfg(feature = "desktop")]
    providers.push(Box::new(TraxSourceProvider));
    providers.push(Box::new(bandcamp::BandcampProvider));

    Ok(Self {
      client: http::build_client()?,
      providers,
      beatport_tokens,
    })
  }

  /// Search every available provider in fixed order.
  ///
  /// Desktop order is `beatport`, `traxsource`, `bandcamp`; on mobile the
  /// desktop-only TraxSource provider is absent, leaving `beatport`, `bandcamp`.
  ///
  /// A provider returning `Err` is reported in that provider's
  /// [`ProviderSearchResult::error`] with empty candidates; the remaining
  /// providers still run.
  pub async fn search_all(
    &self,
    query: &TagSearchQuery,
    limit: usize,
  ) -> Result<Vec<ProviderSearchResult>> {
    let mut results = Vec::with_capacity(self.providers.len());
    for provider in &self.providers {
      let result = match provider.search(&self.client, query, limit).await {
        Ok(candidates) => ProviderSearchResult {
          provider: provider.id().to_string(),
          candidates,
          error: None,
        },
        Err(e) => ProviderSearchResult {
          provider: provider.id().to_string(),
          candidates: Vec::new(),
          error: Some(e.to_string()),
        },
      };
      results.push(result);
    }

    Ok(results)
  }

  /// Provider ids in service order — the tie-break priority for equal scores.
  fn provider_priority(&self) -> Vec<String> {
    self.providers.iter().map(|p| p.id().to_string()).collect()
  }

  /// Search every provider, score each candidate against the local track, and
  /// return the best `max_candidates` ordered by similarity.
  ///
  /// `max_candidates` is used as-is, so callers must pass at least `1`; the
  /// Tauri command applies the clamp.
  ///
  /// A failing provider contributes a [`ProviderError`] through
  /// [`RankedSearchResult::errors`], and any candidates it did return are still
  /// scored rather than dropped; the remaining providers still count. Scoring
  /// uses the default weights (title 0.5, artist 0.3, duration 0.2), with the
  /// candidate artist joined as `", "` and a missing local artist treated as
  /// empty.
  pub async fn search_and_rank(
    &self,
    query: &TagSearchQuery,
    local_duration_ms: Option<i64>,
    search_limit: usize,
    min_score: f64,
    max_candidates: usize,
  ) -> Result<RankedSearchResult> {
    let results = self.search_all(query, search_limit).await?;

    Ok(rank_results(
      results,
      query,
      local_duration_ms.unwrap_or(0),
      &self.provider_priority(),
      min_score,
      max_candidates,
    ))
  }

  /// Enrich one candidate with its per-ID detail, dispatching on
  /// `candidate.provider`. An unknown provider is a [`CrateError::Tagger`].
  ///
  /// The provider id is the sole routing key, so the request cannot enrich a
  /// candidate through the wrong provider. On mobile the desktop-gated
  /// TraxSource provider is absent and `"traxsource"` is therefore unknown — an
  /// error, not a panic.
  pub async fn extend_candidate(&self, candidate: &TagCandidate) -> Result<TagCandidate> {
    let provider = self
      .providers
      .iter()
      .find(|provider| provider.id() == candidate.provider.as_str())
      .ok_or_else(|| {
        CrateError::Tagger(format!("Unknown tagger provider: {}", candidate.provider))
      })?;

    provider.extend(&self.client, candidate).await
  }

  /// Download a remote artwork URL to a temporary file and return its path.
  ///
  /// The caller owns the file and must remove it. An empty URL is an error.
  /// The extension is derived from the response `Content-Type`, then the URL
  /// path, then defaults to `jpg`.
  pub async fn download_artwork(&self, url: &str) -> Result<std::path::PathBuf> {
    let url = url.trim();
    if url.is_empty() {
      return Err(CrateError::Tagger(
        "Cannot download artwork: empty URL".to_string(),
      ));
    }

    let response = self
      .client
      .get(url)
      .send()
      .await
      .map_err(|e| CrateError::Tagger(format!("Artwork request failed: {e}")))?;

    if !response.status().is_success() {
      let status = response.status();
      return Err(CrateError::Tagger(format!(
        "Artwork download returned HTTP {status}"
      )));
    }

    let content_type = response
      .headers()
      .get(reqwest::header::CONTENT_TYPE)
      .and_then(|value| value.to_str().ok())
      .map(str::to_string);
    let extension = artwork_extension(content_type.as_deref(), url);

    let bytes = response
      .bytes()
      .await
      .map_err(|e| CrateError::Tagger(format!("Artwork read failed: {e}")))?;

    let file_name = uuid::Uuid::new_v4();
    let path = std::env::temp_dir().join(format!("crate-artwork-{file_name}.{extension}"));
    std::fs::write(&path, &bytes)?;

    Ok(path)
  }
}

/// Decide the file extension for a downloaded artwork image.
///
/// The response `Content-Type` wins when it maps to a known image type;
/// otherwise the URL's own extension is used, and an unknown or absent
/// extension falls back to `jpg`.
fn artwork_extension(content_type: Option<&str>, url: &str) -> &'static str {
  let from_content_type = content_type.and_then(|value| {
    let mime = value.split(';').next()?.trim().to_ascii_lowercase();
    match mime.as_str() {
      "image/jpeg" | "image/jpg" => Some("jpg"),
      "image/png" => Some("png"),
      "image/webp" => Some("webp"),
      "image/gif" => Some("gif"),
      _ => None,
    }
  });
  if let Some(extension) = from_content_type {
    return extension;
  }

  let without_fragment = url.split(['?', '#']).next().unwrap_or(url);
  let last_segment = without_fragment
    .rsplit('/')
    .next()
    .unwrap_or(without_fragment);
  match last_segment
    .rsplit_once('.')
    .map(|(_, ext)| ext.to_ascii_lowercase())
    .as_deref()
  {
    Some("jpg") | Some("jpeg") => "jpg",
    Some("png") => "png",
    Some("webp") => "webp",
    Some("gif") => "gif",
    _ => "jpg",
  }
}

/// Aggregate raw provider results into the ranked best-N answer.
///
/// Pure: no providers, no I/O. A failing provider contributes a
/// [`ProviderError`], and any candidates it did return are still scored rather
/// than dropped; the remaining providers still count. Scoring uses the default
/// weights (title 0.5, artist 0.3, duration 0.2),
/// with the candidate artist joined as `", "` and a missing local artist treated
/// as empty.
fn rank_results(
  results: Vec<ProviderSearchResult>,
  query: &TagSearchQuery,
  local_duration_ms: i64,
  provider_priority: &[String],
  min_score: f64,
  max_candidates: usize,
) -> RankedSearchResult {
  let scorer = UnifiedScorer::default_weights();
  let local_title = query.title.as_str();
  let local_artist = query.artist.as_deref().unwrap_or("");

  let mut candidates = Vec::new();
  let mut errors = Vec::new();
  for result in results {
    if let Some(error) = result.error {
      errors.push(ProviderError {
        provider: result.provider,
        error,
      });
    }
    for candidate in result.candidates {
      let similarity_score = scorer.score(
        local_title,
        local_artist,
        local_duration_ms,
        &candidate.title,
        &candidate.artists.join(", "),
        candidate.duration_ms,
      );
      candidates.push(ScoredTagCandidate {
        candidate,
        similarity_score,
      });
    }
  }

  let candidates = rank_candidates(candidates, provider_priority, min_score, max_candidates);
  RankedSearchResult { candidates, errors }
}

/// A metadata provider that can search for candidate tracks.
#[async_trait]
pub(super) trait TaggerProvider: Send + Sync {
  /// Stable provider identifier used on the wire.
  fn id(&self) -> &'static str;

  /// Search the provider and return up to `limit` candidates.
  async fn search(
    &self,
    client: &reqwest::Client,
    query: &TagSearchQuery,
    limit: usize,
  ) -> Result<Vec<TagCandidate>>;

  /// Enrich one candidate by its provider id. The default returns the candidate
  /// unchanged: providers override this only where their search payload actually
  /// leaves fields empty.
  async fn extend(
    &self,
    _client: &reqwest::Client,
    candidate: &TagCandidate,
  ) -> Result<TagCandidate> {
    Ok(candidate.clone())
  }
}
