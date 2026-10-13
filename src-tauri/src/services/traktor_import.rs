//! Real (DB-aware) Traktor `collection.nml` import — S4, slice A.
//!
//! This is the write side of the feature: it consumes the hermetic parser + mapper
//! (`services::traktor`, which stays ungated) and persists the plan — tracks, cues,
//! beat grids and keys — into the `tracks`/`cues` tables. It performs **no** Tauri calls
//! and no `async` work; every lock scope is synchronous.
//!
//! The module is `desktop`-only because it reuses the desktop library import machinery
//! (`LibraryService`, which reads tags/hashes a file) plus `compute_audio_hash`.
#![allow(dead_code)] // Slice B wires the Tauri commands; until then only tests consume this surface.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{CrateError, Result};
use crate::models::{Cue, CueType};
use crate::services::cloud_sync::pipeline::{buckets, dirty};
use crate::services::hash::compute_audio_hash;
use crate::services::library::LibraryService;
use crate::services::traktor::mapper::{self, ImportPlan, ImportReport, MappedCue, MappedTrack};
use crate::services::traktor::parser;
use crate::services::CueService;

/// Origin stamped on every beat grid this importer writes.
const GRID_SOURCE_TRAKTOR: &str = "traktor";

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
    /// Entries that were reported and skipped without creating a row.
    pub unmatched: Vec<UnmatchedTraktorTrack>,
}

/// Persists a mapped Traktor plan into the library.
pub struct TraktorImportService {
    conn: Arc<Mutex<Connection>>,
    library: LibraryService,
    cues: CueService,
}

impl TraktorImportService {
    pub fn new(conn: Arc<Mutex<Connection>>, library: LibraryService) -> Self {
        Self {
            cues: CueService::new(conn.clone()),
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
    /// content hash instead of inserting duplicates, and rewrites the same grid/cues.
    pub fn import(&self, nml_path: &Path) -> Result<TraktorImportOutcome> {
        let plan = self.preview(nml_path)?;
        let ImportPlan { tracks, report, .. } = plan;

        let mut outcome = TraktorImportOutcome {
            dry_run: report,
            matched: 0,
            ingested: 0,
            cues_written: 0,
            grids_written: 0,
            keys_written: 0,
            unmatched: Vec::new(),
        };

        for track in &tracks {
            let Some(id) = self.resolve_track_id(track, &mut outcome)? else {
                continue;
            };

            let (key_written, grid_written) = self.apply_key_and_grid(&id, track)?;
            if key_written {
                outcome.keys_written += 1;
            }
            if grid_written {
                outcome.grids_written += 1;
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

        Ok(outcome)
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

    /// Write the Traktor key and/or beat grid onto one resolved track row in a single
    /// mutation, mirroring `insert_track_in`'s `_hlc` stamp and dirty mark.
    ///
    /// Returns `(key_written, grid_written)`. A partial triple is never written: the grid
    /// columns are all written together or not at all.
    fn apply_key_and_grid(&self, id: &str, track: &MappedTrack) -> Result<(bool, bool)> {
        let grid = grid_triple(track);
        let has_key = track.key.is_some();
        if !has_key && grid.is_none() {
            return Ok((false, false));
        }

        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let tx = conn.unchecked_transaction()?;
        let hlc = dirty::next_hlc(&tx)?;

        let mut set: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(key) = &track.key {
            params.push(Box::new(key.clone()));
            set.push(format!("key = ?{}", params.len()));
        }
        if let Some((grid_bpm, grid_anchor_ms, grid_source)) = grid {
            params.push(Box::new(grid_bpm));
            set.push(format!("grid_bpm = ?{}", params.len()));
            params.push(Box::new(grid_anchor_ms));
            set.push(format!("grid_anchor_ms = ?{}", params.len()));
            params.push(Box::new(grid_source));
            set.push(format!("grid_source = ?{}", params.len()));
        }
        params.push(Box::new(hlc));
        set.push(format!("_hlc = ?{}", params.len()));
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

        Ok((has_key, grid.is_some()))
    }
}

/// The mapper's grid is a pair, and Crate stores it as a `(bpm, anchor, source)` triple.
/// Both fields must be present; a partial grid is never written.
fn grid_triple(track: &MappedTrack) -> Option<(f64, i64, &'static str)> {
    match (track.grid_bpm, track.grid_position_ms) {
        (Some(bpm), Some(anchor_ms)) => Some((bpm, anchor_ms, GRID_SOURCE_TRAKTOR)),
        _ => None,
    }
}

/// Convert one mapped Traktor cue into a Crate [`Cue`].
///
/// The mapper already guarantees the type-specific fields (a `Hot` cue carries a 1-based
/// slot, a `Loop` carries an end after its start). The `?` guards are defensive: a future
/// mapper regression drops one cue instead of aborting the whole import inside
/// `replace_cues_for_track`'s validation or inventing a value.
fn mapped_cue_to_model(track_id: &str, mapped: &MappedCue) -> Option<Cue> {
    let mut cue = match mapped.cue_type {
        CueType::Memory => Cue::new_memory(track_id.to_string(), mapped.position_ms),
        CueType::Hot => Cue::new_hot(
            track_id.to_string(),
            mapped.position_ms,
            mapped.hot_cue_index.filter(|i| (1..=8).contains(i))?,
        ),
        CueType::Loop => Cue::new_loop(
            track_id.to_string(),
            mapped.position_ms,
            mapped.loop_end_ms.filter(|end| *end > mapped.position_ms)?,
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

    /// The XML body used by most tests: a Camelot key, an analyzed grid, one hot cue, one
    /// memory cue and one loop.
    fn full_body() -> &'static str {
        r#"<INFO KEY="8B"></INFO><MUSICAL_KEY VALUE="0"></MUSICAL_KEY><TEMPO BPM="128.000000"></TEMPO>
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
        assert!(outcome.unmatched.is_empty());
        assert_eq!(outcome.dry_run.totals.tracks, 1);

        let tracks = service.library.get_tracks(None).unwrap();
        assert_eq!(tracks.len(), 1);
        let track = &tracks[0];
        assert_eq!(track.key.as_deref(), Some("8B"));
        assert_eq!(track.grid_bpm, Some(128.0));
        assert_eq!(track.grid_anchor_ms, Some(1000));
        assert_eq!(track.grid_source.as_deref(), Some("traktor"));
        assert_eq!(
            track.bpm, None,
            "the grid must not be written into tracks.bpm"
        );

        let cues = service.cues.get_cues_for_track(&track.id).unwrap();
        assert_eq!(cues.len(), 3);
        assert!(cues
            .iter()
            .any(|cue| cue.cue_type == CueType::Hot && cue.hot_cue_index == Some(1)));
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
}
