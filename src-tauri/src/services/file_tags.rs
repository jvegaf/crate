//! Desktop-only service for reading and writing audio-file metadata (BPM, key, title, artist,
//! album, year, genre).
//!
//! This replaces the ad-hoc reads scattered across the import and analysis code-paths with a
//! single, reusable abstraction so every place that mutates track-level metadata can write both
//! to the database **and** to the physical audio file in one place.
//!
//! ### API surface
//!
//! | Function | Purpose |
//! | --- | --- |
//! | `FileTagsService::new` | Constructor (clone-safe). |
//! | `read_all` | Full read of all editable fields from an audio file. |
//! | `write_track_meta` | Apply field-by-field metadata updates to the file. |
//! | `write_bpm_and_key` | Convenience helper used by the analysis pipeline. |

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use lofty::config::{ParseOptions, ParsingMode, WriteOptions};
use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::Tag;

use crate::error::{CrateError, Result};

// =============================================================================
// Service struct (must be Clone for Tauri State sharing)
// =============================================================================

/// Desktop-only service for reading/writing audio-file metadata.
/// Designed as a reusable module — many APIs stay unused until future
/// tag-editing features hook into them.
#[allow(dead_code)]
#[derive(Clone)]
pub struct FileTagsService;

impl FileTagsService {
  pub fn new() -> Self {
    Self
  }

  // ---------------------------------------------------------------------------
  // Read helpers (used now; kept public for potential external consumers)
  // ---------------------------------------------------------------------------

  /// Read all editable metadata fields from an audio file at *path*.
  ///
  /// Returns `None` when the file cannot be parsed (unrelated formats, corrupted files).
  #[allow(dead_code)]
  pub fn read_all(&self, path: &Path) -> Option<FileMetadata> {
    let tagged_file = Self::probe_file(path)?;

    let properties = tagged_file.properties();

    // Pick the first usable tag (prefer primary, fall back to any available).
    let tag = tagged_file
      .primary_tag()
      .or_else(|| tagged_file.first_tag());

    Some(FileMetadata {
      duration_ms: properties.duration().as_millis() as i64,
      title: tag.as_ref().and_then(|t| t.title()).map(|s| s.to_string()),
      artist: tag.as_ref().and_then(|t| t.artist()).map(|s| s.to_string()),
      album: tag.as_ref().and_then(|t| t.album()).map(|s| s.to_string()),
      year: tag.as_ref().and_then(|t| t.year()).map(|y| y as i32),
      genre: tag.as_ref().and_then(|t| t.genre()).map(|s| s.to_string()),
      bpm: Self::extract_bpm(tag),
      key: Self::extract_key(tag),
    })
  }

  /// Read a lenient [`TaggedFile`] (shared with import/analysis).
  #[allow(dead_code)]
  pub(crate) fn probe_file(path: &Path) -> Option<TaggedFile> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let parse_options = ParseOptions::new()
      .parsing_mode(ParsingMode::Relaxed)
      .max_junk_bytes(4096);

