# ODD slice: Traktor import fidelity — T1/T2/T4/T5/T6

Continues `odd/tasks/traktor-import-fidelity.md` (T7 done in `628ffe5`, `0f00e09`).
T3 (Windows `VOLUME` drive resolution) is **explicitly out of scope** here: it will be done
on a Windows machine.

## Objective

Land the remaining fidelity work on the Traktor NML importer: full millisecond precision
through the mapper, an observable regression fixture, and the per-track `INFO` fields Crate
can store (plus explicit reporting of the ones it cannot).

## Evidence baseline (real export, 264 entries, captured before any edit)

`ENTRY` attributes seen: `MODIFIED_DATE` 264, `MODIFIED_TIME` 264, `TITLE` 264, `ARTIST` 256,
`AUDIO_ID` 246, `LOCK` 1, `LOCK_MODIFICATION_TIME` 1.

`INFO` attributes seen: `BITRATE` 264, `PLAYTIME` 264, `IMPORT_DATE` 264, `FILESIZE` 264,
`COVERARTID` 255, `RELEASE_DATE` 254, `FLAGS` 246, `GENRE` 233, `RANKING` 224,
`PLAYTIME_FLOAT` 188, `KEY` 167, `LABEL` 158, `COMMENT` 123, `PLAYCOUNT` 73,
`LAST_PLAYED` 73, `REMIXER` 10, `KEY_LYRICS` 1.

Distributions that pin the transforms:

- `RANKING` ∈ {0:1, 51:5, 102:76, 153:108, 204:19, 255:15} — a clean 0-255 scale, six steps.
- `BITRATE` ∈ {192000, 320000, 1536000, 191988, 257150, 264880, …} — **bits/sec**.
- `PLAYCOUNT` ∈ {1,2,3,4} — small positive integers.
- `IMPORT_DATE` / `LAST_PLAYED` / `RELEASE_DATE` — `YYYY/M/D` (not zero-padded).
- `PLAYTIME` ∈ seconds; `FILESIZE` ∈ KB.

## Decisions

- **D7 — `RANKING` → `rating` is `round(v / 51)`, clamped to `0..=5`.** Traktor's 0-255 scale
  maps onto Crate's 0-5 (`255 / 51 = 5` exactly, `102 / 51 = 2`, `204 / 51 = 4`). The clamp is
  defensive: the observed values all land in range, but the writer must never violate the
  `valid_rating` CHECK constraint.
- **D8 — `BITRATE` needs `/ 1000`.** `tracks.bitrate` is kbps in Crate
  (`library/import.rs:526` `pcm_bitrate_kbps`; `sync_diff.rs` compares against `320`), while
  Traktor writes bps (`320000`). Writing the raw value would make every imported track claim
  ~320 Mbps. Rounded division, `u32`-derived.
- **D9 — `IMPORT_DATE` → `date_added`, `LAST_PLAYED` → `last_played`, both parsed to RFC3339.**
  `date_added` is `TEXT NOT NULL` holding RFC3339 (`Track::new` uses
  `chrono::Utc::now().to_rfc3339()`); Traktor's `YYYY/M/D` is not RFC3339 and must not be
  written raw. A date that does not parse leaves the existing column value alone.
- **D10 — do NOT write Traktor's `TEMPO BPM` into `tracks.bpm`.** Unchanged from D4 in the
  parent document: `bpm` belongs to Crate's own analysis (`analysis_source`). Now **reported**
  instead of silently dropped.
- **D11 — persist the new fields through the existing single-mutation path.** `RANKING`,
  `PLAYCOUNT`, `BITRATE`, `IMPORT_DATE`, `LAST_PLAYED`, `GENRE`, `LABEL` extend the same
  `UPDATE tracks SET … _hlc` statement that `apply_key_and_grid` already builds (mirroring
  `library/update.rs:233`), never a second UPDATE that would race the `_hlc` stamp. Renamed to
  `apply_track_fields` since it no longer only handles key+grid.
- **D12 — fields with no Crate column are reported, never silently dropped.** `FILESIZE`,
  `RELEASE_DATE`, `PLAYTIME`, and the preserved identifiers `AUDIO_ID` / `COVERARTID` stay on
  the mapped track and are surfaced through the dry-run report. `TEMPO BPM` joins them (D10).

## Tasks

- [x] **T1** — Keep full `f64` millisecond precision in `MappedCue`/`MappedTrack`. Replace
      `round_ms` (which collapses `52.496647 → 52` and makes two distinct hot cues share a
      position) with lossless millisecond handling where identity/ordering depends on it, so
      `Paseando Por Sans`'s `52.496647` grid + hot cue pair stop colliding. Crate's persisted
      columns are `INTEGER`, so the conversion happens once, at the boundary, not inside the
      mapper's identity.
      Files: `mapper.rs`, `tests/mod.rs`.
