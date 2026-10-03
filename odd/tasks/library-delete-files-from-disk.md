# Library removal modal: "delete files from disk" checkbox

## Objective

Add a checkbox to the "Remove from library" confirmation modal (`ModalOrchestrator` →
`removeFromLibrary`). When checked, removing tracks from the library also deletes their audio
files from disk. Unchecked (default) keeps today's behavior: library entry only.

## Problem / Why

Users managing a crate library currently must remove tracks from the library and then hunt the
files down manually in the OS file manager. A destructive opt-in checkbox inside the existing
confirmation modal is the smallest safe UX for "delete the files too".

## Scope

- In scope: the `removeFromLibrary` modal flow only (track list context menu / bulk delete).
- Out of scope (follow-ups): `removeFromPlaylist` "delete from collection", `removeDiscoveryReleases`,
  deleting cover/metadata sidecar files, moving files to trash instead of hard delete.

## Constraints

- Repo AGENTS.md §1.3: **no commits unless the user explicitly asks** — work stays in the worktree.
- `ConfirmModal.svelte` already supports `checkboxLabel` / `checkboxChecked` ($bindable) and calls
  `onConfirm(checkboxChecked)` — reuse it, do not invent a new modal.
- IPC arg naming: TS camelCase (`deleteFiles`) → Rust snake_case (`delete_files`), like
  `deleteTracksFromCollection` in `delete_playlist`.
