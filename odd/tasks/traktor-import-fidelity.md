# ODD feature: Traktor NML import fidelity

Feature document. Mirrors the Engram recovery copy at topic `odd/traktor-import-fidelity/tasks`.

## Objective

Make Crate's existing Traktor `collection.nml` importer faithful to what the file actually says:
correct cue time units, and the per-track fields Crate currently drops.

## Problem (verified against the real export)

Evidence gathered by running the *shipped* parser/mapper over the private 264-entry
`data/collection.nml` (throwaway probe, deleted afterwards) plus direct XML inspection:

1. **Cues are read as bare milliseconds and the result is wrong.** `parser.rs:106-107` stores
   `CUE_V2 START` / `LEN` straight into `start_ms` / `length_ms`. On the real export every cue
   below 1 s collapses to position 0 or 1 ms.
   - `Paseando Por Sans (Original Mix)`: `AutoGrid START="52.496647"`, hotcue 0 at the *same*
     `52.496647`, hotcue 1 at `14883.715649`. All three are milliseconds that Crate rounds to
     **52, 52, 14884**. Two distinct hotcues land on the same position.
   - `Let the Music (Deep Swing Dub)`: `126.996841 / 126.996841 / 30603.403225` → 127, 127, 30603.
2. **Section 1's tell is the cue structure, not the magnitude break.** 12 of the 33 multi-cue
   entries mix sub-second and multi-second `START`s; the hotcue-0/beatgrid pair shares an exact
   timestamp, which is what a millisecond grid anchor + a hotcue snapped onto beat 1 looks like.
