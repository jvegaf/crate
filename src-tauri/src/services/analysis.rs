use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use stratum_dsp::{analyze_audio, AnalysisConfig};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{CrateError, Result};
use crate::models::{KeyNotationFormat, Tag, Track};
use crate::services::cloud_sync::pipeline::{buckets, dirty};
use crate::services::FileTagsService;

/// Standard key notation (as emitted by stratum-dsp) to Camelot wheel codes.
///
/// Enharmonic variants (e.g. `Gb`, `Db`) are intentionally absent: they resolve to the
/// same Camelot code as their sharp spellings, and the DSP emits a specific spelling that
/// is converted verbatim.
static STANDARD_TO_CAMELOT: std::sync::LazyLock<HashMap<&'static str, &'static str>> =
    std::sync::LazyLock::new(|| {
        HashMap::from([
            ("C", "8B"),
            ("G", "9B"),
            ("D", "10B"),
            ("A", "11B"),
            ("E", "12B"),
            ("B", "1B"),
            ("F#", "2B"),
            ("C#", "3B"),
            ("Ab", "4B"),
            ("Eb", "5B"),
            ("Bb", "6B"),
            ("F", "7B"),
            ("Am", "8A"),
            ("Em", "9A"),
            ("Bm", "10A"),
            ("F#m", "11A"),
            ("C#m", "12A"),
            ("G#m", "1A"),
            ("D#m", "2A"),
            ("Ebm", "2A"),
            ("A#m", "3A"),
            ("Fm", "4A"),
            ("Cm", "5A"),
            ("Gm", "6A"),
            ("Dm", "7A"),
        ])
    });

/// Inverse of [`STANDARD_TO_CAMELOT`], used to render Camelot keys as standard notation.
///
/// Built from the forward table in a fixed order so the single enharmonic collision
/// (`2A`) deterministically resolves to `Ebm`, matching the frontend reverse map.
static CAMELOT_TO_STANDARD: std::sync::LazyLock<HashMap<&'static str, &'static str>> =
    std::sync::LazyLock::new(|| {
        let mut pairs: Vec<(&'static str, &'static str)> = STANDARD_TO_CAMELOT
            .iter()
            .map(|(standard, camelot)| (*camelot, *standard))
            .collect();
        pairs.sort_unstable();
        pairs.into_iter().collect()
    });

/// True when `stem` is one of the Camelot wheel numbers (1–12).
fn is_camelot_number(stem: &str) -> bool {
    matches!(
        stem,
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "10" | "11" | "12"
    )
}

/// Resolve whatever is currently stored in `tracks.key` to its Camelot wheel code.
///
/// Three storage shapes resolve to Camelot:
/// - Camelot codes themselves (`8A`, `12B`), with the letter normalized to uppercase.
/// - Standard notation spellings (`Am`, `F#m`), via [`STANDARD_TO_CAMELOT`].
/// - Legacy harmony `m`/`d` values (`11d` = Camelot 11B, `1m` = Camelot 1A): the number
///   is kept and `m` → `A`, `d` → `B`, case-insensitively. This is the *harmony
///   convention*, NOT true OpenKey — no rotation applies.
///
/// Anything else resolves to `None`: never invent a key.
fn resolve_stored_key_to_camelot(key: &str) -> Option<String> {
    let bytes = key.as_bytes();
    let camelot_or_legacy = bytes.split_last().and_then(|(last, stem)| {
        let stem = std::str::from_utf8(stem).ok()?;
        if !is_camelot_number(stem) {
            return None;
        }
        // A/B is Camelot; m/d is the legacy harmony convention. Both store as Camelot.
        let letter = match last {
            b'A' | b'a' | b'm' | b'M' => 'A',
            b'B' | b'b' | b'd' | b'D' => 'B',
            _ => return None,
        };
        Some(format!("{stem}{letter}"))
    });
    camelot_or_legacy.or_else(|| {
        STANDARD_TO_CAMELOT
            .get(key)
            .map(|converted| (*converted).to_string())
    })
}

/// Convert a stored or freshly detected key to the requested notation format.
///
/// `format` is the raw setting string. Unrecognized keys and unknown format strings are
/// returned unchanged; an unknown format is logged as a warning.
///
/// **OpenKey is display-only: its storage form is Camelot.** Camelot (`A`/`B`) and
/// standard notation (note names) are self-describing, but OpenKey's `m`/`d` alphabet is
/// shared with the legacy harmony convention already present in `tracks.key`. Storing
/// OpenKey would make that data ambiguous and any bulk migration non-idempotent and
/// silently corrupting, so the OpenKey target intentionally resolves to Camelot codes and
/// must NOT be "fixed" into a rotation. True OpenKey rendering is a frontend display
/// concern, never a storage one.
///
/// The Standard arm resolves the stored value to Camelot first, then renders the note
/// name via [`CAMELOT_TO_STANDARD`]: legacy harmony values migrate, Camelot codes
/// canonicalize to the reverse map's canonical spellings (e.g. `D#m` → `Ebm`), and
/// spellings the resolver or reverse map does not hold pass through unchanged.
fn apply_key_conversion(key: &str, format: &str) -> String {
    match format.parse::<KeyNotationFormat>() {
        Ok(KeyNotationFormat::Camelot) | Ok(KeyNotationFormat::OpenKey) => {
            resolve_stored_key_to_camelot(key).unwrap_or_else(|| key.to_string())
        }
        Ok(KeyNotationFormat::Standard) => resolve_stored_key_to_camelot(key)
            .and_then(|camelot| {
                CAMELOT_TO_STANDARD
                    .get(camelot.as_str())
                    .map(|converted| (*converted).to_string())
            })
            .unwrap_or_else(|| key.to_string()),
        Err(_) => {
            log::warn!("Unknown key notation format: {format}, keeping key as-is");
            key.to_string()
        }
    }
}

/// Result of analyzing a single track
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub track_id: String,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

/// Status of an analysis operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    Pending,
    Analyzing,
    Completed,
    Failed,
    Cancelled,
}

