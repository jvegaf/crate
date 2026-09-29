use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::ToSql;
use uuid::Uuid;

use super::*;
use crate::services::cloud_sync::pipeline::{buckets, dirty};

impl LibraryService {
    /// Update one track's metadata after staging requested embedded edits.
    ///
    /// The audio file is written to a sibling staging file first. After replacement, a detected
    /// SQLite failure restores the original audio file. SQLite and filesystem changes are not
    /// crash-atomic; a process or machine failure between those operations can leave them divergent.
    pub fn update_track_metadata(&self, id: &str, patch: TrackMetadataPatch) -> Result<Track> {
        let mut track = self.get_track(id)?;
        let media_path = PathBuf::from(&track.file_path);
        let staged_media = if patch.has_embedded_edits() {
            Some(stage_media_file(&self.file_tags, &media_path, &patch)?)
        } else {
            None
        };

        let backup_path = if staged_media.is_some() {
            Some(unique_sibling_path(&media_path, "crate-backup"))
        } else {
            None
        };

        if let (Some(staged), Some(backup)) = (&staged_media, &backup_path) {
            if let Err(error) = replace_with_staged_file(&media_path, staged, backup) {
                let _ = fs::remove_file(staged);
                return Err(error);
            }
        }

        let date_modified = match self.persist_metadata_patch(id, &patch) {
            Ok(date_modified) => date_modified,
            Err(error) => {
                if let Some(backup) = &backup_path {
                    if let Err(rollback_error) = restore_original_file(&media_path, backup) {
                        return Err(CrateError::FileTags(format!(
              "Metadata database update failed ({error}); audio rollback also failed ({rollback_error}); original audio backup retained at {}",
              backup.display()
            )));
                    }
                }
                return Err(error);
            }
        };

        if let Some(backup) = backup_path {
            if let Err(error) = fs::remove_file(&backup) {
                log::warn!(
                    "Metadata update succeeded but temporary backup cleanup failed for {}: {error}",
                    backup.display()
                );
            }
        }

        apply_patch_to_track(&mut track, &patch);
        track.date_modified = date_modified;
        Ok(track)
    }

    fn persist_metadata_patch(&self, id: &str, patch: &TrackMetadataPatch) -> Result<String> {
        let mut conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let transaction = conn.transaction()?;
        let date_modified = chrono::Utc::now().to_rfc3339();
        let mut updates = vec!["date_modified = ?1".to_string()];
        let mut params: Vec<Box<dyn ToSql>> = vec![Box::new(date_modified.clone())];

        add_patch_value(&mut updates, &mut params, "title", &patch.title);
        add_patch_value(&mut updates, &mut params, "artist", &patch.artist);
        add_patch_value(&mut updates, &mut params, "album", &patch.album);
        add_patch_value(&mut updates, &mut params, "year", &patch.year);
        add_patch_value(&mut updates, &mut params, "genre", &patch.genre);
        add_patch_value(&mut updates, &mut params, "label", &patch.label);
        add_patch_value(
            &mut updates,
            &mut params,
            "catalog_number",
            &patch.catalog_number,
        );
        add_patch_value(&mut updates, &mut params, "bpm", &patch.bpm);
        add_patch_value(&mut updates, &mut params, "key", &patch.key);
        add_rating_patch_value(&mut updates, &mut params, &patch.rating);

        let hlc = dirty::next_hlc(&transaction)?;
        updates.push(format!("_hlc = ?{}", params.len() + 1));
        params.push(Box::new(hlc));
        params.push(Box::new(id.to_string()));

        let sql = format!(
            "UPDATE tracks SET {} WHERE id = ?{}",
            updates.join(", "),
            params.len()
        );
        let params_refs: Vec<&dyn ToSql> = params.iter().map(|value| value.as_ref()).collect();
        let changed = transaction.execute(&sql, params_refs.as_slice())?;
        if changed == 0 {
            return Err(CrateError::TrackNotFound(id.to_string()));
        }
        dirty::mark_dirty(&transaction, &buckets::bucket_for_track_id(id))?;
        transaction.commit()?;
        Ok(date_modified)
    }
}

