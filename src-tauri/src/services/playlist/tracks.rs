use super::*;
use crate::services::cloud_sync::pipeline::{buckets, dirty};

impl PlaylistService {
    pub fn get_playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let mut stmt = conn.prepare(
            r#"
            SELECT
                t.id, t.file_path, t.file_hash,
                t.title, t.artist, t.album, t.year, t.genre, t.label, t.catalog_number,
                t.duration_ms, t.bpm, t.key, t.bitrate, t.sample_rate, t.format,
                t.analysis_source, t.waveform_data,
                t.rating, t.play_count,
                t.date_added, t.date_modified, t.last_played,
                t.rekordbox_id, t.artwork_path, t.artwork_source, t.color,
                t.library_root_id, t.relative_path, t.url,
                t.grid_bpm, t.grid_anchor_ms, t.grid_source
            FROM tracks t
            JOIN playlist_tracks pt ON t.id = pt.track_id
            WHERE pt.playlist_id = ?1
            ORDER BY pt.position
            "#,
        )?;

        let tracks = stmt
            .query_map([playlist_id], |row| {
                Ok(Track {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    file_hash: row.get(2)?,
                    title: row.get(3)?,
                    artist: row.get(4)?,
                    album: row.get(5)?,
                    year: row.get(6)?,
                    genre: row.get(7)?,
                    label: row.get(8)?,
                    catalog_number: row.get(9)?,
                    duration_ms: row.get(10)?,
                    bpm: row.get(11)?,
                    key: row.get(12)?,
                    bitrate: row.get(13)?,
                    sample_rate: row.get(14)?,
                    format: row.get(15)?,
                    analysis_source: row.get(16)?,
                    waveform_data: row.get(17)?,
                    rating: row.get(18)?,
                    play_count: row.get(19)?,
                    date_added: row.get(20)?,
                    date_modified: row.get(21)?,
                    last_played: row.get(22)?,
                    rekordbox_id: row.get(23)?,
                    artwork_path: row.get(24)?,
                    artwork_source: row.get(25)?,
                    color: row.get(26)?,
                    library_root_id: row.get(27)?,
                    relative_path: row.get(28)?,
                    url: row.get(29)?,
                    grid_bpm: row.get(30)?,
                    grid_anchor_ms: row.get(31)?,
                    grid_source: row.get(32)?,
                    tags: Vec::new(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Fetch tags
        let tracks_with_tags = self.fetch_tags_for_tracks(&conn, tracks)?;
        Ok(tracks_with_tags)
    }

    pub(crate) fn fetch_tags_for_tracks(
        &self,
        conn: &Connection,
        mut tracks: Vec<Track>,
    ) -> Result<Vec<Track>> {
        if tracks.is_empty() {
            return Ok(tracks);
        }

        let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        let placeholders: Vec<String> = track_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();

        let sql = format!(
            r#"
            SELECT tt.track_id, t.id, t.category_id, t.name, t.color, t.sort_order
            FROM track_tags tt
            JOIN tags t ON tt.tag_id = t.id
            WHERE tt.track_id IN ({})
            "#,
            placeholders.join(", ")
        );

        let params_refs: Vec<&dyn rusqlite::ToSql> = track_ids
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let mut stmt = conn.prepare(&sql)?;
        let tag_rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    Tag {
                        id: row.get(1)?,
                        category_id: row.get(2)?,
                        name: row.get(3)?,
                        color: row.get(4)?,
                        sort_order: row.get(5)?,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Group tags by track_id
        let mut tags_by_track: std::collections::HashMap<String, Vec<Tag>> =
            std::collections::HashMap::new();
        for (track_id, tag) in tag_rows {
            tags_by_track.entry(track_id).or_default().push(tag);
        }

        // Assign tags to tracks
        for track in &mut tracks {
            if let Some(tags) = tags_by_track.remove(&track.id) {
                track.tags = tags;
            }
        }

        Ok(tracks)
    }

    pub fn add_tracks(
        &self,
        playlist_id: &str,
        track_ids: Vec<String>,
    ) -> Result<AddToPlaylistResult> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        // Get current max position
        let max_position: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) FROM playlist_tracks WHERE playlist_id = ?1",
                [playlist_id],
                |row| row.get(0),
            )
            .unwrap_or(-1);

        let now = chrono::Utc::now().to_rfc3339();
        let hlc = dirty::next_hlc(&conn)?;

        // Positions advance only on a landed row so ignored duplicates leave no gaps.
        let mut added = 0usize;
        for track_id in &track_ids {
            let position = max_position + 1 + added as i32;
            added += conn.execute(
                "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position, date_added, _hlc) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![playlist_id, track_id, position, now, hlc],
            )?;
        }

        // A re-add of existing members changes nothing, so it must not bump the modified
        // date or trigger a cloud-sync push.
        if added > 0 {
            conn.execute(
                "UPDATE playlists SET date_modified = ?1, _hlc = ?2 WHERE id = ?3",
                rusqlite::params![now, hlc, playlist_id],
            )?;
            dirty::mark_dirty(&conn, buckets::PLAYLIST_TRACKS)?;
            dirty::mark_dirty(&conn, buckets::PLAYLISTS)?;
        }

        // Drop the lock before calling get_playlist which acquires its own lock
        drop(conn);

        Ok(AddToPlaylistResult {
            playlist: self.get_playlist(playlist_id)?,
            added,
            already_present: track_ids.len() - added,
        })
    }

    pub fn remove_tracks(&self, playlist_id: &str, track_ids: Vec<String>) -> Result<Playlist> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let hlc = dirty::next_hlc(&conn)?;
        for track_id in &track_ids {
            let deleted = conn.execute(
                "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                rusqlite::params![playlist_id, track_id],
            )?;
            if deleted > 0 {
                dirty::record_tombstone(
                    &conn,
                    buckets::PLAYLIST_TRACKS,
                    &dirty::junction_entity_id(playlist_id, track_id),
                    &hlc,
                )?;
            }
        }

        // Reorder remaining tracks
        let remaining_tracks: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )?;
            let tracks = stmt
                .query_map([playlist_id], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            tracks
        };

        for (i, track_id) in remaining_tracks.iter().enumerate() {
            conn.execute(
                "UPDATE playlist_tracks SET position = ?1, _hlc = ?2 WHERE playlist_id = ?3 AND track_id = ?4",
                rusqlite::params![i as i32, hlc, playlist_id, track_id],
            )?;
        }

        // Update playlist modified date
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE playlists SET date_modified = ?1, _hlc = ?2 WHERE id = ?3",
            rusqlite::params![now, hlc, playlist_id],
        )?;
        dirty::mark_dirty(&conn, buckets::PLAYLIST_TRACKS)?;
        dirty::mark_dirty(&conn, buckets::PLAYLISTS)?;

