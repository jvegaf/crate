use std::fs::File;
use std::io::BufReader;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::{AudioFile, TaggedFile};
use lofty::id3::v2::{Frame, Id3v2Tag};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;

use super::*;
use crate::services::cloud_sync::pipeline::{buckets, dirty};
use crate::services::cloud_sync::resolution;

impl LibraryService {
  pub fn import_tracks(&self, paths: Vec<PathBuf>) -> Result<ImportResult> {
    let mut tracks = Vec::new();
    let mut errors = Vec::new();

    for path in paths {
      match self.import_single_track(&path) {
        Ok(track) => tracks.push(track),
        Err(e) => {
          let error_msg = format!("{}: {}", path.display(), e);
          log::warn!("Failed to import {error_msg}");
          errors.push(error_msg);
        }
      }
    }

    Ok(ImportResult {
      tracks,
      failed_count: errors.len(),
      errors,
    })
  }

  fn import_single_track(&self, path: &PathBuf) -> Result<Track> {
    if !path.exists() {
      return Err(CrateError::FileNotFound(path.clone()));
    }

    // Determine format from extension
    let format = path
      .extension()
      .and_then(|e| e.to_str())
      .map(|e| e.to_lowercase())
      .unwrap_or_default();

    // Check if supported format
    if !SUPPORTED_AUDIO_EXTENSIONS.contains(&format.as_str()) {
      return Err(CrateError::Import(format!("Unsupported format: {format}")));
    }

    // Try to read metadata with lenient parsing first
    let mut track = Track::new(
      path.to_string_lossy().to_string(),
      format.clone(),
      0, // Duration will be set below
    );

    if let Some(tagged_file) = self.read_metadata_lenient(path) {
      // Successfully read with lofty
      let properties = tagged_file.properties();
      track.duration_ms = properties.duration().as_millis() as i64;
      track.bitrate = properties.audio_bitrate().map(|b| b as i32);
      track.sample_rate = properties.sample_rate().map(|s| s as i32);

      // Extract tags if available
      if let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
      {
        track.title = tag.title().map(|s| s.to_string());
        track.artist = tag.artist().map(|s| s.to_string());
        track.album = tag.album().map(|s| s.to_string());
        track.year = tag.year().map(|y| y as i32);
        track.genre = tag.genre().map(|s| s.to_string());

        // Try to get BPM from various tag formats
        track.bpm = self.extract_bpm(tag);

        // Try to get key
        track.key = self.extract_key(tag);

        // Try to get the rating from an ID3v2 POPM frame
        track.rating = self.extract_rating(tag);
      }

      // Extract album artwork
      if let Some(artwork_path) = self
        .artwork_service
        .extract_and_save(&tagged_file, &track.id)
      {
        track.artwork_path = Some(artwork_path);
        track.artwork_source = Some("extracted".to_string());
      }
    } else {
      // Lofty failed completely, use symphonia fallback
      log::warn!(
        "Metadata extraction failed for {}, falling back to symphonia",
        path.display()
      );

      let (dur, sr, br) = self.read_audio_properties_symphonia(path)?;
      track.duration_ms = dur;
      track.sample_rate = sr;
      track.bitrate = br;
    }

    // Compute file hash for future relocation matching
    if let Ok(hash) = compute_audio_hash(path) {
      track.file_hash = Some(hash);
    }

    // Insert into database
    self.insert_track(&track)?;

    Ok(track)
  }

