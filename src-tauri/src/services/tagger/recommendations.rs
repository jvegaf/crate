use serde_json::Value;

use crate::error::{CrateError, Result};
use crate::models::BeatportRecommendation;

use super::TaggerService;

/// Beatport catalog v1 "similar tracks" recommendations endpoint.
///
/// Takes the numeric Beatport track id as the `id` query parameter and returns a
/// top-level JSON array. Authenticated with the same client-credentials bearer
/// token the Beatport search provider uses — no HTML scraping involved.
const API_RECOMMENDATIONS_URL: &str = "https://api.beatport.com/catalog/v1/recommendations/tracks/";

/// The literal size placeholder Beatport ships inside its image URLs.
const ARTWORK_SIZE_PLACEHOLDER: &str = "{w}x{h}";

/// Replacement for [`ARTWORK_SIZE_PLACEHOLDER`], so the URL the frontend receives
/// is loadable as-is.
const ARTWORK_SIZE: &str = "800x800";

impl TaggerService {
  /// Fetch Beatport's recommended similar tracks for one Beatport track id.
  ///
  /// Reuses the shared Beatport token cache owned by the service, so a search and
  /// a recommendations call in the same session mint one OAuth token rather than
  /// two. Transport failures and non-success statuses are errors; a body that is
  /// not an array of recommendations yields an empty list (see
  /// [`parse_beatport_recommendations`]).
  pub async fn find_similar_tracks(&self, track_id: u64) -> Result<Vec<BeatportRecommendation>> {
    let token = self.beatport_tokens.access_token(&self.client).await?;

    let response = self
      .client
      .get(API_RECOMMENDATIONS_URL)
      .query(&[("id", track_id.to_string())])
      .bearer_auth(&token)
      .header(reqwest::header::ACCEPT, "application/json")
      .send()
      .await
      .map_err(|e| CrateError::Tagger(format!("Beatport recommendations request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
      return Err(CrateError::Tagger(match status.as_u16() {
        401 => "Beatport recommendations: Beatport rejected the API token (HTTP 401)".to_string(),
        404 => format!("Beatport recommendations: Beatport track {track_id} not found (HTTP 404)"),
        429 => "Beatport recommendations: rate limited by Beatport (HTTP 429), try again shortly"
          .to_string(),
        code => format!("Beatport recommendations returned HTTP {code}"),
      }));
    }

    let body = response.text().await.map_err(|e| {
      CrateError::Tagger(format!(
        "Beatport recommendations response read failed: {e}"
      ))
    })?;

    Ok(parse_beatport_recommendations(&body))
  }
}

/// Parse the recommendations payload into DTOs.
///
/// Mirrors the tagger's `parse_beatport_search` contract: a payload that is not a
/// JSON array yields an empty vec rather than an error. Each item is decoded on
/// its own, so one item missing its `track_id` / `track_name` identity is dropped
/// without discarding the rest of the batch. Image URLs have their `{w}x{h}`
/// placeholder resolved to [`ARTWORK_SIZE`].
pub(super) fn parse_beatport_recommendations(body: &str) -> Vec<BeatportRecommendation> {
  let Ok(items) = serde_json::from_str::<Vec<Value>>(body) else {
    return Vec::new();
  };

  items
    .into_iter()
    .filter_map(|item| {
      let mut recommendation = serde_json::from_value::<BeatportRecommendation>(item).ok()?;
      recommendation.track_waveform_url = resolve_image_size(&recommendation.track_waveform_url);
      if let Some(release) = recommendation.release.as_mut() {
        release.image_url = resolve_image_size(&release.image_url);
      }
      Some(recommendation)
    })
    .collect()
}

/// Replace Beatport's literal `{w}x{h}` size placeholder in an image URL.
///
/// Both `release.image_url` and `track_waveform_url` are templates on the wire;
/// leaving the braces in place would hand the frontend a URL that never loads.
fn resolve_image_size(url: &Option<String>) -> Option<String> {
  url
    .as_ref()
    .map(|value| value.replace(ARTWORK_SIZE_PLACEHOLDER, ARTWORK_SIZE))
}
