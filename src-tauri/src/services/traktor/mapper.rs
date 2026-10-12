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
#[derive(Debug, Clone, Serialize)]
pub struct MappedCue {
    pub position_ms: i64,
    pub cue_type: CueType,
    pub loop_end_ms: Option<i64>,
    /// 1-based slot (`1..=8`), never Traktor's raw 0-based value.
    pub hot_cue_index: Option<i32>,
    pub name: Option<String>,
}

/// A track as it would be imported.
#[derive(Debug, Clone, Serialize)]
pub struct MappedTrack {
    pub file_path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub bpm: Option<f64>,
    /// Canonical Camelot, e.g. `"6B"`. Never the raw `INFO KEY`.
    pub key: Option<String>,
    pub grid_bpm: Option<f64>,
    pub grid_position_ms: Option<i64>,
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
    pub position_ms: i64,
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

/// The dry-run report. Mirrored in TypeScript in a later slice.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportReport {
    pub nml_version: Option<String>,
    pub totals: ImportTotals,
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
    }
    report.totals.tracks = tracks.len();

    ImportPlan {
        tracks,
        playlists,
        report,
    }
}

/// Round float milliseconds to the `INTEGER` `position_ms` Crate stores.
///
/// Traktor writes sub-millisecond precision (`146487.200771`); the column is an
/// integer, so the fractional part is rounded (not truncated) and the sub-ms
/// loss is accepted for this slice — see the feature brief's open decision 4.
fn round_ms(value: f64) -> i64 {
    value.round() as i64
}

/// Traktor `HOTCUE` is 0-based; Crate's `hot_cue_index` is 1-based (`1..=8`).
fn hot_cue_index(hot_cue: i32) -> Option<i32> {
    (hot_cue >= 0).then_some(hot_cue + 1)
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
    let position_ms = round_ms(cue.start_ms);
    match cue.traktor_type {
        // Beat-grid marker: read as the grid, never emitted as a Crate cue.
        4 => None,
        5 => Some(MappedCue {
            position_ms,
            cue_type: CueType::Loop,
            loop_end_ms: Some(round_ms(cue.start_ms + cue.length_ms)),
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
    cues.sort_by_key(|cue| {
        (
            cue.position_ms,
            cue_rank(&cue.cue_type),
            cue.hot_cue_index.unwrap_or(-1),
        )
    });
    // S1 guarantees cue identity as (position_ms, type, hot_cue_index).
    let before = cues.len();
    let mut seen: HashSet<(i64, u8, Option<i32>)> = HashSet::new();
    cues.retain(|cue| seen.insert((cue.position_ms, cue_rank(&cue.cue_type), cue.hot_cue_index)));
    report.warnings.collapsed_cue_duplicates += before - cues.len();

    MappedTrack {
        file_path,
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        bpm: track.tempo_bpm,
        key,
        grid_bpm: grid.map(|grid| grid.bpm),
        grid_position_ms: grid.map(|grid| round_ms(grid.position_ms)),
        cues,
    }
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
