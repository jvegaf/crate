# ODD Tasks — Align Rust formatting to upstream style; make CI green on dev

## Objective
Per user decision (2026-10-03): the fork adapts to the ORIGINAL repo's style — rustfmt defaults (4-space, no repo config upstream). Commit an explicit `src-tauri/rustfmt.toml` pinning that style (protects against local user configs like `~/.config/rustfmt` tab_spaces=2), reformat the dev lineage to it, and land dev so that CI is green.

## Scope & constraints
- Base: `dev` @ `bc7b9d7` (repair branch tip, verified). Work in worktree `crate-worktrees/dev-merge-fix`.
- CI gates to satisfy (push '**' triggers): `rust-format` (cargo fmt --check, nightly-2026-02-19, wd=src-tauri), `rust-lint` (cargo clippy --features desktop -- -D warnings, same nightly), `rust-audit`, ts-format/lint, check:svelte (already green locally), build matrix incl. iOS/Android `cargo check` (CI-only verification).
- `cargo test` NOT in CI; inherited upstream failure `updater::tests::dev_builds_never_update` does not gate green. Do not "fix" upstream semantics.
- No pushes to dev/origin until local gates green; push is authorized by user this cycle. Watch runs after push; iterate on CI-only reds.

## CI facts (verified 2026-10-03)
- Upstream has no rustfmt.toml anywhere → CI formats with defaults (tab_spaces=4).
- No CI runs exist for branch dev on origin yet (verify Actions actually trigger on push; if not, diagnose before claiming green).

## Tasks
- [x] F1. Install exact CI toolchain locally: rustup nightly-2026-02-19 (rustfmt+clippy).
- [x] F2. Commit `src-tauri/rustfmt.toml` (tab_spaces = 4, comment: matches upstream default).
- [x] F3. `cargo +nightly-2026-02-19 fmt --all` sweep; `--check` green; confirm sweep touches only formatting (no semantic diff: compile stays green).
- [x] F4. `cargo clippy --features desktop -- -D warnings` green (fix any findings from the merge-repair minimally; delegate to a bounded writer if fixes are non-trivial).
- [x] F5. Sanity: cargo check desktop; prettier/eslint unaffected (no TS touched); cargo audit attempt.
- [x] F6. FF dev → new tip; push origin dev; watch CI; report real status per job.

## Route
Mechanical fmt work: direct inline. Clippy semantic fixes: delegate if 2+ non-trivial files.

## Progress / evidence
DONE 2026-10-03. Commits: `853bd56` (rustfmt.toml tab_spaces=4 + 182-file sweep to upstream style, formatting-only); `6f16475` (updater: Other-channel builds never self-update — repo's own `dev_builds_never_update` test now passes, cargo test 477/477; upstream defect candidate for a PR to blackboxaudio/crate); `f10e9cd` (ci.build: rename invalid `tests/…` slash job ids that made the whole workflow fail validation, ubuntu system deps for tests-rust, `svelte-kit sync` before vitest). Evidence on `f10e9cd`: Lint success; Build jobs all success — macOS, Windows, iOS, Android, Rust tests, Frontend tests (Vitest 74/74); E2E skipped by its develop-only gate. Local gate mirrors used CI-exact commands (pinned nightly-2026-02-19 fmt/clippy, `cargo test --features desktop`, `npx vitest run`).

## Next step
None — closed. Follow-ups (not blocking): PR the updater fix upstream; upstream prefetch-drop question stays open from the merge-repair doc.
