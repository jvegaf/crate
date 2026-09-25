# Feature: library-scan-performance

Goal: the folder scan introduced by `library-folder-scan` is unusably slow on a real library
(the user measured a 200+-track first import that felt like a freeze) and gives the user no
feedback while it runs. Fix both: make the scan fast, keep the app responsive, and show live
progress in a global, non-blocking way.

Context: this is the direct follow-up to `library-folder-scan`, which listed "No progress feedback
or cancellation during a long first scan" as a deliberate out-of-scope follow-up. The user tested
the shipped feature and reported the slowness, so the follow-up is now the work.

Branch/session: `library-folder-scan` is the current branch and carries the unreviewed prior
feature. This work must stay separable from that candidate (review workload guard, AGENTS.md §6).

## Diagnosis (read-only reconnaissance, 2026-09-24)

Four compounding causes, ordered by expected impact. All are static-code findings, not a profile.

1. **Per-track autocommit.** `insert_track` (`src-tauri/src/services/library/import.rs:339`) runs in
   autocommit, so every track costs ~5 separate implicit transactions: `next_hlc`
   (`services/cloud_sync/hlc.rs:103` = two `write_state` calls), `assign_root_for_import`
   (`services/cloud_sync/resolution.rs:70`), the `INSERT`, and `mark_dirty`
   (`services/cloud_sync/pipeline/dirty.rs:51`). The database is SQLCipher
   (`src-tauri/Cargo.toml:61`, `bundled-sqlcipher-vendored-openssl`) and `db/mod.rs` sets **no**
   `journal_mode` or `synchronous` pragma anywhere, so the defaults hold: `journal_mode=DELETE`,
   `synchronous=FULL`. Each commit is a journal write, page encryption, and up to two fsyncs.
   200 tracks ≈ 1000 commits ≈ 2000 fsyncs. This is the dominant cost.
2. **No index on `file_hash`.** `find_track_by_hash` (`src-tauri/src/services/library/query.rs:275`)
   filters `WHERE file_hash = ?1`, but no index covers `file_hash`: the index block in
   `db/schema.rs:52-56` covers artist/bpm/key/date_added/color and the cloud-sync migration adds only
   `_hlc` indexes. Every file therefore full-scans the `tracks` table and builds a whole `Track` row
   through `query_row` even when nothing matches. The first import is quadratic (~n²/2) and every
   later rescan still scans the whole table once per file — this is why a non-first import is also
   slow, which the user noticed independently.
3. **The scan blocks the async runtime.** `scan_music_library_folder`
   (`src-tauri/src/commands/library.rs:143`) is an `async` command that runs the entire synchronous
   scan on a tokio worker thread; there is no `spawn_blocking` (the precedent exists in
   `services/backup.rs` and `services/export/orchestration.rs`). Combined with the global
   `libraryStore.loading` flag the frontend sets for the whole scan, the app looks — and partly is —
   frozen.
4. **No progress feedback at all.** No `emit` exists for the scan. The only user-visible signal is the
   Settings button label switching to "Scanning…" (`apps/desktop/src/lib/components/settings/tabs/LibraryTab.svelte:186`).
   Existing precedents: `bulk-import-progress` (`commands/discovery.rs:873`), `analysis-track-event`
   (`services/analysis.rs:110`), `export-progress`, `backup-progress`.

### Deliberately out of scope (found during reconnaissance, do not fix here)

- `std::fs::canonicalize` per file inside `try_assign_root_for_import` — one syscall per track on an
  existing path; negligible next to the commit cost, and fixing it widens the blast radius.
- R3-001 from the prior review: the walk is unbounded (no file cap, no cancellation) and
  `failed_count` conflates walk errors with per-file import errors. Real, still deferred.
- Any `journal_mode=WAL` switch. It would also help, but it changes the durability/backup story for the
  whole database (device sync, `sqlcipher_export`, backups) and needs its own diagnosis.

## Design (fixed before implementation)

Two slices. One writer at a time. Each slice is independently verifiable.

### Slice 1 — backend (speed + non-blocking)

- **Migration (append-only).** Append a new entry at the end of the vec in
  `src-tauri/src/db/schema.rs::get_migrations()`:
  `CREATE INDEX IF NOT EXISTS idx_tracks_file_hash ON tracks(file_hash);`.
  Migrations are versioned by index and never edited in place (AGENTS.md §5.5), so appending is the
  only correct move. `CREATE INDEX IF NOT EXISTS` keeps it idempotent.
