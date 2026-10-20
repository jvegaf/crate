//! Real (DB-aware) Traktor `collection.nml` import.
//!
//! This is the write side of the feature: it consumes the hermetic parser + mapper
//! (`services::traktor`, which stays ungated) and persists the plan — tracks, cues,
//! beat grids and keys into the `tracks`/`cues` tables, plus the playlist forest into
//! `playlists`/`playlist_tracks`. It performs **no** Tauri calls and no `async` work;
//! every lock scope is synchronous.
//!
//! The module is `desktop`-only because it reuses the desktop library import machinery
//! (`LibraryService`, which reads tags/hashes a file) plus `compute_audio_hash`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{CrateError, Result};
use crate::models::{Cue, CueType};
use crate::services::cloud_sync::pipeline::{buckets, dirty};
use crate::services::hash::compute_audio_hash;
use crate::services::library::LibraryService;
use crate::services::traktor::mapper::{
    self, ImportPlan, ImportReport, MappedCue, MappedPlaylistNode, MappedTrack,
};
use crate::services::traktor::model::NmlNodeKind;
use crate::services::traktor::parser;
use crate::services::{CueService, PlaylistService};

/// Origin stamped on every beat grid this importer writes.
const GRID_SOURCE_TRAKTOR: &str = "traktor";

/// Crate playlist context every imported node lands in.
const PLAYLIST_CONTEXT_LIBRARY: &str = "library";

/// A Traktor entry that could not be imported, with the reason it was skipped.
///
/// `reason` is one of `"file-not-found"`, `"hash-failed"` or `"metadata-failed"`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UnmatchedTraktorTrack {
    pub file_path: String,
    pub title: Option<String>,
    pub reason: String,
}

/// The result of a real import: the parser/mapper dry run plus what actually happened.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TraktorImportOutcome {
    /// The plan's own report (identical to [`TraktorImportService::preview`]).
    pub dry_run: ImportReport,
    /// Traktor entries whose audio file was already in Crate (matched by file path, or by
    /// content hash under a different path).
    pub matched: usize,
    /// Traktor entries whose audio file was ingested from disk in this run.
    pub ingested: usize,
    /// Cues persisted across all touched tracks.
    pub cues_written: usize,
    /// Beat grids written across all touched tracks.
    pub grids_written: usize,
    /// Keys written across all touched tracks.
    pub keys_written: usize,
    /// Tracks that received at least one mapped `INFO` field (`rating`, `play_count`,
    /// `bitrate`, `date_added`, `last_played`, `genre` or `label`).
    pub fields_written: usize,
    /// Entries that were reported and skipped without creating a row.
    pub unmatched: Vec<UnmatchedTraktorTrack>,
    /// Playlists and folders created or reused by this run (folders included).
    pub playlists_imported: usize,
    /// Members actually written across every imported playlist.
    pub playlist_tracks_written: usize,
    /// Playlist entries whose audio file is not in Crate (skipped from membership).
    pub unresolved_playlist_entries: usize,
    /// Traktor-internal playlist names skipped together with their whole subtree.
    pub skipped_internal_playlists: Vec<String>,
}

/// Persists a mapped Traktor plan into the library.
pub struct TraktorImportService {
    conn: Arc<Mutex<Connection>>,
    library: LibraryService,
    cues: CueService,
    playlists: PlaylistService,
}

impl TraktorImportService {
    pub fn new(conn: Arc<Mutex<Connection>>, library: LibraryService) -> Self {
        Self {
            cues: CueService::new(conn.clone()),
            playlists: PlaylistService::new(conn.clone()),
            conn,
            library,
        }
    }

    /// Parse + map only. No DB access, no filesystem writes.
    pub fn preview(&self, nml_path: &Path) -> Result<ImportPlan> {
        let document =
            parser::parse_file(nml_path).map_err(|e| CrateError::Import(e.to_string()))?;
        Ok(mapper::build_import_plan(&document))
    }

    /// Real import. Idempotent: a second run over the same NML matches every track by
    /// file path (or content hash under a different path) instead of inserting duplicates,
    /// rewrites the same grid/cues, and re-ensures the same playlist forest by
    /// `(name, parent_id, context)` instead of duplicating it.
    pub fn import(&self, nml_path: &Path) -> Result<TraktorImportOutcome> {
        let plan = self.preview(nml_path)?;
        let ImportPlan {
            tracks,
            playlists,
            report,
        } = plan;

        let mut outcome = TraktorImportOutcome {
            dry_run: report,
            matched: 0,
            ingested: 0,
            cues_written: 0,
            grids_written: 0,
            keys_written: 0,
            fields_written: 0,
            unmatched: Vec::new(),
            playlists_imported: 0,
            playlist_tracks_written: 0,
            unresolved_playlist_entries: 0,
            skipped_internal_playlists: Vec::new(),
        };

        for track in &tracks {
            let Some(id) = self.resolve_track_id(track, &mut outcome)? else {
                continue;
            };

            let (key_written, grid_written, fields_written) =
                self.apply_track_fields(&id, track)?;
            if key_written {
                outcome.keys_written += 1;
            }
            if grid_written {
                outcome.grids_written += 1;
            }
            if fields_written {
                outcome.fields_written += 1;
            }

            let cues: Vec<Cue> = track
                .cues
                .iter()
                .filter_map(|mapped| mapped_cue_to_model(&id, mapped))
                .collect();
            // An empty mapped list must NOT wipe cues the track already carries: a
            // Traktor entry with no cues is not an assertion that Crate's cues are
            // stale, and re-importing would otherwise silently destroy them.
            if !cues.is_empty() {
                let stored = self.cues.replace_cues_for_track(&id, cues)?;
                outcome.cues_written += stored.len();
            }
        }

        self.import_playlists(&playlists, None, &mut outcome)?;

        Ok(outcome)
    }