  /// Build a `Track` from a file on disk, stamping a pre-computed content hash.
  ///
  /// Reads the format, tags, audio properties and artwork, but performs no database
  /// write: callers decide how the row is persisted (single insert or scan batch).
  pub(crate) fn build_track_from_file(&self, path: &PathBuf, file_hash: String) -> Result<Track> {
    // Determine format from extension
    let format = path
      .extension()
      .and_then(|e| e.to_str())
      .map(|e| e.to_lowercase())
      .unwrap_or_default();

    // Create track with pre-computed hash
    let mut track = Track::new(
      path.to_string_lossy().to_string(),
      format.clone(),
      0, // Duration will be set below
    );
    track.file_hash = Some(file_hash);

    if let Some(tagged_file) = self.read_metadata_lenient(path) {
      // Successfully read with lofty
      let properties = tagged_file.properties();
      track.duration_ms = properties.duration().as_millis() as i64;
      track.bitrate = properties.audio_bitrate().map(|b| b as i32);
      track.sample_rate = properties.sample_rate().map(|s| s as i32);

      // Extract tags if available
      if let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
      {
        track.title = tag.title().map(|s| s.to_string());
        track.artist = tag.artist().map(|s| s.to_string());
        track.album = tag.album().map(|s| s.to_string());
        track.year = tag.year().map(|y| y as i32);
        track.genre = tag.genre().map(|s| s.to_string());
        track.bpm = self.extract_bpm(tag);
        track.key = self.extract_key(tag);
        track.rating = self.extract_rating(tag);
      }

      // Extract album artwork
      if let Some(artwork_path) = self
        .artwork_service
        .extract_and_save(&tagged_file, &track.id)
      {
        track.artwork_path = Some(artwork_path);
        track.artwork_source = Some("extracted".to_string());
      }
    } else {
      // Lofty failed completely, use symphonia fallback
      log::warn!(
        "Metadata extraction failed for {}, falling back to symphonia",
        path.display()
      );

      let (dur, sr, br) = self.read_audio_properties_symphonia(path)?;
      track.duration_ms = dur;
      track.sample_rate = sr;
      track.bitrate = br;
    }

    Ok(track)
  }

  /// Import a single track with a pre-computed hash
  pub(crate) fn import_single_track_with_hash(
    &self,
    path: &PathBuf,
    file_hash: String,
  ) -> Result<Track> {
    let track = self.build_track_from_file(path, file_hash)?;

    // Insert into database
    self.insert_track(&track)?;

    Ok(track)
  }

  /// Import tracks with duplicate detection based on content hash
  pub fn import_tracks_with_duplicate_detection(
    &self,
    paths: Vec<PathBuf>,
  ) -> Result<ImportResultWithDuplicates> {
    let mut tracks = Vec::new();
    let mut errors = Vec::new();
    let mut duplicates = Vec::new();

    for path in paths {
      match self.process_import_path(&path) {
        Ok(ImportPathResult::NewTrack(track)) => tracks.push(track),
        Ok(ImportPathResult::Duplicate(dup)) => duplicates.push(dup),
        Err(e) => {
          let error_msg = format!("{}: {}", path.display(), e);
          log::warn!("Failed to import {error_msg}");
          errors.push(error_msg);
        }
      }
    }

    Ok(ImportResultWithDuplicates {
      tracks,
      failed_count: errors.len(),
      errors,
      duplicates,
    })
  }

  /// Process a single import path, checking for duplicates first
  fn process_import_path(&self, path: &PathBuf) -> Result<ImportPathResult> {
    if !path.exists() {
      return Err(CrateError::FileNotFound(path.clone()));
    }

    // Check format
    let format = path
      .extension()
      .and_then(|e| e.to_str())
      .map(|e| e.to_lowercase())
      .unwrap_or_default();

    if !SUPPORTED_AUDIO_EXTENSIONS.contains(&format.as_str()) {
      return Err(CrateError::Import(format!("Unsupported format: {format}")));
    }

    // Compute hash first to check for duplicates
    let file_hash = compute_audio_hash(path)?;

    // Check if a track with this hash already exists
    if let Some(existing_track) = self.find_track_by_hash(&file_hash)? {
      return Ok(ImportPathResult::Duplicate(DuplicateTrack {
        new_file_path: path.to_string_lossy().to_string(),
        new_file_hash: file_hash,
        existing_track,
      }));
    }

    // No duplicate - proceed with normal import
    let track = self.import_single_track_with_hash(path, file_hash)?;
    Ok(ImportPathResult::NewTrack(track))
  }

