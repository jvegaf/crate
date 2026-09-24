---
name: crate-verification
description: The exact Crate quality gates and verification commands — format, lint, svelte-check, cargo check, clippy, rustfmt, cargo test and the mobile compile checks — plus what CI enforces and what it does not. Use before claiming work is done, when reproducing a CI failure, or when deciding which command proves a change.
---

# Crate verification

Never claim a change is verified without running the relevant commands and reporting their real
output. The table below is the source of truth; anything not listed is not a gate.

## Gate matrix (what CI actually enforces)

| CI job | Exact command | Workflow |
| --- | --- | --- |
| ts-format | `yarn format:check` | `ci.lint.yml` |
| ts-lint | `yarn lint:check` | `ci.lint.yml` |
| svelte-check | `yarn check:svelte` | `ci.lint.yml` |
| svelte-check-mobile | `yarn check:svelte:mobile` | `ci.lint.yml` |
| rust-format | `cd src-tauri && cargo fmt --check` | `ci.lint.yml` |
| rust-lint | `cd src-tauri && cargo clippy --features desktop -- -D warnings` | `ci.lint.yml` |
| rust-audit | `cd src-tauri && cargo audit` | `ci.lint.yml` |
| build (macOS/Windows) | `yarn tauri build ... -f desktop` | `ci.build.yml` |
| ios | `cd src-tauri && cargo check --target aarch64-apple-ios --no-default-features --features mobile` | `ci.build.yml` |
| android | `cd src-tauri && cargo check --target aarch64-linux-android --no-default-features --features mobile` | `ci.build.yml` |

Notes that matter:

- **CI clippy does not pass `--release`**, while `yarn lint:rust` does. To reproduce CI exactly, use
  the bare form above.
- **No CI workflow runs `cargo test`.** There is also **no JavaScript/TypeScript test runner** in the
  repo — no test script in `package.json`, no vitest/jest dependency. Do not invent test commands.
  Rust unit tests exist in places like `src-tauri/src/services/export/anlz/**` and
  `src-tauri/src/services/cloud_sync/tests/**`.
- Toolchain: `nightly-2026-02-19` (pinned in `src-tauri/rust-toolchain.toml`), Node 22 in CI.

## Fast local loop (most useful first)

```bash
yarn format:check          # cheapest; catches most CI lint job failures
yarn lint:check            # ESLint
yarn check:svelte          # desktop type-check
yarn check:svelte:mobile   # mobile type-check (run when shared/ changed)
yarn check:cargo           # cargo check --release --features desktop
```

Rust, from `src-tauri/`:

```bash
cargo fmt --check
cargo clippy --features desktop -- -D warnings
cargo test --features desktop
```

## Targeted commands by change type

| Change | Minimum proof |
| --- | --- |
| Docs / markdown only | None required; no gate covers prose. |
| `.svelte` / `.ts` in `apps/desktop` | `yarn lint:check` + `yarn check:svelte` + `yarn format:check` |
| `shared/**` (compiled by both apps) | `yarn check:svelte` + `yarn check:svelte:mobile` + `yarn lint:check` |
| `shared/i18n/locales/*.json` | `yarn format:check` + a JSON parse check of every edited file |
| Rust, shared/gated code | `cargo check ...` + clippy + mobile `cargo check` for the affected target |
| Rust with unit tests | `cargo test --features desktop` |
| `src-tauri/Cargo.toml` (dependency change) | `cargo check --features desktop` + `cargo audit` + `yarn check:svelte` |
| Schema change | `cargo test --features desktop` + a run of the app or a DB test that exercises migrations |

## Mobile safety check

`--features desktop` alone is **not** enough evidence for Rust changes that touch `shared`-adjacent
code, because the mobile build compiles the same crate with different features:

```bash
cd src-tauri
cargo check --target aarch64-apple-ios --no-default-features --features mobile
```

A desktop-only API referenced from non-gated code fails here — and only here.

## Reporting format

State exactly what ran and what happened:

- Command, then result (`passed`, `failed with N errors`, `skipped because ...`).
- If a gate could not run (missing toolchain, no network, unavailable target), say so explicitly and
  name the blocker — never present an unrun gate as green.
- Distinguish "pre-existing failure" from "failure caused by this change" with evidence.

## Checklist

1. Every gate that covers the changed files was run, or its absence explained.
2. `cargo fmt` / `yarn format:fix` applied so formatting is not a gate failure.
3. Mobile check run whenever `shared/` or non-gated Rust changed.
4. No fabricated test command reported for the frontend — there is no JS test runner.
