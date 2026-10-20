//! Mappers from the raw NML model to a dry-run [`ImportPlan`].
//!
//! Nothing here touches the database or the filesystem: the plan is a pure
//! description of what S4 would import, plus a report of everything that could
//! not be mapped cleanly.

use std::collections::HashSet;

use serde::Serialize;

use crate::models::CueType;

use super::key::{self, KeyOutcome};
use super::model::{NmlCue, NmlDocument, NmlNodeKind, NmlPlaylistNode, NmlTrack};
use super::path;

/// A cue as it would be written to Crate's `cues` table.
///
/// Positions keep Traktor's full `f64` millisecond precision. Crate's persisted
/// `cues.position_ms` / `loop_end_ms` columns are `INTEGER`, so the lossy rounding
/// happens exactly once at that boundary (see `traktor_import::round_ms_to_column`),
/// never here: cue identity, ordering and de-duplication must compare the precise
/// value or two distinct cues that share a rounded position would collapse.
#[derive(Debug, Clone, Serialize)]
pub struct MappedCue {
    /// `START`, full float milliseconds as Traktor wrote it.
    pub position_ms: f64,
    pub cue_type: CueType,
    /// `START + LEN` for a loop, full float milliseconds.
    pub loop_end_ms: Option<f64>,
    /// 0-based slot (`0..=7`), the same base as Traktor's raw `HOTCUE`.
    pub hot_cue_index: Option<i32>,
    pub name: Option<String>,
}

/// A track as it would be imported.
///
/// Fields with no Crate column (`filesize_kb`, `release_date`, `playtime_s`,
/// `tempo_bpm`, `audio_id`, `coverart_id`) are still carried so the dry-run report
/// can surface them: nothing Traktor wrote disappears silently (decision D12).
#[derive(Debug, Clone, Serialize)]
pub struct MappedTrack {
    pub file_path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Traktor's `TEMPO BPM`. Reported and preserved, never written to `tracks.bpm`,
    /// which belongs to Crate's own analysis (decision D10).
    pub bpm: Option<f64>,
    /// Canonical Camelot, e.g. `"6B"`. Never the raw `INFO KEY`.
    pub key: Option<String>,
    pub grid_bpm: Option<f64>,
    /// Position of the grid's first beat, full float milliseconds as Traktor wrote it.
    pub grid_position_ms: Option<f64>,
    // --- Fields with a Crate `tracks` column (decision D11) ---
    /// `INFO RANKING` (0-255) mapped onto Crate's 0-5 stars (decision D7).
    pub rating: Option<i32>,
    /// `INFO PLAYCOUNT` as-is.
    pub play_count: Option<i32>,
    /// `INFO BITRATE` (bps) converted to kbps for `tracks.bitrate` (decision D8).
    pub bitrate: Option<i32>,
    /// `INFO IMPORT_DATE`, normalized from `YYYY/M/D` to RFC3339 (decision D9).
    pub date_added: Option<String>,
    /// `INFO LAST_PLAYED`, normalized from `YYYY/M/D` to RFC3339 (decision D9).
    pub last_played: Option<String>,
    pub genre: Option<String>,
    pub label: Option<String>,
    // --- Fields preserved for reporting / exporter input (decision D12) ---
    /// `ENTRY AUDIO_ID`: Traktor's stable track identifier, kept for the exporter.
    pub audio_id: Option<String>,
    /// `INFO COVERARTID`: artwork reference, kept for the exporter.
    pub coverart_id: Option<String>,
    /// `INFO FILESIZE` in KB. No Crate column.
    pub filesize_kb: Option<i64>,
    /// `INFO RELEASE_DATE` as Traktor wrote it (`YYYY/M/D`). No Crate column.
    pub release_date: Option<String>,
    /// `INFO PLAYTIME` in seconds. No Crate column.
    pub playtime_s: Option<i32>,
    pub cues: Vec<MappedCue>,
}

/// A playlist entry resolved against the parsed collection.
#[derive(Debug, Clone, Serialize)]
pub struct MappedPlaylistEntry {
    pub key: String,
    pub path: String,
    pub resolved: bool,
}

