use serde::{Deserialize, Serialize};
// Only `Track::new` (desktop library import) constructs UUIDs here.
#[cfg(feature = "desktop")]
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
  pub id: String,
  pub file_path: String,
  pub file_hash: Option<String>,

  // Metadata
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub year: Option<i32>,
  pub genre: Option<String>,
  pub label: Option<String>,
  pub catalog_number: Option<String>,

  // Audio properties
  pub duration_ms: i64,
  pub bpm: Option<f64>,
  pub key: Option<String>,
  pub bitrate: Option<i32>,
  pub sample_rate: Option<i32>,
  pub format: String,

  // Analysis metadata
  pub analysis_source: Option<String>,
  pub waveform_data: Option<Vec<u8>>,

  // User data
  pub rating: i32,
  pub play_count: i32,

  // Timestamps
  pub date_added: String,
  pub date_modified: String,
  pub last_played: Option<String>,

  // External references
  pub rekordbox_id: Option<String>,
  /// Store page URL (Beatport/Bandcamp/Traxsource), persisted in the audio file's ID3v2 WOAR frame.
  pub url: Option<String>,

  // Album artwork
  pub artwork_path: Option<String>,
  /// Source of artwork: "extracted" (from audio file) or "user_provided"
  pub artwork_source: Option<String>,

  // Track color (Rekordbox-compatible)
  pub color: Option<String>,

  // Cloud sync: library root association
  pub library_root_id: Option<String>,
  pub relative_path: Option<String>,

  // Tags (populated when fetching tracks)
  #[serde(default)]
  pub tags: Vec<super::Tag>,
}

// `Track::new` constructs a fresh imported track — only the desktop library import does this.
#[cfg(feature = "desktop")]
impl Track {
  pub fn new(file_path: String, format: String, duration_ms: i64) -> Self {
    let now = chrono::Utc::now().to_rfc3339();
    Self {
      id: Uuid::new_v4().to_string(),
      file_path,
      file_hash: None,
      title: None,
      artist: None,
      album: None,
      year: None,
      genre: None,
      label: None,
      catalog_number: None,
      duration_ms,
      bpm: None,
      key: None,
      bitrate: None,
      sample_rate: None,
      format,
      analysis_source: None,
      waveform_data: None,
      rating: 0,
      play_count: 0,
      date_added: now.clone(),
      date_modified: now,
      last_played: None,
      rekordbox_id: None,
      url: None,
      artwork_path: None,
      artwork_source: None,
      color: None,
      library_root_id: None,
      relative_path: None,
      tags: Vec::new(),
    }
  }
}

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrackFilter {
  pub search: Option<String>,
  pub tag_ids: Option<Vec<String>>,
  pub tag_filter_mode: Option<String>,
  pub playlist_id: Option<String>,
  pub bpm_min: Option<f64>,
  pub bpm_max: Option<f64>,
  pub key: Option<String>,
}

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrackUpdate {
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub year: Option<i32>,
  pub genre: Option<String>,
  pub label: Option<String>,
  pub url: Option<String>,
  pub bpm: Option<f64>,
  pub key: Option<String>,
  pub rating: Option<i32>,
}

/// A field-level metadata patch. Missing fields remain unchanged; JSON `null` clears;
/// any non-null value replaces the current value.
#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Default, PartialEq)]
pub enum MetadataField<T> {
  #[default]
  Unchanged,
  Set(T),
  Clear,
}

#[cfg(feature = "desktop")]
impl<'de, T> Deserialize<'de> for MetadataField<T>
where
  T: Deserialize<'de>,
{
  fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    struct FieldVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T> serde::de::Visitor<'de> for FieldVisitor<T>
    where
      T: Deserialize<'de>,
    {
      type Value = MetadataField<T>;

      fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a metadata value or null")
      }

      fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
      where
        E: serde::de::Error,
      {
        Ok(MetadataField::Clear)
      }

      fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
      where
        E: serde::de::Error,
      {
        Ok(MetadataField::Clear)
      }

      fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
      where
        D: serde::Deserializer<'de>,
      {
        T::deserialize(deserializer).map(MetadataField::Set)
      }
    }

    deserializer.deserialize_option(FieldVisitor(std::marker::PhantomData))
  }
}

/// Single-track metadata patch. `rating` remains library-owned; text/audio fields and artwork
/// are also embedded into the audio file when supplied.
#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TrackMetadataPatch {
  #[serde(default)]
  pub title: MetadataField<String>,
  #[serde(default)]
  pub artist: MetadataField<String>,
  #[serde(default)]
  pub album: MetadataField<String>,
  #[serde(default)]
  pub year: MetadataField<i32>,
  #[serde(default)]
  pub genre: MetadataField<String>,
  #[serde(default)]
  pub label: MetadataField<String>,
  #[serde(default)]
  pub catalog_number: MetadataField<String>,
  #[serde(default)]
  pub url: MetadataField<String>,
  #[serde(default)]
  pub bpm: MetadataField<f64>,
  #[serde(default)]
  pub key: MetadataField<String>,
  #[serde(default)]
  pub rating: MetadataField<i32>,
  #[serde(default)]
  pub embedded_artwork: MetadataField<EmbeddedArtwork>,
}

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct EmbeddedArtwork {
  pub mime_type: String,
  pub data: Vec<u8>,
}

#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
  pub tracks: Vec<Track>,
  pub failed_count: usize,
  pub errors: Vec<String>,
}

/// Result of validating a replacement file for a missing track
#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMatchResult {
  /// Whether the file content hash matches the original
  pub matches: bool,
  /// The original file's hash (if available)
  pub original_hash: Option<String>,
  /// The new file's computed hash
  pub new_hash: String,
  /// Whether the file format is valid/supported
  pub format_valid: bool,
}

/// A duplicate track detected during import
#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateTrack {
  /// The path of the new file being imported
  pub new_file_path: String,
  /// The computed hash of the new file
  pub new_file_hash: String,
  /// The existing track that has the same hash
  pub existing_track: Track,
}

/// Extended import result with duplicate detection
#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResultWithDuplicates {
  /// Successfully imported tracks (non-duplicates)
  pub tracks: Vec<Track>,
  /// Files that failed to import
  pub failed_count: usize,
  /// Error messages for each failure
  pub errors: Vec<String>,
  /// Duplicates detected that need user resolution
  pub duplicates: Vec<DuplicateTrack>,
}

/// Resolution action for a duplicate track
#[cfg(feature = "desktop")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DuplicateResolution {
  /// Skip this file, don't import
  Skip,
  /// Update the existing track's file_path to the new location
  UpdatePath { new_path: String },
  /// Replace: fresh import keeping only playlist memberships
  Replace { new_path: String, new_hash: String },
}

#[cfg(all(test, feature = "desktop"))]
mod metadata_patch_tests {
  use super::{MetadataField, TrackMetadataPatch};

  #[test]
  fn omitted_set_and_null_fields_have_distinct_patch_states() {
    let patch: TrackMetadataPatch =
      serde_json::from_str(r#"{"artist":"Artist","title":null}"#).unwrap();

    assert_eq!(patch.title, MetadataField::Clear);
    assert_eq!(patch.artist, MetadataField::Set("Artist".to_string()));
    assert_eq!(patch.album, MetadataField::Unchanged);
  }
}