  /// Attempts to read audio file metadata with lenient parsing options.
  /// Returns None if parsing fails completely.
  pub(crate) fn read_metadata_lenient(&self, path: &PathBuf) -> Option<TaggedFile> {
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

  /// Fallback to extract audio properties using symphonia when lofty fails.
  /// Returns (duration_ms, sample_rate, bitrate).
  fn read_audio_properties_symphonia(
    &self,
    path: &PathBuf,
  ) -> Result<(i64, Option<i32>, Option<i32>)> {
    let file =
      File::open(path).map_err(|e| CrateError::Metadata(format!("Failed to open: {e}")))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
      hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
      .format(&hint, mss, &Default::default(), &Default::default())
      .map_err(|e| CrateError::Metadata(format!("Symphonia probe failed: {e}")))?;

    let track = probed
      .format
      .default_track()
      .ok_or_else(|| CrateError::Metadata("No audio track found".to_string()))?;

    let params = &track.codec_params;

    let duration_ms = match (params.n_frames, params.sample_rate) {
      (Some(frames), Some(sample_rate)) => ((frames as f64 / sample_rate as f64) * 1000.0) as i64,
      _ => 0,
    };

    let sample_rate = params.sample_rate.map(|s| s as i32);
    let bitrate = match params.codec {
      symphonia::core::codecs::CODEC_TYPE_PCM_S32LE
      | symphonia::core::codecs::CODEC_TYPE_PCM_S32LE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_S32BE
      | symphonia::core::codecs::CODEC_TYPE_PCM_S32BE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_S24LE
      | symphonia::core::codecs::CODEC_TYPE_PCM_S24LE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_S24BE
      | symphonia::core::codecs::CODEC_TYPE_PCM_S24BE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_S16LE
      | symphonia::core::codecs::CODEC_TYPE_PCM_S16LE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_S16BE
      | symphonia::core::codecs::CODEC_TYPE_PCM_S16BE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_S8
      | symphonia::core::codecs::CODEC_TYPE_PCM_S8_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_U32LE
      | symphonia::core::codecs::CODEC_TYPE_PCM_U32LE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_U32BE
      | symphonia::core::codecs::CODEC_TYPE_PCM_U32BE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_U24LE
      | symphonia::core::codecs::CODEC_TYPE_PCM_U24LE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_U24BE
      | symphonia::core::codecs::CODEC_TYPE_PCM_U24BE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_U16LE
      | symphonia::core::codecs::CODEC_TYPE_PCM_U16LE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_U16BE
      | symphonia::core::codecs::CODEC_TYPE_PCM_U16BE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_U8
      | symphonia::core::codecs::CODEC_TYPE_PCM_U8_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_F32LE
      | symphonia::core::codecs::CODEC_TYPE_PCM_F32LE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_F32BE
      | symphonia::core::codecs::CODEC_TYPE_PCM_F32BE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_F64LE
      | symphonia::core::codecs::CODEC_TYPE_PCM_F64LE_PLANAR
      | symphonia::core::codecs::CODEC_TYPE_PCM_F64BE
      | symphonia::core::codecs::CODEC_TYPE_PCM_F64BE_PLANAR => pcm_bitrate_kbps(
        params.sample_rate,
        params.channels.map(|channels| channels.count()),
        params.bits_per_sample,
      ),
      _ => None,
    };

    Ok((duration_ms, sample_rate, bitrate))
  }

  /// Lock the shared connection and delegate the insert to [`Self::insert_track_in`].
  fn insert_track(&self, track: &Track) -> Result<()> {
    let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;
    Self::insert_track_in(&conn, track)
  }

  /// Insert or update a track row on a caller-supplied connection.
  ///
  /// Takes `&Connection` (not `&self`) so a batch scan can run the insert inside its
  /// own transaction while holding the mutex guard once.
  pub(crate) fn insert_track_in(conn: &Connection, track: &Track) -> Result<()> {
    let hlc = dirty::next_hlc(conn)?;
    let (library_root_id, relative_path) =
      resolution::assign_root_for_import(conn, &track.file_path)?;

    conn.execute(
      r#"
            INSERT INTO tracks (
                id, file_path, file_hash,
                title, artist, album, year, genre, label, catalog_number,
                duration_ms, bpm, key, bitrate, sample_rate, format,
                analysis_source, waveform_data,
                rating, play_count,
                date_added, date_modified, last_played,
                rekordbox_id, artwork_path, artwork_source, color,
                _hlc, library_root_id, relative_path
            ) VALUES (
                ?1, ?2, ?3,
                ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18,
                ?19, ?20,
                ?21, ?22, ?23,
                ?24, ?25, ?26, ?27,
                ?28, ?29, ?30
            )
            ON CONFLICT(file_path) DO UPDATE SET
                title = excluded.title,
                artist = excluded.artist,
                album = excluded.album,
                year = excluded.year,
                genre = excluded.genre,
                artwork_path = excluded.artwork_path,
                artwork_source = excluded.artwork_source,
                date_modified = excluded.date_modified,
                _hlc = excluded._hlc,
                library_root_id = excluded.library_root_id,
                relative_path = excluded.relative_path
            "#,
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
        hlc,
        library_root_id,
        relative_path,
      ],
    )?;

    dirty::mark_dirty(conn, &buckets::bucket_for_track_id(&track.id))?;

    Ok(())
  }

