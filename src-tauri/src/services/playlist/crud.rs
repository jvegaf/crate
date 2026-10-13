use super::*;
use crate::services::cloud_sync::pipeline::{buckets, dirty};
use rusqlite::OptionalExtension;

impl PlaylistService {
    pub fn get_playlists(&self, context: &str) -> Result<Vec<Playlist>> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let mut stmt = conn.prepare(
            r#"
            SELECT
                p.id, p.name, p.parent_id, p.is_folder, p.is_smart,
                p.smart_rules, p.sort_order, p.date_created, p.date_modified,
                COALESCE(
                    CASE WHEN p.context = 'discovery'
                        THEN (SELECT COUNT(*) FROM playlist_discovery_tracks WHERE playlist_id = p.id)
                           + (SELECT COUNT(*) FROM playlist_discovery_releases WHERE playlist_id = p.id)
                        ELSE (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = p.id)
                    END, 0
                ) as track_count,
                p.context
            FROM playlists p
            WHERE p.context = ?1
            ORDER BY p.sort_order, p.name
            "#,
        )?;

        let mut playlists = stmt
            .query_map([context], |row| {
                Ok(Playlist {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    parent_id: row.get(2)?,
                    is_folder: row.get(3)?,
                    is_smart: row.get(4)?,
                    smart_rules: row.get(5)?,
                    sort_order: row.get(6)?,
                    date_created: row.get(7)?,
                    date_modified: row.get(8)?,
                    track_count: row.get(9)?,
                    context: row.get(10)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Compute real counts for smart playlists by evaluating their rules
        for playlist in &mut playlists {
            if playlist.is_smart {
                if let Some(ref json) = playlist.smart_rules {
                    if let Ok(rules) = serde_json::from_str::<SmartRules>(json) {
                        if let Ok(count) = self.count_smart_playlist_items_with_conn(
                            &conn,
                            &rules,
                            &playlist.context,
                        ) {
                            playlist.track_count = count;
                        }
                    }
                }
            }
        }

        Ok(playlists)
    }

    pub fn get_playlist(&self, id: &str) -> Result<Playlist> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let mut playlist = self.get_playlist_with_conn(&conn, id)?;

        // Compute real count for smart playlists
        if playlist.is_smart {
            if let Some(ref json) = playlist.smart_rules {
                if let Ok(rules) = serde_json::from_str::<SmartRules>(json) {
                    if let Ok(count) =
                        self.count_smart_playlist_items_with_conn(&conn, &rules, &playlist.context)
                    {
                        playlist.track_count = count;
                    }
                }
            }
        }

        Ok(playlist)
    }

    pub fn create_playlist(
        &self,
        name: String,
        parent_id: Option<String>,
        context: String,
    ) -> Result<Playlist> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        // Get next sort order
        let max_order: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) FROM playlists WHERE parent_id IS ?1",
                rusqlite::params![parent_id],
                |row| row.get(0),
            )
            .unwrap_or(-1);

        let playlist = Playlist {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            parent_id,
            is_folder: false,
            is_smart: false,
            smart_rules: None,
            sort_order: max_order + 1,
            date_created: chrono::Utc::now().to_rfc3339(),
            date_modified: chrono::Utc::now().to_rfc3339(),
            track_count: 0,
            context: context.clone(),
        };

        let hlc = dirty::next_hlc(&conn)?;
        conn.execute(
            r#"
            INSERT INTO playlists (id, name, parent_id, is_folder, is_smart, smart_rules, sort_order, date_created, date_modified, context, _hlc)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            rusqlite::params![
                playlist.id,
                playlist.name,
                playlist.parent_id,
                playlist.is_folder,
                playlist.is_smart,
                playlist.smart_rules,
                playlist.sort_order,
                playlist.date_created,
                playlist.date_modified,
                context,
                hlc,
            ],
        )?;
        dirty::mark_dirty(&conn, buckets::PLAYLISTS)?;