/// A node of the mapped playlist tree.
#[derive(Debug, Clone, Serialize)]
pub struct MappedPlaylistNode {
    pub name: String,
    pub kind: NmlNodeKind,
    pub uuid: Option<String>,
    pub search_query: Option<String>,
    pub entries: Vec<MappedPlaylistEntry>,
    pub children: Vec<MappedPlaylistNode>,
}

/// The full dry-run result: what would be imported and what would not.
#[derive(Debug, Clone, Serialize)]
pub struct ImportPlan {
    pub tracks: Vec<MappedTrack>,
    pub playlists: Vec<MappedPlaylistNode>,
    pub report: ImportReport,
}

/// Aggregate counts of a dry run.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportTotals {
    pub tracks: usize,
    pub cues: usize,
    pub memory_cues: usize,
    pub hot_cues: usize,
    pub loops: usize,
    pub grids: usize,
}

/// How many mapped tracks carried each field Traktor wrote but Crate has no column
/// for (decision D12). Reported so a dry-run consumer can see the data existed
/// instead of it disappearing silently.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NotPersistedCounts {
    /// Tracks with a Traktor `TEMPO BPM` (never written to `tracks.bpm`, decision D10).
    pub tempo_bpm: usize,
    /// Tracks with `INFO FILESIZE` (no Crate column).
    pub filesize: usize,
    /// Tracks with `INFO RELEASE_DATE` (no Crate column).
    pub release_date: usize,
    /// Tracks with `INFO PLAYTIME` (no Crate column).
    pub playtime: usize,
    /// Tracks with an `ENTRY AUDIO_ID` (preserved for the exporter, no Crate column).
    pub audio_id: usize,
    /// Tracks with an `INFO COVERARTID` (preserved for the exporter, no Crate column).
    pub coverart_id: usize,
}

/// Playlist node counts by type (the virtual `$ROOT` is not counted).
#[derive(Debug, Clone, Default, Serialize)]
pub struct PlaylistTypeCounts {
    pub folders: usize,
    pub playlists: usize,
    pub smartlists: usize,
}

/// A track-attributed warning.
#[derive(Debug, Clone, Serialize)]
pub struct TrackWarning {
    pub file_path: String,
    pub title: Option<String>,
}

/// A `INFO KEY` value that no notation could resolve.
#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedKeyWarning {
    pub file_path: String,
    pub title: Option<String>,
    pub raw_key: String,
}

/// A cue whose Traktor `TYPE` has no Crate equivalent and was kept as a memory cue.
#[derive(Debug, Clone, Serialize)]
pub struct UnsupportedCueTypeWarning {
    pub file_path: String,
    pub traktor_type: i32,
    /// Full float milliseconds, matching [`MappedCue::position_ms`].
    pub position_ms: f64,
}

/// A playlist `PRIMARYKEY` that does not resolve to a parsed track.
#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedPlaylistReferenceWarning {
    pub playlist: String,
    pub key: String,
}

/// Everything the dry run could not map cleanly.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportWarnings {
    pub tracks_without_grid: Vec<TrackWarning>,
    pub tracks_without_tempo: Vec<TrackWarning>,
    pub tracks_without_key: Vec<TrackWarning>,
    pub unresolved_keys: Vec<UnresolvedKeyWarning>,
    pub collapsed_cue_duplicates: usize,
    pub unsupported_cue_types: Vec<UnsupportedCueTypeWarning>,
    pub unresolved_playlist_references: Vec<UnresolvedPlaylistReferenceWarning>,
}

/// How many mapped tracks carried each `INFO` field Crate **can** store (decision D11).
/// Lets the dry run report the same field census the fidelity probe produced.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MappedFieldCounts {
    /// Tracks with an `INFO RANKING` mapped onto `tracks.rating`.
    pub rating: usize,
    /// Tracks with an `INFO PLAYCOUNT` mapped onto `tracks.play_count`.
    pub play_count: usize,
    /// Tracks with an `INFO BITRATE` mapped onto `tracks.bitrate`.
    pub bitrate: usize,
    /// Tracks with an `INFO IMPORT_DATE` parsed into `tracks.date_added`.
    pub date_added: usize,
    /// Tracks with an `INFO LAST_PLAYED` parsed into `tracks.last_played`.
    pub last_played: usize,
    /// Tracks with an `INFO GENRE`.
    pub genre: usize,
    /// Tracks with an `INFO LABEL`.
    pub label: usize,
}

