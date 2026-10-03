use tauri::{Manager, State};

use crate::error::Result;
use crate::models::BeatportRecommendation;
use crate::proxy::{self, ProxyServerState};
use crate::services::TaggerService;
use crate::ProxyServerPort;

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

/// Register Beatport recommendation sample URLs with the local stream proxy and return one
/// proxy URL (`http://127.0.0.1:{port}/samples/{key}`) per input entry, `None` where the URL
/// was rejected (non-`geo-samples.beatport.com` host or non-https) so the caller falls back
/// to the direct URL.
///
/// Routing samples through the proxy works around WebKitGTK's hanging https fetch for the
/// remote sample CDN (local 127.0.0.1 playback is unaffected). A missing proxy state is not
/// an error: every entry comes back `None` and the feature keeps working wherever the
/// webview can fetch remote mp3s directly. Not desktop-gated — the proxy binds on every
/// platform, like `fetch_preview_stream`.
#[tauri::command]
pub async fn register_beatport_sample_streams(
    urls: Vec<String>,
    app: tauri::AppHandle,
) -> Result<Vec<Option<String>>> {
    let fallback = || urls.iter().map(|_| None).collect::<Vec<_>>();
    let (Some(port), Some(state)) = (
        app.try_state::<ProxyServerPort>(),
        app.try_state::<ProxyServerState>(),
    ) else {
        log::warn!(
            "Stream proxy state unavailable; recommendation samples fall back to direct URLs"
        );
        return Ok(fallback());
    };
    Ok(urls
        .iter()
        .map(|url| proxy::register_sample(&state.samples, port.0, url))
        .collect())
}
