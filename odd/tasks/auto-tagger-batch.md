# ODD task: Automatic tagger — batch selection across multiple tracks

Status: IN PROGRESS
Branch: `dev`
Feature name: `auto-tagger-batch`
Created: 2026-09-30

## Objective

Let the tagger work on **several tracks at once**. Today the context-menu entry is disabled unless
exactly one track is selected. Now: one track keeps the current single-track modal (the user likes
it as-is); N tracks get a batch layout modelled on Harmony's `TagCandidatesSelection` — one row per
local track with its candidates side by side, a pre-selected best match, an explicit "not available"
choice, and one confirm that applies them all.

## Reference (Harmony, read before designing)

`src/renderer/src/components/Modal/TagCandidateSelection/TagCandidatesSelection.tsx` and
`src/renderer/src/stores/useTaggerStore.ts`. The parts worth copying:

- one modal for N tracks; per-track row = local header (title, artist, duration, filename, error /
  no-results status) + horizontal candidate cards;
- each card: selection checkmark, artwork, title, artist, mix badge, provider + score badge, and a
  duration/BPM/key/genre/release-date/label grid, with a tick when the candidate duration is within
  5s of the local one;
- a permanent "not available" card per row, so a track can be explicitly skipped;
- the best candidate is pre-selected when it scores ≥ 0.85;
- search progress (`processed`/`total`/`currentTrackTitle`) and a footer with counts plus
  `Confirm (N)`.

## Decisions taken (and why), not asked

1. **Two layouts, not one.** Harmony uses the batch layout for everything, but the user explicitly
   wants the current single-track modal kept for one track. So `tracks.length === 1` renders the
   existing `TagSearchModal` unchanged, and N renders the new batch modal.
2. **No auto-apply of ≥90% matches in this slice.** Harmony auto-applies perfect matches in the
   background and removes them from the modal. That silently overwrites tags on tracks the user
   never looked at, so it is deliberately left out and offered as a separate choice.
3. **Pre-selecting the best candidate at ≥0.85 stays**, because it matches Harmony and is
   non-destructive: nothing is written until the user confirms.
4. **The batch search loops the existing per-track command in the frontend**, sequentially, with
   visible progress. A Rust batch command with real concurrency and progress events is the better
   end state and is recorded as the follow-up; it is not needed to deliver the UX, and inventing an
   event protocol plus cancellation now would balloon the slice.
5. **Non-track layouts stay untouched:** the provider capsules, the threshold-coloured score chip,
   the purple version capsule and `AlbumArt` are reused verbatim in the batch cards, so the two
   layouts look like one feature.

## Design contract (pin these names)

### `shared/stores/tagger.ts` (additive; the single-track path is untouched)

```ts
export interface BatchRow {
  track: Track
  candidates: ScoredTagCandidate[]
  errors: ProviderError[]
  /** Per-track failure (the search itself threw), distinct from per-provider errors. */
  error: string | null
}
```
State additions: `batchRows: BatchRow[]`, `batchSelections: Map<string, ScoredTagCandidate | null>`,
`batchProgress: { processed: number; total: number; currentTitle: string }`, `batchLoading: boolean`,
`batchApplying: boolean`.

Actions:
- `searchBatch(tracks: Track[]): Promise<void>` — sequential; per track sets `currentTitle` and bumps
  `processed`; collects a `BatchRow` per track (a thrown search becomes that row's `error` rather
  than aborting the batch); after the loop, pre-selects the best candidate for every row whose
  `candidates[0].similarity_score >= 0.85`.
- `selectFor(trackId: string, candidate: ScoredTagCandidate | null): void` — `null` is an explicit
  "not available" and must be distinguishable from "not decided yet".
- `applyBatch(): Promise<{ updated: Track[]; failed: Array<{ trackId: string; error: string }> }>` —
  for every row with a non-null selection: `extendTrackTag(selected)`, then
  `updateTrackMetadata(track.id, candidateToPatch(extended))`, then `setTrackArtworkFromUrl` when the
  candidate has an `artwork_url`; one track's failure must not stop the rest and must land in
  `failed`; set `batchApplying` for the duration.
- `resetBatch(): void`.

### `apps/desktop/src/lib/components/tagger/TagBatchModal.svelte` (NEW)

Props `{ open: boolean; tracks: Track[]; onClose: () => void; onApplied: (updated: Track[]) => void }`.
Runs `searchBatch(tracks)` on open and `reset()`/`resetBatch()` on close. Renders, in the visual
language of `TagSearchModal`: progress while searching, one row per track, the candidate cards, the
"not available" card, per-row error/no-results status, a footer with `N to apply · M skipped` and a
Confirm button disabled while nothing is selected or while applying.

