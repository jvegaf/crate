# Feature: analysis-skip-stubs-and-force

Goal: detener el análisis innecesario durante la importación y respetar los metadatos que el archivo ya trae (BPM y key en sus tags), mientras preservar el re-análisis explícito del usuario desde el menú contextual.

Branch/session: misma rama `library-folder-scan`, mismo worktree. Este cambio toca una región de `services/analysis.rs` que no se cruza con la prioridad `library-scan-performance` (cuyo diff es 7 archivos de library/import/query/scan/schema/artwork/comando). Los dos candidatos son separables sin necesidad de branch separado.

## Diagnosis (read-only reconnaissance, 2026-09-24)

El problema tiene dos causas separadas que actúan en cascada. Ninguna se arregla sola:

### Cause A — Stubs que tiran el tag a la basura

`src-tauri/src/services/library/import.rs:405-412` tiene dos métodos hardcodeados:

```rust
fn extract_bpm(&self, _tag: &dyn Accessor) -> Option<f64> {
    None // Will be populated by Rekordbox import or analysis
}
fn extract_key(&self, _tag: &dyn Accessor) -> Option<String> {
    None // Will be populated by Rekordbox import or analysis
}
```

Reciben el tag (que contiene TBPM/TKEY/INITIALKEY/BPM/tmpo según el formato) y lo descartan con el guión bajo `_tag`. Dos llamadas en `import.rs:81,84` y `156,157`. Así Crate importa 200+ tracks y cada uno llega a la DB con `bpm=NULL, key=NULL` — el único camino es que el DSP corra sobre cada uno.

### Cause B — Sin skip guard ni force flag

Todos los paths automáticos (after-scan, after-import, after-discovery-import) llaman `analysisStore.analyzeTracks(ids)` incondicionalmente (`library.ts:99,154`; `OrchestratorLayer.svelte:148`). El backend no chequea `bpm`/`key` existentes ni tiene opción de skip. Y el menú contextual "Analyze" usa exactamente el mismo endpoint Tauri (`invoke('analyze_tracks', {trackIds})`): no hay way de distinguir "auto-import" de "re-analizar manualmente".

### Consecuencia práctica

Importar un track que ya tiene BPM/key en sus tags → import lo guarda como NULL (stub) → análisis automático lo corrige clobberando los valores originales (`analysis_source='crate'`). Si nunca corrés nada en el front, tus tags son inservibles en esta app.

### Qué necesita lofty 0.22.4

Version actual: `src-tauri/Cargo.toml:66`. Scout confirma:
- `Tag::get_string(&ItemKey::IntegerBpm)` → texto plano ("120" etc.) → parse a f64
- `Tag::get_string(&ItemKey::InitialKey)` → texto plano ("Am", "Bm", "5A" etc.)
- `ItemKey` ya está en scope via `lofty::prelude::*`
- Necesita añadir `use lofty::tag::{Tag, TagType}` (precedente en `artwork.rs:15`)
- `TaggedFile::primary_tag()` retorna `&Tag` ya concreto → cambiamos el parámetro de `&dyn Accessor` a `&Tag`
- Write support está disponible (`TagExt::save_to_path`) para tests, pero NO lo necesitamos para implementar

### Decisiones de producto (confirmadas por usuario)

1. **Saltear si AMBOS están completos**: skip solo si `track.bpm.is_some() && track.key.is_some()`. Si falta uno o ambos, corre el DSP y completa los dos.
2. **Menu "Analyze" → force**: cuando el usuario pide explícitamente "Analyze", siempre corre el DSP aunque tenga todo. Requiere un parámetro `force: bool` opcional en el comando Tauri.

Deliberadamente fuera de alcance:
- Extraer BPM/key de PDB (Rekordbox USB export): eso es lectura de export.pdb binario, ya existe parcialmente en `export/pdb/reader.rs:352-354` pero no alimenta `Track.bpm`/`.key`. Fuera de scope.
- Streaming decode (ya documentado en `analysis-worker-limit.md`): separar decoding de análisis en batches pequeños. Cambia la estructura del decodificador, fuera de scope.
- Migrar cloud sync merge para ser condicional con bpm/key: orthogonal, HLC gating no trazado por el scout.

## Design (fixed before implementation)

Un slice que toca backend + frontend, distribuido en tareas pequeñas dentro de un mismo archivo crítico.

### T1 — Fijar extractors en import.rs

- Cambio de firma: `extract_bpm(&self, _tag: &dyn Accessor)` → `extract_bpm(&self, tag: &Tag)`. El caller pasa `&Tag` desde `primary_tag()`/`first_tag()`, así que este cambio es compatible.
- Agregar `use lofty::tag::{Tag, TagType};` al top de import.rs (precedente: `artwork.rs:15`).
- Implementar:
  ```rust
  fn extract_bpm(&self, tag: &Tag) -> Option<f64> {
      tag.get_string(&ItemKey::IntegerBpm)
          .and_then(|s| s.trim().parse::<f64>().ok())
  }
  fn extract_key(&self, tag: &Tag) -> Option<String> {
      tag.get_string(&ItemKey::InitialKey)
          .map(|s| s.trim().to_string())
  }
  ```
