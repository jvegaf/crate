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
use crate::models::Track;

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
