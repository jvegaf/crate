use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use image::imageops::FilterType;
use image::ImageFormat;
// Embedded-art extraction relies on `lofty` (audio metadata), which is desktop-only. The
// image/filesystem methods below stay cross-platform so discovery artwork works on mobile.
#[cfg(feature = "desktop")]
use lofty::file::TaggedFile;
#[cfg(feature = "desktop")]
use lofty::picture::PictureType;
#[cfg(feature = "desktop")]
use lofty::prelude::*;
#[cfg(feature = "desktop")]
use lofty::tag::{Tag, TagType};

/// Service for managing album artwork extraction and storage.
/// Artwork is stored as 500x500 WEBP images in the app data directory.
#[derive(Clone)]
pub struct ArtworkService {
  artwork_dir: PathBuf,
}

impl ArtworkService {
  /// Creates a new ArtworkService with the given app data directory.
  /// The artwork subdirectory will be created if it doesn't exist.
  pub fn new(app_data_dir: PathBuf) -> Self {
    let artwork_dir = app_data_dir.join("artwork");
    // Ensure artwork directory exists
    if let Err(e) = fs::create_dir_all(&artwork_dir) {
      log::warn!("Failed to create artwork directory: {e}");
    }
    Self { artwork_dir }
  }

  /// Extracts album art from a tagged audio file and saves it as WEBP.
  /// Returns the relative path (e.g., "artwork/{track_id}-{hash}.webp") if successful.
  /// Uses multiple strategies to find pictures across different tag systems.
  #[cfg(feature = "desktop")]
  pub fn extract_and_save(&self, tagged_file: &TaggedFile, track_id: &str) -> Option<String> {
    // First try the primary tag (most reliable for common formats)
    if let Some(tag) = tagged_file.primary_tag() {
      if let Some(path) = self.extract_from_tag(tag, track_id) {
        return Some(path);
      }
    }

    // Then try all available tags through the generic interface
    for tag in tagged_file.tags() {
      if let Some(path) = self.extract_from_tag(tag, track_id) {
        return Some(path);
      }
    }

    // For AIFF and MP3 files, explicitly check ID3v2 tag
    // (pictures are stored in ID3v2, not in native AIFF chunks)
    if let Some(tag) = tagged_file.tag(TagType::Id3v2) {
      if let Some(path) = self.extract_from_tag(tag, track_id) {
        return Some(path);
      }
    }

    // Also try APE tags (sometimes used in MP3 files)
    if let Some(tag) = tagged_file.tag(TagType::Ape) {
      if let Some(path) = self.extract_from_tag(tag, track_id) {
        return Some(path);
      }
    }

    log::debug!("No artwork found for track {track_id}");
    None
  }

  /// Extracts a picture from a single tag and saves it.
  #[cfg(feature = "desktop")]
  fn extract_from_tag(&self, tag: &Tag, track_id: &str) -> Option<String> {
    let pictures = tag.pictures();
    if pictures.is_empty() {
      return None;
    }

    // Prefer front cover, fall back to first available
    let picture = pictures
      .iter()
      .find(|p| p.pic_type() == PictureType::CoverFront)
      .or_else(|| pictures.first())?;

    self.save_picture(picture.data(), track_id)
  }

  /// Saves picture data to disk as a WEBP image.
  /// Resizes to 500x500 max while maintaining aspect ratio.
  #[cfg(feature = "desktop")]
  fn save_picture(&self, data: &[u8], track_id: &str) -> Option<String> {
    // Load the image from raw bytes
    let img = image::load_from_memory(data).ok()?;

    // Resize if larger than 500x500, maintaining aspect ratio
    let img = if img.width() > 500 || img.height() > 500 {
      img.resize(500, 500, FilterType::Lanczos3)
    } else {
      img
    };

    // Encode to WEBP in memory so the file name can be derived from the exact bytes
    // that will be written. A changed image therefore becomes a changed path (and URL).
    let mut buffer = Vec::new();
    if let Err(e) = img.write_to(&mut Cursor::new(&mut buffer), ImageFormat::WebP) {
      log::warn!("Failed to encode artwork for track {track_id}: {e}");
      return None;
    }

    let filename = artwork_filename(track_id, &buffer);
    let path = self.artwork_dir.join(&filename);

    if let Err(e) = fs::write(&path, &buffer) {
      log::warn!("Failed to save artwork for track {track_id}: {e}");
      return None;
    }

    self.remove_stale_artwork(track_id, &filename);

    // Return the relative path for database storage
    Some(format!("artwork/{filename}"))
  }