- [x] **T2** — Add a mixed-precision fixture entry to `collection_small.nml` (sub-second grid +
      hot cue sharing one timestamp, plus a multi-second hot cue) and assert each cue keeps its
      own position instead of collapsing into one.
      Files: `fixtures/collection_small.nml`, `tests/mod.rs`.
- [x] **T4** — Parse + map the `INFO`/`ENTRY` fields Crate can store (per D7/D8/D9): `RANKING`,
      `PLAYCOUNT`, `BITRATE`, `IMPORT_DATE`, `LAST_PLAYED`, `GENRE`, `LABEL`; preserve `AUDIO_ID`
      and `COVERARTID` on the mapped track. `COMMENT` has no Crate column (see the delivery note);
      it is parsed and preserved on `NmlTrack` but not mapped onto a `tracks` column.
      Files: `traktor/model.rs`, `traktor/parser.rs`, `traktor/mapper.rs`, `traktor_import.rs`.
- [x] **T5** — Report, never silently drop, the fields with no Crate column: `FILESIZE`,
      `RELEASE_DATE`, `PLAYTIME`, `TEMPO BPM` (D10), plus the preserved identifiers `AUDIO_ID` /
      `COVERARTID`. Surfaced as `ImportReport.not_persisted` (+ `mapped_fields` for the stored
      census).
      Files: `traktor/mapper.rs`, `shared/types/index.ts`.
- [x] **T6** — Extend `tests/mod.rs::real_collection` with the probe's assertions: 264 tracks,
      264 distinct paths, 246 grids, 45/0/43/2 cue totals, the exact `Paseando` cue pair, and INFO
      field counts (224 rankings, 255 cover ids, 73 play counts, 264 bitrates).
      Files: `tests/mod.rs`.

## Acceptance criteria

- `cd src-tauri && cargo fmt --check` and `cargo clippy --features desktop -- -D warnings`: clean.
- `cargo test --features desktop`: all pass (baseline 616 passed / 0 failed / 11 ignored, plus
  the new assertions).
- `cargo test --features desktop real_collection -- --ignored --nocapture`: passes with the
  extended assertions.
- `yarn format:check`, `yarn lint:check`, `yarn check:svelte`: clean.
- Two distinct hot cues at `52.496647` survive mapping as two distinct positions.
- No field listed in T5 disappears from the dry-run report.
- No new `UPDATE tracks` statement: the new fields ride the existing single mutation (D11).

## Progress

| Task | State | Evidence |
| --- | --- | --- |
| T1 | done | `8f7ef31` — `MappedCue`/`MappedTrack` positions are `f64`; `round_ms` removed; single boundary conversion in `traktor_import::round_ms_to_column` |
| T2 | done | `8f7ef31` — fixture entry "Mixed Precision"; `mixed_precision_cues_keep_distinct_positions` |
| T4 | done | `c4317b2` — `NmlTrack` + parser + mapper + D11 write path; `info_fields_map_onto_crate_columns` |
| T5 | done | `c4317b2` — `ImportReport.not_persisted` / `mapped_fields`; mirrored in `shared/types/index.ts` |
| T6 | done | `c4317b2` — `real_collection` extended; passes on the real 264-entry file |
| T3 | out of scope | deferred to a Windows machine |

Baseline: `cargo test --features desktop` → 616 passed, 0 failed, 11 ignored.
`real_collection --ignored` → 1 passed.
After: `cargo test --features desktop` → 621 passed, 0 failed, 11 ignored.
`real_collection --ignored` → 1 passed.

Both code commits were verified green independently of each other, not just as a combined
diff: `8f7ef31` alone → 617 passed / 0 failed; `8f7ef31`+`c4317b2` → 621 passed / 0 failed.

### Delivery split

The slice exceeded the ~400-line review budget (~690 authored lines), so it was split by review
concern as agreed:

- `8f7ef31` — T1/T2, cue position precision + the regression fixture (~183 lines).
- `c4317b2` — T4/T5/T6, the per-track `INFO` fields + the report census.
- This commit — the ODD documents only.

The split could not isolate T1 from T4/T5 by hunk: `MappedTrack` changed a field's type *and*
gained twelve fields in the same struct block, so a hunk-level split would have produced commits
that do not compile. Each commit here builds and tests green on its own.

### Delivery note — `INFO COMMENT`

The brief maps `COMMENT → comment`, but Crate's `tracks` table has **no `comment` column**
(verified: `grep -in comment src/db/schema.rs` returns nothing, and no migration adds one),
`Track` has no comment field, and the UI has no comment surface. Per the brief's explicit rule
("If you believe a column is missing, STOP and report it") the parser still captures
`INFO COMMENT` on `NmlTrack` so nothing is lost at the parse layer, but it is **not** written
to a `tracks` column and is not in the D11 UPDATE. Reported here rather than inventing a column
or a migration.