        // Drop the lock before calling get_playlist which acquires its own lock
        drop(conn);

        // Return the updated playlist with accurate track count
        self.get_playlist(playlist_id)
    }

    pub fn reorder_tracks(&self, playlist_id: &str, track_ids: Vec<String>) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let hlc = dirty::next_hlc(&conn)?;
        for (i, track_id) in track_ids.iter().enumerate() {
            conn.execute(
                "UPDATE playlist_tracks SET position = ?1, _hlc = ?2 WHERE playlist_id = ?3 AND track_id = ?4",
                rusqlite::params![i as i32, hlc, playlist_id, track_id],
            )?;
        }

        // Update playlist modified date
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE playlists SET date_modified = ?1, _hlc = ?2 WHERE id = ?3",
            rusqlite::params![now, hlc, playlist_id],
        )?;
        dirty::mark_dirty(&conn, buckets::PLAYLIST_TRACKS)?;
        dirty::mark_dirty(&conn, buckets::PLAYLISTS)?;

        Ok(())
    }

    /// Replace a playlist's exact ordered membership with `track_ids`.
    ///
    /// Positions are **0-based**, matching `add_tracks` (whose first member lands at
    /// `COALESCE(MAX(position), -1) + 1 == 0`, tracks.rs:147) and `reorder_tracks` (which writes
    /// `i as i32`, tracks.rs:247). Duplicate ids are collapsed to their first occurrence so the
    /// `(playlist_id, track_id)` junction key cannot collide; the return value is the number of
    /// members actually written (the deduplicated length).
    ///
    /// Every member absent from `track_ids` is deleted and tombstoned; every requested member is
    /// upserted at its index. An empty `track_ids` therefore clears the playlist. One HLC stamps
    /// the entire mutation. The operation is idempotent: a second call with an unchanged list
    /// deletes nothing, so it records no additional tombstones.
    pub fn replace_tracks(&self, playlist_id: &str, track_ids: Vec<String>) -> Result<usize> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let tx = conn.unchecked_transaction()?;

        // Deduplicate preserving first occurrence.
        let mut seen = std::collections::HashSet::new();
        let ordered: Vec<String> = track_ids
            .into_iter()
            .filter(|id| seen.insert(id.clone()))
            .collect();
        let requested: std::collections::HashSet<&str> =
            ordered.iter().map(String::as_str).collect();

        let hlc = dirty::next_hlc(&tx)?;

        // Delete members no longer requested, tombstoning each exactly like `remove_tracks`
        // (tracks.rs:197).
        let existing: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )?;
            let rows = stmt
                .query_map([playlist_id], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        for track_id in existing {
            if !requested.contains(track_id.as_str()) {
                tx.execute(
                    "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                    rusqlite::params![playlist_id, track_id],
                )?;
                dirty::record_tombstone(
                    &tx,
                    buckets::PLAYLIST_TRACKS,
                    &dirty::junction_entity_id(playlist_id, &track_id),
                    &hlc,
                )?;
            }
        }

        // Upsert every requested member at its index; an existing row keeps its `date_added`.
        let now = chrono::Utc::now().to_rfc3339();
        for (position, track_id) in ordered.iter().enumerate() {
            tx.execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position, date_added, _hlc) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(playlist_id, track_id) \
                 DO UPDATE SET position = excluded.position, _hlc = excluded._hlc",
                rusqlite::params![playlist_id, track_id, position as i32, now, hlc],
            )?;
        }

        // Mirror the dirty set used by `add_tracks` (tracks.rs:173-174) and `remove_tracks`
        // (tracks.rs:230-231).
        tx.execute(
            "UPDATE playlists SET date_modified = ?1, _hlc = ?2 WHERE id = ?3",
            rusqlite::params![now, hlc, playlist_id],
        )?;
        dirty::mark_dirty(&tx, buckets::PLAYLIST_TRACKS)?;
        dirty::mark_dirty(&tx, buckets::PLAYLISTS)?;

        tx.commit()?;

        Ok(ordered.len())
    }
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../../test_utils.rs"]
mod test_utils;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_playlist_tracks_maps_every_column_to_its_own_value() {
        let conn = test_utils::make_memory_db();
        let track = test_utils::sentinel_track();
        test_utils::insert_sentinel_track(&conn, &track);
        let service = PlaylistService::new(Arc::new(Mutex::new(conn)));

