use super::*;
use crate::services::cloud_sync::pipeline::{buckets, dirty};
use std::path::PathBuf;

impl LibraryService {
  /// Helper: write BPM/key changes back to the audio file after DB update.
  fn write_file_meta_to_path(
    file_tags: &FileTagsService,
    file_path: &str,
    bpm: Option<f64>,
    key: Option<&str>,
  ) -> Result<()> {
    if bpm.is_none() && key.is_none() {
      return Ok(());
    }

    let path = PathBuf::from(file_path);
    if !path.exists() {
      log::debug!("Skipping file tag write – file not found: {path:?}");
      return Ok(());
    }

    file_tags.write_bpm_and_key(&path, bpm.unwrap_or(0.0), key)
  }

  pub fn update_track(&self, id: &str, update: TrackUpdate) -> Result<Track> {
    let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

    let now = chrono::Utc::now().to_rfc3339();

    // Build update query dynamically based on provided fields
    let mut updates: Vec<String> = vec!["date_modified = ?1".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];
    let mut param_idx = 2;

    if let Some(ref title) = update.title {
      updates.push(format!("title = ?{param_idx}"));
      params.push(Box::new(title.clone()));
      param_idx += 1;
    }
    if let Some(ref artist) = update.artist {
      updates.push(format!("artist = ?{param_idx}"));
      params.push(Box::new(artist.clone()));
      param_idx += 1;
    }
    if let Some(ref album) = update.album {
      updates.push(format!("album = ?{param_idx}"));
      params.push(Box::new(album.clone()));
      param_idx += 1;
    }
    if let Some(year) = update.year {
      updates.push(format!("year = ?{param_idx}"));
      params.push(Box::new(year));
      param_idx += 1;
    }
    if let Some(ref genre) = update.genre {
      updates.push(format!("genre = ?{param_idx}"));
      params.push(Box::new(genre.clone()));
      param_idx += 1;
    }
    if let Some(ref label) = update.label {
      updates.push(format!("label = ?{param_idx}"));
      params.push(Box::new(label.clone()));
      param_idx += 1;
    }
    if let Some(ref url) = update.url {
      updates.push(format!("url = ?{param_idx}"));
      params.push(Box::new(url.clone()));
      param_idx += 1;
    }
    if let Some(bpm) = update.bpm {
      updates.push(format!("bpm = ?{param_idx}"));
      params.push(Box::new(bpm));
      param_idx += 1;
    }
    if let Some(ref key) = update.key {
      updates.push(format!("key = ?{param_idx}"));
      params.push(Box::new(key.clone()));
      param_idx += 1;
    }
    if let Some(rating) = update.rating {
      updates.push(format!("rating = ?{param_idx}"));
      params.push(Box::new(rating));
      param_idx += 1;
    }

    let hlc = dirty::next_hlc(&conn)?;
    updates.push(format!("_hlc = ?{param_idx}"));
    params.push(Box::new(hlc));
    param_idx += 1;

    params.push(Box::new(id.to_string()));

    let sql = format!(
      "UPDATE tracks SET {} WHERE id = ?{}",
      updates.join(", "),
      param_idx
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, params_refs.as_slice())?;
    dirty::mark_dirty(&conn, &buckets::bucket_for_track_id(id))?;

    drop(conn);

    // Get the track to access its file path for writing metadata to the audio file
    let track = self.get_track(id)?;

    // Persist BPM/key to the audio file if either was provided.
    let _ = Self::write_file_meta_to_path(
      &self.file_tags,
      &track.file_path,
      update.bpm,
      update.key.as_deref(),
    );

    Ok(track)
  }

