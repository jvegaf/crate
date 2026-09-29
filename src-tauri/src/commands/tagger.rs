use tauri::State;

use crate::error::Result;
use crate::models::{ProviderSearchResult, TagSearchQuery};
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
