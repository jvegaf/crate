# Feature: contributor-docs

Goal: make the fork legible to humans, not just to agents. Three artifacts, split by audience and by
what can travel upstream:

- `CONTRIBUTING.md` — human entry point (setup, dev loop, gates, branch/commit/PR rules). Portable
  upstream as a standalone commit.
- `FORK.md` — fork-only topology and sync workflow. Must never be cherry-picked upstream.
- `odd/README.md` — index that makes the progress record discoverable.

Context: the fork already versions its agent harness (`AGENTS.md`, `.agents/skills/`, `odd/tasks/`,
`openspec/`, `.pi/`). What is missing is anything written for a person: there is no `CONTRIBUTING.md`
in the repo, the `README.md` still describes only upstream, and the only progress record
(`odd/tasks/agent-harness-bootstrap.md`) has no index pointing at it.

## Tasks

- [x] T1 Write `CONTRIBUTING.md` (human entry point, upstream-portable)
- [x] T2 Write `FORK.md` (fork-only, must never travel upstream)
- [x] T3 Write `odd/README.md` (feature index + tracking convention)
- [x] T4 Commit as separate work units, fast-forward into `develop`
- [x] T5 Verify: paths, commands, cross-links, `yarn format:check`

## Constraints

- **Single owner per fact.** `AGENTS.md` owns the contract (invariants, full command table, traps).
  `CONTRIBUTING.md` links to it instead of restating it. No duplicated rule sets.
- **Audience split.** Upstream-portable content and fork-only content never share a commit, so the
  upstream-able commit stays cherry-pickable.
- Do not touch `README.md`, `CHANGELOG.md`, or `docs/` — all three are upstream-owned.
- No commits without explicit user authorization; no pushes.
- Artifacts in English; conversation in Rioplatense Spanish.

## Evidence log

- Recon: no `CONTRIBUTING.md`, `SECURITY.md`, or `CODE_OF_CONDUCT.md` exist (root and `.github/`).
  `README.md` has no fork or contributing section. `odd/` contains exactly one file.
  `CHANGELOG.md` is upstream-managed (`Keep a Changelog`, driven by `scripts/changelog.js`).
- Verification of the new documents (all green): 25 relative links across the three files resolve to
  existing paths; every referenced `yarn` script exists in `package.json` (`format:check`, `lint:check`,
  `check:svelte`, `check:svelte:mobile`, `check:cargo`, `dev`, `dev:vite`); port 1420 confirmed in
  `apps/desktop/vite.config.ts`; the "no JavaScript test runner" claim re-verified (no vitest/jest/
  mocha/ava/playwright/cypress in any `package.json`, and no `*.test.*`/`*.spec.*` files under
  `apps/` or `shared/`).
- `yarn format:check` green after the changes (Markdown is outside Prettier's `{ts,js,json,svelte,css}`
  glob, so the gate is unaffected, but it was run anyway).

## Commit evidence (on `develop`, not pushed)

| Commit | Message | Files |
| --- | --- | --- |
| `9bbf50d` | `docs(odd): track contributor-docs feature` | `odd/tasks/contributor-docs.md` |
| `84e74b6` | `docs: add CONTRIBUTING.md contributor guide` | `CONTRIBUTING.md` |
| `1430150` | `docs: add FORK.md fork workflow guide` | `FORK.md` |
| `96ec6c7` | `docs(odd): add feature progress index` | `odd/README.md` |

`84e74b6` is the only upstream-portable commit in this set: it touches a single new file and can be
cherry-picked onto `upstream/develop` on its own. The other three are fork-only and must not travel.
