use tauri::State;

use crate::error::Result;
use crate::models::{ProviderSearchResult, RankedSearchResult, TagSearchQuery};
use crate::services::TaggerService;

/// Search every metadata provider (Beatport, TraxSource, Bandcamp) for candidate
/// tracks matching `artist` + `title`. Per-provider failures are reported inside
/// the returned [`ProviderSearchResult`]s rather than failing the whole call.
#[tauri::command]
pub async fn search_track_tags(
  artist: Option<String>,
  title: String,
  limit: Option<usize>,
  tagger: State<'_, TaggerService>,
) -> Result<Vec<ProviderSearchResult>> {
  let limit = limit.unwrap_or(5).clamp(1, 25);
  tagger
    .search_all(&TagSearchQuery { artist, title }, limit)
    .await
}

/// Return the best `max_candidates` (default 5) candidates ranked by similarity
/// against the local track, so the user can pick the correct one. Per-provider
/// failures are reported alongside the candidates instead of failing the call.
///
/// The result is a rank, not a score sort: `similarity_score` is not guaranteed
/// to be non-increasing, because a near-tie group is ordered by provider
/// priority. A local track with no artist tag caps every candidate at `0.7`, so
/// a `min_score` above that returns no candidates.
#[tauri::command]
pub async fn search_ranked_track_tags(
  artist: Option<String>,
  title: String,
  duration_ms: Option<i64>,
  limit: Option<usize>,
  max_candidates: Option<usize>,
  min_score: Option<f64>,
  tagger: State<'_, TaggerService>,
) -> Result<RankedSearchResult> {
  let limit = limit.unwrap_or(10).clamp(1, 25);
  let max_candidates = max_candidates.unwrap_or(5).clamp(1, 20);
  let min_score = min_score.unwrap_or(0.3).clamp(0.0, 1.0);
  tagger
    .search_and_rank(
      &TagSearchQuery { artist, title },
      duration_ms,
      limit,
      min_score,
      max_candidates,
    )
    .await
}