Reuse the existing pieces rather than re-inventing them: `AlbumArt` for artwork, the provider
capsule mapping and the score colour mapping (extract them from `TagSearchModal.svelte` into a small
shared helper module under `components/tagger/` if that avoids duplicating them — do NOT fork the
styling).

### Wiring

- `TrackContextMenu.svelte`: the `findTags` item is enabled for **one or more** selected tracks
  (drop the `!== 1` disable) and calls back with the whole selection.
- `ContextMenuOrchestrator.svelte` + `OrchestratorLayer.svelte`: pass the selected tracks through.
- `ModalOrchestrator.svelte`: `openTagSearchModal(tracks: Track[])` renders `TagSearchModal` when
  `tracks.length === 1`, otherwise `TagBatchModal`; the applied handler updates the library for every
  updated track.

### i18n

New keys under `tagger.batch.*` in `shared/i18n/locales/en.json` only (title, subtitle, searching
with counts, applying, notAvailable, noResults, error, confirm with count, applied/skipped stats).
The other 14 locales are a separate translation pass, as with the previous slice.

## Delivery

- Forecast: ~650–750 lines across ~8 files (store, new component, a small shared style helper,
  context menu, both orchestrators, en.json). Above the ~400 heuristic, hence its own slice.
- Reviews are disabled for this clone; this slice is verified by tests and gates only.
- No push, no PR without an explicit instruction.

## Verification

```bash
yarn format:check && yarn lint:check && yarn check:svelte && yarn check:svelte:mobile
yarn check:cargo
```
`svelte-check` must stay at the pre-existing baseline (1 `PUBLIC_APP_VERSION` error + 2 a11y warnings
in `LibraryTab.svelte`); no new diagnostic in any touched file.

## Follow-ups recorded, not built

- A Rust batch search command with bounded concurrency and progress events (the sequential
  frontend loop is the slow part for large selections).
- Optional auto-apply of ≥90% matches, as a deliberate user choice.
- Translating the batch keys into the other 14 locales.

## Result (2026-09-30)

**S1 — batch selection** (`6502a99`): two layouts as decided (one track keeps the existing modal, N
get the batch layout), the context-menu entry enabled for any selection of one or more, pre-select at
0.85, a "not available" card per row, the `N to apply · M skipped` footer, the tone helpers extracted
to `tones.ts` so the two layouts cannot drift apart, and a sequential frontend batch search with
progress.

**S2 — auto-apply**: `AUTO_APPLY_MIN_SCORE = 0.9`, exported as a named constant. Rows whose best
candidate reaches it are applied **without asking**, excluded from the list, and written in the
background while the user chooses for the rest. Failures are surfaced (live card, `state.error` and
one end-of-pass toast), never swallowed.

The threshold is 0.9 and not 0.85 on purpose: Harmony auto-applies at 0.9 (`tagger.ts:324`) and
pre-selects at 0.85 (`TagCandidatesSelection.tsx:279`). At 0.85 the pre-select would be dead code,
because every pre-selected row would already have been applied and removed. Harmony also disables
auto-apply for a single track (`Details.tsx:188`, `autoApply: false`), so `search(track)` is
untouched.

### Bug found in my own scoping, and fixed

The first auto-apply writer returned `partial` and refused to leave its scope: the only wired handler
(`ModalOrchestrator.handleTagBatchApplied`) starts with `closeAll()`, so the first background apply
**closed the modal** before the user could decide anything — the exact opposite of the requested
behaviour. My prompt had scoped `ModalOrchestrator` out.

Fix: the background refresh moved **out of the modal** into a close-free `$effect` in
`ModalOrchestrator`, which is always mounted — so it also refreshes the library when the user closes
the modal mid-pass. `onApplied` is now exclusively the confirm path. A module-level generation
counter keeps a pass belonging to a replaced batch from repopulating `autoApply`.

### Closing the modal does not stop the pass (decided 2026-09-30)

On the user's call, the auto-apply pass **always runs to completion**, open modal or not. The first
implementation had the guard `return` early before each row, which made closing the dialog silently
abandon the remaining tracks. That is now inverted:

- the generation counter is bumped only when a **new batch** starts, so a close never cancels a pass;
- it decides only whether the on-screen counters (`total` / `processed` / `failed`) still describe
  the batch being displayed — never whether a row gets applied;
- `autoApply.updated`, the feed the orchestrator watches to refresh the library, is written
  unconditionally, so tracks auto-applied after the modal closed still appear in the list;
- `resetBatch` clears the on-screen counters but deliberately keeps that feed;
- the end-of-pass failure toast counts locally instead of reading state back, so a reset during the
  pass cannot swallow the "these were not tagged" warning.

Verified by gates only: no automated test covers the store, so the behaviour is proven by reading the
control flow, not by running the app.