3. **Nothing is imported in practice.** `path::resolve_location` reads `VOLUME="C:"`
   (`src-tauri/src/services/traktor/path.rs:33-37`) and, on Linux/macOS, drops the drive so
   `/:Users/:josev/:Music/BOX/:2609/` becomes an absolute `/Users/josev/...`. The real export's
   files live under `C:\Users\josev\Music\BOX\2609\`, reachable from WSL at
   `/mnt/c/Users/josev/Music/BOX/2609/`. All 264 mapped paths report `exists_on_disk=false`, so
   `resolve_track_id` reports `file-not-found` for every entry and no cue, grid or key is written.
   **Even after a perfect unit fix, the import imports nothing on this machine.**
4. **Crate maps `TEMPO BPM` into `MappedTrack.bpm`** and the writer never persists it
   (`traktor_import.rs` writes only `key`, `grid_bpm`, `grid_anchor_ms`, `grid_source`), while the
   code comment claims Crate's `bpm` is its own measurement. The result is a *correctly gated*
   absence of Crate's own BPM, not a bug — see decision D4.
5. **9 importable fields are parsed or dropped:** `RANKING`, `COLOR`, `PLAYCOUNT`, `BITRATE`,
   `FILESIZE`, `RELEASE_DATE`, `IMPORT_DATE`, `PLAYTIME`, `COVERARTID`; `AUDIO_ID` is parsed and
   then unused (`parser.rs:67`).
6. **Traktor's 0-based `HOTCUE` collides with Crate's 1-based slot.** `hot_cue_index(0) == 1`, so
   Traktor hotcue slot 0 is written as Crate slot **1** and every slot shifts by one. Crate's
   `hot_cue_index` is a representation choice (0-based internally; the UI adds one:
   `HotCueButton.svelte` renders `cue.hot_cue_index + 1`), not a different musical fact.
   With 43 hot cues in the real export, this values a whole set of cue slots one position off.
   Evidence for a 0-based Crate slot exists in-tree: `anlz/tags/pco2.rs` writes Pioneer hotcues as
   `hot_cue_index - 1` with `let pioneer_slot = c.hot_cue_index - 1;`.

## Why

The importer is Crate's whole Traktor story today (there is **no** NML exporter — `services/export/`
only writes Rekordbox USB PDB/ANLZ and Device Library Plus). Getting import right is the
prerequisite for the deferred exporter slice: an exporter built on wrong units would round-trip
wrong positions back into Traktor.

## Scope

In scope (this feature document):

- S2 — cue unit decoding from the file's own consistent evidence.
- S5 — map the fields Crate can actually store, and report the ones it cannot.
- S6 — cue slot base alignment (see decision D6, the one open choice).
- S4 (Linux/macOS only) — the volume-prefix path defect, because without it S2/S5/S6 are verifiable
  only through synthetic fixtures.

Out of scope (deferred): the NML **exporter**, smartlist `QUERY` → Crate smart rules, artwork
extraction from `COVERARTID`.

## Constraints

- `services/traktor` is compiled for mobile too and must stay ungated (`mod.rs:1-8`, AGENTS.md §5.7).
- Migrations are appended, never edited (`db/schema.rs`), and the runner keys on declared effects.
- Rust is pinned to `nightly-2026-02-19`; clippy runs `-D warnings`.
- Commits go directly to `dev` (the `traktor-nml-import` branch already merged), Conventional
  Commits, no AI attribution — per the user's instruction and `AGENTS.md` §6.

## Decisions

- **D1 — units: CONFIRMED as bare milliseconds, despite every accessibility workaround I tried
  reporting "seconds".** An earlier analysis concluded the grid/anchor coordinates were seconds; a
  probe of the shipped mapper on the real file, plus the `Paseando` pair structure and
  `LEN="15000.005364"` on `collection_small.nml`, put milliseconds beyond reasonable doubt. No
  seconds→ms rescue is needed for the grid. **Scope change: this is no longer "a units heuristic";
  it is "stop rounding sub-millisecond precision away and prove the round-trip".**
- **D2 — the visible symptom is still real.** Because `round_ms` collapses `52.496647 → 52`, two
  distinct hotcues share a position and `cue identity = (position_ms, type, hot_cue_index)` can
  collapse or mis-sort. Where a duplicate *is* collapsed today (`collapsed_cue_duplicates`), the
  `Grid & Hot` fixture is the in-tree witness: `START="52.764416"` → 53 vs `START="60.100000"` → 60.
- **D3 — the real blocker is the `VOLUME="C:"` path, not the units.** Confirmed by the probe: 264
  mapped, 0 on disk. S4 (Linux/macOS volume handling) is promoted from "nice to have" to the
  actual gate on observable fidelity.
- **D4 — do not write Traktor's `TEMPO BPM` into `tracks.bpm` in this feature.** The existing gate
  is consistent: `bpm` belongs to Crate's own analysis (`analysis_source`). Writing Traktor's value
  into `tracks.bpm` would let Crate's analysis plugin silently overwrite it — the exact
  Traktor-vs-mine mix `models/track.rs:59-72` exists to prevent. Record it in the dry-run report
  instead (see S5) and leave the write for the exporter slice, which needs it.
- **D5 — persist derived analysis fields through Crate's existing path, not bespoke SQL.** Cues,
  grids and keys already go through `apply_key_and_grid` / `replace_cues_for_track`; new fields
  follow the same call, never a second UPDATE that would race the `_hlc` stamp.
- **D6 — cue slot base: RESOLVED (align with Traktor).** Traktor's `HOTCUE` is 0-based and Crate
  now stores the same 0-based slot; the importer maps `n → n` and the accepted range became
  `0..=7`. Rationale: the two on-disk cue formats Crate already writes are 0-based (Pioneer's
  `hot_cue` byte, ANLZ `pco2.rs`), so this makes the import/export round trip symmetrical and
  removes an off-by-one that silently shifted all 43 real hot cues. The cost, accepted by the user:
  previously imported Traktor cues move one slot down (slot 1 → 0) until they are re-imported.
  Evidence gathered before deciding: across the real export's 264 entries the hot slots in use are
  `{0,1,2,3,6}` (max 6), and on the fixture `{0,1}` — no source slot exceeds 7, so no valid cue is
  pushed out of range by the realignment.

## Tasks

- [ ] **T1 (S2/S1).** Keep full `f64` millisecond precision through the mapper for cue positions,
      grid anchors and loop ends: replace the lossy `round_ms` where identity/sorting depends on it,
      and prove the round-trip on the real export's `Paseando`/`Let the Music` cue sets.
      Route: inline. Files: `mapper.rs`, `tests/mod.rs`.
- [ ] **T2 (S2/S1).** Add a mixed-precision fixture entry to `collection_small.nml` (sub-second
      grid + hotcue sharing a timestamp, plus a multi-second hotcue) and assert each cue keeps its
      own position instead of collapsing.
      Route: inline. Files: `fixtures/collection_small.nml`, `tests/mod.rs`.
- [ ] **T3 (S4).** Resolve a Windows drive `VOLUME` on non-Windows hosts without silently
      discarding the path: anchor the drive to its mount (real case: `/mnt/c/...` on WSL) or report
      the entry as unresolved-by-location instead of reporting it as a missing file.
      Route: inline. Files: `path.rs`, `tests/mod.rs`, `traktor_import.rs` (reason string).
- [ ] **T4 (S5).** Parse and map the fields Crate can store: `RANKING` → `tracks.rating` (0-5,
      Traktor 0-255: `/ 51`, clamped), `PLAYCOUNT` → `tracks.play_count`, `BITRATE` → `bitrate`,
      `FILESIZE` → report only (no column), `IMPORT_DATE` → `date_added`, `LAST_PLAYED` → column
      exists, needs the same decision as `RELEASE_DATE`; `AUDIO_ID` and `COVERARTID` preserved on
      the mapped track (report + exporter input, never dropped silently).
      Route: inline for the model/parser, writer may need delegation if it lands >2 non-trivial files.
- [ ] **T5 (S5).** Report, never silently drop, the fields with no Crate column: `FILESIZE`,
      `RELEASE_DATE`, `PLAYTIME`, `COVERARTID`.
- [ ] **T6 (S3).** Extend `tests/mod.rs::real_collection` with the assertions the probe produced:
      264 tracks, 246 grids, the exact `Paseando` cue triple, 0 collided cues, and the dry-run
      field counts (224 rankings, 255 cover ids, 73 play counts).
      Route: inline.
- [x] **T7 (S6, D6 resolved).** Align Crate's `hot_cue_index` base with Traktor's 0-based slots
      (`0..=7`) across the mapper, the write path, the `cues` validator and every doc that claimed
      `1..=8`; update the affected assertions.
      Route: inline. Files: `models/cue.rs`, `services/cue.rs`, `services/traktor/mapper.rs`,
      `services/traktor_import.rs`, `tests/mod.rs`, `db/schema.rs`, `shared/types/index.ts`.
      Evidence: commit `628ffe5`; `cargo test --features desktop` → 616 passed, 0 failed, 11 ignored.

## Acceptance criteria

- `cd src-tauri && cargo test --features desktop`: all pass, including the extended
  `real_collection` assertions.
- `cargo fmt --check` and `cargo clippy --features desktop -- -D warnings`: clean.
- The real export's `Paseando Por Sans` maps three distinct, discarding-nothing cue positions.
- Re-running the import on a machine where the files are reachable (`/mnt/c/Users/josev/...`)
  writes cues, grids and keys instead of reporting 264 `file-not-found`.
- No field listed in T5 disappears without appearing in the dry-run report.

## Delivery strategy

`ask-on-risk` (default). The forecast already exceeds the ~400-line review budget once S4/T1-T6 land,
so a chain strategy will be requested before the second slice, not sprung after it.

## Progress

| Task | State | Evidence |
| --- | --- | --- |
| T1 | done | `8f7ef31` (continued in `odd/tasks/traktor-import-fidelity-remaining.md`) |
| T2 | done | `8f7ef31` |
| T3 | not started | deferred to a Windows machine — tasks, evidence baseline and the open decision are in `odd/tasks/traktor-import-t3-windows-volume.md` |
| T4 | done | `c4317b2` |
| T5 | done | `c4317b2` |
| T6 | done | `c4317b2` |
| T7 | done | `628ffe5`; 616 passed / 0 failed; rustfmt + clippy `-D warnings` + eslint + prettier + svelte-check clean |

The continued slice (T1/T2/T4/T5/T6) is tracked in
`odd/tasks/traktor-import-fidelity-remaining.md`, which carries decisions D7-D12.

Baseline captured before any edit: `cargo test --features desktop` → **616 passed, 0 failed,
11 ignored**. `cargo test --features desktop real_collection -- --ignored` → **1 passed**
(assertions: 264 tracks, 246 grids, 3 smartlists).

## Verification contract (forwarded to every writer)

- `cargo fmt --check`
- `cargo clippy --features desktop -- -D warnings`
- `cargo test --features desktop`
- `cargo test --features desktop real_collection -- --ignored --nocapture`

## Next step

T7 is done (decision **D6** resolved in favour of Traktor's 0-based slots). Implement T1 → T2 → T3
in that order next: T1/T2 are the verifiable unit fix, T3 is what makes them observable against the
real export. T3 remains the gate — on this machine 264 entries map and 0 resolve on disk.
