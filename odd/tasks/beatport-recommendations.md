# Beatport Recommendations (from Harmony)

## Objective

Add a "Beatport Recommendations" entry to the track context menu (visible only when the selected
track has a Beatport **track** URL) that opens a modal listing Beatport's recommended similar
tracks, replicating Harmony's recommendations screen as closely as Crate's design system allows.

## Problem / Why

Harmony (sibling repo `~/Code/harmony`) has a well-received Beatport recommendations screen.
Crate already stores the Beatport URL per track (`Track.url`, WOAR frame, migration 9), so the
feature can be ported directly.

## Verified evidence (research)

- **API** (tested live, HTTP 200): `GET https://api.beatport.com/catalog/v1/recommendations/tracks/?id={beatport_track_id}`
  with `Authorization: Bearer {token}`, `Accept: application/json`, Chrome UA.
  Response: top-level JSON array of recommendation objects.
- **Auth**: works with Crate's existing client-credentials OAuth token from
  `https://account.beatport.com/o/token/` (public embed-app credentials already in
  `src-tauri/src/services/tagger/beatport.rs`). **No Cloudflare scraping needed** — Harmony's
  `cloudscraper` + `__NEXT_DATA__` anonymous-token flow is NOT ported.
- **ID extraction**: `/beatport\.com\/track\/[^/]+\/(\d+)/` on the stored track URL (Harmony parity).
- **Harmony UI map** (target for replication): 90%-size centered modal; header has a playback
  transport cluster (prev / play-all / next) centered; body is a single-row-per-track card list:
  40px artwork (`release.image_url` with `{w}x{h}` template), title + `(mix_name)` unless "Original
  Mix", artists joined, inline `<audio>` sample (`sample_url`, rendered when present), Genre 120px,
  Label 180px, BPM 120px, Release Date 120px (`DD/MM/YYYY`), "Open in Beatport" button →
  `https://www.beatport.com/track/t/{id}`; playing card highlighted in dark amber; loading state
  (spinner + "Finding similar tracks..."); empty state (icon + "No recommendations found"); error →
  notification + modal closes. No source-track header card, no filters/sort, no pagination.

## Scope

- Rust: recommendations service reusing Beatport OAuth token flow + DTOs + IPC command +
  `generate_handler!` registration + unit tests.
- Shared: TS types, `shared/api/` wrapper, Beatport track-id extraction util (with tests).
- Desktop: `TrackContextMenu` item + wiring through `ContextMenuOrchestrator` to host view +
  `BeatportRecommendationsModal` Svelte 5 component with sample playback.
- i18n: new keys in `en.json` (and `es.json`; other locales per repo policy/fallback behavior).

Out of scope: release-URL recommendations, add-to-library from recommendations, persistence/cache
of results, mobile UI (backend stays mobile-safe).

## Constraints

