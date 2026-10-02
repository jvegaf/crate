use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use crate::error::{CrateError, Result};
use crate::services::DiscoveryService;

use tauri::Manager;

/// The only host the sample route may proxy: Beatport's public preview-mp3 CDN. Exact host
/// matching (not suffix) keeps the proxy from being pointed at arbitrary URLs (SSRF).
const SAMPLE_HOST: &str = "geo-samples.beatport.com";

/// Size of each sequential download chunk (~4 MB).
/// With n-param transformation, YouTube CDN allows full downloads for ANDROID_VR client URLs.
const CHUNK_SIZE: u64 = 4_194_304;

/// Maximum number of cached audio entries kept in memory.
const MAX_CACHE_ENTRIES: usize = 3;

/// Maximum total download size to prevent runaway downloads (~50 MB).
const MAX_TOTAL_SIZE: u64 = 52_428_800;

/// Fully downloaded audio file held in memory for instant range serving.
struct CachedAudio {
  data: Vec<u8>,
  content_type: String,
}

/// Shared state threaded into every axum proxy request handler.
#[derive(Clone)]
pub(crate) struct ProxyServerState {
  pub app_handle: tauri::AppHandle,
  pub client: reqwest::Client,
  /// Cache of fully downloaded audio files, keyed by "{release_id}/{track_position}" for
  /// discovery previews and "sample:{key}" for Beatport recommendation samples.
  cache: Arc<tokio::sync::RwLock<HashMap<String, Arc<CachedAudio>>>>,
  /// In-flight downloads: maps cache key to a watch receiver that signals completion.
  /// `None` = download in progress, `Some(true)` = success, `Some(false)` = failed.
  downloads: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::watch::Receiver<Option<bool>>>>>,
  /// Registered sample routes: proxy key → remote https URL. A std `Mutex` is fine here
  /// because every access locks, clones out, and drops the guard before any `.await`.
  pub(crate) samples: Arc<Mutex<HashMap<String, String>>>,
}

impl ProxyServerState {
  pub fn new(app_handle: tauri::AppHandle, client: reqwest::Client) -> Self {
    Self {
      app_handle,
      client,
      cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
      downloads: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
      samples: Arc::new(Mutex::new(HashMap::new())),
    }
  }
}

/// Validate a sample URL for proxying: exactly `https://{SAMPLE_HOST}` and nothing else, so the
/// proxy can never be coerced into fetching localhost or attacker-controlled hosts.
pub(crate) fn validate_sample_url(raw: &str) -> Result<reqwest::Url> {
  let url = reqwest::Url::parse(raw)
    .map_err(|e| CrateError::Discovery(format!("Invalid sample URL: {e}")))?;
  if url.scheme() != "https" || url.host_str() != Some(SAMPLE_HOST) {
    return Err(CrateError::Discovery(format!(
      "Sample URL host {raw:?} is not {SAMPLE_HOST} over https"
    )));
  }
  Ok(url)
}

/// Deterministic short key for a validated sample URL (SipHash-1-3 with the fixed
/// `DefaultHasher` seed, hex-encoded). Stable across runs; 64-bit collision risk is
/// irrelevant at the handful-of-samples-per-modal scale.
pub(crate) fn sample_stream_key(url: &reqwest::Url) -> String {
  let mut hasher = DefaultHasher::new();
  url.as_str().hash(&mut hasher);
  format!("{:016x}", hasher.finish())
}

/// Register one sample URL and return the localhost proxy URL that serves it, or `None` when the
/// URL fails validation (the caller falls back to the direct URL). The guard never spans an await.
pub(crate) fn register_sample(
  samples: &Mutex<HashMap<String, String>>,
  port: u16,
  raw: &str,
) -> Option<String> {
  let Ok(url) = validate_sample_url(raw) else {
    log::warn!("Rejected sample URL for stream proxy registration: {raw}");
    return None;
  };
  let key = sample_stream_key(&url);
  let mut guard = samples.lock().ok()?;
  guard.insert(key.clone(), raw.to_string());
  Some(format!("http://127.0.0.1:{port}/samples/{key}"))
}

