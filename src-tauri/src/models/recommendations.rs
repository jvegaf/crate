use serde::{Deserialize, Serialize};

/// One "similar tracks" recommendation from Beatport's catalog recommendations API.
///
/// Field names mirror the Beatport wire shape (`snake_case`) so the TypeScript
/// side can mirror this struct 1:1. Only the identity pair (`track_id`,
/// `track_name`) is required: Beatport omits other fields per item, and an item
/// without identity is dropped by the parser rather than surfaced with holes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatportRecommendation {
    pub track_id: u64,
    pub track_name: String,
    pub mix_name: Option<String>,
    pub track_length_ms: Option<i64>,
    pub bpm: Option<f64>,
    /// Musical key exactly as Beatport reports it. The live v1 payload uses
    /// long-form names (`"Bb Minor"`, `"G Major"`), and this value is passed
    /// through un-normalized — unlike the tagger's v4 search keys, which are
    /// shortened to Crate's notation (`"Gm"`).
    pub key: Option<String>,
    pub key_camelot: Option<String>,
    pub isrc: Option<String>,
    pub track_number: Option<u32>,
    /// Waveform *image*, not audio: Beatport serves it from the same
    /// `image_size/{w}x{h}` template as the artwork, already resolved here.
    pub track_waveform_url: Option<String>,
    /// Preview audio, trimmed to `sample_start_ms`..`sample_end_ms`.
    pub sample_url: Option<String>,
    pub sample_start_ms: Option<i64>,
    pub sample_end_ms: Option<i64>,
    #[serde(default)]
    pub artists: Vec<BeatportArtist>,
    pub genre: Option<BeatportGenre>,
    pub release: Option<BeatportRelease>,
    pub label: Option<BeatportNamedEntity>,
}

/// An artist credit on a recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatportArtist {
    pub id: Option<u64>,
    pub name: Option<String>,
    /// Credit role as Beatport reports it (wire key is `type`, e.g. main artist
    /// versus remixer).
    #[serde(rename = "type")]
    pub artist_type: Option<String>,
}

/// A genre with its optional sub-genre and top-level category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatportGenre {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub sub_genre: Option<BeatportNamedEntity>,
    pub category: Option<BeatportNamedEntity>,
}

/// A plain `{id, name}` reference — genre sub-category, genre category and label
/// all share this shape, so one struct covers them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatportNamedEntity {
    pub id: Option<u64>,
    pub name: Option<String>,
}

/// The release a recommendation belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatportRelease {
    pub id: Option<u64>,
    pub name: Option<String>,
    /// Artwork URL with Beatport's literal `{w}x{h}` size placeholder already
    /// replaced by a fixed display size (see the recommendations service). `None`
    /// when the item carries no release object at all.
    pub image_url: Option<String>,
    pub catalog_number: Option<String>,
    /// Release date text. Live items arrive as `"2025-10-17T00:00:00"` (no
    /// timezone suffix); the value is passed through untouched, so callers that
    /// need a date should read the leading `YYYY-MM-DD`.
    pub release_date: Option<String>,
}
