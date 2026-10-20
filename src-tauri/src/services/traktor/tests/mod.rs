//! Fixture-driven tests for the NML parser, the mappers and the dry-run report.
//!
//! The committed fixture is a sanitized Traktor Pro 4 export; the real 264-entry
//! export lives at `data/collection.nml` and is exercised by the `#[ignore]`d
//! `real_collection` test below.

use super::mapper::{ImportPlan, MappedTrack};
use super::*;

const FIXTURE: &str = include_str!("../fixtures/collection_small.nml");

fn fixture_plan() -> ImportPlan {
    let doc = parser::parse_str(FIXTURE).expect("fixture must parse");
    mapper::build_import_plan(&doc)
}

fn track<'a>(plan: &'a ImportPlan, title: &str) -> &'a MappedTrack {
    plan.tracks
        .iter()
        .find(|t| t.title.as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no track titled {title:?}"))
}

/// Build the expected native path for a `VOLUME + DIR + FILE` triple without
/// reusing the production mapper, so the assertion is not a tautology.
fn expected_path(volume: &str, segments: &[&str], file: &str) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    let mut parts: Vec<String> = Vec::new();
    if cfg!(windows) && !volume.is_empty() {
        parts.push(volume.to_string());
    }
    parts.extend(segments.iter().map(|s| s.to_string()));
    parts.push(file.to_string());
    let joined = parts.join(&sep.to_string());
    if cfg!(windows) {
        joined
    } else {
        format!("{sep}{joined}")
    }
}

#[test]
fn parses_the_collection_and_playlist_tree() {
    let doc = parser::parse_str(FIXTURE).expect("fixture must parse");
    assert_eq!(doc.version.as_deref(), Some("20"));
    assert_eq!(doc.tracks.len(), 8);
    // `$ROOT` is transparent: its children are the parsed forest.
    assert_eq!(doc.playlists.len(), 2);
    assert_eq!(doc.playlists[0].kind, model::NmlNodeKind::Smartlist);
    assert_eq!(doc.playlists[1].kind, model::NmlNodeKind::Folder);
}

#[test]
fn grid_is_detected_by_type_never_by_name() {
    let plan = fixture_plan();
    // The AutoGrid cue is the grid.
    assert_eq!(track(&plan, "Grid & Hot").grid_bpm, Some(124.0));
    // A grid cue named "Beat Marker" must still be read as the grid.
    assert_eq!(track(&plan, "Don’t Stop").grid_bpm, Some(122.0));
    // A TEMPO without a TYPE="4" cue must NOT synthesize a grid.
    assert_eq!(track(&plan, "No Grid").grid_bpm, None);
    assert_eq!(track(&plan, "No Grid").bpm, Some(128.0));
}

#[test]
fn hot_cue_keeps_traktor_zero_based_slot() {
    let plan = fixture_plan();
    let hot = track(&plan, "Grid & Hot")
        .cues
        .iter()
        .find(|c| c.cue_type == crate::models::CueType::Hot)
        .expect("hot cue mapped");
    // START="52.764416" keeps full precision; HOTCUE="0" stays 0 (Traktor's slot base).
    assert_eq!(hot.position_ms, 52.764416);
    assert_eq!(hot.hot_cue_index, Some(0));
}

#[test]
fn memory_cue_with_hotcue_minus_one_has_no_index() {
    let plan = fixture_plan();
    let memory = track(&plan, "Grid & Hot")
        .cues
        .iter()
        .find(|c| c.name.as_deref() == Some("Memory"))
        .expect("memory cue mapped");
    assert_eq!(memory.cue_type, crate::models::CueType::Memory);
    assert_eq!(memory.position_ms, 60.100000);
    assert_eq!(memory.hot_cue_index, None);
    assert_eq!(memory.loop_end_ms, None);
}

