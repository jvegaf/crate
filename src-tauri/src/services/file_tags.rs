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
use lofty::tag::{ItemValue, Tag, TagItem};

use crate::error::{CrateError, Result};
use crate::models::{EmbeddedArtwork, MetadataField, TrackMetadataPatch};

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
    #[allow(clippy::too_many_arguments)]
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
        url: Option<&str>,
    ) -> Result<()> {
        // Quick no-op check (nothing to write).
        if title.is_none()
            && artist.is_none()
            && album.is_none()
            && year.is_none()
            && genre.is_none()
            && bpm.is_none()
            && key.is_none()
            && url.is_none()
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
        if let Some(v) = url {
            // Store page URL lives in the ID3v2 WOAR frame (ItemKey::TrackArtistUrl). lofty
            // validates WOAR as a URL frame at save time, so it must carry ItemValue::Locator —
            // ItemValue::Text is accepted in memory but rejected by save_to_path.
            tag_mut.insert(TagItem::new(
                ItemKey::TrackArtistUrl,
                ItemValue::Locator(v.to_string()),
            ));
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

    /// Apply explicit unchanged/set/clear metadata fields to an audio file.
    ///
    /// This method is intended for a staged copy. The caller replaces the original only after
    /// every requested embedded field has been written successfully.
    pub fn write_metadata_patch(&self, path: &Path, patch: &TrackMetadataPatch) -> Result<()> {
        let mut tagged_file = lofty::read_from_path(path).map_err(|e| {
            CrateError::FileTags(format!(
                "Failed to read audio file for metadata update: {}: {e}",
                path.display()
            ))
        })?;

        let tag_type = tagged_file.primary_tag_type();
        if tagged_file.tag(tag_type).is_none() {
            tagged_file.insert_tag(Tag::new(tag_type));
        }
        let tag = tagged_file.tag_mut(tag_type).ok_or_else(|| {
            CrateError::FileTags(format!(
                "Audio format has no writable primary tag: {}",
                path.display()
            ))
        })?;

        Self::apply_text_field(tag, ItemKey::TrackTitle, &patch.title)?;
        Self::apply_text_field(tag, ItemKey::TrackArtist, &patch.artist)?;
        Self::apply_text_field(tag, ItemKey::AlbumTitle, &patch.album)?;
        Self::apply_year_field(tag, &patch.year)?;
        Self::apply_text_field(tag, ItemKey::Genre, &patch.genre)?;
        Self::apply_text_field(tag, ItemKey::Label, &patch.label)?;
        Self::apply_text_field(tag, ItemKey::CatalogNumber, &patch.catalog_number)?;
        Self::apply_url_field(tag, &patch.url)?;
        match &patch.bpm {
            MetadataField::Unchanged => {}
            MetadataField::Set(value) => {
                if !tag.insert_text(ItemKey::IntegerBpm, format!("{value:.1}")) {
                    return Err(CrateError::FileTags(
                        "Audio tag format does not support BPM metadata".to_string(),
                    ));
                }
            }
            MetadataField::Clear => tag.remove_key(&ItemKey::IntegerBpm),
        }
        Self::apply_text_field(tag, ItemKey::InitialKey, &patch.key)?;
        Self::apply_artwork_field(tag, &patch.embedded_artwork)?;

        tagged_file
            .save_to_path(path, WriteOptions::default())
            .map_err(|e| {
                CrateError::FileTags(format!(
                    "Audio format does not support the requested metadata write at {}: {e}",
                    path.display()
                ))
            })?;

        Self::verify_metadata_patch(path, patch)
    }

    fn apply_text_field(tag: &mut Tag, key: ItemKey, field: &MetadataField<String>) -> Result<()> {
        match field {
            MetadataField::Unchanged => {}
            MetadataField::Set(value) => {
                if !tag.insert_text(key.clone(), value.clone()) {
                    return Err(CrateError::FileTags(format!(
                        "Audio tag format does not support metadata key {key:?}"
                    )));
                }
            }
            MetadataField::Clear => {
                tag.remove_key(&key);
            }
        }
        Ok(())
    }

    /// Apply the store page URL field to the ID3v2 WOAR frame (`ItemKey::TrackArtistUrl`).
    ///
    /// lofty classifies WOAR as a URL frame and rejects `ItemValue::Text` at save time
    /// (`Attempted to write an invalid frame`), so the value must be inserted as
    /// `ItemValue::Locator`. Text-only `Tag::insert_text` is deliberately not used here.
    fn apply_url_field(tag: &mut Tag, field: &MetadataField<String>) -> Result<()> {
        let key = ItemKey::TrackArtistUrl;
        match field {
            MetadataField::Unchanged => {}
            MetadataField::Set(value) => {
                if !tag.insert(TagItem::new(key.clone(), ItemValue::Locator(value.clone()))) {
                    return Err(CrateError::FileTags(format!(
                        "Audio tag format does not support metadata key {key:?}"
                    )));
                }
            }
            MetadataField::Clear => {
                tag.remove_key(&key);
            }
        }
        Ok(())
    }

    fn apply_year_field(tag: &mut Tag, field: &MetadataField<i32>) -> Result<()> {
        match field {
            MetadataField::Unchanged => {}
            MetadataField::Set(value) => {
                let year = u32::try_from(*value).map_err(|_| {
                    CrateError::FileTags(format!("Year must be a non-negative integer: {value}"))
                })?;
                tag.set_year(year);
            }
            MetadataField::Clear => tag.remove_key(&ItemKey::Year),
        }
        Ok(())
    }

    fn verify_metadata_patch(path: &Path, patch: &TrackMetadataPatch) -> Result<()> {
        let tagged_file = lofty::read_from_path(path).map_err(|error| {
            CrateError::FileTags(format!(
                "Could not verify staged audio metadata at {}: {error}",
                path.display()
            ))
        })?;
        let tag = tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag());
        Self::verify_text_field(tag, ItemKey::TrackTitle, &patch.title)?;
        Self::verify_text_field(tag, ItemKey::TrackArtist, &patch.artist)?;
        Self::verify_text_field(tag, ItemKey::AlbumTitle, &patch.album)?;
        Self::verify_text_field(tag, ItemKey::Genre, &patch.genre)?;
        Self::verify_text_field(tag, ItemKey::Label, &patch.label)?;
        Self::verify_text_field(tag, ItemKey::CatalogNumber, &patch.catalog_number)?;
        Self::verify_url_field(tag, &patch.url)?;
        Self::verify_year_field(tag, &patch.year)?;
        Self::verify_bpm_field(tag, &patch.bpm)?;
        Self::verify_text_field(tag, ItemKey::InitialKey, &patch.key)?;
        Self::verify_artwork_field(tag, &patch.embedded_artwork)
    }

    fn verify_text_field(
        tag: Option<&Tag>,
        key: ItemKey,
        field: &MetadataField<String>,
    ) -> Result<()> {
        let expected = match field {
            MetadataField::Unchanged => return Ok(()),
            MetadataField::Set(value) => Some(value.as_str()),
            MetadataField::Clear => None,
        };
        let actual = tag.and_then(|tag| tag.get_string(&key));
        if actual != expected {
            return Err(CrateError::FileTags(format!(
                "Audio tag write for {key:?} was not retained by the file format"
            )));
        }
        Ok(())
    }

    /// Verify the WOAR write through `url_item_value`, which also recognises
    /// `ItemValue::Locator` items — `Tag::get_string` used by `verify_text_field` matches
    /// only `Text` and would reject a correctly-written URL frame.
    fn verify_url_field(tag: Option<&Tag>, field: &MetadataField<String>) -> Result<()> {
        let expected = match field {
            MetadataField::Unchanged => return Ok(()),
            MetadataField::Set(value) => Some(value.as_str()),
            MetadataField::Clear => None,
        };
        let actual = Self::url_item_value(tag);
        if actual != expected {
            return Err(CrateError::FileTags(format!(
                "Audio tag write for {:?} was not retained by the file format",
                ItemKey::TrackArtistUrl
            )));
        }
        Ok(())
    }

    fn verify_year_field(tag: Option<&Tag>, field: &MetadataField<i32>) -> Result<()> {
        let expected = match field {
            MetadataField::Unchanged => return Ok(()),
            MetadataField::Set(value) => Some(*value),
            MetadataField::Clear => None,
        };
        let actual = tag.and_then(Tag::year).map(|year| year as i32);
        if actual != expected {
            return Err(CrateError::FileTags(
                "Audio tag write for year was not retained by the file format".to_string(),
            ));
        }
        Ok(())
    }

    fn verify_bpm_field(tag: Option<&Tag>, field: &MetadataField<f64>) -> Result<()> {
        match field {
            MetadataField::Unchanged => Ok(()),
            MetadataField::Clear if Self::extract_bpm(tag).is_none() => Ok(()),
            MetadataField::Set(expected)
                if Self::extract_bpm(tag)
                    .is_some_and(|actual| (actual - expected).abs() < 0.051) =>
            {
                Ok(())
            }
            _ => Err(CrateError::FileTags(
                "Audio tag write for BPM was not retained by the file format".to_string(),
            )),
        }
    }

    fn verify_artwork_field(
        tag: Option<&Tag>,
        field: &MetadataField<EmbeddedArtwork>,
    ) -> Result<()> {
        match field {
            MetadataField::Unchanged => Ok(()),
            MetadataField::Clear if tag.is_none_or(|tag| tag.pictures().is_empty()) => Ok(()),
            MetadataField::Set(expected)
                if tag.is_some_and(|tag| {
                    tag.pictures().iter().any(|picture| {
                        picture.pic_type() == lofty::picture::PictureType::CoverFront
                            && picture.data() == expected.data
                    })
                }) =>
            {
                Ok(())
            }
            _ => Err(CrateError::FileTags(
                "Audio artwork write was not retained by the file format".to_string(),
            )),
        }
    }

    fn apply_artwork_field(tag: &mut Tag, field: &MetadataField<EmbeddedArtwork>) -> Result<()> {
        match field {
            MetadataField::Unchanged => Ok(()),
            MetadataField::Clear => {
                while !tag.pictures().is_empty() {
                    tag.remove_picture(tag.pictures().len() - 1);
                }
                Ok(())
            }
            MetadataField::Set(artwork) => {
                let mime_type = lofty::picture::MimeType::from_str(&artwork.mime_type);
                if matches!(mime_type, lofty::picture::MimeType::Unknown(_)) {
                    return Err(CrateError::FileTags(format!(
                        "Unsupported embedded artwork MIME type: {}",
                        artwork.mime_type
                    )));
                }
                let decoded_format = image::guess_format(&artwork.data).map_err(|e| {
                    CrateError::FileTags(format!("Invalid embedded artwork image: {e}"))
                })?;
                if image::ImageFormat::from_mime_type(&artwork.mime_type) != Some(decoded_format) {
                    return Err(CrateError::FileTags(format!(
                        "Embedded artwork MIME type {} does not match decoded image format {}",
                        artwork.mime_type,
                        decoded_format.extensions_str().join("/")
                    )));
                }
                image::load_from_memory(&artwork.data).map_err(|e| {
                    CrateError::FileTags(format!("Invalid embedded artwork image: {e}"))
                })?;
                while !tag.pictures().is_empty() {
                    tag.remove_picture(tag.pictures().len() - 1);
                }
                tag.push_picture(lofty::picture::Picture::new_unchecked(
                    lofty::picture::PictureType::CoverFront,
                    Some(mime_type),
                    None,
                    artwork.data.clone(),
                ));
                Ok(())
            }
        }
    }

    /// Convenience shortcut used by the analysis pipeline to persist just BPM + key.
    pub fn write_bpm_and_key(&self, path: &Path, bpm: f64, key: Option<&str>) -> Result<()> {
        self.write_track_meta(path, None, None, None, None, None, Some(bpm), key, None)
    }

    // ---------------------------------------------------------------------------
    // Extraction helpers
    // ---------------------------------------------------------------------------

    /// Extract BPM string → f64.
    #[allow(dead_code)]
    fn extract_bpm(tag: Option<&Tag>) -> Option<f64> {
        tag.and_then(|t| t.get_string(&ItemKey::IntegerBpm))
            .and_then(|s| s.trim().parse::<f64>().ok())
    }

    /// Extract initial key.
    #[allow(dead_code)]
    fn extract_key(tag: Option<&Tag>) -> Option<String> {
        tag.and_then(|t| t.get_string(&ItemKey::InitialKey))
            .map(|s| s.trim().to_string())
    }

    /// Extract the store page URL from the `ItemKey::TrackArtistUrl` item (ID3v2 WOAR).
    ///
    /// lofty classifies WOAR as a URL frame, whose canonical value is
    /// `ItemValue::Locator`. `Tag::get_string` only matches `ItemValue::Text`, so the value
    /// variants are inspected directly: `Locator` for correctly-written ID3v2/APE tags and
    /// `Text` as a fallback, because non-URL containers persist a Locator as a plain string
    /// and Crate's own pre-fix writes stored `Text` in memory-only tags.
    fn url_item_value(tag: Option<&Tag>) -> Option<&str> {
        tag.and_then(|tag| {
            tag.items()
                .find(|item| item.key() == &ItemKey::TrackArtistUrl)
                .and_then(|item| match item.value() {
                    ItemValue::Locator(s) | ItemValue::Text(s) => Some(s.as_str()),
                    _ => None,
                })
        })
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

    /// A `Set(url)` patch writes the store page URL into the ID3v2 WOAR frame as an
    /// `ItemValue::Locator`, and a `Clear` patch removes it again
    /// (ItemKey::TrackArtistUrl ⇄ "WOAR" in lofty).
    #[test]
    fn metadata_patch_roundtrips_url_through_the_woar_frame() {
        let mut tag = Tag::new(TagType::Id3v2);
        let patch = TrackMetadataPatch {
            url: MetadataField::Set("https://www.beatport.com/track/x/1".to_string()),
            ..Default::default()
        };

        FileTagsService::apply_url_field(&mut tag, &patch.url).unwrap();
        assert_eq!(
            FileTagsService::url_item_value(Some(&tag)),
            Some("https://www.beatport.com/track/x/1")
        );
        // The written item is the Locator variant lofty requires for URL frames.
        assert!(matches!(
            tag.get(&ItemKey::TrackArtistUrl).map(|item| item.value()),
            Some(ItemValue::Locator(_))
        ));

        FileTagsService::apply_url_field(&mut tag, &MetadataField::Clear).unwrap();
        assert_eq!(FileTagsService::url_item_value(Some(&tag)), None);
    }

    /// An `Unchanged` patch state must not create or remove a WOAR frame.
    #[test]
    fn unchanged_url_patch_leaves_the_woar_frame_untouched() {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.insert(TagItem::new(
            ItemKey::TrackArtistUrl,
            ItemValue::Locator("https://existing.example".to_string()),
        ));

        FileTagsService::apply_url_field(&mut tag, &MetadataField::Unchanged).unwrap();
        assert_eq!(
            FileTagsService::url_item_value(Some(&tag)),
            Some("https://existing.example")
        );
    }

    /// Bytes of the tiny silent mono MP3 committed under `test_assets/`. Tests operate on
    /// unique temporary copies so the fixture itself is never mutated.
    const WOAR_SAMPLE_MP3: &[u8] = include_bytes!("../../test_assets/woar-sample.mp3");

    /// Disambiguates the temporary copies taken from the fixture within one test binary.
    static WOAR_TEST_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    /// Regression: `Set(url)` must survive a real `save_to_path`. lofty treats WOAR as a
    /// URL frame and rejects `ItemValue::Text` at save time with
    /// `ID3v2: Attempted to write an invalid frame. ID: "WOAR", Value: "Text"`, which broke
    /// the whole tagger apply flow. `Clear(url)` must then remove the frame from the file.
    #[test]
    fn url_patch_saves_woar_to_a_real_mp3_file() {
        let path = std::env::temp_dir().join(format!(
            "crate-woar-test-{}-{}.mp3",
            std::process::id(),
            WOAR_TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::write(&path, WOAR_SAMPLE_MP3).unwrap();

        let service = FileTagsService::new();
        let url = "https://beatport.com/track/1";

        let set_patch = TrackMetadataPatch {
            url: MetadataField::Set(url.to_string()),
            ..Default::default()
        };
        service.write_metadata_patch(&path, &set_patch).unwrap();

        let tagged =
            FileTagsService::probe_file(&path).expect("fixture should re-parse after write");
        let tag = tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())
            .expect("written tag");
        assert_eq!(FileTagsService::url_item_value(Some(tag)), Some(url));

        let clear_patch = TrackMetadataPatch {
            url: MetadataField::Clear,
            ..Default::default()
        };
        service.write_metadata_patch(&path, &clear_patch).unwrap();

        let tagged =
            FileTagsService::probe_file(&path).expect("fixture should re-parse after clear");
        let tag = tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())
            .expect("remaining tag");
        assert_eq!(FileTagsService::url_item_value(Some(tag)), None);

        std::fs::remove_file(&path).unwrap();
    }
}
