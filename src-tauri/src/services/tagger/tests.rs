use super::bandcamp::{
  enrich_bandcamp_candidate, parse_bandcamp_autocomplete, parse_iso8601_duration,
};
use super::beatport::{enrich_beatport_candidate, parse_beatport_search};
use super::scoring::{
  duration_score, hybrid_text_similarity, levenshtein_similarity, normalize_string,
  rank_candidates, ScoringWeights, UnifiedScorer, DEFAULT_WEIGHTS,
};
#[cfg(feature = "desktop")]
use super::traxsource::parse_traxsource_rows;
use super::{artwork_extension, rank_results, TaggerService};
use crate::models::{
  ProviderSearchResult, RankedSearchResult, ScoredTagCandidate, TagCandidate, TagSearchQuery,
};

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

#[test]
fn iso8601_duration_to_milliseconds() {
  assert_eq!(parse_iso8601_duration("PT5M41S"), Some(341_000));
  assert_eq!(parse_iso8601_duration("PT1H2M3S"), Some(3_723_000));
  assert_eq!(parse_iso8601_duration("PT45S"), Some(45_000));
  assert_eq!(parse_iso8601_duration("P1DT2H"), Some(93_600_000));
  assert_eq!(parse_iso8601_duration("not-a-duration"), None);
}

const BANDCAMP_TRACK_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<script type="application/ld+json">
{"@context":"https://schema.org","@type":"MusicRecording","name":"Your Mind","byArtist":{"@type":"MusicGroup","name":"Adam Beyer"},"inAlbum":{"@type":"MusicAlbum","name":"Your Mind EP"},"datePublished":"2019-05-03","duration":"PT5M41S","image":"https://f4.bcbits.com/img/a123_16.jpg","url":"https://adambeyer.bandcamp.com/track/your-mind"}
</script>
</head>
<body>
<div id="trackInfo" data-tralbum="{&quot;album_release_date&quot;:&quot;2019-05-03&quot;,&quot;genre&quot;:&quot;Techno&quot;,&quot;label&quot;:&quot;Drumcode&quot;}">
</div>
</body>
</html>"#;

#[test]
fn bandcamp_extend_fills_detail_and_preserves_identity() {
  let candidate = TagCandidate {
    provider: "bandcamp".to_string(),
    title: "Your Mind".to_string(),
    artists: vec!["Adam Beyer".to_string()],
    album: Some("Your Mind EP".to_string()),
    url: "https://adambeyer.bandcamp.com/track/your-mind".to_string(),
    provider_track_id: Some("123".to_string()),
    provider_release_id: Some("456".to_string()),
    ..Default::default()
  };

  let enriched = enrich_bandcamp_candidate(&candidate, BANDCAMP_TRACK_HTML);

  // Identity and URL always come from the input candidate.
  assert_eq!(enriched.provider, "bandcamp");
  assert_eq!(enriched.provider_track_id.as_deref(), Some("123"));
  assert_eq!(enriched.provider_release_id.as_deref(), Some("456"));
  assert_eq!(
    enriched.url,
    "https://adambeyer.bandcamp.com/track/your-mind"
  );

  // The search payload left these empty; the page detail fills them.
  assert_eq!(enriched.label.as_deref(), Some("Drumcode"));
  assert_eq!(enriched.genre.as_deref(), Some("Techno"));
  assert_eq!(enriched.release_date.as_deref(), Some("2019-05-03"));
  assert_eq!(enriched.duration_ms, Some(341_000));
  assert_eq!(
    enriched.artwork_url.as_deref(),
    Some("https://f4.bcbits.com/img/a123_16.jpg")
  );
  assert_eq!(enriched.album.as_deref(), Some("Your Mind EP"));
  assert_eq!(enriched.artists, vec!["Adam Beyer".to_string()]);

  // A field the page does not expose stays absent rather than guessed.
  assert_eq!(enriched.bpm, None);
  assert_eq!(enriched.key, None);
}

