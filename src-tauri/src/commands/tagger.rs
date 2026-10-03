use tauri::State;

use crate::error::Result;
#[cfg(feature = "desktop")]
use crate::models::Track;
use crate::models::{ProviderSearchResult, RankedSearchResult, TagCandidate, TagSearchQuery};
#[cfg(feature = "desktop")]
use crate::services::LibraryService;
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

/// Enrich one provider candidate with its per-ID detail (label, release date,
/// duration, artwork) before the user applies it. The provider is taken from the
/// candidate itself, so the request cannot disagree with it; an unknown provider
/// is an error.
#[tauri::command]
pub async fn extend_track_tag(
    candidate: TagCandidate,
    tagger: State<'_, TaggerService>,
) -> Result<TagCandidate> {
    let provider = &candidate.provider;
    let candidate_id = &candidate.provider_track_id;
    tagger.extend_candidate(&candidate).await.inspect_err(|e| {
        log::warn!("extend_track_tag failed for candidate {provider} {candidate_id:?}: {e}")
    })
}

/// Download a candidate's artwork and set it as the track's artwork, reusing the
/// library's user-provided artwork path.
///
/// Desktop-only because it depends on [`LibraryService`], which is gated out of
/// the mobile build. The downloaded temp file is removed on every path.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn set_track_artwork_from_url(
    track_id: String,
    url: String,
    tagger: State<'_, TaggerService>,
    library: State<'_, LibraryService>,
) -> Result<Track> {
    let temp_path = tagger.download_artwork(&url).await.inspect_err(|e| {
        log::warn!("set_track_artwork_from_url: artwork download failed for track {track_id}: {e}")
    })?;

    let result = library
        .set_track_artwork(&track_id, &temp_path)
        .inspect_err(|e| log::warn!("set_track_artwork_from_url failed for track {track_id}: {e}"));

    // The temp file is ours alone: remove it whether or not the library write
    // succeeded, so a failed apply cannot leak it.
    if let Err(e) = std::fs::remove_file(&temp_path) {
        log::warn!("Failed to remove temporary artwork file {temp_path:?}: {e}");
    }

    result
}
