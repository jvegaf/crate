//! Cue points persistence (Traktor import S1).
//!
//! The first write path for the `cues` table. Cues are written as a whole list per
//! track (what an importer needs), validated against the schema's CHECK constraints,
//! and de-duplicated by identity before they reach the database.
//!
//! Every mutation stamps `_hlc` through [`dirty::next_hlc`] and marks the `cues` sync
//! bucket dirty, so edits converge through cloud sync. Dropped rows get tombstones.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{CrateError, Result};
use crate::models::{Cue, CueType};
use crate::services::cloud_sync::pipeline::{buckets, dirty};

/// Cue points CRUD over a single SQLCipher connection.
pub struct CueService {
    conn: Arc<Mutex<Connection>>,
}

impl CueService {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// All cues of `track_id`, ordered deterministically.
    pub fn get_cues_for_track(&self, track_id: &str) -> Result<Vec<Cue>> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT id, track_id, position_ms, type, loop_end_ms, hot_cue_index, name, color \
             FROM cues WHERE track_id = ?1 \
             ORDER BY position_ms ASC, type ASC, hot_cue_index ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([track_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i32>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut cues = Vec::with_capacity(rows.len());
        for (id, track_id, position_ms, type_str, loop_end_ms, hot_cue_index, name, color) in rows {
            let cue_type = type_str
                .parse::<CueType>()
                .map_err(CrateError::InvalidOperation)?;
            cues.push(Cue {
                id,
                track_id,
                position_ms,
                cue_type,
                loop_end_ms,
                hot_cue_index,
                name,
                color,
            });
        }
        Ok(cues)
    }

    /// Replace the entire cue list of `track_id` with `cues`, returning what was stored.
    ///
    /// Validates every cue before touching the database, collapses duplicate identities,
    /// then swaps the whole list inside one transaction.
    pub fn replace_cues_for_track(&self, track_id: &str, cues: Vec<Cue>) -> Result<Vec<Cue>> {
        for cue in &cues {
            validate_cue(cue)?;
        }
        let deduped = dedup_cues(cues);

        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let tx = conn.unchecked_transaction()?;
        // One HLC for the whole logical mutation (deletes + inserts happen as one event).
        let hlc = dirty::next_hlc(&tx)?;

        let existing = existing_cue_ids(&tx, track_id)?;
        // Tombstone ONLY the ids the replacement drops. Re-inserting an id that is also
        // tombstoned at the same HLC would lose to the entity tie-break (`tombstone >=
        // live`) and vanish from the sync bucket; a retained id is an update, not a
        // delete.
        let kept: std::collections::HashSet<&str> =
            deduped.iter().map(|cue| cue.id.as_str()).collect();
        for id in &existing {
            if !kept.contains(id.as_str()) {
                dirty::record_tombstone(&tx, buckets::CUES, id, &hlc)?;
            }
        }
        tx.execute("DELETE FROM cues WHERE track_id = ?1", [track_id])?;

        for cue in &deduped {
            let cue_type = cue.cue_type.to_string();
            tx.execute(
                "INSERT INTO cues \
                     (id, track_id, position_ms, type, loop_end_ms, hot_cue_index, name, color, _hlc) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    cue.id,
                    track_id,
                    cue.position_ms,
                    cue_type,
                    cue.loop_end_ms,
                    cue.hot_cue_index,
                    cue.name,
                    cue.color,
                    hlc,
                ],
            )?;
        }
        dirty::mark_dirty(&tx, buckets::CUES)?;
        tx.commit()?;

        // The read helper locks again, so the guard must be released first.
        drop(conn);
        self.get_cues_for_track(track_id)
    }