    Probe::new(reader)
      .options(parse_options)
      .guess_file_type()
      .ok()?
      .read()
      .ok()
  }

  // ---------------------------------------------------------------------------
  // Write helpers
  // ---------------------------------------------------------------------------

  /// Persist selected metadata fields back into the audio file.
  ///
  /// Each parameter controls whether that field is written (`Some`) or left unchanged (`None`).
  /// If all parameters are `None` the function is a no-op.
  pub fn write_track_meta(
    &self,
    path: &Path,
    title: Option<&str>,
    artist: Option<&str>,
    album: Option<&str>,
    year: Option<i32>,
    genre: Option<&str>,
    bpm: Option<f64>,
    key: Option<&str>,
  ) -> Result<()> {
    // Quick no-op check (nothing to write).
    if title.is_none()
      && artist.is_none()
      && album.is_none()
      && year.is_none()
      && genre.is_none()
      && bpm.is_none()
      && key.is_none()
    {
      return Ok(());
    }

    // Open the file with read+write access using standard lofty read_from_path
    let mut tagged_file = lofty::read_from_path(path).map_err(|e| {
      CrateError::FileTags(format!(
        "Failed to open audio file for metadata update: {}: {e}",
        path.display()
      ))
    })?;

    let tag_type = tagged_file.primary_tag_type();

    // Ensure the tag exists — get_or_insert equivalent
    if tagged_file.tag_mut(tag_type).is_none() {
      tagged_file.insert_tag(Tag::new(tag_type));
    }

    let tag_mut = tagged_file.tag_mut(tag_type).unwrap();

    if let Some(v) = title {
      tag_mut.insert_text(ItemKey::TrackTitle, v.to_string());
    }
    if let Some(v) = artist {
      tag_mut.insert_text(ItemKey::TrackArtist, v.to_string());
    }
    if let Some(v) = album {
      tag_mut.insert_text(ItemKey::AlbumTitle, v.to_string());
    }
    if let Some(v) = year {
      tag_mut.set_year(v as u32);
    }
    if let Some(v) = genre {
      tag_mut.insert_text(ItemKey::Genre, v.to_string());
    }
    if let Some(bpm_val) = bpm {
      tag_mut.insert_text(ItemKey::IntegerBpm, format!("{bpm_val:.1}"));
    }
    if let Some(k) = key {
      tag_mut.insert_text(ItemKey::InitialKey, k.trim().to_string());
    }

    tagged_file
      .save_to_path(path, WriteOptions::default())
      .map_err(|e| {
        CrateError::FileTags(format!(
          "Failed to save metadata to {}: {e}",
          path.display()
        ))
      })?;

    Ok(())
  }

  /// Convenience shortcut used by the analysis pipeline to persist just BPM + key.
  pub fn write_bpm_and_key(&self, path: &Path, bpm: f64, key: Option<&str>) -> Result<()> {
    self.write_track_meta(path, None, None, None, None, None, Some(bpm), key)
  }

  // ---------------------------------------------------------------------------
  // Extraction helpers
  // ---------------------------------------------------------------------------

  /// Extract BPM string → f64.
  #[allow(dead_code)]
  fn extract_bpm(tag: Option<&Tag>) -> Option<f64> {
    tag
      .and_then(|t| t.get_string(&ItemKey::IntegerBpm))
      .and_then(|s| s.trim().parse::<f64>().ok())
  }

  /// Extract initial key.
  #[allow(dead_code)]
  fn extract_key(tag: Option<&Tag>) -> Option<String> {
    tag
      .and_then(|t| t.get_string(&ItemKey::InitialKey))
      .map(|s| s.trim().to_string())
  }
}

// =============================================================================
// Data transfer object — mirrors what the frontend / library store expects
// =============================================================================

/// Complete set of editable file-level metadata fields.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct FileMetadata {
  pub duration_ms: i64,
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub year: Option<i32>,
  pub genre: Option<String>,
  pub bpm: Option<f64>,
  pub key: Option<String>,
}

#[cfg(test)]
mod tests {
  use super::*;
  use lofty::tag::TagType;

  /// The BPM extraction reads the canonical integer-BPM item.
  #[test]
  fn extract_bpm_reads_integer_bpm_tag() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::IntegerBpm, "120".to_string());

    assert_eq!(FileTagsService::extract_bpm(Some(&tag)), Some(120.0));
  }

  /// Key extraction trims whitespace.
  #[test]
  fn extract_key_trims_whitespace() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::InitialKey, "Am ".to_string());

    assert_eq!(
      FileTagsService::extract_key(Some(&tag)),
      Some("Am".to_string())
    );
  }

  /// Empty / missing tags yield None.
  #[test]
  fn empty_tag_yields_no_values() {
    let tag = Tag::new(TagType::Id3v2);

    assert_eq!(FileTagsService::extract_bpm(Some(&tag)), None);
    assert_eq!(FileTagsService::extract_key(Some(&tag)), None);
  }
}
