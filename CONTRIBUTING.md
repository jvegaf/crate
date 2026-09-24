# Contributing to Crate

Thanks for taking the time. This file is the human entry point: how to get the project running, how
to check your work, and how a change gets merged.

For the authoritative rules — architecture invariants, the complete command table, and the traps that
break builds — read [`AGENTS.md`](AGENTS.md). It is written for coding agents, but it is the single
source of truth for everyone, and it is deliberately not duplicated here.

## Prerequisites

- **Node.js 22** or newer.
- **Yarn 4** (Berry) — the repository ships `.yarnrc.yml` and `yarn.lock`.
- **Rust** — the toolchain is pinned to `nightly-2026-02-19` in `src-tauri/rust-toolchain.toml`, so
  [rustup](https://rustup.rs/) picks the right one automatically from the file.
- **Tauri v2 system dependencies** — see the
  [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/).
- **Windows only:** [Strawberry Perl](https://strawberryperl.com/) is needed to build SQLCipher with
  OpenSSL. `choco install strawberryperl`, then restart your terminal.

## Getting set up

```bash
git clone https://github.com/blackboxaudio/crate.git
cd crate
yarn install
yarn dev
```

`yarn dev` starts the Vite dev server on port 1420 together with the Tauri window, using the desktop
feature set. Use `yarn dev:vite` when you only need the frontend.

Working from a fork? Read [`FORK.md`](FORK.md) first — it covers remotes and syncing with the
original project.

## Before you push

These are the same checks CI runs:

```bash
yarn format:check
yarn lint:check
yarn check:svelte
yarn check:svelte:mobile   # whenever you touch shared/ or mobile-reachable code
yarn check:cargo
```

For Rust changes, from `src-tauri/`:

```bash
cargo fmt --check
cargo clippy --features desktop -- -D warnings
```

The complete table — including mobile, iOS/Android and release commands — is in
[`AGENTS.md`](AGENTS.md) §4.

Three things that surprise people:

- **There is no JavaScript test runner.** No `yarn test`, no Vitest, no Jest — don't invent one.
  Rust unit tests do exist: `cd src-tauri && cargo test --features desktop`. The feature flag is
  required, because the crate has no default features and mobile is a separate, non-overlapping set.
- **CI does not run `cargo test`.** Passing locally is on you; there is no pipeline safety net for
  the Rust test suite.
- **The pre-commit hook formats more than you staged.** Husky runs Prettier and ESLint through
  lint-staged, and the configured Prettier command is a repository-wide glob — so committing while
  unrelated files are unformatted will rewrite those files too. Keep the tree clean.

## Branches and commits

- Branch names are `{issue-number}-{description}`, for example `143-mobile-db-key`.
- **All pull requests target `develop`.** There is no `main` or `release/*` branch; see
  [`.github/RELEASE_STRATEGY.md`](.github/RELEASE_STRATEGY.md).
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`,
  `fix:`, `chore:`, `test:`, `docs:`, `build(deps):`.
- Keep each commit a coherent work unit — behavior together with its tests and docs, not "wip"
  followed by "fix tests".

## Pull requests

The [template](.github/pull_request_template.md) asks for:

- `Closes #<issue>`, linking the issue.
- A summary of *what* changed and *why*.
- A self-review of your own diff.
- Confirmation that you tested locally.
- No unrelated changes.

Review attention is the scarcest resource in this project. Keep changes focused and aim well under
about 400 changed lines; if the work is larger, split it into slices instead of opening one big pull
request.

## Where things live

| Path | What it is |
| --- | --- |
| [`AGENTS.md`](AGENTS.md) | Architecture invariants, full command table, known traps |
| [`FORK.md`](FORK.md) | Remotes and how to sync with the original project |
| [`odd/README.md`](odd/README.md) | How features are planned and how to see what is in progress |
| [`docs/`](docs/) | End-user documentation (Astro Starlight), built and deployed separately |
| [`.github/RELEASE_STRATEGY.md`](.github/RELEASE_STRATEGY.md) | Release channels and versioning |
| [`src-tauri/`](src-tauri/) | Rust backend |
| [`shared/`](shared/) | TypeScript shared by both apps |

## License

Crate is source-available under the [PolyForm Shield License 1.0.0](LICENSE). You can read, learn
from, and contribute to the code, but you cannot use it to build a competing product. Contributions
are accepted under the same terms, so do not add dependencies or vendored code under an incompatible
license — flag it instead.
