---
name: crate-frontend
description: Crate SvelteKit 5 frontend conventions — the IPC wrapper layer, classic svelte/store factories, controllers and hooks, Svelte 5 props/snippets, $shared aliasing, i18n locale handling, and Tailwind 4 semantic tokens. Use when reading or editing apps/desktop, apps/mobile, or shared/, or when wiring UI to a backend command.
---

# Crate frontend

The desktop app is a SvelteKit SPA (`ssr = false`, `adapter-static`). Mobile is a scaffold that
imports from `$shared` to prove the alias resolves — do not add desktop-only assumptions to
`shared/`.

## Layering — respect the direction

```
routes/+page.svelte, +layout.svelte      orchestration, $state/$derived, wiring
  -> lib/controllers/*.ts                framework-free command layer (DI factory, no Svelte imports)
  -> lib/hooks/use*.ts                   lifecycle/effects: onMount, keybindings, drag-drop
  -> shared/stores/*.ts | lib/stores/*   state + async actions + toast/error surfacing
  -> shared/api/*.ts                     one thin wrapper per Rust command
  -> invoke(...)                         Tauri IPC
```

Components are presentation only: typed props and callbacks. **Never call `invoke` from a
`.svelte` file** — the only exception is OS integration (`@tauri-apps/plugin-opener`,
`@tauri-apps/api/event` listens in routes).

Deep children reach backend behaviour through the `pageActions` writable
(`apps/desktop/src/lib/stores/pageActions.ts`), set on mount and nulled on destroy — not prop
drilling. Dialogs and context menus are globally orchestrated through `OrchestratorLayer` /
`ModalOrchestrator`, not mounted ad hoc.

## Adding an IPC wrapper (`shared/api/`)

```ts
import { invoke } from '@tauri-apps/api/core'
import type { Tag, TagCategory } from '../types'

/**
 * Create a new tag category (max 4 allowed)
 */
export async function createTagCategory(name: string, color?: string): Promise<TagCategory> {
	return invoke<TagCategory>('create_tag_category', { name, color: color ?? null })
}
```

- The command string is the Rust `snake_case` fn name; TS parameters are camelCase.
- Optional arguments are normalized to `null`, not omitted.
- No try/catch here: errors propagate as **plain strings** (Rust `CrateError` serializes to its
  `Display` message) and are caught one layer up in stores, which set `state.error` and call
  `toastStore.error(...)`.
- Import types relatively inside `shared/` (`from '../types'`), never by alias.
- `shared/api/index.ts` is a **partial** barrel (10 of 19 modules). Import the module directly
  (`$shared/api/discovery`) rather than relying on the barrel.

## State

Shared state is still **classic `svelte/store` factories**, not runes. Only
`shared/utils/virtualizer.svelte.ts` uses `$state`.

```ts
function createXStore() {
	const { subscribe, set, update } = writable<XState>(initialState)
	return {
		subscribe,
		async load() { /* set loading, call api, set error, toast on failure */ },
		reset() { set(initialState) },
	}
}

export const xStore = createXStore()
export const someDerived = derived(xStore, ($s) => /* ... */)
```

Placement rule:

- Cross-platform capable → `shared/stores/` and re-export from `shared/stores/index.ts`.
- Desktop-only → `apps/desktop/src/lib/stores/` and re-export from that barrel.
- A store missing from its barrel is invisible to pages, which import `from '$lib/stores'`.

## Svelte 5 conventions

- Props: `let { variant = 'secondary', class: className = '', onclick, children }: Props = $props()`.
  Rename reserved `class` to `className` when destructuring.
- Events are `onclick={...}` attributes or `on...` callback props. Legacy `on:click` does not exist
  in this codebase (the only colon syntax is `transition:fade`, `transition:scale`,
  `transition:slide`).
- Slots are snippets: declare `children: Snippet`, `footer?: Snippet` in `Props`, render with
  `{@render children()}`, pass with `{#snippet footer()}...{/snippet}`.
- Use `bind:` for form controls (`<Checkbox bind:checked={...} />`).

## Cross-app imports

`$shared` points at the repo-root `shared/` directory; it is declared in each app's
`svelte.config.js` under `kit.alias`, so SvelteKit wires both the Vite resolver and the generated
tsconfig paths. Each app's `tsconfig.json` must also keep `../../shared/**/*.ts` and
`../../shared/**/*.svelte` in `include`, otherwise `svelte-check` misses them.

Use `$shared/...` for shared modules and `$lib/...` for app-local ones. Do not reach across apps
with relative paths.

## i18n

- `svelte-i18n` v4, configured in `shared/i18n/index.ts`; locales are registered lazily and
  initialized with `fallbackLocale: 'en'` so `$translate` never throws before the saved language
  loads.
- In markup: `{$translate('library.tracks')}`; with values:
  `{$translate('library.dragDropHint', { values: { shortcut } })}`.
- In plain `.ts` (stores/controllers) `$translate` is unavailable — use `get(translate)('key', ...)`.
- Keys are nested JSON with `camelCase` leaves (`nav.discovery`, `library.searchPlaceholder`); ICU
  placeholders and plurals are supported.
- **15 locale files exist and all remain in use.** No tooling enforces parity and drift exists
  today (`toast.tracksExported` is missing from several locales; `pl/ro/tr/uk` also miss
  `discovery.unsupportedUrl`, `menu.hide`, `menu.hideOthers`, `menu.showAll`). Minimum bar: add the
  key to `en.json`; update every locale you can translate correctly rather than leaving English
  placeholders silently.

## Styling (Tailwind 4)

- Single stylesheet: `apps/desktop/src/style.css`, imported once from `routes/+layout.svelte`.
- Semantic tokens are declared in `@theme` and backed by CSS variables that switch per theme:
  `bg-surface-0..2`, `text-text-primary`, `border-stroke`, `text-danger|warning|success|info`.
- Dark/light mode is attribute-driven: the settings store sets `data-theme`, `data-accent` and
  `data-font` on `document.documentElement`. **Do not use `dark:` variants.**
- Brand colours bypass `@theme` and are hand-written `@utility` classes because Tailwind's
  `@theme` processing breaks reactivity for dynamic accents:
  use `bg-brand-primary`, `text-brand-primary`, `border-brand-primary`, `ring-brand-primary`, and
  the explicit `bg-brand-primary-10` style variants. `bg-brand-primary/10` does **not** work.
- Class order is enforced by `prettier-plugin-tailwindcss`, pinned to the desktop stylesheet.

## Naming

`PascalCase.svelte` for components (grouped in feature folders, each with an `index.ts` barrel),
`camelCase.ts` for everything else (`tagController.ts`, `useAppSetup.ts`, `pageActions.ts`).

## Checklist before returning frontend work

1. New command? Wrapper in `shared/api/`, then store/controller action, then UI.
2. Store exported from the correct barrel.
3. No `invoke` in components; no `dark:`; no `bg-brand-primary/10`; no `on:click`.
4. New user-facing string added to `en.json` (and other locales where possible).
5. `yarn check:svelte` (and `:mobile` if `shared/` changed) plus `yarn format:check` clean.