/// The dry-run report. Mirrored in TypeScript in a later slice.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportReport {
    pub nml_version: Option<String>,
    pub totals: ImportTotals,
    /// Field census of what Crate will store.
    pub mapped_fields: MappedFieldCounts,
    /// Field census of what Crate has no column for.
    pub not_persisted: NotPersistedCounts,
    pub playlists_by_type: PlaylistTypeCounts,
    pub warnings: ImportWarnings,
}

/// Map a parsed document to its dry-run plan.
pub fn build_import_plan(document: &NmlDocument) -> ImportPlan {
    map_document(document)
}

fn map_document(document: &NmlDocument) -> ImportPlan {
    let mut report = ImportReport {
        nml_version: document.version.clone(),
        ..Default::default()
    };

    let mut known_paths: HashSet<String> = HashSet::new();
    let mut tracks = Vec::with_capacity(document.tracks.len());
    for track in &document.tracks {
        let mapped = map_track(track, &mut report);
        known_paths.insert(mapped.file_path.clone());
        tracks.push(mapped);
    }

    let playlists = map_playlists(&document.playlists, &known_paths, &mut report);

    for track in &tracks {
        if track.grid_bpm.is_some() {
            report.totals.grids += 1;
        }
        for cue in &track.cues {
            report.totals.cues += 1;
            match &cue.cue_type {
                CueType::Memory => report.totals.memory_cues += 1,
                CueType::Hot => report.totals.hot_cues += 1,
                CueType::Loop => report.totals.loops += 1,
            }
        }
        census(track, &mut report);
    }
    report.totals.tracks = tracks.len();

    ImportPlan {
        tracks,
        playlists,
        report,
    }
}

/// Count, for one mapped track, which `INFO` fields it carried — split between the
/// ones Crate stores (`mapped_fields`) and the ones it has no column for
/// (`not_persisted`). Values that failed to map (e.g. an unparseable date) count as
/// absent for the mapped side, so the report never claims a field was persisted.
fn census(track: &MappedTrack, report: &mut ImportReport) {
    let mapped = &mut report.mapped_fields;
    mapped.rating += usize::from(track.rating.is_some());
    mapped.play_count += usize::from(track.play_count.is_some());
    mapped.bitrate += usize::from(track.bitrate.is_some());
    mapped.date_added += usize::from(track.date_added.is_some());
    mapped.last_played += usize::from(track.last_played.is_some());
    mapped.genre += usize::from(track.genre.is_some());
    mapped.label += usize::from(track.label.is_some());

    let not_persisted = &mut report.not_persisted;
    not_persisted.tempo_bpm += usize::from(track.bpm.is_some());
    not_persisted.filesize += usize::from(track.filesize_kb.is_some());
    not_persisted.release_date += usize::from(track.release_date.is_some());
    not_persisted.playtime += usize::from(track.playtime_s.is_some());
    not_persisted.audio_id += usize::from(track.audio_id.is_some());
    not_persisted.coverart_id += usize::from(track.coverart_id.is_some());
}

/// Traktor `HOTCUE` is 0-based (`0` = slot 1 on the hardware). Crate stores the same
/// 0-based slot, so the value passes through unchanged; only the "no slot" sentinel
/// (`-1`) is dropped.
fn hot_cue_index(hot_cue: i32) -> Option<i32> {
    (hot_cue >= 0).then_some(hot_cue)
}

/// Deterministic ordering rank used for cues with the same position.
fn cue_rank(cue_type: &CueType) -> u8 {
    match cue_type {
        CueType::Memory => 0,
        CueType::Hot => 1,
        CueType::Loop => 2,
    }
}

