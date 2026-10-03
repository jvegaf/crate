use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;

use crate::error::{CrateError, Result};
use crate::models::{TagCandidate, TagSearchQuery};

use super::TaggerProvider;

static TRACK_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"data-trid="(\d+)""#).expect("valid trid regex"));
static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<a href="(/track/\d+/[^"]+)"[^>]*>([^<]+)</a>"#).expect("valid title regex")
});
static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<span class="version">\s*([^<]+?)\s*<span class="duration">\((\d+):(\d+)\)"#)
        .expect("valid version regex")
});
static DURATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<span class="duration">\((\d+):(\d+)\)</span>"#).expect("valid duration regex")
});
static ARTISTS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"class="com-artists"[^>]*>([^<]+)</a>"#).expect("valid artists regex")
});
static LABEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div class="trk-cell label">\s*<a[^>]*>([^<]+)</a>"#).expect("valid label regex")
});
static KEY_BPM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div class="trk-cell key-bpm">\s*([^<\s]*)\s*<br\s*/?>\s*(\d+)"#)
        .expect("valid key/bpm regex")
});
static GENRE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div class="trk-cell genre">\s*<a[^>]*>([^<]+)</a>"#).expect("valid genre regex")
});
static RDATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div class="trk-cell r-date">\s*([\d-]+)"#).expect("valid r-date regex")
});
static ARTWORK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<div class="trk-cell thumb">\s*<img src="([^"]+)""#).expect("valid artwork regex")
});

/// TraxSource search provider.
///
/// Cloudflare blocks `reqwest`'s TLS/JA3 fingerprint, so this provider shells
/// out to the system `curl` binary, whose fingerprint Cloudflare accepts. It is
/// compiled only for desktop builds because mobile has no `curl`. Doing it via
/// `curl` added no new Rust dependency.
pub(super) struct TraxSourceProvider;

#[async_trait]
impl TaggerProvider for TraxSourceProvider {
    fn id(&self) -> &'static str {
        "traxsource"
    }

    async fn search(
        &self,
        _client: &reqwest::Client,
        query: &TagSearchQuery,
        limit: usize,
    ) -> Result<Vec<TagCandidate>> {
        let html = fetch_search_html(&query.term()).await?;

        let mut candidates = parse_traxsource_rows(&html);
        candidates.truncate(limit);
        Ok(candidates)
    }
}

/// Fetch the TraxSource search HTML through the system `curl` binary.
///
/// `reqwest` is TLS/JA3-fingerprint blocked by Cloudflare (extra headers and
/// HTTP/1.1 do not help); the platform `curl` passes. The homepage must be
/// fetched first into a cookie jar, otherwise the search endpoint returns `403`
/// because the Cloudflare session cookie is missing. Mobile has no `curl`, which
/// is why the provider is desktop-only.
async fn fetch_search_html(term: &str) -> Result<String> {
    let jar =
        std::env::temp_dir().join(format!("crate-traxsource-{}.cookies", uuid::Uuid::new_v4()));

    // Warm-up: capture the Cloudflare session cookie into the jar. The body is
    // discarded; only the exit status matters.
    let warm_up = tokio::process::Command::new("curl")
        .args(["-sS", "-f", "-A", super::http::USER_AGENT, "-c"])
        .arg(&jar)
        .arg("https://www.traxsource.com/")
        .output()
        .await
        .map_err(|e| CrateError::Tagger(format!("TraxSource requires the `curl` binary: {e}")))?;

    if !warm_up.status.success() {
        remove_cookie_jar(&jar);
        return Err(CrateError::Tagger(format!(
            "TraxSource warm-up failed (curl exit {:?}): {}",
            warm_up.status.code(),
            String::from_utf8_lossy(&warm_up.stderr).trim()
        )));
    }

    let response = tokio::process::Command::new("curl")
        .args(["-sS", "-f", "-A", super::http::USER_AGENT, "-b"])
        .arg(&jar)
        .args(["-G", "--data-urlencode"])
        .arg(format!("term={term}"))
        .arg("https://www.traxsource.com/search/tracks")
        .output()
        .await;

    remove_cookie_jar(&jar);

    let response = response
        .map_err(|e| CrateError::Tagger(format!("TraxSource requires the `curl` binary: {e}")))?;

    if !response.status.success() {
        return Err(CrateError::Tagger(format!(
            "TraxSource search failed (curl exit {:?}): {}",
            response.status.code(),
            String::from_utf8_lossy(&response.stderr).trim()
        )));
    }

    String::from_utf8(response.stdout)
        .map_err(|e| CrateError::Tagger(format!("TraxSource response was not valid UTF-8: {e}")))
}

