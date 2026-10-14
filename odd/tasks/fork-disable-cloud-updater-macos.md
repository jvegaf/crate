# Feature: fork-disable-cloud-updater-macos

Goal: trim this fork down to what it actually uses, **temporarily and reversibly**. Three things are
turned off: cloud sync, the auto-updater, and the macOS/iOS build+release path. Nothing is deleted —
each disablement is a named switch or a commented-out line, so restoring any of them is a one-line
change with no archaeology.

Branch: `dev` (the fork's integration line). This is fork tooling, not a feature for upstream: none of
it may travel in a PR to `blackboxaudio/crate`.

## Context

- The fork no longer follows upstream. `1720eae` had already made the updater inert (no `endpoints`,
  placeholder `pubkey`, no startup/hourly check, inert About button) and gated the GCS publishing steps.
- `43a74f6` already added `/data/` to `.gitignore` on `dev` (an exported Traktor `collection.nml` embeds
  absolute user paths). Confirmed with `git check-ignore`; the earlier warning was only that the
  `traktor-nml-import` branch predates that commit.
- The user runs this fork from Windows (the Traktor fixture's `VOLUME="C:"`), so macOS and iOS builds
  have no consumer. Their CI jobs were burning macOS runner minutes with nothing to verify.
- Cloud sync has no dedicated feature flag today: "disabled" is derived from the config being
  absent/blank (`services/cloud_sync/config.rs`), which makes `CloudSyncState::is_available()` false and
  the frontend `isSyncAvailable` false.

## Tasks

- [x] C1 Rust cloud switch: `CLOUD_SYNC_ENABLED = false` in `services/cloud_sync/config.rs`, early-return
      `None` from `load_cloud_config`
- [x] C2 TS cloud twin: export the same constant from `shared/stores/cloudSync.ts`
- [x] C3 Stop the desktop cloud startup (load/poll/override listener) in `hooks/useAppSetup.ts`
- [x] C4 Stop the mobile cloud startup in `apps/mobile/src/routes/+layout.svelte`
- [x] U1 `UPDATER_ENABLED = false` in `apps/desktop/src/lib/stores/updater.ts`
- [x] U2 Gate the About tab's Updates section on it
- [x] U3 Gate the `UpdateModal` mount on it
- [x] U4 Drop `updater:default` from `src-tauri/capabilities/desktop.json`
- [x] M1 `ci.build.yml`: drop the macOS matrix entry + disable the `ios` job
- [x] M2 `test.release.yml`: drop the macOS matrix entry
- [x] M3 `cd.release.yml`: drop the macOS matrix entry + disable the `ios` job
- [x] X1 (unplanned) Fail the `#[ignore]d` `appcheck::tests::debug_token_probe` fast with the real reason:
      it asserts `CLOUD_SYNC_ENABLED` so a manual run does not report the misleading
      "cloud_sync.config.json present" message once the switch is off
- [ ] Merge `traktor-nml-import` into `dev` (10 commits, see the divergence below)
- [ ] Bump the minor version (0.4.0 → 0.5.0) and tag `v0.5.0`

## Decisions

1. **Reversible switches over deletions.** Cloud and updater get named `*_ENABLED = false` constants
   with a comment saying the revert is a one-line flip. CI gets the macOS matrix entries commented out
   with a marker, and the `ios` jobs get `if: false`.
2. **Cloud is switched off at the backend, not in the UI.** Turning `load_cloud_config` into `None`
   yields phase `disabled`, so the settings tab, the toolbar indicator and the mobile chip hide
   themselves — no UI edits needed to hide them. The explicit frontend gate exists only to stop the
   always-on polling and the event listeners.
3. **Do NOT remove shared cloud infrastructure.** `pipeline::{buckets,dirty}` and `hlc` are imported by
   ~20 non-cloud features (library scan/import/update/relocation, playlists, cues, tags, follow,
   collection, discovery, the Traktor import itself) and stamp `_hlc`/`sync_dirty_buckets` on every
   mutation by design. `library_roots`/`library_root_id` are also used by relocation and playback.
   Removing any of it breaks far more than sync.
4. **Do NOT touch `ci.lint.yml` or `ci.canary.yml`.** The user scoped the cut to "build and release for
   mac" plus iOS; `rust-format`, `rust-lint` and `rust-canary` keep running on macOS runners.
5. **Do NOT remove the Rust updater plugin or the `plugins.updater` config block.** `1720eae` recorded
   that a missing `plugins.updater` object fails at startup (the plugin's config has no serde default),
   and removing `mod updater`/the comparator trips clippy `-D warnings` (a CI gate). Dropping
   `updater:default` from the capability makes a stray IPC call fail without touching either.
6. `if: false` on the `ios` job is safe: `cd.release.yml`'s `release` job already gates on
   `(needs.ios.result == 'success' || needs.ios.result == 'skipped')`, and its `files:` globs already
   tolerate absent artifacts (the same shape already exists for Android).

## Constraints

- `cargo test --features desktop` is mandatory; clippy runs with `-D warnings`.
- Never mix `odd/` into a pure feature branch (AGENTS.md §1.2) — this work is on `dev`.
- The version bump must use the repo tooling (`yarn bump minor`) so `package.json`, `Cargo.toml`,
  `tauri.conf.json`, `tauri.staging.conf.json` and `tauri.dev.conf.json` stay in sync.
- Generated artifacts in English.

## Evidence log

Baseline for every run below: `dev` @ `8bd8975`, index == worktree, nothing committed.

**Gates (2026-10-14) — 9/9 PASS**

| Gate | Command | Result |
| --- | --- | --- |
| Prettier | `yarn format:check` | PASS — `All matched files use Prettier code style!` |
| ESLint | `yarn lint:check` | PASS — exit 0 |
| svelte-check (desktop) | `yarn check:svelte` | PASS — 0 errors, 0 warnings |
| svelte-check (mobile) | `yarn check:svelte:mobile` | PASS — 0 errors, 0 warnings |
| Vitest | `npx vitest run` | PASS — 9 files, 124 tests |
| Vite build | `yarn build:vite` | PASS — built in 8.01s |
| rustfmt | `cargo +nightly-2026-02-19 fmt --check` | PASS — empty output |
| clippy (CI-exact) | `cargo +nightly-2026-02-19 clippy --features desktop -- -D warnings` | PASS — 0 warnings; **cache-hit** (1.11s), so this re-lint is not a cold rebuild |
| Rust tests | `cargo test --features desktop` | PASS — 519 passed, 0 failed, 10 ignored (+1 doc-test ignored) |

**Boot smoke test (2026-10-14) — the real gate for the U4 capability change**

`broadwayd :5` + `GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 timeout 420 yarn dev` (incremental
release rebuild, 1m44s). BOOTED. Verbatim evidence from the run log:

- `INFO crate_lib] Database path: "/home/th3g3ntl3man/.local/share/com.bbx-audio.crate.dev/crate.db"`
- `INFO crate_lib::services::cloud_sync::config] cloud_sync: disabled by CLOUD_SYNC_ENABLED; cloud sync unavailable`
  — C1 proven at runtime, not just by reading the constant
- `grep -niE "not allowed|forbidden|permission denied|updater:default|failed to initialize plugin"` → no
  matches: dropping `updater:default` produces no ACL failure, and nothing calls the updater IPC anymore
- no `Fatal:` / `failed to run Tauri application` / `panicked at`

**Workflow YAML (parsed, not eyeballed)** — all three files load; `build.matrix` reduces to
`windows-latest` only in `ci.build.yml`, `test.release.yml` and `cd.release.yml`; `jobs.ios.if` parses
as the boolean `false` in `ci.build.yml` and `cd.release.yml`. `cd.release.yml`'s `release` job guard
does accept `needs.ios.result == 'skipped'` (read at `cd.release.yml:564`), so decision 6 holds.

**Residual risk**

- clippy was a cache hit; the Rust edits are four lines in `config.rs` plus a test-only `assert!`, so a
  cold re-lint is not worth a `cargo clean` on a 2.8 GB release target.
- Mobile is now unguarded in CI (`ci.build.yml`'s `ios` job is `if: false`) and the iOS cross-compile
  check cannot run on the user's Windows box. Mobile breakage will not be caught until the job returns.

## Commit evidence

All three code commits are on `dev`, verified with `git show --stat` to touch only the intended files:

| Commit | Subject | Files | Task items |
| --- | --- | --- | --- |
| `6bfeced` | chore(fork): disable cloud sync behind a one-line switch | 5 (+48/−15) | C1, C2, C3, C4, X1 |
| `bdbead3` | chore(fork): disable the auto-updater and drop its capability | 4 (+25/−16) | U1, U2, U3, U4 |
| `47676ea` | chore(fork): drop the macOS and iOS build and release jobs | 3 (+15/−5) | M1, M2, M3 |

The `docs(odd)` commit that carries this file follows the three above. Each commit body names the exact
revert. Nothing is pushed: `origin/dev` is 5 commits behind `dev` before this work, and the merge and the
release commit add more.

## Left out of scope (deliberate)

- `odd/` scaffolding stays on `dev`, never on a feature branch (AGENTS.md §1.2). Nothing here is a
  candidate for a PR to `blackboxaudio/crate`.
- Nothing was deleted, so every item above is a one-line revert.
- `scripts/tag.sh` must NOT be used for the version bump — it chains a push to `origin` (memory:
  `crate/fork-release-flow`). Replicate its steps 1–4 by hand.