#[test]
fn loop_maps_length_and_hot_index() {
    let plan = fixture_plan();
    let lp = track(&plan, "Grid & Hot")
        .cues
        .iter()
        .find(|c| c.cue_type == crate::models::CueType::Loop)
        .expect("loop mapped");
    assert_eq!(lp.position_ms, 146487.200771);
    // START + LEN as floats; the integer conversion happens at the DB boundary.
    assert_eq!(lp.loop_end_ms, Some(161487.20613500001));
    assert_eq!(lp.hot_cue_index, Some(1));
}

#[test]
fn unsupported_cue_types_map_to_memory_with_a_warning() {
    let plan = fixture_plan();
    let unsupported: Vec<i32> = plan
        .report
        .warnings
        .unsupported_cue_types
        .iter()
        .map(|w| w.traktor_type)
        .collect();
    assert_eq!(unsupported, vec![1, 2, 3]);
    // They are preserved as memory cues rather than dropped.
    let eps = track(&plan, "Unsupported Cues");
    assert!(eps
        .cues
        .iter()
        .any(|c| c.name.as_deref() == Some("Fade In")
            && c.cue_type == crate::models::CueType::Memory));
}

#[test]
fn duplicate_cue_identity_is_collapsed() {
    let plan = fixture_plan();
    assert_eq!(plan.report.warnings.collapsed_cue_duplicates, 1);
    let eps = track(&plan, "Unsupported Cues");
    assert_eq!(
        eps.cues
            .iter()
            .filter(|c| c.position_ms == 40.0 && c.cue_type == crate::models::CueType::Memory)
            .count(),
        1
    );
    // 3 fade/load cues + 1 surviving duplicate.
    assert_eq!(eps.cues.len(), 4);
}

#[test]
fn cues_are_ordered_by_position_then_type_then_slot() {
    let plan = fixture_plan();
    let cues = &track(&plan, "Grid & Hot").cues;
    let positions: Vec<f64> = cues.iter().map(|c| c.position_ms).collect();
    assert_eq!(positions, vec![52.764416, 60.100000, 146487.200771]);
}

#[test]
fn track_without_tempo_is_reported() {
    let plan = fixture_plan();
    let titles: Vec<&str> = plan
        .report
        .warnings
        .tracks_without_tempo
        .iter()
        .filter_map(|w| w.title.as_deref())
        .collect();
    assert_eq!(titles, vec!["Recording_2026-08-24_00h08m12s"]);
}

#[test]
fn track_without_tempo_key_or_grid_is_reported() {
    let plan = fixture_plan();
    let no_grid = |list: &[mapper::TrackWarning]| {
        list.iter()
            .any(|w| w.title.as_deref() == Some("Recording_2026-08-24_00h08m12s"))
    };
    assert!(no_grid(&plan.report.warnings.tracks_without_grid));
    assert!(no_grid(&plan.report.warnings.tracks_without_tempo));
    assert!(no_grid(&plan.report.warnings.tracks_without_key));
    let rec = track(&plan, "Recording_2026-08-24_00h08m12s");
    assert_eq!(rec.key, None);
    assert_eq!(rec.grid_bpm, None);
    assert_eq!(rec.bpm, None);
}

#[test]
fn musical_key_enum_is_the_preferred_key_source() {
    let plan = fixture_plan();
    // Track 1 has MUSICAL_KEY VALUE="2" (D major -> 10B) and INFO KEY="3d".
    assert_eq!(track(&plan, "Grid & Hot").key.as_deref(), Some("10B"));
}

#[test]
fn info_key_resolves_through_open_key_notation() {
    let plan = fixture_plan();
    // "11d" is Open Key Bb major = Camelot 6B, NOT legacy harmony 11B.
    assert_eq!(track(&plan, "Don’t Stop").key.as_deref(), Some("6B"));
}

#[test]
fn info_key_resolves_camelot_notation() {
    let plan = fixture_plan();
    assert_eq!(track(&plan, "No Grid").key.as_deref(), Some("8B"));
}

#[test]
fn info_key_resolves_note_name_notation() {
    let plan = fixture_plan();
    assert_eq!(track(&plan, "Note Name").key.as_deref(), Some("4B")); // Ab Major
    assert_eq!(track(&plan, "Unsupported Cues").key.as_deref(), Some("8A")); // Am
}

