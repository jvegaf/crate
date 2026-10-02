use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::{CrateError, Result};
use crate::models::{TagCandidate, TagSearchQuery};

use super::TaggerProvider;

/// Beatport OAuth token endpoint (client-credentials grant).
const OAUTH_TOKEN_URL: &str = "https://account.beatport.com/o/token/";

/// Beatport v4 catalog search endpoint.
const API_SEARCH_URL: &str = "https://api.beatport.com/v4/catalog/search/";

/// Beatport v4 single-track detail endpoint (append `{id}/`).
const API_TRACK_URL: &str = "https://api.beatport.com/v4/catalog/tracks";

/// Public OAuth credentials for Beatport's embed app — the same ones onetagger
/// ships. They are not secrets in the security sense (Beatport hands them to
/// browser clients); the v4 API simply requires them to mint a token.
const BEATPORT_CLIENT_ID: &str = "2tiTbKxmQFwnbFjMONU4k7njMRZmV3ZMwRBndiZs";
const BEATPORT_CLIENT_SECRET: &str =
  "RDUJyAk4zFEGtQ8rsTmylDSfxmALRNBn3D1BsRr7MKi3oa1TL9Mq9QxqUPK7loiumXolEWbJcWa4IGAhtwnTz1cSXClGJ1tkkNCNWwRwjxIKTZJKOJxbwaNt0Rm3WG0v";

/// Beatport's client-credentials bearer token, cached across calls.
///
/// Owned by `TaggerService` and cloned into each consumer (the search provider and
/// the recommendations path), so one OAuth token is minted per expiry window for
/// the whole service rather than one per feature. Cloning hands out another handle
/// to the same cache.
#[derive(Clone)]
pub(super) struct BeatportTokenProvider {
  token: Arc<Mutex<Option<CachedToken>>>,
}

impl BeatportTokenProvider {
  pub(super) fn new() -> Self {
    Self {
      token: Arc::new(Mutex::new(None)),
    }
  }

  /// Return a valid bearer token, fetching a new one when the cache is empty or
  /// about to expire.
  ///
  /// The mutex is intentionally held across the token request: overlapping
  /// callers then await the first fetch instead of racing duplicate token calls.
  pub(super) async fn access_token(&self, client: &reqwest::Client) -> Result<String> {
    let mut cached = self.token.lock().await;
    if let Some(token) = cached.as_ref() {
      if token.expires_at > Instant::now() {
        return Ok(token.access_token.clone());
      }
    }

    let response = client
      .post(OAUTH_TOKEN_URL)
      .form(&[
        ("client_id", BEATPORT_CLIENT_ID),
        ("client_secret", BEATPORT_CLIENT_SECRET),
        ("grant_type", "client_credentials"),
      ])
      .send()
      .await
      .map_err(|e| CrateError::Tagger(format!("Beatport token request failed: {e}")))?;

    if !response.status().is_success() {
      let status = response.status();
      return Err(CrateError::Tagger(format!(
        "Beatport token endpoint returned HTTP {status}"
      )));
    }

    let payload: TokenResponse = response
      .json()
      .await
      .map_err(|e| CrateError::Tagger(format!("Failed to parse Beatport token response: {e}")))?;

    let expires_in = payload.expires_in.unwrap_or(600);
    let access_token = payload.access_token;
    let expires_at = Instant::now() + Duration::from_secs(expires_in.saturating_sub(60));
    *cached = Some(CachedToken {
      access_token: access_token.clone(),
      expires_at,
    });

    Ok(access_token)
  }
}

/// Beatport search provider.
///
/// Uses the public v4 JSON API (`api.beatport.com`), which is not behind
/// Cloudflare, instead of scraping the HTML `__NEXT_DATA__` payload. Search
/// requires a bearer token, so it borrows the service-wide
/// [`BeatportTokenProvider`] instead of owning its own.
pub(super) struct BeatportProvider {
  tokens: BeatportTokenProvider,
}

impl BeatportProvider {
  pub(super) fn new(tokens: BeatportTokenProvider) -> Self {
    Self { tokens }
  }
}

/// A bearer token plus the instant it should be considered stale.
struct CachedToken {
  access_token: String,
  expires_at: Instant,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
  access_token: String,
  expires_in: Option<u64>,
}

