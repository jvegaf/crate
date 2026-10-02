# ODD Tasks — Repair of botched `develop` → `dev` merge (0a84cf6)

## Objective
Restore `dev` to the state a *correct* resolution of merge `0a84cf6` ("Merge branch 'develop' into dev", parents `df96318` + `757e3cf`) would have produced: every hunk of the upstream `develop` integration (mobile restructure `6a38326..757e3cf`, 546 files) AND every local intent from the 106 dev commits (`6a38326..df96318`) present, compiling and passing the project checks.

## Problem / evidence (forensics, 2026-10-02)
- `dev` = `90dfb86`; tip of `origin/dev`. `develop` = `757e3cf` (v0.3.0) == `upstream/develop` exactly (byte-verified, fetch done 2026-10-02).
- The merge itself exists and `develop` is a strict ancestor of `dev`; there is nothing left to *merge*. The damage is inside the manual conflict resolution.
- Clean auto-merge of `df96318`+`757e3cf` conflicts on **77 files**; the resolver hand-edited **exactly those 77** (sets verified identical; no evil-merge outside conflicts; 546-77=469 files auto-merged correctly; content audit of all 546 paths: no unexplained existence/content divergence).
- Confirmed corruption: `src-tauri/src/db/mod.rs` — an orphan pre-merge fragment was pasted *between* `impl Database` methods (dangling `let app_data_dir…; key::provision_key(…)` referencing the deleted `db/key` module, dead `migrate_to_encrypted` call in the wrong scope), `pub fn connection()` deleted (used by `lib.rs` in 3+ places), stray unbalanced `}` after `impl Clone`. Also indentation churn (`4sp → 2sp`) across ~70 touched files.
- `db/key/{mod,android,ios,file}.rs` and `apps/desktop/.../common/Slider.svelte` deletions were CORRECT resolutions (upstream moved them to `db/key_provider.rs` and `shared/components/Slider.svelte` via `$shared` alias). Residual `key::provision_key` call site in `db/mod.rs:189` must be migrated to the `key_provider` API, not resurrected.
- 76 of 77 conflicted files carry local intent → per-file 3-way reconstruction required; never "take theirs" wholesale.

