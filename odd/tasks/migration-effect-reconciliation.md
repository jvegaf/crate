# Schema migrations reconcile by effect, not by number

## Goal

Make `db::run_migrations` apply an append-only DDL migration when its **effect is missing**, instead
of trusting `schema_version` alone. The version counter stays as an optimization; a database whose
numbering drifted (dev branch, restored backup, hand-fixed file) self-heals on the next launch
instead of panicking in the Tauri setup hook.

## Context

The reported crash (`/tmp/crate-crash.log`, 2026-10-05):

```text
INFO  crate_lib::db] Running migration 16
ERROR crate_lib] Database initialization failed: Database error: duplicate column name: last_walk_complete
ERROR crate_lib] PANIC: Failed to setup app: error encountered during setup hook: ...
```

Root cause, established by content-hashing both migration chains (`git show <rev>:src-tauri/src/db/schema.rs`):

- `get_migrations()` is versioned by **position** (`version = idx + 1`, `db/mod.rs:229-243`).
- The renumbered chain matches the current one only at `[n1..n6] + [n10..n18]`: it lacks `n7`
  (`CREATE INDEX idx_tracks_file_hash`), `n8` (the bitrate data repair) and `n9`
  (`ALTER TABLE tracks ADD COLUMN url`).
- `~/.local/share/com.bbx-audio.crate.dev/crate.db` was written by that chain and therefore records
  `schema_version = 15` while already carrying the effects of `n10..n18`. The runner skipped `n1..15`
  and re-executed `n16` (`ALTER TABLE collection_account_state ADD COLUMN last_walk_complete`) on a
  table that already had the column.
- Damage beyond the counter: that database is missing `tracks.url` (read by `Track`,
  `src/models/track.rs:45`), `idx_tracks_file_hash`, and the bitrate repair.
- `ALTER TABLE ... ADD COLUMN` cannot be made idempotent in SQLite (no `IF NOT EXISTS` for columns),
  so the per-migration SQL is untouched by this feature: only the *decision* of whether to run it
  changes.

## Constraints

- **Never edit or renumber an existing migration's SQL.** The recovery for the reported case is the
  effect-based decision, not a repaired chain.
- Healthy databases must behave exactly as before: a fresh database applies all 18 migrations and
  ends at `schema_version = 18`; undeclared (counter-gated) migrations keep the old semantics.
- `run_migrations` stays the single writer entry point; no new IPC, no schema change, no data
  migration of its own.
- Mobile-safe: `db/` compiles for iOS/Android, no `desktop`-only API, no new dependency.
- Errors stay `crate::error::Result`; no `unwrap`/`expect` on connection state in production paths.
- Keep the review diff well under ~400 authored lines.

## Design

`schema.rs` gains a declarative footprint per migration:

```rust
pub struct Migration { pub sql: &'static str, pub effects: &'static [Effect] }

pub enum Effect {
    Table(&'static str),
    Column { table: &'static str, column: &'static str },
    Index(&'static str),
    /// Satisfied when this query returns no rows — an idempotent data repair.
    NoRows(&'static str),
}
```

Runner rule (`db/mod.rs`), one transaction per applied migration exactly as today:

| Migration declares | Applied when |
| --- | --- |
| effects | any declared effect is **not** satisfied (counter ignored) |
| no effects | `version > current_version` (legacy counter gate) |

`NoRows` is what lets the bitrate repair (`n8`) run on a drifted database whose counter says the
version already passed: the counter is not evidence about data.

After a successful pass the runner seals `MAX(version)` to the chain head, so a reconciled database
is indistinguishable from a healthy one on the next launch (and the counter cannot stay stuck at a
number that means something else). Sealing is `INSERT OR REPLACE`, monotone under `MAX()`.

Failure modes considered:

- A **mistyped effect** would make a migration re-run on every launch (crash on the next `ALTER`).
  Guarded by a test asserting every declared effect is satisfied after a fresh full migration.
- An **under-declared footprint** degrades recovery but cannot break a healthy database.
- A migration that mixes DDL with a backfill (`n10`) is gated on its column: the backfill rode the
  same transaction when the column was created, and the effect-based skip only triggers once that
  column exists.

## Tasks

- [x] T1 — RED: regression test that emulates the legacy numbering (apply `[n1..n6] + [n10..n18]`,
      record versions `1..15`, then run the migration path) and assert the database ends reconciled.
      Fails first with `duplicate column name: last_walk_complete` — identical to production.