fn map_cue(cue: &NmlCue, file_path: &str, warnings: &mut ImportWarnings) -> Option<MappedCue> {
    // Keep Traktor's full float precision here: identity and ordering depend on it.
    let position_ms = cue.start_ms;
    match cue.traktor_type {
        // Beat-grid marker: read as the grid, never emitted as a Crate cue.
        4 => None,
        5 => Some(MappedCue {
            position_ms,
            cue_type: CueType::Loop,
            // `START + LEN` as floats; the integer conversion happens at the DB boundary.
            loop_end_ms: Some(cue.start_ms + cue.length_ms),
            hot_cue_index: hot_cue_index(cue.hot_cue),
            name: cue.name.clone(),
        }),
        // TYPE 0 with HOTCUE=-1 is a memory cue.
        0 if cue.hot_cue < 0 => Some(MappedCue {
            position_ms,
            cue_type: CueType::Memory,
            loop_end_ms: None,
            hot_cue_index: None,
            name: cue.name.clone(),
        }),
        // TYPE 0 with a slot is a hot cue.
        0 => Some(MappedCue {
            position_ms,
            cue_type: CueType::Hot,
            loop_end_ms: None,
            hot_cue_index: hot_cue_index(cue.hot_cue),
            name: cue.name.clone(),
        }),
        // Fade-in (1), fade-out (2), load (3) and unknown types: preserve the
        // position as a memory cue and flag it, rather than dropping data.
        other => {
            warnings
                .unsupported_cue_types
                .push(UnsupportedCueTypeWarning {
                    file_path: file_path.to_string(),
                    traktor_type: other,
                    position_ms,
                });
            Some(MappedCue {
                position_ms,
                cue_type: CueType::Memory,
                loop_end_ms: None,
                hot_cue_index: None,
                name: cue.name.clone(),
            })
        }
    }
}

fn map_track(track: &NmlTrack, report: &mut ImportReport) -> MappedTrack {
    let file_path = path::resolve_location(
        track.location.volume.as_deref().unwrap_or(""),
        &track.location.dir,
        &track.location.file,
    );
    let warning = || TrackWarning {
        file_path: file_path.clone(),
        title: track.title.clone(),
    };

    let grid = track.grid();
    if grid.is_none() {
        report.warnings.tracks_without_grid.push(warning());
    }
    if track.tempo_bpm.is_none() {
        report.warnings.tracks_without_tempo.push(warning());
    }

    let key = match key::resolve_key(track.musical_key, track.info_key.as_deref()) {
        KeyOutcome::Camelot(camelot) => Some(camelot),
        KeyOutcome::Unresolved(raw_key) => {
            report.warnings.unresolved_keys.push(UnresolvedKeyWarning {
                file_path: file_path.clone(),
                title: track.title.clone(),
                raw_key,
            });
            None
        }
        KeyOutcome::Missing => {
            report.warnings.tracks_without_key.push(warning());
            None
        }
    };

    let mut cues: Vec<MappedCue> = Vec::new();
    for cue in &track.cues {
        if let Some(mapped) = map_cue(cue, &file_path, &mut report.warnings) {
            cues.push(mapped);
        }
    }
    // DISPL_ORDER is 0 for every real cue, so order by (position, type, slot).
    // `f64` is not `Ord`, so order through `partial_cmp` on the precise value; the
    // `TotalOrder` fallback only fires for a NaN position, which a real cue never has.
    cues.sort_by(|a, b| {
        a.position_ms
            .partial_cmp(&b.position_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| cue_rank(&a.cue_type).cmp(&cue_rank(&b.cue_type)))
            .then_with(|| {
                a.hot_cue_index
                    .unwrap_or(-1)
                    .cmp(&b.hot_cue_index.unwrap_or(-1))
            })
    });
    // S1 guarantees cue identity as (position_ms, type, hot_cue_index). Identity uses
    // the exact `f64` bits, so two cues at the same precise position collapse but the
    // `Paseando` `52.496647` grid + hot-cue pair no longer share a rounded position.
    let before = cues.len();
    let mut seen: HashSet<(u64, u8, Option<i32>)> = HashSet::new();
    cues.retain(|cue| {
        seen.insert((
            cue.position_ms.to_bits(),
            cue_rank(&cue.cue_type),
            cue.hot_cue_index,
        ))
    });
    report.warnings.collapsed_cue_duplicates += before - cues.len();

    MappedTrack {
        file_path,
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        bpm: track.tempo_bpm,
        key,
        grid_bpm: grid.map(|grid| grid.bpm),
        grid_position_ms: grid.map(|grid| grid.position_ms),
        rating: track.ranking.map(rating_from_ranking),
        play_count: track.play_count,
        bitrate: track.bitrate_bps.map(bitrate_kbps),
        date_added: track
            .import_date
            .as_deref()
            .and_then(traktor_date_to_rfc3339),
        last_played: track
            .last_played
            .as_deref()
            .and_then(traktor_date_to_rfc3339),
        genre: track.genre.clone(),
        label: track.label.clone(),
        audio_id: track.audio_id.clone(),
        coverart_id: track.coverart_id.clone(),
        filesize_kb: track.filesize_kb,
        release_date: track.release_date.clone(),
        playtime_s: track.playtime_s,
        cues,
    }
}

