# Feature: tracklist-rating-editing

Goal: make the Rating column stars in the tracklist interactive — click a star to rate the track (0–5), click the
same star again to clear — without opening Track Metadata. Follows the read-only rating column from
`import-and-display-popm-rating` (which deliberately excluded editing; this feature adds it).

Branch/session: `dev`. No commit authorized — the user has not asked to commit (AGENTS.md §1.3); changes stay in the
worktree. Push/PR remain user decisions.

## Problem / Why

The rating column in `TrackRow.svelte` renders static `<span>★</span>`. Editing a rating today requires opening the
Track Metadata modal. The user wants in-place editing directly from the song list.

## Existing evidence (exploration)

- Backend is ready: `TrackUpdate.rating` → `update_track` command → `services/library/update.rs` persists rating
  (DB only; file writeback is BPM/key only). No Rust changes needed.
- `shared/api/library.ts` `updateTrack(id, update)` returns the full updated `Track`.
- Store mutation pattern to copy: `libraryStore.setTrackColors` (`apps/desktop/src/lib/stores/library.ts:231`) —
  API call → local state update → `syncStore.notifyTrackChanges`. `updateTracksInState` (line 319) refreshes `tracks`
  and `playlistTracks`.
- Controller pattern: `trackController.setColor` (`apps/desktop/src/lib/controllers/trackController.ts:373`), returned
  at the end of the factory.
- Wiring chain for color: `TrackRow(onColorChange) → TrackList(handleColorChange → onTrackColorChange) →
LibraryView/PlaylistView (pass-through) → +page.svelte:688,763 (trackController.setColor)`.
- Modal clear-on-reclick semantics to mirror: `TrackMetadataModal.svelte:473-474` — clicking the current rating sets 0.
- `TrackRow` already ignores drag-start on buttons (`handlePointerDown`, line 79: `target.closest('button,
[role="button"]')`), so rating buttons are drag-safe. Row click/dblclick (selection/play/keyboard-Enter) must be
  prevented from firing when interacting with stars.
- Test infra: root `vitest.config.ts` includes `shared/**/*.test.ts` and `apps/desktop/src/**/*.{test,spec}.{ts,svelte}`;
  run via `yarn vitest run`. Existing tests are pure TS in `shared/utils/`; no component tests exist yet — not required here.
- i18n: interactive cells inside rows hardcode English labels today (`TrackColorCell` "No color"; rating cell
  `${track.rating}/5`). Decision: aria-labels for stars stay hardcoded English to match the in-row precedent; no new
  locale keys, no 15-locale churn. Store toast matches the `setTrackColors` precedent (hardcoded string).

## Scope

In:

- `shared/utils/rating.ts` — `nextRatingSelection(current, star): number` (returns `star === current ? 0 : star`);
  re-export from `shared/utils/index.ts` barrel if that is the convention there.
- `shared/utils/rating.test.ts` — unit tests for the selection logic (test-first).
- `apps/desktop/src/lib/stores/library.ts` — `setTrackRating(trackId, rating)`: clamp 0–5, `libraryApi.updateTrack(id,
{ rating })`, `updateTracksInState([updated])`, `syncStore.notifyTrackChanges([id])`, error toast on failure
  (`console.error` + `toastStore.error('Failed to update rating')` style consistent with `setTrackColors`).
- `apps/desktop/src/lib/controllers/trackController.ts` — `setRating(trackId, rating)` passthrough, exported in the
  returned object.
- `apps/desktop/src/lib/components/library/TrackRow.svelte` — rating cell becomes 5 `<button type="button">` stars:
  - click → `onRatingChange?.(nextRatingSelection(track.rating, star))`;
  - hover preview (`$state` hoverRating, clear on leave);
  - `stopPropagation` on click, dblclick and keydown (row Enter/play, selection, double-click play must not fire);
  - accessible name per star ("Set rating to N stars" / "Clear rating");
  - optional prop `onRatingChange?: (rating: number) => void`.
- Wiring: `TrackList.svelte` (prop `onTrackRatingChange?: (trackId, rating) => void`, single-track semantics — no bulk
  to selection, keep it simple and predictable), `LibraryView.svelte`, `PlaylistView.svelte` pass-through,
  `+page.svelte` both call sites wired to `trackController.setRating`.

Out:

- No Rust/backend changes; no schema/migration.
- No bulk rating over multi-selection; no keyboard-only star widget redesign; no context-menu rating.
- No mobile changes (`TrackRow` is desktop-only; shared store/api untouched in mobile paths).
- No commit, no branch, no PR.

## Constraints

- AGENTS.md §5: components never call `invoke` directly — go through store/controller; IPC chain preserved.
- Svelte 5 runes; `$props()` typed Props; callback props (no `on:` legacy).
- Tailwind tokens (`text-warning` for filled, `text-text-tertiary/50` empty — already used).
- Formatting: Prettier (tabs, no semicolons, single quotes). ESLint a11y rules for buttons.
- Artifacts/comments in English.

## Route

Delegated direct (writer trigger: 8+ non-trivial files). One writer, foreground. TDD mode: not declared strict;
unit test written test-first for `nextRatingSelection` only (pure logic), the rest verified via gates.
Forecast ≈ 170 authored changed lines — single slice, under the ~400 planning heuristic. No RDD review cycle (no
commit authorized; review candidate is a work-unit commit).

## Tasks

- [x] T1 — `nextRatingSelection` util + unit tests (RED→GREEN) — Route: delegated.
      Checks: `yarn vitest run shared/utils/rating.test.ts` green.
