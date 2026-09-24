# AGENTS.md — Crate

Agent contract for this repository. Read it before editing anything. It is written for coding
agents first and human contributors second.

> **Fork notice.** This clone is `jvegaf/crate`, a **fork** of `blackboxaudio/crate`.
> `origin` = the fork, `upstream` = the original project. Work here is meant to be optionable
> upstream: add features in the fork, then offer them as a focused PR to `blackboxaudio/crate`.

---

## 1. Absolute constraints

1. **License: PolyForm Shield 1.0.0** (`LICENSE`). Source-available. You may read, modify, and
   contribute, but **not** use this code to build a competing product. Never relicense, never strip
   license or `Required Notice` lines.
2. **Never mix agent scaffolding into a feature branch.** A branch intended for upstream must
   contain only the feature. Tooling files — `AGENTS.md`, `.agents/`, `odd/`, `openspec/`, `.pi/`,
   `.atl/` — belong to the fork's integration line (`develop`), never to a pure feature branch.
3. **Do not commit unless the user explicitly asks.** Do not push, open PRs, or tag without an
   explicit instruction.
4. **Both build targets must keep compiling.** Reaching for a desktop-only API from shared code
   breaks the mobile build, and CI checks it. See §5.

## 2. Repository map

| Path | What it is |
| --- | --- |
| `src-tauri/` | Rust backend (Tauri v2). `src/commands/` = IPC surface, `src/services/` = logic, `src/models/` = DTOs, `src/db/` = SQLite/SQLCipher, `src/menu.rs` = native menu, `src/proxy.rs` = local proxy. |
| `apps/desktop/` | Main application: SvelteKit 5 + TS + Tailwind 4. `src/routes/` (SPA, `ssr = false`), `src/lib/{components,controllers,hooks,stores,transitions}/`. |
| `apps/mobile/` | Mobile scaffolding: SvelteKit routes only, no `@tauri-apps/api` yet. Compile-checked in CI, deliberately not feature-complete. |
| `shared/` | Cross-app TypeScript. `api/` (IPC wrappers), `stores/`, `types/`, `utils/`, `i18n/`. Imported as `$shared`. |
| `docs/` | Astro Starlight user documentation. Independent `npm` install, not part of the yarn workspace. |
| `.github/workflows/` | CI: `ci.build.yml`, `ci.lint.yml`, `test.release.yml`, `cd.release.yml`, `cd.docs.yml`. |
| `openspec/`, `odd/`, `.agents/` | Agent artifacts: OpenSpec config, ODD feature tracking, project skills. |

## 3. Toolchain

- **Node 22+** (CI pins 22; local may be newer) and **Yarn 4 Berry** (`nodeLinker: node-modules`,
  workspaces `apps/*` — note `shared/` is *not* a workspace package, it is reached by alias).
- **Rust is pinned to `nightly-2026-02-19`** by `src-tauri/rust-toolchain.toml`, with `rustfmt` and
  `clippy` components. Do not assume `stable` works: clippy runs with `-D warnings` on nightly.

## 4. Commands

Run from the repository root unless stated otherwise.

| Command | Does |
| --- | --- |
| `yarn install --frozen-lockfile` | Install workspaces (what CI uses). |
| `yarn dev` | Tauri dev window: Vite on 1420 + Tauri, `--release --features devtools,desktop`. |
| `yarn dev:vite` | Vite dev server only. |
| `yarn dev:ios` / `yarn dev:android` | Tauri mobile dev with `--features mobile` (no desktop features). |
| `yarn build` / `yarn build:production` | Production bundle. |
| `yarn build:staging` | Staging bundle with devtools. |
| `yarn build:vite` | Frontend static build only. |
| `yarn build:ios` / `yarn build:android` | Mobile bundles. |
| `yarn check` | `check:cargo` + `check:svelte`. |
| `yarn check:cargo` | `cd src-tauri && cargo check --release --features desktop`. |
| `yarn check:svelte` | `svelte-check` for desktop. |
| `yarn check:svelte:mobile` | `svelte-check` for mobile. |
| `yarn check:watch` | `svelte-check` in watch mode. |
| `yarn lint:check` | ESLint (CI gate). |
| `yarn lint:fix` | ESLint with `--fix`. |
| `yarn lint:rust` | `cargo +nightly clippy --release --features desktop`. |
| `yarn format:check` | Prettier check (CI gate). |
| `yarn format:fix` | Prettier write. |
| `yarn format:rust` | `cargo +nightly fmt`. |
| `yarn bump`, `yarn changelog:prepare`, `yarn changelog:graduate` | Version sync and changelog. |
| `yarn preview` | Preview the built frontend. |

