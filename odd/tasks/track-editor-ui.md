# Track Editor UI Redesign (sidebar + metadata modal, bulk tags)

## Objective and accepted decisions

Redesign the track metadata editing surfaces, which are cramped and show an unusably small
artwork, into a consistent two-surface design with a real tags workflow:

1. **Both surfaces, consistent** (user decision): right-sidebar `TrackEditor` and the
   context-menu `TrackMetadataModal` share section grouping and visual language.
2. **Tags in the sidebar, with backend** (user decision). After scouting, **no Rust work is
   required**: bulk `assign_tags` / `remove_tags` commands already exist
   (`src-tauri/src/commands/tag.rs:71-86`, service `src-tauri/src/services/tag.rs:355/373`,
   TS wrappers `shared/api/tags.ts:71-82`). The sidebar will use them.
3. **Sidebar widths** (user decision): default 320→440, clamp max 500→640
   (`apps/desktop/src/lib/stores/uiLayout.ts:43` default, `:106` clamp). Min stays 280.
   Existing users keep their persisted width (localStorage) — accepted.

Artwork outcome: sidebar content column 440 − 32 padding = 408px; `AlbumArt lg`
(`aspect-square w-full max-w-[400px]`, `AlbumArt.svelte:22`) renders ~400px. No change to
AlbumArt itself.

## Scope

### U1 — Sidebar TrackEditor (worker)

Files:
- `apps/desktop/src/lib/stores/uiLayout.ts` — default 440, clamp `Math.max(280, Math.min(640, w))`.
- `apps/desktop/src/lib/components/editor/TrackEditor.svelte` — regroup into sections:
  - Artwork (full column width, existing `EditorArtwork`), divider.
  - Section "Information": Title, Artist, Album, grid-2 [Year, Label].
  - Section "Additional": grid-2 [BPM, Key], Genre full width.
  - Section "Tags": union chips across selection; chip remove → `removeTags(selectedIds,[tagId])`;
    "Add tags" → popover picker → `assignTags(selectedIds, ids)`; applying to all selected tracks.
    After success: refetch each id via `libraryApi.getTrack` → `libraryStore.updateTracksInState`
    → `syncStore.notifyTrackChanges(ids)` (precedent: `ModalOrchestrator.handleTrackMetadataSave`
    lines 732-789). Immediate apply (no Save button), like track colors.
- `apps/desktop/src/lib/components/editor/EditorTagPicker.svelte` — new popover: search input,
  tags grouped by category from `tagsStore`/`allTags`, checkbox multi-select, inline create
  ("Create «name»") via `tagsStore.createTag(categoryId, name)` then assign.
- `apps/desktop/src/lib/components/editor/index.ts` — export picker if needed.
- `shared/i18n/locales/*.json` (15 files) — new `editor.*` keys: section headers
  (information, additional, tags), addTags, searchTags, createTagInline ({name}), noTags.
  English in `en.json`; all other 14 translated (AGENTS §5.8; no parity tooling).

IPC semantics of existing metadata fields are unchanged.

### U2 — TrackMetadataModal consistency (worker, after U1 verify)

File: `apps/desktop/src/lib/components/common/TrackMetadataModal.svelte`
- Crate artwork section: replace 32px `size="sm"` preview with a proper preview:
  wrapper `<div class="w-40 shrink-0"><AlbumArt size="lg" …/></div>` (wrapper controls size;
  do not touch AlbumArt classes).
- Split `details` fields into the same "Information"/"Additional" groups as the sidebar
  (Information: title, artist, album, year, label; Additional: bpm, key, genre, rating),
  same i18n section keys.
- Keep save/diff logic (`ModalOrchestrator` diffing tagIds) untouched.

## Explicit exclusions

### U3 — Modal iteration (user testing feedback, 2026-09-29)

User tested `3733f93` and requested, all on the MODAL (clear buttons, rating, and embedded
crate artwork only exist there):

1. Remove adjacent "Clear" buttons; in-field `x` IconButton (icon x, absolute right, only when
   the field has a value, title `modals.trackMetadata.clearField`, `pr-8` on input).
2. Rating as interactive stars (TrackRow.svelte:228-230 precedent: `★`, filled `text-warning`,
   empty `text-text-tertiary/50`). Click star n sets n; clicking the ACTIVE value clears to 0.
   ValidatePatch 0-5 int rule unchanged.
3. Merge Crate artwork + Embedded artwork into ONE "Artwork" section: Choose → sets BOTH
   `crateArtwork={set,path}` and `embeddedArtwork=bytes` (existing `chooseImage()` already returns
   both). Remove → BOTH clears (`crateArtwork={clear}` + `embeddedArtworkCleared=true`).
   Accepted consequence: library artwork changes now always write the audio file too; per-op
   failures already surface via the orchestrator's partial-save errors. Undo resets all three
   states. Delete the separate embeddedArtwork section. No ModalOrchestrator/backend changes.
4. Layout: two columns. Left column top = merged Artwork preview enlarged (`w-56`, size lg);
   Crate tags below it. Right column = Information (title, artist, album, label, catalog_number),
   then compact row grid-cols-3 [Year, BPM, Key] (user: halve their width, data still readable),
   Genre, Rating stars. technicalDetails full width at bottom. Footer unchanged.
5. New i18n keys ×15 locales: `modals.trackMetadata.artwork` ("Artwork"),
   `modals.trackMetadata.artworkHint` (applies to library and embedded in the file).

Out of U3 scope (follow-up candidates): sidebar `EditorArtwork` still sets crate-only (not
embedded); same for its remove/reextract.

## Explicit exclusions

