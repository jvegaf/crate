# ODD task: Automatic tagger — per-ID extend and the selection UI

Status: IN PROGRESS
Branch: `dev`
Feature name: `auto-tagger-apply`
Created: 2026-09-29

## Objective

Close the automatic tagger loop: enrich a chosen candidate with its per-ID detail (`extend`) and give
the user a desktop UI where they see the best 5 candidates, pick the correct one, and apply it.
Backend `extend` plus the first frontend slice for the tagger.

## Problem / Why

`search_ranked_track_tags` already returns a ranked shortlist, but (a) Bandcamp hits carry no label,
release date or duration, and (b) there is no way for the user to act on the shortlist — the tagger
has zero frontend. The user asked to close both ("dale con todo").

The predecessor docs deferred exactly this: `auto-tagger-providers.md` and
`auto-tagger-matcher.md` both end naming "per-ID `extend` enrichment and the frontend selection UI"
as the next slice.

## Key findings from exploration (they shape the design)

1. **Applying metadata already exists and is proven.** `updateTrackMetadata(id, patch)` reaches
   `LibraryService::update_track_metadata`, which stages a sibling copy, writes the tags with
   `lofty` (`services/file_tags.rs`), replaces the original, and updates the DB with rollback. So
   "apply a match" is a *reuse*, not new file-writing code. No new persistence, no migration.
2. **`TrackMetadataPatch` has no remote artwork field** — only `embedded_artwork`. Downloading
   Beatport/Bandcamp artwork and embedding it is therefore NOT in scope (it was already excluded by
   the providers slice). Candidates' `artwork_url` is displayed, not applied.
3. **The audio-analysis service cannot enrich a remote hit.** `AnalysisService` is desktop-gated,
   track-ID driven, and its path→BPM/key routine is private. Harmony enriches Bandcamp hits by
   analysing the *local* file afterwards; reusing that here would mean new plumbing. Out of scope.
4. **The provider search payloads are already the right shape.** `TagCandidate` carries every field
   `TrackMetadataPatch` accepts (title, artists, album, label, catalog_number, genre, release_date,
   bpm, key, duration_ms, isrc, track_number). `extend` therefore returns an enriched
   `TagCandidate`, not a new type.
