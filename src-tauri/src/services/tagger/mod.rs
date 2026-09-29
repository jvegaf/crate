//! Automatic song tagger — metadata **search** providers (Beatport, TraxSource, Bandcamp).
//!
//! This slice is search-only: it returns candidate tracks for an artist + title.
//! Matching/scoring and per-ID enrichment (`extend`) are deferred. Network-only
//! and mostly mobile-safe: Beatport and Bandcamp run everywhere, TraxSource is
//! desktop-only because it shells out to the system `curl` binary.

mod bandcamp;
mod beatport;
mod http;
#[cfg(test)]
mod tests;
#[cfg(feature = "desktop")]
mod traxsource;

#[cfg(feature = "desktop")]
use traxsource::TraxSourceProvider;

use async_trait::async_trait;

use crate::error::Result;
use crate::models::{ProviderSearchResult, TagCandidate, TagSearchQuery};

/// Shared HTTP client plus the persistent provider instances.
///
/// Providers are owned by the service (not rebuilt per call) so Beatport's OAuth
/// token cache survives across searches.
pub struct TaggerService {
  client: reqwest::Client,
  providers: Vec<Box<dyn TaggerProvider>>,
}

impl TaggerService {
  /// Build the service. The only failure mode is an HTTP client that cannot be built.
  pub fn new() -> Result<Self> {
    let mut providers: Vec<Box<dyn TaggerProvider>> =
      vec![Box::new(beatport::BeatportProvider::new())];
    #[cfg(feature = "desktop")]
    providers.push(Box::new(TraxSourceProvider));
    providers.push(Box::new(bandcamp::BandcampProvider));

    Ok(Self {
      client: http::build_client()?,
      providers,
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
}
