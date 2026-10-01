# Track store URL (WOAR)

## Objective

Add a `url` property to `Track` holding the store page URL of the track (Beatport, Bandcamp or
Traxsource — wherever the tags were fetched from). Persist it in the audio file using the ID3v2
**WOAR** frame, carry it automatically when a tagger candidate is applied, and show it in
`TrackMetadataModal` as an editable field with a clickable action that opens the system browser
(`openUrl` from `@tauri-apps/plugin-opener` — the app-internal equivalent of `xdg-open`, already
registered with `opener:default` permission).

## Problem / Why

Tracks tagged from a store have no link back to the store page. Users want to jump from Crate
metadata to the original Beatport/Bandcamp/Traxsource page.

## Scope

- DB: new `tracks.url` column via appended migration (9).
- Model/IPC: `Track.url: Option<String>`, `TrackUpdate`, `TrackMetadataPatch` (desktop-only), TS mirrors.
- Audio tags: read WOAR on import (`ItemKey::TrackArtistUrl` ⇄ "WOAR" verified in lofty 0.22.4
  `src/tag/item.rs:205`), write WOAR on metadata save through `write_metadata_patch` + legacy
  `write_track_meta`.
- Tagger: `candidateToPatch` must stop dropping `candidate.url` (providers already build and
  preserve it through `extend` — beatport.rs:379, bandcamp.rs:153, traxsource.rs:171).
- UI: `TrackMetadataModal` URL row (input + open-in-browser link button), i18n key in 15 locales.
- Consistency surfaces: import UPSERT, all track SELECT column lists, `persist_metadata_patch`,
  backup restore, cloud-sync serialization + synced columns (follow `catalog_number` exactly).

Out of scope: rekordbox/export integrations, discovery-release URL display changes.

## Constraints

- Track struct is cross-platform; `TrackMetadataPatch`/`TrackUpdate` are `#[cfg(feature = "desktop")]`.
- Migrations: append to `get_migrations()` in `src-tauri/src/db/schema.rs`, never edit existing entries.
- Wire format is snake_case; mirror field names exactly in `shared/types/index.ts`.
- Keep the diff small and focused (~400-line review heuristic).

## Route: delegated direct (writer trigger: 2+ non-trivial files; mapping trigger fired)

Evidence: exploration covered 15+ files across src-tauri and frontend; one bounded writer owns
backend + frontend.

## Tasks

- [x] T1 — Rust model + migration: `Track.url`, `Track::new`, `TrackUpdate`, `TrackMetadataPatch`,
  migration 9 (`ALTER TABLE tracks ADD COLUMN url TEXT`), `column_exists` test assertion.
  Checks: `cargo check --release --features desktop` → passed (`yarn check:cargo`, Finished release
  profile). `Track.url` placed under External references after `rekordbox_id` with WOAR doc comment;
  fresh-db migration test gained `column_exists("tracks","url")` assertion and passes.
