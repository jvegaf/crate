# Track Metadata Modal

## Objective and accepted decisions

Add a dedicated single-track metadata modal opened from the track context menu. It displays comprehensive metadata; edits embedded audio tags/artwork and Crate-owned tags/artwork through separate operations; and presents technical properties read-only. Nullable fields can be explicitly cleared. The user accepted staged embedded writes and best-effort compensation, not crash-atomicity.

The user selected strict TDD for the backend and explicitly disabled it for the UI. No frontend JavaScript test runner exists. No push or PR was requested. The user selected `feature_branch_chain` for future review slicing; no PR/tracker was created.

## Implementation and documentation

- Backend: field-level patch DTO, desktop command/service, staged Lofty metadata/artwork writes, DB update/compensation, rating clear (`0`), MIME validation, and backup-path reporting.
- UI: typed API/model, single-track metadata modal, context-menu action, Crate tags/artwork controls, embedded artwork editing, read-only details, and partial-save error reporting.
- User guide updated at `docs/src/content/docs/user-guide/library-management.md`.
- The feature task file under `odd/` is harness scaffolding and is intentionally excluded from the feature commits.

## Work-unit commits

| Commit | Purpose | Diff |
|---|---|---:|
| `0b94892` | Add metadata patch model and embedded tag writes | 331 additions |
| `5fe109c` | Persist single-track metadata patches | 566 additions, 2 deletions |
| `3255615` | Add single-track metadata modal | 604 additions, 1 deletion |
| `3bc3df0` | Open metadata editor from context menu; add user guide | 97 additions, 9 deletions |

All four commits are on `track-metadata-modal`; no commit contains `odd/**`. Total is about 1,610 changed lines, above the repository's ~400-line review target. Commits are ordered backend → modal/API → menu/docs. No PRs were created or pushed; any later publication needs an appropriately sliced feature-branch chain or an explicitly accepted size exception.

## Verification evidence and caveats

Passed:
- `cd src-tauri && cargo test --features desktop`: 236 passed, 0 failed; 1 doctest ignored.
- `cd src-tauri && cargo clippy --features desktop -- -D warnings`.
- `yarn lint:check`.
- `yarn check:svelte:mobile`: 0 errors, 0 warnings.
- `git diff --check`.

Failed or unavailable:
- `yarn check:svelte`: one error for missing `PUBLIC_APP_VERSION` in unchanged `apps/desktop/src/routes/+layout.svelte`; two existing accessibility warnings in unchanged `LibraryTab.svelte`. The cause remains unresolved.
- `yarn format:check`: fails only on unchanged `shared/utils/format.ts`; both changed Svelte files were targeted-formatted.
- Workspace Rust `cargo fmt --check` already failed on 174 Rust path identities at clean branch-point HEAD. No unrelated mass-formatting was done.
- `rustfmt --check --edition 2021 src/services/library/metadata_update.rs` follows its test-only module path into `src-tauri/src/test_utils.rs`; after restoring that previously clean file, the recursive check reports its baseline formatting. `test_utils.rs` is clean in Git; the new module itself was formatted.
- `cd docs && npm run astro -- build`: unavailable (`astro: not found`, exit 127); dependencies were not installed.
- Initial embedded-write integration tests were added after implementation and lack historical RED evidence. Later rating, MIME, and rollback fixes did observe RED/GREEN; UI TDD is off by explicit user choice.
- The newly added untracked `metadata_update.rs` was accidentally overwritten with formatter configuration and reconstructed. Exact pre-overwrite bytes are unavailable; independent code review and current tests found the reconstruction coherent. No historical exactness is claimed.
- A targeted rustfmt invocation also changed `test_utils.rs`; its diff was formatting-only and it was restored from HEAD. It is clean now.
- Engram mirror remains pending because the installed Engram binary is incompatible with the active provider.

## Completion

The implementation, user documentation, and four requested commits are complete. Worktree status after commits showed only this untracked ODD task file; it remains excluded by repository policy. No push or PR was performed.