- `trim()`: limpiar espacios/trailing zeros de "120  " o "Am ".
- Tests: sintetizar un WAV con tags BPM/Key vía lofty, verificar import populatea los campos.

### T2 — Saltear análisis completo, agregar force flag

**Comando** (`src-tauri/src/commands/analysis.rs`):
```rust
#[tauri::command]
pub async fn analyze_tracks(
    track_ids: Vec<String>,
    analysis: State<'_, AnalysisService>,
    app_handle: AppHandle,
    #[serde(default)] force: bool, // NEW: default=false means skip-if-complete
) -> Result<()> {
    analysis.analyze_tracks_async(app_handle, track_ids, force).await
}
```

**Backend** (`src-tauri/src/services/analysis.rs`):
- `analyze_tracks_async` recibe `force: bool`. Lo pasa como arg de `spawn_blocking`.
- `analyze_single_track_task` recibe `force: bool`. Antes del DSP:
  ```rust
  // Skip if track already has complete analysis data (only when not forcing).
  if !force && Self::has_complete_analysis(&conn, &track_id)? {
      // Emit completion with existing data instead of running DSP.
      Self::emit_skip_completion(&app, &track_id, &tasks)?;
      return;
  }
  ```
- Nueva función privada:
  ```rust
  fn has_complete_analysis(conn: &Arc<Mutex<Connection>>, track_id: &str) -> Result<bool> {
      let conn = conn.lock().map_err(|_| CrateError::LockPoisoned)?;
      let row = conn.query_row(
          "SELECT bpm, key FROM tracks WHERE id = ?1",
          [track_id],
          |row| Ok((row.get::<_, Option<f64>>(0)?, row.get::<_, Option<String>>(1)?)),
      )?;
      Ok(row.0.is_some() && row.1.is_some())
  }
  ```
- Emitir completion sinDSP: leer bpm/key actuales del DB (igual que `has_complete_analysis`), emitir `Completed` con esos valores. Esto evita tener que hacer doble query — puedo usar `get_existing_analysis` que devuelve `(Option<f64>, Option<String>)`.

### T3 — Wiring frontend

**API** (`shared/api/analysis.ts`):
```typescript
export async function analyzeTracks(
  trackIds: string[],
  force?: boolean,  // NEW: defaults to undefined/false
): Promise<void> {
  return invoke('analyze_tracks', { trackIds, force: force ?? false })
}
```

**Store** (`apps/desktop/src/lib/stores/analysis.ts`):
```typescript
async analyzeTracks(trackIds: string[], force?: boolean): Promise<void> {
  // ... existing logic ...
  await analysisApi.analyzeTracks(trackIds, force)
}
```

**OrchestratorLayer**:
- Línea 112 (context menu): `await analysisStore.analyzeTracks(trackIds, true)` → force=true
- Línea 150 (discovery import): `.analyzeTracks(result.tracks.map((t) => t.id))` → omite force (default=falso, auto-skip-if-complete)
- `library.ts` ambas calls (línea 102 y 156): omiten force → behavior automático por defecto (skip si completo)

**Nota UX importante**: el menú contextual ahora fuerza re-análisis. Si el track ya tiene BPM/Key, el DSP los sobrescribe con valores computados. Eso es intencional: el usuario pidió explícitamente "re-analizar". Para los paths automáticos (scan, discovery import, purchase), el valor por defecto `force=false` saltea cuando ya tienen datos.

## Tasks

- [x] T1 Backend: cambiar `extract_bpm`/`extract_key` de stubs a lectura real de tags (`ItemKey::IntegerBpm`, `ItemKey::InitialKey`) + test con WAV con tags
- [x] T2 Backend: agregar `force: bool` al comando + passthrough hasta `analyze_single_track_task` + lógica de skip (`has_complete_analysis` + `emit_skip_completion`)
- [x] T3 Frontend: API `force?: boolean` + store forwards + OrchestratorLayer pasa `force=true` para menú contextual
- [x] T4 Verify: `cargo fmt --check -- --config tab_spaces=4`, `cargo clippy --features desktop -- -D warnings`, `cargo test --features desktop`, `yarn check:svelte`

## Constraints

- `#[cfg(feature = "mobile")]` aparece en ningún lado. `lofty` es opcional (`src-tauri/Cargo.toml:66`) pero ya se importa en `import.rs` y otros módulos. La feature gate de `lofty` debe mantenerse limpia — el módulo `analysis` ya está `#[cfg(feature = "desktop")]`.
- Nunca holds `MutexGuard` across `.await` (AGENTS.md §5.5). `has_complete_analysis` lock→query→unlock todo síncrono. OK.
- `ItemKey` está en scope vía `lofty::prelude::*` (ya importado en import.rs:6). Solo necesito añadir `use lofty::tag::{Tag, TagType}`.
- `truncate`/`format`/`trim` strings: safe on whatever language the key is in (ID3v2 UTF-8/UTF-16 handled by lofty).
- No commit: AGENTS.md §1.3 prohíbe commitear sin pedido explícito del usuario.
- Artifacts en English; conversation en Rioplatense Spanish.

