# ODD task: Automatic tagger — matcher / scoring (backend)

Status: IMPLEMENTED — native review approved and acknowledged
Branch: `dev`
Feature name: `auto-tagger-matcher`
Created: 2026-09-29

## Objective

Port Harmony's unified candidate scoring to Crate and expose **the best 5 candidates ranked by
similarity** so the user can pick the correct one. Backend only, same shape as the previous slice.

The predecessor slice (`odd/tasks/auto-tagger-providers.md`) is CLOSED and its own close note says:
*"Deferred to the next slice: matching/scoring and per-ID `extend`."* This is the matching/scoring
half of that deferral. `extend` stays out of scope.

## Problem / Why

Crate can already ask Beatport / TraxSource / Bandcamp for candidate tracks, but the command returns
them in raw provider order with no notion of which one actually matches the local file. A DJ library
has near-duplicates everywhere — mix variants, remasters, re-releases, wrong-artist edits — so raw
provider order is not a decision aid. The user needs a short, ordered shortlist.

Reference implementation: Harmony's `src/main/lib/tagger/scoring/` (`normalizeString`,
`levenshteinSimilarity`, `hybridTextSimilarity`, `calculateDurationScore`, `UnifiedScorer`) and
`src/main/lib/tagger/providers/orchestrator.ts` (`scoreAndRank`: filter → sort → provider tie-break →
top N). Ported faithfully, with two explicit adaptations (see Design contract).

## Scope

In scope (backend only):
- `services/tagger/scoring.rs`: the pure algorithm (normalization, Levenshtein, hybrid text
  similarity, duration score, weighted scorer, rank/filter/sort/tie-break).
- Models for a scored candidate, a provider error, and the ranked result.
- `TaggerService::search_and_rank`: search all providers, score, rank, return top N + provider errors.
- One new Tauri command, registered in `generate_handler!`.
- Hermetic unit tests for the algorithm and the ranking.

Out of scope:
- Frontend (selection UI / modal), `shared/api` wrapper, any `.svelte` change. Next slice.
- Applying the chosen metadata to tracks or files; album-art download.
- The per-ID `extend` enrichment step (Beatport release detail, Bandcamp track page).
- New Cargo dependencies. The Levenshtein implementation is hand-rolled, matching Harmony.
- Changing or removing the existing `search_track_tags` command or `ProviderSearchResult`.

## Harmony source (ported logic, with line-level provenance)

From `src/main/lib/tagger/scoring/utils.ts` and `scorer.ts`, plus `providers/orchestrator.ts`:

1. `normalizeString(s)`: lowercase; drop everything outside `[\w\s]`; collapse whitespace; trim.
2. `levenshteinSimilarity(a, b)`: `1.0` if `a === b`; `0.0` if either is empty; else
   `1 - distance / max(len(a), len(b))`.
3. `hybridTextSimilarity(query, candidate)`: normalize both; `1.0` on exact equality; tokenize on
   whitespace; **for each query word take the max Levenshtein similarity over candidate words**;
   return the mean of those maxima. Asymmetric on purpose — extra candidate words ("Title (Original
   Mix)") do not penalize a shorter local title, and word reordering still scores ~1.0.
4. `calculateDurationScore(local, remote)`: falsy on either side → `0.7` (neutral); `|diff| <= 5`
   → `1.0`; `<= 15` → `0.8`; `<= 30` → `0.5`; else `0.2`. Thresholds are **seconds** there.
5. `UnifiedScorer`: weights `title 0.5 / artist 0.3 / duration 0.2`, constructor throws unless the
   sum is `1.0 ± 0.001`; `calculate` returns the weighted sum.
6. `scoreAndRank`: score every raw candidate; `filter(score >= minScore)`; sort descending; when two
   scores differ by `<= 0.01` prefer the provider with the lower `providerPriority` index; `slice(0,
   maxCandidates)`. Harmony's defaults: `minScore 0.3`, `maxCandidates 4`.

## Design contract (pin these names)

### `src-tauri/src/models/tagger.rs` (additive)