- [x] T2 — DB plumbing: add `url` following every `catalog_number` occurrence — SELECT lists + row
  maps (library/query.rs, playlist/tracks.rs, playlist/smart.rs, analysis.rs ×2, export/collection.rs
  ×2, cloud_sync rows.rs), import UPSERT, `persist_metadata_patch` + `has_embedded_edits` +
  `apply_patch_to_track`, update.rs dynamic columns, backup restore INSERT, cloud-sync UPSERT +
  synced cols + assert_sync_schema tests.
  Checks: `cargo test --features desktop` → 296 passed, 0 failed, 4 ignored.
  Placement decision: `url` appended at the END of every positional column list (index 29 after
  `relative_path` in Track SELECTs; index 25 after `color` in BackupTrack lists, `_hlc` shifted to
  26 in rows.rs) — existing indices untouched. `BackupTrack.url` is `Option<String>` appended at
  struct end so pre-url backup/sync JSON deserializes as `None`. No cloud-sync test asserted an
  exact column set (verified: no `url`/column-set matches in `cloud_sync/tests`).
  BLOCKER RESOLVED (deviation #2): `db::tests::repairs_bitrates_written_as_bit_depths` assumed the
  bitrate-repair migration was always the LAST entry (`take(migration_count - 1)`, re-applied
  `migrations.last()`); appending migration 9 broke its simulation. Adapted the test to pin
  `REPAIR_MIGRATION_INDEX = 7` (stable — migrations are append-only, never reorder). The migration
  SQL itself was not touched.
- [x] T3 — WOAR tag persistence: read on import (`extract` WOAR → `track.url`), write in
  `write_metadata_patch` + `verify_metadata_patch` + legacy `write_track_meta` via
  `ItemKey::TrackArtistUrl`; roundtrip unit test (in-memory Id3v2 tag: WOAR ⇄ url).
  Checks: `cargo test --features desktop` → new tests pass:
  `file_tags::tests::metadata_patch_roundtrips_url_through_the_woar_frame`,
  `file_tags::tests::unchanged_url_patch_leaves_the_woar_frame_untouched`,
  `library::import::tests::extract_url_reads_woar_tag_trimmed`,
  `extract_url_treats_blank_woar_as_absent`, `extract_url_returns_none_without_woar_frame`.
  Deviation #1: WOAR read added to BOTH import twins (`import_single_track` and
  `build_track_from_file`) — the instruction named only the latter; skipping the twin would let
  `import_tracks` silently drop url. Blank/whitespace URL → `None` everywhere via `extract_url`.
- [x] T4 — Tagger apply: `candidateToPatch` includes `url: candidate.url` when non-empty (omit when
  empty, keeps Unchanged); update the drop-list comment in shared/stores/tagger.ts.
  Checks: `yarn check:svelte` → parity with baseline. Verified `applyBatch` (:436),
  `applyAutoRows` (:174) and single-apply (:287) all route through `candidateToPatch` — no other
  changes needed. Trimmed value is sent; empty → key omitted entirely.
- [x] T5 — TS types: `Track.url`, `TrackUpdate`, `TrackMetadataPatch` in shared/types/index.ts.
  Checks: `yarn check:svelte` + `yarn check:svelte:mobile` → passed.
  Deviation #3: `shared/utils/tracklistColumns.test.ts::makeTrack` (full-Track literal, type-checked
  by svelte-check although no JS runner gates it) required `url: null` — added, mirroring
  `catalog_number: null`.
- [x] T6 — UI: URL editable row in TrackMetadataModal with open-link button using `openUrl`;
  dirty/save wiring via existing `EditableField`/`buildPatch` machinery.
  Checks: `yarn check:svelte` → parity with baseline (only pre-existing env error remains).
  'url' added to `EditableField`, `nullableFields`, `fieldLabels`, and the text-loop filter; clear
  (`null` → `MetadataField::Clear`) rides the existing `buildPatch`. `external-link` IconButton at
  `right-8` (clear 'x' stays at `right-1.5`), shown only when value non-empty,
  `openUrl(value).catch(() => {})` per FollowingModal precedent; title via new i18n key
  `modals.trackMetadata.openUrl` (neighboring buttons all use `$translate`).
- [x] T7 — i18n: `modals.trackMetadata.fields.url` in all 15 locale files ("URL" everywhere —
  universal acronym) + `modals.trackMetadata.openUrl` translated in all 15 (es "Abrir en el
  navegador", pt "Abrir no navegador", fr "Ouvrir dans le navigateur", de "Im Browser öffnen",
  it "Apri nel browser", ja/ko/nl/pl/ro/sv/tr/uk/zh standard phrasings).
  Checks: JSON valid (all 15 parsed+rewritten via json module; `git diff --stat` shows +5/-1 per
  locale), `yarn check:svelte` passed. Known drift found: non-en locales had NO
  `modals.trackMetadata.fields` subtree and no `clearField` key before this change (English
  fallback); the new `fields` subtree therefore contains only `url` in those locales.