#[async_trait]
impl TaggerProvider for BeatportProvider {
  fn id(&self) -> &'static str {
    "beatport"
  }

  async fn search(
    &self,
    client: &reqwest::Client,
    query: &TagSearchQuery,
    limit: usize,
  ) -> Result<Vec<TagCandidate>> {
    let token = self.tokens.access_token(client).await?;

    let response = client
      .get(API_SEARCH_URL)
      .query(&[
        ("q", query.term()),
        ("type", "tracks".to_string()),
        ("per_page", limit.to_string()),
      ])
      .bearer_auth(&token)
      .header(reqwest::header::ACCEPT, "application/json")
      .send()
      .await
      .map_err(|e| CrateError::Tagger(format!("Beatport request failed: {e}")))?;

    if !response.status().is_success() {
      let status = response.status();
      return Err(CrateError::Tagger(format!(
        "Beatport search returned HTTP {status}"
      )));
    }

    let body = response
      .text()
      .await
      .map_err(|e| CrateError::Tagger(format!("Beatport response read failed: {e}")))?;

    let mut candidates = parse_beatport_search(&body);
    candidates.truncate(limit);
    Ok(candidates)
  }

  /// Fetch the authoritative per-ID track detail and merge it over the search
  /// candidate.
  ///
  /// The v4 track object is richer than the search payload (artwork, release
  /// detail). An empty `provider_track_id` returns the candidate unchanged; a
  /// detail payload that does not parse also returns it unchanged rather than
  /// guessing.
  async fn extend(
    &self,
    client: &reqwest::Client,
    candidate: &TagCandidate,
  ) -> Result<TagCandidate> {
    let Some(track_id) = candidate
      .provider_track_id
      .as_deref()
      .map(str::trim)
      .filter(|id| !id.is_empty())
    else {
      return Ok(candidate.clone());
    };

    let token = self.tokens.access_token(client).await?;

    let response = client
      .get(format!("{API_TRACK_URL}/{track_id}/"))
      .bearer_auth(&token)
      .header(reqwest::header::ACCEPT, "application/json")
      .send()
      .await
      .map_err(|e| CrateError::Tagger(format!("Beatport detail request failed: {e}")))?;

    if !response.status().is_success() {
      let status = response.status();
      return Err(CrateError::Tagger(format!(
        "Beatport detail returned HTTP {status}"
      )));
    }

    let body = response
      .text()
      .await
      .map_err(|e| CrateError::Tagger(format!("Beatport detail read failed: {e}")))?;

    Ok(enrich_beatport_candidate(candidate, &body))
  }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
  #[serde(default)]
  tracks: Vec<ApiTrack>,
}

#[derive(Debug, Deserialize)]
struct ApiTrack {
  id: Option<Value>,
  name: Option<String>,
  mix_name: Option<String>,
  slug: Option<String>,
  bpm: Option<f64>,
  length_ms: Option<i64>,
  isrc: Option<String>,
  catalog_number: Option<String>,
  publish_date: Option<String>,
  new_release_date: Option<String>,
  key: Option<ApiKey>,
  genre: Option<ApiGenre>,
  release: Option<ApiRelease>,
  artists: Option<Vec<ApiArtist>>,
}