    /// Import the mapped playlist forest depth-first, in document order, preserving the
    /// NML nesting: `parent_id` is the Crate id of the enclosing node (`None` at the top
    /// level), so a child always lands under its real folder rather than flattened to a
    /// root playlist.
    ///
    /// A `Smartlist` is imported as an ordinary static playlist carrying its current
    /// entries; `ensure_playlist` never creates a smart row, so `is_smart` stays `false`
    /// and `smart_rules` stays `NULL`.
    ///
    /// A node with zero entries does not call `replace_tracks`: an empty NML playlist that
    /// collides with an existing Crate playlist is not an assertion that the playlist is
    /// empty, and clearing it would destroy membership a user may have added in Crate. A
    /// non-empty entry list that resolves to nothing *does* replace membership with the
    /// empty set, because the NML then explicitly asserts those entries — none of which
    /// exist in Crate.
    ///
    /// Nothing here holds the connection lock: `ensure_playlist`, `find_track_by_file_path`
    /// and `replace_tracks` each lock internally and return before the next call.
    fn import_playlists(
        &self,
        nodes: &[MappedPlaylistNode],
        parent_id: Option<&str>,
        outcome: &mut TraktorImportOutcome,
    ) -> Result<()> {
        for node in nodes {
            if is_internal_playlist(&node.name) {
                outcome.skipped_internal_playlists.push(node.name.clone());
                continue;
            }

            let is_folder = matches!(node.kind, NmlNodeKind::Folder);
            let playlist = self.playlists.ensure_playlist(
                &node.name,
                parent_id,
                is_folder,
                PLAYLIST_CONTEXT_LIBRARY,
            )?;
            outcome.playlists_imported += 1;

            if !is_folder && !node.entries.is_empty() {
                let mut ids = Vec::with_capacity(node.entries.len());
                for entry in &node.entries {
                    // Resolve every entry against Crate regardless of the plan's
                    // `resolved` flag: that flag only says the path is in the parsed
                    // collection, not that the file was successfully ingested.
                    match self.library.find_track_by_file_path(&entry.path)? {
                        Some(track) => ids.push(track.id),
                        None => outcome.unresolved_playlist_entries += 1,
                    }
                }
                outcome.playlist_tracks_written +=
                    self.playlists.replace_tracks(&playlist.id, ids)?;
            }

            self.import_playlists(&node.children, Some(&playlist.id), outcome)?;
        }

        Ok(())
    }

    /// Resolve the persisted row id for one mapped track, ingesting the file when it is
    /// not yet in Crate. Returns `None` when the entry is reported as unmatched.
    ///
    /// Takes `&mut TraktorImportOutcome` only to record counts/unmatched entries.
    fn resolve_track_id(
        &self,
        track: &MappedTrack,
        outcome: &mut TraktorImportOutcome,
    ) -> Result<Option<String>> {
        let path = PathBuf::from(&track.file_path);
        if !path.exists() {
            outcome.unmatched.push(unmatched(track, "file-not-found"));
            return Ok(None);
        }

        // Path first: `tracks.file_path` is UNIQUE, and a row that already owns the path
        // is the same track even when its stored hash has drifted (the file changed on
        // disk, or an earlier import never refreshed the hash). Match it without hashing
        // or re-inserting.
        if let Some(existing) = self.library.find_track_by_file_path(&track.file_path)? {
            outcome.matched += 1;
            return Ok(Some(existing.id));
        }

        let hash = match compute_audio_hash(&path) {
            Ok(hash) => hash,
            Err(_) => {
                outcome.unmatched.push(unmatched(track, "hash-failed"));
                return Ok(None);
            }
        };

        // Same content under a different path. Crate's scan keeps only one row per content
        // hash, so this is the same track and must be enriched, not duplicated.
        if let Some(existing) = self.library.find_track_by_hash(&hash)? {
            outcome.matched += 1;
            return Ok(Some(existing.id));
        }

        // The file is new to Crate: read it from disk (tags + hash + metadata) and insert.
        let built = match self.library.build_track_from_file(&path, hash) {
            Ok(built) => built,
            Err(_) => {
                outcome.unmatched.push(unmatched(track, "metadata-failed"));
                return Ok(None);
            }
        };

        // One lock scope, one transaction for the insert.
        {
            let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
            let tx = conn.unchecked_transaction()?;
            LibraryService::insert_track_in(&tx, &built)?;
            tx.commit()?;
        }

        // The upsert conflicts on `file_path`, which now exists, so the persisted row is
        // found by path. The `None` arm is a last-resort guard only.
        match self.library.find_track_by_file_path(&track.file_path)? {
            Some(persisted) => {
                outcome.ingested += 1;
                Ok(Some(persisted.id))
            }
            None => {
                outcome.unmatched.push(unmatched(track, "metadata-failed"));
                Ok(None)
            }
        }
    }