/// Parse a `bytes=start-[end]` Range header value and return `(start, optional_end)`.
fn parse_bytes_range(header: Option<&str>) -> (u64, Option<u64>) {
  let s = match header {
    Some(h) => h.trim_start_matches("bytes="),
    None => return (0, None),
  };
  let mut parts = s.splitn(2, '-');
  let start = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
  let end = parts.next().and_then(|v| v.parse().ok());
  (start, end)
}

/// Parse total file size from a `Content-Range: bytes start-end/total` header.
fn parse_total_from_content_range(headers: &reqwest::header::HeaderMap) -> Option<u64> {
  let cr = headers.get("Content-Range")?.to_str().ok()?;
  cr.split('/').next_back()?.parse().ok()
}

/// Top-level axum handler for `GET /{release_id}/{track_position}`.
///
/// YouTube stream URLs are signed for non-browser user-agents. The HTML5 Audio element sends a
/// browser UA, causing a 403. This server fetches the stream with the correct UA server-side and
/// proxies the bytes back to the media element. Using a real HTTP server (rather than a custom URI
/// scheme) ensures WKWebView's AVFoundation media layer can seek correctly via Range requests.
pub(crate) async fn proxy_http_handler(
  axum::extract::Path((release_id, track_position)): axum::extract::Path<(String, i32)>,
  req_headers: axum::http::HeaderMap,
  axum::extract::State(state): axum::extract::State<ProxyServerState>,
) -> axum::http::Response<axum::body::Body> {
  match proxy_http_handler_inner(&release_id, track_position, &req_headers, &state).await {
    Ok(r) => r,
    Err(e) => {
      log::error!("Stream proxy error: {e}");
      axum::http::Response::builder()
        .status(502)
        .header("Content-Type", "text/plain")
        .body(axum::body::Body::from("Internal proxy error"))
        .unwrap()
    }
  }
}

