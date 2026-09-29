# Tracklist column configuration (visibility, order, auto-fill widths)

## Goal

Let the user decide which tracklist columns exist and in which order, and make the table always
fill its width. Selection happens from a right-click menu on the table header, ordering by dragging
header cells. Both are persisted in the application settings, so the layout survives restarts.

## Context

- Header and rows hardcode the same grid template:
  `TrackListHeader.svelte:43` and `TrackRow.svelte:111` →
  `grid-cols-[24px_40px_1fr_1fr_80px_60px_80px_1fr_60px]` (9 fixed columns).
- `TrackList` is shared by `LibraryView.svelte:121` and `PlaylistView.svelte:265`, so one layout
  serves both views. Discovery has its own row/header and is **out of scope**.
- Sorting is client-side: `shared/utils/sorting.ts` (`sortTracks`, driven by
  `apps/desktop/src/lib/stores/library.ts:390`). No Rust sort surface involved for the tracklist.
- Settings precedent: key/value table, JSON in a string value, typed and parsed in
  `src-tauri/src/services/settings.rs:19` (`ignored_device_ids` is the exact model to copy),
  written through the existing `set_setting` command. **No DB migration needed.**
- Native HTML5 drag events are not usable: `dragDropEnabled: true` in
  `src-tauri/tauri.conf.json:21`. The repo already drags with pointer events
  (`shared/utils/drag.ts`, `DRAG_THRESHOLD`, `TrackRow.svelte:63-92`). Reorder must follow that.
- `ColumnConfig` (`shared/types/index.ts:437`) is dead code, referenced nowhere. It is replaced by
  the new model rather than left behind.
- Bitrate is stored in **bps** (`lofty` `audio_bitrate()`, `import.rs:68`), but
  `formatBitrate` (`shared/utils/format.ts:228`) prints the raw number as "kbps". Fixing it is part
  of this feature because the Bitrate column exposes the bug.

## Accepted decisions (user)

1. Columns available: the 9 today **plus** Label, Origin, Album, Year, Bitrate, Date Added.
2. **Origin** = the immediate parent folder name of `file_path` (users who pre-organize by folder).
3. Reordering is **not** in the menu: it is drag & drop on the header cells. The menu only toggles
   visibility.
4. `title` is locked always-visible; everything else (incl. color and artwork) is hideable.
5. One global layout for library + playlists.
6. New columns are hidden by default: no visible change on upgrade.
7. Persistence in the settings DB (`tracklist_columns`, JSON), device-local — **not** added to
   `SYNCED_SETTING_KEYS` (`cloud_sync/mod.rs:47`).
8. Automatic width: fixed-size columns (bpm, key, duration, bitrate, year, date added, rating,
   color, artwork) keep their px width; fluid columns (title, artist, album, label, origin, tags)
   share the remaining space via `1fr` each. Since `title` is locked and fluid, the grid always has
   at least one `fr` track and never leaves a gap on the right.

## Column model

| id | labelKey | width | sortable field | default visible |
| --- | --- | --- | --- | --- |
| color | `''` | 24px | color | yes |
| artwork | `''` | 40px | — | yes |
| title | `library.columns.title` | 1fr (locked) | title | yes |
| artist | `library.columns.artist` | 1fr | artist | yes |
| album | `library.columns.album` | 1fr | album | no |
| label | `library.columns.label` | 1fr | label | no |
| origin | `library.columns.origin` | 1fr | origin | no |
| bpm | `library.columns.bpm` | 80px | bpm | yes |
| key | `library.columns.key` | 60px | key | yes |
| duration_ms | `library.columns.time` | 80px | duration_ms | yes |
| bitrate | `library.columns.bitrate` | 90px | bitrate | no |
| year | `library.columns.year` | 56px | — | no |
| date_added | `library.columns.dateAdded` | 110px | date_added | no |
| tags | `library.columns.tags` | 1fr | — | yes |
| rating | `library.columns.rating` | 60px | — | yes |

Persisted shape: `[{ id, visible }]` in display order, hidden entries included so their position is
remembered when re-enabled.

## Tasks