#[test]
fn ambiguous_bare_note_name_is_reported_unresolved() {
    let plan = fixture_plan();
    let zeta = track(&plan, "Ambiguous");
    assert_eq!(zeta.key, None); // "Gb" has no mode -> not guessed
    assert!(plan
        .report
        .warnings
        .unresolved_keys
        .iter()
        .any(|w| w.raw_key == "Gb" && w.title.as_deref() == Some("Ambiguous")));
}

#[test]
fn file_names_decode_xml_entities_and_typographic_apostrophes() {
    let plan = fixture_plan();
    let entity = expected_path(
        "C:",
        &["Users", "dj", "Music", "BOX", "One"],
        "Alpha - Grid & Hot.mp3",
    );
    assert_eq!(track(&plan, "Grid & Hot").file_path, entity);

    let apostrophe = expected_path(
        "C:",
        &["Users", "dj", "Music", "BOX", "One"],
        "Beta - Don’t Stop.mp3",
    );
    assert_eq!(track(&plan, "Don’t Stop").file_path, apostrophe);
    assert!(track(&plan, "Don’t Stop").file_path.contains('\u{2019}'));
}

#[test]
fn nested_folder_and_smartlist_are_preserved() {
    let plan = fixture_plan();
    let smart = plan
        .playlists
        .iter()
        .find(|n| n.name == "Top rated tracks")
        .expect("smartlist present");
    assert_eq!(smart.kind, model::NmlNodeKind::Smartlist);
    assert_eq!(smart.search_query.as_deref(), Some("$RATING == 5"));

    let folder = plan
        .playlists
        .iter()
        .find(|n| n.name == "Crates")
        .expect("nested folder present");
    assert_eq!(folder.kind, model::NmlNodeKind::Folder);
    assert!(folder.entries.is_empty());
    let night = folder
        .children
        .iter()
        .find(|n| n.name == "Night")
        .expect("playlist inside nested folder");
    assert_eq!(night.entries.len(), 2);
}

#[test]
fn playlist_primary_keys_resolve_against_parsed_tracks() {
    let plan = fixture_plan();
    let folder = plan.playlists.iter().find(|n| n.name == "Crates").unwrap();
    let night = folder.children.iter().find(|n| n.name == "Night").unwrap();
    assert!(night.entries[0].resolved);
    assert_eq!(night.entries[0].path, track(&plan, "Grid & Hot").file_path);
    assert!(!night.entries[1].resolved);
    assert_eq!(plan.report.warnings.unresolved_playlist_references.len(), 1);
    assert_eq!(
        plan.report.warnings.unresolved_playlist_references[0].key,
        "C:/:Users/:dj/:Music/:BOX/:One/:Missing Track.mp3"
    );
}

#[test]
fn playlist_counts_ignore_the_virtual_root() {
    let plan = fixture_plan();
    assert_eq!(plan.report.playlists_by_type.folders, 1);
    assert_eq!(plan.report.playlists_by_type.playlists, 2);
    assert_eq!(plan.report.playlists_by_type.smartlists, 1);
}

#[test]
fn totals_partition_cues_and_grids() {
    let plan = fixture_plan();
    let t = &plan.report.totals;
    assert_eq!(t.tracks, 8);
    assert_eq!(t.grids, 6);
    assert_eq!(t.hot_cues, 3);
    assert_eq!(t.memory_cues, 6);
    assert_eq!(t.loops, 1);
    assert_eq!(t.cues, 10);
}

