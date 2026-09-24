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
- [x] T2 Backend: single source of truth for supported audio extensions, used by all backend call sites
- [x] T3 Backend: `ensure_root` + `root_mapping` + canonicalize in `set_root_mapping` + deterministic longest-prefix root matching, with tests
- [x] T4 Backend: `services/library/scan.rs` — resolve root, walk, hash-skip, import, count
- [x] T5 Backend: `get_music_library_folder` / `set_music_library_folder` / `scan_music_library_folder` commands, registered in `generate_handler!` under `#[cfg(feature = "desktop")]`
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
| `3ba365b` | `docs(odd): track library-folder-scan feature` | `odd/tasks/library-folder-scan.md` |
| `2af6559` | `fix(cloud-sync): resolve library roots deterministically by longest prefix` | `services/cloud_sync/resolution.rs` |
| `6c54963` | `feat(library): scan a music folder and import new tracks` | `services/library/{mod,import,relocation,scan}.rs`, `commands/library.rs`, `lib.rs` |
| `5c07605` | `feat(library): expose the music folder commands to the frontend` | `shared/api/library.ts`, `shared/types/index.ts` |
| `dde8148` | `feat(settings): choose and scan the music folder in Library settings` | `apps/desktop/src/lib/stores/library.ts`, `settings/tabs/LibraryTab.svelte`, `shared/i18n/locales/*.json` (15) |
| `9f73ff2` | `docs(odd): record library-folder-scan evidence` | `odd/tasks/library-folder-scan.md` |
| `ae5108a` | `docs(odd): record library-folder-scan review outcome` | `odd/tasks/library-folder-scan.md` |
| `892e3c6` | `fix(library): keep a failed folder scan distinguishable and non-fatal` | `services/library/scan.rs`, `apps/desktop/src/lib/stores/library.ts`, `settings/tabs/LibraryTab.svelte` |

Portability: `2af6559`, `6c54963`, `5c07605` and `dde8148` are upstream-portable (no fork
tooling); `3ba365b` and the evidence commit are fork-only and must never travel upstream.
`2af6559` is an independently useful fix and can be offered upstream on its own, before the
feature.

## Slice 1 evidence (backend)

- **Design correction found during reconnaissance.** The plan said `register_root_with_id`; the
  signature scout showed `register_root` generates `uuid::Uuid::new_v4()` (`resolution.rs:105`). The
  implemented shape is an additive `ensure_root(conn, id, name)` (idempotent insert, dirty only when a
  row is really inserted) with `register_root` delegating to it, which keeps existing callers
  byte-identical while allowing the stable id `local-music-library`.
- **Verification run by the implementing writer:** `cargo test --features desktop` → **178 passed,
  0 failed** (4 new tests: `nested_roots_resolve_to_most_specific`,
  `file_outside_every_mapping_resolves_to_none`, `sibling_prefix_does_not_match`,
  `symlinked_path_resolves_to_mapped_root`); `cargo check --features desktop` → ok;
  `cargo fmt --check -- --config tab_spaces=4` → clean.
- **`cargo fmt` could not be run bare**, and this is an environment trap worth remembering: there is a
  global `~/.config/rustfmt/rustfmt.toml` with `tab_spaces = 2` and the repo has no `rustfmt.toml`, so
  a bare `cargo fmt` rewrites the whole crate to 2 spaces. The CI gate is unaffected (CI has no such
  global config); locally, verify with `cargo fmt --check -- --config tab_spaces=4`.
- **Extension dedup verified independently with `ast-grep`**: exactly one copy of the extension array
  remains in Rust (`services/library/mod.rs:29`) and it is referenced from `import.rs:51`,
  `import.rs:230`, `relocation.rs:43` and `scan.rs:89`. The other `"aiff"` occurrences are per-extension
  `match` arms with different semantics (`hash.rs:97` container-header skip,
  `export/device_library_plus/models.rs:40`, `export/pdb/writer.rs:229`) and were correctly left alone.
- **Parent review of the diff** (not delegated): `generate_handler!` registers the three commands with
  `#[cfg(feature = "desktop")]` on its own line; the extension substitution in `import.rs` keeps the
  lowercasing and the unsupported-format error path unchanged; `mod scan;` is declared.