/// Per-track analysis event for real-time UI updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackAnalysisEvent {
    pub track_id: String,
    pub state: AnalysisStatus,
    pub result: Option<AnalysisResult>,
    pub updated_track: Option<Track>,
    pub error: Option<String>,
}

/// State for a single track's analysis task
struct TrackAnalysisTask {
    cancel_token: CancellationToken,
    #[allow(dead_code)]
    handle: JoinHandle<()>,
}

/// Upper bound on concurrent analyses.
///
/// Analysis is CPU-bound and `spawn_blocking` sizes its pool independently of the core count, so
/// without an explicit ceiling a large batch pins every core and freezes the host.
const ANALYSIS_WORKER_LIMIT_MAX: usize = 4;

/// How many tracks may be analyzed at once: half the available cores, capped at
/// [`ANALYSIS_WORKER_LIMIT_MAX`] and never zero. A host that refuses to report its parallelism is
/// treated as a small machine rather than as an unbounded one.
fn analysis_worker_limit() -> usize {
    std::thread::available_parallelism()
        .map(|cores| (cores.get() / 2).clamp(1, ANALYSIS_WORKER_LIMIT_MAX))
        .unwrap_or(2)
}

pub struct AnalysisService {
    conn: Arc<Mutex<Connection>>,
    tasks: Arc<Mutex<HashMap<String, TrackAnalysisTask>>>,
    /// Worker slots bounding how much analysis runs at once.
    slots: Arc<Semaphore>,
    file_tags: FileTagsService,
}

impl AnalysisService {
    pub fn new(conn: Arc<Mutex<Connection>>, file_tags: FileTagsService) -> Self {
        Self {
            conn,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            slots: Arc::new(Semaphore::new(analysis_worker_limit())),
            file_tags,
        }
    }

