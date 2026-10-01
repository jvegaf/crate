//! Shared helpers for the desktop backend unit tests.
//!
//! This file is pulled into each test module that needs it with
//! `#[cfg(test)] #[path = "../../test_utils.rs"] mod test_utils;`, because the crate root
//! (`lib.rs`) is outside this change's edit surface. It is therefore only ever compiled
//! for test builds (`cargo test --features desktop`); it has no effect on production or on
//! the mobile build, whose test modules do not include it.

use std::path::Path;

use rusqlite::Connection;

use crate::db::schema::get_migrations;
use crate::models::{BackupTrack, Track};

/// Throwaway encryption key for test-only SQLCipher databases.
///
/// Never used by production code: [`crate::db::Database`] provisions its key through the
/// platform provider. The value only has to be consistent within a single test.
pub const TEST_DB_KEY: &str = "crate-test-key";

/// Open a real (file-backed, encrypted) SQLCipher database at `path`, apply [`TEST_DB_KEY`],
/// enable foreign keys, and run every migration in order — the same sequence
/// [`crate::db::Database::new`] performs, minus the platform key provider.
///
/// Unlike an in-memory connection, this exercises the bundled SQLCipher build: the key is
/// set before the first statement, so the migrations run against an encrypted file. The
/// caller owns `path` (and any temp directory backing it).
pub fn make_test_db(path: &Path) -> Connection {
  let conn = Connection::open(path).expect("failed to open test database");
  conn
    .pragma_update(None, "key", TEST_DB_KEY)
    .expect("failed to apply test encryption key");
  conn
    .execute_batch("PRAGMA foreign_keys = ON;")
    .expect("failed to enable foreign keys");
  apply_migrations(&conn);
  conn
}

/// In-memory connection with the full migrated schema and foreign keys enabled.
///
/// For tests that need the schema but not a real encrypted file.
pub fn make_memory_db() -> Connection {
  let conn = Connection::open_in_memory().expect("failed to open in-memory database");
  conn
    .execute_batch("PRAGMA foreign_keys = ON;")
    .expect("failed to enable foreign keys");
  apply_migrations(&conn);
  conn
}

/// Apply every migration, mirroring the version-gated, per-migration-transaction loop in
/// `crate::db::run_migrations` (which is private to `db`). `schema_version` is created and
/// each version is recorded in the same transaction as its DDL, so helpers built on this
/// behave exactly like a fresh production database.
fn apply_migrations(conn: &Connection) {
  conn
    .execute(
      "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
      [],
    )
    .expect("failed to create schema_version");

  for (idx, sql) in get_migrations().iter().enumerate() {
    let version = idx as i32 + 1;
    let tx = conn
      .unchecked_transaction()
      .expect("failed to begin migration transaction");
    tx.execute_batch(sql).expect("failed to run migration");
    tx.execute(
      "INSERT INTO schema_version (version) VALUES (?1)",
      [version],
    )
    .expect("failed to record migration version");
    tx.commit().expect("failed to commit migration");
  }
}

/// Build a single [`Track`] with deterministic, distinguishable field values.
///
/// `file_hash` is the content fingerprint the sync diff compares, so tests can pass
/// matching hashes (unchanged) or divergent ones (needs update). The `id` is woven into the
/// file path so fixtures with different ids never share a path.
pub fn fixture_track(id: &str, title: &str, file_hash: &str) -> Track {
  Track {
    id: id.to_string(),
    file_path: format!("/music/{id}.mp3"),
    file_hash: Some(file_hash.to_string()),
    title: Some(title.to_string()),
    artist: Some("Test Artist".to_string()),
    album: Some("Test Album".to_string()),
    year: Some(2024),
    genre: Some("House".to_string()),
    label: None,
    catalog_number: None,
    duration_ms: 180_000,
    bpm: Some(120.0),
    key: Some("Am".to_string()),
    bitrate: Some(320),
    sample_rate: Some(44_100),
    format: "mp3".to_string(),
    analysis_source: None,
    waveform_data: None,
    rating: 0,
    play_count: 0,
    date_added: "2024-01-01T00:00:00Z".to_string(),
    date_modified: "2024-01-01T00:00:00Z".to_string(),
    last_played: None,
    rekordbox_id: None,
    url: None,
    artwork_path: None,
    artwork_source: None,
    color: None,
    library_root_id: None,
    relative_path: None,
    tags: vec![],
  }
}