- **Behaviour changes accepted in `resolution.rs`**: `set_root_mapping` now stores the canonicalized
  path (fallback: raw string); `try_assign_root_for_import` canonicalizes the candidate (fallback: raw
  path) and orders by descending mapped-path length. Pre-existing mappings written non-canonically
  before this commit may stop matching until re-saved — recorded as a known consequence, not a bug.
- **Known limitations carried into the frontend slice**: a hard DB error in `find_track_by_hash`
  propagates and aborts the scan (only per-file hash/import errors accumulate); `ensure_root` advances
  the HLC clock even on an idempotent no-op (deliberate, to keep `register_root` behaviour identical);
  the mobile target had not been compiled yet at slice 1 (it is a T10 gate). The canonical
  `scanned_count` semantics is **"supported audio files discovered"**, not "files walked" — the UI must
  label it that way.

## Slice 2 evidence (frontend)

- **Writer stalled once** mid-slice (timed out after an edit) having completed the types, API wrappers,
  store and component plus 7 of 15 locales. It was resumed with a narrowed prompt rather than relaunched,
  and finished the remaining 8 locales and all four frontend gates.
- **Parent review of the frontend diff** before committing: `LibraryFolderScanResult` matches the Rust
  struct field-for-field in snake_case; the store's success path is correct because `loadTracks()`
  resets `loading` in both paths and swallows its own errors, so a refetch failure cannot be
  misreported as a scan failure; every `Text`/`Button` prop used (`truncate`, `tabular`,
  `color="danger"`, `title`, `disabled`) exists in the component definitions.
- **i18n coverage verified programmatically**: all 13 keys present in all 15 locales, `{error}`
  preserved verbatim in both `*Error` keys, and every key referenced by `LibraryTab.svelte` resolves in
  `en.json`. Translations authored by the model and **not native-reviewed**: `pt` assumes Brazilian
  Portuguese; `ja`/`ko`/`zh` naturalness and `uk`/`ro`/`tr` terminology are the weakest confidence
  (notably `sv` "Sök igenom igen" and `uk` "З помилками" are the least literal renderings).
- **One accepted scope deviation**: `yarn format:check` failed on `LibraryTab.svelte` and the sanctioned
  `yarn format:fix` collapsed a three-line `$translate(...)` call to one line. Whitespace-only, inside
  the authored section.

## T10 verification (independently delegated, read-only)

| Gate | Command | Result |
| --- | --- | --- |
| rustfmt | `cargo fmt --check -- --config tab_spaces=4` | GREEN |
| clippy | `cargo clippy --features desktop -- -D warnings` | **RED — pre-existing, base-only** |
| rust tests | `cargo test --features desktop` | GREEN — 178 passed, 0 failed |
| mobile compile | `cargo check --target aarch64-apple-ios --no-default-features --features mobile` | **NOT VERIFIED — host cannot run it** |
| svelte-check (desktop) | `yarn check:svelte` | GREEN — 0 errors, 0 warnings |
| svelte-check (mobile) | `yarn check:svelte:mobile` | GREEN — 0 errors, 0 warnings |
| Prettier | `yarn format:check` | GREEN |
| ESLint | `yarn lint:check` | GREEN |