#[derive(Debug, Deserialize)]
struct ApiKey {
  name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiGenre {
  name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiArtist {
  name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
  id: Option<Value>,
  name: Option<String>,
  image: Option<ApiImage>,
  label: Option<ApiLabel>,
}

#[derive(Debug, Deserialize)]
struct ApiImage {
  uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiLabel {
  name: Option<String>,
}

/// Parse the Beatport v4 search payload into candidates.
///
/// Any malformed or empty payload yields an empty vec, never a panic. Tracks
/// without a `name` are skipped.
pub(super) fn parse_beatport_search(body: &str) -> Vec<TagCandidate> {
  let Ok(response) = serde_json::from_str::<SearchResponse>(body) else {
    return Vec::new();
  };

  response
    .tracks
    .into_iter()
    .filter_map(|track| parse_track(track, None))
    .collect()
}

/// Enrich a Beatport candidate from a v4 **single-track** detail payload.
///
/// The detail object is one track (not the search envelope). Fields the detail
/// does not carry fall back to the search candidate, and the candidate's
/// provider identity and URL are always preserved. A malformed payload returns
/// the candidate unchanged instead of guessing.
pub(super) fn enrich_beatport_candidate(candidate: &TagCandidate, body: &str) -> TagCandidate {
  let parsed = serde_json::from_str::<ApiTrack>(body)
    .ok()
    .and_then(|track| parse_track(track, candidate.provider_release_id.as_deref()));

  match parsed {
    Some(parsed) => merge_candidate(candidate, parsed),
    None => candidate.clone(),
  }
}

/// Merge a parsed detail candidate over the input candidate.
///
/// A non-empty parsed value wins; otherwise the input value is kept, so the
/// detail never clears a field the search already populated. `provider`,
/// `provider_track_id`, `provider_release_id` and a non-empty `url` always come
/// from the input.
fn merge_candidate(input: &TagCandidate, parsed: TagCandidate) -> TagCandidate {
  TagCandidate {
    provider: input.provider.clone(),
    title: if parsed.title.trim().is_empty() {
      input.title.clone()
    } else {
      parsed.title
    },
    version: parsed.version.or_else(|| input.version.clone()),
    artists: if parsed.artists.is_empty() {
      input.artists.clone()
    } else {
      parsed.artists
    },
    album: parsed.album.or_else(|| input.album.clone()),
    label: parsed.label.or_else(|| input.label.clone()),
    catalog_number: parsed
      .catalog_number
      .or_else(|| input.catalog_number.clone()),
    genre: parsed.genre.or_else(|| input.genre.clone()),
    release_date: parsed.release_date.or_else(|| input.release_date.clone()),
    bpm: parsed.bpm.or(input.bpm),
    key: parsed.key.or_else(|| input.key.clone()),
    duration_ms: parsed.duration_ms.or(input.duration_ms),
    isrc: parsed.isrc.or_else(|| input.isrc.clone()),
    track_number: parsed.track_number.or(input.track_number),
    artwork_url: parsed.artwork_url.or_else(|| input.artwork_url.clone()),
    url: if input.url.trim().is_empty() {
      parsed.url
    } else {
      input.url.clone()
    },
    provider_track_id: input.provider_track_id.clone().or(parsed.provider_track_id),
    provider_release_id: input
      .provider_release_id
      .clone()
      .or(parsed.provider_release_id),
  }
}

/// Parse one Beatport v4 track object.
///
/// `release_id_fallback` supplies the release id when the track payload does not
/// embed one (the search path passes `None`); the detail path passes the
/// candidate's own `provider_release_id` so a missing release id is not lost.
fn parse_track(track: ApiTrack, release_id_fallback: Option<&str>) -> Option<TagCandidate> {
  let title = track.name?;

  let version = track
    .mix_name
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());

  let artists = track
    .artists
    .unwrap_or_default()
    .into_iter()
    .filter_map(|artist| artist.name)
    .collect::<Vec<_>>();

  let key = track
    .key
    .and_then(|key| key.name)
    .and_then(|name| normalize_beatport_key(&name));

  let genre = track.genre.and_then(|genre| genre.name);

  let release_date = track
    .new_release_date
    .or(track.publish_date)
    .map(|date| date.chars().take(10).collect::<String>());

  let (album, label, mut provider_release_id, artwork_url) = match track.release {
    Some(release) => (
      release.name,
      release.label.and_then(|label| label.name),
      release.id.as_ref().and_then(value_to_string),
      release.image.and_then(|image| image.uri),
    ),
    None => (None, None, None, None),
  };
  if provider_release_id.is_none() {
    provider_release_id = release_id_fallback.map(str::to_string);
  }

  let provider_track_id = track.id.as_ref().and_then(value_to_string);
  let url = match (track.slug.as_deref(), provider_track_id.as_deref()) {
    (Some(slug), Some(id)) if !slug.is_empty() => {
      format!("https://www.beatport.com/track/{slug}/{id}")
    }
    (_, Some(id)) => format!("https://www.beatport.com/track/{id}"),
    _ => String::new(),
  };

  Some(TagCandidate {
    provider: "beatport".to_string(),
    title,
    version,
    artists,
    album,
    label,
    catalog_number: track.catalog_number,
    genre,
    release_date,
    bpm: track.bpm,
    key,
    duration_ms: track.length_ms,
    isrc: track.isrc,
    track_number: None,
    artwork_url,
    url,
    provider_track_id,
    provider_release_id,
  })
}

/// Normalize Beatport keys to Crate's notation (`"G Minor"` -> `"Gm"`).
fn normalize_beatport_key(raw: &str) -> Option<String> {
  let normalized = raw.replace(" Major", "").replace(" Minor", "m");
  let trimmed = normalized.trim();
  if trimmed.is_empty() {
    None
  } else {
    Some(trimmed.to_string())
  }
}

/// Convert a JSON scalar to its string form (numbers become their decimal text).
fn value_to_string(value: &Value) -> Option<String> {
  match value {
    Value::String(s) => Some(s.clone()),
    Value::Number(n) => Some(n.to_string()),
    _ => None,
  }
}