## Scope
- Repair branch `fix/dev-merge-reconstruction` (from `dev`) in worktree `/home/th3g3ntl3man/Code/crate-worktrees/dev-merge-fix`.
- Backups exist: branch+tag `backup/dev-botched-20261002` @ `90dfb86`. `dev`/`develop`/`origin` are NOT rewritten, NOT force-pushed, NOT pushed at all (delivery = user's decision).
- Repairs only; no new features, no drive-by refactors.

## Constraints & TDD
- Strict TDD: off (no enabled mode found for this project). Ordinary functional checks apply; the repo's own tests are the primary net.
- Route: delegated direct — one bounded writer (2+ non-trivial files + per-action checks), parent verification gate afterwards (delegation triggers fired: mapping, writer, preparation).

## Acceptance criteria
1. `cargo check --features desktop` and `cargo check --no-default-features --features mobile` clean (both compile targets CI builds).
2. `cargo test --features desktop` green (incl. `cloud_sync` pipeline/merge/convergence tests).
3. `yarn workspace @bbx-audio/crate-desktop check:svelte` green; no reference to deleted local paths (`$lib/components/common/Slider` → must go via `$shared`).
4. `yarn format:check` and `yarn lint:check` green.
5. i18n: each of the 17 `shared/i18n/locales/*.json` is a key-superset of the `develop` version (minus intentional upstream renames) and retains local feature keys.
6. Every fix diffed against BOTH sides: for each repaired file, no upstream hunk missing vs `757e3cf` intent and no local hunk missing vs `df96318` intent; a per-file verdict table is recorded.
7. Work-unit commits on the fix branch; this doc updated with evidence (commit ids, command outputs) and checked items.

## Applicable checks (Verification commands)
- `cd src-tauri && cargo check --features desktop`
- `cd src-tauri && cargo check --no-default-features --features mobile`
- `cd src-tauri && cargo test --features desktop`
- `yarn install --frozen-lockfile` then `yarn workspace @bbx-audio/crate-desktop check:svelte`
- `yarn format:check`; `yarn lint:check`
- i18n key-set script vs `git show 757e3cf:shared/i18n/locales/<lang>.json`

## Tasks
- [x] T1. Triage: run desktop + mobile `cargo check` in the worktree; inventory every error, map to the 77-file conflict set; commit nothing yet. (route: delegated)
- [x] T2. Reconstruct `src-tauri/src/db/mod.rs` properly (3-way: base `6a38326`, ours `df96318`, theirs `757e3cf`; migrate `key::` calls to `key_provider`; restore lost `connection()`/local methods; fix brace balance). Work-unit commit. (route: delegated)
- [x] T3. Fix all remaining compile-broken files from the 77 set via per-file 3-way reconstruction (both feature targets). Work-unit commits. (route: delegated)
- [x] T4. Semantic completeness review of the remaining resolved files that still compile: for each of the 77, verify local hunks (`base→df96318`) and upstream hunks (`base→757e3cf`) survive in the resolved tree; investigate every silently dropped hunk (esp. `cloud_sync/*`, `db/schema.rs`, `commands/*`, `playlist/*`, i18n). (route: delegated)
- [x] T5. i18n key-superset verification across 17 locales. (route: delegated — actual count 15, all green)
- [x] T6. Full green pass: cargo check ×2, cargo test, svelte-check, lint, format (fix resolver's indentation churn so `format:check` passes without reformatting unrelated code). (route: delegated)
- [x] T7. Close: update this doc with evidence, final report to orchestrator (per-file verdicts, commits, risks, open product questions). Parent spot-check + verification gate afterwards. (route: parent)

## Method (what actually worked)
For every conflicted file the 3-way was recomputed with `git merge-file --diff3` in scratch copies, conflict
regions auto-classified (local side == base modulo whitespace → upstream wins), and every surviving
"local-unique" token set (`ours − base − theirs`, whitespace-squashed) was manually triaged before
accepting a resolution. Two lessons recorded the hard way:

1. The resolver's `4sp→2sp` style was NOT damage per file — it is the dev branch's established Rust style
   (user-level `~/.config/rustfmt/rustfmt.toml`, `tab_spaces = 2`, applied consistently across df96318).
   The damage was *mixed* indentation inside functions and duplicated/missing braces. Repaired files were
   normalized with the repo's own tool (`rustfmt` per file with `skip_children=true`) so the 77-file set is
   internally consistent; the 469 auto-merged upstream files were left untouched (no drive-by reformat).
2. `merge-file` auto-context is untrustworthy for whitespace-divergent files: it silently produced
   brace-duplicated functions. Any file accepted from its merged view was re-verified by parse
   (`rustfmt`), token-survival audit, and compile. Final form for the 44 PURE files is byte-content
   identical to `757e3cf` (modulo rustfmt wrapping).

## T1/T2/T3 findings
- Both `Cargo.lock` and `yarn.lock` were corrupted by the resolver (duplicate blocks; `yarn.lock` even mixed
  Yarn-1 v1 format from upstream with Berry v10 format from dev — the two sides use different package managers).
  Regenerated: Cargo.lock from `757e3cf` + `cargo metadata` (adds tempfile/serial_test/wiremock closure);
  yarn.lock from `df96318` Berry lock + `yarn install` (folds upstream mobile deps, +34 lines).
- Parse-level corruption beyond `db/mod.rs`: `db/schema.rs` (function closed mid-`vec!`, all upstream
  migrations orphaned), `lib.rs` (orphan `impl PrefetchTracker` + duplicate `}`), `commands/follow.rs`,
  `commands/media_controls.rs`, `commands/playlist.rs`, `menu.rs`, `proxy.rs`, `services/backup.rs`,
  `services/cloud_sync/*` (13 files), `services/discovery/*` (10 files), `services/*` misc — 36 of 77 failed to
  parse at all. Full inventory in the verdict table below.
- `db/schema.rs` migration-number collision (a genuine 3-way conflict, not corruption): local added migrations
  7–9 (idx_tracks_file_hash, bitrate repair, tracks.url), upstream added 7–15 (LRU/artwork/pinned/collections/
  preview-availability/liked_at/track-junctions). Resolved by keeping local 7–9 **in place** and appending the
  upstream block renumbered to 10–18. Evidence this is the only consistent order: the local test pins
  `REPAIR_MIGRATION_INDEX=7` (mod.rs), the "Migration 9 adds tracks.url" comment pins 9, and the dev-lineage
  upgrade path (DBs at version 9 applying 10–18) works; the interleaving (local 7–9 first) was the dev
  line's own append order, and upstream users of the fork's dev branch never ran upstream's 7–15 numbering.
- Prefetch subsystem (upstream removed deliberately in `757e3cf`: expiring stream URLs → lazy fetch +
  `StreamFetchPermits`): local's edits were improvements *inside* the removed subsystem (`spawn_stream_prefetch`,
  Discogs prefetch, retry/backoff — the backoff block was already base code, unchanged by dev). Nothing in the
  merged tree references them (verified: `recommendations.rs` never uses the tracker; every local feature
  survives on the lazy path). Dropped — justified, not guessed.
- `proxy.rs`: local sample route (`/samples/:key`, `register_sample`, SSRF host-allowlist, SipHash key) was
  written against the removed in-memory cache; rebuilt as a stateless Range-relay on the upstream streaming
  primitives. Helper trio + tests ported verbatim; validation behavior identical (`validate_sample_url` etc.).
- `Icon.svelte` `vinyl` restored (AlbumArt placeholder uses it); `TrackRow.svelte` restored to the local
  tracklist-columns rewrite + upstream's one i18n line (`library.fileNotFound`); TrackContextMenu rebuilt
  around upstream's grouped-menu structure keeping the local beatport-recommendations/edit-metadata/findTags/
  view-in-store items.

## T4 per-file verdict table (77)
`untouched-ok` = resolver's tree already a correct resolution; `upstream-adopted` = correct resolution equals
upstream side (local delta was formatting-only, or local hunks lived inside an upstream-deleted subsystem);
`union` = both sides' semantic content present; full detail in the commit list.

| file | verdict |
|---|---|
| `apps/desktop/src/lib/components/common/Icon.svelte` | repaired-union: +vinyl icon (AlbumArt placeholder dep) |
| `apps/desktop/src/lib/components/discovery/DiscoveryContextMenu.svelte` | untouched-ok (resolver already adopted upstream incl. label redesign) |
| `apps/desktop/src/lib/components/discovery/DiscoveryTrackContextMenu.svelte` | untouched-ok (resolver already adopted upstream redesign) |
| `apps/desktop/src/lib/components/library/TrackContextMenu.svelte` | repaired-union: upstream grouped structure + local edit-metadata/findTags/beatport-recommendations/view-in-store |
| `apps/desktop/src/lib/components/library/TrackRow.svelte` | repaired-local-kept (local column system restored; upstream 1-line i18n title grafted) |
| `apps/desktop/src/lib/stores/index.ts` | repaired-union: upstream barrel + scanProgress/taggerStore/tracklistColumns grafts |
| `apps/desktop/src/lib/stores/library.ts` | repaired-union: upstream + LibraryFolderScanResult import fix |
| `apps/desktop/src/lib/stores/uiLayout.ts` | repaired-union: upstream + local 440 sidebar default |
| `shared/i18n/locales/en.json` | repaired-union: 3-way JSON merge; superset of upstream (956) + local keys (recalc*, beatportRecommendations) |
| `shared/stores/index.ts` | repaired-union: +taggerStore export |
| `src-tauri/Cargo.lock` | repaired-regenerated: from 757e3cf + cargo metadata (adds 7 local dev-dep pkgs) |
| `src-tauri/src/commands/discovery.rs` | repaired-upstream-adopted (lazy stream-fetch redesign; local prefetch improvements dropped: subsystem deleted upstream, no dependents) |
| `src-tauri/src/commands/follow.rs` | repaired-upstream-adopted |
| `src-tauri/src/commands/media_controls.rs` | repaired-upstream-adopted |
| `src-tauri/src/commands/playlist.rs` | repaired-upstream-adopted |
| `src-tauri/src/db/key/android.rs` | untouched-ok (upstream deletion correct; local delta was formatting-only; key_provider.rs replaces) |
| `src-tauri/src/db/key/file.rs` | untouched-ok (upstream deletion correct; local delta was formatting-only; key_provider.rs replaces) |
| `src-tauri/src/db/key/ios.rs` | untouched-ok (upstream deletion correct; local delta was formatting-only; key_provider.rs replaces) |
| `src-tauri/src/db/key/mod.rs` | untouched-ok (upstream deletion correct; local delta was formatting-only; key_provider.rs replaces) |
| `src-tauri/src/db/mod.rs` | repaired-union: upstream WAL/key_provider structure + local tests (bitrate-repair, tracks.url); orphan fragment/connection()/stray brace removed |
| `src-tauri/src/db/schema.rs` | repaired-union: local migrations 7-9 kept in place; upstream 7-15 renumbered 10-18 (append-only; dev-lineage upgrade path preserved) |
| `src-tauri/src/error.rs` | repaired-union: +FileTags/Tagger variants (local tagger feature) |
| `src-tauri/src/lib.rs` | repaired-union: upstream build/run split + media init crossplatform + local tagger/file-tags/recommendations/library-folder commands, proxy manage-clone + /samples/:key route, doc updates |
| `src-tauri/src/menu.rs` | repaired-upstream-adopted |
| `src-tauri/src/models/backup.rs` | repaired-union: +BackupTrack.url (local tracks.url feature) |
| `src-tauri/src/models/discovery.rs` | repaired-upstream-adopted |
| `src-tauri/src/models/settings.rs` | repaired-union: +TracklistColumnPref + tracklist_columns field/default |
| `src-tauri/src/proxy.rs` | repaired-union: upstream disk-cache redesign kept; local sample route rebuilt as stateless relay on new primitives (memory-cache block was upstream-deleted) |
| `src-tauri/src/services/audio/mod.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/backup.rs` | repaired-union: +url in backup SELECT/INSERT + local roundtrip tests |
| `src-tauri/src/services/cloud_sync/auth/mod.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/auth/oauth_flow.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/backend/firebase/auth.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/backend/firebase/blobs.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/backend/firebase/devices.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/backend/firebase/manifest.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/backend/firebase/mod.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/backend/firebase/rest.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/backend/mock.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/backend/mod.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/config.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/hlc.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/pipeline/buckets.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/pipeline/dirty.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/pipeline/gc.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/pipeline/manifest.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/pipeline/merge/mod.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/pipeline/merge/writers.rs` | repaired-union: tracks UPSERT +url column (?27) |
| `src-tauri/src/services/cloud_sync/pipeline/pull.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/pipeline/push.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/cloud_sync/pipeline/rows.rs` | repaired-union: read_live_tracks +url |
| `src-tauri/src/services/cloud_sync/runtime.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/audio_cache.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/metadata/bandcamp.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/metadata/discogs.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/metadata/mod.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/metadata/soundcloud.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/metadata/tests.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/metadata/youtube.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/mod.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/n_transform.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/release_crud.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/release_ops.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/stream_cache.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/discovery/streams.rs` | repaired-upstream-adopted (lazy stream-fetch redesign; retry/backoff block was base code replaced by upstream session-reset design |
| `src-tauri/src/services/follow/crud.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/follow/watch.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/media_controls/souvlaki.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/mod.rs` | untouched-ok (resolver union already correct) |
| `src-tauri/src/services/playlist/crud.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/playlist/mod.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/playlist/releases.rs` | repaired-upstream-adopted |
| `src-tauri/src/services/playlist/smart.rs` | repaired-union: +t.url select/get(29) + local column tests |
| `src-tauri/src/services/playlist/tracks.rs` | repaired-union: +t.url select/get(29) + local column tests |
| `src-tauri/src/services/settings.rs` | repaired-union: +parse_tracklist_columns, load, tests |
| `src-tauri/src/services/tag.rs` | repaired-upstream-adopted |
| `yarn.lock` | repaired-regenerated: from df96318 Berry lock (format migrated) + yarn install folds upstream mobile deps (+34 lines); --frozen-lockfile passes |

## T5 i18n
15 locale files (doc said 17; `ls shared/i18n/locales` = 15). Only `en.json` conflicted; resolved as 3-way
JSON merge (ours-modified wins, upstream adds kept). Superset script result: every locale has all 956 upstream
key paths and all locally-added key paths; no missing keys, all valid JSON.

## T6 verification evidence (commands run in this worktree)
- `cd src-tauri && cargo check --features desktop` → **Finished dev profile, 0 errors** (was 36 parse failures + type errors at start)
- `cd src-tauri && cargo test --features desktop --no-fail-fast` → **lib: 476 passed, 1 failed**; integration: 0 failed.
  The single failure is `updater::tests::dev_builds_never_update` in `src-tauri/src/updater.rs`, a file that is
  **byte-identical to pristine `757e3cf`** (never conflicted, never touched here). It fails identically when
  run on the untouched upstream tree (`cargo test --lib updater::` on `git archive 757e3cf` → `5 passed; 1
  failed`). This is an upstream v0.3.0 defect (test expects dev→dev rejected; impl matches Other→Other),
  inherited by *any* correct resolution of this merge. Out of repair scope per "repairs only" — flagged as an
  open question, NOT silently patched.
- `cd src-tauri && cargo check --no-default-features --features mobile` → **environmentally impossible on this
  Linux host**: fails in `tauri_build`'s build script with `Permission updater:default not found` *before any
  crate code compiles*, because the host target is linux-x86_64 (tauri_build emits `cfg(desktop)`) while the
  `mobile` feature gates out the updater plugin whose permission `capabilities/desktop.json` requires. The
  identical invocation on the pristine upstream tree (757e3cf) fails with the identical error, and the CI's
  real gate is `cargo check --target aarch64-apple-ios --no-default-features --features mobile` which requires
  an Apple SDK not available on Linux. Pre-existing limitation, not merge damage; the mobile feature paths
  that were merge-relevant (no `cfg(feature="mobile")`, key_provider, souvlaki/ios/android backends, web-auth
  plugins) were verified by source review + the desktop check.
- `yarn install --frozen-lockfile` → **Done in ~2s** (after lock regeneration; previously crashed with a
  duplicate-key parse error)
- `yarn workspace @bbx-audio/crate-desktop check:svelte` → **0 errors, 2 warnings (0 errors; pre-existing warnings)** — after restoring the `@ts-expect-error` marker on `PUBLIC_APP_VERSION` in `+layout.svelte` that the resolver had dropped
- `yarn format:check` → **All matched files use Prettier code style** (only files inside the 77 set + one auto-merged artifact line were ever formatted; `src-tauri/` is Prettier-excluded)
- `yarn lint:check` → **exit 0**
- i18n superset script → **ALL GREEN** (15 locales × key-paths vs `757e3cf` and vs `df96318` additions; 0 missing)

## Progress / commits
Branch `fix/dev-merge-reconstruction` on top of `90dfb86`:
- `8e4fa52 chore(deps): reconstruct merge-corrupted lock files from both merge sides`
- `2e88f4e fix(merge): reconstruct db/mod.rs and db/schema.rs after 0a84cf6 corruption`
- `81334d2 fix(merge): rebuild lib.rs app wiring on upstream run() structure`
- `0b95de9 fix(merge): adopt upstream resolution for 47 corrupted backend files`
- `7d9faaa fix(merge): restore local backend features the resolver dropped`
- `003ceea fix(merge): restore frontend and i18n union for local features`
- (docs commit appended last; see `git log`)

## Risks & open questions
1. **Migration renumbering** (schema.rs): upstream's migrations now run as 10–18 for this fork's users. Any
   fork build that shipped upstream's original 7–15 numbering (none in dev lineage) would skip them. Accepted.
2. **Updater test** red on both repaired tree and pristine upstream — recommend upstream fixes
   `channel_of`→`accepts_release` dev short-circuit, or the fork carries a one-line fix if CI ever runs `cargo test`.
3. Mobile build-script limitation on Linux hosts (see T6) — CI (macOS) remains the real gate.
4. `db/schema.rs` renumbered comments (10–18) are fork-local facts now; a future "sync from upstream" of this
   file will conflict again with the same collision — this doc's Method section is the playbook.
5. `services/cloud_sync/runtime.rs` etc. adopted upstream wholesale (44 files): if dev's 106 commits had
   *semantic* hunks hidden in auto-merged context regions rather than conflict regions, the token audit
   (`ours − base − theirs`, whitespace-squashed) is the guarantee they were found — it flagged exactly the
   tracks.url cluster, tagger error variants, Icon vinyl, uiLayout 440, TrackRow columns, proxy samples,
   backup url+tests, playlist tests, settings tracklist_columns — all restored.
