# Tagger Configurable Scoring + Auto-Apply

## Summary

Enhance the track tagger to use additional metadata signals (genre, bpm, label, key) for
scoring, make weights user-configurable, add WOAR URL direct provider matching, and replace
the hardcoded auto-apply threshold with a configurable setting.

## Motivation

Tracks purchased from Beatport already carry rich metadata. When the tagger finds a candidate
from the same provider, the existing tags should produce a high confidence score (>95%),
allowing automatic application of missing tags and the WOAR URL without user intervention.

## Scope

- **In**: Scoring weights, new signals, WOAR shortcut, configurable auto-apply threshold, settings UI
- **Out**: Provider-specific weight profiles (future), per-tag-category scoring, mobile UI

## Phases

### Phase 1: Configurable Weights in Backend

**Files**: `src-tauri/src/services/tagger/scoring.rs`, `src-tauri/src/models/settings.rs`, `src-tauri/src/services/settings.rs`

- Add weight fields to `ScoringWeights` (already exists, currently 3 fields)
- Add `tagger_weights` JSON field to `AppSettings` (camelCase on wire)
- Parse in `get_settings()` with default fallback to current weights
- Thread weights through `search_and_rank` → `rank_results` → `UnifiedScorer`
- Remove `#[allow(dead_code)]` from `UnifiedScorer::new` once production uses it

### Phase 2: New Scoring Signals

**Files**: `src-tauri/src/services/tagger/scoring.rs`, `src-tauri/src/models/tagger.rs`

- Add `genre`, `label`, `key` similarity functions (bpm reuses `duration_score` pattern)
- Extend `ScoringWeights` with `genre`, `label`, `bpm`, `key` fields
- Update `UnifiedScorer::score()` to accept and score new signals
- Update `TagSearchQuery` or `score()` signature to carry local track metadata
- Update `search_ranked_track_tags` command to accept new parameters

**Signal design**:
- `genre`: text similarity (hybrid_text_similarity), weight ~0.10
- `label`: text similarity, weight ~0.10
- `bpm`: numeric similarity (like duration), weight ~0.05
- `key`: exact or fuzzy match (Camelot wheel distance?), weight ~0.05

**Default weights** (sum=1.0):
- title: 0.40 (was 0.50)
- artist: 0.25 (was 0.30)
- duration: 0.10 (was 0.20)
- genre: 0.10
- label: 0.08
- bpm: 0.04
- key: 0.03

### Phase 3: WOAR URL Direct Provider Match

**Files**: `src-tauri/src/services/tagger/mod.rs`, `src-tauri/src/commands/tagger.rs`

- Add `search_by_url(url: &str)` method to `TaggerService`
- Parse URL to identify provider (beatport.com, traxsource.com, bandcamp.com)
- Call provider's `extend` with the extracted ID
- Return as 100% confidence `ScoredTagCandidate`
- Frontend: if track has `url` set, call `search_by_url` first, skip regular search if succeeds

### Phase 4: Configurable Auto-Apply Threshold

**Files**: `src-tauri/src/models/settings.rs`, `src-tauri/src/services/settings.rs`, `shared/stores/tagger.ts`

- Add `tagger_auto_apply_threshold` to `AppSettings` (default 0.95)
- Add `tagger_auto_apply_enabled` boolean (default true)
- Frontend: read from settings store, replace hardcoded `AUTO_APPLY_MIN_SCORE`
- Thread through `searchBatch` and single-track flow

### Phase 5: Frontend Settings UI

**Files**: `apps/desktop/src/lib/components/settings/` (new section), `shared/stores/settings.ts`

- Add tagger settings section to settings page
- Weight inputs (sliders or number inputs)
- Auto-apply threshold slider (0.0 - 1.0)
- Auto-apply enable/disable toggle
- i18n keys for all labels

### Phase 6: Verification

- `cargo clippy --features desktop -- -D warnings`
- `cargo test --features desktop`
- `yarn check:svelte`
- `yarn lint:check`
- Manual test: tag a Beatport track with existing metadata → verify >95% → auto-applies

## Design Decisions

1. **Weights are per-session, not per-provider**: one set of weights for all providers
2. **Auto-apply is all-or-nothing**: >95% auto-applies everything, no partial
3. **WOAR shortcut is provider-scoped**: only works for recognized provider URLs
4. **Settings are device-local**: not synced via cloud (unless explicitly added to whitelist)

## Risks

- **Key scoring complexity**: musical key matching is non-trivial (Camelot wheel, enharmonic equivalents). May need a dedicated `key_similarity` function.
- **Weight validation**: must sum to ~1.0, need good error messages
- **Breaking change**: existing tagger tests will need updating with new weight defaults

## Tasks

- [x] T1: Add weight fields to `AppSettings` + parse in `get_settings()`
- [x] T2: Extend `ScoringWeights` with new signal fields
- [x] T3: Add `genre_score`, `label_score`, `key_score`, `bpm_score` functions
- [x] T4: Update `UnifiedScorer::score()` with new signals
- [x] T5: Thread weights through `search_and_rank` → `rank_results`
- [x] T6: Update `search_ranked_track_tags` command signature
- [x] T7: Add `search_by_url` to `TaggerService`
- [x] T8: Update frontend API wrappers
- [x] T9: Update `taggerStore` to use configurable weights/threshold
- [x] T10: Add WOAR shortcut to frontend flow
- [x] T11: Add settings UI for tagger configuration
- [x] T12: Add i18n keys
- [x] T13: Update existing tests + add new tests
- [x] T14: Verification (clippy, svelte-check, lint)

## Verification Results

- `cargo clippy --features desktop -- -D warnings`: passed
- `cargo test --features desktop`: 502 passed, 0 failed, 10 ignored
- `yarn check:svelte`: 0 errors, 0 warnings
- `yarn check:svelte:mobile`: 0 errors, 0 warnings
- `yarn lint:check`: exit 0