  fn extract_bpm(&self, tag: &Tag) -> Option<f64> {
    // BPM is stored as UTF-8 text (TBPM in ID3v2, tmpo in MP4, BPM in Vorbis)
    tag
      .get_string(&ItemKey::IntegerBpm)
      .and_then(|s| s.trim().parse::<f64>().ok())
  }

  fn extract_key(&self, tag: &Tag) -> Option<String> {
    // Initial key is stored as UTF-8 text (TKEY in ID3v2, INITIALKEY/KEY in Vorbis,
    // com.apple.iTunes:initialkey in MP4). All map to ItemKey::InitialKey.
    tag
      .get_string(&ItemKey::InitialKey)
      .map(|s| s.trim().to_string())
  }

  /// Extract Crate's 0-5 star rating from ID3v2 `POPM` frames.
  ///
  /// Only ID3v2 carries a popularimeter, so every other format yields 0. The
  /// first POPM frame with a nonzero rating wins, which is how most players
  /// pick a single rating out of the per-email POPM entries. An all-zero or
  /// absent POPM yields 0, Crate's "unrated" value.
  fn extract_rating(&self, tag: &Tag) -> i32 {
    if tag.tag_type() != TagType::Id3v2 {
      return 0;
    }

    // The generic `Tag` does not expose typed POPM frames, but it keeps the
    // original ID3v2 tag as a companion; converting back gives access to the
    // parsed `Frame::Popularimeter` values instead of raw bytes under
    // `ItemKey::Popularimeter`.
    let id3v2: Id3v2Tag = tag.clone().into();

    id3v2
      .into_iter()
      .filter_map(|frame| match frame {
        Frame::Popularimeter(popm) => Some(popm.rating),
        _ => None,
      })
      .find(|rating| *rating != 0)
      .map(popm_rating_to_stars)
      .unwrap_or(0)
  }
}

pub(crate) fn pcm_bitrate_kbps(
  sample_rate: Option<u32>,
  channels: Option<usize>,
  bits_per_sample: Option<u32>,
) -> Option<i32> {
  let bits_per_second = u128::from(sample_rate?)
    .checked_mul(channels? as u128)?
    .checked_mul(u128::from(bits_per_sample?))?;
  let rounded_kbps = bits_per_second.checked_add(500)? / 1000;

  i32::try_from(rounded_kbps)
    .ok()
    .filter(|bitrate| *bitrate > 0)
}