- Backend/Rust changes (none needed).
- `TrackUpdate` clear-semantics bug found while scouting: sidebar sends `null` for emptied
  inputs, Rust `Option::None` means "no change" (`update.rs:132-184`), so clearing a bulk
  field is a silent no-op today. **Not fixed here** — needs its own product decision
  (empty-string vs MetadataField-style patch). Follow-up candidate.
- Per-track tag counts in mixed selections (v1 shows the union without counts).
- Mobile app changes; `apps/mobile` untouched.

## Acceptance criteria

1. New-user default sidebar 440px; artwork ~400px square in the editor; max drag 640.
2. Fields grouped Information/Additional/Tags in both surfaces; no IPC behavior change.
3. Chips show union of selected tracks' tags; add (picker incl. inline create) and remove
   apply to all selected tracks; chips update without reopening; auto-sync notified.
4. Modal artwork preview ≥ 140px and readable.
5. All 15 locales have the new keys, translated.
6. Gates: `yarn check:svelte`, `yarn check:svelte:mobile`, `yarn lint:check`,
   `yarn format:check` pass (note: pre-existing clean-HEAD failures must be distinguished
   and reported, not silently "fixed").

## Verification plan / TDD mode

- No JS test runner exists; UI TDD off (repository precedent, `track-metadata-modal.md`).
- No Rust changes → cargo gates not required by this feature; scout evidence recorded instead.
- Worker returns per-file diff summaries; parent runs the yarn gates and mobile check.
- Final candidate goes through native review authority when committing.

## Evidence

- Worker incident (U1 attempt 1, task `mumkldab-2-rq2a`): failed at turn 3 with "assistant reported an error" (transient provider-side). Diagnosis: clean tree except one completed, correct edit already applied to `apps/desktop/src/lib/stores/uiLayout.ts` (default 440, clamp max 640) — kept; relaunched remaining U1 scope as attempt 2 without the width task.
- U1 COMPLETE, verified GO: gates `yarn check:svelte` (only baseline PUBLIC_APP_VERSION + LibraryTab a11y warnings), `yarn lint:check` pass, prettier pass on touched files, `git diff --check` clean; whole U1 diff +389/-65 (locales 105, new picker 139). Verifier caught and parent fixed two defects: missing `translate` import in EditorTagPicker (5 svelte-check errors) and non-existent `text-2xs` token (replaced with `text-xs font-medium` per TagList precedent). Second writer incident: U1b (`mumoyw0i-3-1mge`) reported-as-failed was actually still running concurrently with U1c; parent cancelled it, U1c reconciled as sole writer (single-writer discipline restored).
- U2 COMPLETE, verified GO after parent fix: TrackMetadataModal +60/−30 (Information/Additional grouping reusing existing editor.* keys, crate artwork w-40 `size="lg"` preview, script change = Text import only). Final verify caught ONE regression: `catalog_number` input dropped from the grouping — parent restored it in the Additional filter, then re-ran prettier (pass) and `yarn check:svelte` (baseline-only). Full final gates: check:svelte baseline-only, check:svelte:mobile 0 errors/0 warnings, lint:check pass, format:check fails only on baseline shared/utils/format.ts, git diff --check clean.
- U3 COMPLETE, verified GO after reconciliation: TrackMetadataModal +165/−171 (in-input × clears for every nullable field incl. year/bpm/key/catalog_number, interactive star rating with clear-to-0 on active value, merged single Artwork section driving crate + embedded states with shared Choose/Remove/Undo, two-column layout 18rem artwork+tags left / grouped fields right, technicalDetails full width). Save path (buildPatch/handleSave/onSave/hasChanges) and CrateArtworkChange export verified byte-compatible vs HEAD by the verifier; mobile check 0/0; desktop check baseline-only; lint/prettier/diff-check pass; all 15 locales parse with artwork+artworkHint (es: "Carátula"/hint).
- Verifier NO-GO reconciliation: its two blockers were environmental, not candidate defects — `.eslintcache` is a lint side-effect and is gitignored (.gitignore:17), and the untracked `odd/tasks/track-editor-ui.md` is this ODD task file, intentionally excluded from feature commits per AGENTS.md §2. Branch/HEAD identity independently confirmed: `dev` @ `3733f93`.
- i18n note: the 14 non-English locales never had `modals.trackMetadata` at HEAD (the whole modal fell back to English per-key). U3 adds only the 2 new keys localized; remaining ~25 keys still English-fallback — strictly an improvement over HEAD, consistent with the repo's known parity drift. Full modal localization is a follow-up.
- Scout report: `gentle-ai-explore` task `mumkccj2-1-pmv0`, 20 turns, 79 tool calls, completed;
  findings incorporated above (bulk tag commands, width clamp site, ModalOrchestrator refresh
  precedent, `getTrack` per-id as the only by-id refetch path, no bulk fetch-by-ids command).

## Follow-up

- Field clear-semantics decision (see exclusions).
- Consider bulk `get_tracks_by_ids` if large selections make per-id refetch slow.
- If shipped as PRs: branch `{issue-number}-track-editor-ui` targeting `develop`, no agent
  scaffolding in the feature commits, ≤ ~400 lines per unit (U1 ≈ 250-320, U2 ≈ 100-150).

## Delivery

- Committed as single work unit by explicit user choice: `3733f93` feat(editor): redesign track metadata editor surfaces (20 files, +450/-95; odd/** left untracked). Native review authority UNAVAILABLE at close (gentle-pi package-local binary missing; not attempted install without authorization). Fallback review record: two gentle-ai-verify passes (gates + static sanity), defects fixed pre-commit; delivery gate via native review NOT attested.
