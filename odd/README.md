# Feature tracking (`odd/`)

This directory is the fork's progress record. It answers "what are we building, what is done, and
what is next" without anyone having to reconstruct it from a chat log or a commit history.

> **Fork-only.** `odd/` is agent workflow tooling, not part of the original project, and must never
> appear in a pull request to `blackboxaudio/crate`. See [`../FORK.md`](../FORK.md).

## The convention

One file per feature, at `odd/tasks/<feature-name>.md`, created **before** the first line of code is
written and updated as the work moves.

Every task file has the same shape:

| Section | Contains |
| --- | --- |
| Goal | One paragraph: what the feature is and why it exists |
| Context | The constraints and prior state that make it non-obvious |
| Tasks | A checklist of the implementation steps |
| Constraints | The rules this work must respect |
| Evidence log | What was actually verified — paths, commands, findings, corrections |
| Decisions | Open questions and how they were resolved, with reasoning |
| Commit evidence | The commits that delivered the feature, by hash |

A feature moves `pending` → `in progress` → `done` as it lands.

## Index

| Feature | Status | Task file |
| --- | --- | --- |
| Agent harness bootstrap | Done | [`tasks/agent-harness-bootstrap.md`](tasks/agent-harness-bootstrap.md) |
| Contributor documentation | Done | [`tasks/contributor-docs.md`](tasks/contributor-docs.md) |

Update this table whenever a feature is added or closes. It is the front door — if it is stale, the
record may as well not exist.

## Adding a feature

1. Create `odd/tasks/<feature-name>.md` before starting to write code.
2. Fill in Goal, Context, Tasks, and Constraints.
3. Append to the Evidence log as you verify things — commands run, findings, failed checks,
   corrections to earlier assumptions.
4. Record decisions as they are resolved, with the reasoning, not just the outcome.
5. When the work is committed, add the commit hashes to Commit evidence.
6. Add a row to the index above.

## What this is not

It does not replace the agent contract in [`../AGENTS.md`](../AGENTS.md) or the contributor guide in
[`../CONTRIBUTING.md`](../CONTRIBUTING.md). Those define the rules; this directory records what
following them produced.