- AGENTS.md: register command in `generate_handler!` (no desktop-only gate needed — reqwest runs on
  both targets like the tagger's Beatport provider); `CrateError` everywhere; never hold DB guard
  (none needed here); errors arrive at the frontend as plain strings.
- Serde: recommendation fields are snake_case — mirror in TS after checking the struct.
- Styling: Tailwind 4 `@theme` tokens, `[data-theme]`, brand `@utility` classes (no `/opacity` on
  brand, no `dark:`). Svelte 5 runes; callbacks not `on:click`; components never call `invoke`.
- Commits/push/PRs require explicit user request (§1.3) — none authorized this session.
- Review-size heuristic: keep backend and frontend as separate work units (~≤400 lines each).

## Tasks

Route for every task: **delegated** (writer trigger: 2+ non-trivial files; preparation trigger:
reading that prepares a write goes with the writer).

- [x] **T1 — Rust backend** (`route: delegated/writer`, TDD: fixture tests first) — DONE 2026-10-02.
  Verified: fmt/clippy/test green (356 pass), live `#[ignore]` test ran clean against real API,
  diff 203+/-16 in src-tauri only; parent spot check re-ran focused tests (4 passed).
  Command: `invoke('find_beatport_similar_tracks', { trackId })` → `BeatportRecommendation[]`
  (snake_case; artwork/waveform pre-resolved to 800x800; `key` long-form; errors are plain strings).
  - Extract/share the Beatport OAuth token capability so the new service and the tagger provider
    don't diverge (prefer a small shared helper over duplicated credential constants).
  - Service `find_similar_tracks(beatport_track_id) -> Vec<BeatportRecommendation...>` hitting the
    verified v1 endpoint; handle 401/429/404 as `CrateError::Tagger`-style errors.
  - DTOs in `src-tauri/src/models/` mirroring the used response fields (snake_case on the wire).
  - `#[tauri::command]` + `generate_handler!` entry.
  - Unit test: fixture JSON → DTO parsing; optional `#[ignore]` live test like `live_beatport_search`.
  - Checks: `cargo fmt --check`; `cargo clippy --features desktop -- -D warnings`;
    `cargo test --features desktop`; mobile compile check if toolchain available.
- [x] **T2 — Shared types + API wrapper + id extraction** (delegated, part of frontend writer) — DONE 2026-10-02.
  `BeatportRecommendation*` types mirror `models/recommendations.rs` (snake_case, `artists[].type` on the wire);
  `shared/utils/beatport.ts` + 12 vitest cases pass (`yarn vitest run shared/utils/beatport.test.ts`);
  `shared/api/beatport.ts` wrapper follows tagger pattern (direct import, barrel is partial by precedent).
  - `BeatportRecommendation` TS types in `shared/types/index.ts`.
  - `shared/utils/beatport.ts` `extractBeatportTrackId(url)` + vitest cases (track URL, release
    URL → null, scheme-less, invalid).
  - `shared/api/` wrapper `findBeatportSimilarTracks(trackId)` following existing pattern.
  - Checks: vitest for the util; `yarn check:svelte`.
- [x] **T3 — Context menu entry + wiring** (delegated, frontend writer) — DONE 2026-10-02.
  Evidence: `TrackContextMenu` item gated on single selection + `extractBeatportTrackId(track.url)`;
  threaded menu → `ContextMenuOrchestrator` (handler `closeAll()` +
  callback, mirroring `handleTrackEditMetadata`) → `OrchestratorLayer` →
  (parent polish: orchestrator-facing prop renamed to `onTrackBeatportRecommendations` to match the
  `onTrackX` convention; `TrackContextMenu` keeps its own unprefixed `onShowBeatportRecommendations`
  prop name like `onEditMetadata`; svelte-check + lint re-run after rename) →
  `ModalOrchestrator.openBeatportRecommendationsModal(trackId)` (`activeModal` union extended).
  `yarn lint:check` exit 0; svelte-check adds zero diagnostics vs HEAD baseline.
  - `TrackContextMenu.svelte`: show "Beatport Recommendations" for a single selected track whose
    URL matches a Beatport **track** page; new optional callback prop.
  - Propagate through `ContextMenuOrchestrator.svelte` to its host view(s); host owns modal state.
  - Checks: `yarn check:svelte`, `yarn lint:check`.
- [x] **T4 — Recommendations modal** (delegated, frontend writer) — DONE 2026-10-02.
  Evidence: `library/BeatportRecommendationsModal.svelte` (316 lines) on the shared `Modal` base (new
  additive `'3xl': max-w-[90vw]` size); fetch-on-open, loading/empty/error states, Harmony row anatomy
  (80x80 thumb swap, title+mix, artists, native `<audio controls preload="none">`, Genre/Label/BPM/Date
  columns, DD/MM/YYYY, ghost "Open in Beatport" via `openUrl`), header transport cluster with tooltips,
  `bg-brand-primary-10` playing highlight, one-audio-at-a-time via `SvelteMap` + `registerAudio` action,
  playlist advance on `ended`, skip-on-failed-play, pause+reset on close. `format:check` clean.
  - `BeatportRecommendationsModal.svelte` replicating Harmony layout with Crate tokens:
    loading / empty / error, one-row cards (artwork 40, title+mix, artists, `<audio>` sample,
    genre, label, BPM, release date, "Open in Beatport"), transport cluster in header
    (prev / play-all-pause / next), playing-card highlight, one-audio-at-a-time, advance on end.
  - Checks: `yarn check:svelte`, `yarn lint:check`, `yarn format:check`.
- [x] **T5 — i18n + final gates** — DONE 2026-10-02 (parent verified after rename: `yarn check:svelte`
  = baseline 1 error/2 warnings, zero added; `yarn lint:check` OK; vitest 12/12 re-run by parent;
  cargo gates were green post-T1 and backend untouched since).
  Evidence: 15 keys added to `en.json` (`contextMenu.beatportRecommendations` + full
  `modals.beatportRecommendations.*`) and mirrored in `es.json` (neutral Spanish). Precedent for
  en+es-only: `init({ fallbackLocale: 'en' })` in `shared/i18n/index.ts`, and live drift —
  `toast.tracksExported` missing from 10 locales, `discovery.unsupportedUrl` from 4.
  - Keys in `en.json` + `es.json` (check fallback behavior / precedent from recent features
    before deciding on the remaining 13 locales).
  - Full gates: `yarn check` + `yarn lint:check` + `yarn format:check` + cargo gates;
    structural readback of every claimed file.

## Verification (exact commands)

- `cd src-tauri && cargo fmt --check`
- `cd src-tauri && cargo clippy --features desktop -- -D warnings`
- `cd src-tauri && cargo test --features desktop`
- `yarn check:svelte`
- `yarn lint:check`
- `yarn format:check`
- vitest for `shared/utils/beatport.ts` (runner per package.json scripts)

## TDD mode

`strict_tdd` not declared active for ODD (no sdd-init finding); use ordinary functional checks with
test-first for the pure parsing/extraction units (Rust fixture test, vitest util) since both are
cheap and deterministic. Exact runner: `cargo test --features desktop` and the repo's JS test script.

## Progress / evidence

- 2026-10-02: Research done. Live API test passed (v1 recommendations 200 with client-credentials
  token). Harmony feature mapped (modal layout, row anatomy, states, deps).
- 2026-10-02: T2–T4 + T5-i18n implemented (frontend writer). Files: `shared/types/index.ts`,
  `shared/utils/beatport.ts(.test)`, `shared/api/beatport.ts`, `library/BeatportRecommendationsModal.svelte`,
  `library/index.ts`, `TrackContextMenu.svelte`, `ContextMenuOrchestrator.svelte`,
  `ModalOrchestrator.svelte`, `OrchestratorLayer.svelte`, `common/Modal.svelte` (additive `'3xl'` size),
  `en.json`/`es.json`. Checks: vitest 12/12; `yarn lint:check` exit 0; `yarn format:check` clean;
  `yarn check:svelte` = exactly the HEAD-baseline diagnostics (pre-existing `$env/static/public`
  `PUBLIC_APP_VERSION` error in `+layout.svelte:12` + 2 pre-existing `LibraryTab` a11y warnings —
  reproduced on a fully clean tree via stash; zero added by this work); `yarn check:svelte:mobile` 0/0.
  Authored diff ≈ 567 lines (modal 316 + types 68 + util/test 87 + wrapper 14 + wiring/i18n 82) — over
  the ~400 heuristic because the faithful replica's row anatomy + transport cluster is one cohesive
  unit; splitting it further would add indirection without shrinking review surface.
- (update per task: result, checks output, commit identity if the user later authorizes commits)

- 2026-10-02: Parent final verification — prop renamed to `onTrackBeatportRecommendations` at the
  orchestrator boundary (convention); `yarn check:svelte` = baseline (zero added diagnostics),
  `yarn lint:check` OK, `yarn vitest run shared/utils/beatport.test.ts` 12/12. All 5 tasks checked.
- 2026-10-02: Commits authorized by user and created on `dev` as five work units:
  `6adcee1` backend command (404+) · `ffd39e4` shared types/util/wrapper (171+) ·
  `cd05d79` modal + i18n en/es + `3xl` size (352+) · `98afbd2` context-menu entry + wiring (44+) ·
  plus this `docs(odd)` record. Every feature slice stays under the ~400-line review budget (the
  earlier single-frontend-slice overage was resolved by slicing data layer / screen / entry point);
  the four feature commits are upstream-pure (no scaffolding paths).

## Next step

Feature complete and committed. Remaining: manual UI pass in `yarn dev` against a Beatport-URL
track (webview sample playback + 80x80 artwork sizing are the only things no CLI gate can prove).
A future upstream proposal needs an issue number and a clean branch from `upstream/develop`
cherry-picking `6adcee1..98afbd2` (skipping the docs commit). RDD is off in this clone, so no
review ceremony applies unless the user re-enables it.

## Forecast / delivery

Estimated authored lines: backend ~180–250, frontend ~300–400. Strategy `ask-on-risk` (default):
the user authorized commits on 2026-10-02; delivery kept as work-unit commits on `dev` (no push or
PR authorized yet). If a PR is requested later, keep the four-slice grouping — each slice is a
PR-able unit, or promote backend+shared and screen+entry into two chained PRs.
