//! Cue point commands — thin IPC wrappers over `CueService`.
//!
//! Cross-platform (the `cues` table exists on mobile and `Cue` carries no feature
//! gates), so none of these are `#[cfg(feature = "desktop")]`.

use tauri::State;

use crate::error::Result;
use crate::models::Cue;
use crate::services::CueService;

/// All cues of a track, ordered deterministically.
#[tauri::command]
pub async fn get_cues_for_track(track_id: String, cues: State<'_, CueService>) -> Result<Vec<Cue>> {
    cues.get_cues_for_track(&track_id)
}

/// Replace the whole cue list of a track with `cues`, returning what was stored.
#[tauri::command]
pub async fn replace_cues_for_track(
    track_id: String,
    cues: Vec<Cue>,
    service: State<'_, CueService>,
) -> Result<Vec<Cue>> {
    service.replace_cues_for_track(&track_id, cues)
}

/// Delete every cue of a track.
#[tauri::command]
pub async fn delete_cues_for_track(track_id: String, service: State<'_, CueService>) -> Result<()> {
    service.delete_cues_for_track(&track_id)
}

/// Delete a single cue by id.
#[tauri::command]
pub async fn delete_cue(id: String, service: State<'_, CueService>) -> Result<()> {
    service.delete_cue(&id)
}