Rust-only, from `src-tauri/`:

```bash
cargo fmt --check                                  # CI gate (nightly rustfmt)
cargo clippy --features desktop -- -D warnings      # exact CI gate (no --release)
cargo test --features desktop                       # Rust unit tests
cargo check --target aarch64-apple-ios --no-default-features --features mobile   # CI gate
```

> `cargo test --features desktop` is **required** to have any feature: `default = []`, and without
> `desktop` the crate trips its own `compile_error!` guard (`src-tauri/src/db/key/mod.rs`). No CI
> workflow runs `cargo test` today — it is your local check, not a gate.

## 5. Architecture invariants

1. **IPC chain.** `component → store/controller → shared/api/*.ts → invoke → src-tauri/src/commands/*.rs`.
   Components never call `invoke` directly; the only exception is OS integration plugins
   (`@tauri-apps/plugin-opener`, `@tauri-apps/api/event`).
2. **Register every command.** A `#[tauri::command]` that is not listed in `generate_handler!`
   in `src-tauri/src/lib.rs` does not exist. Desktop-only entries need `#[cfg(feature = "desktop")]`
   on their own line inside that list.
3. **Errors.** Return `crate::error::Result<T>`. `CrateError` implements `Serialize` by emitting its
   `Display` string, so the frontend receives a **plain string** rejection, not a JSON object.
   `src-tauri/src/commands/app.rs` returns `Result<_, String>` — that is legacy, do not copy it.
4. **Feature gates.** `desktop` means "needs heavy native dependencies". There is **no**
   `#[cfg(feature = "mobile")]` anywhere in the source: mobile paths are written as
   `#[cfg(not(feature = "desktop"))]`, and per-OS code uses `cfg(target_os = ...)`. A
   `compile_error!` rejects any build that is neither desktop nor iOS/Android.
5. **Database.** One `Arc<Mutex<Connection>>` (rusqlite with bundled SQLCipher); no pool, no sqlx,
   no `migrations/` directory. Lock with `self.conn.lock().map_err(|_| CrateError::LockPoisoned)?`,
   never `.unwrap()`. **Never hold the guard across an `.await`.** Migrations are SQL strings
   appended to `get_migrations()` in `src-tauri/src/db/schema.rs` and versioned by index — append,
   never edit an existing entry.
6. **Serde naming is per struct.** Most domain models cross the wire in `snake_case` (`Track`, `Tag`,
   `Playlist`, `TagCategory`); `AppSettings`, menu and diagnostics types are camelCase. Check the
   struct before mirroring it in `shared/types/index.ts`. Tauri command *arguments* are camelCase in
   TS and snake_case in Rust.
7. **Mobile-safe code.** Anything in `shared/` is compiled for mobile too. Guard business logic with
   `#[cfg(...)]` in Rust; do not leak desktop-only commands into shared TS modules.
8. **i18n.** 15 locale files in `shared/i18n/locales/`. There is **no parity tooling** and real drift
   exists today. Add new keys to `en.json` at minimum; update all 15 when the wording allows.
9. **Styling.** Tailwind 4 with `@theme` semantic tokens (`bg-surface-0`, `text-text-primary`,
   `border-stroke`). Dark mode is attribute-based (`[data-theme]` set by the settings store), **not**
   `dark:`. Brand colours are `@utility` classes (`bg-brand-primary`, `bg-brand-primary-10`) —
   the `/opacity` suffix does not work on them.
