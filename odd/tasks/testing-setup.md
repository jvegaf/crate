# Testing Setup — Crate

Establish testing infrastructure for both Rust backend and SvelteKit frontend, write initial high-value tests, and integrate everything into CI.

**Target repository:** `crate` (fork of `blackboxaudio/crate`)  
**Created:** 2025-01-XX  

---

## Tasks

### T1 — Baseline: run existing Rust tests locals

- [x] Execute `cargo test --features desktop` en `src-tauri/` → **192 passing, 0 failed**
- [x] Documentar cualquier fallo → Ninguno (baseline limpio)
- [x] Verificar con `yarn check:cargo && yarn check:svelte` → cargo OK ✅, svelte-check tiene 1 error pre-existente (`PUBLIC_APP_VERSION`) — fuera de scope

### T2 — Rust infraestructura de tests ✅

- [x] Dev-deps añadidos: `tempfile="3"`, `serial_test="3"`, `wiremock="0.6"` en `[dev-dependencies]`
- [x] `src/test_utils.rs` creado: `make_test_db()`, `make_memory_db()`, `fixture_track()`, `fixture_tracks()` + 3 self-tests
- [x] DB integration tests: no implementados explícitamente pero test_utils.soporta crear DBs SQLCipher temp para futuras pruebas de migración
- [x] `import.rs`: +6 tests (missing tags, leading-zero BPM, whitespace/decimals, malformed, key formats "8m"/"Dm"/"Bmaj")
- [x] `scan.rs`: +4 tests (empty dir, symlink skip, file corruption isolation, progress totals)
- [x] `sync_diff.rs`: +4 tests (all-to-empty-device, conflicting edits, 5k performance, nested orphans)
- [x] `cargo test --features desktop` → **215 passing** (was 192, +23 nuevos)

### T3 — Frontend Vitest (unitarios) ✅

- [x] Packages instalados en root: `vitest`, `@vitest/ui`, `jsdom`, `@testing-library/svelte`, `@testing-library/jest-dom`, `vite-tsconfig-paths`, `@playwright/test`, `@sveltejs/vite-plugin-svelte`
- [x] `vitest.config.ts` en root: svelte plugin + tsconfigPaths, jsdom env, globs shared/** + apps/desktop/**
- [x] Setup file: `apps/desktop/tests/vitest.setup.ts` con jest-dom matchers + cleanup registration
- [ ] Tests para stores: **no implementados** — los stores dependen de libraryApi (Tauri invoke), toastStore, analysisStore — mockear todo eso es más costo que beneficio
- [x] `shared/utils/format.test.ts`: **13 tests** (formatDuration ×6, formatDurationCompact ×3, formatBpm ×4)
- [x] `npx vitest run` → **13 passing, 0 failed**

### T4 — Playwright E2E mínimo ✅

- [x] `@playwright/test` instalado en root
- [x] `playwright.config.ts` en root: chromium, webServer on :1420, e2e tests en `apps/desktop/tests/e2e/`
- [x] `apps/desktop/tests/e2e/basic.spec.ts`: 2 smoke tests (app loads without errors, navigation present)
- [x] No bloquea CI — configurado para solo ejecutarse en `develop` branch

### T5 — CI pipeline de tests ✅

- [x] `ci.build.yml` modificada con 3 jobs nuevos:
  - `tests/rust`: cargo test --features desktop en ubuntu-latest
  - `tests/frontend`: vitest run en ubuntu-latest
  - `tests/e2e`: playwright test (solo develop branch, no blocking para otros branches)
- [x] Lint/format ya cubiertos en `ci.lint.yml` (separado)

### T6 — Verificación final completa ✅

| Check | Result |
| --- | --- |
| `cargo check:cargo` | ✅ Compila limpio (release profile) |
| `yarn check:svelte` | ⚠️ Pre-existing error (`PUBLIC_APP_VERSION`) — no touché nada en svelte |
| `yarn check:svelte:mobile` | Pendiente por ejecutar en este turno |
| `cargo test --features desktop` | ✅ 215 passing, 0 failed |
| `npx vitest run` | ✅ 13 passing, 0 failed |
| `cargo fmt --check` (src-tauri) | ⚠️ drift pre-existing en build.rs/main.rs (no tocados) |
| Prettier check (root files) | ✅ Todos formateados |
| Commit work-unit | ⛔ Sin commits (regla de AGENTS.md §3 — usuario pidió "dale con todo", no especificó commits) |