  /// Deletes the artwork files for a track: the legacy `{track_id}.webp` and every
  /// content-addressed `{track_id}-*.webp` sibling.
  pub fn delete(&self, track_id: &str) {
    let legacy = format!("{track_id}.webp");
    let prefix = format!("{track_id}-");

    let read_dir = match fs::read_dir(&self.artwork_dir) {
      Ok(read_dir) => read_dir,
      Err(e) => {
        if e.kind() != std::io::ErrorKind::NotFound {
          log::warn!("Failed to delete artwork for track {track_id}: {e}");
        }
        return;
      }
    };

    for entry in read_dir.flatten() {
      let name = entry.file_name();
      let Some(name) = name.to_str() else { continue };
      if name == legacy || (name.starts_with(&prefix) && name.ends_with(".webp")) {
        if let Err(e) = fs::remove_file(entry.path()) {
          // Only log if the file existed (ignore NotFound errors)
          if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("Failed to delete artwork for track {track_id}: {e}");
          }
        }
      }
    }
  }

  /// Saves artwork from a user-provided image file.
  /// Returns the relative path (e.g., "artwork/{track_id}-{hash}.webp") if successful.
  pub fn save_from_file(&self, source_path: &std::path::Path, track_id: &str) -> Option<String> {
    // Load the image from the source file
    let img = image::open(source_path).ok()?;

    // Resize if larger than 500x500, maintaining aspect ratio
    let img = if img.width() > 500 || img.height() > 500 {
      img.resize(500, 500, FilterType::Lanczos3)
    } else {
      img
    };

    // Encode to WEBP in memory so the file name can be derived from the exact bytes
    // that will be written. A changed image therefore becomes a changed path (and URL).
    let mut buffer = Vec::new();
    if let Err(e) = img.write_to(&mut Cursor::new(&mut buffer), ImageFormat::WebP) {
      log::warn!("Failed to encode user-provided artwork for track {track_id}: {e}");
      return None;
    }

    let filename = artwork_filename(track_id, &buffer);
    let path = self.artwork_dir.join(&filename);

    if let Err(e) = fs::write(&path, &buffer) {
      log::warn!("Failed to save user-provided artwork for track {track_id}: {e}");
      return None;
    }

    self.remove_stale_artwork(track_id, &filename);

    // Return the relative path for database storage
    Some(format!("artwork/{filename}"))
  }

  /// Removes every artwork file for `track_id` other than `keep_filename`: the legacy
  /// `{track_id}.webp` and any content-addressed `{track_id}-*.webp` sibling left over
  /// from a previous image. `NotFound` is ignored.
  fn remove_stale_artwork(&self, track_id: &str, keep_filename: &str) {
    let legacy = format!("{track_id}.webp");
    let prefix = format!("{track_id}-");

    let read_dir = match fs::read_dir(&self.artwork_dir) {
      Ok(read_dir) => read_dir,
      Err(e) => {
        if e.kind() != std::io::ErrorKind::NotFound {
          log::warn!("Failed to scan artwork for track {track_id}: {e}");
        }
        return;
      }
    };

    for entry in read_dir.flatten() {
      let name = entry.file_name();
      let Some(name) = name.to_str() else { continue };
      if name == keep_filename {
        continue;
      }
      if name == legacy || (name.starts_with(&prefix) && name.ends_with(".webp")) {
        if let Err(e) = fs::remove_file(entry.path()) {
          if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("Failed to remove stale artwork for track {track_id}: {e}");
          }
        }
      }
    }
  }

  /// Returns the full filesystem path for an artwork file.
  #[cfg(feature = "desktop")]
  pub fn get_full_path(&self, relative_path: &str) -> PathBuf {
    self
      .artwork_dir
      .parent()
      .unwrap_or(&self.artwork_dir)
      .join(relative_path)
  }

  /// Checks if all artwork files at the given relative paths have identical content.
  /// Returns `false` if any file can't be read or if fewer than 2 paths are provided.
  #[cfg(feature = "desktop")]
  pub fn are_artworks_identical(&self, relative_paths: &[String]) -> bool {
    if relative_paths.len() < 2 {
      return false;
    }

    let mut reference_hash: Option<blake3::Hash> = None;

    for path in relative_paths {
      let full_path = self.get_full_path(path);
      let bytes = match fs::read(&full_path) {
        Ok(b) => b,
        Err(_) => return false,
      };
      let hash = blake3::hash(&bytes);

      match &reference_hash {
        None => reference_hash = Some(hash),
        Some(ref_hash) => {
          if hash != *ref_hash {
            return false;
          }
        }
      }
    }

    true
  }
}