  /// Update multiple tracks with the same update data (bulk operation)
  pub fn update_tracks(&self, ids: Vec<String>, update: TrackUpdate) -> Result<Vec<Track>> {
    if ids.is_empty() {
      return Ok(Vec::new());
    }

    let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

    let now = chrono::Utc::now().to_rfc3339();

    // Build update query dynamically based on provided fields
    let mut updates: Vec<String> = vec!["date_modified = ?1".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];
    let mut param_idx = 2;

    if let Some(ref title) = update.title {
      updates.push(format!("title = ?{param_idx}"));
      params.push(Box::new(title.clone()));
      param_idx += 1;
    }
    if let Some(ref artist) = update.artist {
      updates.push(format!("artist = ?{param_idx}"));
      params.push(Box::new(artist.clone()));
      param_idx += 1;
    }
    if let Some(ref album) = update.album {
      updates.push(format!("album = ?{param_idx}"));
      params.push(Box::new(album.clone()));
      param_idx += 1;
    }
    if let Some(year) = update.year {
      updates.push(format!("year = ?{param_idx}"));
      params.push(Box::new(year));
      param_idx += 1;
    }
    if let Some(ref genre) = update.genre {
      updates.push(format!("genre = ?{param_idx}"));
      params.push(Box::new(genre.clone()));
      param_idx += 1;
    }
    if let Some(ref label) = update.label {
      updates.push(format!("label = ?{param_idx}"));
      params.push(Box::new(label.clone()));
      param_idx += 1;
    }
    if let Some(ref url) = update.url {
      updates.push(format!("url = ?{param_idx}"));
      params.push(Box::new(url.clone()));
      param_idx += 1;
    }
    if let Some(bpm) = update.bpm {
      updates.push(format!("bpm = ?{param_idx}"));
      params.push(Box::new(bpm));
      param_idx += 1;
    }
    if let Some(ref key) = update.key {
      updates.push(format!("key = ?{param_idx}"));
      params.push(Box::new(key.clone()));
      param_idx += 1;
    }
    if let Some(rating) = update.rating {
      updates.push(format!("rating = ?{param_idx}"));
      params.push(Box::new(rating));
      param_idx += 1;
    }

    let hlc = dirty::next_hlc(&conn)?;
    updates.push(format!("_hlc = ?{param_idx}"));
    params.push(Box::new(hlc));
    param_idx += 1;

    // Build placeholders for track IDs
    let placeholders: Vec<String> = ids
      .iter()
      .enumerate()
      .map(|(i, _)| format!("?{}", param_idx + i))
      .collect();

    for id in &ids {
      params.push(Box::new(id.clone()));
    }

    let sql = format!(
      "UPDATE tracks SET {} WHERE id IN ({})",
      updates.join(", "),
      placeholders.join(", ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, params_refs.as_slice())?;

    drop(conn);

    // Return all updated tracks, writing BPM/key to audio files along the way.
    let mut updated_tracks = Vec::new();
    for id in &ids {
      if let Ok(track) = self.get_track(id) {
        let _ = Self::write_file_meta_to_path(
          &self.file_tags,
          &track.file_path,
          update.bpm,
          update.key.as_deref(),
        );
        updated_tracks.push(track);
      }
    }

    Ok(updated_tracks)
  }

  /// Set color for multiple tracks (bulk operation)
  pub fn set_track_colors(&self, track_ids: Vec<String>, color: Option<String>) -> Result<()> {
    if track_ids.is_empty() {
      return Ok(());
    }

    let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

    let now = chrono::Utc::now().to_rfc3339();
    let hlc = dirty::next_hlc(&conn)?;

    let placeholders: Vec<String> = track_ids
      .iter()
      .enumerate()
      .map(|(i, _)| format!("?{}", i + 4))
      .collect();

    let sql = format!(
      "UPDATE tracks SET color = ?1, date_modified = ?2, _hlc = ?3 WHERE id IN ({})",
      placeholders.join(", ")
    );

    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
      vec![Box::new(color), Box::new(now), Box::new(hlc)];

    for id in &track_ids {
      params.push(Box::new(id.clone()));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, params_refs.as_slice())?;
    dirty::mark_dirty_track_shards(&conn, &track_ids)?;

    Ok(())
  }

  pub fn delete_tracks(&self, ids: Vec<String>) -> Result<()> {
    // Delete artwork files for each track
    for id in &ids {
      self.artwork_service.delete(id);
    }

    let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

    let placeholders: Vec<String> = ids
      .iter()
      .enumerate()
      .map(|(i, _)| format!("?{}", i + 1))
      .collect();

    let sql = format!(
      "DELETE FROM tracks WHERE id IN ({})",
      placeholders.join(", ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> =
      ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

    let hlc = dirty::next_hlc(&conn)?;
    for id in &ids {
      dirty::record_tombstone(&conn, buckets::TRACKS_ENTITY, id, &hlc)?;
    }

    conn.execute(&sql, params_refs.as_slice())?;

    // The deleted tracks' shards, plus the cascade-deleted child buckets
    // (playlist memberships, tag links, cues).
    dirty::mark_dirty_track_shards(&conn, &ids)?;
    dirty::mark_dirty(&conn, buckets::PLAYLIST_TRACKS)?;
    dirty::mark_dirty(&conn, buckets::TRACK_TAGS)?;
    dirty::mark_dirty(&conn, buckets::CUES)?;

    Ok(())
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

  /// The dynamic `SET` array appends one `?N` slot per present field; a drifted slot
  /// counter must write one column's value into another column.
  #[test]
  fn update_track_sets_only_the_provided_columns() {
    let dir = tempfile::tempdir().unwrap();
    let conn = test_utils::make_memory_db();
    let track = test_utils::sentinel_track();
    test_utils::insert_sentinel_track(&conn, &track);
    let library = library(conn, dir.path());

    let updated = library
      .update_track(
        &track.id,
        TrackUpdate {
          title: Some("updated-title".to_string()),
          year: Some(1988),
          url: Some("https://example.test/updated".to_string()),
          rating: Some(2),
          ..Default::default()
        },
      )
      .unwrap();

    let mut expected = track.clone();
    expected.title = Some("updated-title".to_string());
    expected.year = Some(1988);
    expected.url = Some("https://example.test/updated".to_string());
    expected.rating = 2;
    expected.date_modified = updated.date_modified.clone();
    // None means UNCHANGED in TrackUpdate semantics: every other sentinel must survive,
    // which assert_track_eq checks field-by-field against the untouched fixture.
    test_utils::assert_track_eq(&updated, &expected);

    let reread = library.get_track(&track.id).unwrap();
    test_utils::assert_track_eq(&reread, &updated);
  }

  /// An all-None update must leave stored values (including `url`) untouched, and the
  /// bulk path's `WHERE id IN (...)` placeholders must bind the ids, not earlier slots.
  #[test]
  fn update_track_with_none_leaves_url_and_bulk_update_targets_all_rows() {
    let dir = tempfile::tempdir().unwrap();
    let conn = test_utils::make_memory_db();
    let track_a = test_utils::sentinel_track();
    let mut track_b = track_a.clone();
    track_b.id = "sentinel-track-8".to_string();
    track_b.file_path = "/music/sentinel/track-8.mp3".to_string();
    test_utils::insert_sentinel_track(&conn, &track_a);
    test_utils::insert_sentinel_track(&conn, &track_b);
    let library = library(conn, dir.path());

    let kept = library
      .update_track(&track_b.id, TrackUpdate::default())
      .unwrap();
    assert_eq!(kept.url, track_b.url, "None must not clear the stored url");

    let updated = library
      .update_tracks(
        vec![track_a.id.clone(), track_b.id.clone()],
        TrackUpdate {
          genre: Some("bulk-genre".to_string()),
          bpm: Some(145.5),
          ..Default::default()
        },
      )
      .unwrap();
    assert_eq!(updated.len(), 2);

    for (got, source) in updated.iter().zip([&track_a, &track_b]) {
      assert_eq!(got.id, source.id);
      assert_eq!(got.genre.as_deref(), Some("bulk-genre"));
      assert_eq!(got.bpm, Some(145.5));
      assert_eq!(
        got.url, source.url,
        "unlisted fields must survive the bulk update"
      );
      assert_eq!(got.title, source.title);
    }

    let reread = library.get_track(&track_a.id).unwrap();
    assert_eq!(reread.genre, updated[0].genre);
    assert_eq!(reread.url, track_a.url);
  }
}
