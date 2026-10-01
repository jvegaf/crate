# SQL positional read/write regression tests

## Objective

Close the test-coverage gaps found while shipping Track.url/WOAR: the positional `row.get(n)`
SQL sites that a green `cargo test` does NOT exercise. Bug class proven by the `upsert_cue`
stray-`url` defect (caught only by manual audit + sqlite3 repro, not by the 296 passing tests).

## Scope — sites to cover

1. `src-tauri/src/services/library/query.rs` — `get_tracks` (29-col map; only get_track /
   find_track_by_hash_in were covered before).
2. `src-tauri/src/services/playlist/tracks.rs` — playlist tracks SELECT + closure.
3. `src-tauri/src/services/playlist/smart.rs` — smart playlist evaluation read.
4. `src-tauri/src/services/analysis.rs` — both track queries (~:690/:706, ~:782/:797).
5. `src-tauri/src/services/export/collection.rs` — both USB export reads (~:110/:129, ~:180/:201).
6. `src-tauri/src/services/backup.rs` — export + restore roundtrip (BackupTrack, INSERT ~:454).
7. `src-tauri/src/services/library/update.rs` — `update_track` / `update_tracks` dynamic arrays.
8. `src-tauri/src/services/cloud_sync/pipeline/merge/writers.rs` — `upsert_cue` merge path
   (the exact bug: a column from another table leaking into the cue ON CONFLICT SET must fail
   the test, not runtime).

## Design

Seed an in-memory migrated DB with distinctive sentinel values for EVERY track column (including
`url`), then run each site and assert the full returned `Track` field-by-field — any
SELECT-list/closure index drift breaks a test instead of the app. For `upsert_cue`, add a
cloud-sync merge test applying a cue row (live + tombstone) so the SQL is prepared and executed.

## Constraints

- Tests only; no source-behavior changes. If a test reveals a REAL defect, fix minimally and
  record it here + in Engram.
- Follow existing test patterns (in-db modules, `db/mod.rs` migration helpers, merge/tests.rs
  fixture style). Desktop feature-gated tests use `--features desktop`.
- Route: delegated direct (writer trigger: 2+ non-trivial files).

## Tasks

- [x] T1 — get_tracks full-column sentinel assertion.
  `services/library/query.rs`: `get_tracks_maps_every_column_to_its_own_value` (30 positional
  fields + the `fetch_tags_for_tracks` closure via a seeded tag), `get_tracks_filters_bind_
  placeholders_to_the_right_slots` (search/key/bpm-range/tag/combined arms).
- [x] T2 — playlist/tracks.rs + smart.rs reads.
  `services/playlist/tracks.rs`: `get_playlist_tracks_maps_every_column_to_its_own_value`
  (real path: create_playlist → add_tracks → get_playlist_tracks).
  `services/playlist/smart.rs`: `get_smart_playlist_tracks_maps_every_column_to_its_own_value`
  (text-equals rule on `title`, asserts evaluated `track_count` too).
- [x] T3 — analysis.rs both queries.
  `services/analysis.rs`: `get_updated_track_maps_every_column_to_its_own_value` (public entry
  over the instance query) and `get_track_static_maps_every_column_to_its_own_value` (the
  blocking-side query — its only caller chain is `analyze_tracks_async`, which needs an
  AppHandle + real audio; the in-module test calls the containing private fn directly).
- [x] T4 — export/collection.rs both reads.
  `services/export/collection.rs`: `collect_tracks_for_export_maps_every_column_to_its_own_value`
  — one plain + one smart playlist through `collect_tracks_for_export`, exercising BOTH
  positional closures; numeric bpm rule covers placeholder binding.
- [x] T5 — backup export+restore roundtrip with url.
  `services/backup.rs`: `backup_roundtrip_keeps_every_track_column` — seed → create_backup_data
  (26-col positional SELECT, `url` at index 25) → restore into a fresh migrated DB → re-export;
  field-by-field BackupTrack equality both directions + direct `SELECT url` check.
- [x] T6 — update.rs dynamic update paths (set + unchanged url).
  `services/library/update.rs`: `update_track_sets_only_the_provided_columns` (slot-counter
  drift would move values across columns; full-equality of returned AND re-read track) and
  `update_track_with_none_leaves_url_and_bulk_update_targets_all_rows` (TrackUpdate `None` =
  UNCHANGED semantics — no clear path exists in update_track; clearing is `update_track_metadata`,
  already covered elsewhere; bulk `WHERE id IN` placeholder tail).
