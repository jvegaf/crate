# WOAR URL frame fix — tagger apply blocked since b681257

## Objective
Restore `update_track_metadata` (and therefore the whole "find track tags" apply flow) by
writing the store URL into ID3v2 `WOAR` the way lofty requires, with a real-file regression test.

## Problem (diagnosed with the new logging, 2026-10-01 20:09Z)
`update_track_metadata failed ... Audio format does not support the requested metadata
write: ID3v2: Attempted to write an invalid frame. ID: "WOAR", Value: "Text"` for every track
whose candidate carries a URL (i.e. every Beatport match) — 10/10 auto-apply failures.

Root cause: lofty 0.22.4 treats `ItemKey::TrackArtistUrl` (WOAR) as a **URL frame** that must
carry `ItemValue::Locator`. The feature added in `b681257` wrote it with `insert_text`
(`ItemValue::Text`), which is accepted in memory but rejected at `save_to_path`. Every test
was in-memory, so the whole class passed while the real write path was broken.
Regression window: `feat(track): add store url property persisted via WOAR tag and openable in
browser` (b681257), shipped today.

Secondary defect found by the same reading: `import.rs::extract_url` reads WOAR via
`get_string`, which only matches `ItemValue::Text` — so Crate never reads back a
correctly-written WOAR (e.g. files tagged by Picard/MusicBrainz), silently losing the URL on
import.

## Root cause file:line evidence
- Write: `src-tauri/src/services/file_tags.rs:166-169` (write_track_meta) and
  `Self::apply_text_field(tag, ItemKey::TrackArtistUrl, ...)` (~:212, write_metadata_patch)
- Verify reads Text only: `verify_text_field(..., TrackArtistUrl, ...)` (~:293)
- Import read: `src-tauri/src/services/library/import.rs:473-477`
- lofty semantics: `lofty-0.22.4/src/tag/mod.rs` — `insert` validates the key↔TagType mapping
  (returns false when unsupported); `ItemValue::Locator` is the URL kind ("special treatment in
  ID3v2 and APE, written as a normal string in other tags"); `get_string` matches Text only.

## Scope
In: the two Rust services above + their tests + a committed tiny MP3 fixture for the e2e test.
Out: TS/frontend (describeError stays), DB layer, i18n, command surface, no new dependencies.

## Design
- `file_tags.rs`:
  - `fn url_item_value(tag: Option<&Tag>) -> Option<&str>` — TrackArtistUrl value matching
    `ItemValue::Locator(s)` OR `ItemValue::Text(s)` (Text fallback: non-ID3 containers store the
    Locator as plain text).
  - `fn apply_url_field(tag, &MetadataField<String>) -> Result<()>` — Set → `tag.insert(
    TagItem::new(ItemKey::TrackArtistUrl, ItemValue::Locator(v)))`; insert==false → `CrateError::
    FileTags("Audio tag format does not support metadata key TrackArtistUrl")` (same shape as
    apply_text_field); Clear → `remove_key`.
  - `fn verify_url_field(tag: Option<&Tag>, field) -> Result<()>` using `url_item_value`, same
    Set/Clear semantics as verify_text_field (trim + blank handling preserved).
  - write_track_meta: `tag.insert(TagItem::new(..., Locator(v)))` (bool ignored, matching the
    neighboring fields' existing style).
- `import.rs`: extract_url uses the Locator|Text match + existing trim/blank-to-None rule.
- Tests:
  - Update the 3 existing in-memory WOAR tests to Locator expectations.
  - NEW real-file regression: commit a ~2-4 KB silent mono MP3 fixture (generated once with
    `ffmpeg -f lavfi -i anullsrc=r=44100:cl=mono -t 0.12 -b:a 32k ...`), copy to
    `std::env::temp_dir()` with a unique name, run the full `write_metadata_patch` Set(url) →
    re-read `url_item_value` == the URL (this test FAILS on the current code — capture that RED
    evidence before the fix lands) → Clear(url) → frame removed. No tempfile crate (not a
    dependency; keep it that way).

## Acceptance criteria
1. `cargo test --features desktop` passes including the new fixture-backed save/verify test,
   which fails ("invalid frame WOAR") if reverted to `insert_text`.
2. Tagger apply with a Beatport candidate persists metadata + WOAR end-to-end (user
   reproduction of the original bug).
3. `import` reads WOAR back from files (unit test with a Locator item, not just Text).
4. Gates green: `cargo fmt --check` clean for touched files (three pre-existing out-of-scope
   diffs remain, documented by the previous writer), `cargo clippy --features desktop --
   -D warnings` exit 0. No frontend changes → TS gates unaffected.

## Tasks (route: delegated — 2 non-trivial Rust files + fixture; writer trigger fired)
- [x] T4 file_tags.rs: url helpers + write/verify switch + existing tests updated.
      DONE — `url_item_value`/`apply_url_field`/`verify_url_field` added; both write paths use
      `ItemValue::Locator`; clippy+test evidence in T7.
- [x] T5 import.rs: extract_url Locator|Text + test for the Locator case.
      DONE — `extract_url_reads_a_locator_valued_woar_item` passes.
- [x] T6 MP3 fixture + real-file regression test (capture RED-before-fix evidence).
      DONE — `src-tauri/test_assets/woar-sample.mp3` (853 B, ID3v2.4 + MPEG L3 mono);
      RED proof captured: pre-fix run of `url_patch_saves_woar_to_a_real_mp3_file` failed with
      `ID3v2: Attempted to write an invalid frame. ID: "WOAR", Value: "Text"`; post-fix green.
- [x] T7 Gates: fmt/clippy/test; report `command: observed result`.
      DONE — writer: `cargo fmt --check` zero diffs in the two touched files (3 pre-existing
      out-of-scope diffs remain: build.rs, src/main.rs, services/library/metadata_update.rs);
      `cargo clippy --features desktop -- -D warnings` exit 0; `cargo test --features desktop`
      352 passed / 0 failed / 4 ignored. Parent spot-check (filter `url`): 20 passed, incl. the
      real-file regression test.

## Verification
```
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --features desktop -- -D warnings
cd src-tauri && cargo test --features desktop
```

## TDD
Advisory RED/GREEN required ONLY for T6: the regression test must be shown failing against the
pre-fix write (`insert_text` → save error) before the Locator fix makes it green. Rest:
ordinary checks (project TDD config is OFF; carried from tagger-error-logging doc, source:
same session).

## Delivery
- Forecast ~80-140 authored lines + ~4 KB binary; single slice, under the 400-line budget.
- Strategy: ask-on-risk (inherited session choice); running count for this feature: 0.
- Commits: NONE without explicit user request (repo §1.3) — the user has two stacked uncommitted
  units now (error logging + this fix); flag the commit decision in the final report.

## Progress log
- 2026-10-01: created after the error-logging change (odd/tasks/tagger-error-logging.md, T1-T3
  done) exposed the root cause in the terminal log at 20:09Z.
- 2026-10-01 (writer ses_f06e383f): T4-T7 done; RED captured pre-fix (`WOAR, Value: "Text"` on
  real-file save), GREEN after; +188/−31 across two Rust files + 853 B fixture. RDD is disabled
  (clone_local), so gates above are the verification of record — no native receipt.

## Status
CLOSED except AC2 (user re-runs the tagger apply on a Beatport candidate) and the commit
decision — worktree holds two stacked uncommitted units (error logging + this fix) and
`src-tauri/test_assets/woar-sample.mp3` must be `git add`ed with the fix.