```rust
/// A candidate plus its similarity score against the local track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredTagCandidate {
  /// Flattened: the wire shape is the candidate's own fields plus `similarity_score`.
  #[serde(flatten)]
  pub candidate: TagCandidate,
  /// Weighted similarity, 0.0..=1.0 (title 0.5, artist 0.3, duration 0.2).
  pub similarity_score: f64,
}

/// A provider that failed during a ranked search; the others still contributed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderError {
  pub provider: String,
  pub error: String,
}

/// Best-N candidates for the user to choose from, plus any provider failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedSearchResult {
  pub candidates: Vec<ScoredTagCandidate>,
  pub errors: Vec<ProviderError>,
}
```

Re-export all three from `models/mod.rs`. Do **not** add `similarity_score` to `TagCandidate` —
that model stays a pure provider-search DTO.

### `src-tauri/src/services/tagger/scoring.rs` (new, `pub(crate)`)

```rust
pub(crate) struct ScoringWeights { pub title: f64, pub artist: f64, pub duration: f64 }
pub(crate) const DEFAULT_WEIGHTS: ScoringWeights; // 0.5 / 0.3 / 0.2
const TIE_TOLERANCE: f64 = 0.01;

pub(crate) fn normalize_string(s: &str) -> String;
pub(crate) fn levenshtein_similarity(a: &str, b: &str) -> f64;
pub(crate) fn hybrid_text_similarity(query: &str, candidate: &str) -> f64;

/// Harmony's `calculateDurationScore`, thresholds expressed in milliseconds:
/// 5_000 / 15_000 / 30_000. `candidate_ms == None` or `local_ms == 0` -> 0.7.
pub(crate) fn duration_score(local_ms: i64, candidate_ms: Option<i64>) -> f64;

pub(crate) struct UnifiedScorer { /* weights */ }
impl UnifiedScorer {
  pub(crate) fn new(weights: ScoringWeights) -> Result<Self>; // Err on |sum - 1.0| > 0.001
  pub(crate) fn default_weights() -> Self;                    // infallible
  pub(crate) fn score(
    &self,
    local_title: &str, local_artist: &str, local_duration_ms: i64,
    candidate_title: &str, candidate_artist: &str, candidate_duration_ms: Option<i64>,
  ) -> f64;
}

/// Filter `>= min_score`, sort desc, tie-break within `TIE_TOLERANCE` by
/// `provider_priority` index, truncate to `max_candidates`. Stable sort, so equal
/// scores from the same provider keep their input (provider search) order.
pub(crate) fn rank_candidates(
  candidates: Vec<ScoredTagCandidate>,
  provider_priority: &[String],
  min_score: f64,
  max_candidates: usize,
) -> Vec<ScoredTagCandidate>;
```

