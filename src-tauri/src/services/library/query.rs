use super::*;

impl LibraryService {
    pub fn get_tracks(&self, filter: Option<TrackFilter>) -> Result<Vec<Track>> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let mut sql = String::from(
            r#"
            SELECT
                t.id, t.file_path, t.file_hash,
                t.title, t.artist, t.album, t.year, t.genre, t.label, t.catalog_number,
                t.duration_ms, t.bpm, t.key, t.bitrate, t.sample_rate, t.format,
                t.analysis_source, t.waveform_data,
                t.rating, t.play_count,
                t.date_added, t.date_modified, t.last_played,
                t.rekordbox_id, t.artwork_path, t.artwork_source, t.color,
                t.library_root_id, t.relative_path, t.url
            FROM tracks t
            "#,
        );

        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref filter) = filter {
            if let Some(ref search) = filter.search {
                let escaped = search.replace('%', "\\%").replace('_', "\\_");
                let search_param = format!("%{escaped}%");
                conditions.push(
                    "(t.title LIKE ?1 ESCAPE '\\' OR t.artist LIKE ?1 ESCAPE '\\' OR t.album LIKE ?1 ESCAPE '\\')"
                        .to_string(),
                );
                params.push(Box::new(search_param));
            }

            if let Some(ref tag_ids) = filter.tag_ids {
                if !tag_ids.is_empty() {
                    let placeholders: Vec<String> = tag_ids
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", params.len() + i + 1))
                        .collect();

                    // Check filter mode: "and" requires all tags, "or" (default) requires any tag
                    let is_and_mode = filter
                        .tag_filter_mode
                        .as_ref()
                        .map(|m| m == "and")
                        .unwrap_or(false);

                    if is_and_mode {
                        // AND mode: track must have ALL selected tags
                        conditions.push(format!(
                            "t.id IN (SELECT track_id FROM track_tags WHERE tag_id IN ({}) GROUP BY track_id HAVING COUNT(DISTINCT tag_id) = {})",
                            placeholders.join(", "),
                            tag_ids.len()
                        ));
                    } else {
                        // OR mode: track must have ANY of the selected tags
                        conditions.push(format!(
                            "t.id IN (SELECT track_id FROM track_tags WHERE tag_id IN ({}))",
                            placeholders.join(", ")
                        ));
                    }

                    for tag_id in tag_ids {
                        params.push(Box::new(tag_id.clone()));
                    }
                }
            }

            if let Some(ref playlist_id) = filter.playlist_id {
                conditions.push(format!(
                    "t.id IN (SELECT track_id FROM playlist_tracks WHERE playlist_id = ?{})",
                    params.len() + 1
                ));
                params.push(Box::new(playlist_id.clone()));
            }

            if let Some(bpm_min) = filter.bpm_min {
                conditions.push(format!("t.bpm >= ?{}", params.len() + 1));
                params.push(Box::new(bpm_min));
            }

            if let Some(bpm_max) = filter.bpm_max {
                conditions.push(format!("t.bpm <= ?{}", params.len() + 1));
                params.push(Box::new(bpm_max));
            }

            if let Some(ref key) = filter.key {
                conditions.push(format!("t.key = ?{}", params.len() + 1));
                params.push(Box::new(key.clone()));
            }
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY t.date_added DESC");

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let tracks = stmt
            .query_map(params_refs.as_slice(), |row| {
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
                    tags: Vec::new(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Fetch tags for all tracks
        let tracks_with_tags = Self::fetch_tags_for_tracks(&conn, tracks)?;

        Ok(tracks_with_tags)
    }

    /// Fetch tags for a batch of tracks on a caller-supplied connection.
    ///
    /// Associated (no `&self`) so hash lookups can run while a scan holds the mutex guard.
    pub(crate) fn fetch_tags_for_tracks(
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

    pub fn get_track(&self, id: &str) -> Result<Track> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let track = conn.query_row(
            r#"
            SELECT
                id, file_path, file_hash,
                title, artist, album, year, genre, label, catalog_number,
                duration_ms, bpm, key, bitrate, sample_rate, format,
                analysis_source, waveform_data,
                rating, play_count,
                date_added, date_modified, last_played,
                rekordbox_id, artwork_path, artwork_source, color,
                library_root_id, relative_path, url
            FROM tracks WHERE id = ?1
            "#,
            [id],
            |row| {
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
                    tags: Vec::new(),
                })
            },
        )?;

        // Fetch tags
        let tracks_with_tags = Self::fetch_tags_for_tracks(&conn, vec![track])?;
        tracks_with_tags
            .into_iter()
            .next()
            .ok_or_else(|| CrateError::TrackNotFound(id.to_string()))
    }

    /// Find an existing track by its file hash
    pub fn find_track_by_hash(&self, file_hash: &str) -> Result<Option<Track>> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        Self::find_track_by_hash_in(&conn, file_hash)
    }

    /// Find an existing track by its file hash on a caller-supplied connection.
    ///
    /// Takes `&Connection` (not `&self`) so the folder scan can look a hash up while it
    /// already holds the mutex guard; locking again from the same thread would deadlock.
    pub(crate) fn find_track_by_hash_in(
        conn: &Connection,
        file_hash: &str,
    ) -> Result<Option<Track>> {
        let result = conn.query_row(
            r#"
            SELECT
                id, file_path, file_hash,
                title, artist, album, year, genre, label, catalog_number,
                duration_ms, bpm, key, bitrate, sample_rate, format,
                analysis_source, waveform_data,
                rating, play_count,
                date_added, date_modified, last_played,
                rekordbox_id, artwork_path, artwork_source, color,
                library_root_id, relative_path, url
            FROM tracks WHERE file_hash = ?1
            "#,
            [file_hash],
            |row| {
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
                    tags: Vec::new(),
                })
            },
        );

        match result {
            Ok(track) => {
                // Fetch tags for the track
                let tracks_with_tags = Self::fetch_tags_for_tracks(conn, vec![track])?;
                Ok(tracks_with_tags.into_iter().next())
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CrateError::Database(e)),
        }
    }
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../../test_utils.rs"]
mod test_utils;