/// A small, fixed set of sample tracks for import/scan/sync tests.
pub fn fixture_tracks() -> Vec<Track> {
  vec![
    fixture_track("track-1", "First Track", "hash-1"),
    fixture_track("track-2", "Second Track", "hash-2"),
    fixture_track("track-3", "Third Track", "hash-3"),
  ]
}

/// Build a [`Track`] whose every column carries a unique, recognizable sentinel value.
///
/// Track rows are read back through positional `row.get(index)` closures over explicit
/// SELECT lists. Because no two sentinel values repeat, comparing a read result against
/// this fixture detects ANY index permutation across all columns (not just `url`, the
/// column whose addition exposed the missing coverage).
pub fn sentinel_track() -> Track {
  Track {
    id: "sentinel-track-9".to_string(),
    file_path: "/music/sentinel/track-9.mp3".to_string(),
    file_hash: Some("sentinel-hash-09".to_string()),
    title: Some("s-title".to_string()),
    artist: Some("s-artist".to_string()),
    album: Some("s-album".to_string()),
    year: Some(1971),
    genre: Some("s-genre".to_string()),
    label: Some("s-label".to_string()),
    catalog_number: Some("s-catalog-9".to_string()),
    duration_ms: 2_345_678,
    bpm: Some(123.45),
    key: Some("s-key".to_string()),
    bitrate: Some(231),
    sample_rate: Some(32_100),
    format: "s-format".to_string(),
    analysis_source: Some("s-analysis".to_string()),
    waveform_data: Some(vec![9, 8, 7]),
    rating: 4,
    play_count: 9,
    date_added: "2021-02-03T04:05:06Z".to_string(),
    date_modified: "2022-03-04T05:06:07Z".to_string(),
    last_played: Some("2023-04-05T06:07:08Z".to_string()),
    rekordbox_id: Some("s-rekordbox-9".to_string()),
    url: Some("https://example.test/track-9".to_string()),
    artwork_path: Some("s-artwork-path".to_string()),
    artwork_source: Some("s-artwork-source".to_string()),
    color: Some("s-color".to_string()),
    library_root_id: Some("s-library-root".to_string()),
    relative_path: Some("s/relative/track-9.mp3".to_string()),
    tags: vec![],
  }
}

/// Persist `track` into the `tracks` table with an explicit column list (bypassing import
/// and file I/O), creating its `library_roots` parent first so the FK holds under
/// `PRAGMA foreign_keys = ON`.
pub fn insert_sentinel_track(conn: &Connection, track: &Track) {
  if let Some(root_id) = &track.library_root_id {
    conn
      .execute(
        "INSERT INTO library_roots (id, name) VALUES (?1, 'Sentinel root') \
         ON CONFLICT(id) DO NOTHING",
        [root_id],
      )
      .expect("failed to seed library root");
  }
  conn
    .execute(
      "INSERT INTO tracks (id, file_path, file_hash, title, artist, album, year, genre, label,
                           catalog_number, duration_ms, bpm, key, bitrate, sample_rate, format,
                           analysis_source, waveform_data, rating, play_count, date_added,
                           date_modified, last_played, rekordbox_id, artwork_path, artwork_source,
                           color, library_root_id, relative_path, url)
       VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
      rusqlite::params![
        track.id,
        track.file_path,
        track.file_hash,
        track.title,
        track.artist,
        track.album,
        track.year,
        track.genre,
        track.label,
        track.catalog_number,
        track.duration_ms,
        track.bpm,
        track.key,
        track.bitrate,
        track.sample_rate,
        track.format,
        track.analysis_source,
        track.waveform_data,
        track.rating,
        track.play_count,
        track.date_added,
        track.date_modified,
        track.last_played,
        track.rekordbox_id,
        track.artwork_path,
        track.artwork_source,
        track.color,
        track.library_root_id,
        track.relative_path,
        track.url,
      ],
    )
    .expect("failed to seed sentinel track");
}