        Ok(playlist)
    }

    pub fn create_folder(
        &self,
        name: String,
        parent_id: Option<String>,
        context: String,
    ) -> Result<Playlist> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let max_order: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) FROM playlists WHERE parent_id IS ?1",
                rusqlite::params![parent_id],
                |row| row.get(0),
            )
            .unwrap_or(-1);

        let folder = Playlist {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            parent_id,
            is_folder: true,
            is_smart: false,
            smart_rules: None,
            sort_order: max_order + 1,
            date_created: chrono::Utc::now().to_rfc3339(),
            date_modified: chrono::Utc::now().to_rfc3339(),
            track_count: 0,
            context: context.clone(),
        };

        let hlc = dirty::next_hlc(&conn)?;
        conn.execute(
            r#"
            INSERT INTO playlists (id, name, parent_id, is_folder, is_smart, smart_rules, sort_order, date_created, date_modified, context, _hlc)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            rusqlite::params![
                folder.id,
                folder.name,
                folder.parent_id,
                folder.is_folder,
                folder.is_smart,
                folder.smart_rules,
                folder.sort_order,
                folder.date_created,
                folder.date_modified,
                context,
                hlc,
            ],
        )?;
        dirty::mark_dirty(&conn, buckets::PLAYLISTS)?;

        Ok(folder)
    }

    /// Find-or-create a playlist (or folder) with the exact `name` under `parent_id` in
    /// `context`.
    ///
    /// A repeatable import needs an idempotent lookup, so this searches for an existing child
    /// by `(name, parent_id, context)` first. When one exists it is returned untouched — no
    /// write and no HLC bump — so a no-op re-import does not mark the playlist dirty or trigger
    /// a cloud-sync push.
    ///
    /// If the existing row's `is_folder` differs from the requested `is_folder`, the existing row
    /// is still returned as-is: this method never renames an item nor creates a sibling under the
    /// same name. Callers that need the requested kind must reconcile the divergence themselves.
    pub fn ensure_playlist(
        &self,
        name: &str,
        parent_id: Option<&str>,
        is_folder: bool,
        context: &str,
    ) -> Result<Playlist> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let tx = conn.unchecked_transaction()?;

        // NULL-safe `parent_id IS ?1` comparison mirrors `find_conflict` (movement.rs:26).
        let existing: Option<Playlist> = tx
            .query_row(
                r#"
                SELECT
                    p.id, p.name, p.parent_id, p.is_folder, p.is_smart,
                    p.smart_rules, p.sort_order, p.date_created, p.date_modified,
                    COALESCE(
                        CASE WHEN p.context = 'discovery'
                            THEN (SELECT COUNT(*) FROM playlist_discovery_tracks WHERE playlist_id = p.id)
                               + (SELECT COUNT(*) FROM playlist_discovery_releases WHERE playlist_id = p.id)
                            ELSE (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = p.id)
                        END, 0
                    ) as track_count,
                    p.context
                FROM playlists p
                WHERE p.parent_id IS ?1 AND p.name = ?2 AND p.context = ?3
                "#,
                rusqlite::params![parent_id, name, context],
                |row| {
                    Ok(Playlist {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        parent_id: row.get(2)?,
                        is_folder: row.get(3)?,
                        is_smart: row.get(4)?,
                        smart_rules: row.get(5)?,
                        sort_order: row.get(6)?,
                        date_created: row.get(7)?,
                        date_modified: row.get(8)?,
                        track_count: row.get(9)?,
                        context: row.get(10)?,
                    })
                },
            )
            .optional()?;

        if let Some(playlist) = existing {
            return Ok(playlist);
        }

        // Same sibling sort-order probe (crud.rs:98) and INSERT column list (crud.rs:121) as
        // `create_playlist`.
        let max_order: i32 = tx
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) FROM playlists WHERE parent_id IS ?1",
                rusqlite::params![parent_id],
                |row| row.get(0),
            )
            .unwrap_or(-1);

        let playlist = Playlist {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            parent_id: parent_id.map(str::to_string),
            is_folder,
            is_smart: false,
            smart_rules: None,
            sort_order: max_order + 1,
            date_created: chrono::Utc::now().to_rfc3339(),
            date_modified: chrono::Utc::now().to_rfc3339(),
            track_count: 0,
            context: context.to_string(),
        };

        let hlc = dirty::next_hlc(&tx)?;
        tx.execute(
            r#"
            INSERT INTO playlists (id, name, parent_id, is_folder, is_smart, smart_rules, sort_order, date_created, date_modified, context, _hlc)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            rusqlite::params![
                playlist.id,
                playlist.name,
                playlist.parent_id,
                playlist.is_folder,
                playlist.is_smart,
                playlist.smart_rules,
                playlist.sort_order,
                playlist.date_created,
                playlist.date_modified,
                context,
                hlc,
            ],
        )?;
        dirty::mark_dirty(&tx, buckets::PLAYLISTS)?;

        tx.commit()?;

        Ok(playlist)
    }

    pub fn rename_playlist(&self, id: &str, name: String) -> Result<Playlist> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let now = chrono::Utc::now().to_rfc3339();
        let hlc = dirty::next_hlc(&conn)?;

        conn.execute(
            "UPDATE playlists SET name = ?1, date_modified = ?2, _hlc = ?3 WHERE id = ?4",
            rusqlite::params![name, now, hlc, id],
        )?;
        dirty::mark_dirty(&conn, buckets::PLAYLISTS)?;

        drop(conn);
        self.get_playlist(id)
    }

    /// Collect all track IDs and release IDs associated with a playlist (or folder subtree).
    /// Must be called BEFORE delete_playlist since CASCADE deletes junction table entries.
    /// Returns (track_ids, release_ids) — one will be empty depending on context.
    pub fn collect_associated_item_ids(&self, id: &str) -> Result<(Vec<String>, Vec<String>)> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        // Use recursive CTE to find all playlist IDs in the subtree (handles folders)
        let mut stmt = conn.prepare(
            r#"
            WITH RECURSIVE subtree(id, context) AS (
                SELECT id, context FROM playlists WHERE id = ?1
                UNION ALL
                SELECT p.id, p.context FROM playlists p
                JOIN subtree s ON p.parent_id = s.id
            )
            SELECT id, context FROM subtree
            "#,
        )?;

        let rows: Vec<(String, String)> = stmt
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut track_ids = Vec::new();
        let mut release_ids = Vec::new();

        for (playlist_id, context) in &rows {
            if context == "discovery" {
                // Member tracks' parent releases, plus whole releases still pending expansion.
                let mut rel_stmt = conn.prepare(
                    "SELECT dt.release_id FROM playlist_discovery_tracks pdt \
                     JOIN discovery_tracks dt ON dt.id = pdt.track_id WHERE pdt.playlist_id = ?1 \
                     UNION \
                     SELECT release_id FROM playlist_discovery_releases WHERE playlist_id = ?1",
                )?;
                let ids: Vec<String> = rel_stmt
                    .query_map([playlist_id], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                release_ids.extend(ids);
            } else {
                let mut trk_stmt =
                    conn.prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1")?;
                let ids: Vec<String> = trk_stmt
                    .query_map([playlist_id], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                track_ids.extend(ids);
            }
        }

        // Deduplicate
        track_ids.sort();
        track_ids.dedup();
        release_ids.sort();
        release_ids.dedup();

        Ok((track_ids, release_ids))
    }

    pub fn delete_playlist(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        // Foreign key cascade deletes child playlists + junction entries; the
        // tombstone drives the same cascade on peers.
        let hlc = dirty::next_hlc(&conn)?;
        dirty::record_tombstone(&conn, buckets::PLAYLISTS, id, &hlc)?;
        conn.execute("DELETE FROM playlists WHERE id = ?1", [id])?;
        dirty::mark_dirty(&conn, buckets::PLAYLISTS)?;
        dirty::mark_dirty(&conn, buckets::PLAYLIST_TRACKS)?;
        dirty::mark_dirty(&conn, buckets::PLAYLIST_DISCOVERY_RELEASES)?;
        dirty::mark_dirty(&conn, buckets::PLAYLIST_DISCOVERY_TRACKS)?;

        Ok(())
    }

    /// Get direct children of a folder
    pub fn get_children(&self, parent_id: &str) -> Result<Vec<Playlist>> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let mut stmt = conn.prepare(
            r#"
            SELECT
                p.id, p.name, p.parent_id, p.is_folder, p.is_smart,
                p.smart_rules, p.sort_order, p.date_created, p.date_modified,
                COALESCE(
                    CASE WHEN p.context = 'discovery'
                        THEN (SELECT COUNT(*) FROM playlist_discovery_tracks WHERE playlist_id = p.id)
                           + (SELECT COUNT(*) FROM playlist_discovery_releases WHERE playlist_id = p.id)
                        ELSE (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = p.id)
                    END, 0
                ) as track_count,
                p.context
            FROM playlists p
            WHERE p.parent_id = ?1
            ORDER BY p.sort_order, p.name
            "#,
        )?;

        let playlists = stmt
            .query_map([parent_id], |row| {
                Ok(Playlist {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    parent_id: row.get(2)?,
                    is_folder: row.get(3)?,
                    is_smart: row.get(4)?,
                    smart_rules: row.get(5)?,
                    sort_order: row.get(6)?,
                    date_created: row.get(7)?,
                    date_modified: row.get(8)?,
                    track_count: row.get(9)?,
                    context: row.get(10)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(playlists)
    }

    /// Internal helper to get playlist with an existing connection
    pub(crate) fn get_playlist_with_conn(&self, conn: &Connection, id: &str) -> Result<Playlist> {
        conn.query_row(
            r#"
            SELECT
                p.id, p.name, p.parent_id, p.is_folder, p.is_smart,
                p.smart_rules, p.sort_order, p.date_created, p.date_modified,
                COALESCE(
                    CASE WHEN p.context = 'discovery'
                        THEN (SELECT COUNT(*) FROM playlist_discovery_tracks WHERE playlist_id = p.id)
                           + (SELECT COUNT(*) FROM playlist_discovery_releases WHERE playlist_id = p.id)
                        ELSE (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = p.id)
                    END, 0
                ) as track_count,
                p.context
            FROM playlists p
            WHERE p.id = ?1
            "#,
            [id],
            |row| {
                Ok(Playlist {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    parent_id: row.get(2)?,
                    is_folder: row.get(3)?,
                    is_smart: row.get(4)?,
                    smart_rules: row.get(5)?,
                    sort_order: row.get(6)?,
                    date_created: row.get(7)?,
                    date_modified: row.get(8)?,
                    track_count: row.get(9)?,
                    context: row.get(10)?,
                })
            },
        )
        .map_err(|e| e.into())
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
    fn ensure_playlist_creates_once_and_returns_the_same_row() {
        let conn = test_utils::make_memory_db();
        let shared = Arc::new(Mutex::new(conn));
        let service = PlaylistService::new(shared.clone());

        let first = service
            .ensure_playlist("Imported", None, false, "library")
            .unwrap();
        let second = service
            .ensure_playlist("Imported", None, false, "library")
            .unwrap();
        assert_eq!(first.id, second.id, "a re-ensure must return the same row");

        // A same-named child under a different parent is a distinct row.
        let folder = service
            .ensure_playlist("Folder", None, true, "library")
            .unwrap();
        let nested = service
            .ensure_playlist("Imported", Some(&folder.id), false, "library")
            .unwrap();
        assert_ne!(
            nested.id, first.id,
            "same name under a different parent is a distinct row"
        );
        assert_eq!(nested.parent_id.as_deref(), Some(folder.id.as_str()));

        let guard = shared.lock().unwrap();
        let root_count: i64 = guard
            .query_row(
                "SELECT COUNT(*) FROM playlists \
                 WHERE name = 'Imported' AND parent_id IS NULL AND context = 'library'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let nested_count: i64 = guard
            .query_row(
                "SELECT COUNT(*) FROM playlists WHERE name = 'Imported' AND parent_id = ?1",
                [&folder.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            root_count, 1,
            "no duplicate rows at the same (name, parent_id)"
        );
        assert_eq!(nested_count, 1);
    }

    #[test]
    fn ensure_playlist_preserves_folder_divergence_without_a_sibling() {
        let conn = test_utils::make_memory_db();
        let shared = Arc::new(Mutex::new(conn));
        let service = PlaylistService::new(shared.clone());

        let folder = service
            .ensure_playlist("Shared", None, true, "library")
            .unwrap();
        let replayed = service
            .ensure_playlist("Shared", None, false, "library")
            .unwrap();

        assert_eq!(replayed.id, folder.id);
        assert!(
            replayed.is_folder,
            "the existing folder row is returned unchanged, not replaced by a playlist"
        );

        let guard = shared.lock().unwrap();
        let count: i64 = guard
            .query_row(
                "SELECT COUNT(*) FROM playlists WHERE name = 'Shared' AND parent_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "divergence must not create a sibling");
    }
}