const BEATPORT_TRACK_DETAIL_BODY: &str = r#"{"id":123456,"name":"Your Mind","mix_name":"Original Mix","slug":"your-mind","bpm":128,"length_ms":405000,"isrc":"GBABC1234567","catalog_number":"DC123","new_release_date":"2019-05-03","key":{"name":"G Minor"},"genre":{"name":"Techno"},"release":{"id":999,"name":"Your Mind EP","image":{"uri":"https://images.beatport.com/your-mind.jpg"},"label":{"name":"Drumcode"}},"artists":[{"name":"Adam Beyer"}]}"#;

#[test]
fn beatport_extend_fills_detail_and_preserves_identity() {
  let candidate = TagCandidate {
    provider: "beatport".to_string(),
    title: "Your Mind".to_string(),
    artists: vec!["Adam Beyer".to_string()],
    url: "https://www.beatport.com/track/your-mind/123456".to_string(),
    provider_track_id: Some("123456".to_string()),
    provider_release_id: Some("999".to_string()),
    // Not present in the detail payload: the merge must not clear it.
    track_number: Some(5),
    ..Default::default()
  };

  let enriched = enrich_beatport_candidate(&candidate, BEATPORT_TRACK_DETAIL_BODY);

  // Identity and URL always come from the input candidate.
  assert_eq!(enriched.provider, "beatport");
  assert_eq!(enriched.provider_track_id.as_deref(), Some("123456"));
  assert_eq!(enriched.provider_release_id.as_deref(), Some("999"));
  assert_eq!(
    enriched.url,
    "https://www.beatport.com/track/your-mind/123456"
  );

  // The detail payload fills the search gaps.
  assert_eq!(enriched.title, "Your Mind");
  assert_eq!(enriched.version.as_deref(), Some("Original Mix"));
  assert_eq!(enriched.album.as_deref(), Some("Your Mind EP"));
  assert_eq!(enriched.label.as_deref(), Some("Drumcode"));
  assert_eq!(enriched.genre.as_deref(), Some("Techno"));
  assert_eq!(enriched.release_date.as_deref(), Some("2019-05-03"));
  assert_eq!(enriched.duration_ms, Some(405_000));
  assert_eq!(enriched.bpm, Some(128.0));
  assert_eq!(enriched.key.as_deref(), Some("Gm"));
  assert_eq!(enriched.isrc.as_deref(), Some("GBABC1234567"));
  assert_eq!(enriched.catalog_number.as_deref(), Some("DC123"));
  assert_eq!(
    enriched.artwork_url.as_deref(),
    Some("https://images.beatport.com/your-mind.jpg")
  );

  // A field only the search candidate had is preserved.
  assert_eq!(enriched.track_number, Some(5));
}

#[tokio::test]
async fn extend_candidate_unknown_provider_fails_before_network() {
  let service = TaggerService::new().unwrap();
  let candidate = TagCandidate {
    provider: "nope".to_string(),
    ..Default::default()
  };

  let result = service.extend_candidate(&candidate).await;

  assert!(result.is_err());
  let message = result.unwrap_err().to_string();
  assert!(message.contains("nope"), "unexpected message: {message}");
}