    /// Write every Traktor field Crate can store onto one resolved track row in a single
    /// mutation: key, beat grid, and the per-track `INFO` fields (`rating`, `play_count`,
    /// `bitrate`, `date_added`, `last_played`, `genre`, `label`). Mirrors
    /// `insert_track_in`'s `_hlc` stamp and dirty mark so there is exactly ONE UPDATE and
    /// one `_hlc` stamp per track — a second statement would race the stamp (decision D11).
    ///
    /// Returns `(key_written, grid_written, fields_written)`. A partial triple is never
    /// written: the grid columns are all written together or not at all. `fields_written`
    /// is `true` when at least one of the mapped `INFO` fields was persisted.
    fn apply_track_fields(&self, id: &str, track: &MappedTrack) -> Result<(bool, bool, bool)> {
        let grid = grid_triple(track);
        let has_key = track.key.is_some();
        let has_fields = has_persisted_fields(track);
        if !has_key && grid.is_none() && !has_fields {
            return Ok((false, false, false));
        }

        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let tx = conn.unchecked_transaction()?;
        let hlc = dirty::next_hlc(&tx)?;

        let mut set: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        // Append `column = ?N` only for the fields the NML actually carried, so a missing
        // attribute leaves the existing Crate value alone instead of overwriting it.
        let mut push = |column: &str, value: Box<dyn rusqlite::ToSql>| {
            params.push(value);
            set.push(format!("{column} = ?{}", params.len()));
        };

        if let Some(key) = &track.key {
            push("key", Box::new(key.clone()));
        }
        if let Some((grid_bpm, grid_anchor_ms, grid_source)) = grid {
            push("grid_bpm", Box::new(grid_bpm));
            push("grid_anchor_ms", Box::new(grid_anchor_ms));
            push("grid_source", Box::new(grid_source));
        }
        if let Some(rating) = track.rating {
            push("rating", Box::new(rating));
        }
        if let Some(play_count) = track.play_count {
            push("play_count", Box::new(play_count));
        }
        if let Some(bitrate) = track.bitrate {
            push("bitrate", Box::new(bitrate));
        }
        if let Some(date_added) = &track.date_added {
            push("date_added", Box::new(date_added.clone()));
        }
        if let Some(last_played) = &track.last_played {
            push("last_played", Box::new(last_played.clone()));
        }
        if let Some(genre) = &track.genre {
            push("genre", Box::new(genre.clone()));
        }
        if let Some(label) = &track.label {
            push("label", Box::new(label.clone()));
        }

        push("_hlc", Box::new(hlc));
        params.push(Box::new(id.to_string()));
        let id_index = params.len();

        let sql = format!(
            "UPDATE tracks SET {} WHERE id = ?{}",
            set.join(", "),
            id_index
        );
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        tx.execute(&sql, refs.as_slice())?;
        dirty::mark_dirty(&tx, &buckets::bucket_for_track_id(id))?;
        tx.commit()?;

        Ok((has_key, grid.is_some(), has_fields))
    }
}

/// Whether any of the mapped per-track `INFO` fields Crate can store is present.
///
/// The grid is handled separately ([`grid_triple`]); this covers `rating`, `play_count`,
/// `bitrate`, `date_added`, `last_played`, `genre` and `label`.
fn has_persisted_fields(track: &MappedTrack) -> bool {
    track.rating.is_some()
        || track.play_count.is_some()
        || track.bitrate.is_some()
        || track.date_added.is_some()
        || track.last_played.is_some()
        || track.genre.is_some()
        || track.label.is_some()
}

/// Traktor's own bookkeeping playlists (`_LOOPS`, `_RECORDINGS`) are not user playlists.
/// They are skipped by exact name together with their entire subtree.
fn is_internal_playlist(name: &str) -> bool {
    matches!(name, "_LOOPS" | "_RECORDINGS")
}

/// Round a mapped float millisecond value to the `INTEGER` Crate column that stores it.
///
/// The mapper deliberately keeps Traktor's full `f64` precision so cue identity,
/// ordering and de-duplication compare the precise value (a `52.496647` grid and a
/// hot cue at the same timestamp must not collapse). Crate's `cues.position_ms`,
/// `cues.loop_end_ms` and `tracks.grid_anchor_ms` are `INTEGER`, so this is the ONE
/// place the fractional part is dropped — at the persistence boundary, never inside
/// the mapper.
fn round_ms_to_column(value: f64) -> i64 {
    value.round() as i64
}

/// The mapper's grid is a pair, and Crate stores it as a `(bpm, anchor, source)` triple.
/// Both fields must be present; a partial grid is never written. The anchor is rounded
/// to the `INTEGER` column by [`round_ms_to_column`].
fn grid_triple(track: &MappedTrack) -> Option<(f64, i64, &'static str)> {
    match (track.grid_bpm, track.grid_position_ms) {
        (Some(bpm), Some(anchor_ms)) => {
            Some((bpm, round_ms_to_column(anchor_ms), GRID_SOURCE_TRAKTOR))
        }
        _ => None,
    }
}

/// Convert one mapped Traktor cue into a Crate [`Cue`].
///
/// The mapper already guarantees the type-specific fields (a `Hot` cue carries a
/// 0-based slot, a `Loop` carries an end after its start). The `?` guards are
/// defensive: a future mapper regression drops one cue instead of aborting the whole
/// import inside `replace_cues_for_track`'s validation or inventing a value.
///
/// Positions arrive as full-precision `f64` and are rounded once here, at the DB
/// boundary ([`round_ms_to_column`]).
fn mapped_cue_to_model(track_id: &str, mapped: &MappedCue) -> Option<Cue> {
    let position_ms = round_ms_to_column(mapped.position_ms);
    let mut cue = match mapped.cue_type {
        CueType::Memory => Cue::new_memory(track_id.to_string(), position_ms),
        CueType::Hot => Cue::new_hot(
            track_id.to_string(),
            position_ms,
            mapped.hot_cue_index.filter(|i| (0..=7).contains(i))?,
        ),
        CueType::Loop => Cue::new_loop(
            track_id.to_string(),
            position_ms,
            round_ms_to_column(mapped.loop_end_ms.filter(|end| *end > mapped.position_ms)?),
        ),
    };
    cue.name = mapped.name.clone();
    Some(cue)
}

