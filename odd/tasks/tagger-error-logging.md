# Tagger error logging — "find track tags" failures must be visible

## Objective
When applying tagger results fails, the user must be able to see WHY: the actual backend
error must reach the logs (webview console + Rust terminal log) instead of being replaced by
the generic `"Some automatic matches could not be applied ({count})"` toast.

## Problem
- `CrateError` serializes to its Display string over IPC (AGENTS.md §5.3), so `invoke` rejects
  with a **plain string**, not an `Error`.
- Every tagger catch in `shared/stores/tagger.ts` extracts `error instanceof Error ?
  error.message : <generic translation>` — the real string rejection is **discarded** and only a
  count survives in the toast (`applyAutoRows:193-215`, `applyBatch:448-458`, row error in
  `searchBatch:371-376`).
- The Rust commands on the apply path do not log failures either, so nothing appears in the
  `yarn dev` terminal log.

## Scope
In: diagnostic logging + faithful error extraction in the tagger flow (TS + the Rust commands it
invokes). Out: retry logic, behavior changes, toast redesign, i18n changes.

## Constraints
- Repo contract: no commit without explicit user request (AGENTS.md §1.3).
- Prettier: tabs, no semicolons, single quotes; Rust: nightly fmt/clippy `-D warnings`.
- Artifacts/comments in English. Both build targets must keep compiling (mobile-safe: no new
  desktop-only surface; logging is inside shared TS + already-registered commands).
- ~400 authored changed lines is a planning heuristic only, not a cap.

## Acceptance criteria
1. A failing auto-apply or batch-apply logs, per track: track id + title, failing stage
   (`extend` / `patch` / `artwork` / `search`), and the raw error (string rejections preserved
   verbatim).
2. Toast/failure entries carry the real error message when available, generic translation only
   as last resort.
3. The three Rust commands the apply flow calls (`extend_track_tag`,
   `update_track_metadata`, `set_track_artwork_from_url`) log their `Err` with the id and error
   before propagating, following existing command-log patterns.
4. All quality gates pass (see Verification).

## Tasks (route: delegated — writer trigger fired, 2+ non-trivial files across stacks)
- [x] T1 `shared/stores/tagger.ts`: `describeError(error: unknown): string` preserving Tauri
      string rejections; `console.error` with track+stage context in every tagger catch
      (`search`, `searchBatch`, `extendSelected`, `apply`, `applyAutoRows`, `applyBatch`).
      Evidence: 7 catches log `'[tagger] …' { trackId, title, stage, error }`; `stage` tracks
      extend→patch→artwork in `applyAutoRows`/`applyBatch`; `yarn check:svelte` shows zero
      tagger.ts issues (baseline-only failures).
- [x] T2 Rust: failure logging in `src-tauri/src/commands/tagger.rs` (`extend_track_tag`,
      `set_track_artwork_from_url`) and `src-tauri/src/commands/library.rs`
      (`update_track_metadata`) — log `Err` then propagate, semantics unchanged.
      Evidence: `.inspect_err(|e| log::warn!("… failed for <id>: {e}"))` per command, following
      the discovery/follow command pattern; clippy `-D warnings` + scoped fmt clean.
- [x] T3 Run all gates and record outputs: `yarn check:svelte`, `yarn lint:check`,
      `yarn format:check`, `cargo fmt --check`, `cargo clippy --features desktop -- -D warnings`,
      `cargo test --features desktop`. Evidence: command: result lines below.

## Verification
```
yarn check:svelte                       # exit 1 — pre-existing baseline ONLY: +layout.svelte:12
                                        # PUBLIC_APP_VERSION error + 2 LibraryTab.svelte:279 a11y
                                        # warnings; 0 issues in tagger.ts
yarn check:svelte:mobile                # exit 0 — 0 errors, 0 warnings (shared/ changed)
yarn lint:check                         # exit 0
yarn format:check                       # exit 0
cargo fmt --check                       # exit 1 — baseline drift ONLY in build.rs, main.rs,
                                        # services/library/metadata_update.rs (confirmed identical
                                        # on the untouched tree before the change; none of the
                                        # three touched files appears)
cargo clippy --features desktop -- -D warnings   # exit 0
cargo test --features desktop           # exit 0 — 350 passed, 0 failed, 4 ignored
```

## TDD
Resolved: OFF. Source: no explicit project/session TDD config; no tagger store tests exist and
the change is observability-only. Ordinary functional checks above apply instead.

## Delivery
- Forecast: ~100–180 authored lines → single slice, well under the 400-line review budget.
- Strategy: ask-on-risk (default); running count: 0.
- Commits: NONE without explicit user request (repo §1.3). User asked for logs, not a commit.

## Progress log
- 2026-10-01: Feature doc created. Route: delegated direct (writer). RDD state to be read after
  implementation.
- 2026-10-01: Gates green (writer + parent spot check). RDD OFF for this clone (clone_local) →
  gates are the verification of record; no native review opened.
- Committed as work unit `a56f8bd` (code) + `2d470fb` (this doc) on `dev`.
- 2026-10-01: T1+T2 implemented by delegated writer (describeError + console.error in 7 tagger
  catches with stage tracking; log::warn via inspect_err in the 3 apply-path commands). All gates
  run — see Verification. No commit (repo §1.3).

## Next step
Delegate one bounded writer for T1+T2+T3; verify; report; user decides on run + commit.
