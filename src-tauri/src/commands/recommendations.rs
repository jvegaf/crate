use tauri::State;

use crate::error::Result;
use crate::models::BeatportRecommendation;
use crate::services::TaggerService;

/// Fetch Beatport's "similar tracks" recommendations for a numeric Beatport track
/// id, the id the caller extracts from the track's stored Beatport URL.
///
/// Returns an empty list when Beatport has nothing to recommend or the payload
/// does not parse; auth, rate-limit and transport failures are errors, which the
/// frontend receives as plain strings.
///
/// Not desktop-gated: like the Beatport tagger provider this needs only `reqwest`,
/// so it compiles for the mobile targets too.
#[tauri::command]
pub async fn find_beatport_similar_tracks(
  track_id: u64,
  tagger: State<'_, TaggerService>,
) -> Result<Vec<BeatportRecommendation>> {
  tagger.find_similar_tracks(track_id).await.inspect_err(|e| {
    log::warn!("find_beatport_similar_tracks failed for Beatport track {track_id}: {e}")
  })
}