10. **Svelte 5 surface.** `$props()` with a typed `Props`, callback props (no legacy `on:click`),
    snippets for slots, `$state`/`$derived`/`$effect` inside `.svelte` and `.svelte.ts`. Shared
    **stores** remain classic `svelte/store` factories, despite the runes elsewhere.

## 6. Conventions

- **Formatting:** Prettier — tabs, no semicolons, single quotes, 120 columns, Tailwind class sorting
  (`prettier-plugin-tailwindcss` is pinned to `apps/desktop/src/style.css`). Rust: nightly rustfmt.
- **Naming:** `PascalCase.svelte` components, `camelCase.ts` modules, `snake_case.rs` with `mod.rs`
  directory modules. Rust helpers that are not IPC surface are `pub(crate)`.
- **Commits:** Conventional Commits, as the history shows — `feat:`, `fix:`, `chore:`, `test:`,
  `build(deps):`, `docs:`.
- **Branches:** `{issue-number}-{description}` (e.g. `143-mobile-db-key`). All PRs target `develop`;
  there is no `main` or `release/*` branch (see `.github/RELEASE_STRATEGY.md`).
- **Pull requests:** the template requires `Closes #<issue>`, a self-review, local testing, and no
  unrelated changes. Keep them focused and small.
- **Review workload:** the project's own strategy is to protect review focus. Prefer changes well
  under ~400 changed lines; split larger work into slices rather than one large PR.

## 7. Known traps

- Camel-casing a field that is snake_case on the wire (or the reverse) — see §5.6.
- Adding a command but forgetting the `generate_handler!` entry, so the IPC call fails at runtime.
- Forgetting `#[cfg(feature = "desktop")]` on a desktop-only command: compiles locally, breaks
  mobile CI.
- Assuming `mobile` is a `#[cfg]` feature. It is only a CLI/build marker.
- Editing an existing migration instead of appending a new one.
- Writing `bg-brand-primary/10` (broken) instead of `bg-brand-primary-10`.
- Using Tailwind `dark:` variants instead of the `[data-theme]` tokens.
- Calling `invoke` from a `.svelte` file instead of adding a `shared/api/` wrapper.
- Adding a store but not re-exporting it from the relevant barrel (`shared/stores/index.ts` or
  `apps/desktop/src/lib/stores/index.ts`), making it invisible to pages.
- Importing across apps by relative path instead of `$shared`; `apps/desktop/tsconfig.json` must also
  keep `../../shared/**` in `include` for `svelte-check`.
- Trusting `README.md` on details: it says "11 languages" while 15 locale files exist, and
  `shared/api/index.ts` is a partial barrel (10 of 19 modules).
- Trusting SSH for GitHub: `origin` uses `git@github.com:...` and public-key auth fails in this
  environment. Use HTTPS (that is how `upstream` is configured).

## 8. How agents should work here

Default workflow is **ODD (Organic Driven Development)**: authorize → explore → resolve uncertainty →
classify → track substantial work before the first write → implement task by task → close each task →
report. Substantial work gets `odd/tasks/<feature>.md` plus an Engram mirror; small mechanical work
stays small and creates no durable artifacts. SDD/OpenSpec runs only when explicitly requested.

- **Delegate** when the work crosses the routing triggers (4+ files to understand, 2+ non-trivial
  files to change, an incident to diagnose, a long session without delegation, or any command-running
  verification). Keep one writer at a time.
- **Verify before claiming done.** Use the commands in §4; cite what you actually ran.
- **Load the matching project skill** from `.agents/skills/` before working in an area:

| Skill | Load when |
| --- | --- |
| `crate-rust-backend` | Editing anything under `src-tauri/`. |
| `crate-frontend` | Editing `apps/**` or `shared/**`. |
| `crate-feature-flow` | Adding a capability that crosses Rust and the frontend. |
| `crate-verification` | Before reporting work complete, or reproducing a CI failure. |
| `crate-upstream-contribution` | Preparing commits, branches, or a PR to `blackboxaudio/crate`. |

Keep generated technical artifacts (code, comments, commit messages, docs, tests, identifiers) in
**English**, regardless of the conversation language.