async fn proxy_http_handler_inner(
  release_id: &str,
  track_position: i32,
  req_headers: &axum::http::HeaderMap,
  state: &ProxyServerState,
) -> Result<axum::http::Response<axum::body::Body>> {
  let cache_key = format!("{release_id}/{track_position}");
  let incoming_range = req_headers.get("Range").and_then(|v| v.to_str().ok());
  let has_range_header = incoming_range.is_some();
  let (start, end_opt) = parse_bytes_range(incoming_range);

  // 1. Check memory cache — if audio data is available, serve from memory.
  {
    let cache = state.cache.read().await;
    if let Some(cached) = cache.get(&cache_key) {
      return serve_range_from_cache(cached, start, end_opt, has_range_header);
    }
  }

  // 2. Check disk cache — load into memory and serve if available.
  {
    let discovery = state.app_handle.state::<DiscoveryService>();
    if let Ok(Some((content_type, file_size))) =
      discovery.get_cached_audio_meta(release_id, track_position)
    {
      let file_path = discovery.audio_cache_path(release_id, track_position);

      if let Ok(data) = std::fs::read(&file_path) {
        if data.len() as i64 == file_size {
          let cached = Arc::new(CachedAudio { data, content_type });

          // Promote to memory cache
          let mut cache = state.cache.write().await;
          while cache.len() >= MAX_CACHE_ENTRIES {
            if let Some(oldest_key) = cache.keys().next().cloned() {
              cache.remove(&oldest_key);
            }
          }
          cache.insert(cache_key.clone(), cached.clone());

          return serve_range_from_cache(&cached, start, end_opt, has_range_header);
        } else {
          log::warn!("Audio cache file size mismatch for {cache_key}, removing stale entry");
          let _ = std::fs::remove_file(&file_path);
          let _ = discovery.delete_cached_audio_files(release_id);
        }
      }
    }
  }

  // 3. Check if a download is already in progress for this key.
  //    If so, wait for it. If not, start one.
  let mut rx = {
    let mut downloads = state.downloads.lock().await;
    if let Some(existing_rx) = downloads.get(&cache_key) {
      existing_rx.clone()
    } else {
      let discovery = state.app_handle.state::<DiscoveryService>();
      let cached_stream = discovery
        .get_cached_stream(release_id, track_position)?
        .ok_or_else(|| CrateError::Discovery("No cached stream for proxy request".into()))?;

      // Use proxy_ua if set (YouTube/Discogs), empty string otherwise (Bandcamp/SoundCloud)
      let proxy_ua = cached_stream.proxy_ua.unwrap_or_default();

      let (tx, rx) = tokio::sync::watch::channel(None);
      downloads.insert(cache_key.clone(), rx.clone());

      let state_clone = state.clone();
      let url = cached_stream.stream_url.clone();
      let key = cache_key.clone();
      let rid = release_id.to_string();
      let tp = track_position;

      tokio::spawn(async move {
        let success = match download_stream(&state_clone, &url, &proxy_ua).await {
          Ok(cached_audio) => {
            let entry = Arc::new(cached_audio);
            let mut cache = state_clone.cache.write().await;

            while cache.len() >= MAX_CACHE_ENTRIES {
              if let Some(oldest_key) = cache.keys().next().cloned() {
                cache.remove(&oldest_key);
              }
            }

            cache.insert(key.clone(), entry.clone());

            // Write to disk cache
            let discovery = state_clone.app_handle.state::<DiscoveryService>();
            let file_path = discovery.audio_cache_path(&rid, tp);
            if let Err(e) = std::fs::create_dir_all(file_path.parent().unwrap()) {
              log::warn!("Failed to create audio cache dir: {e}");
            } else if let Err(e) = std::fs::write(&file_path, &entry.data) {
              log::warn!("Failed to write audio cache file: {e}");
            } else if let Err(e) = discovery.save_audio_cache_entry(
              &rid,
              tp,
              &entry.content_type,
              entry.data.len() as i64,
            ) {
              log::warn!("Failed to record audio cache entry: {e}");
            } else {
              log::info!(
                "Cached audio to disk: {rid}/{tp} ({} bytes)",
                entry.data.len()
              );
            }

            true
          }
          Err(e) => {
            log::warn!("Stream download failed for {key}: {e}");
            false
          }
        };

        let _ = tx.send(Some(success));
        state_clone.downloads.lock().await.remove(&key);
      });

      rx
    }
  };

  // 4. Wait for the download to complete.
  if rx.changed().await.is_err() {
    return Err(CrateError::Discovery(
      "Download task dropped unexpectedly".into(),
    ));
  }

  if *rx.borrow() != Some(true) {
    return Ok(
      axum::http::Response::builder()
        .status(502)
        .header("Content-Type", "text/plain")
        .body(axum::body::Body::from("Stream download failed"))
        .unwrap(),
    );
  }

  // 5. Serve from cache.
  let cache = state.cache.read().await;
  let cached = cache
    .get(&cache_key)
    .ok_or_else(|| CrateError::Discovery("Download succeeded but cache entry missing".into()))?;
  serve_range_from_cache(cached, start, end_opt, has_range_header)
}