`priority_index` helper: `usize::MAX` for a provider absent from the priority list (mirrors
Harmony's `Number.MAX_SAFE_INTEGER`).

### `src-tauri/src/services/tagger/mod.rs`

- `mod scoring;` (private module, sibling of `bandcamp` / `beatport`).
- `fn provider_priority(&self) -> Vec<String>` — provider ids in service order (this is the
  tie-break order, equivalent to Harmony's `providers.map(p => p.name)`).
- `pub async fn search_and_rank(&self, query: &TagSearchQuery, local_duration_ms: Option<i64>,
  search_limit: usize, min_score: f64, max_candidates: usize) -> Result<RankedSearchResult>`:
  call `search_all`, collect each `ProviderSearchResult::error` into `errors`, score every candidate
  (`artists.join(", ")` as the candidate artist, exactly like Harmony's `raw.artists.join(', ')`),
  then `rank_candidates`.

### `src-tauri/src/commands/tagger.rs`

```rust
#[tauri::command]
pub async fn search_ranked_track_tags(
  artist: Option<String>,
  title: String,
  duration_ms: Option<i64>,
  limit: Option<usize>,          // per-provider search limit; default 10, clamp 1..=25
  max_candidates: Option<usize>, // top N returned; default 5, clamp 1..=20
  min_score: Option<f64>,        // default 0.3 (Harmony), clamp 0.0..=1.0
  tagger: State<'_, TaggerService>,
) -> Result<RankedSearchResult>;
```

Keep `search_track_tags` untouched. Register the new command in `lib.rs` immediately after it, in the
same `// Tagger commands (shared, not feature-gated)` block.

## Adaptations vs Harmony (deliberate, documented divergences)

1. **Duration units.** Crate's local `Track.duration_ms` is `i64` **milliseconds** and candidates
   carry `Option<i64>` **milliseconds** (Bandcamp is always `None`). Harmony works in seconds, so the
   thresholds are ported 1:1 as `5_000 / 15_000 / 30_000` ms. No conversion, no float.
2. **Unicode-aware normalization.** Harmony's `[^\w\s]` is JS ASCII `\w`, which erases accented and
   non-Latin text entirely — a Cyrillic or CJK artist normalizes to `""` and can never match. Crate
   keeps any `char::is_alphanumeric()` character and turns everything else into a word separator, so
   accents and non-Latin scripts survive. Equal-score behavior on ASCII input is unchanged.
3. **`max_candidates` default is 5, not 4** — the product requirement is "best 5 for the user to
   pick".
4. **`min_score` stays a parameter** (default 0.3, Harmony's value). A caller that wants a shortlist
   even when nothing scores well can pass `0.0`; the default keeps obvious garbage out.
5. **The tie-break is not a comparator (post-implementation correction).** Harmony puts the
   `<= 0.01` tolerance *inside* its `Array.sort` comparator. JavaScript tolerates (and ignores) an
   inconsistent comparator; Rust requires a total order and does not guarantee one. A
   tolerance-based comparison is provably **not transitive** — reproduced with the pinned toolchain
   `nightly-2026-02-19`: scores `0.99` (priority 0), `0.995` (priority 1), `1.0` (priority 2) give
   `a < b` (tolerance, priority), `b < c` (tolerance, priority) and `c < a`
   (`1.0 - 0.99 = 0.010000000000000009 > 0.01`, so score decides) — a cycle. A 1,000,000-element
   `sort_by` / `sort_unstable_by` with that comparator did **not** panic on this toolchain, so this
   is not a demonstrated crash; but the resulting order is unspecified by contract, and the ordering
   of near-tied candidates is exactly what this feature promises. `rank_candidates` therefore sorts
   by score first (a total order) and then orders each maximal run whose members are within
   `TIE_TOLERANCE` of the run's leader by provider priority. Same intent, deterministic, no
   inconsistent comparator. Documented in the function's doc comment; covered by a regression test
   that pins the cycle case.

## TDD resolution

- Mode: **standard (not strict TDD)**. Source: no `sdd-init/crate` testing-capabilities entry with
  `strict_tdd: true` exists in Engram, and the session has no explicit TDD choice.
- Runner: `cargo test --features desktop` (required — `default = []`).
- Tests are still mandatory for the algorithm: hermetic, offline, no network.

## Acceptance criteria

1. `normalize_string`, `levenshtein_similarity`, `hybrid_text_similarity`, `duration_score` and
   `UnifiedScorer` behave as specified in Harmony, proven by hermetic unit tests.
2. Reordered words score ~1.0 (`hybrid("strobe deadmau5", "deadmau5 strobe")`), extra candidate
   words do not penalize (`hybrid("strobe", "strobe (original mix)") == 1.0`).
3. `duration_score` returns `0.7` when the candidate duration is `None` and when the local duration
   is `0`; `1.0 / 0.8 / 0.5 / 0.2` at the ms thresholds.
4. `UnifiedScorer::new` rejects weights whose sum is off by more than `0.001`.
5. `rank_candidates` filters below `min_score`, sorts descending, breaks ties within `0.01` by
   provider priority, and truncates to `max_candidates`.
6. `search_ranked_track_tags` returns at most `max_candidates` (default 5) candidates ranked by
   `similarity_score`, plus one `ProviderError` per failed provider, without failing the whole call.
7. `ScoredTagCandidate` serializes flat: candidate fields plus `similarity_score`, snake_case.
8. Hermetic suite green; `cargo fmt --check` and `cargo clippy --features desktop -- -D warnings`
   clean; no new dependency in `Cargo.toml`; mobile-safety unchanged (no new gate, no desktop API).

## Tasks

| ID | Task | Route | Trigger evidence | Checks |
| --- | --- | --- | --- | --- |
| T1 | `models/tagger.rs`: `ScoredTagCandidate`, `ProviderError`, `RankedSearchResult` + `models/mod.rs` re-export | delegated writer | part of the 2+ non-trivial-file writer batch | `cargo check --features desktop` |
| T2 | `services/tagger/scoring.rs`: normalization, Levenshtein, hybrid, duration, scorer, `rank_candidates` | delegated writer | same batch | hermetic unit tests |
| T3 | `services/tagger/mod.rs`: `mod scoring`, `provider_priority`, `search_and_rank` | delegated writer | same batch | `cargo check --features desktop` |
| T4 | `commands/tagger.rs`: `search_ranked_track_tags`; `lib.rs` registration | delegated writer | same batch | `cargo clippy --features desktop -- -D warnings` |
| T5 | `services/tagger/tests.rs`: hermetic tests for T2 + ranking + a ranked-search smoke test | delegated writer | same batch | `cargo test --features desktop` |
| T6 | `rank_candidates` tie-break correction: score-only sort, then a priority pass per near-tie group, plus a regression test pinning the non-transitive case | delegated writer | parent structural readback found a non-transitive comparator (evidence in Adaptations #5) | `cargo test --features desktop` |
| T7 | Parent verification: fmt/clippy/test + review preflight | inline (parent) | verification gate | see Verification |

All of T1–T5 are one coherent slice with one writer (routing: 2+ non-trivial files -> one bounded
writer). Reading that prepared the write was done before T1.

## Delivery

- Forecast: ~420–480 authored changed lines across 7 files (one new module, additive models).
  Above the ~400 review heuristic, so `ask-on-risk` applies.
- Repo policy (`AGENTS.md` §1.3): **no commit, no push, no PR without an explicit user instruction.**
  Delivery/chaining decisions are therefore deferred until the user asks to commit; the review
  candidate for this slice is the worktree diff.
- This is a `dev` integration-line feature, not an upstream feature branch; agent scaffolding
  (`odd/`) stays out of any future upstream PR.

## Known traps (for the writer)

- `cargo +nightly fmt` (i.e. `yarn format:rust`) uses the machine's global nightly and reformats
  three pre-existing 4-space files (`build.rs`, `src/main.rs`, `services/library/metadata_update.rs`).
  Use `cargo fmt` inside `src-tauri/` (respects `rust-toolchain.toml`) and confirm `git status`
  lists only the files this task touches.
- `default = []`: `cargo test`/`cargo clippy` need `--features desktop`, otherwise the crate trips
  its own `compile_error!`.
- `TagCandidate::artists` is `Vec<String>` but local `Track::artist` is `Option<String>` — join the
  candidate side with `", "` and pass `""` for a missing local artist.
- Don't hold anything across an `.await`; `TaggerService` is `app.manage`d state.

## Verification

```bash
cd src-tauri
cargo fmt --check
cargo clippy --features desktop -- -D warnings
cargo test --features desktop            # hermetic only (live tests are #[ignore])
```

Mobile safety (the tagger is shared, non-gated — no new gate was added):
```bash
cargo check --target aarch64-apple-ios --no-default-features --features mobile
```

## Progress log

- 2026-09-29: exploration done. Harmony scoring + orchestrator read and pinned; Crate tagger surface
  mapped (duration/artist/title shapes, IPC, tests, errors). Scope decision: backend first (user).
- 2026-09-29: task doc created.
- 2026-09-29: Engram mirror (`odd/auto-tagger-matcher/tasks`) is **PENDING** — `mem_save` returned
  "could not confirm Engram session registration" on every attempt (three, including the final
  outcome save), and `mem_session_summary` failed the same way. Engram is unavailable from this
  session; this file is the authoritative record until the mirror can be written.
- 2026-09-29: T1–T5 delivered by one bounded writer. Reported: clippy clean, `cargo test --features
  desktop` 268 passed / 0 failed / 4 ignored, only the 6 intended files changed (`models/mod.rs`
  needed no edit — it already does `pub use tagger::*`). Writer flagged a possible inconsistent
  comparator in `rank_candidates`.
- 2026-09-29: parent structural readback **confirmed** the comparator is non-transitive and
  reproduced the cycle with a standalone program on the pinned toolchain (see Adaptations #5). No
  panic observed at 1M elements, but the order is unspecified by contract. T6 opened to correct it
  before freezing the candidate.
- 2026-09-29: T6 delivered by a second bounded writer (score sort + per-near-tie-group priority
  pass + two regression tests). Parent spot-check re-ran `cargo test --features desktop`:
  270 passed / 0 failed / 4 ignored, matching the writer's report.
- 2026-09-29: native review (RDD on, medium risk, lens `review-reliability`) **approved**; authority
  burned via `review acknowledge-approved`. Findings R3-001..R3-004 are non-blocking; recorded under
  Outcome.

## Outcome (2026-09-29)

Status: **IMPLEMENTED**, reviewed and acknowledged. Backend only.

Delivered (no commit):
- `src-tauri/src/services/tagger/scoring.rs` (new, 244 lines): pure port of Harmony's scorer plus
  `rank_candidates`.
- `src-tauri/src/models/tagger.rs`: `ScoredTagCandidate` (`#[serde(flatten)]`), `ProviderError`,
  `RankedSearchResult`.
- `src-tauri/src/services/tagger/mod.rs`: `mod scoring`, `provider_priority`, `search_and_rank`.
- `src-tauri/src/commands/tagger.rs` + `src-tauri/src/lib.rs`: `search_ranked_track_tags` registered.
- `src-tauri/src/services/tagger/tests.rs`: 19 hermetic tests, including the two regression tests
  that pin the non-transitive comparator case.

Verification (observed):
- `cargo clippy --features desktop -- -D warnings`: clean.
- `cargo test --features desktop`: **270 passed, 0 failed, 4 ignored**.
- `cargo fmt --check`: **not green in this environment**, and not because of this slice — it reports
  diffs only in three pre-existing 4-space files (`build.rs`, `src/main.rs`,
  `services/library/metadata_update.rs`) caused by this machine's global
  `~/.config/rustfmt/rustfmt.toml` (`tab_spaces = 2`). None of this slice's files are flagged and
  those three files are unchanged from HEAD.
- Mobile (`aarch64-apple-ios`) not run: the target is not installed. No new feature gate and no
  desktop-only API, so mobile safety is unchanged by construction.

Native review (RDD on):
- candidate: 6 paths (the new `scoring.rs` declared as intended untracked; this doc excluded),
  630 changed lines, projection `workspace`.
- risk `medium`; one lens (`review-reliability`); correction budget 200 lines.
- outcome: **approved**, authority burned (`review acknowledge-approved`, lineage
  `review-b2279c72e724c747`, consumed revision `sha256:01c6e894...`). No correction was opened.
- Non-blocking findings (informational; not corrections, and they must not reopen this review):
  - `R3-001` (WARNING) `src-tauri/src/services/tagger/mod.rs:115-122` — the provider-failure branch
    of `search_and_rank` has no hermetic test; there is no stub-provider seam, so the documented
    "failing provider yields a `ProviderError`, the others still count" behavior is unproved.
  - `R3-002` (WARNING) `src-tauri/src/services/tagger/scoring.rs:228-240` — the returned list is not
    guaranteed non-increasing in `similarity_score` (the tie-group pass can reorder inside a
    tolerance group; the cycle fixture itself returns `[0.995, 1.0, 0.99]`). Consumers must not
    assume a strictly score-sorted list.
  - `R3-003` (SUGGESTION) `src-tauri/src/services/tagger/mod.rs:110` — with no local artist the
    artist weight contributes `0`, so the maximum achievable score is `0.7`; a `min_score` above
    ~`0.7` silently returns nothing.
  - `R3-004` (SUGGESTION) `src-tauri/src/services/tagger/scoring.rs:232-234` — the tie boundary is a
    raw f64 `<=` comparison, so a mathematically exact `0.01` gap can fall outside tolerance through
    rounding.
  - `R3-001` and `R3-002` are the two worth acting on next; all four are follow-ups, not blockers.

Delivery:
- **No commit, no push, no PR** (repo `AGENTS.md` §1.3); the change lives in the worktree.
- Authored size: **630 lines** (383 insertions + 3 deletions across tracked files, plus the 244-line
  new file), above the ~400 review budget. When committing, chain it (a natural split: scoring core
  + ranking / models + service + command wiring / tests) or use an explicit `size:exception`.
- The review candidate excluded this tracking doc, so committing the doc later does not disturb the
  reviewed source bytes.

Next slice (unchanged): per-ID `extend` enrichment, and the frontend selection UI that consumes
`search_ranked_track_tags`.

## Follow-up — R3 findings (2026-09-29)

Reopened after the approved review, on explicit user instruction ("sigue con R3"). The review
receipt for the acknowledged target stands; this is a new candidate, so it needs its own review.

### Disposition per finding

| Finding | Severity | Disposition | Rationale |
| --- | --- | --- | --- |
| R3-001 | WARNING | **Real code fix + tests** | The provider-failure branch is unproved. Extract the pure aggregation into `rank_results(...)` so the failure path is testable without a network or a stub trait, then test both paths hermetically. |
| R3-002 | WARNING | **Documented contract + pinned test** | The list is rank-ordered, not score-monotonic — that is the deliberate point of the provider tie-break, so it must not be "fixed" away. Document it on `rank_candidates`, `RankedSearchResult` and the command, and pin it with a test that asserts a lower score *can* come first. |
| R3-003 | SUGGESTION | **Documented contract + pinned test** | A missing local artist is a new input Crate has and Harmony does not. It depresses every candidate's score equally, so ranking (what the product uses) is unaffected; only an explicit `min_score > 0.7` empties the list. Document the cap and pin it. Changing the scorer to treat a missing local signal as neutral would deviate from Harmony and is left as an explicit product option, not taken here. |
| R3-004 | SUGGESTION | **Documented boundary + pinned test** | The boundary is the computed f64 `leader - member <= 0.01`. Adding an epsilon would only move the knife edge and introduce a magic constant; the exact `0.01` gap is a measure-zero event with real Levenshtein-derived scores. Document that there is deliberately no epsilon and pin both sides of the boundary. |

### Tasks

| ID | Task | Route | Trigger evidence | Checks |
| --- | --- | --- | --- | --- |
| T8 | Extract pure `rank_results(results, query, local_duration_ms, provider_priority, min_score, max_candidates) -> RankedSearchResult` from `search_and_rank` (`services/tagger/mod.rs`) | delegated writer | 2+ non-trivial files, one coherent change | `cargo check --features desktop` |
| T9 | New hermetic tests in `services/tagger/tests.rs`: failure branch + happy path for `rank_results`; the non-monotonic ordering contract (R3-002); the missing-artist cap (R3-003); both sides of the tie boundary (R3-004) | delegated writer | same batch | `cargo test --features desktop` |
| T10 | Contract docs: ordering note on `rank_candidates` / `RankedSearchResult` / the command; missing-artist cap; explicit no-epsilon note on `TIE_TOLERANCE` | delegated writer | same batch | `cargo clippy --features desktop -- -D warnings` |
| T11 | Parent verification + new native review preflight (new candidate) | inline (parent) | verification gate | see Verification |

### Result (2026-09-29)

Delivered by one bounded writer: `rank_results` extracted out of `search_and_rank` (pure, no
providers, no I/O), five new hermetic tests, and the four contract notes.

Verification observed: `cargo clippy --features desktop -- -D warnings` clean;
`cargo test --features desktop` **275 passed / 0 failed / 4 ignored** (parent spot-check matched).
`cargo fmt --check` still reports only the three pre-existing 4-space files.

Native review #2 (new candidate `sha256:9dce855a...`, lineage `review-f607f6869cc95be0`, 6 paths,
900 changed lines, risk `medium`, lens `review-reliability`, budget 200): **approved**, authority
burned. The reviewer explicitly confirmed the first round is resolved — determinism is "addressed
rather than assumed" and the provider-failure branch is now covered by a test.

Three **new** non-blocking SUGGESTIONs (all `introduced`; separate later work, must not reopen this
review):

- `R3-001` `src-tauri/src/commands/tagger.rs:41-43` — the command's `limit` / `max_candidates` /
  `min_score` defaults and clamps are unproved by tests, and `search_and_rank` applies no clamp of
  its own, so a direct caller passing `max_candidates = 0` gets a silent empty list.
- `R3-002` `src-tauri/src/services/tagger/mod.rs:142-148` — when a `ProviderSearchResult` carries
  both an `error` and candidates, the `continue` silently drops those candidates. Not reachable
  today (`search_all` sets `candidates: Vec::new()` on its error branch), but the type permits it.
- `R3-003` `src-tauri/src/services/tagger/scoring.rs:143` — `(local_ms - candidate_ms).abs()` can
  overflow on an extreme `i64` duration from a corrupt/hostile provider payload: panic under
  overflow checks, wrap in release. `i64::abs_diff` would be total.

Assessment: `R3-003` is the one with real robustness value (untrusted network input reaching an
arithmetic panic); `R3-002` is a cheap silent-data-loss path; `R3-001` is a clamp/duplication
question. Left as follow-ups because the approved receipt explicitly scopes them out and chasing
every round's suggestions would be a loop-until-clean cycle.

## Follow-up 2 — hardening: R3-003 and R3-002 (2026-09-29)

Reopened on explicit user go-ahead ("los dos que valen"). Same rule as before: the second review's
receipt is acknowledged and history; this is a new candidate with its own review.

| ID | Task | Route | Trigger evidence | Checks |
| --- | --- | --- | --- | --- |
| T12 | R3-003: make `duration_score` total — replace `(local_ms - candidate_ms).abs()` with the non-overflowing `i64::abs_diff` and compare the `u64` gap against the thresholds. Add a test with `i64::MIN` / `i64::MAX` proving no panic and a bounded score. | delegated writer | 2+ non-trivial files, one coherent hardening | `cargo test --features desktop` |
| T13 | R3-002: in `rank_results`, stop dropping candidates that a result carries alongside an `error` — record the `ProviderError` and still score the candidates. Add a hermetic test for the error-plus-candidates case. | delegated writer | same batch | `cargo test --features desktop` |
| T14 | R3-001 (documentation only, no behavior change): state on `search_and_rank` that `max_candidates` is used as-is, so callers must pass `>= 1`. | delegated writer | same batch | `cargo clippy --features desktop -- -D warnings` |
| T15 | Parent verification + new native review preflight | inline (parent) | verification gate | see Verification |

Out of scope, deliberately: clamping inside `search_and_rank` or moving the clamp constants
(a design change with duplication implications, not a defect).

### Result (2026-09-29)

Delivered: `duration_score` now uses `i64::abs_diff` (total for any `i64` pair, no overflow on
extreme untrusted durations), `rank_results` no longer drops candidates that arrive alongside a
provider error, and `search_and_rank` documents its `max_candidates` precondition.

Verification: `cargo clippy --features desktop -- -D warnings` clean;
`cargo test --features desktop` **277 passed / 0 failed / 4 ignored** (parent spot-check matched).

Native review #3 (lineage `review-8e0c119632268e96`, 6 paths, 944 changed lines, risk `medium`,
lens `review-reliability`): **approved**, authority burned. The reviewer confirmed both hardening
changes in its evidence (`abs_diff` "total for i64::MIN/i64::MAX, no overflow panic"; the
partial-failure path "records a ProviderError per failing provider yet still scores candidates
returned alongside that error; this is proved by tests").

It also caught a **real defect this pass introduced**:

- `R3-partial-failure-doc` (WARNING) `src-tauri/src/services/tagger/mod.rs:97` — T13 updated
  `rank_results`' doc but left `search_and_rank`'s doc claiming a failing provider contributes
  "instead of candidates", contradicting the new behavior and its test. Fixed inline as a
  documentation-comment-only edit (the review contract exempts a trivial passive documentation-only
  edit); `cargo check --features desktop` clean afterwards and no file of this slice is flagged by
  `cargo fmt --check`.

Two further non-blocking SUGGESTIONs, left as later work:

- `R3-coverage-wrappers` `src-tauri/src/commands/tagger.rs:41-43` — the command's defaults/clamps
  and the `search_and_rank` wrapper are still unproved by tests (the suite covers the pure
  `rank_results` / `rank_candidates` and re-threads the values by hand).
- `R3-both-empty-similarity` `src-tauri/src/services/tagger/scoring.rs:106-108` —
  `hybrid_text_similarity` returns `1.0` when BOTH sides normalize to empty, because the equality
  short-circuit precedes the empty-side check. That is a faithful port of Harmony's ordering, not a
  porting slip, but it means the documented "empty side is zero" rule does not cover both-empty.

### Stop decision (2026-09-29)

Three consecutive review cycles each produced ~3 fresh non-blocking findings while every prior round
was verifiably resolved. That is the loop-until-clean pattern the review contract warns against, and
the receipts explicitly scope findings out as separate later work. The review chain stops here.
Remaining findings are recorded, not chased; `R3-coverage-wrappers` is the one with real value if
this feature is picked up again.

## Orchestration gotcha (2026-09-29)

The OpenCode reviewer Task must receive **only** the provider-issued
`provider_task.prompt` (here a 451-character `GENTLE_AI_REVIEW_BINDING {...}` line) — nothing else.
The live transport hook materializes the patches/instruction/schema from that binding. Prefixing or
reconstructing the materialized pack makes the transport refuse with
`opencode_reviewer_result_refused`; the recovery is an exact-lineage `review status`, which reoffers
the same slot, then a clean relaunch with the untouched prompt.