- **Split metadata reading from insertion** in `services/library/import.rs`:
  - new `pub(crate) fn build_track_from_file(&self, path: &PathBuf, file_hash: String) -> Result<Track>`
    — everything `import_single_track_with_hash` does today *except* the DB insert: format resolution,
    `read_metadata_lenient`, the symphonia fallback, tag extraction, artwork extraction, hash stamping.
  - `import_single_track_with_hash` becomes `build_track_from_file` + `insert_track`, so its
    behaviour and signature are unchanged for existing callers.
  - `insert_track` is split so the DB write can join a caller-owned transaction: a
    `pub(crate) fn insert_track_in(conn: &Connection, track: &Track) -> Result<()>` holding the
    existing SQL verbatim, with the current `fn insert_track(&self, track)` reduced to
    "lock, then delegate". `dirty::next_hlc` and `resolution::assign_root_for_import` take
    `&Connection` and `&Transaction` derefs to it, so both calls compile unchanged.
- **Batch the scan** in `services/library/scan.rs`, keeping the existing per-file semantics:
  - `const SCAN_BATCH_SIZE: usize = 32;`
  - `scan_music_library_folder(&self)` keeps its signature and delegates to a new
    `scan_music_library_folder_with_progress<F: FnMut(&LibraryScanProgress)>(&self, on_progress: F)`
    with a no-op closure, so the existing entry point and its tests are untouched.
  - Loop over `files.chunks(SCAN_BATCH_SIZE)`. Per chunk: compute each hash **before** taking the
    lock, then take the lock once, open one `unchecked_transaction()`, and for each file do the
    existing hash lookup → existing skip → `build_track_from_file` → `insert_track_in`, then commit
    once and release the lock.
  - **Duplicate semantics must not regress.** Today the first file is committed before the second is
    checked, so two byte-identical files anywhere in the folder yield one track + one skip. No extra
    seen-hash bookkeeping is needed to keep that: a transaction makes its own uncommitted inserts
    visible to `find_track_by_hash_in`, so an intra-chunk duplicate is caught by the lookup itself, and a
    pair straddling a chunk boundary is caught by the previous commit. See "Parent correction" in the
    evidence log for why a `HashSet` was designed in and then removed.
  - Emit `on_progress` after each file is processed, with `current` = files processed so far,
    `total` = `scanned_count`, the running counts, and `current_file` = the file name.
  - **Never hold the guard across an `.await`** (AGENTS.md §5.5); the scan stays fully synchronous.
- **New result type** `LibraryScanProgress` next to `LibraryFolderScanResult` in
  `services/library/mod.rs`, `serde::Serialize + Clone`, snake_case fields like its neighbours. Keep
  `LibraryFolderScanResult` byte-identical so the shared TS type does not churn.
- **Command**: `scan_music_library_folder(app: tauri::AppHandle, library: State<'_, LibraryService>)`
  clones the service and runs the scan under `tauri::async_runtime::spawn_blocking`, emitting
  `library-scan-progress` inside the progress closure. Requires `#[derive(Clone)]` on
  `LibraryService` and on `ArtworkService` (its only field is a `PathBuf`).

### Slice 2 — frontend (persistent progress toast)

The user's decision (session 2026-09-24): the toast system already exists and is mounted globally
(`apps/desktop/src/routes/+layout.svelte:444`), but it is fire-and-forget and cannot show progress.
Extend it rather than adding a modal.

- `shared/stores/toast.ts`: add `update(id, patch)` so an existing toast can be mutated in place, and
  add an optional `progress?: { current: number; total: number }` to `Toast`. `show` already returns
  the id, which is the handle the caller updates.
- `apps/desktop/src/lib/components/common/Toast.svelte`: render a thin determinate bar when
  `toast.progress` is present.
- `shared/types/index.ts`: `LibraryScanProgress` mirroring the Rust struct field-for-field in
  snake_case.
- `apps/desktop/src/lib/stores/library.ts`: `startScanProgressListening()` on `library-scan-progress`;
  create the persistent toast (`duration: 0`) on the first event, `update` it thereafter, and dismiss
  it when the scan resolves. Started once from `apps/desktop/src/lib/hooks/useAppSetup.ts` next to the
  existing `exportStore.startListening()` (line 651) so it survives navigation away from Settings.
- i18n: new `settings.library.*` keys in all 15 locales (`en` at minimum, per AGENTS.md §5.8).

