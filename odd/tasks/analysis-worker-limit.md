# Feature: analysis-worker-limit

Goal: the analysis module must never saturate the machine. Today it launches one OS thread per
track with no ceiling, so a folder scan that imports 200+ tracks (auto-analysis is on by default)
pins every core, allocates gigabytes of decoded PCM, and freezes the desktop.

Branch/session: `library-folder-scan` is the current branch and its worktree already carries the
unreviewed `library-scan-performance` candidate. This work must stay separable from it (AGENTS.md §6,
review workload guard). `src-tauri/src/services/analysis.rs` is clean in that diff and is the only
source file this feature touches, so the two candidates never overlap.

## Diagnosis (read-only reconnaissance, 2026-09-24)

The user reports the whole machine locks up during analysis. Static reading confirms a real
oversubscription defect, not a hunch.

1. **No concurrency ceiling at all.** `AnalysisService::analyze_tracks_async`
   (`src-tauri/src/services/analysis.rs:95`) iterates every requested track id and calls
   `tauri::async_runtime::spawn` once per id, unconditionally, in a plain `for` loop.
2. **Each task lands on the blocking pool.** `analyze_single_track_task` (line 182) runs the DSP work
   under `tokio::task::spawn_blocking`. Tokio's blocking pool defaults to a 512-thread ceiling and is
   *not* sized by the number of cores, so a 200-track request can run ~200 threads on a 16-core host.
   That pool is also global, shared with `services/backup.rs` — resizing it is not the right knob.
3. **Each thread holds a full decoded track.** `decode_audio_with_cancellation` (line 342) pushes
   *every* sample into one `Vec<f32>` before `stratum_dsp::analyze_audio` runs. A 5-minute track at
   44.1 kHz is ~53 MB resident; 200 concurrent tasks is 10 GB+ of PCM before FFT/HMM intermediates.
4. **The trigger path is the default one.** `AppSettings::auto_analyze_on_import` defaults to `true`
   (`src-tauri/src/models/settings.rs:411`) and `libraryStore.scanMusicLibraryFolder()` calls
   `analysisStore.analyzeTracks(result.imported_track_ids)` with the whole import batch at once
   (`apps/desktop/src/lib/stores/library.ts:155`).

Combined: 200 threads fighting for 16 cores, ~10 GB resident, with the SQLCipher mutex serializing the
per-track writes. That is the freeze.

### Deliberately out of scope (found during reconnaissance, do not fix here)

- Streaming decode instead of decoding the whole file into one buffer. It would cut peak memory
  several-fold, but it rewrites `decode_audio_with_cancellation` and its cancel-check cadence, and it
  changes what `stratum_dsp::analyze_audio` is fed. Separate diagnosis.
- The unbounded `tasks` map: `analyze_tracks_async` inserts the task **after** spawning it, so a task
  that finishes first (or one cancelled mid-flight) leaks its entry. Pre-existing, orthogonal to the
  ceiling, and fixing it means restructuring the spawn/insert handshake.
- Emitting a `Pending` event for every track up-front. Kept as-is: it is honest (they *are* queued) and
  the frontend already renders `pending` from its own optimistic update.

## Design (fixed before implementation)

One slice, backend only, one file plus tests.

- **The ceiling.** A `tokio::sync::Semaphore` owned by the service:

  ```rust
  const ANALYSIS_WORKER_LIMIT_MAX: usize = 4;

  fn analysis_worker_limit() -> usize {
      std::thread::available_parallelism()
          .map(|n| (n.get() / 2).clamp(1, ANALYSIS_WORKER_LIMIT_MAX))
          .unwrap_or(2)
  }
  ```

  Half the cores capped at 4: 4 workers on a 16-core host, 2 on a 4-core laptop, never 0, and the
  `unwrap_or(2)` keeps a host that refuses to report parallelism from getting an unbounded pool.
  Derived rather than user-facing so the change stays one file.
- **Where the permit is taken.** Inside `analyze_single_track_task`, *after* the early cancel check and
  *before* the `Analyzing` emit, held across the `spawn_blocking` await. Taking it in
  `analyze_tracks_async` would make the command await its own queue and would register cancel tokens
  only after the fact, breaking `cancel_track_analysis` for queued tracks.
- **Cancellation while queued.** The wait is `tokio::select!` with `biased;` on the cancel token, so a
  queued track that is cancelled emits `Cancelled` immediately instead of waiting for a slot it will
  then throw away. The frontend already treats `cancelled` as terminal
  (`apps/desktop/src/lib/stores/analysis.ts:50`).
- **`Analyzing` now means analyzing.** Because the emit moves after the acquire, queued tracks stay
  `pending`. That is the correct semantic and a direct UX improvement for large batches.
- **No new command, no `generate_handler!` entry, no model, no settings, no i18n, no migration.**

## Tasks

- [x] T1 Backend: add `analysis_worker_limit()`, the `slots: Arc<Semaphore>` field on `AnalysisService`, and thread it into the per-track task
- [x] T2 Backend: make `analyze_single_track_task` acquire a slot with cancellation-aware waiting, and move the `Analyzing` emit behind it
- [x] T3 Backend: tests — the clamp bounds, the service handing out exactly the limit, the wait actually blocking, and a cancelled track not consuming a slot
- [x] T4 Verify: `cargo fmt --check -- --config tab_spaces=4`, `cargo clippy --features desktop -- -D warnings`, `cargo test --features desktop`

## Constraints