/// Best-effort removal of the temporary cookie jar; failures are ignored.
fn remove_cookie_jar(jar: &std::path::Path) {
    let _ = std::fs::remove_file(jar);
}

/// Split the search HTML into `trk-row` blocks and parse each one.
pub(super) fn parse_traxsource_rows(html: &str) -> Vec<TagCandidate> {
    let mut candidates = Vec::new();
    let mut search = 0usize;

    while let Some(rel) = html[search..].find("trk-row") {
        let marker = search + rel;
        let Some(div_rel) = html[..marker].rfind("<div") else {
            search = marker + "trk-row".len();
            continue;
        };

        // The row ends where the next `trk-row` block's opening `<div` begins.
        let end = html[marker + 1..]
            .find("trk-row")
            .map(|next_rel| {
                let next_marker = marker + 1 + next_rel;
                html[..next_marker].rfind("<div").unwrap_or(next_marker)
            })
            .unwrap_or(html.len());

        if end > div_rel {
            if let Some(candidate) = parse_traxsource_row(&html[div_rel..end]) {
                candidates.push(candidate);
            }
        }
        search = end;
    }

    candidates
}

fn parse_traxsource_row(row: &str) -> Option<TagCandidate> {
    let captures = TITLE_RE.captures(row)?;
    let href = captures.get(1)?.as_str().to_string();
    let title = decode_entities(captures.get(2)?.as_str().trim());
    if title.is_empty() {
        return None;
    }

    let url = format!("https://www.traxsource.com{href}");
    let provider_track_id = TRACK_ID_RE
        .captures(row)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let version = VERSION_RE
        .captures(row)
        .and_then(|c| c.get(1))
        .map(|m| decode_entities(m.as_str().trim()))
        .filter(|s| !s.is_empty());

    let duration_ms = match VERSION_RE.captures(row) {
        Some(c) => parse_mmss(c.get(2).map(|m| m.as_str()), c.get(3).map(|m| m.as_str())),
        None => DURATION_RE
            .captures(row)
            .and_then(|c| parse_mmss(c.get(1).map(|m| m.as_str()), c.get(2).map(|m| m.as_str()))),
    };

    let artists: Vec<String> = ARTISTS_RE
        .captures_iter(row)
        .filter_map(|c| c.get(1))
        .map(|m| decode_entities(m.as_str().trim()))
        .filter(|s| !s.is_empty())
        .collect();

    let label = first_cell_text(row, &LABEL_RE);
    let genre = first_cell_text(row, &GENRE_RE);

    let (key, bpm) = match KEY_BPM_RE.captures(row) {
        Some(c) => (
            normalize_trax_key(c.get(1).map(|m| m.as_str()).unwrap_or_default()),
            c.get(2).and_then(|m| m.as_str().parse::<f64>().ok()),
        ),
        None => (None, None),
    };

    let release_date = RDATE_RE
        .captures(row)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let artwork_url = ARTWORK_RE
        .captures(row)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    Some(TagCandidate {
        provider: "traxsource".to_string(),
        title,
        version,
        artists,
        album: None,
        label,
        catalog_number: None,
        genre,
        release_date,
        bpm,
        key,
        duration_ms,
        isrc: None,
        track_number: None,
        artwork_url,
        url,
        provider_track_id,
        provider_release_id: None,
    })
}

/// Return the first capture group of `re`, trimmed and entity-decoded.
fn first_cell_text(row: &str, re: &Regex) -> Option<String> {
    let text = decode_entities(re.captures(row)?.get(1)?.as_str().trim());
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Parse `mm:ss` into milliseconds.
fn parse_mmss(minutes: Option<&str>, seconds: Option<&str>) -> Option<i64> {
    let minutes: i64 = minutes?.parse().ok()?;
    let seconds: i64 = seconds?.parse().ok()?;
    Some(minutes * 60_000 + seconds * 1_000)
}

/// Normalize TraxSource keys to Crate's notation (`"Gmaj"` -> `"G"`, `"Gmin"` -> `"Gm"`).
fn normalize_trax_key(raw: &str) -> Option<String> {
    let normalized = raw.replace("maj", "").replace("min", "m");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Decode the handful of HTML entities that appear in provider text fields.
fn decode_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}