## Tasks

- [ ] T1 Backend: append the `idx_tracks_file_hash` migration
- [ ] T2 Backend: split `build_track_from_file` / `insert_track_in` in `import.rs`, behaviour-preserving
- [ ] T3 Backend: batch `scan.rs` in chunked transactions with intra-run hash dedupe + progress callback
- [ ] T4 Backend: `LibraryScanProgress`, `Clone` on the services, `spawn_blocking` + `library-scan-progress` emit
- [ ] T5 Backend: tests for batching (intra-chunk duplicate, rescan skip, batched counts) and `cargo test --features desktop`
- [ ] T6 Frontend: `toastStore.update` + `progress` bar in `Toast.svelte`
- [ ] T7 Frontend: `LibraryScanProgress` type, store listener, persistent toast lifecycle, `useAppSetup` wiring
- [ ] T8 i18n: new keys across all 15 locales
- [ ] T9 Verify: `cargo fmt --check -- --config tab_spaces=4`, `cargo clippy --features desktop -- -D warnings`, `cargo test --features desktop`, `yarn check:svelte`, `yarn check:svelte:mobile`, `yarn format:check`, `yarn lint:check`

## Constraints

- Migrations are appended, never edited (AGENTS.md §5.5).
- Register every command in `generate_handler!`; the scan command already exists and stays desktop-only
  (`#[cfg(feature = "desktop")]`). No signature change that breaks the existing registration.
- No `#[cfg(feature = "mobile")]` anywhere. `ArtworkService` is not desktop-gated, so its `Clone` derive
  must stay portable.
- `failed_count` keeps its current meaning; do not add fields to `LibraryFolderScanResult`.
- Upstream-portable and fork-only content never share a commit. `odd/` must never travel upstream.
- Do not touch `README.md`, `CHANGELOG.md`, or `docs/`.
- Artifacts in English; conversation in Rioplatense Spanish.

## Evidence log

### Slice 1 — backend (implemented, verified)

Delivered in the working tree in 7 files (367 insertions / 48 deletions): `db/schema.rs` (appended the
`idx_tracks_file_hash` migration), `services/library/import.rs` (`build_track_from_file` +
`insert_track_in`), `services/library/query.rs` (`find_track_by_hash_in`, and `fetch_tags_for_tracks`
converted to an associated fn so it can run under a caller-held guard), `services/library/scan.rs`
(chunked scan + progress + 5 tests), `services/library/mod.rs` (`LibraryScanProgress`, `Clone`),
`services/artwork.rs` (`Clone`), `commands/library.rs` (`spawn_blocking` + emit).

