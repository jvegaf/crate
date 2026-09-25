//! Recursive scan of the device's configured music folder.
//!
//! The user picks one folder per device; it is stored as the device-local mapping of a
//! well-known logical library root. Scanning walks that tree, imports every supported
//! audio file, and silently skips files whose content hash already exists (no duplicate
//! payloads are created).

use super::*;
use crate::services::cloud_sync::resolution;

use walkdir::WalkDir;

/// Files imported within a single transaction.
///
/// Hashing happens before the database lock is taken; the whole chunk then shares one
/// `unchecked_transaction`, so a first import commits once per chunk instead of once per
/// file. That matters because the SQLCipher `journal_mode=DELETE` / `synchronous=FULL`
/// defaults make every implicit commit a journal write plus fsyncs.
const SCAN_BATCH_SIZE: usize = 32;

/// Logical library root that represents this device's music folder.
pub const LOCAL_MUSIC_ROOT_ID: &str = "local-music-library";
/// Name stored on that root. It syncs to other devices; each device maps its own folder.
pub const LOCAL_MUSIC_ROOT_NAME: &str = "Music Library";

impl LibraryService {
    /// The local absolute folder mapped to this device's music library root, if set.
    pub fn music_library_folder(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        resolution::root_mapping(&conn, LOCAL_MUSIC_ROOT_ID)
    }

    /// Persist the device-local music folder for the well-known root.
    ///
    /// Returns the canonical value that was actually stored, so the caller sees exactly
    /// what later scans will read back.
    pub fn set_music_library_folder(&self, path: &str) -> Result<String> {
        if !std::path::Path::new(path).is_dir() {
            return Err(CrateError::InvalidOperation(format!(
                "Not a directory: {path}"
            )));
        }

        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        resolution::ensure_root(&conn, LOCAL_MUSIC_ROOT_ID, LOCAL_MUSIC_ROOT_NAME)?;
        resolution::set_root_mapping(&conn, LOCAL_MUSIC_ROOT_ID, path)?;
        resolution::root_mapping(&conn, LOCAL_MUSIC_ROOT_ID)?.ok_or_else(|| {
            CrateError::InvalidOperation("Failed to persist music library folder".to_string())
        })
    }