#[cfg(test)]
mod tests {
    use super::*;

    fn library(conn: Connection, app_data_dir: &std::path::Path) -> LibraryService {
        LibraryService::new(
            Arc::new(Mutex::new(conn)),
            app_data_dir.to_path_buf(),
            FileTagsService::new(),
        )
    }

    /// Seed one category/tag/link with values distinct from every track column, so the
    /// positional `fetch_tags_for_tracks` closure is checked alongside the track map.
    fn seed_tag(conn: &Connection) -> Tag {
        conn
      .execute(
        "INSERT INTO tag_categories (id, name, sort_order, color) VALUES ('s-cat', 's-category-name', 91, '#111111')",
        [],
      )
      .unwrap();
        conn
      .execute(
        "INSERT INTO tags (id, category_id, name, color, sort_order) VALUES ('s-tag', 's-cat', 's-tag-name', '#222222', 92)",
        [],
      )
      .unwrap();
        conn.execute(
            "INSERT INTO track_tags (track_id, tag_id) VALUES ('sentinel-track-9', 's-tag')",
            [],
        )
        .unwrap();
        Tag {
            id: "s-tag".to_string(),
            category_id: "s-cat".to_string(),
            name: "s-tag-name".to_string(),
            color: Some("#222222".to_string()),
            sort_order: 92,
        }
    }

    #[test]
    fn get_tracks_maps_every_column_to_its_own_value() {
        let dir = tempfile::tempdir().unwrap();
        let conn = test_utils::make_memory_db();
        let track = test_utils::sentinel_track();
        test_utils::insert_sentinel_track(&conn, &track);
        let tag = seed_tag(&conn);
        let library = library(conn, dir.path());

        let mut expected = track.clone();
        expected.tags = vec![tag];

        let tracks = library.get_tracks(None).unwrap();
        assert_eq!(tracks.len(), 1);
        test_utils::assert_track_eq(&tracks[0], &expected);
    }

    #[test]
    fn get_tracks_filters_bind_placeholders_to_the_right_slots() {
        let dir = tempfile::tempdir().unwrap();
        let conn = test_utils::make_memory_db();
        let track = test_utils::sentinel_track();
        test_utils::insert_sentinel_track(&conn, &track);
        let tag = seed_tag(&conn);
        let library = library(conn, dir.path());

        // Each filter arm appends its own placeholders; a miscounted ?N slot must return
        // the wrong set. Search uses ?1; combined search + tag pushes the tag to ?2.
        let hits = library
            .get_tracks(Some(TrackFilter {
                search: Some("s-titl".to_string()),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(hits.len(), 1);
        let mut expected = track;
        expected.tags = vec![tag];
        test_utils::assert_track_eq(&hits[0], &expected);

        let misses = library
            .get_tracks(Some(TrackFilter {
                search: Some("no-such-text".to_string()),
                ..Default::default()
            }))
            .unwrap();
        assert!(misses.is_empty());

        let by_key = library
            .get_tracks(Some(TrackFilter {
                key: Some("s-key".to_string()),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(by_key.len(), 1);

        let by_bpm_range = library
            .get_tracks(Some(TrackFilter {
                bpm_min: Some(123.0),
                bpm_max: Some(124.0),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(by_bpm_range.len(), 1);

        let by_tag = library
            .get_tracks(Some(TrackFilter {
                tag_ids: Some(vec!["s-tag".to_string()]),
                tag_filter_mode: Some("and".to_string()),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(by_tag.len(), 1);

        let search_and_tag = library
            .get_tracks(Some(TrackFilter {
                search: Some("s-titl".to_string()),
                tag_ids: Some(vec!["s-tag".to_string()]),
                tag_filter_mode: Some("and".to_string()),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(search_and_tag.len(), 1);

        let wrong_tag = library
            .get_tracks(Some(TrackFilter {
                tag_ids: Some(vec!["no-such-tag".to_string()]),
                ..Default::default()
            }))
            .unwrap();
        assert!(wrong_tag.is_empty());
    }
}