/// Build the `unmatched` entry for a skipped Traktor track.
fn unmatched(track: &MappedTrack, reason: &str) -> UnmatchedTraktorTrack {
    UnmatchedTraktorTrack {
        file_path: track.file_path.clone(),
        title: track.title.clone(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../test_utils.rs"]
mod test_utils;

#[cfg(test)]
mod tests {
    use super::*;

    use crate::services::FileTagsService;

    /// Fresh service over an in-memory DB, plus the shared connection and the temp dir to
    /// keep alive (it backs the artwork/output paths).
    fn test_service() -> (
        TraktorImportService,
        Arc<Mutex<Connection>>,
        tempfile::TempDir,
    ) {
        let tmp = tempfile::tempdir().expect("temp dir");
        let conn = Arc::new(Mutex::new(test_utils::make_memory_db()));
        let library = LibraryService::new(
            conn.clone(),
            tmp.path().join("appdata"),
            FileTagsService::new(),
        );
        let service = TraktorImportService::new(conn.clone(), library);
        (service, conn, tmp)
    }

    /// Write a minimal valid 16-bit mono PCM WAV (same technique the scan tests use).
    ///
    /// `seed` varies the sample bytes so files with different seeds hash differently.
    fn write_wav(path: &Path, seed: u8, frames: u32) {
        let sample_rate: u32 = 44_100;
        let channels: u16 = 1;
        let bits: u16 = 16;
        let byte_rate = sample_rate * u32::from(channels) * u32::from(bits) / 8;
        let block_align = channels * bits / 8;
        let data_len = frames * u32::from(block_align);

        let mut bytes: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for frame in 0..frames {
            let sample = i16::from(seed).wrapping_mul(257).wrapping_add(frame as i16);
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, &bytes).expect("write wav");
    }

    /// Encode an absolute path as a Traktor `LOCATION` element (`/:` separator, separate
    /// `VOLUME`). Matches `services::traktor::path::resolve_location`.
    fn location_element(path: &Path) -> String {
        let file = path.file_name().expect("file name").to_string_lossy();
        let parent = path.parent().expect("parent");

        #[allow(unused_mut)]
        let mut volume = String::new();
        #[cfg(windows)]
        if let Some(std::path::Component::Prefix(prefix)) = parent.components().next() {
            volume = prefix
                .as_os_str()
                .to_string_lossy()
                .trim_end_matches('\\')
                .to_string();
        }

        let segments: Vec<String> = parent
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(segment) => {
                    Some(segment.to_string_lossy().to_string())
                }
                _ => None,
            })
            .collect();
        // Traktor prefixes every segment with `/:` and terminates the directory with it.
        let mut dir = String::new();
        for segment in &segments {
            dir.push_str("/:");
            dir.push_str(segment);
        }
        dir.push_str("/:");
        format!(r#"<LOCATION DIR="{dir}" FILE="{file}" VOLUME="{volume}"></LOCATION>"#)
    }

    /// One `COLLECTION/ENTRY` whose `LOCATION` points at `path` and whose XML body is
    /// `body` (INFO / MUSICAL_KEY / TEMPO / CUE_V2 elements).
    fn entry(path: &Path, title: &str, body: &str) -> String {
        format!(
            r#"<ENTRY TITLE="{title}" ARTIST="Tester">{}{body}</ENTRY>"#,
            location_element(path)
        )
    }

    /// Wrap entries in a minimal `NML VERSION="20"` document.
    fn nml(entries: &[String]) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<NML VERSION="20"><COLLECTION ENTRIES="{}">{}</COLLECTION></NML>"#,
            entries.len(),
            entries.join("")
        )
    }

    /// Encode `path` as a Traktor `PRIMARYKEY KEY`, the same `/:`-separated form
    /// `path::resolve_primary_key` decodes. Mirrors `location_element`'s volume/dir split so a
    /// playlist reference resolves to exactly the path its `LOCATION` produces.
    fn primary_key(path: &Path) -> String {
        let file = path.file_name().expect("file name").to_string_lossy();
        let parent = path.parent().expect("parent");

        #[allow(unused_mut)]
        let mut volume = String::new();
        #[cfg(windows)]
        if let Some(std::path::Component::Prefix(prefix)) = parent.components().next() {
            volume = prefix
                .as_os_str()
                .to_string_lossy()
                .trim_end_matches('\\')
                .to_string();
        }

        let segments: Vec<String> = parent
            .components()
            .filter_map(|component| match component {
                std::path::Component::Normal(segment) => {
                    Some(segment.to_string_lossy().to_string())
                }
                _ => None,
            })
            .collect();
        let mut dir = String::new();
        for segment in &segments {
            dir.push_str("/:");
            dir.push_str(segment);
        }
        dir.push_str("/:");
        format!("{volume}{dir}{file}")
    }

    /// A test playlist-tree node rendered into Traktor's `PLAYLISTS` XML.
    struct Pl {
        kind: &'static str,
        name: &'static str,
        entries: Vec<String>,
        query: Option<&'static str>,
        children: Vec<Pl>,
    }

    fn folder(name: &'static str, children: Vec<Pl>) -> Pl {
        Pl {
            kind: "FOLDER",
            name,
            entries: Vec::new(),
            query: None,
            children,
        }
    }

    fn playlist(name: &'static str, entries: Vec<String>) -> Pl {
        Pl {
            kind: "PLAYLIST",
            name,
            entries,
            query: None,
            children: Vec::new(),
        }
    }

    /// A Traktor `SMARTLIST` node carrying a search expression and (as Traktor caches them) the
    /// current result entries.
    fn smartlist(name: &'static str, query: &'static str, entries: Vec<String>) -> Pl {
        Pl {
            kind: "SMARTLIST",
            name,
            entries,
            query: Some(query),
            children: Vec::new(),
        }
    }

    fn render_node(node: &Pl, out: &mut String) {
        out.push_str(&format!(
            r#"<NODE TYPE="{}" NAME="{}">"#,
            node.kind, node.name
        ));
        if node.kind == "SMARTLIST" {
            out.push_str(r#"<SMARTLIST UUID="0000000000000000000000000000abcd">"#);
            if let Some(query) = node.query {
                out.push_str(&format!(
                    r#"<SEARCH_EXPRESSION VERSION="1" QUERY="{query}"></SEARCH_EXPRESSION>"#
                ));
            }
            out.push_str("</SMARTLIST>");
        }
        if node.kind != "FOLDER" {
            out.push_str(&format!(
                r#"<PLAYLIST ENTRIES="{}" TYPE="LIST">"#,
                node.entries.len()
            ));
            for key in &node.entries {
                out.push_str(&format!(
                    r#"<ENTRY><PRIMARYKEY TYPE="TRACK" KEY="{key}"></PRIMARYKEY></ENTRY>"#
                ));
            }
            out.push_str("</PLAYLIST>");
        }
        if !node.children.is_empty() {
            out.push_str(&format!(r#"<SUBNODES COUNT="{}">"#, node.children.len()));
            for child in &node.children {
                render_node(child, out);
            }
            out.push_str("</SUBNODES>");
        }
        out.push_str("</NODE>");
    }

    /// Render a playlist forest under Traktor's virtual `$ROOT` folder.
    fn render_playlists(nodes: &[Pl]) -> String {
        let mut out = format!(
            r#"<PLAYLISTS><NODE TYPE="FOLDER" NAME="$ROOT"><SUBNODES COUNT="{}">"#,
            nodes.len()
        );
        for node in nodes {
            render_node(node, &mut out);
        }
        out.push_str("</SUBNODES></NODE></PLAYLISTS>");
        out
    }

    /// Wrap collection entries and a rendered playlist forest into a full NML document.
    fn nml_with_playlists(entries: &[String], playlists: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<NML VERSION="20"><COLLECTION ENTRIES="{}">{}</COLLECTION>{playlists}</NML>"#,
            entries.len(),
            entries.join("")
        )
    }

    /// The XML body used by most tests: a Camelot key, an analyzed grid, one hot cue, one
    /// memory cue, one loop, and the `INFO` fields Crate can store (D11).
    fn full_body() -> &'static str {
        r#"<INFO KEY="8B" BITRATE="320000" PLAYCOUNT="3" RANKING="204" IMPORT_DATE="2026/8/23" LAST_PLAYED="2026/8/24" GENRE="House" LABEL="One Records" COMMENT="keep me" COVERARTID="001/AAAA" FILESIZE="12345" RELEASE_DATE="2019/5/17" PLAYTIME="300"></INFO><MUSICAL_KEY VALUE="0"></MUSICAL_KEY><TEMPO BPM="128.000000"></TEMPO>
<CUE_V2 NAME="AutoGrid" TYPE="4" START="1000.000000" LEN="0.000000" REPEATS="-1" HOTCUE="-1"><GRID BPM="128.000000"></GRID></CUE_V2>
<CUE_V2 NAME="Hot" TYPE="0" START="2000.000000" LEN="0.000000" REPEATS="-1" HOTCUE="0"></CUE_V2>
<CUE_V2 NAME="Memory" TYPE="0" START="3000.000000" LEN="0.000000" REPEATS="-1" HOTCUE="-1"></CUE_V2>
<CUE_V2 NAME="Loop" TYPE="5" START="4000.000000" LEN="2000.000000" REPEATS="-1" HOTCUE="-1"></CUE_V2>"#
    }

    fn write_nml(tmp: &tempfile::TempDir, contents: &str) -> PathBuf {
        let path = tmp.path().join("collection.nml");
        std::fs::write(&path, contents).expect("write nml");
        path
    }

    fn seed_track(conn: &Arc<Mutex<Connection>>, track: &crate::models::Track) {
        let guard = conn.lock().unwrap();
        test_utils::insert_sentinel_track(&guard, track);
    }

    #[test]
    fn ingest_creates_a_row_with_grid_key_and_cues() {
        let (service, _conn, tmp) = test_service();
        let wav = tmp.path().join("ingest.wav");
        write_wav(&wav, 1, 512);
        let nml_path = write_nml(&tmp, &nml(&[entry(&wav, "Ingest", full_body())]));

        let outcome = service.import(&nml_path).unwrap();

        assert_eq!(outcome.matched, 0);
        assert_eq!(outcome.ingested, 1);
        assert_eq!(outcome.cues_written, 3);
        assert_eq!(outcome.grids_written, 1);
        assert_eq!(outcome.keys_written, 1);
        assert_eq!(outcome.fields_written, 1);
        assert!(outcome.unmatched.is_empty());
        assert_eq!(outcome.dry_run.totals.tracks, 1);

        let tracks = service.library.get_tracks(None).unwrap();
        assert_eq!(tracks.len(), 1);
        let track = &tracks[0];
        assert_eq!(track.key.as_deref(), Some("8B"));
        assert_eq!(track.grid_bpm, Some(128.0));
        assert_eq!(track.grid_anchor_ms, Some(1000));
        assert_eq!(track.grid_source.as_deref(), Some("traktor"));
        // The mapped INFO fields reach their Crate columns (D11).
        assert_eq!(track.rating, 4, "RANKING=204 -> 4 stars");
        assert_eq!(track.play_count, 3);
        assert_eq!(track.bitrate, Some(320), "320000 bps -> 320 kbps");
        assert_eq!(track.genre.as_deref(), Some("House"));
        assert_eq!(track.label.as_deref(), Some("One Records"));
        assert_eq!(track.date_added, "2026-08-23T00:00:00+00:00");
        assert_eq!(
            track.last_played.as_deref(),
            Some("2026-08-24T00:00:00+00:00")
        );
        assert_eq!(
            track.bpm, None,
            "the grid must not be written into tracks.bpm"
        );

        let cues = service.cues.get_cues_for_track(&track.id).unwrap();
        assert_eq!(cues.len(), 3);
        assert!(cues
            .iter()
            .any(|cue| cue.cue_type == CueType::Hot && cue.hot_cue_index == Some(0)));
        assert!(cues.iter().any(|cue| cue.cue_type == CueType::Memory));
        assert!(cues
            .iter()
            .any(|cue| cue.cue_type == CueType::Loop && cue.loop_end_ms == Some(6000)));
    }

    #[test]
    fn existing_track_is_matched_not_duplicated_and_receives_traktor_data() {
        let (service, conn, tmp) = test_service();
        let wav = tmp.path().join("matched.wav");
        write_wav(&wav, 2, 512);
        let hash = compute_audio_hash(&wav).unwrap();

        // A track already in Crate with the same content hash but a stale key.
        let mut seeded = test_utils::fixture_track("existing-track", "Existing", &hash);
        seeded.key = Some("Am".to_string());
        seed_track(&conn, &seeded);

        let nml_path = write_nml(&tmp, &nml(&[entry(&wav, "Matched", full_body())]));
        let outcome = service.import(&nml_path).unwrap();

        assert_eq!(outcome.matched, 1);
        assert_eq!(outcome.ingested, 0);
        assert_eq!(outcome.cues_written, 3);
        assert_eq!(outcome.unmatched.len(), 0);

        let tracks = service.library.get_tracks(None).unwrap();
        assert_eq!(tracks.len(), 1, "matching must not create a duplicate");
        assert_eq!(tracks[0].id, "existing-track");
        // Traktor wins over whatever Crate read from the file.
        assert_eq!(tracks[0].key.as_deref(), Some("8B"));
        assert_eq!(tracks[0].grid_source.as_deref(), Some("traktor"));

        assert_eq!(
            service
                .cues
                .get_cues_for_track("existing-track")
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn missing_file_is_reported_and_creates_no_row() {
        let (service, _conn, tmp) = test_service();
        let missing = tmp.path().join("does-not-exist.wav");
        let nml_path = write_nml(&tmp, &nml(&[entry(&missing, "Missing", full_body())]));

        let outcome = service.import(&nml_path).unwrap();

        assert_eq!(outcome.matched, 0);
        assert_eq!(outcome.ingested, 0);
        assert_eq!(outcome.unmatched.len(), 1);
        assert_eq!(outcome.unmatched[0].reason, "file-not-found");
        assert_eq!(outcome.unmatched[0].file_path, missing.to_string_lossy());
        assert_eq!(outcome.unmatched[0].title.as_deref(), Some("Missing"));
        assert!(service.library.get_tracks(None).unwrap().is_empty());
    }

    #[test]
    fn importing_the_same_nml_twice_is_idempotent() {
        let (service, _conn, tmp) = test_service();
        let wav = tmp.path().join("twice.wav");
        write_wav(&wav, 3, 512);
        let nml_path = write_nml(&tmp, &nml(&[entry(&wav, "Twice", full_body())]));

        let first = service.import(&nml_path).unwrap();
        assert_eq!(first.ingested, 1);
        let id = service.library.get_tracks(None).unwrap()[0].id.clone();

        let second = service.import(&nml_path).unwrap();

        assert_eq!(second.matched, 1);
        assert_eq!(second.ingested, 0);
        assert_eq!(second.cues_written, first.cues_written);
        assert_eq!(second.grids_written, first.grids_written);
        assert_eq!(second.keys_written, first.keys_written);
        assert_eq!(second.fields_written, first.fields_written);
        assert!(second.unmatched.is_empty());

        let tracks = service.library.get_tracks(None).unwrap();
        assert_eq!(tracks.len(), 1, "no duplicate track");
        assert_eq!(tracks[0].id, id, "the persisted id is stable");
        assert_eq!(
            service.cues.get_cues_for_track(&id).unwrap().len(),
            3,
            "no duplicate cues"
        );
    }

    #[test]
    fn path_match_enriches_a_row_with_a_stale_hash() {
        let (service, conn, tmp) = test_service();
        let wav = tmp.path().join("conflict.wav");
        write_wav(&wav, 4, 512);

        // A row already owns this file_path but with a *stale* hash: the path match must
        // win and enrich it, not report it. It also carries a grid and a cue from a
        // previous import.
        let mut seeded = test_utils::fixture_track("persisted-row", "Persisted", "stale-hash");
        seeded.file_path = wav.to_string_lossy().to_string();
        seeded.key = Some("Am".to_string());
        seeded.grid_bpm = Some(111.0);
        seeded.grid_anchor_ms = Some(222);
        seeded.grid_source = Some("traktor".to_string());
        seed_track(&conn, &seeded);
        service
            .cues
            .replace_cues_for_track(
                "persisted-row",
                vec![Cue::new_hot("persisted-row".to_string(), 500, 1)],
            )
            .unwrap();

        let nml_path = write_nml(&tmp, &nml(&[entry(&wav, "Conflict", full_body())]));
        let outcome = service.import(&nml_path).unwrap();

        // The row is matched by path even though its stored hash is stale.
        assert_eq!(outcome.matched, 1);
        assert_eq!(outcome.ingested, 0);
        assert!(outcome.unmatched.is_empty());
        assert_eq!(outcome.keys_written, 1);
        assert_eq!(outcome.grids_written, 1);
        assert_eq!(outcome.cues_written, 3);

        let tracks = service.library.get_tracks(None).unwrap();
        assert_eq!(
            tracks.len(),
            1,
            "the path match must not create a second row"
        );
        assert_eq!(tracks[0].id, "persisted-row");
        assert_eq!(tracks[0].key.as_deref(), Some("8B"));
        assert_eq!(tracks[0].grid_bpm, Some(128.0));
        assert_eq!(tracks[0].grid_anchor_ms, Some(1000));
        assert_eq!(tracks[0].grid_source.as_deref(), Some("traktor"));
        assert_eq!(
            service
                .cues
                .get_cues_for_track("persisted-row")
                .unwrap()
                .len(),
            3,
            "the mapped cues replace the stale list"
        );
    }

    #[test]
    fn same_content_under_a_different_path_is_matched_by_hash() {
        let (service, conn, tmp) = test_service();
        let wav = tmp.path().join("cross-path.wav");
        write_wav(&wav, 5, 512);
        let hash = compute_audio_hash(&wav).unwrap();

        // The same audio already lives in Crate under a different path (it moved, or was
        // imported through another folder): the hash match must enrich it in place.
        let mut seeded = test_utils::fixture_track("hash-match", "Hash Match", &hash);
        seeded.key = Some("Am".to_string());
        seed_track(&conn, &seeded);

        let nml_path = write_nml(&tmp, &nml(&[entry(&wav, "Cross Path", full_body())]));
        let outcome = service.import(&nml_path).unwrap();

        assert_eq!(outcome.matched, 1);
        assert_eq!(outcome.ingested, 0);
        assert!(outcome.unmatched.is_empty());

        let tracks = service.library.get_tracks(None).unwrap();
        assert_eq!(
            tracks.len(),
            1,
            "hash matching must not create a second row"
        );
        assert_eq!(tracks[0].id, "hash-match");
        assert_eq!(tracks[0].key.as_deref(), Some("8B"));
        assert_eq!(tracks[0].grid_source.as_deref(), Some("traktor"));
        assert_eq!(tracks[0].grid_anchor_ms, Some(1000));
        assert_eq!(
            service.cues.get_cues_for_track("hash-match").unwrap().len(),
            3
        );
    }

    /// The Crate id a file path resolves to after the import ingested it.
    fn track_id(service: &TraktorImportService, path: &Path) -> String {
        service
            .library
            .find_track_by_file_path(&path.to_string_lossy())
            .unwrap()
            .unwrap_or_else(|| panic!("no track row for {}", path.display()))
            .id
    }

    #[test]
    fn folder_child_playlist_keeps_its_real_parent() {
        let (service, _conn, tmp) = test_service();
        let wav = tmp.path().join("parented.wav");
        write_wav(&wav, 11, 512);

        let playlists = render_playlists(&[folder(
            "Crates",
            vec![playlist("Night", vec![primary_key(&wav)])],
        )]);
        let nml_path = write_nml(
            &tmp,
            &nml_with_playlists(&[entry(&wav, "Parented", "")], &playlists),
        );

        let outcome = service.import(&nml_path).unwrap();
        assert_eq!(outcome.playlists_imported, 2, "folder + playlist");
        assert_eq!(outcome.playlist_tracks_written, 1);
        assert!(outcome.skipped_internal_playlists.is_empty());

        let rows = service.playlists.get_playlists("library").unwrap();
        let folder_row = rows
            .iter()
            .find(|p| p.name == "Crates")
            .expect("folder row");
        assert!(folder_row.is_folder);
        assert_eq!(folder_row.parent_id, None, "the folder stays at the root");

        let children = service.playlists.get_children(&folder_row.id).unwrap();
        assert_eq!(children.len(), 1, "the playlist must not be flattened");
        let night = &children[0];
        assert_eq!(night.name, "Night");
        assert_eq!(
            night.parent_id.as_deref(),
            Some(folder_row.id.as_str()),
            "the child playlist keeps the real folder as its parent"
        );
        assert!(!night.is_smart);
        assert_eq!(
            service
                .playlists
                .get_playlist_tracks(&night.id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn internal_playlists_are_skipped_with_their_subtree() {
        let (service, conn, tmp) = test_service();
        let playlists = render_playlists(&[
            folder("_LOOPS", vec![playlist("Loops Child", Vec::new())]),
            playlist("_RECORDINGS", Vec::new()),
            playlist("Keep", Vec::new()),
        ]);
        let nml_path = write_nml(&tmp, &nml_with_playlists(&[], &playlists));

        let outcome = service.import(&nml_path).unwrap();

        assert_eq!(outcome.playlists_imported, 1, "only the non-internal node");
        assert_eq!(
            outcome.skipped_internal_playlists,
            vec!["_LOOPS".to_string(), "_RECORDINGS".to_string()]
        );

        let skipped_rows: i64 = {
            let guard = conn.lock().unwrap();
            guard
                .query_row(
                    "SELECT COUNT(*) FROM playlists \
                     WHERE name IN ('_LOOPS', '_RECORDINGS', 'Loops Child')",
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(
            skipped_rows, 0,
            "skipped nodes and their subtree create no row"
        );

        let rows = service.playlists.get_playlists("library").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Keep");
    }

    #[test]
    fn smartlist_is_imported_as_a_static_playlist() {
        let (service, _conn, tmp) = test_service();
        let wav = tmp.path().join("smart.wav");
        write_wav(&wav, 21, 512);

        let playlists = render_playlists(&[smartlist(
            "Top rated",
            "$RATING == 5",
            vec![primary_key(&wav)],
        )]);
        let nml_path = write_nml(
            &tmp,
            &nml_with_playlists(&[entry(&wav, "Smart", "")], &playlists),
        );

        let outcome = service.import(&nml_path).unwrap();
        assert_eq!(outcome.playlists_imported, 1);
        assert_eq!(outcome.playlist_tracks_written, 1);

        let rows = service.playlists.get_playlists("library").unwrap();
        let smart = rows
            .iter()
            .find(|p| p.name == "Top rated")
            .expect("smartlist row");
        assert!(
            !smart.is_smart,
            "a Traktor smartlist must be imported as an ordinary static playlist"
        );
        assert!(smart.smart_rules.is_none());
        assert!(!smart.is_folder);
        assert_eq!(
            service
                .playlists
                .get_playlist_tracks(&smart.id)
                .unwrap()
                .len(),
            1,
            "the smartlist's current entries become static membership"
        );
    }

    #[test]
    fn playlist_membership_uses_the_right_ids_in_entry_order() {
        let (service, conn, tmp) = test_service();
        let a = tmp.path().join("a.wav");
        let b = tmp.path().join("b.wav");
        let c = tmp.path().join("c.wav");
        write_wav(&a, 31, 512);
        write_wav(&b, 32, 512);
        write_wav(&c, 33, 512);

        // Collection order is a, b, c; the playlist lists c, a, b.
        let playlists = render_playlists(&[playlist(
            "Ordered",
            vec![primary_key(&c), primary_key(&a), primary_key(&b)],
        )]);
        let nml_path = write_nml(
            &tmp,
            &nml_with_playlists(
                &[entry(&a, "A", ""), entry(&b, "B", ""), entry(&c, "C", "")],
                &playlists,
            ),
        );

        service.import(&nml_path).unwrap();

        let (id_a, id_b, id_c) = (
            track_id(&service, &a),
            track_id(&service, &b),
            track_id(&service, &c),
        );
        let rows = service.playlists.get_playlists("library").unwrap();
        let ordered = rows.iter().find(|p| p.name == "Ordered").unwrap();
        let members: Vec<(String, i32)> = {
            let guard = conn.lock().unwrap();
            let mut stmt = guard
                .prepare(
                    "SELECT track_id, position FROM playlist_tracks \
                     WHERE playlist_id = ?1 ORDER BY position",
                )
                .unwrap();
            stmt.query_map([&ordered.id], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            members,
            vec![(id_c, 0), (id_a, 1), (id_b, 2)],
            "membership follows the NML entry order, not the collection order"
        );
    }

    #[test]
    fn unresolved_playlist_entries_are_counted_and_omitted() {
        let (service, _conn, tmp) = test_service();
        let present = tmp.path().join("present.wav");
        write_wav(&present, 41, 512);
        let missing = tmp.path().join("absent.wav");

        let playlists = render_playlists(&[playlist(
            "Mixed",
            vec![primary_key(&present), primary_key(&missing)],
        )]);
        let nml_path = write_nml(
            &tmp,
            &nml_with_playlists(&[entry(&present, "Present", "")], &playlists),
        );

        let outcome = service.import(&nml_path).unwrap();
        assert_eq!(outcome.unresolved_playlist_entries, 1);
        assert_eq!(outcome.playlist_tracks_written, 1);

        let rows = service.playlists.get_playlists("library").unwrap();
        let mixed = rows.iter().find(|p| p.name == "Mixed").unwrap();
        let members = service.playlists.get_playlist_tracks(&mixed.id).unwrap();
        assert_eq!(members.len(), 1, "the unresolved entry is omitted");
        assert_eq!(members[0].file_path, present.to_string_lossy());
    }

    #[test]
    fn reimporting_playlists_is_idempotent() {
        let (service, conn, tmp) = test_service();
        let one = tmp.path().join("one.wav");
        let two = tmp.path().join("two.wav");
        write_wav(&one, 51, 512);
        write_wav(&two, 52, 512);
        let entries = [entry(&one, "One", ""), entry(&two, "Two", "")];

        let full = render_playlists(&[folder(
            "Crates",
            vec![playlist(
                "Night",
                vec![primary_key(&one), primary_key(&two)],
            )],
        )]);
        let nml_path = write_nml(&tmp, &nml_with_playlists(&entries, &full));
        let first = service.import(&nml_path).unwrap();
        assert_eq!(first.playlists_imported, 2);
        assert_eq!(first.playlist_tracks_written, 2);

        // A second run with a shrunk member list must replace, never append.
        let shrunk = render_playlists(&[folder(
            "Crates",
            vec![playlist("Night", vec![primary_key(&one)])],
        )]);
        let nml_path = write_nml(&tmp, &nml_with_playlists(&entries, &shrunk));
        let second = service.import(&nml_path).unwrap();

        assert_eq!(second.playlists_imported, first.playlists_imported);
        assert_eq!(second.playlist_tracks_written, 1);

        let rows = service.playlists.get_playlists("library").unwrap();
        assert_eq!(rows.len(), 2, "no duplicate playlists");
        let folder_row = rows.iter().find(|p| p.name == "Crates").unwrap();
        let (by_name, by_name_parent): (i64, i64) = {
            let guard = conn.lock().unwrap();
            let by_name: i64 = guard
                .query_row(
                    "SELECT COUNT(*) FROM playlists WHERE name = 'Night' AND context = 'library'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let by_name_parent: i64 = guard
                .query_row(
                    "SELECT COUNT(*) FROM playlists WHERE name = 'Night' AND parent_id = ?1",
                    [&folder_row.id],
                    |row| row.get(0),
                )
                .unwrap();
            (by_name, by_name_parent)
        };
        assert_eq!(by_name, 1, "no duplicate playlist by name");
        assert_eq!(by_name_parent, 1, "no duplicate playlist by name+parent");

        let children = service.playlists.get_children(&folder_row.id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(
            service
                .playlists
                .get_playlist_tracks(&children[0].id)
                .unwrap()
                .len(),
            1,
            "membership is replaced, not appended"
        );
    }

    #[test]
    fn a_playlist_with_zero_entries_does_not_clear_existing_membership() {
        let (service, _conn, tmp) = test_service();
        let wav = tmp.path().join("kept.wav");
        write_wav(&wav, 61, 512);
        let entries = [entry(&wav, "Kept", "")];

        let seeded = render_playlists(&[playlist("Night", vec![primary_key(&wav)])]);
        let nml_path = write_nml(&tmp, &nml_with_playlists(&entries, &seeded));
        service.import(&nml_path).unwrap();

        let rows = service.playlists.get_playlists("library").unwrap();
        let night = rows.iter().find(|p| p.name == "Night").unwrap().id.clone();
        assert_eq!(
            service.playlists.get_playlist_tracks(&night).unwrap().len(),
            1
        );

        // Same node name, zero entries: the existing membership must survive.
        let empty = render_playlists(&[playlist("Night", Vec::new())]);
        let nml_path = write_nml(&tmp, &nml_with_playlists(&entries, &empty));
        let outcome = service.import(&nml_path).unwrap();

        assert_eq!(outcome.playlist_tracks_written, 0);
        assert_eq!(
            service.playlists.get_playlist_tracks(&night).unwrap().len(),
            1,
            "an empty NML node is not an assertion that the playlist is empty"
        );
    }
}