#[test]
fn mixed_precision_cues_keep_distinct_positions() {
    let plan = fixture_plan();
    let cues = &track(&plan, "Mixed Precision").cues;
    // The AutoGrid is read as the grid, never as a cue.
    assert_eq!(
        track(&plan, "Mixed Precision").grid_position_ms,
        Some(52.496647)
    );
    assert_eq!(track(&plan, "Mixed Precision").grid_bpm, Some(129.000046));
    // Two hot cues: one shares the grid's exact timestamp, the other is multi-second.
    // Both must survive as distinct positions instead of collapsing into one rounded ms.
    assert_eq!(cues.len(), 2, "both hot cues must survive");
    let hot0 = cues.iter().find(|c| c.hot_cue_index == Some(0)).unwrap();
    let hot1 = cues.iter().find(|c| c.hot_cue_index == Some(1)).unwrap();
    assert_eq!(hot0.position_ms, 52.496647);
    assert_eq!(hot1.position_ms, 14883.715649);
    assert_ne!(
        hot0.position_ms, hot1.position_ms,
        "the pair must not collapse to one position"
    );
}

#[test]
fn cue_positions_keep_full_float_precision() {
    let plan = fixture_plan();
    let intro = track(&plan, "No Grid")
        .cues
        .iter()
        .find(|c| c.name.as_deref() == Some("Intro"))
        .unwrap();
    // START="5.500000": the mapper keeps the exact value; no rounding, no truncation.
    assert_eq!(intro.position_ms, 5.5);
}

#[test]
fn report_serializes_with_the_expected_shape() {
    let plan = fixture_plan();
    let value = serde_json::to_value(&plan.report).expect("report serializes");
    assert_eq!(value["nml_version"], "20");
    assert_eq!(value["totals"]["tracks"], 8);
    assert_eq!(value["playlists_by_type"]["smartlists"], 1);
    assert!(value["warnings"]["unsupported_cue_types"].is_array());
    // The two new censuses are present so nothing is silently dropped (D12).
    assert_eq!(value["mapped_fields"]["rating"], 8);
    assert!(value["not_persisted"]["tempo_bpm"].is_number());
}

#[test]
fn primary_key_path_matches_the_location_triple() {
    let loc = path::resolve_location(
        "C:",
        "/:Users/:dj/:Music/:BOX/:One/:",
        "Alpha - Grid & Hot.mp3",
    );
    let key = path::resolve_primary_key("C:/:Users/:dj/:Music/:BOX/:One/:Alpha - Grid & Hot.mp3");
    assert_eq!(loc, key);
}

#[test]
fn info_fields_map_onto_crate_columns() {
    let plan = fixture_plan();
    let t = track(&plan, "Grid & Hot");
    // RANKING="204" -> round(204 / 51) = 4; BITRATE="320000" bps -> 320 kbps.
    assert_eq!(t.rating, Some(4));
    assert_eq!(t.bitrate, Some(320));
    assert_eq!(t.play_count, Some(1));
    assert_eq!(t.genre.as_deref(), Some("House"));
    assert_eq!(t.label.as_deref(), Some("One Records"));
    // IMPORT_DATE="2026/8/23" / LAST_PLAYED="2026/8/24" -> RFC3339, never raw.
    assert_eq!(t.date_added.as_deref(), Some("2026-08-23T00:00:00+00:00"));
    assert_eq!(t.last_played.as_deref(), Some("2026-08-24T00:00:00+00:00"));
    // Preserved identifiers, never silently dropped.
    assert_eq!(t.audio_id.as_deref(), Some("AAA1"));
    assert_eq!(
        t.coverart_id.as_deref(),
        Some("001/AAAAAAAAAAAAAAAAAAAAAAAA")
    );
}

#[test]
fn ranking_maps_onto_the_full_zero_to_five_scale() {
    let plan = fixture_plan();
    // The fixture spans RANKING="255" (Mixed Precision) and RANKING="0" (Recording).
    assert_eq!(track(&plan, "Mixed Precision").rating, Some(5));
    assert_eq!(
        track(&plan, "Recording_2026-08-24_00h08m12s").rating,
        Some(0)
    );
    // RANKING="153" -> round(153 / 51) = 3.
    assert_eq!(track(&plan, "Don’t Stop").rating, Some(3));
}