/// Download an audio stream in sequential chunks.
///
/// Downloads chunks sequentially. Without n-param transformation, YouTube CDN throttles
/// downloads to ~1 MB per video. With transformation, full downloads are possible.
/// Browser-compatible clients (WEB, WEB_EMBEDDED) don't go through the proxy at all.
async fn download_stream(
  state: &ProxyServerState,
  stream_url: &str,
  proxy_ua: &str,
) -> Result<CachedAudio> {
  let mut data = Vec::new();
  let mut content_type = String::from("audio/mp4");
  let mut total_size: Option<u64> = None;

  loop {
    let offset = data.len() as u64;

    if let Some(total) = total_size {
      if offset >= total {
        break;
      }
    }
    if offset >= MAX_TOTAL_SIZE {
      break;
    }

    let range_end = offset + CHUNK_SIZE - 1;
    let mut req = state
      .client
      .get(stream_url)
      .header("Range", format!("bytes={offset}-{range_end}"));
    if !proxy_ua.is_empty() {
      req = req.header("User-Agent", proxy_ua);
    }
    let response = req
      .send()
      .await
      .map_err(|e| CrateError::Discovery(format!("Stream chunk request failed: {e:#}")))?;

    let status = response.status();
    if !status.is_success() && status.as_u16() != 206 {
      if data.is_empty() {
        return Err(CrateError::Discovery(format!(
          "Stream download returned {status}"
        )));
      }
      // CDN rejected subsequent chunk — cache what we have.
      log::info!(
        "CDN rejected chunk at offset {offset} ({status}), caching {} bytes of partial audio",
        data.len()
      );
      break;
    }

    // Extract metadata from the first chunk.
    if offset == 0 {
      total_size = parse_total_from_content_range(response.headers());
      if let Some(ct) = response
        .headers()
        .get("Content-Type")
        .and_then(|v| v.to_str().ok())
      {
        content_type = ct.to_string();
      }
      log::info!("Stream download started: total_size={total_size:?}, content-type={content_type}");
    }

    let chunk_bytes = response
      .bytes()
      .await
      .map_err(|e| CrateError::Discovery(format!("Stream chunk body failed: {e:#}")))?;

    if chunk_bytes.is_empty() {
      break;
    }

    data.extend_from_slice(&chunk_bytes);
  }

  if data.is_empty() {
    return Err(CrateError::Discovery(
      "Stream download produced no data".into(),
    ));
  }

  let full = total_size.is_none_or(|total| data.len() as u64 >= total);
  log::info!(
    "Stream download {}: {} bytes cached, content-type: {content_type}",
    if full { "complete" } else { "partial" },
    data.len()
  );

  Ok(CachedAudio { data, content_type })
}

/// Common CORS headers applied to every proxy response.
fn apply_cors_headers(builder: axum::http::response::Builder) -> axum::http::response::Builder {
  builder
    .header("Access-Control-Allow-Origin", "*")
    .header(
      "Access-Control-Expose-Headers",
      "Content-Range, Content-Length, Accept-Ranges",
    )
    .header("Access-Control-Allow-Headers", "Range")
}

/// Handle OPTIONS preflight requests for the proxy endpoint.
pub(crate) async fn proxy_cors_preflight_handler() -> axum::http::Response<axum::body::Body> {
  apply_cors_headers(axum::http::Response::builder().status(204))
    .header("Access-Control-Allow-Methods", "GET, OPTIONS")
    .body(axum::body::Body::empty())
    .unwrap()
}

/// Serve cached audio data. Returns `200 OK` with the full body when there is no Range header,
/// or `206 Partial Content` with the requested byte slice when there is one. Returning `206`
/// unconditionally (even without a Range request) caused WKWebView's AVFoundation to
/// miscalculate the duration as ~2x the real value.
fn serve_range_from_cache(
  cached: &CachedAudio,
  start: u64,
  end_opt: Option<u64>,
  is_range_request: bool,
) -> Result<axum::http::Response<axum::body::Body>> {
  let total = cached.data.len() as u64;

  if is_range_request && start >= total {
    return Ok(
      apply_cors_headers(axum::http::Response::builder().status(416))
        .header("Content-Range", format!("bytes */{total}"))
        .body(axum::body::Body::empty())
        .unwrap(),
    );
  }

  if is_range_request {
    let end = end_opt.map_or(total - 1, |e| e.min(total - 1));
    let slice = &cached.data[start as usize..=end as usize];

    Ok(
      apply_cors_headers(axum::http::Response::builder().status(206))
        .header("Content-Type", &cached.content_type)
        .header("Content-Range", format!("bytes {start}-{end}/{total}"))
        .header("Content-Length", slice.len().to_string())
        .header("Accept-Ranges", "bytes")
        .body(axum::body::Body::from(slice.to_vec()))
        .unwrap(),
    )
  } else {
    Ok(
      apply_cors_headers(axum::http::Response::builder().status(200))
        .header("Content-Type", &cached.content_type)
        .header("Content-Length", total.to_string())
        .header("Accept-Ranges", "bytes")
        .body(axum::body::Body::from(cached.data.clone()))
        .unwrap(),
    )
  }
}

