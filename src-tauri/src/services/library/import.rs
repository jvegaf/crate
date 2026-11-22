use std::fs::File;
use std::io::BufReader;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::{AudioFile, TaggedFile};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::Tag;
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
    let supported_formats = ["mp3", "wav", "aiff", "aif", "flac", "m4a", "aac"];
    if !supported_formats.contains(&format.as_str()) {
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

  /// Import a single track with a pre-computed hash
  pub(crate) fn import_single_track_with_hash(
    &self,
    path: &PathBuf,
    file_hash: String,
  ) -> Result<Track> {
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

    let supported_formats = ["mp3", "wav", "aiff", "aif", "flac", "m4a", "aac"];
    if !supported_formats.contains(&format.as_str()) {
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
    let bitrate = params.bits_per_sample.map(|b| b as i32);

    Ok((duration_ms, sample_rate, bitrate))
  }

  fn insert_track(&self, track: &Track) -> Result<()> {
    let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

    let hlc = dirty::next_hlc(&conn)?;
    let (library_root_id, relative_path) =
      resolution::assign_root_for_import(&conn, &track.file_path)?;

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

    dirty::mark_dirty(&conn, &buckets::bucket_for_track_id(&track.id))?;

    Ok(())
  }

  fn extract_bpm(&self, tag: &Tag) -> Option<f64> {
    // BPM is stored as UTF-8 text (TBPM in ID3v2, tmpo in MP4, BPM in Vorbis).
    // Reject non-positive and implausible values so a bad tag falls through to
    // analysis instead of being treated as already analyzed forever.
    tag
      .get_string(&ItemKey::IntegerBpm)
      .and_then(|s| s.trim().parse::<f64>().ok())
      .filter(|bpm| *bpm > 0.0 && (40.0..=300.0).contains(bpm))
  }

  fn extract_key(&self, tag: &Tag) -> Option<String> {
    // Initial key is stored as UTF-8 text (TKEY in ID3v2, INITIALKEY/KEY in Vorbis,
    // com.apple.iTunes:initialkey in MP4). All map to ItemKey::InitialKey.
    // Normalize to the standard notation stratum-dsp produces (Am, F#, C#m) so
    // display and USB export stay consistent; unrecognized notations return None and
    // let analysis fill the value.
    tag.get_string(&ItemKey::InitialKey).and_then(normalize_key)
  }
}

/// Parse a DJ-style numbered key: Camelot (1A-12B) or Open Key (1m-12d).
fn parse_numbered_key(raw: &str) -> Option<&'static str> {
  let bytes = raw.as_bytes();
  if bytes.len() < 2 || bytes.len() > 3 {
    return None;
  }
  let (num, suffix) = match bytes {
    [a, b] if a.is_ascii_digit() => (*a - b'0', *b),
    [a, b, c] if a.is_ascii_digit() && b.is_ascii_digit() => ((*a - b'0') * 10 + (*b - b'0'), *c),
    _ => return None,
  };
  if !(1..=12).contains(&num) {
    return None;
  }

  const CAMELOT_MINOR: [&str; 12] = [
    "G#m", "D#m", "A#m", "Fm", "Cm", "Gm", "Dm", "Am", "Em", "Bm", "F#m", "C#m",
  ];
  const CAMELOT_MAJOR: [&str; 12] = [
    "B", "F#", "C#", "G#", "D#", "A#", "F", "C", "G", "D", "A", "E",
  ];
  const OPEN_MINOR: [&str; 12] = [
    "Am", "Em", "Bm", "F#m", "C#m", "G#m", "D#m", "A#m", "Fm", "Cm", "Gm", "Dm",
  ];
  const OPEN_MAJOR: [&str; 12] = [
    "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#", "F",
  ];

  let idx = (num - 1) as usize;
  match suffix.to_ascii_lowercase() {
    b'a' => Some(CAMELOT_MINOR[idx]),
    b'b' => Some(CAMELOT_MAJOR[idx]),
    b'm' => Some(OPEN_MINOR[idx]),
    b'd' => Some(OPEN_MAJOR[idx]),
    _ => None,
  }
}

/// Parse a musical key name into standard notation (sharps, stratum-dsp style).
fn parse_musical_key(raw: &str) -> Option<String> {
  let normalized = raw.trim().to_ascii_lowercase();
  let mut chars = normalized.chars().peekable();

  let note_letter = match chars.next()? {
    c @ 'a'..='g' => c,
    _ => return None,
  };
  let accidental = match chars.peek().copied() {
    Some('#') | Some('♯') => {
      chars.next();
      Some('#')
    }
    Some('b') | Some('♭') => {
      // Flat accidental. "Bm"/"Bmin"/"B major" never reach this branch because the
      // character after the B note is 'm' or a space, not 'b'.
      chars.next();
      Some('b')
    }
    _ => None,
  };

  let rest: String = chars.collect();
  let mode = parse_key_mode(rest.trim())?;

  let semitone = match (note_letter, accidental) {
    ('c', None) => 0,
    ('c', Some('#')) => 1,
    ('d', Some('b')) => 1,
    ('d', None) => 2,
    ('d', Some('#')) => 3,
    ('e', Some('b')) => 3,
    ('e', None) => 4,
    ('f', Some('b')) => 4,
    ('e', Some('#')) => 5,
    ('f', None) => 5,
    ('f', Some('#')) => 6,
    ('g', Some('b')) => 6,
    ('g', None) => 7,
    ('g', Some('#')) => 8,
    ('a', Some('b')) => 8,
    ('a', None) => 9,
    ('a', Some('#')) => 10,
    ('b', Some('b')) => 10,
    ('b', None) => 11,
    ('c', Some('b')) => 11,
    ('b', Some('#')) => 0,
    _ => return None,
  };

  const SHARP_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
  ];
  let name = SHARP_NAMES[semitone as usize];
  Some(match mode {
    KeyMode::Major => name.to_string(),
    KeyMode::Minor => format!("{name}m"),
  })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyMode {
  Major,
  Minor,
}