5. Multi-track selection exists (`uiStore.selectedTrackIds` + `+page.svelte`'s handler), but the
   first UI slice is deliberately single-track (see Decisions).

## Scope

In scope:
- **S1 (backend)** — `extend`: per-provider per-ID enrichment returning an enriched `TagCandidate`,
  plus one Tauri command. Hermetic tests on the parsers.
- **S2 (frontend)** — mirror the tagger DTOs in `shared/types`, add `shared/api/tagger.ts`, add a
  `taggerStore`.
- **S3 (frontend)** — a `TagSearchModal` (ranked candidates + provider errors + pick + apply via the
  existing metadata path), the track context-menu entry, and the orchestrator wiring.
- **S4 (frontend)** — i18n keys (en + the other locales where the wording translates).

Out of scope, deliberately:
- Album-art download and embedding (see finding 2).
- Batch/queue UX over many selected tracks (see Decisions).
- Audio analysis to synth BPM/key for Bandcamp hits (see finding 3).
- Any new dependency, migration, or persisted state.
- TraxSource live behaviour is still Cloudflare-blocked for the search path; its `extend` reuses the
  desktop-gated `curl` transport.

## Decisions taken (and why), not asked

1. **Single-track modal first.** The context menu can act on N selected tracks, but a ranked
   shortlist is a per-track judgement. S3 opens for one track; the store is written so a batch queue
   can layer on later without a rewrite. Reversible and cheap if the user wants batch.
2. **`extend` is on demand, not automatic.** Harmony extends during apply; so do we. The ranked
   search stays one network round per provider, and an extra request happens only for the candidate
   the user actually picks.
3. **`extend` takes the whole candidate, statelessly.** The command receives `provider` + the
   `TagCandidate` the frontend already holds and returns an enriched copy. No server-side session
   state, no re-search. TraxSource and Bandcamp use `candidate.url`; Beatport uses
   `candidate.provider_track_id`.
4. **Apply maps only fields the candidate actually has.** Missing candidate fields stay `None` and
   are omitted from the patch, so applying never clears an existing tag by accident.

## Design contract (pin these names)

### S1 — backend

`src-tauri/src/models/tagger.rs`: no new types. `extend` returns `TagCandidate`.

`src-tauri/src/services/tagger/mod.rs`:
```rust
pub(super) trait TaggerProvider: Send + Sync {
  fn id(&self) -> &'static str;
  async fn search(&self, client, query, limit) -> Result<Vec<TagCandidate>>;
  /// Enrich one candidate by its provider id. Defaults to returning the
  /// candidate unchanged; providers override it only where the search payload
  /// actually leaves fields empty.
  async fn extend(&self, client: &reqwest::Client, candidate: &TagCandidate) -> Result<TagCandidate> {
    Ok(candidate.clone())
  }
}

impl TaggerService {
  /// Enrich one candidate with its per-ID detail. Dispatches on
  /// `candidate.provider`; an unknown provider is a `CrateError::Tagger`.
  pub async fn extend_candidate(&self, candidate: &TagCandidate) -> Result<TagCandidate>;
}
```

`src-tauri/src/commands/tagger.rs`:
```rust
#[tauri::command]
pub async fn extend_track_tag(
  candidate: TagCandidate,
  tagger: State<'_, TaggerService>,
) -> Result<TagCandidate>;
```
Register in `lib.rs` in the non-gated tagger block. The command dispatches on
`candidate.provider`; no separate `provider` argument, so nothing can disagree with the candidate.
An unknown provider (including the desktop-gated TraxSource on mobile) is a `CrateError::Tagger`.

Per provider:
- **Bandcamp** — the one with a real gap (its autocomplete search returns no label, no release date
  and no duration). `GET {candidate.url}` and parse the page's `application/ld+json`
  (`MusicRecording`: `name`, `byArtist.name`, `inAlbum.name`, `datePublished`, `duration` ISO-8601,
  `image`), plus the page's embedded release JSON for label/genre when present. No new dependency —
  `regex` + `serde_json` are already there. Needs a small hand-rolled ISO-8601 duration parser
  (`PT5M41S` → `341_000` ms) with its own tests. Fields the page does not expose stay `None`; never
  guess.
- **Beatport** — override `extend` with the authoritative detail fetch:
  `GET https://api.beatport.com/v4/catalog/tracks/{id}/` reusing the existing cached Bearer token,
  then the existing `parse_track` on the returned object. Fills whatever the search payload left
  empty (e.g. artwork, release detail).
- **TraxSource** — inherits the default (candidate unchanged). Its search payload already carries
  label, key, BPM, genre and release date, and its detail page is Cloudflare-gated, so a scraper
  there would be unverifiable code. Documented, not silently missing.

### S2 — frontend types/API/store

- `shared/types/index.ts` (or a new `shared/types/tagger.ts` re-exported from it): mirror
  **snake_case** exactly — `TagCandidate`, `ScoredTagCandidate` (`similarity_score`),
  `ProviderError`, `RankedSearchResult`. Field-for-field from `models/tagger.rs`.
- `shared/api/tagger.ts`:
```ts
export async function searchRankedTrackTags(params: {
	artist: string | null
	title: string
	durationMs: number | null
	limit?: number
	maxCandidates?: number
	minScore?: number
}): Promise<RankedSearchResult>

export async function extendTrackTag(candidate: TagCandidate): Promise<TagCandidate>
```
  camelCase args on the wire (Tauri maps to the snake_case Rust params).
- `shared/stores/tagger.ts` — `createTaggerStore()` factory, exported from `shared/stores/index.ts`
  and re-exported from `apps/desktop/src/lib/stores/index.ts`. State: `trackId`, `loading`,
  `extending`, `candidates`, `errors`, `error`, `selectedProvider`. Actions: `search(track)`,
  `select(provider)`, `apply(track)` (calls `updateTrackMetadata` with the mapped patch),
  `reset()`. Failures surface through `state.error` + `toastStore.error`.

### S3 — frontend UI

- `apps/desktop/src/lib/components/tagger/TagSearchModal.svelte` + folder `index.ts` export. Follows
  `RelocateTrackModal`'s shape (`open`, domain props, `onClose`, submit callback, `{#snippet footer}`).
  Shows: candidate rows (provider, title, version, artists, album, label, bpm, key, duration,
  `similarity_score` as a percentage), a distinct block for `errors`, a selected state on the row,
  and an apply action. Tokens: `bg-surface-0/1/2`, `text-text-*`, `border-stroke[-subtle]`,
  `bg-brand-muted` for the selected row.
- Context menu entry: `onSearchTags` prop on `TrackContextMenu`, item id `searchTags`, label from
  `contextMenu.searchTags` — note `contextMenu.searchTags` is **already taken** by the tag *filter*
  search, so the new key must be distinct (use `contextMenu.findTrackTags`).
- Wiring: `ContextMenuOrchestrator` (union + opener + handler) → `OrchestratorLayer` (callback) →
  `ModalOrchestrator` (union member, `openTagSearchModal(track)`, render block, apply handler).

### S4 — i18n

New top-level namespace `tagger` in `shared/i18n/locales/en.json` (75+ keys not needed; keep it
tight: title, searching, noResults, providersFailed, columns, apply, applied, errors). Translate the
other 14 locales where the wording is correct — leaving English inside a translated locale is a
visible defect.

## Task slices

| Slice | Content | Files (approx) | Route | Checks |
| --- | --- | --- | --- | --- |
| S1 | `extend` trait method + 3 provider impls + `extend_candidate` + command + registration + hermetic parser tests | 6 Rust files | delegated writer | `cargo test --features desktop`, clippy, fmt |
| S2 | shared types + `shared/api/tagger.ts` + `taggerStore` + both barrels | ~4 TS files | delegated writer | `yarn check:svelte`, lint, format |
| S3 | `TagSearchModal` + context-menu entry + orchestrator wiring | ~6 files | delegated writer | `yarn check:svelte`, lint, format |
| S4 | i18n keys | 15 locale files | delegated writer | `yarn format:check` |
| S5 | Parent verification per slice + native review per candidate (RDD is on) | — | inline (parent) | see Verification |

Each slice is one work-unit commit on `dev`, feature-only (no `odd/` scaffolding mixed in; the doc
closes in its own `docs(odd):` commit, as the two previous slices did).

## Delivery

- Forecast: S1 ~450 lines, S2 ~250, S3 ~450, S4 ~200 → **~1350 lines total**, well above the ~400
  review heuristic. That is why it is sliced: each slice is one candidate and one commit, so review
  stays focused and the chain is honest.
- Repo policy: commit only on explicit instruction (the user gave it for this feature); no push.

## Verification

Backend (S1):
```bash
cd src-tauri
cargo fmt --check
cargo clippy --features desktop -- -D warnings
cargo test --features desktop
```
Frontend (S2–S4):
```bash
yarn format:check && yarn lint:check && yarn check:svelte && yarn check:svelte:mobile
yarn check:cargo
```
Known environment trap: `cargo fmt --check` reports three pre-existing 4-space files
(`build.rs`, `src/main.rs`, `services/library/metadata_update.rs`) because of this machine's global
`~/.config/rustfmt/rustfmt.toml`; none of this feature's files may appear in that list.

## Progress log

- 2026-09-29: exploration done (provider extend reference from Harmony; frontend surface mapped:
  modal/context-menu/store/i18n patterns, the lofty write path, the artwork and audio-analysis
  boundaries). Task doc created.
- 2026-09-29: **S1 implemented and verified**: trait default `extend` + Beatport/Bandcamp overrides +
  `TaggerService::extend_candidate` + `extend_track_tag` command + registration + 5 hermetic tests.
  `cargo clippy --features desktop -- -D warnings` clean; `cargo test --features desktop`
  **282 passed / 0 failed / 4 ignored** (parent spot-check matched). Not committed yet.
- 2026-09-29: **S1's native review could not complete.** Lineage `review-c82f3ab6f25ba11c`
  (risk `medium`, lens `review-reliability`, target `sha256:88d1b78c…`) is stuck in `reviewing`:
  the OpenCode reviewer Task returned **no output three times** (`opencode_task_output_empty`), each
  time after the exact-lineage STATUS re-offered the same bound slot with an identical binding. The
  slot is still offered; retries stopped there rather than looping. This is a client-runtime Task
  failure, not a Gentle AI contract defect, so there is no defect report and no workaround to apply
  from here.

  **S1 is verified but NOT reviewed.** Options for the user: retry the reviewer later, continue
  without this candidate's review (leaving the lineage non-terminal until a maintainer abandons it),
  or disable reviews (`gentle-ai review mode disable`) and proceed under ordinary policy. S2–S4 were
  not started, so nothing depends on S1's uncommitted state.
- 2026-09-29: user chose to continue with S2–S4 and leave S1's review pending. **S1 committed**
  (`5a1acf7`) with that status recorded here, so S2 has a clean slice diff.
- 2026-09-29: **S2 done and committed** (`e3f77dc`): tagger types in `shared/types/index.ts`,
  `shared/api/tagger.ts`, `shared/stores/tagger.ts`, both barrels. Verified: `yarn format:check`
  clean, `yarn lint:check` exit 0, `yarn check:svelte:mobile` 0 errors / 0 warnings,
  `yarn check:svelte` exit 1 with the **pre-existing** `PUBLIC_APP_VERSION` error plus two a11y
  warnings, zero diagnostics in this slice's files.
  Parent correction: the writer keyed the selection by provider id, which collides when two
  candidates share a provider; changed to hold the selected candidate **by value**.
- Remaining: **S3** (`TagSearchModal` + track context-menu entry + orchestrator wiring) and **S4**
  (i18n across locales). Not started.
- 2026-09-29: **S3 done and committed** (`a8fd660`): `TagSearchModal` + barrel, the `findTags` context
  menu entry (disabled unless exactly one track is selected), the `ModalOrchestrator` /
  `ContextMenuOrchestrator` / `OrchestratorLayer` wiring, the store's toasts moved to `translate`,
  and the English keys. Apply reuses the metadata-editor refresh path
  (`libraryStore.updateTracksInState` + `syncStore.notifyTrackChanges`); no new callback prop was
  needed. Re-sliced from the plan: the `en.json` keys moved into S3 so the UI is never left
  rendering raw keys; S4 became the other 14 locales only.
  Verified by the parent: `yarn format:check` exit 0, `yarn lint:check` exit 0,
  `yarn check:svelte:mobile` 0 errors / 0 warnings, `yarn check:svelte` exit 1 with exactly the
  pre-existing `PUBLIC_APP_VERSION` error and two a11y warnings, **zero diagnostics in this slice's
  files**.
- 2026-09-29: **S4 done and committed**: `contextMenu.findTrackTags` plus the `tagger` namespace
  translated into all 14 non-English locales. Parent-verified: every locale parses, holds both key
  sets, and none copied the English values verbatim. Pre-existing drift found and left alone: the
  non-English locales are missing `contextMenu.editMetadata`, which only `en.json` has.

### Final state

Slices S1–S4 are implemented, verified and committed on `dev`. Two things are NOT done and are
recorded here rather than implied:

1. **S1's native review never completed** (empty reviewer output three times, lineage
   `review-c82f3ab6f25ba11c` left non-terminal).
2. **Nothing was exercised against a running app.** The UI passes the type, lint and compile gates,
   but the search → select → extend → apply flow has not been run by hand.

## Orchestration gotcha (reused)

The OpenCode reviewer Task must receive ONLY the provider-issued `provider_task.prompt`.