fn add_patch_value<T: ToSql + Clone + 'static>(
    updates: &mut Vec<String>,
    params: &mut Vec<Box<dyn ToSql>>,
    column: &str,
    field: &MetadataField<T>,
) {
    match field {
        MetadataField::Unchanged => {}
        MetadataField::Set(value) => {
            updates.push(format!("{column} = ?{}", params.len() + 1));
            params.push(Box::new(value.clone()));
        }
        MetadataField::Clear => {
            updates.push(format!("{column} = ?{}", params.len() + 1));
            params.push(Box::new(Option::<T>::None));
        }
    }
}

fn add_rating_patch_value(
    updates: &mut Vec<String>,
    params: &mut Vec<Box<dyn ToSql>>,
    field: &MetadataField<i32>,
) {
    match field {
        MetadataField::Unchanged => {}
        MetadataField::Set(value) => {
            updates.push(format!("rating = ?{}", params.len() + 1));
            params.push(Box::new(*value));
        }
        MetadataField::Clear => {
            updates.push(format!("rating = ?{}", params.len() + 1));
            params.push(Box::new(0_i32));
        }
    }
}

fn stage_media_file(
    file_tags: &FileTagsService,
    original_path: &Path,
    patch: &TrackMetadataPatch,
) -> Result<PathBuf> {
    if !original_path.is_file() {
        return Err(CrateError::FileNotFound(original_path.to_path_buf()));
    }

    let staged_path = unique_sibling_path(original_path, "crate-stage");
    fs::copy(original_path, &staged_path).map_err(|error| {
        CrateError::FileTags(format!(
            "Failed to stage audio file {}: {error}",
            original_path.display()
        ))
    })?;

    if let Err(error) = file_tags.write_metadata_patch(&staged_path, patch) {
        let _ = fs::remove_file(&staged_path);
        return Err(error);
    }
    Ok(staged_path)
}

fn unique_sibling_path(original: &Path, role: &str) -> PathBuf {
    let extension = original.extension().and_then(|value| value.to_str());
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("audio");
    let name = match extension {
        Some(extension) => format!("{stem}.{role}-{}.{}", Uuid::new_v4(), extension),
        None => format!("{stem}.{role}-{}", Uuid::new_v4()),
    };
    original.with_file_name(name)
}

fn replace_with_staged_file(original: &Path, staged: &Path, backup: &Path) -> Result<()> {
    replace_with_staged_file_using(original, staged, backup, |from, to| fs::rename(from, to))
}

fn replace_with_staged_file_using<F>(
    original: &Path,
    staged: &Path,
    backup: &Path,
    mut rename: F,
) -> Result<()>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    rename(original, backup).map_err(|error| {
        CrateError::FileTags(format!(
            "Failed to preserve original audio file {} before replacement: {error}",
            original.display()
        ))
    })?;
    if let Err(error) = rename(staged, original) {
        let rollback = rename(backup, original);
        return Err(CrateError::FileTags(match rollback {
      Ok(()) => format!("Failed to replace audio file {}: {error}", original.display()),
      Err(rollback_error) => format!(
        "Failed to replace audio file {} ({error}); restoring the original also failed ({rollback_error}); original audio backup retained at {}",
        original.display(),
        backup.display()
      ),
    }));
    }
    Ok(())
}

fn restore_original_file(current: &Path, backup: &Path) -> std::io::Result<()> {
    fs::remove_file(current).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!(
                "failed to remove replacement {} before restoring backup {}: {error}",
                current.display(),
                backup.display()
            ),
        )
    })?;
    fs::rename(backup, current).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!(
                "failed to restore original from backup {} to {}: {error}",
                backup.display(),
                current.display()
            ),
        )
    })
}

fn apply_patch_to_track(track: &mut Track, patch: &TrackMetadataPatch) {
    apply_nullable(&mut track.title, &patch.title);
    apply_nullable(&mut track.artist, &patch.artist);
    apply_nullable(&mut track.album, &patch.album);
    apply_nullable(&mut track.year, &patch.year);
    apply_nullable(&mut track.genre, &patch.genre);
    apply_nullable(&mut track.label, &patch.label);
    apply_nullable(&mut track.catalog_number, &patch.catalog_number);
    apply_nullable(&mut track.bpm, &patch.bpm);
    apply_nullable(&mut track.key, &patch.key);
    if let MetadataField::Set(rating) = &patch.rating {
        track.rating = *rating;
    } else if matches!(&patch.rating, MetadataField::Clear) {
        track.rating = 0;
    }
}

fn apply_nullable<T: Clone>(value: &mut Option<T>, patch: &MetadataField<T>) {
    match patch {
        MetadataField::Unchanged => {}
        MetadataField::Set(new_value) => *value = Some(new_value.clone()),
        MetadataField::Clear => *value = None,
    }
}

