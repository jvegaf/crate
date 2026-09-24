# FORK.md — working in this fork

> **Fork-only document.** This describes `jvegaf/crate`, a fork of
> [`blackboxaudio/crate`](https://github.com/blackboxaudio/crate). It must never be included in a
> pull request to the original project.

## Why this fork exists

To develop features that can later be offered upstream as focused, self-contained pull requests.
The fork carries agent tooling and workflow documentation on top of the original project, and the
features built on top of that are meant to be ordinary contributions.

## Remotes

| Remote | URL | Transport |
| --- | --- | --- |
| `origin` | `git@github.com:jvegaf/crate.git` | SSH |
| `upstream` | `https://github.com/blackboxaudio/crate.git` | HTTPS |

If `git fetch origin` fails with `Permission denied (publickey)`, that is a local SSH setup problem,
not a repository problem. `upstream` deliberately uses HTTPS because it works without a key.

A fresh clone needs the upstream remote added:

```bash
git remote add upstream https://github.com/blackboxaudio/crate.git
git fetch upstream --no-tags
```

## How far ahead the fork is

Check this rather than trusting a written number:

```bash
git fetch upstream --no-tags
git rev-list --left-right --count upstream/develop...develop   # left = upstream-only, right = fork-only
git log --oneline upstream/develop..develop                     # what the fork adds
git log --oneline develop..upstream/develop                     # what upstream has that we don't
```

## Staying in sync

```bash
git fetch upstream --no-tags
git checkout develop
git merge upstream/develop        # or: git rebase upstream/develop
```

`--ff-only` only works while the fork adds nothing of its own. Since `develop` now carries fork-only
commits, use a plain merge or a rebase.

## Branches in this fork

- `develop` is the fork's integration line: the original project's history **plus** fork-only
  commits — agent tooling (`AGENTS.md`, `.agents/`, `odd/`, `openspec/`, `.pi/`) and these workflow
  documents.
- Feature branches are created from `develop` and named `{issue-number}-{description}`.
- **A feature branch must contain only the feature.** Keep fork-only tooling out of it, or the branch
  cannot be replayed cleanly onto upstream.

Check a branch before offering it upstream:

```bash
git log --oneline upstream/develop..HEAD
git diff --stat upstream/develop...HEAD
```

If `AGENTS.md`, `.agents/`, `odd/`, `openspec/`, `.pi/` or `FORK.md` appear in that list, the branch
is fork-only and not upstream-ready. `CONTRIBUTING.md` is the one document written to travel
upstream — but as its own pull request, never mixed into a feature branch.

## Producing an upstream pull request

Build it from upstream rather than from the fork's tip:

```bash
git fetch upstream --no-tags
git checkout -b upstream/<issue>-<description> upstream/develop
git cherry-pick <feature-commits>          # only the feature
git push -u origin HEAD
```

Then open the pull request against `blackboxaudio/crate:develop`. Rebase on a fresh
`upstream/develop` before requesting review if upstream has moved.

## What this fork adds, and what it does not

**Adds:** an agent contract ([`AGENTS.md`](AGENTS.md)), a project skill pack
([`.agents/skills/`](.agents/skills/)), a feature progress index ([`odd/`](odd/README.md)), and these
workflow documents ([`CONTRIBUTING.md`](CONTRIBUTING.md), `FORK.md`).

**Does not change:** application behavior, dependencies, CI configuration, release and versioning, or
anything under [`docs/`](docs/). If a harness change touches those, it is a bug in the harness, not a
feature.

## License

Unchanged from upstream: [PolyForm Shield License 1.0.0](LICENSE) — source-available, no competing
product, no relicensing. [`CONTRIBUTING.md`](CONTRIBUTING.md) covers what that means when
contributing.
