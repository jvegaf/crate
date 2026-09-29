use super::bandcamp::parse_bandcamp_autocomplete;
use super::beatport::parse_beatport_search;
#[cfg(feature = "desktop")]
use super::traxsource::parse_traxsource_rows;
use super::TaggerService;
use crate::models::TagSearchQuery;

#[test]
fn term_joins_artist_and_title() {
  let query = TagSearchQuery {
    artist: Some("Adam Beyer".to_string()),
    title: "Your Mind".to_string(),
  };
  assert_eq!(query.term(), "Adam Beyer Your Mind");
}

#[test]
fn term_falls_back_to_title_without_artist() {
  let no_artist = TagSearchQuery {
    artist: None,
    title: "  Your Mind  ".to_string(),
  };
  assert_eq!(no_artist.term(), "Your Mind");

  let blank_artist = TagSearchQuery {
    artist: Some("   ".to_string()),
    title: "Your Mind".to_string(),
  };
  assert_eq!(blank_artist.term(), "Your Mind");
}

const BEATPORT_API_BODY: &str = r#"{"tracks":[{"id":123456,"name":"Your Mind","mix_name":"Original Mix","slug":"your-mind","bpm":128,"length_ms":405000,"isrc":"GBABC1234567","catalog_number":"DC123","publish_date":"2019-05-03","new_release_date":"2019-05-03","key":{"name":"G Minor"},"genre":{"name":"Techno"},"release":{"id":999,"name":"Your Mind EP","image":{"uri":"https://images.beatport.com/your-mind.jpg"},"label":{"name":"Drumcode"}},"artists":[{"name":"Adam Beyer"}]},{"id":654321,"name":"Daybreak","mix_name":"","slug":"daybreak","artists":[{"name":"Adam Beyer"}]}],"count":2,"page":1,"per_page":5}"#;

#[test]
fn beatport_parses_api_search() {
  let candidates = parse_beatport_search(BEATPORT_API_BODY);
  assert_eq!(candidates.len(), 2);

  let first = &candidates[0];
  assert_eq!(first.provider, "beatport");
  assert_eq!(first.title, "Your Mind");
  assert_eq!(first.artists, vec!["Adam Beyer".to_string()]);
  assert_eq!(first.key.as_deref(), Some("Gm"));
  assert_eq!(first.duration_ms, Some(405_000));
  assert_eq!(
    first.artwork_url.as_deref(),
    Some("https://images.beatport.com/your-mind.jpg")
  );
  assert_eq!(first.url, "https://www.beatport.com/track/your-mind/123456");
  assert_eq!(first.genre.as_deref(), Some("Techno"));
  assert_eq!(first.label.as_deref(), Some("Drumcode"));
  assert_eq!(first.version.as_deref(), Some("Original Mix"));
  assert_eq!(first.bpm, Some(128.0));
  assert_eq!(first.catalog_number.as_deref(), Some("DC123"));
  assert_eq!(first.isrc.as_deref(), Some("GBABC1234567"));
  assert_eq!(first.release_date.as_deref(), Some("2019-05-03"));
  assert_eq!(first.album.as_deref(), Some("Your Mind EP"));
  assert_eq!(first.track_number, None);
  assert_eq!(first.provider_track_id.as_deref(), Some("123456"));
  assert_eq!(first.provider_release_id.as_deref(), Some("999"));

  // An empty `mix_name` must not produce a version.
  let second = &candidates[1];
  assert_eq!(second.version, None);
}

