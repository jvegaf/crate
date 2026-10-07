# Tagger Provider Toggle

## Goal
Allow users to enable/disable individual tag providers (Beatport, Traxsource, Bandcamp) from the Tagger settings tab.

## Approach
- New KV setting `tagger_providers_enabled` stored as a JSON object (`{"beatport":true,"traxsource":false,...}`)
- Default: all providers enabled — missing key or missing entry in the map = enabled (forward-compatible for new providers)
- Tagger service receives the enabled provider id list from the command layer; filters in `search_all` and guards `search_by_url`
- Desktop settings UI adds per-provider `Checkbox` rows in `TaggerTab.svelte`
- Setting is device-local (not in cloud sync whitelist)

## Tasks

### 1. Rust model + settings service
**Files**: `src-tauri/src/models/settings.rs`, `src-tauri/src/services/settings.rs`
- Add `use std::collections::HashMap;` to models/settings.rs
- Add field `pub tagger_providers_enabled: HashMap<String, bool>` to `AppSettings` (before closing brace, near other tagger fields)
- Default impl: `tagger_providers_enabled: HashMap::new()` (empty = all enabled by absence logic)
- In `services/settings.rs` `get_settings()`: add match arm for `"tagger_providers_enabled"` parsing JSON string into `HashMap<String, bool>` via `serde_json::from_str`
- No `set_setting` change needed — it's generic key/value

### 2. Rust tagger service + command
**Files**: `src-tauri/src/services/tagger/mod.rs`, `src-tauri/src/commands/tagger.rs`
- Add `pub fn provider_ids(&self) -> Vec<&str>` method to `TaggerService` that maps `self.providers` to their `id()` values
- Add `enabled_providers: &[String]` parameter to `search_all`, `search_and_rank`, `search_by_url`
- In `search_all`: filter iteration to providers where `enabled_providers.contains(provider.id())`
- In `search_and_rank`: pass through to `search_all`
- In `search_by_url`: check parsed provider id is in `enabled_providers`; if not, return `Ok(None)` (same as "provider not available in this build")
- Update `provider_priority()` if it's used externally, or keep it internal-only
- In `commands/tagger.rs`: compute `enabled_ids: Vec<String>` from `tagger_service.provider_ids()` filtered by `app_settings.tagger_providers_enabled.get(id).copied().unwrap_or(true)`, pass `&enabled_ids` to all service calls
- Update any tests in `services/tagger/tests.rs` that call the changed signatures

### 3. Shared types + settings store
**Files**: `shared/types/index.ts`, `shared/stores/settings.ts`
- Add `taggerProvidersEnabled: Record<string, boolean>` to `AppSettings` interface (near other tagger fields)
- In store state interface: add `taggerProvidersEnabled: Record<string, boolean>`
- In `load()`: map with `settings.taggerProvidersEnabled ?? {}`
- Add setter: `setTaggerProvidersEnabled(provider: string, enabled: boolean)` following the optimistic update + `setSetting` pattern (key: `tagger_providers_enabled`, value: JSON.stringify of the full updated map)
- Add derived export: `export const taggerProvidersEnabled = derived(...)`

### 4. Frontend UI + i18n
**Files**: `apps/desktop/src/lib/components/settings/tabs/TaggerTab.svelte`, `shared/i18n/locales/en.json` + 14 other locales
- In TaggerTab: add a "Providers" section with per-provider `Checkbox` rows for beatport, traxsource, bandcamp
- Wire each to `settingsStore.setTaggerProvidersEnabled(provider, checked)`
- Read state from `$taggerProvidersEnabled` derived store
- Use `$translate('settings.tagger.providers.beatport')` etc. for labels
- i18n keys to add to en.json under `settings.tagger`:
  - `providersEnabled` — section label ("Enabled Providers")
  - `providers.beatport` — "Beatport"
  - `providers.traxsource` — "Traxsource"
  - `providers.bandcamp` — "Bandcamp"
- Best-effort update 14 other locale files with translated labels

### 5. Verify
```bash
yarn format:check && yarn lint:check && yarn check:svelte && yarn check:svelte:mobile && yarn check:cargo
cd src-tauri && cargo clippy --features desktop -- -D warnings
```

## Notes
- `providerCapsuleClass` in `tones.ts` and error rendering in modals are already provider-agnostic — no changes needed
- `storeUrl.ts` `StoreName` is a display concern (WOAR URLs), separate from tagger providers — no changes needed
- Traxsource is `#[cfg(feature = "desktop")]` in the provider vec — mobile builds naturally exclude it; the UI (desktop-only TaggerTab) handles this implicitly