/// Content-addressed artwork file name: `{track_id}-{blake3_hex[..12]}.webp`.
///
/// The suffix is derived from the encoded bytes so a changed image is a changed
/// path — and therefore a changed URL — which is what makes the webview pick it
/// up instead of serving a cached response for a stable path.
fn artwork_filename(track_id: &str, encoded: &[u8]) -> String {
  let hash = blake3::hash(encoded).to_hex().to_string();
  format!("{track_id}-{}.webp", &hash[..12])
}

#[cfg(all(test, feature = "desktop"))]
mod tests {
  use super::*;
  use image::{ImageBuffer, Rgb, RgbImage};

  const TRACK_ID: &str = "11111111-1111-1111-1111-111111111111";

  /// Builds a solid-colour 600x600 RGB image and encodes it to PNG bytes in memory,
  /// so tests need no fixtures on disk.
  fn png_bytes(color: [u8; 3]) -> Vec<u8> {
    let img: RgbImage = ImageBuffer::from_pixel(600, 600, Rgb(color));
    let mut buffer = Vec::new();
    image::DynamicImage::ImageRgb8(img)
      .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
      .expect("encode test PNG");
    buffer
  }

  /// Sorted list of file names currently present in the artwork directory.
  fn artwork_files(service: &ArtworkService) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&service.artwork_dir)
      .expect("read artwork dir")
      .flatten()
      .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
      .collect();
    names.sort();
    names
  }

  fn relative_filename(relative_path: &str) -> String {
    relative_path
      .strip_prefix("artwork/")
      .expect("relative artwork path")
      .to_string()
  }

  fn service() -> (tempfile::TempDir, ArtworkService) {
    let dir = tempfile::tempdir().expect("tempdir");
    let service = ArtworkService::new(dir.path().to_path_buf());
    (dir, service)
  }

  #[test]
  fn different_images_get_different_names_and_cleanup() {
    let (_dir, service) = service();

    let first = service
      .save_picture(&png_bytes([255, 0, 0]), TRACK_ID)
      .expect("first save");
    let second = service
      .save_picture(&png_bytes([0, 0, 255]), TRACK_ID)
      .expect("second save");

    assert_ne!(first, second, "different bytes must map to different paths");
    assert_eq!(
      artwork_files(&service),
      vec![relative_filename(&second)],
      "only the newest content-addressed file remains"
    );
  }

  #[test]
  fn same_image_twice_is_a_noop() {
    let (_dir, service) = service();
    let bytes = png_bytes([10, 200, 30]);

    let first = service.save_picture(&bytes, TRACK_ID).expect("first save");
    let second = service.save_picture(&bytes, TRACK_ID).expect("second save");

    assert_eq!(first, second, "identical bytes must map to the same path");
    assert_eq!(artwork_files(&service).len(), 1, "exactly one file remains");
  }

  #[test]
  fn save_removes_legacy_file() {
    let (_dir, service) = service();
    let legacy = service.artwork_dir.join(format!("{TRACK_ID}.webp"));
    fs::write(&legacy, b"legacy").expect("write legacy file");

    let saved = service
      .save_picture(&png_bytes([1, 2, 3]), TRACK_ID)
      .expect("save");

    assert!(!legacy.exists(), "legacy file is removed by the next save");
    assert_eq!(artwork_files(&service), vec![relative_filename(&saved)]);
  }

  #[test]
  fn delete_removes_legacy_and_content_addressed_files() {
    let (_dir, service) = service();
    let saved = service
      .save_picture(&png_bytes([9, 9, 9]), TRACK_ID)
      .expect("save");
    let legacy = service.artwork_dir.join(format!("{TRACK_ID}.webp"));
    fs::write(&legacy, b"legacy").expect("write legacy file");

    service.delete(TRACK_ID);

    assert!(!legacy.exists(), "legacy file is removed by delete");
    assert!(
      !service.artwork_dir.join(relative_filename(&saved)).exists(),
      "content-addressed file is removed by delete"
    );
    assert!(artwork_files(&service).is_empty());
  }
}