/// Map a raw ID3v2 `POPM` rating (0-255) to Crate's 0-5 star scale.
///
/// Boundaries follow the de-facto `POPM` star mapping used by players:
/// <https://en.wikipedia.org/wiki/ID3#ID3v2_star_rating_tag_issue>.
fn popm_rating_to_stars(raw: u8) -> i32 {
  match raw {
    0 => 0,
    1..=31 => 1,
    32..=95 => 2,
    96..=159 => 3,
    160..=223 => 4,
    224..=255 => 5,
  }
}

#[cfg(test)]
#[path = "../../test_utils.rs"]
mod test_utils;

#[cfg(test)]
mod tests {
  use super::*;
  use lofty::id3::v2::PopularimeterFrame;
  use lofty::tag::TagType;

  /// The tag path needs no database state and no real artwork directory, so an
  /// in-memory connection and a throwaway path are enough.
  fn service() -> LibraryService {
    use crate::services::FileTagsService;
    LibraryService::new(
      Arc::new(Mutex::new(test_utils::make_memory_db())),
      PathBuf::from("/tmp"),
      FileTagsService::new(),
    )
  }

  #[test]
  fn extract_bpm_reads_integer_bpm_tag() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::IntegerBpm, "120".to_string());

    assert_eq!(service().extract_bpm(&tag), Some(120.0));
  }

  #[test]
  fn extract_key_reads_initial_key_tag_trimmed() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::InitialKey, "Am ".to_string());

    assert_eq!(service().extract_key(&tag), Some("Am".to_string()));
  }

  #[test]
  fn empty_tag_yields_no_bpm_or_key() {
    let tag = Tag::new(TagType::Id3v2);

    assert_eq!(service().extract_bpm(&tag), None);
    assert_eq!(service().extract_key(&tag), None);
  }

  #[test]
  fn tag_with_other_fields_but_no_bpm_or_key_yields_none() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::TrackTitle, "Some Title".to_string());
    tag.insert_text(ItemKey::TrackArtist, "Some Artist".to_string());

    assert_eq!(service().extract_bpm(&tag), None);
    assert_eq!(service().extract_key(&tag), None);
  }

  #[test]
  fn extract_bpm_accepts_leading_zeros() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::IntegerBpm, "0120".to_string());

    assert_eq!(service().extract_bpm(&tag), Some(120.0));
  }

  #[test]
  fn extract_bpm_accepts_whitespace_and_decimals() {
    for (raw, expected) in [(" 128 ", 128.0), ("127.5", 127.5), ("0", 0.0)] {
      let mut tag = Tag::new(TagType::Id3v2);
      tag.insert_text(ItemKey::IntegerBpm, raw.to_string());

      assert_eq!(service().extract_bpm(&tag), Some(expected), "raw = {raw:?}");
    }
  }

  #[test]
  fn malformed_bpm_tag_is_ignored_instead_of_panicking() {
    for raw in ["", "  ", "not-a-number", "120 bpm", "1.2.3", "--8"] {
      let mut tag = Tag::new(TagType::Id3v2);
      tag.insert_text(ItemKey::IntegerBpm, raw.to_string());

      assert_eq!(service().extract_bpm(&tag), None, "raw = {raw:?}");
    }
  }

  #[test]
  fn extract_key_supports_common_musical_formats() {
    for raw in ["8m", "Dm", "Bmaj", "C#min", "Dbm", "1d"] {
      let mut tag = Tag::new(TagType::Id3v2);
      tag.insert_text(ItemKey::InitialKey, raw.to_string());

      assert_eq!(
        service().extract_key(&tag),
        Some(raw.to_string()),
        "raw = {raw:?}"
      );
    }
  }

  #[test]
  fn extract_key_trims_surrounding_whitespace_only() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::InitialKey, "  Dbm  ".to_string());

    assert_eq!(service().extract_key(&tag), Some("Dbm".to_string()));
  }

  /// Build a generic `Tag` that carries ID3v2 `POPM` frames, mirroring how lofty
  /// hands back an ID3v2 tag: the typed frames live in the companion tag.
  fn tag_with_popm(ratings: &[u8]) -> Tag {
    let mut id3v2 = Id3v2Tag::new();
    for (index, rating) in ratings.iter().enumerate() {
      id3v2.insert(Frame::Popularimeter(PopularimeterFrame::new(
        format!("user{index}@example.com"),
        *rating,
        0,
      )));
    }

    Tag::from(id3v2)
  }

  #[test]
  fn popm_rating_boundaries_map_to_expected_stars() {
    for (raw, expected) in [
      (0u8, 0),
      (1, 1),
      (31, 1),
      (32, 2),
      (95, 2),
      (96, 3),
      (159, 3),
      (160, 4),
      (223, 4),
      (224, 5),
      (255, 5),
    ] {
      assert_eq!(popm_rating_to_stars(raw), expected, "raw = {raw}");
    }
  }

  #[test]
  fn extract_rating_maps_single_popm_rating() {
    for (raw, expected) in [(1u8, 1), (96, 3), (255, 5)] {
      assert_eq!(
        service().extract_rating(&tag_with_popm(&[raw])),
        expected,
        "raw = {raw}"
      );
    }
  }

  #[test]
  fn extract_rating_uses_first_nonzero_popm_frame() {
    let tag = tag_with_popm(&[0, 200, 32]);

    assert_eq!(service().extract_rating(&tag), 4);
  }

  #[test]
  fn extract_rating_finds_popm_among_other_id3v2_items() {
    let mut tag = tag_with_popm(&[128]);
    tag.insert_text(ItemKey::TrackTitle, "Some Title".to_string());
    tag.insert_text(ItemKey::IntegerBpm, "128".to_string());

    assert_eq!(service().extract_rating(&tag), 3);
  }

  #[test]
  fn extract_rating_returns_zero_when_all_popm_frames_are_zero() {
    let tag = tag_with_popm(&[0, 0]);

    assert_eq!(service().extract_rating(&tag), 0);
  }

  #[test]
  fn extract_rating_returns_zero_without_popm_frame() {
    let tag = Tag::new(TagType::Id3v2);

    assert_eq!(service().extract_rating(&tag), 0);
  }

  #[test]
  fn extract_rating_ignores_non_id3v2_tags() {
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.insert_text(ItemKey::TrackTitle, "Some Title".to_string());

    assert_eq!(service().extract_rating(&tag), 0);
  }

  #[test]
  fn pcm_bitrate_kbps_matches_uncompressed_audio_rates() {
    assert_eq!(
      pcm_bitrate_kbps(Some(44_100), Some(2), Some(16)),
      Some(1411)
    );
    assert_eq!(
      pcm_bitrate_kbps(Some(48_000), Some(2), Some(24)),
      Some(2304)
    );
    assert_eq!(pcm_bitrate_kbps(Some(44_100), Some(1), Some(16)), Some(706));
  }

  #[test]
  fn pcm_bitrate_kbps_returns_none_for_missing_inputs() {
    assert_eq!(pcm_bitrate_kbps(None, Some(2), Some(16)), None);
    assert_eq!(pcm_bitrate_kbps(Some(44_100), None, Some(16)), None);
    assert_eq!(pcm_bitrate_kbps(Some(44_100), Some(2), None), None);
  }

  #[test]
  fn pcm_bitrate_kbps_returns_none_for_zero_inputs_or_rounded_result() {
    assert_eq!(pcm_bitrate_kbps(Some(0), Some(2), Some(16)), None);
    assert_eq!(pcm_bitrate_kbps(Some(44_100), Some(0), Some(16)), None);
    assert_eq!(pcm_bitrate_kbps(Some(44_100), Some(2), Some(0)), None);
    assert_eq!(pcm_bitrate_kbps(Some(1), Some(1), Some(1)), None);
  }
}