fn parse_key_mode(raw: &str) -> Option<KeyMode> {
  let m = raw.trim().to_ascii_lowercase().replace(['-', '_'], " ");
  let m = m.split_whitespace().collect::<Vec<_>>().join(" ");
  match m.as_str() {
    "" | "maj" | "major" | "dur" => Some(KeyMode::Major),
    "m" | "min" | "minor" | "moll" => Some(KeyMode::Minor),
    _ => None,
  }
}

/// Normalize a tag key value to standard notation (Am, F#, C#m) as produced by
/// stratum-dsp's `Key::name()`. Accepts Camelot, Open Key, and common spelled-out or
/// abbreviated musical notation. Returns `None` for anything unrecognized so the track
/// falls through to normal analysis.
fn normalize_key(raw: &str) -> Option<String> {
  let raw = raw.trim();
  if raw.is_empty() {
    return None;
  }
  if let Some(key) = parse_numbered_key(raw) {
    return Some(key.to_string());
  }
  parse_musical_key(raw)
}

#[cfg(test)]
mod tests {
  use super::*;
  use lofty::tag::TagType;

  /// The tag path needs no database state and no real artwork directory, so an
  /// in-memory connection and a throwaway path are enough.
  fn service() -> LibraryService {
    LibraryService::new(
      Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
      PathBuf::from("/tmp"),
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
  fn extract_bpm_rejects_non_positive_and_implausible_values() {
    let mut tag = Tag::new(TagType::Id3v2);

    for value in ["0", "-1", "39.9", "300.1", "999", "abc"] {
      tag.insert_text(ItemKey::IntegerBpm, value.to_string());
      assert_eq!(
        service().extract_bpm(&tag),
        None,
        "BPM {value} should be rejected"
      );
    }
  }

  #[test]
  fn extract_bpm_accepts_plausible_range() {
    let mut tag = Tag::new(TagType::Id3v2);

    for (value, expected) in [
      ("40", 40.0),
      ("128", 128.0),
      ("300", 300.0),
      ("174.5", 174.5),
    ] {
      tag.insert_text(ItemKey::IntegerBpm, value.to_string());
      assert_eq!(
        service().extract_bpm(&tag),
        Some(expected),
        "BPM {value} should be accepted"
      );
    }
  }

  #[test]
  fn normalize_key_accepts_camelot_notation() {
    assert_eq!(normalize_key("8A"), Some("Am".to_string()));
    assert_eq!(normalize_key("11B"), Some("A".to_string()));
    assert_eq!(normalize_key("1A"), Some("G#m".to_string()));
    assert_eq!(normalize_key("12B"), Some("E".to_string()));
  }

  #[test]
  fn normalize_key_accepts_open_key_notation() {
    assert_eq!(normalize_key("1m"), Some("Am".to_string()));
    assert_eq!(normalize_key("6d"), Some("B".to_string()));
    assert_eq!(normalize_key("8m"), Some("A#m".to_string()));
    assert_eq!(normalize_key("10d"), Some("D#".to_string()));
  }

  #[test]
  fn normalize_key_accepts_spelled_out_and_abbreviated_notation() {
    for (input, expected) in [
      ("A minor", "Am"),
      ("Amin", "Am"),
      ("A min", "Am"),
      ("Abm", "G#m"),
      ("G#m", "G#m"),
      ("C major", "C"),
      ("F#", "F#"),
      ("Db", "C#"),
      ("Bb minor", "A#m"),
    ] {
      assert_eq!(
        normalize_key(input),
        Some(expected.to_string()),
        "key {input} should normalize to {expected}"
      );
    }
  }

  #[test]
  fn normalize_key_returns_none_for_unrecognized_values() {
    for input in ["", "  ", "not a key", "13A", "0B", "1x", "H#", "Z minor"] {
      assert_eq!(normalize_key(input), None, "key {input} should be rejected");
    }
  }

  #[test]
  fn extract_key_normalizes_tag_value() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::InitialKey, "8A".to_string());

    assert_eq!(service().extract_key(&tag), Some("Am".to_string()));
  }

  #[test]
  fn extract_key_rejects_unrecognized_notation() {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::InitialKey, "not a key".to_string());

    assert_eq!(service().extract_key(&tag), None);
  }
}