/// Map Traktor's 0-255 `INFO RANKING` onto Crate's 0-5 star rating (decision D7).
///
/// `round(v / 51)` places the six steps Traktor writes `{0, 51, 102, 153, 204, 255}`
/// exactly on `{0, 1, 2, 3, 4, 5}` (`255 / 51 == 5`). The result is clamped to
/// `0..=5` so a malformed value can never violate the `valid_rating` CHECK
/// constraint on `tracks.rating`.
fn rating_from_ranking(ranking: i32) -> i32 {
    let ranking = ranking.clamp(0, 255);
    ((ranking as f64 / 51.0).round() as i32).clamp(0, 5)
}

/// Convert Traktor's `INFO BITRATE` from bits per second to the kbps Crate stores
/// in `tracks.bitrate` (decision D8). Traktor writes `320000`; writing that raw
/// would make the track claim ~320 Mbps.
fn bitrate_kbps(bits_per_second: i32) -> i32 {
    ((bits_per_second as f64) / 1000.0).round() as i32
}

/// Normalize Traktor's `YYYY/M/D` date attributes to the RFC3339 string Crate's
/// `date_added` / `last_played` columns hold (decision D9).
///
/// `tracks.date_added` is `TEXT NOT NULL` written as RFC3339 by `Track::new`
/// (`chrono::Utc::now().to_rfc3339()`); Traktor's format is neither zero-padded nor
/// RFC3339 and must never be written raw. The components are split on `/` and fed to
/// `NaiveDate::from_ymd_opt` rather than a `strftime` pattern, so no platform-specific
/// format specifier (`%-m`, `%-d`) is involved. A date that does not parse yields
/// `None` — the importer then leaves the existing column value alone, never panics.
fn traktor_date_to_rfc3339(value: &str) -> Option<String> {
    let mut parts = value.split('/');
    let year: i32 = parts.next()?.trim().parse().ok()?;
    let month: u32 = parts.next()?.trim().parse().ok()?;
    let day: u32 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let datetime = date.and_hms_opt(0, 0, 0)?.and_utc();
    Some(datetime.to_rfc3339())
}

fn map_playlists(
    nodes: &[NmlPlaylistNode],
    known_paths: &HashSet<String>,
    report: &mut ImportReport,
) -> Vec<MappedPlaylistNode> {
    nodes
        .iter()
        .map(|node| map_playlist(node, known_paths, report))
        .collect()
}

fn map_playlist(
    node: &NmlPlaylistNode,
    known_paths: &HashSet<String>,
    report: &mut ImportReport,
) -> MappedPlaylistNode {
    match node.kind {
        NmlNodeKind::Folder => report.playlists_by_type.folders += 1,
        NmlNodeKind::Playlist => report.playlists_by_type.playlists += 1,
        NmlNodeKind::Smartlist => report.playlists_by_type.smartlists += 1,
    }

    let entries = node
        .track_keys
        .iter()
        .map(|key| {
            let path = path::resolve_primary_key(key);
            let resolved = known_paths.contains(&path);
            if !resolved {
                report.warnings.unresolved_playlist_references.push(
                    UnresolvedPlaylistReferenceWarning {
                        playlist: node.name.clone(),
                        key: key.clone(),
                    },
                );
            }
            MappedPlaylistEntry {
                key: key.clone(),
                path,
                resolved,
            }
        })
        .collect();

    let children = map_playlists(&node.children, known_paths, report);

    MappedPlaylistNode {
        name: node.name.clone(),
        kind: node.kind,
        uuid: node.uuid.clone(),
        search_query: node.search_query.clone(),
        entries,
        children,
    }
}