- DB rows delete first, then best-effort disk delete (a failed file delete must never leave a
  deleted-file/orphaned-row inconsistency worse than today; log warnings, don't fail the command).
- `delete_playlist` (src-tauri/src/commands/playlist.rs) also calls `LibraryService::delete_tracks`
  → pass `false` there; its "collection" checkbox must not touch disk.
- shared/ compiles for mobile too — keep the API wrapper generic, no desktop-only imports.
- i18n: new key in `en.json` minimum; update all 15 locale files (simple translatable label).
- Prettier: tabs, no semicolons, single quotes, 120 cols. Artifacts/comments in English.

## Design decisions

- Service signature: `delete_tracks(&self, ids: Vec<String>, delete_files: bool) -> Result<()>`.
  - Before the DB DELETE, `SELECT file_path FROM tracks WHERE id IN (...)` and dedupe paths.
  - Existing artwork deletion + tombstones + dirty-marking unchanged.
  - After successful DB delete, `std::fs::remove_file` each path: `NotFound` → skip silently,
    other errors → `log::warn!` and continue. Return `Ok(())` regardless (best-effort).
- Command: `delete_tracks(ids, delete_files, library)` — Tauri default arg naming handles camelCase.
- Frontend flag threading (default `false` everywhere for backward compatibility):
  `ConfirmModal checkbox → handleRemoveFromLibraryConfirm(deleteFilesToo) → onRemoveFromLibrary(trackIds, deleteFilesToo)
   → wiring handler → libraryStore.removeTracks(ids, deleteFiles) → shared/api deleteTracks(ids, deleteFiles)
   → invoke('delete_tracks', { ids, deleteFiles })`.
- Checkbox state resets on modal open/close (same pattern as `deleteTracksFromCollection`).

## Tasks

Route: delegated (writer trigger: 5+ non-trivial files; mapping trigger fired — 4+ files to understand).

- [x] T1 — Rust service: `delete_tracks(ids, delete_files)`; paths selected under the guard
      (root_mapping resolution + dedupe), guard dropped before fs; DB delete first, then
      best-effort `fs::remove_file` (NotFound silent, other errors warned). `delete_playlist`
      passes `false`. 4 unit tests added. Evidence: `cargo test --features desktop` → 481
      passed / 0 failed; filter run confirmed the 4 new tests execute and pass.
- [x] T2 — Command `delete_tracks(ids, delete_files, library)` forwards the flag; registration
      unchanged (already `#[cfg(feature = "desktop")]` in generate_handler!).
- [x] T3 — `shared/api/library.ts`: `deleteTracks(ids, deleteFiles = false)` →
      `invoke('delete_tracks', { ids, deleteFiles })`.
- [x] T4 — `libraryStore.deleteTracks(ids, deleteFiles = false)` passthrough; OrchestratorLayer
      `onRemoveFromLibrary` threads the flag (:466); the playlist-remove path (:440) keeps the
      false default on purpose.
- [x] T5 — ModalOrchestrator: `deleteFilesFromDisk` state, reset on open + `closeAll`, handler
      `handleRemoveFromLibraryConfirm(deleteFilesToo)`, ConfirmModal gains `checkboxLabel` +
      `bind:checkboxChecked`; Props type updated.
- [x] T6 — i18n key `modals.confirm.deleteFilesFromDisk` present in 15/15 locale files
      (grep-verified by independent validator).
- [x] T7 — Verification (writer + independent verifier agree):
      `cargo test --features desktop` 481 passed; `cargo clippy --features desktop -- -D warnings`
      clean; `cargo fmt --check` clean; `yarn check:svelte` 0 errors (2 pre-existing a11y warnings
      in untouched LibraryTab.svelte); `yarn check:svelte:mobile` 0/0; `yarn lint:check` exit 0;
      `yarn format:check` clean. `cargo check --target aarch64-apple-ios` NOT runnable on this
      Linux host (vendored-openssl needs xcrun) — mobile safety argued via cfg-gates.

## Checks (acceptance)

- [x] Removing without the checkbox: identical behavior to today (files stay on disk).
      Evidence: `keeps_the_audio_file` test + false defaults verified at every layer.
- [x] Removing with the checkbox: DB row gone AND file gone from disk.
      Evidence: flag-on removal test; structural readback of the command/service chain.
- [x] Same file imported twice: N/A — `tracks.file_path` is `TEXT NOT NULL UNIQUE`
      (db/schema.rs), so one file can never back two rows; dedupe kept as defensive.
- [x] A file already missing on disk: no error surfaced, tracks still removed.
      Evidence: NotFound-tolerance test.
- [x] `delete_playlist` with "delete from collection": tracks leave the library, files
      untouched. Evidence: playlist.rs passes `false` (validator-verified, desktop-only call site).

## Progress log

- (init) Feature doc created; route = delegated direct writer. TDD mode: not configured for the
  project (no strict-TDD cache found in memory); ordinary functional checks apply.
- Commits: initially held back per AGENTS.md §1.3; user then authorized — feature work-unit
  commit `7ec08a8` (feat(library): add opt-in delete-files-from-disk to library removal) on `dev`,
  22 files +222/−40. This tracking doc follows as a separate `docs(odd)` commit (AGENTS.md §1.2:
  scaffolding never rides on a feature commit).
- CLOSED: writer implemented T1–T6; independent targeted validator verdict **PASS**, zero
  blockers, all gates green (writer + verifier ran them separately). 22 files, +222/−40.
  Deferred follow-ups (not blockers):
  1. nit — update.rs comment says it "mirrors check_track_file_exists" but reimplements
     `resolution::resolve_track_path` mapping via `root_mapping`; semantics equivalent, shared
     helper would prevent drift.
  2. nit — no test for the cloud-synced branch (mapped root → join; unmapped root → file kept);
     verified by code reading only.
  3. runtime smoke test in `yarn dev` (headless env prevented a live IPC check).
  4. AGENTS.md drift found: `src-tauri/src/db/key/mod.rs` doesn't exist (it's `db/key_provider.rs`)
     and there is no `compile_error!` in src-tauri — docs need a fix sometime.
- Review posture: receipt-driven development is OFF (clone-local user decision) — no review
  transaction started; verification follows ordinary checks only.
- NOTE for upstream: this odd/ document must stay out of any pure feature branch (AGENTS.md §1.2).