impl TrackMetadataPatch {
    fn has_embedded_edits(&self) -> bool {
        !matches!(&self.title, MetadataField::Unchanged)
            || !matches!(&self.artist, MetadataField::Unchanged)
            || !matches!(&self.album, MetadataField::Unchanged)
            || !matches!(&self.year, MetadataField::Unchanged)
            || !matches!(&self.genre, MetadataField::Unchanged)
            || !matches!(&self.label, MetadataField::Unchanged)
            || !matches!(&self.catalog_number, MetadataField::Unchanged)
            || !matches!(&self.bpm, MetadataField::Unchanged)
            || !matches!(&self.key, MetadataField::Unchanged)
            || !matches!(&self.embedded_artwork, MetadataField::Unchanged)
    }
}

#[cfg(test)]
#[path = "../../test_utils.rs"]
mod test_utils;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    use lofty::file::TaggedFileExt;
    use tempfile::tempdir;

    use crate::models::EmbeddedArtwork;
    use crate::services::FileTagsService;

    fn write_minimal_wav(path: &Path) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&44_100_u32.to_le_bytes());
        bytes.extend_from_slice(&88_200_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        fs::write(path, bytes).unwrap();
    }

    fn insert_track(conn: &rusqlite::Connection, path: &Path) {
        conn
      .execute(
        "INSERT INTO tracks (id, file_path, title, duration_ms, format, date_added, date_modified) \
         VALUES ('metadata-test', ?1, 'Before', 1, 'wav', 'added', 'modified')",
        [path.to_string_lossy().as_ref()],
      )
      .unwrap();
    }

    fn service(conn: rusqlite::Connection, app_data_dir: &Path) -> LibraryService {
        LibraryService::new(
            Arc::new(Mutex::new(conn)),
            app_data_dir.to_path_buf(),
            FileTagsService::new(),
        )
    }

    fn tiny_png() -> Vec<u8> {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::new(1, 1));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn clearing_rating_reads_back_as_unrated() {
        let dir = tempdir().unwrap();
        let audio_path = dir.path().join("track.wav");
        write_minimal_wav(&audio_path);
        let conn = test_utils::make_memory_db();
        insert_track(&conn, &audio_path);
        conn.execute(
            "UPDATE tracks SET rating = 4 WHERE id = 'metadata-test'",
            [],
        )
        .unwrap();
        let library = service(conn, dir.path());

        library
            .update_track_metadata(
                "metadata-test",
                TrackMetadataPatch {
                    rating: MetadataField::Clear,
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(library.get_track("metadata-test").unwrap().rating, 0);
    }

    #[test]
    fn staged_text_and_artwork_are_written_before_library_metadata_is_returned() {
        let dir = tempdir().unwrap();
        let audio_path = dir.path().join("track.wav");
        write_minimal_wav(&audio_path);
        FileTagsService::new()
            .write_track_meta(
                &audio_path,
                None,
                Some("Old Artist"),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let conn = test_utils::make_memory_db();
        insert_track(&conn, &audio_path);
        let library = service(conn, dir.path());
        let patch = TrackMetadataPatch {
            title: MetadataField::Set("After".to_string()),
            artist: MetadataField::Clear,
            embedded_artwork: MetadataField::Set(EmbeddedArtwork {
                mime_type: "image/png".to_string(),
                data: tiny_png(),
            }),
            ..Default::default()
        };

        let updated = library
            .update_track_metadata("metadata-test", patch)
            .unwrap();
        let embedded = FileTagsService::new()
            .read_all(&audio_path)
            .expect("updated audio tags should be readable");
        let tagged_file = lofty::read_from_path(&audio_path).unwrap();

        assert_eq!(updated.title.as_deref(), Some("After"));
        assert_eq!(updated.artist, None);
        assert_eq!(embedded.title.as_deref(), Some("After"));
        assert_eq!(embedded.artist, None);
        assert_eq!(tagged_file.primary_tag().unwrap().pictures().len(), 1);
        assert_eq!(
            tagged_file.primary_tag().unwrap().pictures()[0].pic_type(),
            lofty::picture::PictureType::CoverFront
        );

        let clear_artwork = TrackMetadataPatch {
            embedded_artwork: MetadataField::Clear,
            ..Default::default()
        };
        library
            .update_track_metadata("metadata-test", clear_artwork)
            .unwrap();
        let cleared_file = lofty::read_from_path(&audio_path).unwrap();
        assert!(cleared_file.primary_tag().unwrap().pictures().is_empty());
    }

    #[test]
    fn unsupported_audio_staging_does_not_commit_library_metadata() {
        let dir = tempdir().unwrap();
        let audio_path = dir.path().join("invalid.mp3");
        fs::write(&audio_path, b"not an audio file").unwrap();
        let conn = test_utils::make_memory_db();
        insert_track(&conn, &audio_path);
        let library = service(conn, dir.path());
        let patch = TrackMetadataPatch {
            title: MetadataField::Set("Should not commit".to_string()),
            ..Default::default()
        };

        assert!(library
            .update_track_metadata("metadata-test", patch)
            .is_err());
        assert_eq!(
            library.get_track("metadata-test").unwrap().title.as_deref(),
            Some("Before")
        );
        assert_eq!(fs::read(audio_path).unwrap(), b"not an audio file");
    }

    #[test]
    fn failed_staged_install_and_restore_report_retained_backup_path() {
        let dir = tempdir().unwrap();
        let original_path = dir.path().join("track.wav");
        let staged_path = dir.path().join("track.crate-stage.wav");
        let backup_path = dir.path().join("track.crate-backup.wav");
        fs::write(&original_path, b"original media").unwrap();
        fs::write(&staged_path, b"staged media").unwrap();
        let mut rename_count = 0;

        let error = replace_with_staged_file_using(
            &original_path,
            &staged_path,
            &backup_path,
            |from, to| {
                rename_count += 1;
                match rename_count {
                    1 => fs::rename(from, to),
                    2 => Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "forced staged install failure",
                    )),
                    3 => Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "forced backup restore failure",
                    )),
                    _ => unreachable!(),
                }
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains(&backup_path.display().to_string()));
        assert!(!original_path.exists());
        assert_eq!(fs::read(&backup_path).unwrap(), b"original media");
    }

    #[test]
    fn failed_audio_restore_reports_and_preserves_backup_path() {
        let dir = tempdir().unwrap();
        let current_path = dir.path().join("track.wav");
        let backup_path = dir.path().join("track.crate-backup.wav");
        fs::create_dir(&current_path).unwrap();
        fs::write(&backup_path, b"original media").unwrap();

        let error = restore_original_file(&current_path, &backup_path).unwrap_err();

        assert!(error
            .to_string()
            .contains(&backup_path.display().to_string()));
        assert_eq!(fs::read(&backup_path).unwrap(), b"original media");
    }

    #[test]
    fn artwork_with_mismatched_declared_mime_is_rejected_before_file_write() {
        let dir = tempdir().unwrap();
        let audio_path = dir.path().join("track.wav");
        write_minimal_wav(&audio_path);
        let original_bytes = fs::read(&audio_path).unwrap();
        let patch = TrackMetadataPatch {
            embedded_artwork: MetadataField::Set(EmbeddedArtwork {
                mime_type: "image/jpeg".to_string(),
                data: tiny_png(),
            }),
            ..Default::default()
        };

        let result = FileTagsService::new().write_metadata_patch(&audio_path, &patch);

        assert!(result.is_err());
        assert_eq!(fs::read(&audio_path).unwrap(), original_bytes);
    }

    #[test]
    fn database_failure_after_media_replacement_restores_original_audio() {
        let dir = tempdir().unwrap();
        let audio_path = dir.path().join("track.wav");
        write_minimal_wav(&audio_path);
        let original_bytes = fs::read(&audio_path).unwrap();
        let conn = test_utils::make_memory_db();
        insert_track(&conn, &audio_path);
        conn.execute_batch(
            "CREATE TRIGGER reject_metadata_update BEFORE UPDATE ON tracks \
         BEGIN SELECT RAISE(ABORT, 'forced metadata failure'); END;",
        )
        .unwrap();
        let library = service(conn, dir.path());
        let patch = TrackMetadataPatch {
            title: MetadataField::Set("Should roll back".to_string()),
            ..Default::default()
        };

        assert!(library
            .update_track_metadata("metadata-test", patch)
            .is_err());
        assert_eq!(fs::read(&audio_path).unwrap(), original_bytes);
        assert_eq!(
            library.get_track("metadata-test").unwrap().title.as_deref(),
            Some("Before")
        );
    }
}