- [x] **W1** — Shared model + pure layout utils, TDD.
  `shared/types/index.ts`: `TracklistColumnId`, `TracklistColumnPref`, replace dead `ColumnConfig`,
  extend `TrackSortField` with `label | bitrate | origin | date_added? (already present)` and add
  `tracklistColumns` to `AppSettings`.
  `shared/utils/tracklistColumns.ts`: registry table above, `defaultTracklistColumns`,
  `normalizeTracklistColumns` (drop unknown ids, append missing defs, force title visible),
  `resolveTracklistLayout` → visible defs + `grid-template-columns` string.
  `shared/utils/sorting.ts`: sort values for label/bitrate/origin (origin via new
  `getTrackOriginFolder` in `shared/utils/format.ts`) and `formatBitrate` bps→kbps fix.
  Tests in `shared/utils/tracklistColumns.test.ts` + extend `format.test.ts`.
  Check: `npx vitest run`.

- [x] **W2** — Backend persistence. `src-tauri/src/models/settings.rs`:
  `TracklistColumnPref { id, visible }` camelCase, `Vec` field + `Default` empty.
  `src-tauri/src/services/settings.rs`: parse `tracklist_columns` JSON next to
  `ignored_device_ids`, tolerate malformed JSON → empty vec (frontend supplies defaults).
  Not synced. Check: `yarn check:cargo`, `cargo fmt --check`,
  `cargo clippy --features desktop -- -D warnings`, `cargo test --features desktop`,
  iOS compile check.

- [x] **W3** — Store wiring. `shared/stores/settings.ts`: state field, load mapping,
  `setTracklistColumns()` (optimistic + `set_setting('tracklist_columns', JSON)`), derived store;
  re-export from `apps/desktop/src/lib/stores/index.ts`.

- [x] **W4** — Dynamic rendering. Header and row read one resolved layout and apply
  `style="grid-template-columns: …"`; row cells rendered per column id (snippet switch), keeping the
  existing color/artwork/tags/rating behavior intact; new cells for album, label, origin, year,
  bitrate, date_added. Check: `yarn check:svelte`, `yarn check:svelte:mobile`.

- [x] **W5** — Right-click column menu on the header: one item per column, check = visible,
  `title` disabled, divider + "Reset to defaults". Reuses `ContextMenu.svelte`; add
  `keepOpen?: boolean` to `ContextMenuItem` so toggling several columns does not close the menu.

- [x] **W6** — Pointer-based header reorder with insertion indicator, commit + persist on pointerup.
  Must not swallow the click-to-sort path (threshold from `shared/utils/drag.ts`).