- [x] T7 — upsert_cue merge regression test.
  `cloud_sync/pipeline/merge/tests.rs`: `cue_merge_insert_update_delete_exercises_the_cue_upsert`
  — live insert, ON-CONFLICT update (the exact clause that carried the stray `url=excluded.url`),
  older-remote-ignored HLC guard, tombstone delete + tombstone record, tracks-row-untouched
  invariant. PROVEN: re-injecting the historical stray column fails this test with
  `no such column: excluded.url` at prepare time.
- [x] T8 — Gates (see Verification evidence; all four commands run against the final tree).
- [x] T9 — Work-unit commit on `dev` (user's cached branch decision). Over-budget (898 lines): user
  chose single commit (tests-only, cfg(test)-gated, zero runtime risk). SHA below. NOT executed — writer was
  contractually forbidden from committing.
- [x] T10 — RDD: skipped by policy (mode off clone-local); gates are the proof.

## Design notes

- Shared fixture in `src-tauri/src/test_utils.rs` (the repo's established `#[path]`-included
  harness): `sentinel_track()` (every column a pairwise-unique value — self-tested by
  `sentinel_track_values_are_pairwise_distinct`), `insert_sentinel_track()` (named-column
  INSERT + library-root parent), `assert_track_eq()` / `assert_backup_track_eq()` via an
  `assert_fields_eq!` macro naming the drifted field on failure, `backup_track_of()`.
- 11 new real tests; test count moved 296 → 350 because each `#[path]` include re-runs the
  harness's own tests (established pattern: the 4 pre-existing includes already did this).

## Verification evidence

Final-tree runs (branch `dev`, dirty worktree, `src-tauri/` unless noted):

- `cargo test --features desktop` → `test result: ok. 350 passed; 0 failed; 4 ignored`
  (baseline before work: 296 passed; 0 failed; 4 ignored).
- `cargo clippy --features desktop -- -D warnings` (exact CI form) → exit 0, clean.
- `cargo clippy --features desktop --tests -- -D warnings` → exit 101 with **5 errors, same
  count and classes as the pre-change baseline** (2× `io_other_error` in `metadata_update.rs`
  tests, 2× deref lint in `file_tags.rs` tests, 1× crate-global `duplicate_mod` diagnostic for
  the `#[path] test_utils` pattern). All my include sites carry `#[allow(clippy::duplicate_mod)]`;
  the diagnostic is emitted crate-globally and cannot be site-suppressed. Net-new test-code
  lints: 0. The repo gate (CI form, no `--tests`) is clean.
- `cargo fmt --check` → diffs only in `build.rs`, `src/main.rs`,
  `src/services/library/metadata_update.rs` = exact baseline parity. No blanket `cargo fmt`
  was run; new code was formatted with targeted `rustfmt --edition 2021 <file>` (verified
  equivalent to the cargo-fmt config by probe before use).
- `yarn check:cargo` (repo root) → exit 0, `Finished release profile`.
- Mobile gate (`cargo check --target aarch64-apple-ios --no-default-features --features
  mobile`): NOT RUNNABLE here — the cross target is not installed (deps fail before
  `crate-app`). Every change is `#[cfg(test)]`-only and excluded from non-test mobile builds.
- Anti-regression proof: temporarily re-injected (a) `url=excluded.url` into `upsert_cue` and
  (b) a `get(29)→get(2)` url index drift in `get_tracks` → exactly the new tests failed
  (`no such column: excluded.url`; url-mismatch asserts). Both mutations reverted byte-identically
  (`git diff` clean on `writers.rs`); final suite re-run green.

## Discovered bugs

None in current code — every positional site maps correctly today. The tests are prevention,
not correction. (One writer-side slip during authoring — the seed helper briefly bound `url`
into the `artwork_path` slot — was caught by the harness self-test and fixed before landing,
demonstrating the helper's own drift-detection.)

## Line budget

898 insertions / 1 deletion across 9 files — ABOVE the ~600 target and the forecast (250–400).
The overage is irreducible fixture data (sentinel values ×30, two named-field lists ×30/×26,
the 30-column seed INSERT) plus the harness self-tests that keep the `#[path]`-copy pattern
lint-clean. All additions are tests/test-infrastructure; zero source-behavior changes. If this
goes upstream as a PR, `odd`'s ~400-line review guidance suggests slicing: (1) test_utils
harness + query/playlist, (2) analysis/export/backup/update, (3) merge cue anti-regression.