- `#[cfg(feature = "mobile")]` appears nowhere in the source; this change must not introduce it.
  `tokio` is already a non-optional dependency (`tokio = { version = "1.48.0", features = ["full"] }`),
  so `Semaphore` and `#[tokio::test]` need no feature work.
- Never hold the database `MutexGuard` across an `.await` (AGENTS.md §5.5). The semaphore permit is a
  different object and *must* be held across the `spawn_blocking` await — that is the mechanism.
- `AnalysisService::new`'s signature stays; `lib.rs:403` and `app.manage` are untouched.
- No bare `cargo fmt`: `~/.config/rustfmt/rustfmt.toml` sets `tab_spaces = 2` and there is no committed
  `rustfmt.toml`, so a bare run silently reindents the file away from the repo's 4-space style. This
  trap already cost the previous feature a correction; always pass `--config tab_spaces=4`.
- No commit: AGENTS.md §1.3 forbids committing without an explicit request from the user, which
  overrides the ODD per-task work-unit commit. The change is left in the worktree and offered.
- Artifacts in English; conversation in Rioplatense Spanish.

## Evidence log

### Reconnaissance (read-only, 2026-09-24)

Read `services/analysis.rs` in full, `commands/analysis.rs`, `services/mod.rs`, `lib.rs:286-457`,
`models/settings.rs:374-430`, `apps/desktop/src/lib/stores/analysis.ts`, `apps/desktop/src/lib/stores/library.ts`,
`apps/desktop/src/lib/components/layout/OrchestratorLayer.svelte`. Confirmed zero occurrences of
`Semaphore`, `buffer_unordered`, `concurrency`, or `join_all` anywhere under `src-tauri/src`, and
confirmed no tokio runtime builder anywhere in `main.rs`/`lib.rs` that could be limiting the blocking
pool. Host: 16 cores, 31 GB RAM. `stratum-dsp` 1.0.0 depends on `rayon` but uses it only in its
`examples/`, so the oversubscription is entirely ours.

### Implementation (2026-09-24)

Delivered in the working tree in 1 source file: `src-tauri/src/services/analysis.rs`
(**176 insertions / 27 deletions**, of which ~130 are the new test module). Plus this task document.

What landed, beyond the design above:

- **`impl Clone for AnalysisService` had to be updated.** It is a hand-written impl listing fields
  explicitly, so the new `slots` field would not have compiled until it was added. Missing it a
  different way — cloning a *fresh* semaphore — would have silently multiplied the ceiling by the
  number of clones, which is exactly the defect this feature exists to remove. `clones_share_one_slot_pool`
  is the regression guard for that.
- **The repeated `Cancelled` emit was extracted** into `AnalysisService::emit_cancelled`. The acquire
  path added a third copy of a 15-line event-plus-cleanup block that already existed twice; extracting
  it made the diff smaller than copying it a third time.

| Gate | Command | Result |
| --- | --- | --- |
| rustfmt | `cargo fmt --check -- --config tab_spaces=4` | GREEN for `analysis.rs` (0 matches). Ran RED overall, but only because of files belonging to the unrelated `library-scan-performance` candidate that sit 2-space indented in this same worktree (`commands/library.rs`, `db/schema.rs`, `services/artwork.rs`, `services/library/*`). One real violation of mine was found and fixed (`assert_eq!` at line 832 needed splitting). |
| rust tests | `cargo test --features desktop` | GREEN — **188 passed, 0 failed** (183 pre-existing + 5 new) |
| cargo check | `cargo check --features desktop` | GREEN — no warnings |
| clippy | `cargo clippy --features desktop -- -D warnings` | **RED — pre-existing, base-only** (`services/device.rs:95`, `Iterator::last` on a `DoubleEndedIterator`, file untouched by this feature). `grep -c "services/analysis.rs"` = **0**. |
| mobile compile | `cargo check --target aarch64-apple-ios --no-default-features --features mobile` | **NOT RUN** — the host is Linux and the build dies in a third-party C build script (`cc-rs: failed to find tool "xcrun"`), not in Rust code. |

**Mobile safety was proved structurally instead of by running the gate**, and the proof is stronger
than the gate: `services::analysis` is `#[cfg(feature = "desktop")]` at `services/mod.rs:4`, its
re-export at `services/mod.rs:32`, `lib.rs:74`, all four command registrations
(`lib.rs:284-292`), and the `AnalysisService::new` call (`lib.rs:403`). The module is therefore not
compiled at all for mobile, and the change cannot regress that build. `tokio` is a non-optional
dependency (`features = ["full"]`), so `Semaphore` and `#[tokio::test]` needed no feature work and
no `#[cfg(feature = "mobile")]` was introduced.

**Behaviour change worth flagging to a reviewer:** the `Analyzing` event now fires *after* the slot
acquire, so tracks waiting in the queue stay `pending` instead of all reporting `Analyzing` at once.
That is the honest semantic and it is what makes the ceiling visible in the UI. `AnalysisStatus::Pending`
already existed and the frontend already renders it (`apps/desktop/src/lib/stores/analysis.ts:50`),
and `cancelled` was already a terminal state that clears the entry, so no frontend change was needed.

**Tuning knob:** `ANALYSIS_WORKER_LIMIT_MAX` is the single constant to lower if 4 concurrent analyses
still feel heavy on this host.

**Not verified:** the live end-to-end run (import 200+ tracks, watch the machine stay responsive).
There is no Rust integration harness that can drive a real `AppHandle` emit, and this session did not
launch the app. The four tests exercise the gate itself — bounds, pool sharing, real blocking, and the
cancelled fast path — not the wiring through `analyze_tracks_async`.
