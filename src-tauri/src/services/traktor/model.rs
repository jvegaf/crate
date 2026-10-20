//! Typed model of a Traktor `collection.nml` document.
//!
//! These types mirror the XML closely (raw Traktor values, 0-based `HOTCUE`,
//! float milliseconds). Interpretation — path resolution, cue mapping, key
//! conversion — lives in [`crate::services::traktor::mapper`].

/// A parsed NML document.
#[derive(Debug, Clone, Default)]
pub struct NmlDocument {
    /// `NML VERSION` (e.g. `"20"`).
    pub version: Option<String>,
    /// `COLLECTION/ENTRY` tracks, in document order.
    pub tracks: Vec<NmlTrack>,
    /// The playlist forest. The virtual `$ROOT` folder is transparent, so this
    /// holds its children directly (nested folders included).
    pub playlists: Vec<NmlPlaylistNode>,
}

/// A `COLLECTION/ENTRY` track.
#[derive(Debug, Clone, Default)]
pub struct NmlTrack {
    pub audio_id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub location: NmlLocation,
    /// `ENTRY MODIFIED_DATE` (`YYYY/M/D`). Parsed and preserved, not yet mapped.
    pub modified_date: Option<String>,
    /// `ENTRY MODIFIED_TIME` (seconds of day).
    pub modified_time: Option<i32>,
    /// `ALBUM OF_TRACKS`: how many tracks the release carries.
    pub album_of_tracks: Option<i32>,
    /// `ALBUM TRACK`: this track's position on the release.
    pub album_track: Option<i32>,
    /// `INFO KEY`: Traktor's free-text key field (heterogeneous notation).
    pub info_key: Option<String>,
    /// `INFO BITRATE`, bits per second (e.g. `320000`).
    pub bitrate_bps: Option<i32>,
    pub genre: Option<String>,
    pub label: Option<String>,
    /// `INFO COMMENT`: Traktor's free-text comment. Crate has no column for it.
    pub comment: Option<String>,
    /// `INFO COVERARTID`: artwork reference (`"001/<base64>"` or empty).
    pub coverart_id: Option<String>,
    pub play_count: Option<i32>,
    /// `INFO PLAYTIME`, whole seconds.
    pub playtime_s: Option<i32>,
    /// `INFO PLAYTIME_FLOAT`, sub-second seconds.
    pub playtime_float: Option<f64>,
    /// `INFO RANKING`, Traktor's 0-255 scale.
    pub ranking: Option<i32>,
    /// `INFO IMPORT_DATE` (`YYYY/M/D`).
    pub import_date: Option<String>,
    /// `INFO LAST_PLAYED` (`YYYY/M/D`).
    pub last_played: Option<String>,
    /// `INFO RELEASE_DATE` (`YYYY/M/D`). Crate has no column for it.
    pub release_date: Option<String>,
    /// `INFO FLAGS`, Traktor's per-track bit flags.
    pub flags: Option<i32>,
    /// `INFO FILESIZE`, kilobytes. Crate has no column for it.
    pub filesize_kb: Option<i64>,
    /// `INFO REMIXER`.
    pub remixer: Option<String>,
    /// `MUSICAL_KEY VALUE`: 0–11 major, 12–23 minor.
    pub musical_key: Option<i32>,
    /// `TEMPO BPM`. Independent of the beat grid.
    pub tempo_bpm: Option<f64>,
    /// All `CUE_V2` children, in document order.
    pub cues: Vec<NmlCue>,
}

impl NmlTrack {
    /// The beat grid, read from the first `TYPE="4"` cue carrying a `<GRID>`.
    ///
    /// The grid is never detected by cue `NAME`: a real collection contains a
    /// grid cue named `Beat Marker` instead of `AutoGrid`.
    pub fn grid(&self) -> Option<NmlGrid> {
        self.cues
            .iter()
            .filter(|c| c.traktor_type == 4)
            .find_map(|c| {
                c.grid_bpm.map(|bpm| NmlGrid {
                    bpm,
                    position_ms: c.start_ms,
                })
            })
    }
}

/// A `LOCATION` element.
#[derive(Debug, Clone, Default)]
pub struct NmlLocation {
    pub volume: Option<String>,
    pub dir: String,
    pub file: String,
}

/// A `CUE_V2` element, kept as Traktor wrote it.
#[derive(Debug, Clone, Default)]
pub struct NmlCue {
    pub name: Option<String>,
    /// Traktor `TYPE`: 0 cue, 1 fade-in, 2 fade-out, 3 load, 4 beat grid, 5 loop.
    pub traktor_type: i32,
    /// `START`, float milliseconds.
    pub start_ms: f64,
    /// `LEN`, float milliseconds (the loop length for `TYPE="5"`).
    pub length_ms: f64,
    /// `HOTCUE`: 0-based slot, `-1` when the cue is not a hot cue.
    pub hot_cue: i32,
    /// Nested `GRID BPM`, present on real beat-grid cues only.
    pub grid_bpm: Option<f64>,
}

/// An extracted beat grid.
#[derive(Debug, Clone, Copy)]
pub struct NmlGrid {
    pub bpm: f64,
    /// `START` of the owning `TYPE="4"` cue, float milliseconds.
    pub position_ms: f64,
}

/// Kind of a `NODE` in the playlist tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NmlNodeKind {
    Folder,
    #[default]
    Playlist,
    Smartlist,
}

/// A `NODE` of the playlist tree.
#[derive(Debug, Clone, Default)]
pub struct NmlPlaylistNode {
    pub name: String,
    pub kind: NmlNodeKind,
    pub uuid: Option<String>,
    /// `SMARTLIST/SEARCH_EXPRESSION QUERY`, Traktor's own query language.
    pub search_query: Option<String>,
    /// `PLAYLIST/ENTRY/PRIMARYKEY KEY` values, in order.
    pub track_keys: Vec<String>,
    pub children: Vec<NmlPlaylistNode>,
}