**The clippy gate is red on this branch and was already red before it.** The single error is
`clippy::double-ended-iterator-last` at `src-tauri/src/services/device.rs:95`
(`device.split('/').last()` where clippy wants `next_back()`), promoted to an error by `-D warnings`.
Evidence that it is not ours: `git diff --name-only 3ba365b~1 HEAD` does not list `device.rs`, and the
offending line is byte-identical at `3ba365b~1`. The resolved toolchain honours the pin
(`rustc 1.95.0-nightly c04308580`, `clippy 0.1.95`). This means the exact CI clippy gate fails on
`develop` as well — an open question worth its own diagnosis (whether upstream CI is genuinely red, or
CI's clippy differs from the pinned toolchain), deliberately **not** fixed here because an unrelated
fix does not belong in this feature branch.

**The mobile compile gate is not runnable on this Linux host, and this is a property of the host, not
the change.** The target was installed correctly (`aarch64-apple-ios` on `nightly-2026-02-19`), but the
build dies in the build script of the transitive dependency `objc2-exception-helper`, which needs
`xcrun` to locate the iOS SDK; `xcrun`/`xcode-select` do not exist off macOS. No `Checking crate` line
is ever reached, so `services/cloud_sync/resolution.rs` — the one feature file that is **not**
desktop-gated and therefore must compile on mobile — was never actually type-checked for iOS. CI runs
this gate on `runs-on: macos-latest` (`.github/workflows/ci.build.yml:87`, job defined at `:62`). The feature touches neither
`Cargo.toml` nor `Cargo.lock`, so the dependency graph is identical to the base.

Residual mobile risk, stated instead of assumed: the desktop-gated feature files cannot be part of a
mobile build by construction (`services/mod.rs:21`, `commands/mod.rs:19`), and the `resolution.rs`
changes use only cross-platform std (`std::fs::canonicalize`, `std::path::Path`) plus the already-present
`rusqlite` extension trait and `uuid`. That is a structural argument, not a compile result.

## Native review, first pass (RDD, lineage `review-8569dcf40fea291e`)

RDD reads `on (decided by default)`, so the candidate went through the native lifecycle. The candidate was
the **feature slice** (`0d69bd2..HEAD`, committed-only, 27 paths), not the fork's accumulated divergence:
the first `inspect` derived `746c7c5` as the base, which would have swept in 12 commits of harness and docs
work from earlier sessions, and the review contract explicitly forbids the accumulated feature branch as a
candidate. Re-inspecting with the narrower `baseRef` produced the correct 27-path projection.

| Field | Value |
| --- | --- |
| Lineage | `review-8569dcf40fea291e` |
| Target | `sha256:806e705bffb45814af057923f7106e199a6018fb270b10938c23d375067bace8` |
| Tier / changed lines | medium / 1081 |
| Lenses selected | `review-reliability` (one consolidated lens for this tier) |
| Reviewers prepared / submitted | 1 / 1 (`pi_host_relay`, 90439 prompt bytes, 3863 result bytes) |
| Outcome | **approved** |
| Acknowledgement | `gentle-ai.review-acknowledged/v1`, authority **burned** |
| Delivery | ordinary repository policy (not granted by the review) |

### Advisory findings — all non-blocking, no correction opened

The closure states it plainly: every finding is informational, none opened a correction, none reopens the
review, and no correction transition is offered for this candidate. They are recorded as later work.

| Id | Lens | Location | Severity | The parent's reading of the location |
| --- | --- | --- | --- | --- |
| R3-001 | reliability | `services/library/scan.rs:48-97` | WARNING | The scan body has no bound on the walk (no file cap, no cancellation) and folds `WalkDir` traversal errors into the same `failed_count` as per-file import errors, so the summary conflates two different failure classes |
| R3-002 | reliability | `services/library/scan.rs:102-115` | SUGGESTION | `self.find_track_by_hash(&hash)?` propagates a DB error and aborts the whole scan, discarding counts for everything already imported in this run |
| R3-003 | reliability | `settings/tabs/LibraryTab.svelte:50-57` | WARNING | The component infers scan failure from `get(libraryStore).error`, shared mutable state, instead of from the outcome of the call it just made — a stale error from another operation would be misattributed to the scan |

The reviewers' own finding text lives in the native store; the table records the provider-issued
locations and severities plus the parent's reading, never a paraphrase presented as the reviewer's words.

> **This approval is weak evidence — see the second review below.** The same lens on a nearly identical
> candidate produced these three advisory findings with one model and two *blocking* findings with another.
> An approval emitted by a model that intermittently cannot produce output is not a reliable signal, and
> the fact that it arrived first is an accident of ordering.

### Infrastructure blockers hit and resolved during the lifecycle

1. `inspect` was first blocked with `package-local-binary-missing`: the `gentle-pi` package had no
   package-local `gentle-ai` binary (only `bin/gentle-shell.mjs`). Resolved with the sanctioned recovery
   `node scripts/install-gentle-ai.mjs` from the package directory, which installed v3.7.0 into
   `.gentle-ai/v3.7.0/gentle-ai`. **Side effect, user-visible and expected:** that installer also ran
   `installTuiModeSetting()` (the package directory is a pi-managed install) and enabled fullscreen in the
   global Pi settings. `GENTLE_PI_SKIP_GENTLE_AI_INSTALL` was not set, so this was an install that had
   never completed rather than a deliberate opt-out.
2. The first reviewer run failed with `reviewer-config-invalid`: *"no model is configured for
   review-reliability"*. The review host relay refuses to fall back to an ambient default model by design
   (`lib/review-host-relay.ts:594-598`), which is why `gentle-ai-worker` ran fine via `subagent_run` while
   the reviewer could not. Root cause: **`~/.pi/gentle-ai/models.json` did not exist** (`gentlePiConfigHome()`
   is `~/.pi/gentle-ai` per `lib/agent-home.ts:14`; `modelConfigPath` is that directory's `models.json` per
   `extensions/gentle-ai.ts:1959`). A first attempt wrote `~/.pi/agent/subagents.json`, which is a
   **different** mechanism (`agentModelProfileConfigPath`, the subagent model profiles) and did not resolve
   the blocker. The working fix is a flat `routingKey -> {model}` map at `~/.pi/gentle-ai/models.json`,
   verified by importing the package's own `readModelConfigFile` (status `valid`, all four keys accepted,
   `review-reliability -> command-code/deepseek/deepseek-v4.1-flash`). Both files now exist; the
   `subagents.json` one is harmless but was not the fix.