#[test]
fn beatport_empty_when_payload_malformed() {
  assert!(parse_beatport_search("{}").is_empty());
  assert!(parse_beatport_search("not json").is_empty());
  assert!(parse_beatport_search(r#"{"tracks":[]}"#).is_empty());
}

#[cfg(feature = "desktop")]
const TRAXSOURCE_HTML: &str = r#"<div id="searchTrackList">
  <div class="trk-row odd" data-trid="12345">
    <div class="trk-cell title"><a href="/track/12345/your-mind">Your Mind</a><span class="version">Extended Mix <span class="duration">(6:45)</span></span></div>
    <div class="trk-cell artists"><a class="com-artists" href="/artist/adam-beyer">Adam Beyer</a></div>
    <div class="trk-cell label"><a href="/label/drumcode">Drumcode</a></div>
    <div class="trk-cell key-bpm">Gmaj<br>138</div>
    <div class="trk-cell genre"><a href="/genre/techno">Techno</a></div>
    <div class="trk-cell r-date">2019-05-03</div>
    <div class="trk-cell thumb"><img src="https://images.traxsource.com/your-mind.jpg"></div>
  </div>
</div>"#;

#[cfg(feature = "desktop")]
#[test]
fn traxsource_parses_row() {
  let candidates = parse_traxsource_rows(TRAXSOURCE_HTML);
  assert_eq!(candidates.len(), 1);

  let first = &candidates[0];
  assert_eq!(first.provider, "traxsource");
  assert_eq!(first.title, "Your Mind");
  assert_eq!(first.artists, vec!["Adam Beyer".to_string()]);
  assert_eq!(first.label.as_deref(), Some("Drumcode"));
  assert_eq!(first.key.as_deref(), Some("G"));
  assert_eq!(first.bpm, Some(138.0));
  assert_eq!(first.genre.as_deref(), Some("Techno"));
  assert_eq!(first.release_date.as_deref(), Some("2019-05-03"));
  assert_eq!(
    first.artwork_url.as_deref(),
    Some("https://images.traxsource.com/your-mind.jpg")
  );
  assert_eq!(first.duration_ms, Some(405_000));
  assert_eq!(first.version.as_deref(), Some("Extended Mix"));
  assert_eq!(
    first.url,
    "https://www.traxsource.com/track/12345/your-mind"
  );
  assert_eq!(first.provider_track_id.as_deref(), Some("12345"));
}

#[test]
fn bandcamp_parses_autocomplete() {
  let body = r#"{"auto":{"results":[{"id":123,"name":"Your Mind","band_name":"Adam Beyer","album_name":"Your Mind EP","album_id":456,"item_url_path":"https://adambeyer.bandcamp.com/track/your-mind","img":"https://f4.bcbits.com/img/a123_16.jpg"}]}}"#;
  let candidates = parse_bandcamp_autocomplete(body).unwrap();
  assert_eq!(candidates.len(), 1);

  let first = &candidates[0];
  assert_eq!(first.provider, "bandcamp");
  assert_eq!(first.title, "Your Mind");
  assert_eq!(first.artists, vec!["Adam Beyer".to_string()]);
  assert_eq!(first.album.as_deref(), Some("Your Mind EP"));
  assert_eq!(first.url, "https://adambeyer.bandcamp.com/track/your-mind");
  assert_eq!(first.provider_track_id.as_deref(), Some("123"));
  assert_eq!(first.provider_release_id.as_deref(), Some("456"));
  assert_eq!(
    first.artwork_url.as_deref(),
    Some("https://f4.bcbits.com/img/a123_16.jpg")
  );
}

fn live_query() -> TagSearchQuery {
  TagSearchQuery {
    artist: Some("Adam Beyer".to_string()),
    title: "Your Mind".to_string(),
  }
}

#[ignore = "live network"]
#[tokio::test]
async fn live_beatport_search() {
  let service = TaggerService::new().unwrap();
  let results = service.search_all(&live_query(), 5).await.unwrap();
  let outcome = results
    .iter()
    .find(|r| r.provider == "beatport")
    .expect("beatport result present");
  assert!(
    outcome.error.is_none(),
    "beatport error: {:?}",
    outcome.error
  );
  assert!(
    !outcome.candidates.is_empty(),
    "beatport returned no candidates"
  );
  let first = &outcome.candidates[0];
  assert!(!first.title.is_empty());
  assert!(!first.artists.is_empty());
  println!("beatport candidates: {:#?}", outcome.candidates);
}

#[ignore = "live network"]
#[tokio::test]
async fn live_traxsource_search() {
  let service = TaggerService::new().unwrap();
  let results = service.search_all(&live_query(), 5).await.unwrap();
  let outcome = results
    .iter()
    .find(|r| r.provider == "traxsource")
    .expect("traxsource result present");
  assert!(
    outcome.error.is_none(),
    "traxsource error: {:?}",
    outcome.error
  );
  assert!(
    !outcome.candidates.is_empty(),
    "traxsource returned no candidates"
  );
  let first = &outcome.candidates[0];
  assert!(!first.title.is_empty());
  assert!(!first.artists.is_empty());
  println!("traxsource candidates: {:#?}", outcome.candidates);
}

#[ignore = "live network"]
#[tokio::test]
async fn live_bandcamp_search() {
  let service = TaggerService::new().unwrap();
  let results = service.search_all(&live_query(), 5).await.unwrap();
  let outcome = results
    .iter()
    .find(|r| r.provider == "bandcamp")
    .expect("bandcamp result present");
  assert!(
    outcome.error.is_none(),
    "bandcamp error: {:?}",
    outcome.error
  );
  assert!(
    !outcome.candidates.is_empty(),
    "bandcamp returned no candidates"
  );
  let first = &outcome.candidates[0];
  assert!(!first.title.is_empty());
  assert!(!first.artists.is_empty());
  println!("bandcamp candidates: {:#?}", outcome.candidates);
}

#[ignore = "live network"]
#[tokio::test]
async fn live_search_all() {
  let service = TaggerService::new().unwrap();
  let results = service.search_all(&live_query(), 5).await.unwrap();
  assert_eq!(results.len(), 3);
  for result in &results {
    assert!(
      result.error.is_none(),
      "{} error: {:?}",
      result.provider,
      result.error
    );
  }
}