/// Asserts each named field of the two values is equal, naming the field on failure.
macro_rules! assert_fields_eq {
  ($actual:expr, $expected:expr, $($field:ident),+ $(,)?) => {
    $({
      let (a, e) = ($actual, $expected);
      assert_eq!(a.$field, e.$field, concat!(stringify!($field), " mismatch"));
    })+
  };
}

/// Field-by-field [`Track`] equality with the drifted field named in the failure.
///
/// `Track` has no `PartialEq`, and a positional-read regression must report WHICH
/// column landed in WHICH field. Tags are compared field-wise too, covering the
/// `fetch_tags_for_tracks` closure.
pub fn assert_track_eq(actual: &Track, expected: &Track) {
  assert_fields_eq!(
    actual,
    expected,
    id,
    file_path,
    file_hash,
    title,
    artist,
    album,
    year,
    genre,
    label,
    catalog_number,
    duration_ms,
    bpm,
    key,
    bitrate,
    sample_rate,
    format,
    analysis_source,
    waveform_data,
    rating,
    play_count,
    date_added,
    date_modified,
    last_played,
    rekordbox_id,
    url,
    artwork_path,
    artwork_source,
    color,
    library_root_id,
    relative_path
  );
  assert_eq!(actual.tags.len(), expected.tags.len(), "tags length");
  for (actual_tag, expected_tag) in actual.tags.iter().zip(expected.tags.iter()) {
    assert_fields_eq!(
      actual_tag,
      expected_tag,
      id,
      category_id,
      name,
      color,
      sort_order
    );
  }
}

/// Project a [`Track`] into the [`BackupTrack`] shape the backup service reads positionally
/// (waveform/analysis/library-root columns are intentionally absent from backups).
pub fn backup_track_of(track: &Track) -> BackupTrack {
  BackupTrack {
    id: track.id.clone(),
    file_path: track.file_path.clone(),
    file_hash: track.file_hash.clone(),
    title: track.title.clone(),
    artist: track.artist.clone(),
    album: track.album.clone(),
    year: track.year,
    genre: track.genre.clone(),
    label: track.label.clone(),
    catalog_number: track.catalog_number.clone(),
    duration_ms: track.duration_ms,
    bpm: track.bpm,
    key: track.key.clone(),
    bitrate: track.bitrate,
    sample_rate: track.sample_rate,
    format: Some(track.format.clone()),
    rating: track.rating,
    play_count: track.play_count,
    date_added: track.date_added.clone(),
    date_modified: track.date_modified.clone(),
    last_played: track.last_played.clone(),
    rekordbox_id: track.rekordbox_id.clone(),
    artwork_path: track.artwork_path.clone(),
    artwork_source: track.artwork_source.clone(),
    color: track.color.clone(),
    url: track.url.clone(),
  }
}

