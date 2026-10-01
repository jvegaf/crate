# Tracklist "View on {store}" context menu item

Follow-up to `odd/tasks/track-store-url-woar.md` (commit b681257, `Track.url` persisted via the WOAR
frame and editable in `TrackMetadataModal`).

## Objective

Add one dynamic entry to the track-list context menu that opens the track's store page in the
system browser. The label is derived from the URL domain: "View on Beatport" / "View on Bandcamp" /
"View on Traxsource"; a URL from any other host falls back to a generic "View in store". The item is
hidden when the track has no URL or when more than one track is selected.

## Problem / Why

`Track.url` is only reachable through the metadata modal today. Jumping to the store page where the
track was bought should be one right-click away.

## Scope

- `shared/utils/storeUrl.ts` (new): pure `getStoreName(url)` helper mapping a URL to a known store
  display name (Beatport / Bandcamp / Traxsource) or `null`. Hostname match accepts bare hosts and
  subdomains (`www.beatport.com`, `artist.bandcamp.com`), tolerates a missing scheme, returns `null`
  on unparseable input instead of throwing.
- `shared/utils/index.ts`: export the new module from the barrel (otherwise unreachable via
  `$shared/utils` — documented trap, AGENTS.md §7).
- `shared/utils/storeUrl.test.ts` (new): vitest unit tests for the mapping table.
- `apps/desktop/src/lib/components/library/TrackContextMenu.svelte`: new item inside the existing
  `selectedTracks.length === 1` block, after `reveal-in-explorer`; action calls `openUrl` from
  `@tauri-apps/plugin-opener` directly (sanctioned exception, precedent
  `DiscoveryTrackContextMenu.svelte:48,56` and `TrackMetadataModal.svelte:421`).
- i18n: `contextMenu.viewOnStore` (with the `{store}` ICU placeholder) and `contextMenu.viewInStore`
  in all 15 locale files.

Out of scope: backend/IPC changes, DB, discovery menus, multi-selection submenu, toasts on failure,
repairing pre-existing locale drift (e.g. `es.json` missing `contextMenu.editMetadata`).

## Constraints

- Menu closes itself after `item.action()` (`ContextMenu.svelte:105-112`) — do not call `closeAll`.
- Follow the file's existing convention for single-track items: hide the item, do not disable it;
  read `selectedTracks[0]`.
- Error handling matches repo precedent: `openUrl(...).catch(() => {})`.
- `{store}` must stay byte-exact in every locale — translating the placeholder breaks svelte-i18n
  interpolation.
- `shared/` is compiled for mobile: the helper must stay pure TS, no desktop API.
- Prettier: tabs, no semicolons, single quotes. Artifacts/comments in English.
- Keep the diff small (~400-line review heuristic).

## Route: delegated direct (writer trigger: 2+ non-trivial files)

Evidence: mapping trigger already fired (a narrow explorer mapped the orchestrator chain, i18n,
icons and test setup across 10+ files). One bounded writer owns helper + test + menu + 15 locales.
SDD not used: scope, placement and decisions are settled, nothing needs a durable proposal/spec.

TDD mode: `off` (no strict-TDD config or `sdd-init` record for this project). Ordinary checks apply;
the helper still gets real unit tests because a vitest runner and sibling fixtures exist.

## Tasks

- [x] T1 — `shared/utils/storeUrl.ts` + barrel export. Checks: pure module (0 imports),
  `getStoreName` + `StoreName` exported, `extractHost` private; trims, prefixes `https://` when no
  scheme, lowercases hostname, `try/catch` → `null`; match rule `host === domain ||
  host.endsWith('.' + domain)` so `beatport.com.evil.io` and `notbeatport.com` are rejected.
- [x] T2 — `shared/utils/storeUrl.test.ts`. Checks: `npx vitest run shared/utils/storeUrl.test.ts`
  → 11 passed (all behavioral).
- [x] T3 — `TrackContextMenu.svelte`. Checks: `git diff` shows a pure insertion at `:128-139` inside
  the `selectedTracks.length === 1` block, after `reveal-in-explorer`, before `reveal-divider`;
  hidden (not disabled) when URL blank (`url?.trim() ?? ''`); icon `external-link` (exists in
  `Icon.svelte`); `openUrl(storeUrl).catch(() => {})`; no `onClose`/`closeAll` added.
  `yarn check:svelte` → parity.
- [x] T4 — i18n in all 15 locales. Checks: node script over every file — valid JSON, no duplicate
  keys, both keys under `contextMenu` right after `viewInFileManager`, `{store}` byte-exact in all
  15 (incl. prefix placement in ja/ko/tr).
- [x] T5 — Full gates (see Verification evidence). `src-tauri/` untouched, so no cargo gate applies.
- [x] T6 — RDD: SKIPPED by policy. `gentle-ai review mode status` reads **off** (decided by
  clone_local) → no review started, no consent prompt, no lineage. Read-only risk probe returned
  `unassessable` (untracked files undeclared) → treated as `high`, never lowered; proof rests on the
  functional gates plus an independent fresh-context verifier.