- [x] T8 — Full gates (see Verification evidence below for per-command results). Mobile cargo
  check NOT available: only target `x86_64-unknown-linux-gnu` is installed (`rustup target list
  --installed`); `aarch64-apple-ios` / `aarch64-linux-android` absent. Manual mobile-safety review:
  every Rust addition sits inside the same cfg regions as its neighbors; new fields are plain
  `Option<String>`; opener permission unchanged (`opener:default` already present).
- [x] T9 — Work-unit commit. User decided: commit directly on `dev` (over the `{issue-number}-{slug}`
  branch convention; no tracking issue existed). Evidence: SHAs recorded at the bottom of this file.
- [x] T10 — RDD assess: SKIPPED by policy. `gentle-ai review mode status` reads OFF
  (decided by clone_local); while disabled no review is started (orchestrator contract). Functional
  gates + independent verification stand as the record of proof. Assessment tier was probed
  read-only pre-commit: `high` (hot_path metadata_update.rs, 37 paths / 316 lines).

## Verification evidence

Observed by the delegated writer, 2026-10-01, repo root unless stated (`src-tauri/` marked):

- `yarn format:fix` → passed; `yarn format:check` re-run after final edits → "All matched files use
  Prettier code style!" (exit 0) — converged.
- `yarn lint:check` → exit 0, 0 rule hits. (Baseline before edits: also exit 0.)
- `yarn check:svelte` → "found 1 error and 2 warnings in 2 files" — IDENTICAL to the baseline
  captured before any edit. Pre-existing env error: `apps/desktop/src/routes/+layout.svelte:12:11
  PUBLIC_APP_VERSION missing from $env/static/public` (generated-env types not built locally). One
  transient error introduced mid-run (missing `url` in `tracklistColumns.test.ts` fixture) was fixed
  (deviation #3) and re-verified at parity.
- `yarn check:svelte:mobile` → "found 0 errors and 0 warnings" (also 0 at baseline).
- `yarn check:cargo` → Finished `release` profile [optimized] target(s) — passed.
- src-tauri `cargo fmt --check` → exit 1, diffing ONLY the three files already non-conformant at
  baseline before any edit (`build.rs`, `src/main.rs`, `services/library/metadata_update.rs`) —
  known environmental failure, deliberately NOT "fixed": blanket `cargo fmt` would reformat those
  unrelated files and bloat the diff; metadata_update.rs (touched file) kept in its local 4-space
  style. All other edited Rust files were individually normalized with `rustfmt --edition 2021`.
- src-tauri `cargo clippy --features desktop -- -D warnings` → Finished, zero warnings — passed.
- src-tauri `cargo test --features desktop` → "296 passed; 0 failed; 4 ignored" — passed, including
  the 5 new url/WOAR tests and the pre-existing suite.
- Mobile `cargo check --target aarch64-apple-ios --no-default-features --features mobile` →
  UNAVAILABLE: target not installed (only `x86_64-unknown-linux-gnu`).

INDEPENDENT VERIFICATION (fresh verifier, 2026-10-01): 1) Positional SQL audit PASS after one fix —
all Track SELECTs (query.rs ×3, tracks.rs, smart.rs, analysis.rs ×2, collection.rs ×2) carry `url`
last at index 29 with `url: row.get(29)?`; rows.rs `read_live_tracks` url=25/_hlc=26 verified;
import.rs UPSERT 31 columns ↔ 31 params aligned (SET omits url exactly like catalog_number); backup
export SELECT url=25↔get(25); restore INSERT ?26↔`t.url` last; update.rs/metadata_update.rs dynamic
arrays order-independent by construction; writers.rs upsert_track 27 cols/27 params/SET `url` all
aligned. DEFECT FOUND & FIXED (minimal, inside already-changed lines): `upsert_cue` SET clause had
stray `url=excluded.url` on the `cues` table, which has no `url` column (migration 9 is tracks-only;
SQLite rejects it at prepare: "no such column: excluded.url", reproduced with sqlite3 CLI) — cue
merges would fail at runtime with zero test coverage (`Bucket::Cues` never exercised in tests);
removed the term, restoring the baseline cue SET line. 2) Test coverage: covered — query.rs
get_track/find_track_by_hash (metadata_update + scan tests), import twin + extract_url (scan tests,
3 new unit tests), metadata_update persist/apply (7 tests), rows.rs read_live_tracks + writers.rs
upsert_track (convergence, incl. `serialize_bucket` over all buckets); NO test coverage —
analysis.rs ×2 (worker-pool tests only; real analysis ignored), export/collection.rs ×2,
playlist/tracks.rs, playlist/smart.rs, library/update.rs, services/backup.rs (SELECT+INSERT, zero
tests), query.rs get_tracks (command-only path), writers.rs upsert_cue (the bug's hiding spot).
3) `cargo test --features desktop` → "296 passed; 0 failed; 4 ignored"; `cargo clippy --features
desktop -- -D warnings` → Finished, exit 0; `cargo fmt --check` → exit 1 diffing ONLY the three
allowed baseline files (build.rs, src/main.rs, metadata_update.rs) — PASS. 4) `yarn check:svelte` →
1 error + 2 warnings, identical to baseline (PUBLIC_APP_VERSION @ +layout.svelte:12:11, LibraryTab
a11y); `yarn check:svelte:mobile` → 0 errors/0 warnings; `yarn lint:check` → exit 0; `yarn
format:check` → exit 0; node JSON.parse over all 15 locales → valid, every locale has
`modals.trackMetadata.fields.url` + `modals.trackMetadata.openUrl` (0 failures). 5) Semantic spot
PASS: tagger.ts includes `url` only when trim non-empty (TagCandidate.url is `string`, trim safe);
modal buildPatch emits `url: null` on clear (nullableFields loop), openUrl button guarded on
non-empty value, imports `openUrl` from `@tauri-apps/plugin-opener`; file_tags.rs write/clear/verify
all use `ItemKey::TrackArtistUrl` (Clear removes frame — roundtrip test); import.rs `extract_url`
trims + blank→None in BOTH twins. Migration-index claim verified: repair = migrations[7] of 9,
append-only. 6) Final totals: 37 files, +271/−44. OVERALL: PASS (with 1 fix applied). T9/T10 remain
unchecked.