- [x] T2 — `libraryStore.setTrackRating` + `trackController.setRating` — Checks: code matches `setTrackColors` pattern.
- [x] T3 — Interactive stars in `TrackRow` + wiring through `TrackList`/`LibraryView`/`PlaylistView`/`+page.svelte`.
      Checks: `yarn check:svelte` (known pre-existing failures below).
- [x] T4 — Full gates: `yarn vitest run shared`, `yarn check:svelte`, `npx eslint <touched files>`,
      `npx prettier --check <touched files>`, `git diff --check`.
      Checks: all green except known pre-existing items.
- [x] T5 — Post-user-check padding: give the rating column more horizontal padding (user confirmed live that the
      feature works, then asked for it). Rating column width 60px → 72px (`tracklistColumns.ts`), cell gets `px-1`,
      grid-template test expectation updated. Checks: scoped vitest + eslint + prettier green.
- [x] T6 — Centering (user: stars not centered in the column): rating cell uses `justify-center` (dropped the now
      redundant `px-1`), and the Rating header label is centered too (`TrackListHeader.svelte` non-sortable branch
      gets a per-column `text-center` override for `rating` only, leaving year/tags left-aligned).
      Checks: eslint + prettier + `git diff --check` green; template-class-only edits, no type surface touched.

## Known environmental failures (pre-existing on base, not caused by this work)

- `yarn check:svelte` reports `PUBLIC_APP_VERSION` error in `+layout.svelte` and two a11y warnings in
  `LibraryTab.svelte` (evidence: `odd/tasks/import-and-display-popm-rating.md` T2/T5). Only files touched by this
  feature must be clean.

## Acceptance

- Clicking star N on a row sets that track's rating to N; clicking the currently-rated star clears to 0.
- Stars reflect persisted rating immediately after the store round-trip (returned `Track` applied in state).
- Row selection, double-click-to-play, drag, and keyboard Enter do not fire when clicking/operating stars.
- Rating edits keep working in Library and Playlist views identically.
- All gates pass (except known pre-existing failures); no unrelated diff hunks.

## Progress / Evidence

- T1: `shared/utils/rating.test.ts` written first; RED — `yarn vitest run shared/utils/rating.test.ts` failed to
  resolve `./rating` (1 file failed, no tests). Implemented `shared/utils/rating.ts` + barrel export in
  `shared/utils/index.ts`; GREEN — 5 passed (5). tsconfig-paths docs/ warning is pre-existing environment noise.
- T2: `libraryStore.setTrackRating` added in `apps/desktop/src/lib/stores/library.ts` after `setTrackColors` — clamps
  `Math.max(0, Math.min(5, Math.round(rating)))`, `libraryApi.updateTrack(id, { rating })` →
  `this.updateTracksInState([updated])` (`this.` cross-method precedent at line 151) → `syncStore.notifyTrackChanges`
  → `console.error` + `toastStore.error('Failed to update rating')`. `trackController.setRating` mirrors `setColor`:
  interface entry under new "Rating operations" group + passthrough function + return-object export. Types validated
  by full `yarn check:svelte` in T4.
- T3: `TrackRow` rating cell now 5 `<button type="button">` stars — `hoverRating = $state(0)` preview
  (`onpointerenter` per star, cleared on cell `onpointerleave`), filled condition `(hoverRating || track.rating) >= star`
  (explicit parens required: `>=` binds tighter than `||`), `stopPropagation` on click/dblclick/keydown, aria-labels
  "Set rating to N stars"/"Clear rating", optional `onRatingChange` prop, cell div `role="presentation"` (a11y-safe like
  the color cell). Wiring: `TrackList.onTrackRatingChange` (single-track, no bulk), `LibraryView`/`PlaylistView`
  pass-through, `+page.svelte` both call sites → `trackController.setRating` (line 688 is the PlaylistView usage,
  763 the LibraryView one).
- T4: `yarn vitest run shared` → 3 files, 51 passed. `yarn check:svelte` → 1 error + 2 warnings, all in the known
  pre-existing set (+layout.svelte `PUBLIC_APP_VERSION`; two LibraryTab.svelte a11y warnings) — zero diagnostics on
  touched files. `npx eslint` on touched .ts and .svelte → clean. `npx prettier --check <touched files>` → all pass
  (TrackRow + doc needed one `--write` pass for 120-col wrapping). `git diff --check` → clean. Diff: 8 tracked files,
  65 insertions/2 deletions, plus 2 new shared/utils files — matches feature scope, nothing unrelated.
- T5 (inline, parent): user confirmed the feature works live and asked for more horizontal padding on the rating
  column. `shared/utils/tracklistColumns.ts` rating width 60px → 72px; `TrackRow.svelte` rating cell class gets
  `px-1`; `tracklistColumns.test.ts` default grid-template expectation updated (`1fr 60px` → `1fr 72px`).
  `yarn vitest run shared/utils/tracklistColumns.test.ts shared/utils/rating.test.ts` → 2 files, 35 passed.
  `npx prettier --check` + `npx eslint` on the 3 touched files → clean. Header/row alignment preserved automatically
  (both consume `tracklistGridTemplate`). No commits made.

## Result

Implemented end-to-end and user-confirmed live on `dev`. Commits: `367e15b` (`feat(tracklist): edit track ratings
inline from the rating column`, 13 files, 106+/5−). T5/T6 visual follow-ups (column 72px, centered stars + rating
header override) are included in that commit. Push/PR remain user decisions.

## Next step

T1→T3 by one writer, then parent spot-check of one gate before reporting.
