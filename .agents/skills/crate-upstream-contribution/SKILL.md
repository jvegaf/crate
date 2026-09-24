---
name: crate-upstream-contribution
description: Fork and upstream workflow for this repository — remote topology, keeping the fork in sync with blackboxaudio/crate, branch naming, producing clean upstream PRs without agent scaffolding, license and commit conventions. Use when preparing commits, branches, pulls, or an upstream sync.
---

# Crate fork and upstream contribution

This clone is a fork: `origin` = `jvegaf/crate`, `upstream` = `blackboxaudio/crate`.

## Remote topology

```
origin    git@github.com:jvegaf/crate.git            (SSH — may fail: public-key auth is not set up here)
upstream  https://github.com/blackboxaudio/crate.git (HTTPS — works)
```

If `git fetch origin` fails with `Permission denied (publickey)`, that is the environment, not the
repository. HTTPS works; prefer it for fetches, or add an SSH key before pushing.

## Current state (verify before relying on it)

The fork's `develop` is exactly **one commit ahead** of `upstream/develop`:

```
build(deps): migrate from Yarn 1 to Yarn Berry     (746c7c5, fork-only)
```

Keep this in mind: that fork-only commit is *not* something you want to smuggle into an unrelated
upstream PR.

```bash
git fetch upstream --no-tags
git rev-list --left-right --count upstream/develop...origin/develop
git log --oneline upstream/develop..origin/develop     # what the fork adds
git log --oneline origin/develop..upstream/develop     # what upstream adds
```

## Keeping in sync

```bash
git fetch upstream --no-tags
git checkout develop
git merge --ff-only upstream/develop     # or: git rebase upstream/develop
```

Prefer `--ff-only` when the fork has nothing of its own pending; rebase when replaying fork-only
scaffolding commits on top of new upstream work.

## Branch model

- `develop` is the integration branch. `upstream/develop` is the real upstream. There is no `main`,
  no `release/*`.
- Feature branches are named `{issue-number}-{description}` (e.g. `143-mobile-db-key`) and branch
  from `develop`.
- Branch first when on `develop`; never commit directly to `develop` for feature work.

## The one rule that keeps upstream PRs possible

**A feature branch must contain only the feature.**

Agent scaffolding — `AGENTS.md`, `.agents/`, `odd/`, `openspec/`, `.pi/`, `.atl/` — must never be
committed on a branch you intend to propose upstream. Keep scaffolding commits on the fork's
`develop` line (or a dedicated harness branch), and keep feature branches pure so they replay onto
`upstream/develop` without noise.

Check before proposing anything:

```bash
git diff --stat upstream/develop...HEAD
git log --oneline upstream/develop..HEAD
```

If you see `AGENTS.md`, `.agents/`, `odd/`, `openspec/`, or `.pi/` in that list, the branch is not
upstream-ready.

## Producing a clean upstream PR

Build it from upstream, not from the fork's tip:

```bash
git fetch upstream --no-tags
git checkout -b upstream/<issue>-<description> upstream/develop
git cherry-pick <feature-commit>...          # only the feature commits
git push -u origin HEAD
```

Then open the pull request against `blackboxaudio/crate:develop`. Rebase onto a fresh
`upstream/develop` before requesting review if upstream moved.

## Commit and PR conventions

- **Conventional Commits**, matching history: `feat:`, `fix:`, `chore:`, `test:`, `docs:`,
  `build(deps):`. Scope when it helps (`build(deps): migrate from Yarn 1 to Yarn Berry`).
- Reference the issue in the subject as the project does: `feat: add "follow" operation for labels
  and artists (#132)`.
- **PR template** (`.github/pull_request_template.md`) requires: `Closes #<issue>`, a summary of
  what and why, self-review of the diff, local testing, and no unrelated changes.
- **Review workload**: the project protects review focus, and its release strategy is built around
  small focused changes. Aim for well under ~400 changed lines; split rather than grow.
- Every commit on a feature branch should be a coherent work unit: behavior with its tests and docs
  together, not "wip" then "fix tests".

## License

PolyForm Shield 1.0.0 (`LICENSE`). Contributions are made under the same terms. You may read,
modify, and contribute, but you may not use this code to build a competing product, and you must not
remove license notices. Do not add a dependency or vendored file whose license conflicts with this
one — flag it instead.

## Release and versioning (upstream-owned)

`yarn bump` synchronizes the version across `package.json`, `src-tauri/Cargo.toml`,
`src-tauri/tauri.conf.json` and the staging config; `scripts/tag.sh` drives staging and production
tags, and CI builds them. Do not invent version bumps or tags in the fork — release management is
the upstream maintainers' call.

## Checklist before proposing upstream

1. Feature branch contains only feature commits (scaffolding excluded).
2. Rebased on a freshly fetched `upstream/develop`.
3. Conventional Commit messages, issue referenced, no unrelated changes.
4. Gates from `crate-verification` run and green (or failures explained).
5. Diff size within review budget, or split into chained PRs.
6. License constraints respected; no conflicting dependency added.