/// Top-level axum handler for `GET /samples/{key}`.
///
/// Beatport recommendation samples are remote `https://geo-samples.beatport.com` mp3s. The
/// WebKitGTK webview's own https fetch (libsoup/souphttpsrc) hangs for those URLs on some
/// systems, so the modal routes them through this proxy instead: reqwest fetches server-side,
/// the bytes are memory-cached, and Range serving matches the discovery preview behavior.
pub(crate) async fn sample_http_handler(
  axum::extract::Path(key): axum::extract::Path<String>,
  req_headers: axum::http::HeaderMap,
  axum::extract::State(state): axum::extract::State<ProxyServerState>,
) -> axum::http::Response<axum::body::Body> {
  match sample_http_handler_inner(&key, &req_headers, &state).await {
    Ok(r) => r,
    Err(e) => {
      log::error!("Sample proxy error: {e}");
      axum::http::Response::builder()
        .status(502)
        .header("Content-Type", "text/plain")
        .body(axum::body::Body::from("Internal proxy error"))
        .unwrap()
    }
  }
}

async fn sample_http_handler_inner(
  key: &str,
  req_headers: &axum::http::HeaderMap,
  state: &ProxyServerState,
) -> Result<axum::http::Response<axum::body::Body>> {
  // Resolve the registered URL first — an unknown key is a 404, not a proxy attempt.
  let remote_url = {
    let samples = state.samples.lock().map_err(|_| CrateError::LockPoisoned)?;
    samples.get(key).cloned()
  };
  let Some(remote_url) = remote_url else {
    return Ok(
      apply_cors_headers(axum::http::Response::builder().status(404))
        .body(axum::body::Body::from("Unknown sample key"))
        .unwrap(),
    );
  };

  let cache_key = format!("sample:{key}");
  let incoming_range = req_headers.get("Range").and_then(|v| v.to_str().ok());
  let has_range_header = incoming_range.is_some();
  let (start, end_opt) = parse_bytes_range(incoming_range);

  // 1. Serve from memory cache when the full sample is already downloaded.
  {
    let cache = state.cache.read().await;
    if let Some(cached) = cache.get(&cache_key) {
      return serve_range_from_cache(cached, start, end_opt, has_range_header);
    }
  }

  // 2. Dedupe concurrent requests on one download task (same watch-channel pattern as discovery).
  let mut rx = {
    let mut downloads = state.downloads.lock().await;
    if let Some(existing_rx) = downloads.get(&cache_key) {
      existing_rx.clone()
    } else {
      let (tx, rx) = tokio::sync::watch::channel(None);
      downloads.insert(cache_key.clone(), rx.clone());

      let state_clone = state.clone();
      let key = cache_key.clone();

      tokio::spawn(async move {
        let success = match download_sample(&state_clone, &remote_url).await {
          Ok(cached_audio) => {
            let entry = Arc::new(cached_audio);
            let mut cache = state_clone.cache.write().await;
            while cache.len() >= MAX_CACHE_ENTRIES {
              if let Some(oldest_key) = cache.keys().next().cloned() {
                cache.remove(&oldest_key);
              }
            }
            cache.insert(key.clone(), entry);
            true
          }
          Err(e) => {
            log::warn!("Sample download failed for {key}: {e}");
            false
          }
        };

        let _ = tx.send(Some(success));
        state_clone.downloads.lock().await.remove(&key);
      });

      rx
    }
  };

  // 3. Wait for the download, then serve exactly like discovery: 200 without a Range header,
  //    206 with one (unconditional 206 doubles AVFoundation's duration estimate).
  if rx.changed().await.is_err() {
    return Err(CrateError::Discovery(
      "Sample download task dropped unexpectedly".into(),
    ));
  }

  if *rx.borrow() != Some(true) {
    return Ok(
      axum::http::Response::builder()
        .status(502)
        .header("Content-Type", "text/plain")
        .body(axum::body::Body::from("Sample download failed"))
        .unwrap(),
    );
  }

  let cache = state.cache.read().await;
  let cached = cache.get(&cache_key).ok_or_else(|| {
    CrateError::Discovery("Sample download succeeded but cache entry missing".into())
  })?;
  serve_range_from_cache(cached, start, end_opt, has_range_header)
}

