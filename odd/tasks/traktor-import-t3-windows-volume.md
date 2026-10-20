# ODD task: T3 — resolve a Windows `VOLUME` drive on non-Windows hosts

Continues `odd/tasks/traktor-import-fidelity.md` (parent) and
`odd/tasks/traktor-import-fidelity-remaining.md` (T1/T2/T4/T5/T6, landed in `8f7ef31` +
`c4317b2`). **T3 is the only task left in the Traktor import fidelity feature.**

**Planned to be run on a Windows machine.** Nothing in this document requires the Windows
host to reproduce the defect — the defect reproduces anywhere the export is imported, and
the fix itself is host-agnostic. The Windows machine is where the *real* import becomes
verifiable end-to-end, which is why it was deferred there.

## Objective

Make `path::resolve_location` stop silently discarding a Windows drive-letter `VOLUME` on
non-Windows hosts, so an import of a real Traktor export either resolves the files that are
actually present on this host or reports the entry honestly (unresolved-by-location) instead
of mislabelling it as a missing file.

## Why this is the gate on everything else

T1/T2/T4/T5/T6 are all verified — but only through the synthetic fixture and the parser/mapper
unit tests. The real 264-entry export imports **nothing** in practice:

```
data/collection.nml → 264 entries mapped → 0 exists_on_disk → 264 × "file-not-found"
```

Because `resolve_track_id` refuses a track whose path does not exist, no cue, grid, key or
`INFO` field is ever written. The fidelity work is correct and unobservable in the same breath.
T3 is what makes it observable.

## The exact defect

`src-tauri/src/services/traktor/path.rs`, `resolve_location` (~lines 27-56):

```rust
if is_windows_drive(volume) {
    if cfg!(windows) {
        parts.push(volume.to_string());
        absolute = true;
    }
    // ← on non-Windows: the drive is dropped, nothing replaces it
}
```

The real export's location triple is:

```
VOLUME="C:"  DIR="/:Users/:josev/:Music/:BOX/:2609/:"  FILE="Alex Kennon - ... .mp3"
```