#[cfg(feature = "desktop")]
#[tokio::test]
async fn extend_candidate_traxsource_returns_the_candidate_unchanged() {
  let service = TaggerService::new().unwrap();
  let candidate = TagCandidate {
    provider: "traxsource".to_string(),
    title: "Your Mind".to_string(),
    artists: vec!["Adam Beyer".to_string()],
    url: "https://www.traxsource.com/track/12345/your-mind".to_string(),
    provider_track_id: Some("12345".to_string()),
    ..Default::default()
  };

  let extended = service.extend_candidate(&candidate).await.unwrap();

  assert_eq!(extended.provider, "traxsource");
  assert_eq!(extended.title, "Your Mind");
  assert_eq!(extended.artists, vec!["Adam Beyer".to_string()]);
  assert_eq!(extended.url, candidate.url);
  assert_eq!(extended.provider_track_id.as_deref(), Some("12345"));
  assert_eq!(extended.label, None);
  assert_eq!(extended.duration_ms, None);
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

#[test]
fn normalize_string_strips_punctuation_and_collapses_whitespace() {
  assert_eq!(normalize_string("  Beyoncé's  Song!! "), "beyoncé s song");
  assert_eq!(
    normalize_string("Strobe (Original Mix)"),
    "strobe original mix"
  );
  assert_eq!(normalize_string("   "), "");
}

#[test]
fn normalize_string_keeps_non_latin_text() {
  assert_eq!(normalize_string("Мумий Тролль"), "мумий тролль");
  assert!(!normalize_string("Мумий Тролль").is_empty());
}

#[test]
fn levenshtein_similarity_identical_and_empty() {
  assert_eq!(levenshtein_similarity("strobe", "strobe"), 1.0);
  assert_eq!(levenshtein_similarity("", "strobe"), 0.0);
  assert_eq!(levenshtein_similarity("strobe", ""), 0.0);
}

#[test]
fn levenshtein_similarity_known_distance() {
  // "kitten" -> "sitting" is the classic distance-3 example over 7 characters.
  let expected = 1.0 - 3.0 / 7.0;
  assert!((levenshtein_similarity("kitten", "sitting") - expected).abs() < 1e-9);
}

#[test]
fn levenshtein_similarity_is_symmetric() {
  assert_eq!(
    levenshtein_similarity("deadmau5", "deadmouse"),
    levenshtein_similarity("deadmouse", "deadmau5")
  );
}

#[test]
fn hybrid_text_similarity_exact_and_reordered_are_one() {
  assert_eq!(hybrid_text_similarity("Strobe", "Strobe"), 1.0);
  assert_eq!(
    hybrid_text_similarity("strobe deadmau5", "deadmau5 strobe"),
    1.0
  );
}

#[test]
fn hybrid_text_similarity_ignores_extra_candidate_words() {
  assert_eq!(
    hybrid_text_similarity("strobe", "strobe (original mix)"),
    1.0
  );
}

#[test]
fn hybrid_text_similarity_typo_scores_high() {
  let similarity = hybrid_text_similarity("strobe", "stroba");
  assert!(
    similarity > 0.7 && similarity < 1.0,
    "unexpected {similarity}"
  );
}

#[test]
fn hybrid_text_similarity_empty_side_is_zero() {
  assert_eq!(hybrid_text_similarity("", "strobe"), 0.0);
  assert_eq!(hybrid_text_similarity("strobe", ""), 0.0);
}

#[test]
fn duration_score_is_neutral_when_unknown() {
  assert_eq!(duration_score(300_000, None), 0.7);
  assert_eq!(duration_score(300_000, Some(0)), 0.7);
  assert_eq!(duration_score(0, Some(300_000)), 0.7);
}

#[test]
fn duration_score_respects_thresholds() {
  assert_eq!(duration_score(300_000, Some(300_000)), 1.0);
  assert_eq!(duration_score(300_000, Some(305_000)), 1.0);
  assert_eq!(duration_score(300_000, Some(315_000)), 0.8);
  assert_eq!(duration_score(300_000, Some(330_000)), 0.5);
  assert_eq!(duration_score(300_000, Some(330_001)), 0.2);
}

#[test]
fn duration_score_is_total_for_extreme_durations() {
  // abs_diff makes the gap total; a plain subtraction would overflow here.
  assert_eq!(duration_score(i64::MIN, Some(i64::MAX)), 0.2);
  assert_eq!(duration_score(i64::MAX, Some(i64::MIN)), 0.2);
  assert_eq!(duration_score(i64::MIN, Some(i64::MIN + 1_000)), 1.0);
}

#[test]
fn unified_scorer_rejects_weights_that_do_not_sum_to_one() {
  let bad = ScoringWeights {
    title: 0.5,
    artist: 0.5,
    duration: 0.5,
  };
  assert!(UnifiedScorer::new(bad).is_err());
}

#[test]
fn unified_scorer_accepts_default_weights() {
  assert!(UnifiedScorer::new(DEFAULT_WEIGHTS).is_ok());
}

#[test]
fn unified_scorer_perfect_match_scores_one() {
  let scorer = UnifiedScorer::default_weights();
  let score = scorer.score(
    "Your Mind",
    "Adam Beyer",
    405_000,
    "Your Mind",
    "Adam Beyer",
    Some(405_000),
  );
  assert!((score - 1.0).abs() < 1e-9, "unexpected {score}");
}

#[test]
fn unified_scorer_wrong_candidate_scores_low() {
  let scorer = UnifiedScorer::default_weights();
  let score = scorer.score(
    "Your Mind",
    "Adam Beyer",
    405_000,
    "Completely Different",
    "Nobody",
    Some(120_000),
  );
  assert!(score < 0.5, "unexpected {score}");
}

fn scored(provider: &str, similarity_score: f64) -> ScoredTagCandidate {
  ScoredTagCandidate {
    candidate: TagCandidate {
      provider: provider.to_string(),
      title: format!("{provider} track"),
      ..Default::default()
    },
    similarity_score,
  }
}

fn candidate(
  provider: &str,
  title: &str,
  artists: &[&str],
  duration_ms: Option<i64>,
) -> TagCandidate {
  TagCandidate {
    provider: provider.to_string(),
    title: title.to_string(),
    artists: artists.iter().map(|artist| (*artist).to_string()).collect(),
    duration_ms,
    ..Default::default()
  }
}

#[test]
fn rank_candidates_filters_sorts_and_truncates() {
  let no_priority: Vec<String> = Vec::new();
  let candidates = vec![
    scored("beatport", 0.2),
    scored("beatport", 0.9),
    scored("beatport", 0.6),
    scored("beatport", 0.5),
  ];

  let ranked = rank_candidates(candidates, &no_priority, 0.3, 2);

  assert_eq!(ranked.len(), 2);
  assert!((ranked[0].similarity_score - 0.9).abs() < 1e-9);
  assert!((ranked[1].similarity_score - 0.6).abs() < 1e-9);
}

#[test]
fn rank_candidates_breaks_ties_by_provider_priority() {
  let priority = vec![
    "beatport".to_string(),
    "traxsource".to_string(),
    "bandcamp".to_string(),
  ];
  let candidates = vec![
    scored("bandcamp", 0.9),
    scored("beatport", 0.9),
    scored("traxsource", 0.9),
  ];

  let ranked = rank_candidates(candidates, &priority, 0.0, 10);
  let providers: Vec<&str> = ranked
    .iter()
    .map(|candidate| candidate.candidate.provider.as_str())
    .collect();

  assert_eq!(providers, vec!["beatport", "traxsource", "bandcamp"]);
}

#[test]
fn rank_candidates_orders_the_non_transitive_cycle_deterministically() {
  // Regression for the old `<= TIE_TOLERANCE` comparator, which was not
  // transitive: scores 0.99/"p0", 0.995/"p1", 1.0/"p2" gave
  //   0.99 < 0.995 (tolerance, priority), 0.995 < 1.0 (tolerance, priority),
  //   1.0 < 0.99  (1.0 - 0.99 = 0.010000000000000009 > 0.01, so score decides),
  // a cycle. The total-order sort plus priority pass fixes the ordering:
  // `1.0 - 0.995` and `0.995 - 0.99` are within tolerance, so 0.995/"p1" and
  // 1.0/"p2" form one tie group that priority orders as p1, p2; the
  // `1.0 - 0.99` float boundary exceeds the tolerance, so 0.99/"p0" starts the
  // next group and lands last. The float boundary is deliberate and documented.
  let priority = vec!["p0".to_string(), "p1".to_string(), "p2".to_string()];
  let candidates = vec![scored("p2", 1.0), scored("p1", 0.995), scored("p0", 0.99)];

  let ranked = rank_candidates(candidates, &priority, 0.0, 10);

  let providers: Vec<&str> = ranked
    .iter()
    .map(|candidate| candidate.candidate.provider.as_str())
    .collect();
  let scores: Vec<f64> = ranked
    .iter()
    .map(|candidate| candidate.similarity_score)
    .collect();
  assert_eq!(providers, vec!["p1", "p2", "p0"]);
  assert_eq!(scores, vec![0.995, 1.0, 0.99]);
}

#[test]
fn rank_candidates_is_deterministic_for_a_large_set_containing_the_cycle() {
  // 1_000 candidates cycling the three scores that used to produce a
  // non-transitive comparator order. The sort must not panic and must yield
  // identical provider sequences across runs.
  let priority = vec!["p0".to_string(), "p1".to_string(), "p2".to_string()];
  let build = || -> Vec<ScoredTagCandidate> {
    (0..1_000)
      .map(|index| match index % 3 {
        0 => scored("p2", 1.0),
        1 => scored("p1", 0.995),
        _ => scored("p0", 0.99),
      })
      .collect()
  };

  let first = rank_candidates(build(), &priority, 0.0, 1_000);
  let second = rank_candidates(build(), &priority, 0.0, 1_000);

  let providers = |ranked: &[ScoredTagCandidate]| -> Vec<String> {
    ranked
      .iter()
      .map(|candidate| candidate.candidate.provider.clone())
      .collect()
  };

  assert_eq!(first.len(), 1_000);
  assert_eq!(providers(&first), providers(&second));
}

#[test]
fn ranked_result_keeps_at_most_default_five_candidates_in_order() {
  const DEFAULT_MAX_CANDIDATES: usize = 5;
  let priority = vec!["beatport".to_string(), "bandcamp".to_string()];
  let candidates: Vec<ScoredTagCandidate> = (0..8)
    .map(|index| scored("beatport", 0.3 + f64::from(index) * 0.05))
    .collect();

  let ranked = RankedSearchResult {
    candidates: rank_candidates(candidates, &priority, 0.3, DEFAULT_MAX_CANDIDATES),
    errors: Vec::new(),
  };

  assert_eq!(ranked.candidates.len(), DEFAULT_MAX_CANDIDATES);
  assert!(ranked.errors.is_empty());
  for pair in ranked.candidates.windows(2) {
    assert!(pair[0].similarity_score >= pair[1].similarity_score);
  }
}

#[test]
fn rank_results_reports_failed_providers_and_keeps_the_others() {
  let query = TagSearchQuery {
    artist: Some("Adam Beyer".to_string()),
    title: "Your Mind".to_string(),
  };
  let results = vec![
    ProviderSearchResult {
      provider: "beatport".to_string(),
      candidates: Vec::new(),
      error: Some("403 Forbidden".to_string()),
    },
    ProviderSearchResult {
      provider: "bandcamp".to_string(),
      candidates: vec![candidate(
        "bandcamp",
        "Your Mind",
        &["Adam Beyer"],
        Some(405_000),
      )],
      error: None,
    },
  ];
  let priority = vec!["beatport".to_string(), "bandcamp".to_string()];

  let ranked = rank_results(results, &query, 405_000, &priority, 0.3, 5);

  assert_eq!(ranked.errors.len(), 1);
  assert_eq!(ranked.errors[0].provider, "beatport");
  assert_eq!(ranked.errors[0].error, "403 Forbidden");

  assert_eq!(ranked.candidates.len(), 1);
  let only = &ranked.candidates[0];
  assert_eq!(only.candidate.provider, "bandcamp");
  assert!((only.similarity_score - 1.0).abs() < 1e-9);
  assert!(ranked
    .candidates
    .iter()
    .all(|candidate| candidate.candidate.provider != "beatport"));
}

#[test]
fn rank_results_keeps_candidates_that_arrive_alongside_a_provider_error() {
  let query = TagSearchQuery {
    artist: Some("Adam Beyer".to_string()),
    title: "Your Mind".to_string(),
  };
  let results = vec![ProviderSearchResult {
    provider: "traxsource".to_string(),
    candidates: vec![candidate(
      "traxsource",
      "Your Mind",
      &["Adam Beyer"],
      Some(405_000),
    )],
    error: Some("partial: page 2 timed out".to_string()),
  }];
  let priority = vec!["traxsource".to_string()];

  let ranked = rank_results(results, &query, 405_000, &priority, 0.3, 5);

  assert_eq!(ranked.errors.len(), 1);
  assert_eq!(ranked.errors[0].provider, "traxsource");
  assert_eq!(ranked.errors[0].error, "partial: page 2 timed out");

  assert_eq!(ranked.candidates.len(), 1);
  let only = &ranked.candidates[0];
  assert_eq!(only.candidate.provider, "traxsource");
  assert!((only.similarity_score - 1.0).abs() < 1e-9);
}

#[test]
fn rank_results_happy_path_orders_near_ties_by_provider_priority() {
  // A literal one-char typo on these short titles drops the hybrid mean well
  // below the 0.01 window ("Your Mind" -> "Your Mine" is a 0.0625 gap), so the
  // fixture uses a five-word title whose single typo lands the two scores inside
  // one tolerance group. The expected scores are derived from the real scorer,
  // not hardcoded.
  let query = TagSearchQuery {
    artist: Some("Adam Beyer".to_string()),
    title: "Reminiscing About The Summer Days".to_string(),
  };
  let beatport_title = "Reminiszing About The Summer Days";
  let results = vec![
    ProviderSearchResult {
      provider: "traxsource".to_string(),
      candidates: vec![candidate(
        "traxsource",
        "Reminiscing About The Summer Days",
        &["Adam Beyer"],
        Some(405_000),
      )],
      error: None,
    },
    ProviderSearchResult {
      provider: "beatport".to_string(),
      candidates: vec![candidate(
        "beatport",
        beatport_title,
        &["Adam Beyer"],
        Some(405_000),
      )],
      error: None,
    },
  ];
  // Priority puts beatport (the typo, the lower score) ahead of traxsource.
  let priority = vec!["beatport".to_string(), "traxsource".to_string()];

  let scorer = UnifiedScorer::default_weights();
  let perfect = scorer.score(
    &query.title,
    "Adam Beyer",
    405_000,
    "Reminiscing About The Summer Days",
    "Adam Beyer",
    Some(405_000),
  );
  let typo = scorer.score(
    &query.title,
    "Adam Beyer",
    405_000,
    beatport_title,
    "Adam Beyer",
    Some(405_000),
  );
  assert!(
    (perfect - typo).abs() <= 0.01,
    "gap {} must fall inside one tolerance group",
    perfect - typo
  );

  let ranked = rank_results(results, &query, 405_000, &priority, 0.3, 5);

  assert!(ranked.errors.is_empty());
  let providers: Vec<&str> = ranked
    .candidates
    .iter()
    .map(|candidate| candidate.candidate.provider.as_str())
    .collect();
  let scores: Vec<f64> = ranked
    .candidates
    .iter()
    .map(|candidate| candidate.similarity_score)
    .collect();
  assert_eq!(providers, vec!["beatport", "traxsource"]);
  assert!((scores[0] - typo).abs() < 1e-9);
  assert!((scores[1] - perfect).abs() < 1e-9);
  assert!(
    scores[0] < scores[1],
    "the near-tie group may place the lower score first"
  );
}

#[test]
fn rank_results_without_local_artist_caps_the_score_at_seven_tenths() {
  let query = TagSearchQuery {
    artist: None,
    title: "Your Mind".to_string(),
  };
  let results = vec![ProviderSearchResult {
    provider: "bandcamp".to_string(),
    candidates: vec![candidate(
      "bandcamp",
      "Your Mind",
      &["Adam Beyer"],
      Some(405_000),
    )],
    error: None,
  }];
  let priority = vec!["bandcamp".to_string()];

  let loose = rank_results(results.clone(), &query, 405_000, &priority, 0.3, 5);
  assert_eq!(loose.candidates.len(), 1);
  assert!((loose.candidates[0].similarity_score - 0.7).abs() < 1e-9);

  let strict = rank_results(results, &query, 405_000, &priority, 0.8, 5);
  assert!(strict.candidates.is_empty());
}

#[test]
fn rank_candidates_may_order_a_lower_score_first() {
  // Non-monotonic by contract: 1.0 - 0.995 is within the tie tolerance, so
  // provider priority places the lower score first.
  let priority = vec!["p0".to_string(), "p1".to_string()];
  let candidates = vec![scored("p1", 1.0), scored("p0", 0.995)];

  let ranked = rank_candidates(candidates, &priority, 0.0, 10);

  let providers: Vec<&str> = ranked
    .iter()
    .map(|candidate| candidate.candidate.provider.as_str())
    .collect();
  let scores: Vec<f64> = ranked
    .iter()
    .map(|candidate| candidate.similarity_score)
    .collect();
  assert_eq!(providers, vec!["p0", "p1"]);
  assert_eq!(scores, vec![0.995, 1.0]);
}

#[test]
fn rank_candidates_grouping_boundary_follows_the_computed_gap() {
  let priority = vec!["p0".to_string(), "p1".to_string()];

  // (a) 1.0 - 0.9901 = 0.00990000000000002 <= 0.01 -> one group; priority wins.
  let inside = rank_candidates(
    vec![scored("p1", 1.0), scored("p0", 0.9901)],
    &priority,
    0.0,
    10,
  );
  let inside_providers: Vec<&str> = inside
    .iter()
    .map(|candidate| candidate.candidate.provider.as_str())
    .collect();
  let inside_scores: Vec<f64> = inside
    .iter()
    .map(|candidate| candidate.similarity_score)
    .collect();
  assert_eq!(inside_providers, vec!["p0", "p1"]);
  assert_eq!(inside_scores, vec![0.9901, 1.0]);

  // (b) 1.0 - 0.99 = 0.010000000000000009 > 0.01 -> two groups; score wins.
  // The float artifact is deliberate: the boundary has no epsilon.
  let outside = rank_candidates(
    vec![scored("p1", 1.0), scored("p0", 0.99)],
    &priority,
    0.0,
    10,
  );
  let outside_providers: Vec<&str> = outside
    .iter()
    .map(|candidate| candidate.candidate.provider.as_str())
    .collect();
  let outside_scores: Vec<f64> = outside
    .iter()
    .map(|candidate| candidate.similarity_score)
    .collect();
  assert_eq!(outside_providers, vec!["p1", "p0"]);
  assert_eq!(outside_scores, vec![1.0, 0.99]);
}

#[test]
fn artwork_extension_follows_the_content_type() {
  let url = "https://images.example.com/art";
  assert_eq!(artwork_extension(Some("image/jpeg"), url), "jpg");
  assert_eq!(artwork_extension(Some("image/png"), url), "png");
  assert_eq!(artwork_extension(Some("image/webp"), url), "webp");
  assert_eq!(artwork_extension(Some("image/gif"), url), "gif");
}

#[test]
fn artwork_extension_ignores_content_type_parameters_and_case() {
  assert_eq!(
    artwork_extension(Some("image/jpeg; charset=binary"), "https://x/art"),
    "jpg"
  );
  assert_eq!(artwork_extension(Some("IMAGE/PNG"), "https://x/art"), "png");
}

#[test]
fn artwork_extension_falls_back_to_the_url_path() {
  assert_eq!(
    artwork_extension(None, "https://images.example.com/cover.png"),
    "png"
  );
  assert_eq!(
    artwork_extension(None, "https://images.example.com/cover.JPEG"),
    "jpg"
  );
  // A query string must not be mistaken for the extension.
  assert_eq!(
    artwork_extension(None, "https://images.example.com/cover.webp?token=1"),
    "webp"
  );
}

#[test]
fn artwork_extension_defaults_to_jpg_without_a_usable_hint() {
  assert_eq!(
    artwork_extension(
      Some("application/octet-stream"),
      "https://images.example.com/cover"
    ),
    "jpg"
  );
  assert_eq!(
    artwork_extension(None, "https://images.example.com/cover"),
    "jpg"
  );
  assert_eq!(artwork_extension(None, "not-a-url"), "jpg");
}

#[test]
fn artwork_extension_prefers_content_type_over_a_disagreeing_url() {
  assert_eq!(
    artwork_extension(Some("image/png"), "https://images.example.com/cover.jpg"),
    "png"
  );
  assert_eq!(
    artwork_extension(Some("image/jpeg"), "https://images.example.com/cover.png"),
    "jpg"
  );
}