## Deviations & decisions

1. WOAR read added to both import twins, not just `build_track_from_file` (see T3).
2. `repairs_bitrates_written_as_bit_depths` test adapted to reference the repair migration by stable
   index 7 instead of `migration_count - 1` (see T2).
3. `makeTrack` fixture in `shared/utils/tracklistColumns.test.ts` gained `url: null`.
4. Column-placement strategy: `url` appended at the end of all positional SQL lists (no index
   renumbering); struct fields placed as specified (`Track.url` near `rekordbox_id`,
   `BackupTrack.url` at end).
5. `openUrl` i18n key added in all 15 locales (neighbor buttons are `$translate`-driven, so the key
   — not a literal — is the consistent choice).
6. No new IPC commands, no capability changes, no scope creep into smart_rules.rs, export/pdb/anlz,
   or discovery UI.

## Next step

T9 branch decision + work-unit commit (owner: orchestrator/user), then T10 RDD assess on the commit.

## Delivery forecast

Forecast was ~350–450 changed lines. **Actual measured diff: 37 files, +272 / -44 = 316 authored
changed lines** (`S=$(git stash create); git diff --stat "$S^" "$S"` — note: `git diff "$S"` alone
differs the snapshot against the identical worktree and yields empty). Below the ~400 review-focus
threshold → `ask-on-risk` resolves to no split needed. Running count: 316.

## Work-unit commit evidence

- feat commit: b681257 `feat(track): add store url property persisted via WOAR tag and openable in browser` (37 files, +271/−43) on `dev`
