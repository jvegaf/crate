---
name: crate-rust-backend
description: Crate Rust/Tauri backend conventions — command pattern, generate_handler registration, error handling, service layout, SQLCipher database and migrations, and the desktop/mobile feature gates. Use when reading or editing anything under src-tauri/, adding a Tauri command, changing the schema, or debugging an IPC call that fails.
---

# Crate Rust backend

Read `src-tauri/src/lib.rs` first — it is the composition root: module gating, plugin setup,
service construction, `app.manage(...)`, and the full command registration list.

## Layer layout

```
src-tauri/src/
  lib.rs        composition root + generate_handler! list (the only place commands are registered)
  main.rs       calls crate_lib::run()
  error.rs      CrateError + Result alias
  menu.rs       native menu (desktop only)
  proxy.rs      local proxy server
  commands/     one module per feature area; thin async wrappers around services
  services/     business logic; one file or a directory split by responsibility
  models/       serde DTOs, re-exported wholesale from models/mod.rs
  db/           Database (Arc<Mutex<Connection>>), schema.rs migrations, key/ providers
```

Dependencies arrive in commands as `tauri::State<'_, SomeService>`; services are constructed once
in `lib.rs` and `app.manage`d. `Database` itself is **not** managed — services receive a cloned
`Arc<Mutex<Connection>>` via `Database::connection()`.

## Adding a command

```rust
use crate::error::Result;
use crate::models::Playlist;
use crate::services::PlaylistService;
use tauri::State;

#[tauri::command]
pub async fn create_playlist(
    name: String,
    parent_id: Option<String>,
    context: String,
    playlists: State<'_, PlaylistService>,
) -> Result<Playlist> {
    playlists.create_playlist(name, parent_id, context)
}
```

Then register it — this step is mandatory and easy to forget:

```rust
// src-tauri/src/lib.rs, inside .invoke_handler(tauri::generate_handler![ ... ])
#[cfg(feature = "desktop")]          // only when the backing service is desktop-only
commands::playlist::create_playlist,
```

Rules:

- Argument names are `snake_case` in Rust and `camelCase` in TypeScript calls (Tauri maps them).
- Return `crate::error::Result<T>`, never `Result<_, String>` and never `anyhow`. The only
  `Result<_, String>` commands live in `commands/app.rs` as legacy; do not replicate them.
- Progress/streaming goes through `app: tauri::AppHandle` + `app.emit("kebab-case-event", payload)`
  with `use tauri::Emitter;`.
- The backing service method may be sync; the command stays `async`.

## Errors

`CrateError` (`src-tauri/src/error.rs`) uses `thiserror`, has `#[from]` conversions for
`rusqlite::Error` and `std::io::Error` (so `?` mostly just works), and implements `Serialize` by
emitting its `Display` string. **The frontend therefore receives a plain string rejection.**

- Use `?` to propagate; avoid `unwrap()` outside startup code in `lib.rs`.
- Database locking: `self.conn.lock().map_err(|_| CrateError::LockPoisoned)?`.
- `CrateError::is_transient()` marks the connectivity case surfaced as `Offline`; keep that
  contract when adding network error variants.

## Services

- Single-file services for one clear responsibility (`tag.rs`, `settings.rs`, `device.rs`,
  `backup.rs`, `diagnostics.rs`).
- Directory services split by responsibility, not by type — `playlist/{crud,movement,releases,smart,tracks}.rs`,
  `library/{query,import,relocation,update,duplicates,artwork}.rs`, `discovery/`, `cloud_sync/`,
  `export/`, `follow/`, `audio/`.
- Sync services for pure DB work; `async` + `#[async_trait]` for network/audio/OS work.
- Cross-module helpers that are not IPC surface are `pub(crate)`.
- **Never hold the DB `Mutex` guard across an `.await`.** Use `tokio::sync` locks for async state.

## Database

- Engine: `rusqlite` with `bundled-sqlcipher-vendored-openssl`. No sqlx, no `migrations/` folder.
- One connection behind `Arc<Mutex<Connection>>`; foreign keys enabled; the key is applied per
  connection (`PRAGMA key`) after provisioning from `db/key/`.
- Migrations: SQL string literals in `get_migrations()` in `src-tauri/src/db/schema.rs`, applied by
  index+1 and recorded in `schema_version` inside the same transaction as their DDL.
  **Append a new string; never edit an existing migration.**
- Key providers are selected at compile time (`desktop` file provider, iOS Keychain, Android
  Keystore) behind `provision_key`. A transient provider failure must return `Err`, never a freshly
  generated key — otherwise the existing encrypted database is orphaned.

## Feature gates

| Gate | Meaning |
| --- | --- |
| `#[cfg(feature = "desktop")]` | Needs heavy native deps: audio (`rodio`, `symphonia`), tags (`lofty`), analysis (`stratum-dsp`), USB (`souvlaki`, `sysinfo`), binary export (`binrw`, `base85`, `walkdir`), window-state/process/updater plugins, native menu. |
| `#[cfg(not(feature = "desktop"))]` | Mobile path. There is **no** `#[cfg(feature = "mobile")]` in the source; `mobile` is an empty CLI marker feature. |
| `#[cfg(target_os = "...")]` / `#[cfg(unix)]` | Per-platform implementation, orthogonal to features. |

`default = []` is deliberate: Tauri's mobile build has no `--no-default-features` opt-out, so a
`desktop` default would drag native deps into iOS/Android. Desktop builds pass `--features desktop`;
mobile builds pass `--no-default-features --features mobile`. A `compile_error!` in
`src-tauri/src/db/key/mod.rs` makes any other combination fail loudly.

## Models

Plain structs with `#[derive(Debug, Clone, Serialize, Deserialize)]`, re-exported from
`models/mod.rs`. `Option<T>` for nullable columns, `#[serde(default)]` for collection-like fields so
older payloads still deserialize. `rename_all` is **per struct** — most domain models stay
`snake_case` on the wire; `AppSettings`, menu and diagnostics opt into `camelCase`. Check the struct
before mirroring it in `shared/types/index.ts`.

## Tests

- Unit tests live in a bottom-of-file `#[cfg(test)] mod tests { use super::*; ... }`, or a sibling
  `tests/` module declared with `#[cfg(test)] mod tests;` for larger suites
  (`services/cloud_sync/tests/`, which uses `#[tokio::test]` and in-module mock backends).
- Style is direct: construct, act, `assert_eq!`, `.unwrap()`. No fixture crates.
- Run: `cd src-tauri && cargo test --features desktop` (the feature is required — see above).
  No CI workflow runs `cargo test`; it is a local check.

## Clippy discipline

CI runs `cargo clippy --features desktop -- -D warnings` on **nightly**. Keep intentional unused
items behind a deliberate `#[allow(dead_code)]` with a comment, and scope narrow
`#[allow(clippy::...)]` attributes to the item rather than the module.

## Checklist before returning backend work

1. Command defined, `#[cfg]`-gated if desktop-only, and registered in `generate_handler!`.
2. Errors use `CrateError`; locks mapped to `LockPoisoned`.
3. Migration appended, not edited (if schema changed).
4. `cargo fmt` + `cargo clippy --features desktop -- -D warnings` clean.
5. Mobile still compiles when shared code changed (see `crate-verification`).
