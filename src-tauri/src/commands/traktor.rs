//! Traktor NML import commands — thin IPC wrappers over `TraktorImportService`.
//!
//! Desktop-only: the service reuses the desktop library import machinery
//! (`LibraryService`, which reads tags/hashes a file) plus `compute_audio_hash`.
//! The module is gated in `commands/mod.rs`; both commands are registered under
//! `#[cfg(feature = "desktop")]` in `lib.rs`.

use std::path::Path;

use tauri::State;

use crate::error::Result;
use crate::services::traktor::mapper::ImportPlan;
use crate::services::traktor_import::{TraktorImportOutcome, TraktorImportService};

/// Parse + map an NML file into a dry-run import plan. No DB access, no writes.
#[tauri::command]
pub async fn preview_traktor_import(
    nml_path: String,
    service: State<'_, TraktorImportService>,
) -> Result<ImportPlan> {
    service.preview(Path::new(&nml_path))
}

/// Import an NML collection into the library, returning what actually happened.
#[tauri::command]
pub async fn import_traktor_collection(
    nml_path: String,
    service: State<'_, TraktorImportService>,
) -> Result<TraktorImportOutcome> {
    service.import(Path::new(&nml_path))
}