## Second review and the correction blocker

A later reminder reported an unreviewed candidate: the evidence commit `ae5108a` had moved HEAD inside the
committed range, so the fork's **accumulated divergence** reappeared as the candidate (41 paths, 2412
lines). `inspect` named exactly that target (`a5dba4db`) and offered `review.start` for it. The user chose
to postpone it, so its lineage `review-db1c6d4828a4bd60` was left **open and `reviewing`** (a forecast was
taken; nothing ran) rather than abandoned — abandoning is irreversible and would foreclose reviewing it
later. The review was then narrowed again to the feature slice: lineage `review-b101f3dba7f53a07`, target
`e37b1820`, 27 paths, **1147** changed lines (the extra 66 over the first pass are the evidence docs).

### The reviewer model had to be replaced

The first attempt reused `command-code/deepseek/deepseek-v4.1-flash` and **failed with
`stopReason: length` after 115 s, emitting no text at all**. The same model and lens had produced a result
on the nearly identical earlier candidate, so it is not reliable in this role: it intermittently burns its
whole output budget on reasoning. It was replaced with `command-code/Qwen/Qwen3.7-Flash` plus
`thinking: low`. Two facts discovered on the way:

- `thinking: "minimal"` is **rejected upstream**: *"Invalid option: expected one of
  \"low\"|\"medium\"|\"high\"|\"xhigh\"|\"max\""*. Pi's `THINKING_LEVELS` enum
  (`lib/model-routing-authority.ts:4`) includes `off` and `minimal`, but the command-code API does not
  accept them. `low` is the floor.
- With a working model the verdict flipped to **`correction_required`**.

### The correction

The provider named exactly two findings, both `evidence_class: deterministic` and
`causal_disposition: introduced` (ours):

| Id | Severity | Location | Claim |
| --- | --- | --- | --- |
| R3-002 | CRITICAL | `services/library/scan.rs:90-95` | A DB error in `find_track_by_hash` aborts the entire scan, discarding all counts and imported ids collected so far |
| R3-003 | BLOCKER | `stores/library.ts:155-158` | The store returns an empty result on failure, so the caller misreads a failed scan as a successful empty scan |

Both were real and both were ours. Fixed in `892e3c6` (**41 diff lines**: 20 insertions + 21 deletions,
inside the frozen 200-correction budget): the lookup error is absorbed as a per-file failure, the store
rethrows instead of fabricating a result, and the component derives the outcome only from its own call.
All seven gates were re-run green after the fix (rustfmt with `--config tab_spaces=4`, `cargo test` 178
passed, `cargo check`, svelte-check desktop and mobile, `format:check`, `lint:check`).

### Blocker: the correction-plan slot cannot be submitted

