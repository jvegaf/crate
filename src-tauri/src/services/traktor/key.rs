//! Traktor key resolution to canonical Camelot.
//!
//! `MUSICAL_KEY VALUE` is the preferred source: it is an unambiguous enum
//! (0–11 major, 12–23 minor). `INFO KEY` is the fallback and is heterogeneous
//! in real exports — Open Key (`3d`), Camelot (`8B`), and free note names
//! (`Ab Major`, `Am`, `Gb`) all appear.
//!
//! **Traktor's `INFO KEY` is true Open Key, rotated relative to Camelot**
//! (`11d` = Bb major = `6B`, `3d` = D major = `10B`). This is specific to the
//! NML import path: the legacy `m`/`d` handling used by the file-tag importer
//! (`services/analysis.rs`) is a different, unrotated convention and must not be
//! changed here.
//!
//! Every result is canonical Camelot — the raw `INFO KEY` is never stored.

/// Outcome of resolving a track's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Canonical Camelot code, e.g. `"6B"`.
    Camelot(String),
    /// A key value was present but no notation resolved it.
    Unresolved(String),
    /// The track carries no key at all.
    Missing,
}

/// Camelot code per pitch class for major keys: C, Db, D, Eb, E, F, Gb, G, Ab, A, Bb, B.
const MAJOR_CAMELOT: [&str; 12] = [
    "8B", "3B", "10B", "5B", "12B", "7B", "2B", "9B", "4B", "11B", "6B", "1B",
];

/// Camelot code per pitch class for minor keys, same pitch-class order.
const MINOR_CAMELOT: [&str; 12] = [
    "5A", "12A", "7A", "2A", "9A", "4A", "11A", "6A", "1A", "8A", "3A", "10A",
];

/// Resolve a track's key, preferring the numeric enum over free text.
pub fn resolve_key(musical_key: Option<i32>, info_key: Option<&str>) -> KeyOutcome {
    if let Some(value) = musical_key {
        if let Some(camelot) = musical_key_to_camelot(value) {
            return KeyOutcome::Camelot(camelot);
        }
    }
    if let Some(raw) = info_key.map(str::trim).filter(|s| !s.is_empty()) {
        return match resolve_info_key(raw) {
            Some(camelot) => KeyOutcome::Camelot(camelot),
            None => KeyOutcome::Unresolved(raw.to_string()),
        };
    }
    KeyOutcome::Missing
}

/// Map a `MUSICAL_KEY VALUE` enum to Camelot, or `None` when out of range.
pub fn musical_key_to_camelot(value: i32) -> Option<String> {
    if !(0..=23).contains(&value) {
        return None;
    }
    let minor = value >= 12;
    Some(camelot_for_pitch_class((value % 12) as u32, minor))
}

/// Resolve a free-text `INFO KEY` through Open Key, then Camelot, then note names.
pub fn resolve_info_key(raw: &str) -> Option<String> {
    open_key_to_camelot(raw)
        .or_else(|| camelot_code(raw))
        .or_else(|| note_name_to_camelot(raw))
}

/// Resolve true Open Key notation (`3d`, `9m`, `12d`) to Camelot.
pub fn open_key_to_camelot(raw: &str) -> Option<String> {
    let (stem, last) = split_trailing_char(raw.trim())?;
    let minor = match last {
        'm' | 'M' => true,
        'd' | 'D' => false,
        _ => return None,
    };
    let number: u32 = stem.trim().parse().ok()?;
    if !(1..=12).contains(&number) {
        return None;
    }
    Some(format!(
        "{}{}",
        rotate_open_key(number),
        if minor { 'A' } else { 'B' }
    ))
}

/// Resolve an already-Camelot code (`8B`, `12a`) to its canonical uppercase form.
pub fn camelot_code(raw: &str) -> Option<String> {
    let (stem, last) = split_trailing_char(raw.trim())?;
    let letter = match last {
        'A' | 'a' => 'A',
        'B' | 'b' => 'B',
        _ => return None,
    };
    let number: u32 = stem.trim().parse().ok()?;
    if !(1..=12).contains(&number) {
        return None;
    }
    Some(format!("{number}{letter}"))
}

/// Open Key number to Camelot number: `camelot = ((openkey + 7) % 12)`, with 12 in place of 0.
fn rotate_open_key(open_key: u32) -> u32 {
    match (open_key + 7) % 12 {
        0 => 12,
        value => value,
    }
}

