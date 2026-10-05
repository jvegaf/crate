pub mod handle;
mod key_provider;
pub mod schema;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::error::{CrateError, Result};

pub use handle::Db;
use handle::ReaderPool;

/// Number of read-only pooled connections. Small on purpose: WAL readers never block
/// each other or the writer, so this only needs to cover concurrent hot paths (the
/// stream proxy + a feed query + one spare). Each open pays the SQLCipher KDF
/// (~50-300 ms), so they open once at startup, in parallel.
const READER_POOL_SIZE: usize = 3;

pub struct Database {
    conn: Arc<Mutex<Connection>>,
    readers: Option<Arc<ReaderPool>>,
}

/// Check whether a database file is unencrypted by attempting to read its header.
/// An unencrypted SQLite database starts with "SQLite format 3\0".
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn is_unencrypted(db_path: &std::path::Path) -> bool {
    std::fs::read(db_path)
        .map(|bytes| bytes.starts_with(b"SQLite format 3\0"))
        .unwrap_or(false)
}

/// Migrate an existing unencrypted database to an encrypted one using `sqlcipher_export`.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn migrate_to_encrypted(db_path: &std::path::Path, key: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    let encrypted_path = db_path.with_extension("db.encrypted");

    conn.execute_batch(&format!(
        "ATTACH DATABASE '{}' AS encrypted KEY '{}';
         SELECT sqlcipher_export('encrypted');
         DETACH DATABASE encrypted;",
        encrypted_path.display(),
        key
    ))?;

    drop(conn);
    std::fs::rename(&encrypted_path, db_path)?;
    Ok(())
}

/// Migrate a pre-existing unencrypted database to encrypted form, if needed (desktop only).
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn migrate_if_unencrypted(db_path: &std::path::Path, key: &str) -> Result<()> {
    if db_path.exists() && is_unencrypted(db_path) {
        log::info!("Migrating unencrypted database to encrypted format");
        migrate_to_encrypted(db_path, key)?;
    }
    Ok(())
}

/// Mobile databases are encrypted from creation, so there is never anything to migrate.
#[cfg(any(target_os = "ios", target_os = "android"))]
fn migrate_if_unencrypted(_db_path: &std::path::Path, _key: &str) -> Result<()> {
    Ok(())
}

/// Apply the per-connection pragma set. The SQLCipher `key` MUST be the first statement
/// on every connection. The writer additionally sets `journal_mode=WAL` (persistent in
/// the DB file, verified — SQLCipher encrypts WAL frames) and `synchronous=NORMAL`
/// (safe under WAL, skips the per-commit main-file fsync). Returns whether WAL is
/// active; when it isn't (exotic filesystem), the caller skips the reader pool and
/// everything degrades to the single-connection behavior.
///
/// NOTE: WAL means `crate.db` alone is not the whole database on disk (`-wal`/`-shm`
/// live next to it). Any future feature that file-copies the DB must run
/// `PRAGMA wal_checkpoint(TRUNCATE)` first (see the exit handler in `lib.rs`) or copy
/// all three files. Current backup (row-level JSON), cloud sync (bucket blobs), and
/// device export (separate DB file) are unaffected.
fn configure_connection(conn: &Connection, key: &str, writer: bool) -> Result<bool> {
    conn.pragma_update(None, "key", key)?;
    let mut wal_active = true;
    if writer {
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        wal_active = mode.eq_ignore_ascii_case("wal");
        if wal_active {
            conn.pragma_update(None, "synchronous", "NORMAL")?;
        } else {
            log::warn!("journal_mode=WAL not applied (got {mode}); reader pool disabled");
        }
    }
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(wal_active)
}

