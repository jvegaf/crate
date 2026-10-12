//! Traktor path resolution.
//!
//! Traktor encodes locations with its own `/:` separator and a separate
//! `VOLUME`, e.g. `VOLUME="C:" DIR="/:Users/:dj/:Music/:" FILE="x.mp3"`. There
//! is no percent-encoding and no `file://`. `PRIMARYKEY KEY` carries the same
//! information as `VOLUME + DIR + FILE` (`C:` + `/:Users/.../:` + file), so it
//! is parsed rather than assumed.

use std::path::MAIN_SEPARATOR;

/// Whether `volume` is a Windows drive designator (`C:`, `d:`).
fn is_windows_drive(volume: &str) -> bool {
    let bytes = volume.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Split a `/:`-encoded path fragment into its non-empty segments.
fn split_segments(fragment: &str) -> impl Iterator<Item = &str> {
    fragment.split("/:").filter(|segment| !segment.is_empty())
}

/// Resolve a `(VOLUME, DIR, FILE)` triple to an OS-native path.
///
/// On Windows a drive-letter `VOLUME` becomes the path prefix; elsewhere it is
/// dropped (a Windows drive is meaningless on Unix) and the root is taken from
/// `DIR`. A non-drive `VOLUME` (a mounted volume path) is kept on every OS.
pub fn resolve_location(volume: &str, dir: &str, file: &str) -> String {
    let volume = volume.trim();
    let mut parts: Vec<String> = Vec::new();
    let mut absolute = dir.starts_with('/');

    if !volume.is_empty() {
        if is_windows_drive(volume) {
            if cfg!(windows) {
                parts.push(volume.to_string());
                absolute = true;
            }
        } else {
            parts.extend(split_segments(volume).map(str::to_string));
            absolute = absolute || volume.starts_with('/');
        }
    }

    parts.extend(split_segments(dir).map(str::to_string));
    if !file.is_empty() {
        parts.push(file.to_string());
    }

    let separator = MAIN_SEPARATOR.to_string();
    let joined = parts.join(&separator);
    if absolute && !cfg!(windows) && !joined.starts_with(MAIN_SEPARATOR) {
        format!("{MAIN_SEPARATOR}{joined}")
    } else {
        joined
    }
}

/// Split a `/:`-encoded `DIR + FILE` tail at the last `/:`.
fn split_dir_and_file(tail: &str) -> (&str, &str) {
    match tail.rfind("/:") {
        Some(index) => (&tail[..index + 2], &tail[index + 2..]),
        None => (tail, ""),
    }
}

/// Resolve a Traktor `PRIMARYKEY KEY` to the same native path as
/// [`resolve_location`] would produce from the track's `LOCATION` triple.
pub fn resolve_primary_key(key: &str) -> String {
    match key.find("/:") {
        Some(index) => {
            let volume = &key[..index];
            let tail = &key[index..];
            let (dir, file) = split_dir_and_file(tail);
            resolve_location(volume, dir, file)
        }
        // No separator at all: treat it as a bare filename.
        None => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_key_and_location_triple_agree() {
        let loc = resolve_location("C:", "/:Users/:dj/:Music/:", "a.mp3");
        let key = resolve_primary_key("C:/:Users/:dj/:Music/:a.mp3");
        assert_eq!(loc, key);
    }

    #[test]
    fn primary_key_without_a_trailing_segment_keeps_the_file_name() {
        // The last `/:` separates the file: a file name containing `:` (but not
        // `/:`) must survive intact.
        let resolved = resolve_primary_key("C:/:Users/:dj/:Music/:https___vk.com_x.mp3");
        assert!(resolved.ends_with("https___vk.com_x.mp3"));
    }

    #[test]
    fn a_lone_file_name_resolves_to_itself() {
        assert_eq!(resolve_primary_key("just-a-file.mp3"), "just-a-file.mp3");
    }
}
