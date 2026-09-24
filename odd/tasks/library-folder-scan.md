# Feature: library-folder-scan

Goal: let the user point Crate at their music folder from Settings > Library. The app persists the
folder as a device-local library root, recursively walks the tree, and imports every supported audio
file it finds. Files already in the library are skipped silently; the scan reports how many were new,
how many already existed, and how many failed.

Context: today imports are file-driven only — the toolbar/menu/drag-drop flows call
`import_tracks_with_duplicates` with an explicit list of files picked in a dialog
(`apps/desktop/src/lib/controllers/trackController.ts:154`). There is no setting for a music folder
and no recursive walk anywhere in the codebase. The user decided (session 2026-09-24) to:

1. **Reuse `library_roots`** instead of adding a field to `AppSettings`. This is the correct call and
   the scouts confirmed why: absolute paths belong in `sync_root_mappings`, which is **device-local**
   by design (`src-tauri/src/db/schema.rs:283-290`: "these tables are device-local — they are never
   themselves serialized as sync buckets"), while only the root *name* syncs. Putting an absolute
   machine path in `AppSettings` would have required excluding it from the settings sync whitelist
   (`src-tauri/src/services/cloud_sync/mod.rs:46`).
2. **One folder**, not a list.
3. **Import automatically** after the folder is chosen.
4. **Skip duplicates silently** — no per-file duplicate modal. Rescanning a 2000-file already-imported
   folder must not open a 2000-step modal.

## Design (fixed before implementation)

- **Designated root, stable id.** A single well-known root id, `local-music-library`, created through
  a new `register_root_with_id(conn, id, name)`. `register_root` generates a random UUID
  (`resolution.rs:105`), so it cannot express "the same logical root on every device"; the new function
  is an additive refactor and `register_root` delegates to it, keeping existing callers byte-identical.
  The stable id matters: `sync_root_mappings` is keyed by `library_root_id`, so both devices map their
  own folder to the **same** logical root and synced tracks (`library_root_id` + `relative_path`)
  resolve on each device. A per-device UUID would instead produce unrelated, permanently unmapped
  roots. The root row is a normal synced `library_roots` row; only its local path stays device-local.
- **Path goes local, name goes synced.** No new setting key, no sync-whitelist change.
- **Canonicalize on write, once.** `set_root_mapping` canonicalizes before storing. The walk uses the
  stored (canonical) value as its root, so `Path::strip_prefix` in `assign_root_for_import` matches.
- **Do not follow symlinks** during the walk: avoids cycles and keeps every produced path under the
  canonical root prefix.
- **Deterministic root assignment.** `try_assign_root_for_import` currently selects with **no
  `ORDER BY`** and returns the first matching row, so nested roots resolve nondeterministically
  (`src-tauri/src/services/cloud_sync/resolution.rs:88-98`). Add
  `ORDER BY length(local_absolute_path) DESC` so the longest (most specific) prefix always wins. This
  can only change behavior where it was previously arbitrary.
- **Silent duplicate skip.** The scan computes the content hash, checks `find_track_by_hash`, and on a
  hit increments `skipped_existing_count` without building a `DuplicateTrack` payload. On a miss it
  imports through the existing `import_single_track_with_hash`.
- **Counts + ids, not full track payloads.** The command returns counts, error strings, and the
  imported track **ids** (so the frontend can still honor `auto_analyze_on_import`), then the
  frontend refetches the library. Returning thousands of `Track` objects over IPC is not acceptable
  for a 5000-file folder; the repo already has this "full refetch" pattern
  (`OrchestratorLayer.svelte` after purchase import).

## Tasks

- [x] T1 Recon: scout the `library_roots` subsystem, the import pipeline, and the Settings > Library UI
- [ ] T2 Backend: single source of truth for supported audio extensions, used by all backend call sites
- [ ] T3 Backend: `register_root_with_id` + canonicalize in `set_root_mapping` + deterministic longest-prefix root matching, with tests
- [ ] T4 Backend: `services/library/scan.rs` — resolve root, walk, hash-skip, import, count
- [ ] T5 Backend: `get_music_library_folder` / `set_music_library_folder` / `scan_music_library_folder` commands, registered in `generate_handler!` under `#[cfg(feature = "desktop")]`
- [ ] T6 Shared: API wrappers in `shared/api/library.ts` + `LibraryFolderScanResult` type
- [ ] T7 Desktop: `libraryStore` scan action (refresh tracks + honor auto-analyze)
- [ ] T8 Desktop: Settings > Library section — path display, folder picker, rescan, result summary
- [ ] T9 i18n: new `settings.library.*` keys across all 15 locale files
- [ ] T10 Verify: `cargo fmt --check`, `cargo clippy --features desktop -- -D warnings`, `cargo check --target aarch64-apple-ios --no-default-features --features mobile`, `cargo test --features desktop`, `yarn check:svelte`, `yarn check:svelte:mobile`, `yarn format:check`, `yarn lint:check`

## Constraints

- **Never hold the DB mutex guard across an `.await`** (AGENTS.md §5.5). The scan is synchronous
  work; keep it out of async suspension points or scope the guard tightly.
- **Register every command** in `src-tauri/src/lib.rs` `generate_handler!` (AGENTS.md §5.2). Every new
  command here is desktop-only and needs `#[cfg(feature = "desktop")]`; `services::library` is already
  gated at `src-tauri/src/services/mod.rs:21`, so the backend work is inherently desktop-scoped.
- **No `#[cfg(feature = "mobile")]`** anywhere — mobile paths are `#[cfg(not(feature = "desktop"))]`.
- **Serde casing is per struct**: new result/domain types follow `ImportResult`
  (`snake_case`, `src-tauri/src/models/track.rs:130`). Command *arguments* are camelCase in TS,
  snake_case in Rust.
- **Do not import `@tauri-apps/plugin-dialog` from `shared/`**: `apps/mobile/tsconfig.json` type-checks
  all of `shared/**`, and `apps/mobile` declares no `@tauri-apps/*` dependency. The picker stays in the
  desktop component.
- **No new duplicate copy of the extension list** in the backend (already copy-pasted at
  `import.rs:51`, `import.rs:231`, `relocation.rs:43`).
- **Upstream-portable and fork-only content never share a commit.** `odd/` is fork-only and must never
  travel upstream (AGENTS.md §1.2, `FORK.md`). Every feature commit must be individually
  cherry-pickable to `upstream/develop` with no `odd/`, `.agents/`, `.pi/` or `openspec/` content.
- Do not touch `README.md`, `CHANGELOG.md`, or `docs/` — all upstream-owned.
- Artifacts in English; conversation in Rioplatense Spanish.

## Out of scope (deliberate, recorded so it is not lost)

- `clear_music_library_folder` / unsetting the folder.
- Removing or renaming the root from Settings.
- Watching the folder for changes (no filesystem watcher; scan is manual/explicit).
- Progress reporting during the scan, and any cancellation. A large first scan will look busy with no
  granular feedback — v1 shows a loading state and a final summary.
- Backfilling `library_root_id`/`relative_path` for tracks imported before the folder was set. Root
  assignment happens only at import/relocate time (`import.rs:316`, `duplicates.rs:50`,
  `relocation.rs:94`); existing tracks stay absolute until they are re-imported.
- Deduplicating the extension list in the **frontend** (`trackController.ts:157`,
  `playlistController.ts:255`) and in `relocation.rs` if it turns out to be a different concern.
- The `README.md` "11 languages" vs 15 locale files upstream finding.

## Evidence log

- **Recon (scouts, read-only, 2026-09-24).** `library_roots (id, name, _hlc)` at `schema.rs:249`; the
  path lives in `sync_root_mappings (library_root_id PRIMARY KEY, local_absolute_path)` at
  `schema.rs:286`. Serializer/merger touch only `id`+`name` (`cloud_sync/pipeline/rows.rs:848`,
  `merge/writers.rs:229`); no `sync_root_mappings` bucket exists in `Bucket::all()`
  (`pipeline/buckets.rs:196`). `assign_root_for_import` is best-effort and returns `(None, None)` on
  any DB error (`resolution.rs:70-82`); the `SELECT` inside has no `ORDER BY` (`resolution.rs:88`) and
  no canonicalization/symlink handling — pure `Path::strip_prefix` on raw strings.
- **No recursive audio walk exists.** The only recursive walker in the backend is
  `diagnostics.rs:125` (`WalkDir` for data-dir size, no extension filter, desktop-gated).
  `follow/watch.rs` watches *web pages*, not the filesystem.
- **`walkdir` is an optional dependency under the `desktop` feature** (`src-tauri/Cargo.toml`), which
  is sufficient because `services::library` is itself desktop-only (`services/mod.rs:21`).
- **Duplicate detection is content-based**: BLAKE3 over the first 64 KB after skipping container
  headers (`services/hash.rs:16`, `HASH_CHUNK_SIZE`), looked up via
  `find_track_by_hash` (`import.rs:240`). `file_hash` is nullable with no unique index.
- **Re-importing identical paths is idempotent**: `tracks.file_path` is `TEXT NOT NULL UNIQUE`
  (`schema.rs:8`) and `insert_track` uses `ON CONFLICT(file_path) DO UPDATE` (`import.rs:339`).
- **Settings sync is whitelist-based**: `SYNCED_SETTING_KEYS` at `cloud_sync/mod.rs:46`; `set_setting`
  only stamps dirty for whitelisted keys (`services/settings.rs:213`).
- **UI prior art**: `LibraryRootsWizard.svelte:9` uses `open({ directory: true, multiple: false })`;
  `cloudSyncStore.setRootMapping` writes then refetches (`shared/stores/cloudSync.ts:207`);
  `LibraryTab.svelte` currently does not import `Button`; `SettingsModal.svelte:112` renders the tab.
- **Mobile check scope**: `apps/mobile/tsconfig.json` includes `../../shared/**/*.{ts,svelte}` but not
  `apps/desktop/**`, and `apps/mobile/package.json` declares no `@tauri-apps/*` dependency.
- **Signature verification (read-only, before the first source write).** `resolution.rs` has no service
  struct: every function is a free function taking `&Connection` (`register_root:104`,
  `rename_root:116`, `remove_root:127`, `set_root_mapping:137`; `try_assign_root_for_import:83` is
  private). `register_root` generates `uuid::Uuid::new_v4()` and marks `buckets::LIBRARY_ROOTS` dirty.
  In `library/`: `find_track_by_hash` is `pub` and lives in `query.rs:275` (not `import.rs`);
  `import_single_track_with_hash` is `pub(crate)` at `import.rs:121`; `import_single_track`,
  `process_import_path`, `insert_track` are private to the `impl`; `read_metadata_lenient` is
  `pub(crate)` at `import.rs:255`. `compute_audio_hash(path: &Path) -> Result<String>`
  (`hash.rs:16`, `HASH_CHUNK_SIZE` private).
- **Foreign keys are enforced**: `PRAGMA foreign_keys = ON` at `db/mod.rs:78`, and
  `tracks.library_root_id` is `REFERENCES library_roots(id) ON DELETE SET NULL` (`schema.rs:267`) —
  the root row must exist before any track referencing it is inserted. The test helper pattern does the
  same (`db/mod.rs:152`).
- **Test convention confirmed**: in-memory `Connection::open_in_memory()` + explicit
  `PRAGMA foreign_keys = ON` + `get_migrations()` applied by hand (`db/mod.rs:150`,
  `services/cloud_sync/tests/mod.rs:26`). `services/library/*` has no tests today, so T3's tests
  establish the precedent there.
- **`RescanResult` precedent**: it lives in `services/library/mod.rs:35`, not in `models/`, and carries
  no `#[cfg(feature = "desktop")]` (the module is already desktop-gated). The new scan result type
  follows it.
- **Error variant**: there is no dedicated "not configured" variant; `CrateError::InvalidOperation(String)`
  is the fit for both "no folder configured" and "path is not a directory".
- **i18n capabilities**: `translate` is a re-export of svelte-i18n `_` (`shared/i18n/index.ts:108`), so
  ICU placeholders work — `{ values: { ... } }`, including ICU plurals (`en.json:78`);
  `fallbackLocale: 'en'` (`index.ts:56`) means an untranslated key degrades to English, not to a raw key.

## Commit evidence (on the feature branch, not pushed)

| Commit | Message | Files |
| --- | --- | --- |
| _pending_ | `docs(odd): track library-folder-scan feature` | `odd/tasks/library-folder-scan.md` |
