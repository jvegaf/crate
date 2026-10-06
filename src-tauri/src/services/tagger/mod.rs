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

use scoring::{rank_candidates, ScoringWeights, UnifiedScorer};
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
    /// uses the weights in `weights_json` (the `AppSettings::tagger_weights`
    /// JSON form, defaulting to [`scoring::DEFAULT_WEIGHTS`] when absent or
    /// invalid), with the candidate artist joined as `", "` and a missing local
    /// artist treated as empty. Missing local metadata falls back to the
    /// per-signal neutral scores.
    // The seven local-metadata fields plus the four ranking knobs are the full
    // scoring contract; bundling them would only hide the shape.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_and_rank(
        &self,
        query: &TagSearchQuery,
        local_duration_ms: Option<i64>,
        local_genre: Option<String>,
        local_label: Option<String>,
        local_bpm: Option<f64>,
        local_key: Option<String>,
        search_limit: usize,
        min_score: f64,
        max_candidates: usize,
        weights_json: Option<&str>,
    ) -> Result<RankedSearchResult> {
        let results = self.search_all(query, search_limit).await?;

        Ok(rank_results(
            results,
            query,
            local_duration_ms.unwrap_or(0),
            local_genre.as_deref().unwrap_or(""),
            local_label.as_deref().unwrap_or(""),
            local_bpm.unwrap_or(0.0),
            local_key.as_deref().unwrap_or(""),
            &self.provider_priority(),
            min_score,
            max_candidates,
            weights_json,
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

    /// Skip the search and go directly to the provider identified by the store
    /// `url`, enriching through that provider's `extend` and returning a single
    /// candidate at `1.0` similarity.
    ///
    /// Returns `Ok(None)` when the URL is not a recognized store URL, or when
    /// its provider is unavailable in this build (e.g. TraxSource on mobile).
    pub async fn search_by_url(&self, url: &str) -> Result<Option<ScoredTagCandidate>> {
        let Some((provider_id, track_ref)) = parse_provider_url(url) else {
            return Ok(None);
        };

        let Some(provider) = self
            .providers
            .iter()
            .find(|provider| provider.id() == provider_id)
        else {
            // Provider not available in this build (e.g. traxsource on mobile).
            return Ok(None);
        };

        let (candidate_url, provider_track_id) = match &track_ref {
            ProviderTrackRef::Id(id) => (url.to_string(), Some(id.clone())),
            ProviderTrackRef::Url(track_url) => (track_url.clone(), None),
        };
        let candidate = TagCandidate {
            provider: provider_id.to_string(),
            url: candidate_url,
            provider_track_id,
            ..Default::default()
        };

        // Enrich from the provider; a network failure is an error, not a miss.
        let candidate = provider.extend(&self.client, &candidate).await?;

        Ok(Some(ScoredTagCandidate {
            candidate,
            similarity_score: 1.0,
        }))
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

/// What [`parse_provider_url`] extracted from a store URL.
#[derive(Debug, PartialEq)]
pub(crate) enum ProviderTrackRef {
    /// A provider track ID (Beatport, TraxSource) — the `extend` step fetches
    /// the track detail endpoint directly.
    Id(String),
    /// The full track page URL (Bandcamp) — the `extend` step scrapes it.
    Url(String),
}

/// Parse a store URL and return the provider id and either a track ID (for
/// Beatport/TraxSource) or the full URL (for Bandcamp).
///
/// Returns `None` for any URL that is not a recognized store track URL.
pub(crate) fn parse_provider_url(url: &str) -> Option<(&'static str, ProviderTrackRef)> {
    // Beatport: beatport.com/track/{slug}/{id} or beatport.com/track/{id}
    if url.contains("beatport.com/track/") {
        let path = url.split("beatport.com/track/").nth(1)?;
        let id = path.trim_end_matches('/').split('/').next_back()?;
        if !id.is_empty() {
            return Some(("beatport", ProviderTrackRef::Id(id.to_string())));
        }
    }

    // TraxSource: traxsource.com/track/{id}/{slug} or traxsource.com/track/{id}
    if url.contains("traxsource.com/track/") {
        let path = url.split("traxsource.com/track/").nth(1)?;
        let id = path.trim_end_matches('/').split('/').next()?;
        if !id.is_empty() {
            return Some(("traxsource", ProviderTrackRef::Id(id.to_string())));
        }
    }

    // Bandcamp: {artist}.bandcamp.com/track/{slug}
    if url.contains(".bandcamp.com/track/") {
        return Some(("bandcamp", ProviderTrackRef::Url(url.to_string())));
    }

    None
}

/// Aggregate raw provider results into the ranked best-N answer.
///
/// Pure: no providers, no I/O. A failing provider contributes a
/// [`ProviderError`], and any candidates it did return are still scored rather
/// than dropped; the remaining providers still count. Scoring uses the weights
/// in `weights_json` (the `AppSettings::tagger_weights` JSON form); when the
/// blob is absent, unparsable, or parses to a set that fails
/// [`UnifiedScorer::new`]'s sum validation, the default weights apply — a bad
/// stored setting degrades scoring instead of failing the search. The candidate
/// artist is joined as `", "`, and missing local metadata falls back to the
/// per-signal neutral scores.
// Mirrors `search_and_rank`'s flat scoring contract; see the note there.
#[allow(clippy::too_many_arguments)]
fn rank_results(
    results: Vec<ProviderSearchResult>,
    query: &TagSearchQuery,
    local_duration_ms: i64,
    local_genre: &str,
    local_label: &str,
    local_bpm: f64,
    local_key: &str,
    provider_priority: &[String],
    min_score: f64,
    max_candidates: usize,
    weights_json: Option<&str>,
) -> RankedSearchResult {
    let scorer = match weights_json {
        None => UnifiedScorer::default_weights(),
        Some(json) => {
            match serde_json::from_str::<ScoringWeights>(json)
                .map_err(|e| CrateError::Tagger(format!("invalid tagger_weights JSON: {e}")))
                .and_then(UnifiedScorer::new)
            {
                Ok(scorer) => scorer,
                Err(e) => {
                    log::warn!("Tagger scoring weights rejected ({e}); using defaults");
                    UnifiedScorer::default_weights()
                }
            }
        }
    };
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
                local_genre,
                local_label,
                local_bpm,
                local_key,
                &candidate.title,
                &candidate.artists.join(", "),
                candidate.duration_ms,
                candidate.genre.as_deref(),
                candidate.label.as_deref(),
                candidate.bpm,
                candidate.key.as_deref(),
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

// The sibling `tests` module holds the provider/parsing suite; the store-URL
// direct-match helpers are tested here beside the function under test.
#[cfg(test)]
mod store_url_tests {
    use super::{parse_provider_url, ProviderTrackRef, TaggerService};

    #[test]
    fn parses_beatport_slug_and_id_url() {
        let parsed = parse_provider_url("https://www.beatport.com/track/your-mind/123456");
        assert_eq!(
            parsed,
            Some(("beatport", ProviderTrackRef::Id("123456".to_string())))
        );
    }

    #[test]
    fn parses_beatport_bare_id_url() {
        let parsed = parse_provider_url("https://www.beatport.com/track/123456");
        assert_eq!(
            parsed,
            Some(("beatport", ProviderTrackRef::Id("123456".to_string())))
        );
    }

    #[test]
    fn parses_beatport_url_with_trailing_slash() {
        let parsed = parse_provider_url("https://www.beatport.com/track/your-mind/123456/");
        assert_eq!(
            parsed,
            Some(("beatport", ProviderTrackRef::Id("123456".to_string())))
        );
    }

    #[test]
    fn parses_traxsource_id_and_slug_url() {
        let parsed = parse_provider_url("https://www.traxsource.com/track/123456/your-mind");
        assert_eq!(
            parsed,
            Some(("traxsource", ProviderTrackRef::Id("123456".to_string())))
        );
    }

    #[test]
    fn parses_traxsource_bare_id_url() {
        let parsed = parse_provider_url("https://www.traxsource.com/track/123456");
        assert_eq!(
            parsed,
            Some(("traxsource", ProviderTrackRef::Id("123456".to_string())))
        );
    }

    #[test]
    fn bandcamp_url_is_kept_verbatim() {
        let url = "https://artist.bandcamp.com/track/your-mind";
        let parsed = parse_provider_url(url);
        assert_eq!(
            parsed,
            Some(("bandcamp", ProviderTrackRef::Url(url.to_string())))
        );
    }

    #[test]
    fn unrecognized_urls_parse_to_none() {
        assert_eq!(
            parse_provider_url("https://soundcloud.com/artist/track"),
            None
        );
        assert_eq!(parse_provider_url("https://example.com/track/123456"), None);
        assert_eq!(parse_provider_url("not a url"), None);
        // A store track path with an empty id must not produce an empty id.
        assert_eq!(parse_provider_url("https://www.beatport.com/track/"), None);
        assert_eq!(
            parse_provider_url("https://www.traxsource.com/track/"),
            None
        );
    }

    #[tokio::test]
    async fn search_by_url_returns_none_for_unrecognized_url() {
        let service = TaggerService::new().expect("tagger service builds");
        let result = service
            .search_by_url("https://soundcloud.com/artist/track")
            .await
            .expect("unrecognized URL is not an error");
        assert!(result.is_none());
    }
}