/// Open the read-only reader pool in parallel threads (each open pays the SQLCipher
/// KDF). Best-effort: any failure disables the pool rather than failing startup.
fn open_reader_pool(db_path: &std::path::Path, key: &str) -> Option<Arc<ReaderPool>> {
    let handles: Vec<_> = (0..READER_POOL_SIZE)
        .map(|_| {
            let path = db_path.to_path_buf();
            let key = key.to_string();
            std::thread::spawn(move || -> Result<Connection> {
                let conn = Connection::open_with_flags(
                    &path,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                configure_connection(&conn, &key, false)?;
                Ok(conn)
            })
        })
        .collect();
    let mut conns = Vec::with_capacity(READER_POOL_SIZE);
    for h in handles {
        match h.join() {
            Ok(Ok(conn)) => conns.push(conn),
            Ok(Err(e)) => {
                log::warn!("db: reader connection open failed ({e}); reader pool disabled");
                return None;
            }
            Err(_) => {
                log::warn!("db: reader connection open panicked; reader pool disabled");
                return None;
            }
        }
    }
    Some(Arc::new(ReaderPool::new(conns)))
}

impl Database {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        Self::new_inner(db_path, true)
    }

    /// Writer-only open (no reader pool): for short-lived headless contexts (the Android
    /// WorkManager sync) where pooled readers would never be used but each would still
    /// pay the SQLCipher KDF.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn new_writer_only(db_path: PathBuf) -> Result<Self> {
        Self::new_inner(db_path, false)
    }

    fn new_inner(db_path: PathBuf, open_readers: bool) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let app_data_dir = db_path.parent().ok_or_else(|| {
            CrateError::KeyStorage("database path has no parent directory".to_string())
        })?;
        let key = key_provider::for_platform(app_data_dir).get_or_create_key()?;

        // If an existing database is unencrypted, migrate it (desktop only). Mobile
        // databases are encrypted from creation, so this is a no-op there.
        migrate_if_unencrypted(&db_path, &key)?;

        let conn = Connection::open(&db_path)?;
        let wal_active = configure_connection(&conn, &key, true)?;
        log::info!(
            "db: opened writer (journal_mode={})",
            if wal_active { "wal" } else { "legacy" }
        );

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            readers: None,
        };

        // Run migrations on the writer BEFORE opening readers (a read-only connection
        // can't create the schema, and WAL's `-shm` needs a live writer first).
        db.migrate()?;

        let readers = if wal_active && open_readers {
            open_reader_pool(&db_path, &key)
        } else {
            None
        };
        Ok(Self { readers, ..db })
    }

    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    /// The read/write-split handle: pooled snapshot reads + the shared writer.
    pub fn handle(&self) -> Db {
        Db::new(self.conn.clone(), self.readers.clone())
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        run_migrations(&conn)
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            readers: self.readers.clone(),
        }
    }
}

/// Whether a table exists in the schema.
fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Whether an index exists in the schema.
fn index_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Whether a column exists on a table. `table` comes from a static footprint in `schema.rs`,
/// never from user input.
fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether a single declared effect is present.
fn effect_satisfied(conn: &Connection, effect: &schema::Effect) -> Result<bool> {
    match effect {
        schema::Effect::Table(name) => table_exists(conn, name),
        schema::Effect::Column { table, column } => column_exists(conn, table, column),
        schema::Effect::Index(name) => index_exists(conn, name),
        // A data repair is satisfied once its probe stops matching rows. Reading the data
        // instead of the version counter is what keeps the repair due on a drifted database.
        schema::Effect::NoRows(probe) => {
            Ok(conn.query_row(probe, [], |_| Ok(())).optional()?.is_none())
        }
    }
}