    /// Delete every cue of `track_id`.
    pub fn delete_cues_for_track(&self, track_id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let tx = conn.unchecked_transaction()?;
        let hlc = dirty::next_hlc(&tx)?;

        let existing = existing_cue_ids(&tx, track_id)?;
        if !existing.is_empty() {
            for id in &existing {
                dirty::record_tombstone(&tx, buckets::CUES, id, &hlc)?;
            }
            tx.execute("DELETE FROM cues WHERE track_id = ?1", [track_id])?;
            dirty::mark_dirty(&tx, buckets::CUES)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete a single cue by id. Idempotent: a missing id is not an error.
    pub fn delete_cue(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let tx = conn.unchecked_transaction()?;
        let hlc = dirty::next_hlc(&tx)?;

        let deleted = tx.execute("DELETE FROM cues WHERE id = ?1", [id])?;
        if deleted > 0 {
            dirty::record_tombstone(&tx, buckets::CUES, id, &hlc)?;
            dirty::mark_dirty(&tx, buckets::CUES)?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// Ids of every cue belonging to `track_id` (used to tombstone rows before a delete).
fn existing_cue_ids(conn: &Connection, track_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM cues WHERE track_id = ?1")?;
    let rows = stmt.query_map([track_id], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Reject cues the `cues` table CHECK constraints would reject, with a clearer message.
fn validate_cue(cue: &Cue) -> Result<()> {
    if let Some(index) = cue.hot_cue_index {
        // 0-based, matching Traktor's `HOTCUE` and Pioneer's `hot_cue` byte.
        if !(0..=7).contains(&index) {
            return Err(CrateError::InvalidOperation(format!(
                "hot_cue_index must be within 0..=7, got {index}"
            )));
        }
    }
    match cue.cue_type {
        CueType::Loop => {
            let end = cue.loop_end_ms.ok_or_else(|| {
                CrateError::InvalidOperation("loop cue requires loop_end_ms".to_string())
            })?;
            if end <= cue.position_ms {
                return Err(CrateError::InvalidOperation(format!(
                    "loop cue loop_end_ms ({end}) must be greater than position_ms ({})",
                    cue.position_ms
                )));
            }
        }
        CueType::Hot => {
            if cue.hot_cue_index.is_none() {
                return Err(CrateError::InvalidOperation(
                    "hot cue requires hot_cue_index".to_string(),
                ));
            }
        }
        CueType::Memory => {
            if cue.loop_end_ms.is_some() {
                return Err(CrateError::InvalidOperation(
                    "memory cue must not carry loop_end_ms".to_string(),
                ));
            }
            if cue.hot_cue_index.is_some() {
                return Err(CrateError::InvalidOperation(
                    "memory cue must not carry hot_cue_index".to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// Collapse cues sharing the identity `(position_ms, type, hot_cue_index)`, keeping the
/// first occurrence.
///
/// `INSERT OR REPLACE` cannot do this: every cue carries its own UUID `id`, so two rows
/// with the same identity but different ids coexist unless collapsed explicitly.
fn dedup_cues(cues: Vec<Cue>) -> Vec<Cue> {
    let mut seen = std::collections::HashSet::new();
    cues.into_iter()
        .filter(|cue| seen.insert((cue.position_ms, cue.cue_type.to_string(), cue.hot_cue_index)))
        .collect()
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../test_utils.rs"]
mod test_utils;

#[cfg(test)]
mod tests {
    use super::*;

    /// A service over an in-memory DB that already owns `track_id` (the FK target).
    fn service_with_track(track_id: &str) -> CueService {
        service_and_conn(track_id).0
    }

    /// The service plus a handle to the same connection, for asserting stored columns.
    fn service_and_conn(track_id: &str) -> (CueService, Arc<Mutex<Connection>>) {
        let conn = test_utils::make_memory_db();
        let track = test_utils::fixture_track(track_id, "Cue track", "hash");
        test_utils::insert_sentinel_track(&conn, &track);
        let shared = Arc::new(Mutex::new(conn));
        (CueService::new(shared.clone()), shared)
    }

    fn memory(track_id: &str, position_ms: i64) -> Cue {
        Cue::new_memory(track_id.to_string(), position_ms)
    }

    fn hot(track_id: &str, position_ms: i64, index: i32) -> Cue {
        Cue::new_hot(track_id.to_string(), position_ms, index)
    }

    fn loop_cue(track_id: &str, position_ms: i64, end_ms: i64) -> Cue {
        Cue::new_loop(track_id.to_string(), position_ms, end_ms)
    }

    #[test]
    fn list_orders_by_position_then_identity() {
        let service = service_with_track("track-1");
        service
            .replace_cues_for_track(
                "track-1",
                vec![
                    hot("track-1", 2_000, 2),
                    hot("track-1", 1_000, 1),
                    loop_cue("track-1", 3_000, 4_000),
                ],
            )
            .unwrap();

        let cues = service.get_cues_for_track("track-1").unwrap();
        let positions: Vec<i64> = cues.iter().map(|c| c.position_ms).collect();
        assert_eq!(positions, vec![1_000, 2_000, 3_000]);
    }

    #[test]
    fn replace_replaces_the_whole_list() {
        let service = service_with_track("track-1");
        service
            .replace_cues_for_track(
                "track-1",
                vec![hot("track-1", 1_000, 1), memory("track-1", 2_000)],
            )
            .unwrap();

        let stored = service
            .replace_cues_for_track("track-1", vec![memory("track-1", 5_000)])
            .unwrap();

        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].position_ms, 5_000);
        assert_eq!(service.get_cues_for_track("track-1").unwrap().len(), 1);
    }

    #[test]
    fn replace_collapses_duplicate_identities() {
        let service = service_with_track("track-1");
        let mut first = hot("track-1", 1_000, 1);
        first.id = "dup-first".to_string();
        let mut second = hot("track-1", 1_000, 1);
        second.id = "dup-second".to_string();

        let stored = service
            .replace_cues_for_track("track-1", vec![first, second])
            .unwrap();

        assert_eq!(
            stored.len(),
            1,
            "duplicate identity must collapse to one row"
        );
        assert_eq!(stored[0].id, "dup-first");
        assert_eq!(service.get_cues_for_track("track-1").unwrap().len(), 1);
    }

    #[test]
    fn replace_does_not_tombstone_ids_it_keeps() {
        let (service, shared) = service_and_conn("track-1");
        let mut cue = hot("track-1", 1_000, 1);
        cue.id = "keep-me".to_string();
        service
            .replace_cues_for_track("track-1", vec![cue.clone()])
            .unwrap();

        // Re-saving the same id (an edit round-trip) must not tombstone it: an entity
        // tombstone ties at `>=` the live HLC and would hide the cue from cloud sync.
        service
            .replace_cues_for_track("track-1", vec![cue])
            .unwrap();

        let conn = shared.lock().unwrap();
        let tombstones: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync_tombstones \
                 WHERE entity_type = 'cues' AND entity_id = 'keep-me'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tombstones, 0);
        let live: i64 = conn
            .query_row("SELECT COUNT(*) FROM cues WHERE id = 'keep-me'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(live, 1);
    }

    #[test]
    fn replace_tombstones_only_dropped_ids() {
        let (service, shared) = service_and_conn("track-1");
        let mut keep = hot("track-1", 1_000, 1);
        keep.id = "keep".to_string();
        let mut drop = hot("track-1", 2_000, 2);
        drop.id = "drop".to_string();
        service
            .replace_cues_for_track("track-1", vec![keep.clone(), drop])
            .unwrap();

        service
            .replace_cues_for_track("track-1", vec![keep])
            .unwrap();

        let conn = shared.lock().unwrap();
        let tombstone_count = |entity_id: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM sync_tombstones \
                 WHERE entity_type = 'cues' AND entity_id = ?1",
                [entity_id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(tombstone_count("drop"), 1);
        assert_eq!(tombstone_count("keep"), 0);
    }

    #[test]
    fn delete_cues_for_track_removes_every_cue() {
        let service = service_with_track("track-1");
        service
            .replace_cues_for_track(
                "track-1",
                vec![
                    hot("track-1", 1_000, 1),
                    memory("track-1", 2_000),
                    loop_cue("track-1", 3_000, 4_000),
                ],
            )
            .unwrap();

        service.delete_cues_for_track("track-1").unwrap();

        assert!(service.get_cues_for_track("track-1").unwrap().is_empty());
    }

    #[test]
    fn delete_cue_removes_only_the_target() {
        let service = service_with_track("track-1");
        let mut first = hot("track-1", 1_000, 1);
        first.id = "cue-a".to_string();
        let mut second = hot("track-1", 2_000, 2);
        second.id = "cue-b".to_string();
        service
            .replace_cues_for_track("track-1", vec![first, second])
            .unwrap();

        service.delete_cue("cue-a").unwrap();

        let remaining = service.get_cues_for_track("track-1").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "cue-b");
    }

    #[test]
    fn loop_without_end_is_rejected() {
        let service = service_with_track("track-1");
        let mut cue = loop_cue("track-1", 1_000, 2_000);
        cue.loop_end_ms = None;

        let err = service
            .replace_cues_for_track("track-1", vec![cue])
            .unwrap_err();
        assert!(
            matches!(err, CrateError::InvalidOperation(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn loop_end_must_be_greater_than_position() {
        let service = service_with_track("track-1");
        for end_ms in [1_000, 999] {
            let cue = loop_cue("track-1", 1_000, end_ms);
            let err = service
                .replace_cues_for_track("track-1", vec![cue])
                .unwrap_err();
            assert!(
                matches!(err, CrateError::InvalidOperation(_)),
                "end {end_ms}: got {err:?}"
            );
        }
    }

    #[test]
    fn hot_without_index_is_rejected() {
        let service = service_with_track("track-1");
        let mut cue = hot("track-1", 1_000, 1);
        cue.hot_cue_index = None;

        let err = service
            .replace_cues_for_track("track-1", vec![cue])
            .unwrap_err();
        assert!(
            matches!(err, CrateError::InvalidOperation(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn memory_with_loop_end_is_rejected() {
        let service = service_with_track("track-1");
        let mut cue = memory("track-1", 1_000);
        cue.loop_end_ms = Some(2_000);

        let err = service
            .replace_cues_for_track("track-1", vec![cue])
            .unwrap_err();
        assert!(
            matches!(err, CrateError::InvalidOperation(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn memory_with_hot_index_is_rejected() {
        let service = service_with_track("track-1");
        let mut cue = memory("track-1", 1_000);
        cue.hot_cue_index = Some(1);

        let err = service
            .replace_cues_for_track("track-1", vec![cue])
            .unwrap_err();
        assert!(
            matches!(err, CrateError::InvalidOperation(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn hot_index_out_of_range_is_rejected() {
        let service = service_with_track("track-1");
        // Slots are 0-based: 0 and 7 are valid, -1 and 8 are not.
        for index in [-1, 8] {
            let cue = hot("track-1", 1_000, index);
            let err = service
                .replace_cues_for_track("track-1", vec![cue])
                .unwrap_err();
            assert!(
                matches!(err, CrateError::InvalidOperation(_)),
                "index {index}: got {err:?}"
            );
        }
    }

    #[test]
    fn distinct_types_at_the_same_position_are_kept() {
        // The real Traktor fixture stores an AutoGrid cue and a type-0 cue at the same
        // START; identity includes the type, so both must survive a write.
        let service = service_with_track("track-1");
        let stored = service
            .replace_cues_for_track(
                "track-1",
                vec![hot("track-1", 1_000, 1), memory("track-1", 1_000)],
            )
            .unwrap();
        assert_eq!(stored.len(), 2);
    }

    #[test]
    fn replace_stamps_a_real_hlc_on_every_row() {
        let (service, shared) = service_and_conn("track-1");
        service
            .replace_cues_for_track(
                "track-1",
                vec![hot("track-1", 1_000, 1), memory("track-1", 2_000)],
            )
            .unwrap();

        let conn = shared.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT _hlc FROM cues WHERE track_id = 'track-1'")
            .unwrap();
        let hlcs: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(hlcs.len(), 2);
        for hlc in hlcs {
            // The value must come from the sync clock, not be invented: a real,
            // well-formed HLC parses; the unstamped sentinel would fail.
            assert!(
                crate::services::cloud_sync::hlc::Hlc::parse(&hlc).is_ok(),
                "expected a real HLC, got {hlc:?}"
            );
        }
    }

    #[test]
    fn invalid_replacement_writes_nothing() {
        let service = service_with_track("track-1");
        service
            .replace_cues_for_track("track-1", vec![hot("track-1", 1_000, 1)])
            .unwrap();

        let bad = loop_cue("track-1", 5_000, 5_000); // end == position
        assert!(service
            .replace_cues_for_track("track-1", vec![bad])
            .is_err());

        // The previous list survives a rejected replacement untouched.
        assert_eq!(service.get_cues_for_track("track-1").unwrap().len(), 1);
    }
}