/// A note name without a mode token (`Gb`, `B`) is ambiguous, so it does not resolve.
fn note_name_to_camelot(raw: &str) -> Option<String> {
    let normalized = raw.trim().replace(['♯', '＃'], "#").replace('♭', "b");
    let bytes = normalized.as_bytes();
    let root = (*bytes.first()? as char).to_ascii_uppercase();
    if !('A'..='G').contains(&root) {
        return None;
    }

    let mut index = 1;
    let mut accidental: i32 = 0;
    while let Some(&byte) = bytes.get(index) {
        match byte as char {
            '#' => accidental += 1,
            'b' => accidental -= 1,
            _ => break,
        }
        index += 1;
    }

    let minor = match normalized[index..].trim().to_ascii_lowercase().as_str() {
        "m" | "min" | "minor" | "moll" | "mol" => true,
        "maj" | "major" | "dur" => false,
        // No mode token: unguessable (major vs minor), so leave it unresolved.
        _ => return None,
    };

    let pitch = pitch_class(root, accidental)?;
    Some(camelot_for_pitch_class(pitch, minor))
}

fn pitch_class(root: char, accidental: i32) -> Option<u32> {
    let base = match root {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    Some((base + accidental).rem_euclid(12) as u32)
}

fn camelot_for_pitch_class(pitch: u32, minor: bool) -> String {
    let table = if minor { MINOR_CAMELOT } else { MAJOR_CAMELOT };
    table[(pitch % 12) as usize].to_string()
}

/// Split the last character off a non-empty string, returning `(stem, last)`.
fn split_trailing_char(raw: &str) -> Option<(&str, char)> {
    let last = raw.chars().last()?;
    Some((&raw[..raw.len() - last.len_utf8()], last))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camelot(musical_key: Option<i32>, info_key: &str) -> String {
        match resolve_key(musical_key, Some(info_key)) {
            KeyOutcome::Camelot(value) => value,
            other => panic!("expected a Camelot resolution, got {other:?}"),
        }
    }

    #[test]
    fn open_key_is_rotated_relative_to_camelot() {
        // The three anchors from the measured real collection.
        assert_eq!(camelot(None, "1d"), "8B"); // C major
        assert_eq!(camelot(None, "3d"), "10B"); // D major
        assert_eq!(camelot(None, "11d"), "6B"); // Bb major
        assert_eq!(camelot(None, "9m"), "4A"); // F minor
        assert_eq!(camelot(None, "12m"), "7A"); // D minor
        assert_eq!(camelot(None, "1m"), "8A"); // A minor
    }

    #[test]
    fn legacy_harmony_reading_is_not_used_on_this_path() {
        // The legacy rule reads `11d` as Camelot 11B; Traktor means 6B.
        assert_ne!(camelot(None, "11d"), "11B");
    }

    #[test]
    fn musical_key_enum_majors_and_minors() {
        assert_eq!(musical_key_to_camelot(0).as_deref(), Some("8B")); // C
        assert_eq!(musical_key_to_camelot(2).as_deref(), Some("10B")); // D
        assert_eq!(musical_key_to_camelot(10).as_deref(), Some("6B")); // Bb
        assert_eq!(musical_key_to_camelot(11).as_deref(), Some("1B")); // B
        assert_eq!(musical_key_to_camelot(12).as_deref(), Some("5A")); // Cm
        assert_eq!(musical_key_to_camelot(17).as_deref(), Some("4A")); // Fm
        assert_eq!(musical_key_to_camelot(21).as_deref(), Some("8A")); // Am
        assert_eq!(musical_key_to_camelot(23).as_deref(), Some("10A")); // Bm
        assert_eq!(musical_key_to_camelot(24), None);
        assert_eq!(musical_key_to_camelot(-1), None);
    }

    #[test]
    fn musical_key_enum_wins_over_free_text() {
        // "8B" would resolve to 8B; the enum (D major -> 10B) must win.
        assert_eq!(camelot(Some(2), "8B"), "10B");
    }

    #[test]
    fn camelot_and_note_names_resolve() {
        assert_eq!(camelot(None, "8B"), "8B");
        assert_eq!(camelot(None, "12a"), "12A");
        assert_eq!(camelot(None, "Ab Major"), "4B");
        assert_eq!(camelot(None, "Am"), "8A");
        assert_eq!(camelot(None, "Bbm"), "3A");
        assert_eq!(camelot(None, "F Minor"), "4A");
        assert_eq!(camelot(None, "E Minor"), "9A");
    }

    #[test]
    fn bare_note_name_is_unresolved() {
        assert_eq!(resolve_info_key("Gb"), None);
        assert_eq!(
            resolve_key(None, Some("Gb")),
            KeyOutcome::Unresolved("Gb".into())
        );
    }

    #[test]
    fn absent_key_is_missing() {
        assert_eq!(resolve_key(None, None), KeyOutcome::Missing);
        assert_eq!(resolve_key(None, Some("  ")), KeyOutcome::Missing);
    }

    #[test]
    fn out_of_range_enum_falls_back_to_info_key() {
        assert_eq!(camelot(Some(99), "3d"), "10B");
    }
}