/// Whether every effect a migration declares is already present.
fn effects_satisfied(conn: &Connection, effects: &[schema::Effect]) -> Result<bool> {
    for effect in effects {
        if !effect_satisfied(conn, effect)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Apply any pending schema migrations to `conn`, reconciled by effect and atomic.
///
/// Each migration's SQL and its `schema_version` bump commit together in one transaction, so an
/// interrupted run (e.g. the process is killed mid-migration) rolls back cleanly and is retried
/// from scratch on the next launch — never leaving a half-applied schema.
///
/// A migration runs when a declared effect is missing (DDL) or when the counter says it is still
/// due (data repairs, and any entry that declares no footprint). The counter is therefore an
/// optimization, never the source of truth: a database whose numbering drifted — a dev branch
/// that renumbered migrations, a restored backup, a hand-edited file — carries some later
/// migrations' effects under lower numbers, and a version-only gate would replay
/// `ALTER TABLE ... ADD COLUMN` on columns that already exist and panic the app at setup.
fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
        [],
    )?;

    let migrations = schema::get_migrations();

    let current_version: i32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;

    for (idx, migration) in migrations.iter().enumerate() {
        let version = idx as i32 + 1;
        let due = if migration.effects.is_empty() {
            version > current_version
        } else {
            !effects_satisfied(conn, migration.effects)?
        };
        if due {
            log::info!("Running migration {version}");
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(migration.sql)?;
            // `OR REPLACE`: a migration whose effect was missing can run while its version row
            // already exists (a renumbered or restored database), and re-recording a number it
            // already occupies is a no-op under `MAX(version)`, not an error.
            tx.execute(
                "INSERT OR REPLACE INTO schema_version (version) VALUES (?1)",
                [version],
            )?;
            tx.commit()?;
        }
    }

    // Every migration is satisfied by now, so record the head. A reconciled database must not
    // keep a counter that describes some other chain: `MAX()` makes this monotone, so a database
    // whose counter is genuinely ahead of this chain is left alone.
    conn.execute(
        "INSERT OR REPLACE INTO schema_version (version) VALUES (?1)",
        [migrations.len() as i32],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;

    fn open_mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()
        .unwrap()
        .is_some()
    }

    fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        cols.iter().any(|c| c == column)
    }

    fn index_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()
        .unwrap()
        .is_some()
    }

    fn version(conn: &Connection) -> i32 {
        conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// Every sync table + rooting/`_hlc` column that migrations 3–4 must create.
    fn assert_sync_schema(conn: &Connection) {
        for t in [
            "library_roots",
            "sync_root_mappings",
            "sync_tombstones",
            "sync_dirty_buckets",
            "sync_state",
        ] {
            assert!(table_exists(conn, t), "expected table `{t}` to exist");
        }
        for (tbl, col) in [
            ("tracks", "_hlc"),
            ("tracks", "library_root_id"),
            ("tracks", "relative_path"),
            ("playlists", "_hlc"),
            ("playlist_tracks", "_hlc"),
            ("cues", "_hlc"),
            ("tag_categories", "_hlc"),
            ("tags", "_hlc"),
            ("track_tags", "_hlc"),
            ("discovery_releases", "_hlc"),
            ("discovery_tracks", "_hlc"),
            ("discovery_release_tags", "_hlc"),
            ("playlist_discovery_releases", "_hlc"),
            ("playlist_discovery_tracks", "_hlc"),
            ("discovery_track_tags", "_hlc"),
        ] {
            assert!(
                column_exists(conn, tbl, col),
                "expected column `{tbl}.{col}` to exist"
            );
        }
    }

    #[test]
    fn fresh_db_migrates_to_latest_and_reruns_cleanly() {
        let conn = open_mem();
        run_migrations(&conn).unwrap();

        assert_sync_schema(&conn);
        // Migration 9 adds the store page URL column (`tracks.url`).
        assert!(
            column_exists(&conn, "tracks", "url"),
            "expected column `tracks.url` to exist"
        );
        let latest = schema::get_migrations().len() as i32;
        assert_eq!(version(&conn), latest);

        // Re-running must be a version-gated no-op, never an error.
        run_migrations(&conn).unwrap();
        assert_eq!(version(&conn), latest);
    }

    #[test]
    fn repairs_bitrates_written_as_bit_depths() {
        fn insert_track(conn: &Connection, suffix: usize, bitrate: Option<i64>) -> String {
            let id = format!("bitrate-repair-{suffix}");
            conn
        .execute(
          "INSERT INTO tracks (id, file_path, duration_ms, date_added, date_modified, bitrate) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
          rusqlite::params![
            id,
            format!("/music/bitrate-repair-{suffix}.mp3"),
            180_000,
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00Z",
            bitrate,
          ],
        )
        .unwrap();
            id
        }

        let conn = open_mem();
        let migrations = schema::get_migrations();
        // The bitrate repair is migration 8 (index 7). Migrations are append-only, so its
        // position is stable regardless of later additions; do not re-derive it from the length.
        const REPAIR_MIGRATION_INDEX: usize = 7;
        assert!(migrations.len() > REPAIR_MIGRATION_INDEX);

        // Simulate a database at the version immediately before the data-repair migration.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
            [],
        )
        .unwrap();
        for (idx, migration) in migrations.iter().take(REPAIR_MIGRATION_INDEX).enumerate() {
            conn.execute_batch(migration.sql).unwrap();
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [(idx as i32) + 1],
            )
            .unwrap();
        }

        let cases = [
            (Some(8), None),
            (Some(16), None),
            (Some(24), None),
            (Some(32), None),
            (Some(95), None),
            (Some(96), Some(96)),
            (Some(128), Some(128)),
            (Some(320), Some(320)),
            (Some(1411), Some(1411)),
            (None, None),
        ];
        let track_ids: Vec<String> = cases
            .iter()
            .enumerate()
            .map(|(suffix, (bitrate, _))| insert_track(&conn, suffix, *bitrate))
            .collect();

        // Applying all migrations repairs corrupted values and bumps the schema version.
        run_migrations(&conn).unwrap();
        assert_eq!(version(&conn), schema::get_migrations().len() as i32);

        for (id, (_, expected_bitrate)) in track_ids.iter().zip(cases) {
            let actual_bitrate: Option<i64> = conn
                .query_row("SELECT bitrate FROM tracks WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(actual_bitrate, expected_bitrate, "track {id}");
        }

        // Reapplying the repair SQL is an idempotent no-op: no rows match after repair.
        conn.execute_batch(migrations[REPAIR_MIGRATION_INDEX].sql)
            .unwrap();
        assert_eq!(conn.changes(), 0);
        for (id, (_, expected_bitrate)) in track_ids.iter().zip(cases) {
            let actual_bitrate: Option<i64> = conn
                .query_row("SELECT bitrate FROM tracks WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(actual_bitrate, expected_bitrate, "track {id} after rerun");
        }
    }

    #[test]
    fn existing_v2_database_upgrades_cleanly() {
        let conn = open_mem();

        // Simulate a shipped (schema v2) database: apply only migrations 1 & 2.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
            [],
        )
        .unwrap();
        for (idx, migration) in schema::get_migrations().iter().take(2).enumerate() {
            conn.execute_batch(migration.sql).unwrap();
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [(idx as i32) + 1],
            )
            .unwrap();
        }
        assert_eq!(version(&conn), 2);
        assert!(!table_exists(&conn, "sync_state"));

        // Upgrading applies only the new migrations (3 & 4), atomically.
        run_migrations(&conn).unwrap();
        assert_sync_schema(&conn);
        assert_eq!(version(&conn), schema::get_migrations().len() as i32);
    }

    /// Guards the footprints themselves: an effect a migration's SQL never actually creates would
    /// stay unsatisfied forever, re-running that migration on every launch (a crash on the next
    /// `ALTER TABLE ... ADD COLUMN`).
    #[test]
    fn fresh_db_satisfies_every_declared_effect() {
        let conn = open_mem();
        run_migrations(&conn).unwrap();

        for (idx, migration) in schema::get_migrations().iter().enumerate() {
            assert!(
                effects_satisfied(&conn, migration.effects).unwrap(),
                "migration {} declares an effect its SQL does not create",
                idx + 1
            );
        }
    }

    /// A database that lost a column while reporting the head version gets it back: the counter
    /// is not evidence about the schema, only about what has run so far.
    #[test]
    fn drifted_schema_reconciles_despite_a_current_counter() {
        let conn = open_mem();
        run_migrations(&conn).unwrap();
        assert_eq!(version(&conn), schema::get_migrations().len() as i32);

        conn.execute("ALTER TABLE tracks DROP COLUMN url", [])
            .unwrap();
        assert!(!column_exists(&conn, "tracks", "url"));

        run_migrations(&conn).unwrap();
        assert!(column_exists(&conn, "tracks", "url"));
    }
    /// effects of later migrations under a *lower* version, and is missing earlier ones.
    ///
    /// That chain was `[0..=5] + [9..=17]` over the current list (0-based): `tracks.url` (index 8)
    /// and `idx_tracks_file_hash` (index 6) were never applied, while `last_walk_complete`
    /// (index 15) landed as its 13th migration. The database reports version 15 — high enough that
    /// a purely version-gated runner replays DDL it already ran and dies on
    /// `duplicate column name: last_walk_complete`, panicking the Tauri setup hook.
    #[test]
    fn renumbered_chain_recovers_cleanly() {
        const LEGACY_ORDER: [usize; 15] = [0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 14, 15, 16, 17];
        const CORRUPT_BITRATE: i64 = 16; // a bit depth written into `bitrate`

        let conn = open_mem();
        let migrations = schema::get_migrations();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
            [],
        )
        .unwrap();
        for (idx, migration) in LEGACY_ORDER.iter().enumerate() {
            conn.execute_batch(migrations[*migration].sql).unwrap();
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [(idx as i32) + 1],
            )
            .unwrap();
        }
        // The drift, stated as facts about the database rather than about numbers:
        assert_eq!(version(&conn), 15);
        assert!(!column_exists(&conn, "tracks", "url"));
        assert!(!index_exists(&conn, "idx_tracks_file_hash"));
        assert!(column_exists(
            &conn,
            "collection_account_state",
            "last_walk_complete"
        ));

        conn.execute(
            "INSERT INTO tracks (id, file_path, duration_ms, date_added, date_modified, bitrate) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                "renumbered-repair",
                "/music/renumbered.mp3",
                180_000,
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                CORRUPT_BITRATE,
            ],
        )
        .unwrap();

        // Must reconcile by effect instead of replaying `ADD COLUMN` on existing columns.
        run_migrations(&conn).unwrap();

        assert!(column_exists(&conn, "tracks", "url"));
        assert!(index_exists(&conn, "idx_tracks_file_hash"));
        let bitrate: Option<i64> = conn
            .query_row(
                "SELECT bitrate FROM tracks WHERE id = ?1",
                ["renumbered-repair"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            bitrate, None,
            "the data repair must run even though the counter says its version passed"
        );
        assert_eq!(version(&conn), schema::get_migrations().len() as i32);
    }
}