The correction plan was rejected **three times, with three different reasons**, none of them about content,
and nothing was consumed on any attempt (`mutation_performed: false`):

1. `capture-binding-rejected` — *"unknown, expired, or belongs to a different session route"*;
2. `capture-binding-rejected` — *"unknown, expired, or belongs to a different session route"*, using the
   freshly re-rendered binding with its argument order preserved;
3. `capture-binding-rejected` — *"does not carry one non-empty matching provider lineage and target token"*.

Evidence that this is a defect in the route rather than a mistake in the submission:

- two renders of the **same** binding serialized `submission.argumentTokens` with `--request-hash` and
  `--repository-context` **swapped**, while `substitutionLocation: 5` pointed at `--correction-lines` in
  both;
- the current render contradicts itself: `arguments` orders `repository-context` before `request-hash`,
  while `submission.argumentTokens` orders them the other way;
- after the fix was committed, `STATUS` exposed **both** identities at once —
  `target_identity: sha256:33ff9a35...` (current, tree `9964d725`) and `authority_target_identity:
  sha256:e37b1820...` (what the correction is bound to) — while still offering the plan request bound to the
  pre-correction target.

The plausible reading, which could **not** be proven without destructive git surgery, is that the plan must
be declared while the candidate still matches the authority target, and that window was closed because the
fix was prepared in the working tree (then committed) before the plan was submitted. The user chose to
document the blocker rather than rewrite history, so lineage `review-b101f3dba7f53a07` remains
`correction_required` with `892e3c6` committed and verified, and the review lifecycle is **not closed**.

## Follow-ups (recorded, deliberately not done here)

1. **`clippy::double-ended-iterator-last` at `services/device.rs:95`** — pre-existing, blocks the CI
   clippy gate for every branch. Diagnose whether upstream CI is red or diverges from the pin, then fix
   on its own branch; it must not ride along with this feature.
2. **The correction lifecycle is stuck** (see the blocker section). Two ways forward: retry the plan with
   the candidate restored to the authority target, or review the corrected candidate under a fresh
   lineage. Until one of them happens, `review-b101f3dba7f53a07` stays `correction_required`.
3. **R3-001: bound the walk** (cap and/or cancellation) and separate traversal failures from per-file
   import failures in `LibraryFolderScanResult` so the summary stops conflating them. Not fixed here
   because the correction was bounded to R3-002 and R3-003.
4. **No `rustfmt.toml` in the repo** — a global `~/.config/rustfmt/rustfmt.toml` with `tab_spaces = 2`
   makes a bare `cargo fmt` rewrite the whole crate locally. A committed `rustfmt.toml` would pin the
   style for every contributor and is upstream-portable.
5. **`clear_music_library_folder`** and the ability to unset or rename the folder from Settings.
6. **No progress feedback or cancellation** during a long first scan.
7. **No backfill** of `library_root_id`/`relative_path` for tracks imported before the folder was set.
8. **Extension list still duplicated in the frontend** (`trackController.ts:157`,
   `playlistController.ts:255`) — the backend now has one source of truth; the TS copies are separate.
9. **Native review of the new i18n keys** in the 13 lower-confidence locales (`pt`, `ja`, `ko`, `zh`,
   `uk`, `ro`, `tr`, `sv` are the weakest).
10. **Assign models to the remaining agent routing keys** in `~/.pi/gentle-ai/models.json`, which currently
    covers the four `review-*` lenses only. `jd-judge-a`, `jd-judge-b`, `jd-fix-agent` and every `sdd-*`
    agent will hit the same `reviewer-config-invalid` refusal the first time they are driven through the
    host relay.
11. **Enable a stronger model for reviewers.** All six models in `enabledModels` are lightweight/flash-tier,
    and that already cost one wasted run. An adversarial reviewer benefits from the strongest available
    model.

**Resolved by this work and therefore removed from the list:** R3-002 and R3-003, both fixed in `892e3c6`.

## Open question for the user

`library_roots` rows sync, so the designated `local-music-library` root appears on other devices as an
*unmapped* root until the user picks a folder there. This was judged the intended multi-device behaviour
(each device maps its own path, and the synced `library_root_id` + `relative_path` resolve locally), but
it is a product-visible consequence the user has not explicitly confirmed.