- [x] T2 — `src-tauri/src/db/schema.rs`: `Migration` + `Effect`, footprints for all 18 migrations
      (generated from the SQL itself, then reviewed by hand). SQL bodies byte-identical.
- [x] T3 — `src-tauri/src/db/mod.rs`: effect helpers + runner rework + seal.
- [x] T4 — Tests: existing migration tests ported to the new API; fresh-DB effect consistency;
      reconciliation of a structurally damaged database (dropped column, stale counter).
- [x] T5 — Gates: `cargo fmt --check`, `cargo clippy --features desktop -- -D warnings`,
      `cargo test --features desktop` (nightly-2026-02-19).
- [x] T6 — Real-database verification on a **copy** of the dev database, opened with its own
      `db.key` through the crate's SQLCipher: before/after `schema_version`, `tracks.url`,
      `idx_tracks_file_hash`. Temporary test, removed after the evidence was captured.
- [x] T7 — Update `AGENTS.md` §5.5 (migration invariant) and this fork's `odd/README.md` index.
- [ ] T8 — RDD: `gentle_review` inspect + STATUS, then relay the provider envelope unchanged.

## Verification evidence

RED → GREEN, both observed (2026-10-05, `src-tauri/`, nightly-2026-02-19):

- `cargo test --features desktop --lib renumbered_chain_recovers_cleanly` **before** the runner
  change: `FAILED` — `Database(SqliteFailure(..., "duplicate column name: last_walk_complete"))`.
  The emulation reproduces the production crash message exactly.
- `cargo test --features desktop --lib db::` **after**: **24 passed, 0 failed** — includes
  `renumbered_chain_recovers_cleanly`, `fresh_db_satisfies_every_declared_effect` (footprint typo
  guard) and `drifted_schema_reconciles_despite_a_current_counter` (dropped column, counter at
  head), plus every pre-existing migration test.
- `cargo test --features desktop --lib` → **484 passed, 0 failed, 10 ignored**.
- `cargo fmt --check` → clean. `cargo clippy --features desktop -- -D warnings` → clean.
- `cargo check --release --features desktop` (CI gate) → `Finished release profile`.
- **NOT run: the iOS/mobile compile gate** — only `x86_64-unknown-linux-gnu` is installed on this
  host (`rustup target list --installed`), so `--target aarch64-apple-ios` cannot be checked here.
  The change adds no platform-specific code: `rusqlite` + `sqlite_master` only.

Real-database evidence (temporary in-crate test, copy only — the user's `crate.db` was never opened
for write; the test was deleted after capturing this):

```text
BEFORE schema_version=15 tracks=0 tracks.url=false idx_tracks_file_hash=false last_walk_complete=true corrupt_bitrates=0
AFTER  schema_version=18 tracks=0 tracks.url=true  idx_tracks_file_hash=true  last_walk_complete=true corrupt_bitrates=0
```

That is the predicted drift, measured on the exact database that panicked: the counter stops
claiming 15, the two missing objects are created, and nothing is replayed.

Implementation finding worth keeping: when a migration runs because its effect was missing while
its version row already exists (the whole point of the reconciliation), the per-migration
`INSERT INTO schema_version` collided with the primary key. Both inserts are `OR REPLACE` now — a
no-op under `MAX(version)`, and it was the new tests that exposed it.

Review workload: 14 tracked files, **+514/−64**, of which 265 insertions are the declarative
footprints in `schema.rs` and 10 files (`src/services/**`, `src/test_utils.rs`) receive a
one-token `.sql` change because they build test databases from the migration list. Above the ~400
line heuristic; inseparable (the struct change cannot compile without every call site), so it ships
as one unit rather than as chained PRs.

## Decisions

- Scope chosen by the user: engine-level effect reconciliation over a point fix for the renumbered
  chain (the fork-local recovery would be dead code upstream and would leave the class open).
- Git policy: `AGENTS.md` §1.3 (no commits without an explicit request) is honored — the change is
  left in the working tree; the user decides branch and commit afterwards.
- Engram mirror: **not written** — the Engram binary (`/home/th3g3ntl3man/.local/bin/engram`) is
  absent, so `mem_save`/`mem_session_summary` fail in this session. This document is the only
  durable record; the orchestrator said so explicitly instead of pretending the mirror exists.
- The version seal was added rather than left implicit: without it a reconciled database would keep
  reporting `9` (the highest effect-gated migration it had to run) while being fully migrated, and
  the next data-repair migration would be gated against a meaningless number.

## Status: implemented, awaiting native review (T8) and the owner's commit decision