- [x] **W7 docs** — user-guide section added in `14632bb` (docs markdown is not Prettier-formatted in
  this repo; `yarn format:check` only globs ts/js/json/svelte/css, so the section follows the
  file's own style: +30/−0).

## Constraints

- AGENTS.md §5.4: no `#[cfg(feature = "mobile")]`; shared TS stays mobile-safe (no
  `@tauri-apps/api` outside `shared/api`).
- Row height stays 33px and the virtualizer math untouched.
- No column resizing (explicitly out of scope).
- One writer at a time; work units stay well under ~400 changed lines each.

## Explicit exclusions

- Discovery list columns.
- Per-playlist or per-view layouts.
- Cloud-syncing the column preference.
- The `import.rs:318` symphonia fallback storing `bits_per_sample` as bitrate — separate defect,
  file an issue instead of fixing here.

## Verification plan

- TDD is available and used for W1 (pure logic, vitest exists, CI runs `npx vitest run`).
- Rust gates for W2 (real code change, both targets must compile).
- `yarn check:svelte` + `yarn check:svelte:mobile` + `yarn lint:check` + `yarn format:check` at close.
- Manual smoke in `yarn dev`: toggle, drag, restart, verify persistence.
- Review candidate is one work-unit commit, not the accumulated branch.

## Evidence log

- **Native review authority UNAVAILABLE at close.** `gentle_review` `{"operation":"inspect"}` returned
  `blocked` / `native-status-package-binary-missing` (`error_code: package-local-binary-missing`,
  `lineage_created: false`, `mutation_performed: false`). The recovery command is
  `node scripts/install-gentle-ai.mjs`, which was **not** run — installing tooling is the user's call.
  RDD itself reads `on (decided by default)` in both scopes, so this is a tooling gap, not a disabled
  switch. Fallback review record = the two `gentle-ai-verify` passes below. Delivery is therefore
  **not** attested by a native receipt; same disposition already recorded in
  `odd/tasks/track-editor-ui.md`.
- **W5 + W6 + W7 COMPLETE, verified.** W5 (`mumx0lky-a-biji`): `TrackListColumnsMenu.svelte` mounted
  from the header, `keepOpen` added to `ContextMenuItem` + `ContextMenu.svelte` close path, 6 new
  `library.columns.*` keys + `library.columnMenu.reset` in all 15 locales. Parent added
  `library.columnMenu.alwaysVisible` ×15 so the locked Title entry explains itself instead of
  repeating its own label, and pointed the tooltip at that key. W6: `TracklistMovePlacement`
  before/after on `moveTracklistColumn`, pointer-drag reorder with `DRAG_THRESHOLD`, cached cell
  rects, `setPointerCapture`, half-cell drop, edge insertion marker, post-drag click suppression, and
  the menu-click selection fix (W6 was cancelled mid-run; the parent finished and reviewed it).
  `docs/user-guide/library-management.md` gained a "Customizing the Tracklist Columns" section.
- **W5/W6 adversarial verify: NO-GO on 2 real defects, both fixed by the parent, plus 2 hardening
  items taken from its runtime-unknown list:**
  1. `yarn lint:check` **failed** — unused `svelte-ignore a11y_no_static_element_interactions` on the
     header cell wrapper (`TrackListHeader.svelte:190`). The parent had initially marked lint green
     because the grep used to read the output hid the exit code; the verifier caught it. Removed.
  2. Docs fixed-width list omitted Color and Artwork, which are fixed 24px/40px in the registry.
     Added.
  3. Hardening: cached drag rects were measured once at threshold and could go stale if the visible
     column set changed mid-drag; now keyed to a `columnSignature` and re-measured when it changes.
  4. Hardening: a pointer released while the window is blurred never delivers `pointerup`, leaving
     `activePointerId` set and blocking every later drag; added `svelte:window onblur` reset and
     cleanup of the pending `suppressClickTimer` + drag state on destroy.
  5. Parent UX fix the verifier could not see: header cells had no `select-none`, so dragging
     selected header text. Added `select-none touch-none` and a `cursor-grab`/`cursor-grabbing`
     affordance.
- **Legacy equivalence proven independently, not just asserted:** a throwaway vitest file (created,
  run, deleted — never committed) exercised all 20 ordered id pairs and confirmed
  `moveTracklistColumn(prefs, from, to)` with omitted placement is identical to the old
  `insertionIndex = targetIndex` rule: rightward resolves to `'after'`, leftward to `'before'`, and
  both land on the target's original slot. W1's own rightward/leftward tests still pass unchanged.
- **Verifier's sound non-defect judgments kept as-is:** `keepOpen` is safe — 14 existing menu
  producers checked, none sets it, and no producer spreads store/server objects into menu items, so
  every existing menu keeps closing on click. `TrackList.svelte` is untouched
  (`git diff --name-only HEAD` confirms): the fix is a `click`-stopping wrapper around the menu, and
  a plain header left-click still clears the selection exactly as at HEAD.
- **Parent process note:** `prettier --write` was run over the docs file before noticing that no
  `docs/**.md` in this repo is Prettier-formatted and `yarn format:check` only globs
  `**/*.{ts,js,json,svelte,css}`. That churn (74/29) was reverted with `git checkout` and the
  section re-applied in the repo's own markdown style: the doc diff is now +30/-0.

- **Sequencing correction from verification (do not re-litigate):** `$translate` renders the raw key
  when a message is missing, so the six new `library.columns.*` labels must exist in the locales
  **before** W5 lets users enable those columns. W7's i18n work therefore ships merged into W5, not
  after it.
- **Header right-click collision (required by W5):** `TrackList.svelte:140` guards its container
  `oncontextmenu` only with `target.closest('[data-track-row]')`, and the header lives inside that
  container. A right-click on the header would also open the library/playlist empty-space menu, so
  W5 must `preventDefault()` + `stopPropagation()` on the header handler (or add a header marker to
  the container guard).
- **W3 + W4 COMPLETE (worker `mumwmojx-7-zva7`, strict TDD explicitly OFF — store wiring and
  component rendering, no component-test runner in the repo).** `settings.ts` gains
  `tracklistColumns` state (`defaultTracklistColumns()` seed, `normalizeTracklistColumns` on load),
  `setTracklistColumns()` persisting the full hidden-inclusive list as JSON under `tracklist_columns`,
  and a derived store re-exported through `apps/desktop/src/lib/stores/index.ts`. Header and row now
  share one `$derived(visibleTracklistColumns(...))` + `$derived(tracklistGridTemplate(...))` pair and
  apply `style="grid-template-columns: …"`, with the dead `grid-cols-[…]` utility removed from both
  (+16/+1/+13−29/+143−100).
- **W1–W4 adversarial verify: GO on behavior preservation.** The verifier diffed old vs new cell by
  cell and confirmed all 9 original cells kept their markup, classes, guards and handlers (color
  spinner + cancel Tooltip, artwork modal click, missing-file icon, tags capped at 3 with `+N` and
  category order, rating stars), that the default template is byte-identical to the old hardcoded
  `24px 40px 1fr 1fr 80px 60px 80px 1fr 60px`, that hidden columns emit no cell in either file, that
  `sortable` in the registry agrees with `getTrackSortValue` for **every** id, and that hiding
  columns cannot change row height (fluid cells are `truncate`, so the virtualizer's 33px estimate
  holds). Wire contract confirmed: Rust `tracklist_columns` → camelCase `tracklistColumns`, the TS
  writer and the Rust reader use the same key string byte-for-byte, `id: String` in Rust is safe
  because the frontend drops unknown ids, the key is absent from `SYNCED_SETTING_KEYS`, and no
  migration was added.
- **Verifier's NO-GO was environmental, not a candidate defect:** it refused to report GO because
  `yarn lint:check` persisted repository-root `.eslintcache`. That path is gitignored
  (`.gitignore:17: **/.eslintcache`) and does not appear in `git status`; the same
  non-defect was adjudicated identically in `odd/tasks/track-editor-ui.md`. Treated as resolved.
- Full green gate set at this point: `npx vitest run` 39/39; `yarn check:svelte` baseline-only
  (1 pre-existing `PUBLIC_APP_VERSION` error + 2 `LibraryTab` warnings); `yarn check:svelte:mobile`
  0/0; `yarn lint:check` clean; prettier clean on all candidate files; `cargo test --features
  desktop` 240/240; `cargo clippy --features desktop -- -D warnings` clean; `cargo check --release
  --features desktop` clean; `rustfmt --check` clean on the two touched `.rs` files; `git diff
  --check` clean. **The `aarch64-apple-ios` compile gate is still UNRUNNABLE here** (target not
  installed) — CI must cover it.

- **W1 COMPLETE (worker `mumvec86-1-6kmq`, fix pass `mumw1lu1-5-7x3g`).** Delivered
  `TracklistColumnId`, `TracklistColumnPref`, `AppSettings.tracklistColumns`, dead `ColumnConfig`
  removed, `TrackSortField` + `label|bitrate|origin`, the 15-entry
  `TRACKLIST_COLUMN_DEFINITIONS` registry, and `defaultTracklistColumns` /
  `normalizeTracklistColumns` / `visibleTracklistColumns` / `tracklistGridTemplate` /
  `moveTracklistColumn` / `toggleTracklistColumn` / `getTrackOriginFolder` in
  `shared/utils/tracklistColumns.ts`. `formatBitrate` now converts stored bps → kbps.
  Gates: `npx vitest run` 39 passed (2 files); `yarn check:svelte` 1 error = pre-existing
  `PUBLIC_APP_VERSION` baseline at `apps/desktop/src/routes/+layout.svelte:12` (2 warnings
  pre-existing); `yarn check:svelte:mobile` 0/0; prettier + lint clean; `git diff --check` clean.
  Diff: +445/−11 initial, then +73 in the fix pass (uncommitted, on `dev`).
- **W1 verifier NO-GO (background `gentle-ai-verify` `mumvmjsu-2-drv4`) caught two real defects,
  both fixed and re-verified:**
  1. `shared/utils/tracklistColumns.ts:78-79` — the `TracklistColumnId | undefined` narrowing broke
     `svelte-check` on **both** targets. `||` short-circuit comparisons do not narrow, so
     `canonicalPosition.get(predecessor)!` failed. Fixed by carrying predecessors/successors as
     `{ id, position }` candidates instead of adding non-null assertions.
  2. `moveTracklistColumn` used `targetIndex - 1` for rightward moves, so a dragged column landed
     *before* the target instead of taking its slot — the exact semantics the header drag (W6)
     depends on. Fixed to `targetIndex` in both directions (removal already shifts the target's
     former slot down by one), locked in by RED→GREEN tests (2 failed / 37 passed → 39 passed).
  Note the verifier's own claim that "desktop/mobile checks failed" was partly baseline: the
  `PUBLIC_APP_VERSION` error is pre-existing, per `odd/tasks/track-editor-ui.md`.
- **W2 COMPLETE (worker `mumvn3kz-3-c8z0`, strict TDD active).** `TracklistColumnPref { id, visible }`
  (camelCase) + `AppSettings.tracklist_columns: Vec<_>` defaulting to an **empty** vec — Rust stays
  agnostic about column ids and lets the frontend supply defaults — plus
  `parse_tracklist_columns(Option<String>)` in `src-tauri/src/services/settings.rs`, called from
  `get_settings` next to `ignored_device_ids`. Malformed JSON degrades to empty, never errors.
  `crate::models::settings` is already glob-re-exported, so no barrel edit was needed.
  Gates: `cargo test --features desktop` RED (2 unresolved-import errors) → GREEN **240 passed /
  0 failed**; `cargo clippy --features desktop -- -D warnings` pass; `cargo check --release
  --features desktop` pass; `rustfmt --check` clean on both touched files; `git diff --check` clean.
  `SYNCED_SETTING_KEYS` and the settings table schema untouched (key/value row, no migration).
  Diff: +10 / +53.
- **Known baseline, not candidate defects:** `cargo fmt --check` at repo level fails on 3 files we
  never touched (`src-tauri/build.rs`, `src/main.rs`, `src/services/library/metadata_update.rs`,
  17 diffs) — those files are unmodified in `git status`, so the drift predates this feature.
  Do not "fix" them inside this work unit.
- **Toolchain note:** the pinned toolchain is `nightly-2026-02-19` (AGENTS §3) via
  `src-tauri/rust-toolchain.toml`, so run bare `cargo fmt` / `cargo clippy`, never `cargo +nightly fmt`:
  the latter auto-installed the latest nightly and reported a *different* formatting set. The
  `aarch64-apple-ios` compile gate cannot run in this environment — the target's std is absent, so
  `cargo check --target aarch64-apple-ios --no-default-features --features mobile` fails inside
  `memchr` and `libc`. Mobile CI must cover it; this feature adds no `#[cfg]` and no new Rust
  dependency, so the risk is low but UNVERIFIED.

## Commit evidence

Four work-unit commits on `dev` (user chose grouped commits at close, no feature branch, nothing
pushed, no PR opened). Base was `f003473`.

| Commit | Work unit | Size |
| --- | --- | --- |
| `83b5151` feat(tracklist): add column registry and layout helpers | W1 | 7 files, +603/−11 (398 of it tests) |
| `cab761e` feat(settings): persist the tracklist column layout | W2 + W3 | 4 files, +80/−1 |
| `1fc71dc` feat(i18n): add tracklist column and column-menu labels | W7 i18n | 15 files, +150 |
| `14632bb` feat(tracklist): render, pick and reorder columns from the saved layout | W4 + W5 + W6 + docs | 6 files, +471/−151 |

Ordering is deliberate: i18n lands **before** the UI because `$translate` renders the raw key when a
message is missing, so no commit ever shows a user a `library.columns.*` string.

odd/tasks/tracklist-column-config.md` and `odd/README.md` were committed separately as
`a78023f chore(odd): record tracklist column feature tracking` — `odd/` is versioned on this
integration branch, but workflow tooling never rides inside a feature commit (AGENTS.md §2).
Tree is clean at `a78023f`. Verified green on the final committed tree: vitest 43/43, `cargo test
--features desktop` 240/240, clippy `-D warnings` clean, `check:svelte` baseline-only
(pre-existing `PUBLIC_APP_VERSION` error + 2 `LibraryTab` warnings), `check:svelte:mobile` 0/0,
`lint:check` exit 0, prettier exit 0, `git diff --check` clean.

## Manual verification still owed

No display in this environment, so `yarn dev` was never run. A human must confirm:

1. Fresh launch shows today's 9 columns, unchanged, filling the width.
2. Right-click the header → menu opens, library/playlist empty-space menu does **not** also open.
3. Toggle Album/Origin/Bitrate/Date Added → checks flip live, menu stays open, selection survives.
4. Drag BPM left of Title → marker on the left edge → drop reorders → restart the app → still there.
5. Hide Album, then drag across the gap it leaves → Album keeps its slot, does not jump to the end.
6. Click a header without dragging → still sorts and toggles direction.
7. Reset to default columns → back to the original layout.
8. Window resize during a drag (cached-rect staleness) and a pointer released while the window is
   blurred (now handled by the `onblur` reset, unverified at runtime).