/// Field-by-field [`BackupTrack`] equality — same rationale as [`assert_track_eq`]: the
/// backup SELECT is a second drift-prone positional list (`url` reads at index 25).
pub fn assert_backup_track_eq(actual: &BackupTrack, expected: &BackupTrack) {
  assert_fields_eq!(
    actual,
    expected,
    id,
    file_path,
    file_hash,
    title,
    artist,
    album,
    year,
    genre,
    label,
    catalog_number,
    duration_ms,
    bpm,
    key,
    bitrate,
    sample_rate,
    format,
    rating,
    play_count,
    date_added,
    date_modified,
    last_played,
    rekordbox_id,
    artwork_path,
    artwork_source,
    color,
    url
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use rusqlite::OptionalExtension;
  use tempfile::tempdir;

  #[test]
  fn make_test_db_migrates_an_encrypted_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("library.db");
    let conn = make_test_db(&path);

    let latest = get_migrations().len() as i64;
    let version: i64 = conn
      .query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
      )
      .unwrap();
    assert_eq!(version, latest);

    // The file must not be readable as a plaintext SQLite database, proving the key was
    // applied: a raw `SQLite format 3` header would mean the encryption was skipped.
    let magic = std::fs::read(&path).unwrap();
    assert!(!magic.starts_with(b"SQLite format 3\0"));
  }

  #[test]
  fn make_memory_db_has_the_full_schema() {
    let conn = make_memory_db();
    let tracks_exists: Option<i64> = conn
      .query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tracks'",
        [],
        |r| r.get(0),
      )
      .optional()
      .unwrap();
    assert_eq!(tracks_exists, Some(1));
  }

  /// The sentinel fixture is only drift-proof if no two column values repeat —
  /// otherwise an index swap could still compare equal against it.
  #[test]
  fn sentinel_track_values_are_pairwise_distinct() {
    let t = sentinel_track();
    let values: Vec<String> = vec![
      format!("{:?}", t.id),
      format!("{:?}", t.file_path),
      format!("{:?}", t.file_hash),
      format!("{:?}", t.title),
      format!("{:?}", t.artist),
      format!("{:?}", t.album),
      format!("{:?}", t.year),
      format!("{:?}", t.genre),
      format!("{:?}", t.label),
      format!("{:?}", t.catalog_number),
      format!("{:?}", t.duration_ms),
      format!("{:?}", t.bpm),
      format!("{:?}", t.key),
      format!("{:?}", t.bitrate),
      format!("{:?}", t.sample_rate),
      format!("{:?}", t.format),
      format!("{:?}", t.analysis_source),
      format!("{:?}", t.waveform_data),
      format!("{:?}", t.rating),
      format!("{:?}", t.play_count),
      format!("{:?}", t.date_added),
      format!("{:?}", t.date_modified),
      format!("{:?}", t.last_played),
      format!("{:?}", t.rekordbox_id),
      format!("{:?}", t.url),
      format!("{:?}", t.artwork_path),
      format!("{:?}", t.artwork_source),
      format!("{:?}", t.color),
      format!("{:?}", t.library_root_id),
      format!("{:?}", t.relative_path),
    ];
    let unique: std::collections::HashSet<&String> = values.iter().collect();
    assert_eq!(
      values.len(),
      unique.len(),
      "sentinel values must be distinct"
    );
  }

  /// Exercises the seeding + comparison helpers (they are compiled into every module
  /// that includes this file) and proves the seed lands where the named columns say.
  #[test]
  fn sentinel_seed_and_assert_helpers_are_consistent() {
    let track = sentinel_track();
    let conn = make_memory_db();
    insert_sentinel_track(&conn, &track);

    let (url, root_id): (Option<String>, Option<String>) = conn
      .query_row(
        "SELECT url, library_root_id FROM tracks WHERE id = ?1",
        [&track.id],
        |r| Ok((r.get(0)?, r.get(1)?)),
      )
      .unwrap();
    assert_eq!(url, track.url);
    assert_eq!(root_id, track.library_root_id);

    let read_back = conn
      .query_row(
        "SELECT id, url, waveform_data, color FROM tracks WHERE id = ?1",
        [&track.id],
        |r| {
          Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<Vec<u8>>>(2)?,
            r.get::<_, Option<String>>(3)?,
          ))
        },
      )
      .unwrap();
    assert_eq!(read_back.0, track.id);
    assert_eq!(read_back.1, track.url);
    assert_eq!(read_back.2, track.waveform_data);
    assert_eq!(read_back.3, track.color);

    let mut with_tag = track.clone();
    with_tag.tags = vec![crate::models::Tag {
      id: "s-tag".to_string(),
      category_id: "s-cat".to_string(),
      name: "s-tag-name".to_string(),
      color: Some("#112233".to_string()),
      sort_order: 92,
    }];
    assert_track_eq(&track, &track);
    assert_track_eq(&with_tag, &with_tag);

    let backup = backup_track_of(&track);
    assert_backup_track_eq(&backup, &backup_track_of(&with_tag));
  }

  #[test]
  fn fixture_tracks_are_deterministic_and_distinct() {
    let tracks = fixture_tracks();
    assert_eq!(tracks.len(), 3);
    assert_eq!(tracks[0].id, "track-1");
    assert_eq!(tracks[0].file_hash.as_deref(), Some("hash-1"));

    let ids: std::collections::HashSet<&str> = tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids.len(), tracks.len());
    assert!(tracks.iter().all(|t| t.file_hash.is_some()));
  }
}