The actual files live at `C:\Users\josev\Music\BOX\2609\` — from WSL, reachable at
`/mnt/c/Users/josev/Music/BOX/2609/` (255 of the 264 files were present on the analysis
machine). Dropping `C:` turns the DIR into an absolute `/Users/josev/Music/BOX/2609/...`,
which does not exist on either a Linux host or a Windows host — so the entry is reported as a
missing file when the file is right there under the mount.

## Evidence baseline (captured before any edit; reproduce it yourself)

Run this against the gitignored `data/collection.nml` (264 entries) from the repo root:

```python
import re, collections
s = open('data/collection.nml', encoding='utf-8').read()
entries = re.findall(r'<ENTRY ([^>]*)>', s)
print("ENTRIES:", len(entries))
```

Observed on the analysis machine:

- 264 `ENTRY` blocks; 264 distinct `FILE` names; `VOLUME="C:"` on every one.
- 264 mapped file paths; **0** `exists_on_disk`.
- 246 grids; cue totals `cues=45 / memory_cues=0 / hot_cues=43 / loops=2`.
- `path::resolve_location("C:", "/:Users/:josev/:Music/:BOX/:2609/:", f)` returns
  `/Users/josev/Music/BOX/2609/<f>` on Linux — drive gone, no substitute.

## Decisions carried forward (do not relitigate)

- **D3** (parent doc, confirmed by the probe): the real blocker is the `VOLUME` path, not cue
  units. This task is that decision's implementation.
- **D1** (parent doc): `CUE_V2 START`/`LEN` are plain milliseconds. The earlier
  "mixed seconds/ms" reading is **retracted** — do not reintroduce it.
- **Cue slot base is already aligned** (`628ffe5`). Do not touch `hot_cue_index`.
- **Integer boundary**: mapped positions keep full `f64` ms; the single rounding point is
  `traktor_import::round_ms_to_column` (`8f7ef31`). Do not reintroduce rounding in the mapper.

## The decision this task must make (unresolved — the implementer decides, with a reason)

There are two honest resolutions and they are not equivalent. Pick ONE, document why:

1. **Anchor the drive to a mount.** On WSL, rewrite `C:` → `/mnt/c/`; on macOS, optionally
   probe `/Volumes/<X>/`. Resolves real files, but requires hardcoding mount conventions and
   is wrong for a drive mounted elsewhere.
2. **Report unresolved-by-location, never silently.** Keep the drive in the path
   representation (`C:/Users/...`) and give `resolve_track_id` a distinct reason so the dry-run
   report says "cannot resolve a Windows volume on this host" instead of "file not found".

Option 2 is the conservative default: it never guesses a mount and never lies about why an
entry was skipped. Option 1 is what makes the real export import on this machine. A hybrid
(attempt the mount, fall back to the honest reason) is acceptable if both paths are tested.

**Whatever you pick, the `file-not-found` reason string must stop being emitted for an entry
whose path was mangled by this function.** That is the actual defect: the report is wrong, not
just the path.

## Tasks

- [ ] **T3a — `path.rs`**: stop dropping a Windows drive silently. Introduce a resolved/
      unresolved distinction rather than returning a `String` that pretends to be a real path.
      Keep the behaviour for a non-drive `VOLUME` (a mounted volume path is kept on every OS)
      and for a Windows host (`C:` becomes the prefix) exactly as it is today.
- [ ] **T3b — `traktor_import.rs`**: give `UnmatchedTraktorTrack` a reason that distinguishes
      "the file does not exist" from "the location cannot be resolved on this host". Do not
      report the second as the first.
- [ ] **T3c — `tests/mod.rs`**: unit-test both branches on the host running the suite
      (`cfg!(windows)` and the non-Windows path), plus a case with a non-drive `VOLUME` that
      must keep working unchanged.
- [ ] **T3d — end-to-end observation**: on the Windows machine, run the real import against
      `data/collection.nml` and confirm the counts move from `0 on disk` to the actual number of
      files present, with cues/grids/keys/INFO fields written for those tracks.

## Acceptance criteria

- `cd src-tauri && cargo fmt --check` and `cargo clippy --features desktop -- -D warnings`: clean.
- `cargo test --features desktop`: 0 failures (baseline after `c4317b2`: **621 passed / 0 failed /
  11 ignored**).
- `cargo test --features desktop real_collection -- --ignored --nocapture`: passes; extend it with
  the resolved/unresolved counts once the behaviour is decided.
- `yarn format:check`, `yarn lint:check`, `yarn check:svelte`: clean.
- A Windows-drive entry on a non-Windows host is **never** reported as `file-not-found`.
- A non-drive `VOLUME` still resolves identically on every OS (no regression).
- `services/traktor` stays ungated (AGENTS.md §5.7) — no `#[cfg(feature = "desktop")]`.

## Environment notes for the Windows machine

- `cargo test` must run from `src-tauri/` with `--features desktop`; without it the crate trips
  its own `compile_error!` in `src-tauri/src/db/key/mod.rs` (`default = []`). This is documented
  in `AGENTS.md` §4 and is not a bug.
- Rust is pinned to `nightly-2026-02-19` by `src-tauri/rust-toolchain.toml`. Clippy runs with
  `-D warnings` on nightly.
- `data/collection.nml` is **gitignored** and lives only on the machine that produced it. Copy it
  over if the Windows host does not already have it. `real_collection` skips gracefully when the
  file is absent — a silent skip is not a pass.
- The iOS CI gate (`cargo check --target aarch64-apple-ios`) cannot run off macOS: it fails in
  `cc-rs` looking for `xcrun` before compiling any source, on Linux and Windows alike. That is
  environmental; do not chase it.
- Remote/network access is not required for this task.

## Progress

| Task | State | Evidence |
| --- | --- | --- |
| T3a | not started | — |
| T3b | not started | — |
| T3c | not started | — |
| T3d | not started | blocked on reaching a Windows host with the real export |

Baseline entering this task: `cargo test --features desktop` → **621 passed, 0 failed, 11 ignored**;
`real_collection --ignored` → 1 passed.
