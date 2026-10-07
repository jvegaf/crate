use tauri::State;

use crate::error::Result;
#[cfg(feature = "desktop")]
use crate::models::Track;
use crate::models::{
    AppSettings, ProviderSearchResult, RankedSearchResult, ScoredTagCandidate, TagCandidate,
    TagSearchQuery,
};
#[cfg(feature = "desktop")]
use crate::services::LibraryService;
use crate::services::{SettingsService, TaggerService};

/// Resolve which providers a call may use: every provider id the service knows,
/// filtered by the device-local `AppSettings::tagger_providers_enabled` map.
/// A missing entry means enabled, so a fresh setting and any new provider id
/// both default to on.
fn enabled_provider_ids(tagger: &TaggerService, app_settings: &AppSettings) -> Vec<String> {
    tagger
        .provider_ids()
        .into_iter()
        .filter(|id| {
            app_settings
                .tagger_providers_enabled
                .get(*id)
                .copied()
                .unwrap_or(true)
        })
        .map(String::from)
        .collect()
}

/// Search the enabled metadata providers (Beatport, TraxSource, Bandcamp — see
/// `AppSettings::tagger_providers_enabled`) for candidate tracks matching
/// `artist` + `title`. Per-provider failures are reported inside the returned
/// [`ProviderSearchResult`]s rather than failing the whole call.
#[tauri::command]
pub async fn search_track_tags(
    artist: Option<String>,
    title: String,
    limit: Option<usize>,
    tagger: State<'_, TaggerService>,
    settings: State<'_, SettingsService>,
) -> Result<Vec<ProviderSearchResult>> {
    let limit = limit.unwrap_or(5).clamp(1, 25);
    // `get_settings` locks internally and drops the guard on return, so no DB
    // mutex is held across the `.await` below.
    let app_settings = settings.get_settings()?;
    let enabled_ids = enabled_provider_ids(&tagger, &app_settings);
    tagger
        .search_all(&TagSearchQuery { artist, title }, limit, &enabled_ids)
        .await
}

/// Return the best `max_candidates` (default 5) candidates ranked by similarity
/// against the local track, so the user can pick the correct one. Per-provider
/// failures are reported alongside the candidates instead of failing the call.
///
/// The result is a rank, not a score sort: `similarity_score` is not guaranteed
/// to be non-increasing, because a near-tie group is ordered by provider
/// priority. Scoring combines title, artist, duration, genre, label, bpm and
/// key with the weights stored in `AppSettings::tagger_weights` (defaults when
/// absent or invalid). A signal the local track does not provide cannot earn
/// its full weight, so an aggressive `min_score` can return no candidates.
// The IPC surface mirrors the frontend's flat parameter object; Tauri maps
// camelCase TS fields onto these snake_case arguments.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn search_ranked_track_tags(
    artist: Option<String>,
    title: String,
    duration_ms: Option<i64>,
    genre: Option<String>,
    label: Option<String>,
    bpm: Option<f64>,
    key: Option<String>,
    limit: Option<usize>,
    max_candidates: Option<usize>,
    min_score: Option<f64>,
    tagger: State<'_, TaggerService>,
    settings: State<'_, SettingsService>,
) -> Result<RankedSearchResult> {
    let limit = limit.unwrap_or(10).clamp(1, 25);
    let max_candidates = max_candidates.unwrap_or(5).clamp(1, 20);
    let min_score = min_score.unwrap_or(0.3).clamp(0.0, 1.0);

    // `get_settings` locks internally and drops the guard on return, so no DB
    // mutex is held across the `.await` below.
    let app_settings = settings.get_settings()?;
    let enabled_ids = enabled_provider_ids(&tagger, &app_settings);
    tagger
        .search_and_rank(
            &TagSearchQuery { artist, title },
            duration_ms,
            genre,
            label,
            bpm,
            key,
            limit,
            min_score,
            max_candidates,
            app_settings.tagger_weights.as_deref(),
            &enabled_ids,
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

/// Skip the search and fetch metadata directly from a store URL (Beatport,
/// TraxSource, Bandcamp), returning a single candidate at similarity `1.0`.
///
/// Returns `null` if the URL is not a recognized store URL, if the provider is
/// unavailable in this build (e.g. TraxSource on mobile), or if the provider is
/// disabled in `AppSettings::tagger_providers_enabled`.
#[tauri::command]
pub async fn search_track_by_url(
    url: String,
    tagger: State<'_, TaggerService>,
    settings: State<'_, SettingsService>,
) -> Result<Option<ScoredTagCandidate>> {
    let app_settings = settings.get_settings()?;
    let enabled_ids = enabled_provider_ids(&tagger, &app_settings);
    tagger.search_by_url(&url, &enabled_ids).await
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