## Evidence log

### 2026-09-26 — T1/T2/T3 implemented

Implementation notes:

- **Naming deviation**: the design sketch above suggested `has_complete_analysis` returning `bool`
  plus a second query. The implementation uses a single `get_existing_analysis` returning
  `Result<Option<(f64, String)>>`, so the skip path reads the pair once and can emit it back to the
  UI without a second query. Semantics are identical: skip only when **both** are `Some`.
- **Command arg**: the design sketch marked `force` with `#[serde(default)]`; per the explicit task
  instruction and the existing `relocate_track` precedent (`commands/library.rs:88-96`), `force` is
  a plain required `bool`. The TS wrapper always sends it (default `false`), so no caller can omit
  it.
- **Skip guard placement**: right after the `cancel_token.is_cancelled()` check and **before**
  `acquire_analysis_slot`, so a skipped track never occupies a DSP worker slot.
- **`import.rs` test**: uses in-memory `Connection` + `Tag::new(TagType::Id3v2)`; no WAV synthesis.
  The helpers are pure tag parsing, so a synthetic tag exercises the same code path without I/O.

Commands run and real results:

```
cd src-tauri && cargo fmt -- --config tab_spaces=4          # exit 0
cd src-tauri && cargo fmt --check -- --config tab_spaces=4  # exit 0, no diff
cd src-tauri && cargo test --features desktop               # 192 passed; 0 failed; 0 ignored
```

Focused confirmation:

```
cargo test --features desktop get_existing_analysis
  -> services::analysis::tests::get_existing_analysis_only_returns_a_complete_pair ... ok
cargo test --features desktop extract_
  -> services::library::import::tests::extract_bpm_reads_integer_bpm_tag ... ok
  -> services::library::import::tests::extract_key_reads_initial_key_tag_trimmed ... ok
  -> 9 passed; 0 failed
```

### 2026-09-26 — rustfmt churn normalization

A previous agent ran bare `cargo fmt`, which picked up the machine-global
`~/.config/rustfmt/rustfmt.toml` (`tab_spaces = 2`) and reindented 12 Rust files, adding ~4.700
lines of pure whitespace churn. Re-running from `src-tauri` with the explicit
`cargo fmt -- --config tab_spaces=4` collapsed it: the `*.rs` diff went from 12 files / ~4.700
whitespace lines to the genuinely-touched files only (see `git diff --stat -- '*.rs'`), and
`cargo fmt --check -- --config tab_spaces=4` is clean. **Never run bare `cargo fmt` in this repo.**

### 2026-09-26 — T4 verification (fresh worker, read-only)

| Gate | Command | Result |
| --- | --- | --- |
| Rust format | `cargo fmt --check -- --config tab_spaces=4` | PASS (exit 0, no diff) |
| Rust clippy | `cargo clippy --features desktop -- -D warnings` | FAIL — pre-existing, see below |
| Rust tests | `cargo test --features desktop` | PASS — 192 passed; 0 failed; 0 ignored |
| Svelte | `yarn check:svelte` | PASS — 0 errors, 0 warnings |
| ESLint | `yarn lint:check` | PASS (exit 0) |
| Prettier | `yarn format:check` | FAIL — only untracked root `models.json`, not committed/CI-relevant |

Clippy failure was **not caused by this feature**. The single error was
`src-tauri/src/services/device.rs:95` (`clippy::double_ended_iterator_last`,
`device.split('/').last()` -> `next_back()`). `device.rs` is byte-identical to `HEAD` (it did not
appear in `git status`), and re-running clippy with only that lint allowed
(`cargo clippy --features desktop -- -D warnings -A clippy::double_ended_iterator_last`) compiled
clean. So the feature files introduced zero clippy warnings; the failure was a pre-existing base
issue that also failed CI's exact `cargo clippy --features desktop -- -D warnings`
(`.github/workflows/ci.lint.yml:124`).

That pre-existing lint was then fixed as a **separate one-line change** outside this feature
(`device.rs:95`, `last()` -> `next_back()`, not committed). After it,
`cargo fmt --check -- --config tab_spaces=4` and `cargo clippy --features desktop -- -D warnings`
both pass, so T4 is fully green.

End-to-end behavior after the change: automatic paths (`import_tracks`, `scanMusicLibraryFolder`,
purchase auto-analyze) call `analyzeTracks(ids)` -> `force=false` -> the backend skips the DSP and
emits a terminal `Completed` event when `bpm` **and** `key` are both already present; the
context-menu "Analyze" passes `force=true` -> the DSP always runs.