#[test]
fn empty_info_attributes_map_to_none_not_empty_strings() {
    let plan = fixture_plan();
    let rec = track(&plan, "Recording_2026-08-24_00h08m12s");
    // Traktor writes GENRE="" / LABEL="" / COMMENT="" for unset fields; those must be
    // absent so a re-import never blanks an existing Crate value.
    assert_eq!(rec.genre, None);
    assert_eq!(rec.label, None);
    // BITRATE="0" stays a value (0 kbps) rather than an empty string; it is a real number.
    assert_eq!(rec.bitrate, Some(0));
}

#[test]
fn report_census_surfaces_fields_crate_cannot_store() {
    let plan = fixture_plan();
    let census = &plan.report.not_persisted;
    // TEMPO BPM is reported, never written to tracks.bpm (D10).
    assert_eq!(census.tempo_bpm, 7, "7 of 8 entries carry a TEMPO");
    assert_eq!(census.filesize, 8);
    assert_eq!(census.release_date, 7);
    assert_eq!(census.audio_id, 8);
    assert_eq!(census.coverart_id, 7, "the empty COVERARTID is absent");
    // Shared with T6's field-count assertions: what Crate *can* store.
    assert_eq!(plan.report.mapped_fields.bitrate, 8);
    assert_eq!(plan.report.mapped_fields.genre, 7);
}

/// Parses the private 264-entry export when it is present on disk. The file is
/// gitignored upstream and may legitimately be absent, so the test no-ops.
///
/// ```text
/// cd src-tauri && cargo test --features desktop -- --ignored real_collection
/// ```
#[test]
#[ignore = "reads the private data/collection.nml export; run manually with --ignored"]
fn real_collection() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/collection.nml");
    if !path.exists() {
        eprintln!("skipping: {} is not present", path.display());
        return;
    }
    let doc = parser::parse_file(&path).expect("real collection parses");
    assert_eq!(doc.tracks.len(), 264);

    let plan = mapper::build_import_plan(&doc);
    assert_eq!(plan.report.totals.tracks, 264);
    // Every entry maps to a distinct file path (no path collisions in the export).
    let distinct_paths: std::collections::HashSet<&str> =
        plan.tracks.iter().map(|t| t.file_path.as_str()).collect();
    assert_eq!(distinct_paths.len(), 264);
    assert_eq!(plan.report.totals.grids, 246);
    // 246 memory/grid cues with HOTCUE=-1 are grids, not Crate cues.
    assert_eq!(plan.report.playlists_by_type.smartlists, 3);

    // Crate cue totals: 43 hot cues + 2 loops + 0 memory cues.
    let t = &plan.report.totals;
    assert_eq!(t.cues, 45);
    assert_eq!(t.memory_cues, 0);
    assert_eq!(t.hot_cues, 43);
    assert_eq!(t.loops, 2);

    // `Paseando Por Sans`: the AutoGrid and hot cue 0 share the exact float timestamp
    // `52.496647` (only possible because T1 keeps full precision), hot cue 1 sits at
    // `14883.715649`. This is the proof that the rounded collision is gone.
    let paseando = plan
        .tracks
        .iter()
        .find(|t| t.title.as_deref() == Some("Paseando Por Sans (Original Mix)"))
        .expect("Paseando present in the real export");
    assert_eq!(paseando.grid_position_ms, Some(52.496647));
    assert_eq!(paseando.cues.len(), 2);
    let hot0 = paseando
        .cues
        .iter()
        .find(|c| c.hot_cue_index == Some(0))
        .expect("hot cue 0");
    let hot1 = paseando
        .cues
        .iter()
        .find(|c| c.hot_cue_index == Some(1))
        .expect("hot cue 1");
    assert_eq!(hot0.position_ms, 52.496647);
    assert_eq!(hot1.position_ms, 14883.715649);

    // `INFO` field census: what Crate stores, and what it can only report.
    let mapped = &plan.report.mapped_fields;
    assert_eq!(mapped.rating, 224);
    assert_eq!(mapped.play_count, 73);
    assert_eq!(mapped.bitrate, 264);
    assert_eq!(plan.report.not_persisted.coverart_id, 255);
}
