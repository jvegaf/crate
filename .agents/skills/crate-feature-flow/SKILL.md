---
name: crate-feature-flow
description: End-to-end recipe for adding a capability to Crate across the Rust backend and the SvelteKit frontend — from command and model to API wrapper, store, controller and component, including the review-size and mobile-safety checks. Use when implementing a feature that touches both src-tauri/ and the frontend.
---

# Crate feature flow

Use this when a feature crosses the stack. For single-layer edits load `crate-rust-backend` or
`crate-frontend` instead.

## Step 0 — Decide the shape before writing

Answer these first; they determine every later file:

1. Is the capability **desktop-only** or does it need to work on mobile? This decides the
   `#[cfg(feature = "desktop")]` gate and whether the code may live in `shared/`.
2. Does it need **new persisted state**? If yes, a migration is appended in
   `src-tauri/src/db/schema.rs` (never edited) and the model changes.
3. Does it **cross the IPC boundary**? Most features do. If it is pure UI, skip steps 1–3 below.
4. Does it need **new user-facing text**? Then `shared/i18n/locales/*.json` changes too.

## Step 1 — Model

Add or extend the DTO in `src-tauri/src/models/<area>.rs`, re-exported from `models/mod.rs`.
Decide `rename_all` deliberately: the default is `snake_case` on the wire, and that is what most
domain models use. Mirror the exact field names in `shared/types/index.ts`.

## Step 2 — Service

Put the logic in `src-tauri/src/services/<area>.rs` (or a responsibility-split submodule if the
directory form already exists). Return `crate::error::Result<T>`. Acquire the DB lock with
`self.conn.lock().map_err(|_| CrateError::LockPoisoned)?` and never hold it across an `.await`.

## Step 3 — Command + registration

Thin `#[tauri::command] pub async fn` in `src-tauri/src/commands/<area>.rs` that delegates to the
service. Then **register it** in `generate_handler!` in `src-tauri/src/lib.rs`, with
`#[cfg(feature = "desktop")]` on its own line if desktop-only. A missing registration is a runtime
failure, not a compile error.

## Step 4 — API wrapper

Add one thin wrapper to `shared/api/<area>.ts`. If the module is new, create it (do not expect
`shared/api/index.ts` to list it — that barrel is partial).

```ts
export async function doThing(id: string, option?: string): Promise<Thing> {
	return invoke<Thing>('do_thing', { id, option: option ?? null })
}
```

## Step 5 — State

Add the state where it belongs:

- Cross-platform → `shared/stores/<area>.ts`, exported from `shared/stores/index.ts`.
- Desktop-only → `apps/desktop/src/lib/stores/<area>.ts`, exported from that barrel.

Follow the existing factory shape (`createXStore()`, `subscribe` + async actions, `reset()`),
surface failures through `state.error` + `toastStore.error(...)`, and successes through
`toastStore.success(...)`.

## Step 6 — Controller and/or hook

Business orchestration that must be testable without Svelte goes into
`apps/desktop/src/lib/controllers/<area>Controller.ts`: a DI factory whose deps include getter
functions for reactive values. Lifecycle wiring (mount, keybindings, drag-drop) goes into
`apps/desktop/src/lib/hooks/use*.ts`, composed from `useAppSetup.ts`. Re-export both through their
barrels, and add anything the UI triggers through `pageActions` to the `PageActions` interface.

## Step 7 — UI

Component in `apps/desktop/src/lib/components/<area>/` as `PascalCase.svelte`, exported from that
folder's `index.ts`. Typed `Props`, callback props for events, snippets for slots. Use semantic
Tailwind tokens and `$translate` for every string.

If the feature is a modal or context menu, register it in the orchestrator rather than rendering it
locally.

## Step 8 — Strings

Add keys to `shared/i18n/locales/en.json`. Update the other 14 locales where you can translate
correctly; parity is not enforced by tooling, so leaving English text inside a translated locale is
a visible defect.

## Step 9 — Verify

Run the gates in `crate-verification`. Minimum for a cross-stack feature:

```bash
yarn format:check && yarn lint:check && yarn check:svelte && yarn check:svelte:mobile && yarn check:cargo
cd src-tauri && cargo clippy --features desktop -- -D warnings
```

If the Rust side changed in any way that could leak into mobile, also run the mobile
`cargo check --target ... --no-default-features --features mobile` from that skill.

## Step 10 — Review size

Before finishing, look at the diff size. This project explicitly protects review focus and its own
strategy targets focused changes (roughly ≤400 changed lines). If the feature exceeds that, stop and
propose a slice split rather than opening one large change.

## Common failure modes

- Command written but not registered in `generate_handler!`.
- Wrapper added but the store is missing from its barrel, so the page never sees it.
- Field names camel-cased in TS while the Rust struct serializes `snake_case`.
- Desktop-only command without `#[cfg(feature = "desktop")]` — green locally, red on mobile CI.
- A new migration edited into an existing one instead of appended.
- A component calling `invoke` directly.
- User-facing string added only to `en.json` when the feature ships to all locales.
