# Feature: agent-harness-bootstrap

Goal: leave the fork ready to work — a repo-wide agent contract (`AGENTS.md`) and a small,
evidence-based project skill pack under `.agents/skills/`, plus the fork/upstream wiring that
lets us add features here and still ship clean PRs to `blackboxaudio/crate`.

Context: `jvegaf/crate` is a fork of `blackboxaudio/crate`. Stack: Tauri v2 + Rust backend
(`src-tauri/`), SvelteKit 5 + TS + Tailwind 4 frontends (`apps/desktop`, `apps/mobile`) sharing
`shared/`, Astro Starlight docs (`docs/`). Yarn 4 Berry workspaces. License: PolyForm Shield 1.0.0
(source-available, no competing product).

## Tasks

- [x] T1 Recon: verify stack, package manager, CI gates, toolchain, fork topology
- [x] T2 Scout Rust backend conventions (read-only, delegated)
- [x] T3 Scout frontend conventions (read-only, delegated)
- [x] T4 Author `AGENTS.md`
- [x] T5 Author project skills under `.agents/skills/`
- [x] T6 Wire fork remotes (`upstream`) and document upstream sync
- [x] T7 Verify: commands exist, paths resolve, no fabricated references
- [x] T8 Close: report outcome, pending checks, next step

## Constraints

- No commits without explicit user authorization (ODD work-unit commits stay pending).
- Agent scaffolding must never leak into an upstream PR: feature branches stay pure.
- Every documented command must come from `package.json`, CI workflows, or a verified file.
- Language: artifacts in English; conversation in Rioplatense Spanish.

## Evidence log

- T1: `package.json` scripts, `.github/workflows/ci.{build,lint}.yml`,
  `src-tauri/rust-toolchain.toml` (nightly-2026-02-19), `.github/RELEASE_STRATEGY.md`
  (branch model `{issue}-{description}`, all PRs target `develop`), `LICENSE` (PolyForm Shield 1.0.0).
- T2: commands `#[tauri::command]` + hand-maintained `generate_handler!` (`lib.rs:120-358`),
  `error.rs` stringifying `Serialize`, rusqlite single `Arc<Mutex<Connection>>`,
  migrations appended in `db/schema.rs`, `desktop` feature = heavy native deps,
  no `#[cfg(feature = "mobile")]` in source, `compile_error!` guard in `db/key/mod.rs:74`.
- T3: IPC chain `component -> store/controller -> shared/api -> invoke -> commands`,
  classic `svelte/store` factories (only `virtualizer.svelte.ts` uses `$state`),
  Svelte 5 `$props()` + callback props, `$shared` alias via `svelte.config.js`,
  Tailwind `@theme` tokens + `[data-theme]` dark mode, `@utility` brand classes.
- T3 correction (parent-verified): locale drift is real — `en.json` 743 leaves; `de/es/fr/it/ko/nl/pt/sv/zh`
  miss `toast.tracksExported`; `pl/ro/tr/uk` miss 4 keys (`discovery.unsupportedUrl`, `menu.hide`,
  `menu.hideOthers`, `menu.showAll`); `ja` is at parity. No parity tooling exists.
- T1/T2 correction (parent-verified): no `cargo test` anywhere in `.github/workflows/`;
  CI clippy runs `cargo clippy --features desktop -- -D warnings` (no `--release`) while
  `yarn lint:rust` adds `--release`.
- T6: `upstream` remote added over HTTPS because SSH auth to GitHub fails in this environment
  (`git@github.com: Permission denied (publickey)`); `git ls-remote https://...` succeeds.
  Fork divergence measured: `upstream/develop...origin/develop` = `0 1`; the only fork-only commit is
  `746c7c5 build(deps): migrate from Yarn 1 to Yarn Berry`.
- T4/T5 artifacts: `AGENTS.md`; `.agents/skills/{crate-rust-backend,crate-frontend,crate-feature-flow,crate-verification,crate-upstream-contribution}/SKILL.md`.
- T7 verification (all green): every `yarn <script>` referenced exists in `package.json`; 52 referenced
  paths exist; skill frontmatter names match their directories and descriptions are within limits;
  confirmed in source — `compile_error!` at `db/key/mod.rs:79`, stringifying `Serialize` at
  `error.rs:93-98`, `invoke_handler` at `lib.rs:120`, `rename_all` per struct, `rusqlite` with
  `bundled-sqlcipher-vendored-openssl` at `Cargo.toml:61`, `@theme`/`@utility bg-brand-primary-10`
  and `data-theme`/`data-accent`/`data-font` wiring, `ssr = false` + `fallback: 'index.html'`,
  and zero `feature = "mobile"` gates in Rust source.

## Pending (user decisions)

- Whether to commit the harness (`AGENTS.md`, `.agents/`, `odd/`, `openspec/`, `.pi/`) on `develop`
  or keep parts local. Nothing is committed; commits require explicit authorization.
- Whether to add `.agents/`, `odd/`, `openspec/`, `.pi/` to `.gitignore` (only `.atl/` is ignored today).