    /// Cancel analysis for a specific track
    pub fn cancel_track_analysis(&self, track_id: &str) -> Result<bool> {
        let mut tasks = self.tasks.lock().map_err(|_| CrateError::LockPoisoned)?;
        if let Some(task) = tasks.remove(track_id) {
            task.cancel_token.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Cancel all running analysis tasks
    pub fn cancel_all_analysis(&self) -> Result<()> {
        let mut tasks = self.tasks.lock().map_err(|_| CrateError::LockPoisoned)?;
        for (_, task) in tasks.drain() {
            task.cancel_token.cancel();
        }
        Ok(())
    }

    /// Take one of the service's worker slots, giving up immediately if the track is cancelled
    /// while it waits in the queue.
    ///
    /// `biased` puts the cancellation arm first, so a track cancelled while queued releases at once
    /// instead of waiting for a slot it is about to discard.
    async fn acquire_analysis_slot(
        slots: &Arc<Semaphore>,
        cancel_token: &CancellationToken,
    ) -> Option<OwnedSemaphorePermit> {
        tokio::select! {
            biased;
            _ = cancel_token.cancelled() => None,
            permit = slots.clone().acquire_owned() => permit.ok(),
        }
    }

    /// Emit the terminal `Cancelled` event for a track and drop it from the task map.
    fn emit_cancelled(
        app: &AppHandle,
        track_id: &str,
        tasks: &Arc<Mutex<HashMap<String, TrackAnalysisTask>>>,
    ) {
        let _ = app.emit(
            "analysis-track-event",
            TrackAnalysisEvent {
                track_id: track_id.to_string(),
                state: AnalysisStatus::Cancelled,
                result: None,
                updated_track: None,
                error: None,
            },
        );
        if let Ok(mut t) = tasks.lock() {
            t.remove(track_id);
        }
    }

    /// Read the stored BPM/key for a track. Returns `Some` only when BOTH are present,
    /// because a partial analysis is not a reason to skip the DSP.
    fn get_existing_analysis(
        conn: &Arc<Mutex<Connection>>,
        track_id: &str,
    ) -> Result<Option<(f64, String)>> {
        let conn = conn.lock().map_err(|_| CrateError::LockPoisoned)?;
        let row = conn.query_row(
            "SELECT bpm, key FROM tracks WHERE id = ?1",
            [track_id],
            |row| {
                Ok((
                    row.get::<_, Option<f64>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )?;
        Ok(match row {
            (Some(bpm), Some(key)) => Some((bpm, key)),
            _ => None,
        })
    }

    /// Emit a terminal `Completed` event for a track that was skipped, so the UI clears
    /// its pending state without any DSP having run.
    fn emit_skip_completion(
        app: &AppHandle,
        track_id: &str,
        bpm: f64,
        key: String,
        tasks: &Arc<Mutex<HashMap<String, TrackAnalysisTask>>>,
    ) {
        let _ = app.emit(
            "analysis-track-event",
            TrackAnalysisEvent {
                track_id: track_id.to_string(),
                state: AnalysisStatus::Completed,
                result: Some(AnalysisResult {
                    track_id: track_id.to_string(),
                    bpm: Some(bpm),
                    key: Some(key),
                    success: true,
                    error: None,
                }),
                updated_track: None,
                error: None,
            },
        );
        if let Ok(mut t) = tasks.lock() {
            t.remove(track_id);
        }
    }

    /// Analyze multiple tracks with per-track events (async, non-blocking)
    pub async fn analyze_tracks_async(
        &self,
        app_handle: AppHandle,
        track_ids: Vec<String>,
        force: bool,
        key_notation_format: Option<String>,
    ) -> Result<()> {
        for track_id in track_ids {
            let cancel_token = CancellationToken::new();
            let conn = self.conn.clone();
            let app = app_handle.clone();
            let tid = track_id.clone();
            let token = cancel_token.clone();
            let tasks = self.tasks.clone();
            let slots = self.slots.clone();
            let format = key_notation_format.clone();
            let file_tags = self.file_tags.clone();

            // Emit "pending" event immediately
            let _ = app.emit(
                "analysis-track-event",
                TrackAnalysisEvent {
                    track_id: tid.clone(),
                    state: AnalysisStatus::Pending,
                    result: None,
                    updated_track: None,
                    error: None,
                },
            );

            let handle = tauri::async_runtime::spawn(async move {
                Self::analyze_single_track_task(
                    conn, app, tid, token, tasks, slots, force, format, file_tags,
                )
                .await;
            });

            // Store task for potential cancellation
            let mut tasks_guard = self.tasks.lock().map_err(|_| CrateError::LockPoisoned)?;
            tasks_guard.insert(
                track_id.clone(),
                TrackAnalysisTask {
                    cancel_token,
                    handle,
                },
            );
        }

        Ok(())
    }

    /// Single track analysis task - runs in its own Tokio task
    ///
    /// The argument list is the per-track context needed by the spawned task; grouping it into a
    /// struct would only move the same parameters one level away from the single call site.
    #[allow(clippy::too_many_arguments)]
    async fn analyze_single_track_task(
        conn: Arc<Mutex<Connection>>,
        app: AppHandle,
        track_id: String,
        cancel_token: CancellationToken,
        tasks: Arc<Mutex<HashMap<String, TrackAnalysisTask>>>,
        slots: Arc<Semaphore>,
        force: bool,
        key_notation_format: Option<String>,
        file_tags: FileTagsService,
    ) {
        // Check if already cancelled before starting
        if cancel_token.is_cancelled() {
            Self::emit_cancelled(&app, &track_id, &tasks);
            return;
        }

        // Skip tracks that already carry BPM and key (typically from their own tags at import)
        // unless the caller explicitly forced a re-analysis.
        if !force {
            match Self::get_existing_analysis(&conn, &track_id) {
                Ok(Some((bpm, key))) => {
                    Self::emit_skip_completion(&app, &track_id, bpm, key, &tasks);
                    return;
                }
                Ok(None) => {}
                Err(e) => {
                    // A lookup failure must not silently skip analysis; fall through to the
                    // normal path so the real error surfaces the usual way.
                    log::warn!("Failed to check existing analysis for {track_id}: {e}");
                }
            }
        }

        // Wait for a free worker slot before any heavy work, and hold it until this track is done.
        // The `Analyzing` emit stays behind the acquire: a queued track is `pending`, not running.
        let _slot = match Self::acquire_analysis_slot(&slots, &cancel_token).await {
            Some(permit) => permit,
            None => {
                Self::emit_cancelled(&app, &track_id, &tasks);
                return;
            }
        };

        // Emit "analyzing" status
        let _ = app.emit(
            "analysis-track-event",
            TrackAnalysisEvent {
                track_id: track_id.clone(),
                state: AnalysisStatus::Analyzing,
                result: None,
                updated_track: None,
                error: None,
            },
        );

        // Run analysis on blocking thread pool with cancellation
        let conn_clone = conn.clone();
        let tid_clone = track_id.clone();
        let token_clone = cancel_token.clone();
        let file_tags_clone = file_tags.clone();

        let result = tokio::task::spawn_blocking(move || {
            Self::analyze_track_with_cancellation(
                &conn_clone,
                &tid_clone,
                &token_clone,
                key_notation_format.as_deref(),
                &file_tags_clone,
            )
        })
        .await;

        // Check if cancelled during analysis
        if cancel_token.is_cancelled() {
            Self::emit_cancelled(&app, &track_id, &tasks);
            return;
        }

        // Handle result
        match result {
            Ok(Ok((analysis_result, updated_track))) => {
                let _ = app.emit(
                    "analysis-track-event",
                    TrackAnalysisEvent {
                        track_id: track_id.clone(),
                        state: AnalysisStatus::Completed,
                        result: Some(analysis_result),
                        updated_track,
                        error: None,
                    },
                );
            }
            Ok(Err(e)) => {
                let _ = app.emit(
                    "analysis-track-event",
                    TrackAnalysisEvent {
                        track_id: track_id.clone(),
                        state: AnalysisStatus::Failed,
                        result: None,
                        updated_track: None,
                        error: Some(e.to_string()),
                    },
                );
            }
            Err(e) => {
                let _ = app.emit(
                    "analysis-track-event",
                    TrackAnalysisEvent {
                        track_id: track_id.clone(),
                        state: AnalysisStatus::Failed,
                        result: None,
                        updated_track: None,
                        error: Some(format!("Task panicked: {e}")),
                    },
                );
            }
        }

        // Clean up task from map
        if let Ok(mut t) = tasks.lock() {
            t.remove(&track_id);
        }
    }

    /// Analyze a track with cancellation support - runs on blocking thread
    fn analyze_track_with_cancellation(
        conn: &Arc<Mutex<Connection>>,
        track_id: &str,
        cancel_token: &CancellationToken,
        key_notation_format: Option<&str>,
        file_tags: &FileTagsService,
    ) -> Result<(AnalysisResult, Option<Track>)> {
        // Get track from database
        let track = Self::get_track_static(conn, track_id)?;
        let file_path = Path::new(&track.file_path);

        if !file_path.exists() {
            return Ok((
                AnalysisResult {
                    track_id: track_id.to_string(),
                    bpm: None,
                    key: None,
                    success: false,
                    error: Some(format!("File not found: {}", track.file_path)),
                },
                None,
            ));
        }

        // Check cancellation before starting heavy work
        if cancel_token.is_cancelled() {
            return Err(CrateError::Analysis("Cancelled".to_string()));
        }

        // Analyze the audio file with cancellation checks
        match Self::analyze_audio_file_with_cancellation(file_path, cancel_token) {
            Ok((bpm, key)) => {
                // Check cancellation before saving
                if cancel_token.is_cancelled() {
                    return Err(CrateError::Analysis("Cancelled".to_string()));
                }

                // stratum-dsp always emits standard notation; convert it to the caller's
                // preferred notation before persisting when one was requested.
                let key = match (key, key_notation_format) {
                    (Some(key), Some(format)) => Some(apply_key_conversion(&key, format)),
                    (key, _) => key,
                };

                // Update the database
                Self::update_track_analysis_static(conn, track_id, bpm, key.as_deref())?;

                // Persist BPM/key back to the audio file.
                let _ = file_tags.write_bpm_and_key(file_path, bpm.unwrap_or(0.0), key.as_deref());

                // Get updated track
                let updated_track = Self::get_track_static(conn, track_id).ok();

                Ok((
                    AnalysisResult {
                        track_id: track_id.to_string(),
                        bpm,
                        key,
                        success: true,
                        error: None,
                    },
                    updated_track,
                ))
            }
            Err(e) if e.to_string().contains("Cancelled") => {
                Err(CrateError::Analysis("Cancelled".to_string()))
            }
            Err(e) => Ok((
                AnalysisResult {
                    track_id: track_id.to_string(),
                    bpm: None,
                    key: None,
                    success: false,
                    error: Some(e.to_string()),
                },
                None,
            )),
        }
    }

    /// Analyze an audio file for BPM and key with cancellation support
    fn analyze_audio_file_with_cancellation(
        path: &Path,
        cancel_token: &CancellationToken,
    ) -> Result<(Option<f64>, Option<String>)> {
        // Decode audio to mono f32 samples with cancellation checks
        let (samples, sample_rate) = Self::decode_audio_with_cancellation(path, cancel_token)?;

        if samples.is_empty() {
            return Err(CrateError::Analysis("No audio samples found".to_string()));
        }

        // Check cancellation before analysis
        if cancel_token.is_cancelled() {
            return Err(CrateError::Analysis("Cancelled".to_string()));
        }

        // Analyze using stratum-dsp
        let result = analyze_audio(&samples, sample_rate, AnalysisConfig::default())
            .map_err(|e| CrateError::Analysis(format!("Analysis failed: {e}")))?;

        // Round BPM to nearest integer (most tracks are produced at whole BPMs)
        let bpm = Some((result.bpm as f64).round());
        let key = Some(result.key.name().to_string());

        Ok((bpm, key))
    }

    /// Decode audio file to mono f32 samples with cancellation checks
    fn decode_audio_with_cancellation(
        path: &Path,
        cancel_token: &CancellationToken,
    ) -> Result<(Vec<f32>, u32)> {
        let file = File::open(path).map_err(|e| CrateError::Analysis(e.to_string()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        let decoder_opts = DecoderOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|e| CrateError::Analysis(format!("Failed to probe audio: {e}")))?;

        let mut format = probed.format;

        let track = format
            .default_track()
            .ok_or_else(|| CrateError::Analysis("No audio track found".to_string()))?;

        let sample_rate = track
            .codec_params
            .sample_rate
            .ok_or_else(|| CrateError::Analysis("Unknown sample rate".to_string()))?;

        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

        let track_id = track.id;

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
            .map_err(|e| CrateError::Analysis(format!("Failed to create decoder: {e}")))?;

        let mut samples: Vec<f32> = Vec::new();
        let mut packet_count: u32 = 0;

        // Decode all packets with cancellation checks
        loop {
            // Check cancellation every 100 packets
            packet_count += 1;
            if packet_count.is_multiple_of(100) && cancel_token.is_cancelled() {
                return Err(CrateError::Analysis("Cancelled".to_string()));
            }

            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => {
                    log::warn!("Error reading packet: {e}");
                    break;
                }
            };

            // Skip packets from other tracks
            if packet.track_id() != track_id {
                continue;
            }

            let decoded = match decoder.decode(&packet) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("Error decoding packet: {e}");
                    continue;
                }
            };

            // Convert to f32 samples
            let spec = *decoded.spec();
            let num_frames = decoded.frames();

            let mut sample_buf = SampleBuffer::<f32>::new(num_frames as u64, spec);
            sample_buf.copy_interleaved_ref(decoded);

            let interleaved = sample_buf.samples();

            // Convert to mono by averaging channels
            for chunk in interleaved.chunks(channels) {
                let mono: f32 = chunk.iter().sum::<f32>() / channels as f32;
                samples.push(mono);
            }
        }

        Ok((samples, sample_rate))
    }

    /// Static version of update_track_analysis for use in blocking context
    fn update_track_analysis_static(
        conn: &Arc<Mutex<Connection>>,
        track_id: &str,
        bpm: Option<f64>,
        key: Option<&str>,
    ) -> Result<()> {
        let conn = conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let now = chrono::Utc::now().to_rfc3339();
        let hlc = dirty::next_hlc(&conn)?;

        conn.execute(
            "UPDATE tracks SET bpm = ?1, key = ?2, analysis_source = 'crate', date_modified = ?3, _hlc = ?4 WHERE id = ?5",
            rusqlite::params![bpm, key, now, hlc, track_id],
        )?;
        dirty::mark_dirty(&conn, &buckets::bucket_for_track_id(track_id))?;

        Ok(())
    }

    /// Static version of get_track for use in blocking context
    fn get_track_static(conn: &Arc<Mutex<Connection>>, id: &str) -> Result<Track> {
        let conn = conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let mut stmt = conn.prepare(
            r#"
            SELECT id, file_path, file_hash,
                   title, artist, album, year, genre, label, catalog_number,
                   duration_ms, bpm, key, bitrate, sample_rate, format,
                   analysis_source, waveform_data,
                   rating, play_count,
                   date_added, date_modified, last_played,
                   rekordbox_id, artwork_path, artwork_source, color,
                   library_root_id, relative_path, url
            FROM tracks
            WHERE id = ?1
            "#,
        )?;

        let mut track = stmt.query_row([id], |row| {
            Ok(Track {
                id: row.get(0)?,
                file_path: row.get(1)?,
                file_hash: row.get(2)?,
                title: row.get(3)?,
                artist: row.get(4)?,
                album: row.get(5)?,
                year: row.get(6)?,
                genre: row.get(7)?,
                label: row.get(8)?,
                catalog_number: row.get(9)?,
                duration_ms: row.get(10)?,
                bpm: row.get(11)?,
                key: row.get(12)?,
                bitrate: row.get(13)?,
                sample_rate: row.get(14)?,
                format: row.get(15)?,
                analysis_source: row.get(16)?,
                waveform_data: row.get(17)?,
                rating: row.get(18)?,
                play_count: row.get(19)?,
                date_added: row.get(20)?,
                date_modified: row.get(21)?,
                last_played: row.get(22)?,
                rekordbox_id: row.get(23)?,
                artwork_path: row.get(24)?,
                artwork_source: row.get(25)?,
                color: row.get(26)?,
                library_root_id: row.get(27)?,
                relative_path: row.get(28)?,
                url: row.get(29)?,
                tags: Vec::new(),
            })
        })?;

        // Fetch tags
        Self::fetch_tags_for_track_static(&conn, &mut track)?;
        Ok(track)
    }

    /// Static version of fetch_tags for use in blocking context
    fn fetch_tags_for_track_static(conn: &Connection, track: &mut Track) -> Result<()> {
        let mut stmt = conn.prepare(
            r#"
            SELECT t.id, t.category_id, t.name, t.color, t.sort_order
            FROM track_tags tt
            JOIN tags t ON tt.tag_id = t.id
            WHERE tt.track_id = ?1
            "#,
        )?;

        let tags = stmt
            .query_map([&track.id], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    category_id: row.get(1)?,
                    name: row.get(2)?,
                    color: row.get(3)?,
                    sort_order: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        track.tags = tags;
        Ok(())
    }

    /// Cancel the current analysis operation (cancels all)
    pub fn cancel_analysis(&self) -> Result<()> {
        self.cancel_all_analysis()
    }

    /// Get a track by ID
    fn get_track(&self, id: &str) -> Result<Track> {
        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let mut stmt = conn.prepare(
            r#"
            SELECT id, file_path, file_hash,
                   title, artist, album, year, genre, label, catalog_number,
                   duration_ms, bpm, key, bitrate, sample_rate, format,
                   analysis_source, waveform_data,
                   rating, play_count,
                   date_added, date_modified, last_played,
                   rekordbox_id, artwork_path, artwork_source, color,
                   library_root_id, relative_path, url
            FROM tracks
            WHERE id = ?1
            "#,
        )?;

        let track = stmt.query_row([id], |row| {
            Ok(Track {
                id: row.get(0)?,
                file_path: row.get(1)?,
                file_hash: row.get(2)?,
                title: row.get(3)?,
                artist: row.get(4)?,
                album: row.get(5)?,
                year: row.get(6)?,
                genre: row.get(7)?,
                label: row.get(8)?,
                catalog_number: row.get(9)?,
                duration_ms: row.get(10)?,
                bpm: row.get(11)?,
                key: row.get(12)?,
                bitrate: row.get(13)?,
                sample_rate: row.get(14)?,
                format: row.get(15)?,
                analysis_source: row.get(16)?,
                waveform_data: row.get(17)?,
                rating: row.get(18)?,
                play_count: row.get(19)?,
                date_added: row.get(20)?,
                date_modified: row.get(21)?,
                last_played: row.get(22)?,
                rekordbox_id: row.get(23)?,
                artwork_path: row.get(24)?,
                artwork_source: row.get(25)?,
                color: row.get(26)?,
                library_root_id: row.get(27)?,
                relative_path: row.get(28)?,
                url: row.get(29)?,
                tags: Vec::new(),
            })
        })?;

        let tracks_with_tags = self.fetch_tags_for_tracks(&conn, vec![track])?;
        tracks_with_tags
            .into_iter()
            .next()
            .ok_or_else(|| CrateError::TrackNotFound(id.to_string()))
    }

    fn fetch_tags_for_tracks(
        &self,
        conn: &Connection,
        mut tracks: Vec<Track>,
    ) -> Result<Vec<Track>> {
        if tracks.is_empty() {
            return Ok(tracks);
        }

        let track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        let placeholders: Vec<String> = track_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();

        let sql = format!(
            r#"
            SELECT tt.track_id, t.id, t.category_id, t.name, t.color, t.sort_order
            FROM track_tags tt
            JOIN tags t ON tt.tag_id = t.id
            WHERE tt.track_id IN ({})
            "#,
            placeholders.join(", ")
        );

        let params_refs: Vec<&dyn rusqlite::ToSql> = track_ids
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let mut stmt = conn.prepare(&sql)?;
        let tag_rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    Tag {
                        id: row.get(1)?,
                        category_id: row.get(2)?,
                        name: row.get(3)?,
                        color: row.get(4)?,
                        sort_order: row.get(5)?,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut tags_by_track: std::collections::HashMap<String, Vec<Tag>> =
            std::collections::HashMap::new();
        for (track_id, tag) in tag_rows {
            tags_by_track.entry(track_id).or_default().push(tag);
        }

        for track in &mut tracks {
            if let Some(tags) = tags_by_track.remove(&track.id) {
                track.tags = tags;
            }
        }

        Ok(tracks)
    }

    /// Get updated track after analysis (for returning to frontend)
    pub fn get_updated_track(&self, track_id: &str) -> Result<Track> {
        self.get_track(track_id)
    }

    /// Recalculate all track keys in the library to the target notation format.
    ///
    /// Only updates tracks whose stored key actually changes. Returns the count of updated
    /// tracks. The database lock is held only for synchronous read/update work.
    pub fn recalculate_all_keys(&self, target_format: String) -> Result<i32> {
        use rusqlite::params;

        const BATCH_SIZE: u32 = 500;

        let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

        let mut total_updated = 0i32;
        let mut cursor_offset: u32 = 0;

        loop {
            let mut stmt = conn.prepare(
        "SELECT id, key FROM tracks WHERE key IS NOT NULL ORDER BY rowid LIMIT ?1 OFFSET ?2",
      )?;

            let batch: Vec<(String, String)> = stmt
                .query_map(params![BATCH_SIZE, cursor_offset], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            if batch.is_empty() {
                break;
            }

            for (track_id, stored_key) in &batch {
                let converted = apply_key_conversion(stored_key, &target_format);
                if converted != *stored_key {
                    conn.execute(
                        "UPDATE tracks SET key = ?1 WHERE id = ?2 AND key != ?1",
                        params![converted, track_id],
                    )?;
                    total_updated += 1;
                }
            }

            cursor_offset += BATCH_SIZE;
        }

        Ok(total_updated)
    }
}

impl Clone for AnalysisService {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            tasks: self.tasks.clone(),
            // Shared on purpose: a clone must not bring its own set of slots, or every clone would
            // multiply the ceiling instead of honoring it.
            slots: self.slots.clone(),
            file_tags: self.file_tags.clone(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../test_utils.rs"]
mod test_utils;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Bounds the derived ceiling: never zero, never above the cap.
    #[test]
    fn worker_limit_is_never_zero_and_never_above_the_cap() {
        let limit = analysis_worker_limit();

        assert!(limit >= 1, "a zero limit would stall the queue forever");
        assert!(limit <= ANALYSIS_WORKER_LIMIT_MAX);
    }

    /// The service must expose exactly the ceiling it advertises.
    #[test]
    fn service_offers_exactly_the_worker_limit() {
        use crate::services::FileTagsService;
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let service = AnalysisService::new(conn, FileTagsService::new());

        assert_eq!(service.slots.available_permits(), analysis_worker_limit());
    }

    /// A clone shares the slot pool instead of doubling it.
    #[test]
    fn clones_share_one_slot_pool() {
        use crate::services::FileTagsService;
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let service = AnalysisService::new(conn, FileTagsService::new());
        let clone = service.clone();

        let taken = service.slots.clone().try_acquire_owned().unwrap();

        assert_eq!(clone.slots.available_permits(), analysis_worker_limit() - 1);

        drop(taken);
    }

    /// The gate has to actually block: with the only slot taken, the next waiter waits until that
    /// permit is released.
    #[tokio::test]
    async fn a_taken_slot_blocks_the_next_waiter() {
        let slots = Arc::new(Semaphore::new(1));

        let held = AnalysisService::acquire_analysis_slot(&slots, &CancellationToken::new())
            .await
            .expect("the only slot must be free");

        let mut waiter = {
            let slots = slots.clone();
            tokio::spawn(async move {
                AnalysisService::acquire_analysis_slot(&slots, &CancellationToken::new()).await
            })
        };

        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut waiter)
                .await
                .is_err(),
            "the waiter acquired a slot that was already taken"
        );

        drop(held);

        let permit = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("releasing the slot must let the waiter through")
            .expect("the waiter task must not panic");

        assert!(permit.is_some());
    }

    /// A track cancelled while queued gives up immediately and takes nothing, even with every slot
    /// occupied.
    #[tokio::test]
    async fn a_cancelled_waiter_gives_up_without_taking_a_slot() {
        let slots = Arc::new(Semaphore::new(1));

        let held = AnalysisService::acquire_analysis_slot(&slots, &CancellationToken::new())
            .await
            .expect("the only slot must be free");

        let cancelled = CancellationToken::new();
        cancelled.cancel();

        let permit = tokio::time::timeout(
            Duration::from_secs(5),
            AnalysisService::acquire_analysis_slot(&slots, &cancelled),
        )
        .await
        .expect("a cancelled track must not block on the queue");

        assert!(permit.is_none(), "a cancelled track must not take a slot");

        drop(held);
        assert_eq!(
            slots.available_permits(),
            1,
            "the cancelled track leaked a slot"
        );
    }

    /// The skip guard only fires on a complete pair: a row missing either half must not
    /// be treated as already analyzed.
    #[test]
    fn get_existing_analysis_only_returns_a_complete_pair() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        conn.lock()
            .unwrap()
            .execute_batch("CREATE TABLE tracks (id TEXT PRIMARY KEY, bpm REAL, key TEXT)")
            .unwrap();

        let insert = |id: &str, bpm: Option<f64>, key: Option<&str>| {
            conn.lock()
                .unwrap()
                .execute(
                    "INSERT INTO tracks (id, bpm, key) VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, bpm, key],
                )
                .unwrap();
        };
        insert("complete", Some(120.0), Some("Am"));
        insert("bpm-only", Some(120.0), None);
        insert("neither", None, None);

        assert_eq!(
            AnalysisService::get_existing_analysis(&conn, "complete").unwrap(),
            Some((120.0, "Am".to_string()))
        );
        assert!(AnalysisService::get_existing_analysis(&conn, "bpm-only")
            .unwrap()
            .is_none());
        assert!(AnalysisService::get_existing_analysis(&conn, "neither")
            .unwrap()
            .is_none());
    }

    /// The instance-side positional map (`get_track`, reached via the public
    /// `get_updated_track`) must send every SELECT column to its own Track field.
    #[test]
    fn get_updated_track_maps_every_column_to_its_own_value() {
        use crate::services::FileTagsService;

        let track = test_utils::sentinel_track();
        let conn = Arc::new(Mutex::new(test_utils::make_memory_db()));
        test_utils::insert_sentinel_track(&conn.lock().unwrap(), &track);
        let service = AnalysisService::new(conn, FileTagsService::new());

        let read = service.get_updated_track(&track.id).unwrap();
        test_utils::assert_track_eq(&read, &track);
    }

    /// The blocking-side positional map (`get_track_static`, run inside every analysis
    /// job before and after the DB write) must do the same.
    #[test]
    fn get_track_static_maps_every_column_to_its_own_value() {
        let track = test_utils::sentinel_track();
        let conn = Arc::new(Mutex::new(test_utils::make_memory_db()));
        test_utils::insert_sentinel_track(&conn.lock().unwrap(), &track);

        let read = AnalysisService::get_track_static(&conn, &track.id).unwrap();
        test_utils::assert_track_eq(&read, &track);
    }

    // --- Key conversion: legacy harmony resolution + storage-form contracts ---

    /// Legacy harmony values (`11d` = Camelot 11B, `1m` = Camelot 1A) must resolve to
    /// Camelot on the storage path, case-insensitively. These are NOT true OpenKey.
    #[test]
    fn legacy_harmony_keys_resolve_to_camelot() {
        assert_eq!(apply_key_conversion("11d", "camelot"), "11B");
        assert_eq!(apply_key_conversion("1m", "camelot"), "1A");
        assert_eq!(apply_key_conversion("11D", "camelot"), "11B");
        assert_eq!(apply_key_conversion("12m", "camelot"), "12A");
        assert_eq!(apply_key_conversion("1M", "camelot"), "1A");
    }

    /// OpenKey is display-only: its storage form IS Camelot. A rotation here would be a
    /// bug (a rotation would turn `8A` into `1d`) and would make bulk migration
    /// non-idempotent, because `m`/`d` is shared with the legacy harmony convention.
    #[test]
    fn openkey_target_stores_camelot_not_rotated_openkey() {
        assert_eq!(apply_key_conversion("11d", "openkey"), "11B");
        assert_eq!(apply_key_conversion("8A", "openkey"), "8A");
        assert_eq!(apply_key_conversion("1m", "openkey"), "1A");
    }

    /// Values the resolver cannot map, and already-canonical values, pass through
    /// untouched — never invent a key.
    #[test]
    fn unresolvable_and_canonical_inputs_pass_through_untouched() {
        assert_eq!(apply_key_conversion("8A", "camelot"), "8A");
        assert_eq!(apply_key_conversion("Am", "standard"), "Am");
        assert_eq!(apply_key_conversion("13d", "camelot"), "13d");
        assert_eq!(apply_key_conversion("0d", "camelot"), "0d");
        assert_eq!(apply_key_conversion("s-key", "camelot"), "s-key");
        assert_eq!(apply_key_conversion("", "camelot"), "");
    }

    /// Behaviour that existed before this slice must not change.
    #[test]
    fn existing_conversion_behaviour_is_preserved() {
        assert_eq!(apply_key_conversion("Am", "camelot"), "8A");
        assert_eq!(apply_key_conversion("8A", "standard"), "Am");
        // Unknown format strings pass through unchanged (a warning is logged internally).
        assert_eq!(apply_key_conversion("11d", "bogus"), "11d");
        assert_eq!(apply_key_conversion("8A", "bogus"), "8A");
    }

    /// The Camelot/OpenKey storage target must be idempotent: applying it twice returns
    /// the same value, so re-running the bulk converter is always safe.
    #[test]
    fn camelot_target_is_idempotent() {
        for key in ["11d", "1m", "11D", "8A", "Am", "13d", "s-key", ""] {
            let once = apply_key_conversion(key, "camelot");
            let twice = apply_key_conversion(&once, "camelot");
            assert_eq!(
                once, twice,
                "re-applying the Camelot target changed {key:?}"
            );
        }
    }

    /// The bulk converter must actually migrate legacy rows: before this slice it was a
    /// silent no-op on them (`WHERE key != ?1` wrote nothing).
    #[test]
    fn recalculate_all_keys_migrates_legacy_harmony_rows() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        conn.lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE tracks (id TEXT PRIMARY KEY, bpm REAL, key TEXT);
                 INSERT INTO tracks (id, bpm, key) VALUES ('legacy', NULL, '11d');
                 INSERT INTO tracks (id, bpm, key) VALUES ('camelot', NULL, '8A');
                 INSERT INTO tracks (id, bpm, key) VALUES ('null-key', NULL, NULL);",
            )
            .unwrap();
        let service = AnalysisService::new(conn.clone(), FileTagsService::new());

        let updated = service.recalculate_all_keys("camelot".to_string()).unwrap();

        assert_eq!(updated, 1, "only the legacy row should change");
        {
            let conn = conn.lock().unwrap();
            let stored: String = conn
                .query_row("SELECT key FROM tracks WHERE id = 'legacy'", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(stored, "11B");
        }
        // Idempotence at the converter level: a second run finds nothing to change.
        assert_eq!(
            service.recalculate_all_keys("camelot".to_string()).unwrap(),
            0
        );
    }

    // --- Standard target: resolve-then-render + canonicalization contract ---

    /// The Standard arm resolves the stored value to Camelot first, then renders the
    /// note name: legacy harmony values migrate, canonical spellings round-trip, and
    /// unrecognizable values pass through untouched.
    #[test]
    fn standard_target_resolves_legacy_and_preserves_existing_behaviour() {
        // Newly fixed: legacy harmony resolves to note names (11d = 11B, 1m = 1A).
        // 1A renders as the Rust map's canonical spelling, G#m (sharp family).
        assert_eq!(apply_key_conversion("11d", "standard"), "A");
        assert_eq!(apply_key_conversion("1m", "standard"), "G#m");
        // Preserved behaviour.
        assert_eq!(apply_key_conversion("Am", "standard"), "Am");
        assert_eq!(apply_key_conversion("8A", "standard"), "Am");
        assert_eq!(apply_key_conversion("s-key", "standard"), "s-key");
        assert_eq!(apply_key_conversion("G Minor", "standard"), "G Minor");
    }

    /// The Standard target renders only the canonical spellings held by
    /// `CAMELOT_TO_STANDARD` — the Rust map's sharp family (`Ebm` for the single 2A
    /// collision). This is a render contract, not a full normalization promise:
    /// enharmonic spellings the forward map does not hold pass through unchanged
    /// (see [`standard_target_leaves_absent_enharmonic_spellings_unchanged`]).
    #[test]
    fn standard_target_renders_reverse_map_canonical_spellings() {
        let cases: &[(&str, &str)] = &[
            // Deliberate canonicalization: the 2A collision resolves to Ebm.
            ("D#m", "Ebm"),
            // The Rust reverse map holds the sharp family: G#m for 1A, A#m for 3A.
            ("G#m", "G#m"),
            ("A#m", "A#m"),
            // Not in the forward map — render unchanged (known limitation).
            ("Gbm", "Gbm"),
            ("Db", "Db"),
            ("G#", "G#"),
        ];
        let mismatches: Vec<String> = cases
            .iter()
            .filter_map(|(input, expected)| {
                let actual = apply_key_conversion(input, "standard");
                (actual != *expected).then(|| format!("{input}: expected {expected}, got {actual}"))
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "standard-target render mismatches:\n{}",
            mismatches.join("\n")
        );
    }

    /// Known limitation, deliberate policy: enharmonic standard spellings the Rust
    /// forward map does not hold pass through unchanged under the Standard target.
    /// `STANDARD_TO_CAMELOT` documents that enharmonic variants (e.g. `Gb`, `Db`) are
    /// intentionally absent — the DSP emits a specific spelling that is converted
    /// verbatim, and widening the map would change what the `camelot` target writes
    /// for these inputs as well. The frontend's `format.ts` map is enharmonic-aware;
    /// aligning the two layers is a separate decision, not this slice's.
    #[test]
    fn standard_target_leaves_absent_enharmonic_spellings_unchanged() {
        assert_eq!(apply_key_conversion("Gbm", "standard"), "Gbm");
        assert_eq!(apply_key_conversion("Db", "standard"), "Db");
        assert_eq!(apply_key_conversion("G#", "standard"), "G#");
    }

    /// The Standard target must be idempotent too: applying it twice is stable, so
    /// re-running the bulk converter for a standard-notation user is always safe.
    #[test]
    fn standard_target_is_idempotent() {
        for key in ["11d", "1m", "8A", "Am", "D#m", "s-key", ""] {
            let once = apply_key_conversion(key, "standard");
            let twice = apply_key_conversion(&once, "standard");
            assert_eq!(
                once, twice,
                "re-applying the Standard target changed {key:?}"
            );
        }
    }

    /// A user whose notation setting is standard must get a real migration, not the
    /// previous silent no-op on legacy rows. Under the standard target both the legacy
    /// row and the Camelot row migrate to note names.
    #[test]
    fn recalculate_all_keys_migrates_legacy_rows_under_standard_target() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        conn.lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE tracks (id TEXT PRIMARY KEY, bpm REAL, key TEXT);
                 INSERT INTO tracks (id, bpm, key) VALUES ('legacy', NULL, '11d');
                 INSERT INTO tracks (id, bpm, key) VALUES ('camelot', NULL, '8A');
                 INSERT INTO tracks (id, bpm, key) VALUES ('null-key', NULL, NULL);",
            )
            .unwrap();
        let service = AnalysisService::new(conn.clone(), FileTagsService::new());

        let updated = service
            .recalculate_all_keys("standard".to_string())
            .unwrap();

        // Legacy 11d → "A" (11B) and Camelot 8A → "Am": both rows change.
        assert_eq!(
            updated, 2,
            "both the legacy and the Camelot row should migrate"
        );
        {
            let conn = conn.lock().unwrap();
            let stored = |id: &str| -> String {
                conn.query_row("SELECT key FROM tracks WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .unwrap()
            };
            assert_eq!(stored("legacy"), "A");
            assert_eq!(stored("camelot"), "Am");
        }
        // Idempotence at the converter level: a second run finds nothing to change.
        assert_eq!(
            service
                .recalculate_all_keys("standard".to_string())
                .unwrap(),
            0
        );
    }

    /// The resolver maps every storage shape to Camelot, or `None` when it cannot —
    /// never inventing a key.
    #[test]
    fn stored_key_resolver_maps_storage_shapes_to_camelot() {
        // Camelot codes: returned as themselves, letter normalized to uppercase.
        assert_eq!(resolve_stored_key_to_camelot("8A").as_deref(), Some("8A"));
        assert_eq!(resolve_stored_key_to_camelot("12B").as_deref(), Some("12B"));
        assert_eq!(resolve_stored_key_to_camelot("8a").as_deref(), Some("8A"));
        // Standard spellings via the existing map.
        assert_eq!(resolve_stored_key_to_camelot("Am").as_deref(), Some("8A"));
        assert_eq!(resolve_stored_key_to_camelot("F#m").as_deref(), Some("11A"));
        // Legacy harmony m/d: number kept, letter mapped, case-insensitive.
        assert_eq!(resolve_stored_key_to_camelot("11d").as_deref(), Some("11B"));
        assert_eq!(resolve_stored_key_to_camelot("1m").as_deref(), Some("1A"));
        assert_eq!(resolve_stored_key_to_camelot("11D").as_deref(), Some("11B"));
        assert_eq!(resolve_stored_key_to_camelot("12m").as_deref(), Some("12A"));
        assert_eq!(resolve_stored_key_to_camelot("1M").as_deref(), Some("1A"));
        // Anything else: None.
        assert!(resolve_stored_key_to_camelot("13d").is_none());
        assert!(resolve_stored_key_to_camelot("0d").is_none());
        assert!(resolve_stored_key_to_camelot("s-key").is_none());
        assert!(resolve_stored_key_to_camelot("").is_none());
    }

    /// KeyNotationFormat: Display/FromStr round-trip for every variant, and the default
    /// is OpenKey (display default; storage stays Camelot).
    #[test]
    fn key_notation_format_round_trips_and_defaults_to_openkey() {
        for format in [
            KeyNotationFormat::Standard,
            KeyNotationFormat::Camelot,
            KeyNotationFormat::OpenKey,
        ] {
            let rendered = format.to_string();
            assert_eq!(rendered.parse::<KeyNotationFormat>().unwrap(), format);
        }
        assert_eq!(
            "openkey".parse::<KeyNotationFormat>().unwrap(),
            KeyNotationFormat::OpenKey
        );
        assert_eq!(KeyNotationFormat::default(), KeyNotationFormat::OpenKey);
    }
}