/// Download a Beatport sample fully into memory.
///
/// A plain single GET is enough: `geo-samples.beatport.com` serves mp3s without the signed-URL /
/// user-agent games that discovery's chunked `download_stream` exists for. The `MAX_TOTAL_SIZE`
/// cap rejects anything unreasonably large (samples are ~1–2 MB) before it reaches the cache.
async fn download_sample(state: &ProxyServerState, url: &str) -> Result<CachedAudio> {
  let response = state
    .client
    .get(url)
    .send()
    .await
    .map_err(|e| CrateError::Discovery(format!("Sample request failed: {e:#}")))?;

  let status = response.status();
  if !status.is_success() {
    return Err(CrateError::Discovery(format!(
      "Sample download returned {status}"
    )));
  }

  let content_type = response
    .headers()
    .get("Content-Type")
    .and_then(|v| v.to_str().ok())
    .unwrap_or("audio/mpeg")
    .to_string();

  let data = response
    .bytes()
    .await
    .map_err(|e| CrateError::Discovery(format!("Sample body read failed: {e:#}")))?
    .to_vec();

  if data.is_empty() {
    return Err(CrateError::Discovery(
      "Sample download produced no data".into(),
    ));
  }
  if data.len() as u64 > MAX_TOTAL_SIZE {
    return Err(CrateError::Discovery(format!(
      "Sample of {} bytes exceeds the {MAX_TOTAL_SIZE} byte cache limit",
      data.len()
    )));
  }

  log::info!(
    "Sample cached: {} bytes, content-type: {content_type}",
    data.len()
  );
  Ok(CachedAudio { data, content_type })
}

#[cfg(test)]
mod tests {
  use super::*;

  const SAMPLE_URL: &str = "https://geo-samples.beatport.com/media/9208584/LateNight.mp3";

  #[test]
  fn validate_sample_url_accepts_https_geo_samples() {
    assert!(validate_sample_url(SAMPLE_URL).is_ok());
  }

  #[test]
  fn validate_sample_url_rejects_everything_else() {
    for bad in [
      // Wrong scheme, even on the right host.
      "http://geo-samples.beatport.com/media/1.mp3",
      // Right scheme, wrong host.
      "https://www.beatport.com/track/name/123",
      // Suffix trick must not pass exact-host matching.
      "https://geo-samples.beatport.com.evil.net/1.mp3",
      "https://evil.net/?u=https://geo-samples.beatport.com/1.mp3",
      // SSRF-shaped localhost URLs.
      "https://localhost:8080/x.mp3",
      "http://127.0.0.1:1234/samples/abc",
      // Garbage.
      "not a url",
      "",
    ] {
      assert!(
        validate_sample_url(bad).is_err(),
        "expected rejection for {bad:?}"
      );
    }
  }

  #[test]
  fn sample_stream_key_is_deterministic_and_unique_per_url() {
    let a = sample_stream_key(&reqwest::Url::parse(SAMPLE_URL).unwrap());
    let b = sample_stream_key(&reqwest::Url::parse(SAMPLE_URL).unwrap());
    let c = sample_stream_key(
      &reqwest::Url::parse("https://geo-samples.beatport.com/media/1/Other.mp3").unwrap(),
    );
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert!(a.len() == 16 && a.chars().all(|ch| ch.is_ascii_hexdigit()));
  }

  #[test]
  fn register_sample_returns_proxy_url_for_valid_and_none_for_invalid() {
    let samples = Mutex::new(HashMap::new());
    let key = sample_stream_key(&reqwest::Url::parse(SAMPLE_URL).unwrap());

    let proxy_url = register_sample(&samples, 4321, SAMPLE_URL).unwrap();
    assert_eq!(proxy_url, format!("http://127.0.0.1:4321/samples/{key}"));
    assert_eq!(
      samples.lock().unwrap().get(&key).map(String::as_str),
      Some(SAMPLE_URL)
    );

    assert_eq!(
      register_sample(&samples, 4321, SAMPLE_URL).unwrap(),
      proxy_url
    );
    assert!(register_sample(&samples, 4321, "https://evil.example.com/a.mp3").is_none());
    assert_eq!(samples.lock().unwrap().len(), 1);
  }
}