- [x] T7 — Work-unit commit. User explicitly chose to commit on `dev`, as with the WOAR feature
  (overriding the `{issue-number}-{slug}` branch convention, which AGENTS.md §1.3 lets the owner
  waive). SHAs are recorded at the bottom of this file.

## Verification evidence

Writer (2026-10-01) + orchestrator re-run and independent verifier (2026-10-02), repo root:

- `npx vitest run` → **4 files / 62 tests passed** (final, after the orchestrator added the
  whitespace-padding case; 61 before it). Store suite itself: 11 passed.
- `yarn check:svelte` → `found 1 error and 2 warnings in 2 files` — IDENTICAL to the pre-edit
  baseline (pre-existing `+layout.svelte:12:11 PUBLIC_APP_VERSION` + `LibraryTab.svelte:279` a11y).
- `yarn check:svelte:mobile` → 0 errors / 0 warnings (baseline parity).
- `yarn lint:check` → exit 0. `yarn format:check` → "All matched files use Prettier code style!".
- Locale integrity: all 15 JSON parse; both keys present; `{store}` exact everywhere.
- Scope: `git diff --name-only | grep ^src-tauri/` → 0 matches. Untracked additions are exactly
  `shared/utils/storeUrl.ts`, `shared/utils/storeUrl.test.ts` (plus this ODD doc).
- `yarn check:cargo` / clippy / rustfmt: NOT applicable — zero Rust changes.

INDEPENDENT VERIFICATION (fresh verifier, read-only, VERDICT: **PASS**, 0 defects, 0 minor):
1. Placement/gating PASS (pure insertion, hidden not disabled, no manual close, unique item id).
2. **Real-world URL coverage PASS**: 23 shapes evaluated against the actual helper via a /tmp scratch
   eval, taken from the real producers — `beatport.rs:381,383`, `bandcamp.rs:153`
   (`{artist}.bandcamp.com/track/…`), label subdomains, `traxsource.rs:171` (regex requires
   `/track/\d+/`), scheme-less legacy WOAR values per `import.rs:473-488`, trailing slash + UTMs from
   the verbatim WOAR roundtrip (`file_tags.rs:166`), and unknown hosts. Zero misclassifications.
   Discovery never writes `Track.url` (`release_crud.rs:444` targets `discovery_releases`), so no
   foreign shapes leak into the field.
3. Robustness PASS: control chars, userinfo decoy `beatport.com@evil.io`, path decoy, lookalikes →
   `null` without throwing (no throw across 23 adversarial probes).
4. i18n + interpolation syntax PASS (matches the proven `trackController.ts:139` form; a plain ICU
   argument cannot throw at format time).
5. Mobile safety PASS (zero imports; `check:svelte:mobile` 0/0).
6. Gates re-run by the verifier → same green/parity results; git status unchanged by its run.

## Follow-ups (informational, not defects)

- `DiscoveryTrackContextMenu.svelte:54-56` opens `release.url` with the same icon but a static
  label — the natural next candidate for `getStoreName` reuse (it would need its own keys, since
  discogs/soundcloud/youtube dominate there).
- Protocol-relative stored URLs (`//artist.bandcamp.com/…`) classify correctly but would not launch a
  browser through `openUrl`; unreachable today because no producer emits that shape.

## Delivery strategy

`ask-on-risk` (carried over from the previous feature). **Measured diff: 17 tracked files, +45, plus
89 lines in two new files = ~134 authored changed lines** — far under the ~400 review heuristic, so
no PR split is required. Running count for this feature: 134.

## Work-unit commit evidence

- feat commit: **f4ab43d** `feat(track): open the track store page from the context menu` — 19 files,
  +134/−0 (17 tracked modifications + `shared/utils/storeUrl.ts` and its test), on `dev`.
- docs commit: this file, landed immediately after as `docs(odd): record the tracklist store
  context-menu work unit`. Scaffolding stays on the fork's integration line so a future pure feature
  branch replays upstream without noise (crate-upstream-contribution skill).
- Rollback boundary: reverting f4ab43d removes the menu entry, the helper, its tests and the 30 locale
  keys. No backend, DB or IPC surface is involved, so nothing else is affected.
- Runtime harness: **N/A** for the browser handoff itself — `openUrl` is a Tauri plugin call covered by
  typecheck plus four existing precedents, not by a headless test. Manual confirmation: `yarn dev` →
  right-click a track tagged from a store.

## Status: CLOSED

All tasks done, gates green, independent verification PASS with zero defects.

## Next step

Optional follow-up, not started: reuse `getStoreName` in `DiscoveryTrackContextMenu.svelte:54-56`,
which still opens `release.url` with a static label.
