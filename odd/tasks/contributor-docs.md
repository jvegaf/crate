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

- [ ] T1 Write `CONTRIBUTING.md` (human entry point, upstream-portable)
- [ ] T2 Write `FORK.md` (fork-only, must never travel upstream)
- [ ] T3 Write `odd/README.md` (feature index + tracking convention)
- [ ] T4 Commit as separate work units, fast-forward into `develop`
- [ ] T5 Verify: paths, commands, cross-links, `yarn format:check`

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