| Gate | Command | Result |
| --- | --- | --- |
| rustfmt | `cargo fmt --check -- --config tab_spaces=4` | GREEN |
| rust tests | `cargo test --features desktop` | GREEN — 183 passed, 0 failed (5 new) |
| cargo check | `cargo check --features desktop` | GREEN — no warnings |
| clippy | `cargo clippy --features desktop -- -D warnings` | **RED — pre-existing, base-only** (`device.rs:95`, untouched); error set is exactly that one |
| mobile compile | `cargo check --target aarch64-apple-ios ...` | **NOT RUN** — host cannot run it (see the prior feature's note) |

New tests in `services/library/scan.rs::tests`, all passing: `scan_imports_every_new_file`,
`rescan_skips_every_existing_file`, `duplicate_content_within_one_chunk_imports_once`,
`scan_imports_more_files_than_one_batch` (40 files, past the 32 chunk boundary),
`progress_runs_once_per_file_and_ends_at_total`. The suite synthesizes real 44-byte RIFF/WAVE PCM
files, so the import path is exercised end to end rather than mocked.

### Parent correction: `seen_hashes` designed in, then removed

The writer implemented the design as specified, including a `HashSet<String>` of hashes seen during the
run, inserted **before** the database lookup. Reviewing that diff, the parent identified two problems,
both with the mechanism itself rather than the writer's execution:

1. **It was unnecessary.** A database transaction makes its own uncommitted inserts visible to later
   reads on the same connection, so `find_track_by_hash_in(&tx, ..)` already finds a row inserted
   earlier in the same chunk. The `HashSet` duplicated what the transaction guarantees.
2. **It introduced a regression.** Because the hash was recorded before the lookup and the insert, a
   file whose insert failed still poisoned its hash, so a byte-identical second copy was counted as
   `skipped_existing_count` and silently lost. The previous per-file order imported that second copy.

The mechanism was removed. `duplicate_content_within_one_chunk_imports_once` still passes, which is the
empirical proof that in-transaction visibility does the job: had it not, that test would report 2
imports instead of 1.

### Environment trap hit again, and fixed

The global `~/.config/rustfmt/rustfmt.toml` (`tab_spaces = 2`) reindented 11 files in the working tree
(a bare formatter run, outside the parent's turn), which silently broke the repo's 4-space style while
`cargo fmt --check -- --config tab_spaces=4` was reported green. Restored with
`cargo fmt -- --config tab_spaces=4`; the diff then collapsed to the 7 intended files. This is the same
trap the prior feature recorded — a committed `rustfmt.toml` remains the durable fix.

### Slice 2 — frontend (implemented, verified)

21 files: `shared/types/index.ts` (`LibraryScanProgress`), `shared/stores/toast.ts` (`progress` field +
in-place `update(id, patch)`), `components/common/Toast.svelte` (thin determinate bar on semantic
tokens), `apps/desktop/src/lib/stores/scanProgress.ts` (new module-level store + idempotent listener +
toast lifecycle), `stores/library.ts` (dismiss in `finally`), `stores/index.ts` (barrel),
`hooks/useAppSetup.ts` (start once at bootstrap), and the one new key
`settings.library.musicFolderProgress` in all 15 locales.

| Gate | Command | Result |
| --- | --- | --- |
| svelte-check (desktop) | `yarn check:svelte` | GREEN — 0 errors, 0 warnings |
| svelte-check (mobile) | `yarn check:svelte:mobile` | GREEN — 0 errors, 0 warnings |
| Prettier | `yarn format:check` | GREEN |
| ESLint | `yarn lint:check` | GREEN |

**Deliberate architectural choice, recorded because a reviewer will ask:** progress state lives in its
own store module, **not** on `libraryStore`. `sortedTracks` is derived from `libraryStore`, so one
progress event per file written there would re-run `sortTracks` over the whole library thousands of
times — hanging the UI for exactly the large folders this work exists to make tolerable.

**Not verified:** the live event reaching the rendering toast. There is no frontend test runner in this
repository (no vitest/jest, no `test` script), so frontend coverage is type-check, lint and format only.
The event name and the snake_case field set are taken from the Rust struct by construction.

### Native review: not invoked, and now disabled at clone scope

The user directed in this session that the review lifecycle's cost is disproportionate for work of this
size and must be cut. No `inspect`, START, capture, or acknowledgement was run for either slice. Both
slices are therefore **verified but unreviewed**, by the same deliberate disposition the prior feature
recorded.

The switch was then turned off for this clone with explicit user authorization:
`gentle-ai review mode disable --scope clone`. Status reads `receipt-driven development: off (decided by
clone_local)`. That is what stops the reminder loop which repeatedly re-derived the fork's accumulated
divergence as a candidate; disabling is confined to this clone and is reversible with the matching
`enable --scope clone`. The two lineages left open by the prior feature are untouched — abandoning is
irreversible and was not requested.

## Evidence log (earlier reconnaissance)

- **Reconnaissance (read-only, 2026-09-24).** Root cause, index gap, blocking-runtime call, and the
  absence of any progress emit all confirmed by reading `import.rs`, `scan.rs`, `query.rs`,
  `db/mod.rs`, `db/schema.rs`, `resolution.rs`, `dirty.rs`, `hlc.rs`, `Cargo.toml`,
  `commands/library.rs:143`, `commands/discovery.rs:873`, `services/analysis.rs:110`.
- **Notification inventory (read-only).** `toastStore` at `shared/stores/toast.ts` renders through
  `ToastContainer.svelte`, mounted once at `apps/desktop/src/routes/+layout.svelte:444`; `show()`
  returns an id and `duration: 0` means persistent, but there is no in-place update path. The only
  progress-bar precedent is inline in `apps/desktop/src/lib/components/discovery/BulkImportView.svelte:245`.
- **`spawn_blocking` precedent**: `services/backup.rs:829` and `services/export/orchestration.rs`
  already run blocking work off the async runtime.
- **State ownership**: `lib.rs:385` builds `LibraryService::new(conn, app_data_dir)` and `lib.rs:440`
  manages the plain value (not an `Arc`), so the command must clone the service into the blocking task.