    /// Recursively import every supported audio file under the configured music folder.
    ///
    /// Files whose content hash already exists in the library are skipped silently — no
    /// duplicate payload is created and no duplicate flow is opened.
    ///
    /// Retained as the no-progress entry point (used by tests); the Tauri command calls
    /// [`Self::scan_music_library_folder_with_progress`] so it can emit live progress.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn scan_music_library_folder(&self) -> Result<LibraryFolderScanResult> {
        self.scan_music_library_folder_with_progress(|_| {})
    }

    /// Recursively import every supported audio file, reporting live progress.
    ///
    /// Identical to [`Self::scan_music_library_folder`] but invokes `on_progress` once per
    /// processed file (never per chunk) so a caller can render a live counter. The whole
    /// scan stays synchronous: the connection guard is never held across an `.await`.
    pub fn scan_music_library_folder_with_progress<F>(
        &self,
        mut on_progress: F,
    ) -> Result<LibraryFolderScanResult>
    where
        F: FnMut(&LibraryScanProgress),
    {
        let root = self.music_library_folder()?.ok_or_else(|| {
            CrateError::InvalidOperation("No music library folder configured".to_string())
        })?;

        if !std::path::Path::new(&root).is_dir() {
            return Err(CrateError::InvalidOperation(format!(
                "Music library folder is unavailable: {root}"
            )));
        }

        let mut result = LibraryFolderScanResult {
            scanned_count: 0,
            imported_count: 0,
            skipped_existing_count: 0,
            failed_count: 0,
            imported_track_ids: Vec::new(),
            errors: Vec::new(),
        };

        // Walk without following symlinks: this avoids symlink cycles and guarantees every
        // produced path stays under the canonical root prefix, so `strip_prefix` matches.
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in WalkDir::new(&root).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    result.failed_count += 1;
                    result.errors.push(e.to_string());
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let extension = entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            if !SUPPORTED_AUDIO_EXTENSIONS.contains(&extension.as_str()) {
                continue;
            }
            files.push(entry.into_path());
        }

        // Sort so the run (and its result) is deterministic and testable.
        files.sort();
        result.scanned_count = files.len();

        let mut processed = 0usize;

        for chunk in files.chunks(SCAN_BATCH_SIZE) {
            // Hash every file before taking the lock: file I/O must not extend the
            // critical section. A hash failure is a per-file failure, as before.
            let mut prepared: Vec<(PathBuf, Option<String>)> = Vec::with_capacity(chunk.len());
            for path in chunk {
                match compute_audio_hash(path) {
                    Ok(hash) => prepared.push((path.clone(), Some(hash))),
                    Err(e) => {
                        result.failed_count += 1;
                        result.errors.push(format!("{}: {e}", path.display()));
                        prepared.push((path.clone(), None));
                    }
                }
            }

            // One guard, one transaction per chunk. The guard is scoped to this block and
            // is never held across an `.await` (the whole scan is synchronous).
            {
                let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
                let tx = conn.unchecked_transaction()?;

                for (path, hash) in prepared {
                    if let Some(hash) = hash {
                        // The transaction makes this run's own uncommitted inserts visible
                        // to this lookup, so a duplicate inside the chunk is still caught
                        // (and one straddling a chunk boundary is caught by the commit).
                        // No separate seen-hash set is needed, and a copy whose insert
                        // failed leaves the next one free to import — matching the
                        // previous per-file order.
                        //
                        // Absorb a lookup error like any other per-file failure:
                        // propagating it would discard the counts and imported ids
                        // already accumulated for this run.
                        match Self::find_track_by_hash_in(&tx, &hash) {
                            Ok(Some(_)) => {
                                result.skipped_existing_count += 1;
                            }
                            Ok(None) => match self.build_track_from_file(&path, hash) {
                                Ok(track) => match Self::insert_track_in(&tx, &track) {
                                    Ok(()) => {
                                        result.imported_count += 1;
                                        result.imported_track_ids.push(track.id);
                                    }
                                    Err(e) => {
                                        result.failed_count += 1;
                                        result.errors.push(format!("{}: {e}", path.display()));
                                    }
                                },
                                Err(e) => {
                                    result.failed_count += 1;
                                    result.errors.push(format!("{}: {e}", path.display()));
                                }
                            },
                            Err(e) => {
                                result.failed_count += 1;
                                result.errors.push(format!("{}: {e}", path.display()));
                            }
                        }
                    }

                    processed += 1;
                    on_progress(&LibraryScanProgress {
                        current: processed,
                        total: result.scanned_count,
                        imported_count: result.imported_count,
                        skipped_existing_count: result.skipped_existing_count,
                        failed_count: result.failed_count,
                        current_file: path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(str::to_string),
                    });
                }

                tx.commit()?;
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use rusqlite::Connection;

    /// Fresh in-memory device with the real schema applied and FKs enforced.
    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        for sql in crate::db::schema::get_migrations() {
            conn.execute_batch(sql).unwrap();
        }
        conn
    }

    /// Unique temp directory; callers remove it with `remove_dir_all`.
    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("crate-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Build a service whose music folder points at `<tmp>/music`, returning the service,
    /// the temp root to clean up, and the music folder itself.
    fn service_with_root() -> (LibraryService, PathBuf, PathBuf) {
        let tmp = temp_dir();
        let root = tmp.join("music");
        let app_data = tmp.join("appdata");
        std::fs::create_dir_all(&root).unwrap();

        let service = LibraryService::new(Arc::new(Mutex::new(test_conn())), app_data);
        service
            .set_music_library_folder(root.to_str().unwrap())
            .unwrap();

        (service, tmp, root)
    }

    /// Write a minimal valid 16-bit mono PCM WAV (44-byte header + `frames` sample frames).
    ///
    /// `seed` varies the sample bytes: equal seeds produce byte-identical files (so they
    /// hash equal), different seeds produce different hashes. `lofty` may parse this and,
    /// if it does not, `read_metadata_lenient` returns `None` and the symphonia fallback
    /// probes it — both paths need a genuinely decodable file.
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

        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(&bytes).unwrap();
    }

    #[test]
    fn scan_imports_every_new_file() {
        let (service, tmp, root) = service_with_root();
        let count = 5;
        for i in 0..count {
            write_wav(&root.join(format!("track-{i}.wav")), i as u8, 512);
        }

        let result = service.scan_music_library_folder().unwrap();

        assert_eq!(result.scanned_count, count);
        assert_eq!(result.imported_count, count);
        assert_eq!(result.imported_track_ids.len(), count);
        assert_eq!(result.skipped_existing_count, 0);
        assert_eq!(result.failed_count, 0);
        assert!(result.errors.is_empty());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn rescan_skips_every_existing_file() {
        let (service, tmp, root) = service_with_root();
        let count = 5;
        for i in 0..count {
            write_wav(&root.join(format!("track-{i}.wav")), i as u8, 512);
        }

        let first = service.scan_music_library_folder().unwrap();
        assert_eq!(first.imported_count, count);

        let second = service.scan_music_library_folder().unwrap();

        assert_eq!(second.scanned_count, count);
        assert_eq!(second.imported_count, 0);
        assert_eq!(second.skipped_existing_count, count);
        assert_eq!(second.failed_count, 0);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn duplicate_content_within_one_chunk_imports_once() {
        let (service, tmp, root) = service_with_root();
        write_wav(&root.join("a.wav"), 3, 512);
        write_wav(&root.join("b.wav"), 3, 512);

        let result = service.scan_music_library_folder().unwrap();

        assert_eq!(result.scanned_count, 2);
        assert_eq!(result.imported_count, 1);
        assert_eq!(result.imported_track_ids.len(), 1);
        assert_eq!(result.skipped_existing_count, 1);
        assert_eq!(result.failed_count, 0);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn scan_imports_more_files_than_one_batch() {
        let (service, tmp, root) = service_with_root();
        let count = SCAN_BATCH_SIZE + 8;
        for i in 0..count {
            write_wav(&root.join(format!("track-{i}.wav")), i as u8, 512);
        }

        let result = service.scan_music_library_folder().unwrap();

        assert_eq!(result.scanned_count, count);
        assert_eq!(result.imported_count, count);
        assert_eq!(result.imported_track_ids.len(), count);
        assert_eq!(result.skipped_existing_count, 0);
        assert_eq!(result.failed_count, 0);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn progress_runs_once_per_file_and_ends_at_total() {
        let (service, tmp, root) = service_with_root();
        let count = 5;
        for i in 0..count {
            write_wav(&root.join(format!("track-{i}.wav")), i as u8, 512);
        }

        let mut seen: Vec<LibraryScanProgress> = Vec::new();
        let result = service
            .scan_music_library_folder_with_progress(|progress| seen.push(progress.clone()))
            .unwrap();

        assert_eq!(seen.len(), count);
        assert_eq!(
            seen.iter().map(|p| p.current).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert_eq!(seen.last().unwrap().current, result.scanned_count);
        assert_eq!(seen.last().unwrap().total, result.scanned_count);
        assert!(seen.iter().all(|p| p.current_file.is_some()));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