        let playlist = service
            .create_playlist("Sentinel PL".to_string(), None, "library".to_string())
            .unwrap();
        service
            .add_tracks(&playlist.id, vec![track.id.clone()])
            .unwrap();

        let tracks = service.get_playlist_tracks(&playlist.id).unwrap();
        assert_eq!(tracks.len(), 1);
        test_utils::assert_track_eq(&tracks[0], &track);
    }

    fn seeded_service() -> (Arc<Mutex<Connection>>, PlaylistService) {
        let conn = test_utils::make_memory_db();
        for track in test_utils::fixture_tracks() {
            test_utils::insert_sentinel_track(&conn, &track);
        }
        let shared = Arc::new(Mutex::new(conn));
        (shared.clone(), PlaylistService::new(shared))
    }

    /// `(track_id, position)` pairs ordered by position.
    fn membership(shared: &Arc<Mutex<Connection>>, playlist_id: &str) -> Vec<(String, i32)> {
        let conn = shared.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT track_id, position FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )
            .unwrap();
        stmt.query_map([playlist_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    fn has_tombstone(shared: &Arc<Mutex<Connection>>, playlist_id: &str, track_id: &str) -> bool {
        let entity_id = dirty::junction_entity_id(playlist_id, track_id);
        let conn = shared.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync_tombstones WHERE entity_type = ?1 AND entity_id = ?2",
                rusqlite::params![buckets::PLAYLIST_TRACKS, entity_id],
                |row| row.get(0),
            )
            .unwrap();
        count > 0
    }

    fn tombstone_count(shared: &Arc<Mutex<Connection>>, playlist_id: &str) -> i64 {
        let conn = shared.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM sync_tombstones WHERE entity_type = ?1 AND entity_id LIKE ?2",
            rusqlite::params![buckets::PLAYLIST_TRACKS, format!("{playlist_id}|%")],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn replace_tracks_writes_exact_ordered_membership() {
        let (shared, service) = seeded_service();
        let playlist = service
            .create_playlist("Ordered".to_string(), None, "library".to_string())
            .unwrap();

        let written = service
            .replace_tracks(
                &playlist.id,
                vec![
                    "track-3".to_string(),
                    "track-1".to_string(),
                    "track-2".to_string(),
                ],
            )
            .unwrap();

        assert_eq!(written, 3);
        assert_eq!(
            membership(&shared, &playlist.id),
            vec![
                ("track-3".to_string(), 0),
                ("track-1".to_string(), 1),
                ("track-2".to_string(), 2),
            ]
        );
    }

    #[test]
    fn replace_tracks_removes_stale_members_and_records_tombstones() {
        let (shared, service) = seeded_service();
        let playlist = service
            .create_playlist("Stale".to_string(), None, "library".to_string())
            .unwrap();
        service
            .replace_tracks(
                &playlist.id,
                vec![
                    "track-1".to_string(),
                    "track-2".to_string(),
                    "track-3".to_string(),
                ],
            )
            .unwrap();

        service
            .replace_tracks(
                &playlist.id,
                vec!["track-1".to_string(), "track-3".to_string()],
            )
            .unwrap();

        assert_eq!(
            membership(&shared, &playlist.id),
            vec![("track-1".to_string(), 0), ("track-3".to_string(), 1)]
        );
        assert!(
            has_tombstone(&shared, &playlist.id, "track-2"),
            "the removed member must be tombstoned"
        );
        assert!(!has_tombstone(&shared, &playlist.id, "track-1"));
    }

    #[test]
    fn replace_tracks_is_idempotent() {
        let (shared, service) = seeded_service();
        let playlist = service
            .create_playlist("Idempotent".to_string(), None, "library".to_string())
            .unwrap();
        let list = vec!["track-1".to_string(), "track-2".to_string()];

        service.replace_tracks(&playlist.id, list.clone()).unwrap();
        let first_positions = membership(&shared, &playlist.id);

        service.replace_tracks(&playlist.id, list).unwrap();

        assert_eq!(membership(&shared, &playlist.id), first_positions);
        assert_eq!(
            tombstone_count(&shared, &playlist.id),
            0,
            "a second identical call must not record tombstones"
        );
    }

    #[test]
    fn replace_tracks_deduplicates_repeated_ids() {
        let (shared, service) = seeded_service();
        let playlist = service
            .create_playlist("Dedup".to_string(), None, "library".to_string())
            .unwrap();

        let written = service
            .replace_tracks(
                &playlist.id,
                vec![
                    "track-1".to_string(),
                    "track-1".to_string(),
                    "track-2".to_string(),
                    "track-1".to_string(),
                ],
            )
            .unwrap();

        assert_eq!(written, 2);
        assert_eq!(
            membership(&shared, &playlist.id),
            vec![("track-1".to_string(), 0), ("track-2".to_string(), 1)]
        );
    }

    #[test]
    fn replace_tracks_with_empty_list_clears_membership() {
        let (shared, service) = seeded_service();
        let playlist = service
            .create_playlist("Clear".to_string(), None, "library".to_string())
            .unwrap();
        service
            .replace_tracks(
                &playlist.id,
                vec!["track-1".to_string(), "track-2".to_string()],
            )
            .unwrap();

        let written = service.replace_tracks(&playlist.id, vec![]).unwrap();

        assert_eq!(written, 0);
        assert!(membership(&shared, &playlist.id).is_empty());
        assert!(has_tombstone(&shared, &playlist.id, "track-1"));
        assert!(has_tombstone(&shared, &playlist.id, "track-2"));
    }
}
