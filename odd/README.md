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
| Analysis skip stubs + force re-analysis | Done | [`tasks/analysis-skip-stubs-and-force.md`](tasks/analysis-skip-stubs-and-force.md) |
| Analysis worker limit | Done — live end-to-end run never verified (its own evidence log) | [`tasks/analysis-worker-limit.md`](tasks/analysis-worker-limit.md) |
| Contributor documentation | Done | [`tasks/contributor-docs.md`](tasks/contributor-docs.md) |
| Import + display POPM rating | Done — `1433315` | [`tasks/import-and-display-popm-rating.md`](tasks/import-and-display-popm-rating.md) |
| Key format display fix | Done | [`tasks/key-format-display-fix.md`](tasks/key-format-display-fix.md) |
| Library folder scan | Shipped — checkboxes T6–T10 left unticked although the API wrapper, the `LibraryFolderScanResult` type and the Settings → Library UI all exist; the record is stale, not the work | [`tasks/library-folder-scan.md`](tasks/library-folder-scan.md) |
| Library scan performance | **Not started** — reconnaissance and design only, 0 of 9 tasks ticked | [`tasks/library-scan-performance.md`](tasks/library-scan-performance.md) |
| Testing setup | Done — store tests explicitly not implemented (stores need Tauri `invoke` mocks) | [`tasks/testing-setup.md`](tasks/testing-setup.md) |
| Track Editor UI redesign | Done | [`tasks/track-editor-ui.md`](tasks/track-editor-ui.md) |
| Track Metadata Modal | Done | [`tasks/track-metadata-modal.md`](tasks/track-metadata-modal.md) |
| Tracklist column configuration | Done — UI confirmed by the user; native review unattested | [`tasks/tracklist-column-config.md`](tasks/tracklist-column-config.md) |

Update this table whenever a feature is added or closes. It is the front door — if it is stale, the
record may as well not exist.

> **Provenance of the 2026-09-29 reconciliation.** This table listed 3 of the 12 files actually in
> `tasks/`. The added statuses were derived, not guessed: each was read against its own Task
> checklist, its Evidence log / Delivery section, and — where the two disagreed — against the tree
> (e.g. `shared/api/library.ts:183`, `shared/types/index.ts:206` and `LibraryTab.svelte` confirm the
> folder-scan work exists despite unticked boxes; `git cat-file` confirmed the cited hashes exist).
> Where a document's checkboxes contradict its own evidence, the row says so instead of silently
> picking one.

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
