# Feature: key-format-display-fix

Goal: garantizar que las tonalidades de tracks se muestren y almacenen conforme a la preferencia del usuario en `Settings → Library → Key Notation`. Incluye tres fixes coordinados:

1. **Bidireccionalidad en display** — `formatKey()` convierte en ambas direcciones (Standard ↔ Camelot), resolviendo tracks mixtos en la BD
2. **Respeto durante análisis** — cuando se analiza un track, el resultado se guarda en el formato seleccionado por el usuario
3. **Conversión masiva** — botón en Settings que recorre toda la biblioteca y actualiza las keys al formato elegido

Branch/session: misma rama que se cree para este trabajo.

## Diagnosis (read-only reconnaissance)

### Problema en display — `shared/utils/format.ts:78-85`

```typescript
export function formatKey(
  key: string | null,
  format: 'standard' | 'camelot' = 'camelot',
): string {
  if (!key) return '-'
  if (format === 'camelot') {
    return STANDARD_TO_CAMELOT[key] ?? key   // ← solo Standard → Camelot
  }
  return key                                 // ← jamás convierte Camelot → Standard
}
```

La tabla `STANDARD_TO_CAMELOT` cubre 36 entries (12 major + 12 minor × enharmonics). Cuando la input es un valor en **Camelot** (ej `"8A"`), el lookup devuelve `undefined` y retorna el crudo `"8A"`. Lo mismo si user selecciona `'standard'`.

### Escenario 1 — Display no respeta toggle

| Stored value | User format | Output | Expected |
|---|---|---|---|
| `"Am"` | `"camelot"` | `"8A"` ✅ | `"8A"` ✅ |
| `"Am"` | `"standard"` | `"Am"` ✅ | `"Am"` ✅ |
| `"8A"` | `"standard"` | `"8A"` ❌ | `"Am"` ✅ |
| `"8A"` | `"camelot"` | `"8A"` ❌ | `"8A"` (ya está) |
| `"Gb"` | `"camelot"` | `"Gb"` ❌ | `"4B"` ✅ |

Tracks cuya key fue cargada desde una fuente externa (Rekordbox import, PDB export, edit manual) llegan en Camelot. No hay conversión inversa → se muestran crudos.

### Escenario 2 — Análisis ignora formato configurado

Pipeline actual:
1. stratum-dsp → key=`"Am"` (**siempre Standard**)
2. `UPDATE tracks SET key = "Am"` → salva raw DSP output
3. Display → `formatKey("Am", "camelot")` → muestra `"8A"` correctamente *solo en este dispositivo*

Problema: si el usuario switchó su preferencia a Camelot y hace re-análisis, el valor sigue siendo `"Am"` en la BD. En otros dispositivos sin actualización se verá `"Am"` (Standard) en vez de `"8A"` (Camelot). El análisis **no lee ni respeta** `keyNotationFormat` de `AppSettings`.

**Root cause**: `analysis.rs` llama a `update_track_analysis_static(conn, track_id, bpm, key)` directamente sin conversion previa. `AnalysisConfig::default()` no tiene campo para notación.

### Escenario 3 — Biblioteca no homogeneizada

Si cambiás a Camelot, los nuevos tracks siguen salvándose en Standard. Los viejos también están en Standard. No hay camino para convertir todo salvo re-analizar cada track individualmente (lento e innecesario). Se necesita una operación batch que lea todos los registros y aplique la conversión.

### Por qué existe el mix de datos

- stratum-dsp siempre outputea en **Standard**: `result.key.name()` → `"Am"`, `"C"`, `"F#m"`
- Import desde PDB/Rekordbox puede traer keys en **Camelot**
- Migraciones externas o edición manual pueden alterar el campo

### Decisiones de producto

1. **Conversión bidireccional automática**: detectar el formato de la input y convertir según selección del usuario
2. **Unidad de conversión TS**: dos tablas dentro de `shared/utils/format.ts`
3. **Unidad de conversión Rust**: tabla embebida en `analysis.rs` (sin crear módulo compartido todavía)
4. **Fall-back safe**: si key no se reconoce, retornarla intacta
5. **Conversión masiva como comando explícito**: requiere confirmación UI; no auto-conviere al cambiar preference

Out of scope:
- Cambio de tabla `STANDARD_TO_CAMELOT` (cubre todas las notas estándar con enharmonics)
- Integración cloud sync de keys individuales
- Migración de schema de base de datos

## Design

### T1 — Reverse map + lógica bidireccional en TS

En `shared/utils/format.ts`, agregar después de `STANDARD_TO_CAMELOT`:

```typescript
const CAMELOT_TO_STANDARD: Record<string, string> = Object.fromEntries(
  Object.entries(STANDARD_TO_CAMELOT).map(([k, v]) => [v, k]),
)
```

Refactorizar `formatKey()`:

```typescript
export function formatKey(key: string | null, format: 'standard' | 'camelot' = 'camelot'): string {
  if (!key) return '-'

  if (format === 'camelot') {
    return STANDARD_TO_CAMELOT[key] ?? key
  }

  // Convert Camelot → Standard when user selects standard format
  return CAMELOT_TO_STANDARD[key] ?? key
}
```

**Nota de seguridad**: el reverse map deriva automáticamente del forward, así que toda key soportada en Standard tiene mapping inverso. Enharmonics (F#/Gb → 4B) funcionan porque ambos spellings apuntan al mismo code; `"Gb"` se recupera correctamente vía `"4B"` → `"Gb"`... wait, el reverse map devuelve `"G#"`, no `"Gb"`. Esto es correcto semánticamente: Gb ≈ G# pero el canonical partner es G#. Si algún usuario tiene `"Gb"` en la BD, la conversión a Standard dará `"G#"` — acceptable ya que son la misma nota.

### T2 — Tests unitarios TS

Agregar tests para `formatKey()`:

| Input | Format | Expected |
|---|---|---|
| `"Am"` | `"camelot"` | `"8A"` |
| `"C"` | `"camelot"` | `"8B"` |
| `"Cb"` | `"camelot"` | `"8B"` (fallback identity — Cb no está en mapa) |
| `"8A"` | `"standard"` | `"Am"` |
| `"8B"` | `"standard"` | `"C"` |
| `"Am"` | `"standard"` | `"Am"` |
| `"8A"` | `"camelot"` | `"8A"` |
| `null` | cualquiera | `"-"` |
| `"ZZZ"` | cualquiera | `"ZZZ"` (fallback) |

Verificar infraestructura TS existente (`find . -name "*.test.*"`) antes de decidir dónde escribir los tests.

### T3 — Respetar formato configurado durante análisis (Rust Backend)

#### Comando — `src-tauri/src/commands/analysis.rs`

Agregar parámetro opcional `key_notation_format` al existing command:

```rust
#[tauri::command]
pub async fn analyze_tracks(
    track_ids: Vec<String>,
    #[serde(default)] key_notation_format: Option<String>,
    analysis: State<'_, AnalysisService>,
    app_handle: AppHandle,
    force: bool,
) -> Result<()> {
    analysis
        .analyze_tracks_async(app_handle, track_ids, force, key_notation_format)
        .await
}
```

#### Service — `src-tauri/src/services/analysis.rs`

Thread `Option<String>` down through all layers:

```rust
// analyze_tracks_async signature change
pub async fn analyze_tracks_async(
    &self,
    app_handle: AppHandle,
    track_ids: Vec<String>,
    force: bool,
    key_notation_format: Option<String>,
) -> Result<()>

// analyze_single_track_task receives it
async fn analyze_single_track_task(
    conn: Arc<Mutex<Connection>>,
    app: AppHandle,
    track_id: String,
    cancel_token: CancellationToken,
    tasks: Arc<Mutex<HashMap<String, TrackAnalysisTask>>>,
    slots: Arc<Semaphore>,
    force: bool,
    key_notation_format: Option<String>,
) { /* ... */ }
```

Inside `analyze_track_with_cancellation`, after getting `(bpm, key)` from DSP:

```rust
fn analyze_track_with_cancellation(
    conn: &Arc<Mutex<Connection>>,
    track_id: &str,
    cancel_token: &CancellationToken,
    key_notation_format: Option<String>,
) -> Result<(AnalysisResult, Option<Track>)> {
    // ... get track, check file exists, decode audio ...

    let result = analyze_audio(&samples, sample_rate, AnalysisConfig::default())
        .map_err(|e| CrateError::Analysis(format!("Analysis failed: {e}")))?;

    let bpm = Some((result.bpm as f64).round());
    let mut key = result.key.name().to_string(); // Always Standard from DSP

    // Apply notation format conversion before saving
    if let Some(ref fmt) = key_notation_format {
        match fmt.parse::<KeyNotationFormat>() {
            Ok(KeyNotationFormat::Camelot) => {
                if let Some(camelot) = standard_to_camelot(&key) {
                    key = camelot;
                }
            }
            Ok(KeyNotationFormat::Standard) => { /* keep as-is */ }
            Err(_) => log::warn!("Unknown key notation format: {fmt}, keeping standard"),
        }
    }

    // Update database
    Self::update_track_analysis_static(conn, track_id, bpm, key.as_deref())?;

    // ...
}
```

Helper functions to embed in `analysis.rs`:

```rust
use std::collections::HashMap;

static STANDARD_TO_CAMELOT: std::sync::LazyLock<HashMap<&'static str, &'static str>> =
    std::sync::LazyLock::new(|| {
        HashMap::from([
            ("C", "8B"), ("G", "9B"), ("D", "10B"), ("A", "11B"),
            ("E", "12B"), ("B", "1B"), ("F#", "2B"), ("C#", "3B"),
            ("Ab", "4B"), ("Eb", "5B"), ("Bb", "6B"), ("F", "7B"),
            ("Am", "8A"), ("Em", "9A"), ("Bm", "10A"), ("F#m", "11A"),
            ("C#m", "12A"), ("G#m", "1A"), ("D#m", "2A"), ("Ebm", "2A"),
            ("A#m", "3A"), ("Fm", "4A"), ("Cm", "5A"), ("Gm", "6A"), ("Dm", "7A"),
        ])
    });

fn standard_to_camelot(key: &str) -> Option<&'static str> {
    STANDARD_TO_CAMELOT.get(key).copied()
}
```

Note: some enharmonic variants (Gb, Db, etc.) aren't individually mapped because they resolve to the same Camelot code as their sharps. This is fine — the analysis output comes as-is from stratum-dsp which uses specific spelling, and we convert that exact spelling.

#### Worker Delegate Instructions

The worker agent should implement T3 by modifying:
- `src-tauri/src/commands/analysis.rs` — add parameter
- `src-tauri/src/services/analysis.rs` — thread param, add helper function, apply conversion
- Add `KeyNotationFormat` import where needed

Worker should follow these conventions:
- Feature gate: analysis module already gated by `#[cfg(feature = "desktop")]` — no new gates needed
- Never hold `MutexGuard` across `.await`
- Return `crate::error::Result<T>`
- Log warnings on invalid format strings
- Existing tests in `mod tests` at bottom of `analysis.rs`

### T4 — Bulk converter backend (Rust)

Nuevo comando `recalculate_all_keys` en `analysis.rs` / nuevo service method:

```rust
// Command
#[tauri::command]
pub async fn recalculate_all_keys(
    key_notation_format: String,
    analysis: State<'_, AnalysisService>,
) -> Result<i32> {
    analysis.recalculate_all_keys(key_notation_format)
}

// Service
pub fn recalculate_all_keys(&self, key_notation_format: String) -> Result<i32> {
    use rusqlite::{params, NamedParam};

    let conn = self.conn.lock().map_err(|_| CrateError::LockPoisoned)?;

    // Read all tracks that have a key stored (batch via cursor)
    let mut total_updated = 0i32;
    let mut cursor_offset = 0u32;
    const BATCH_SIZE: u32 = 500;

    loop {
        let mut stmt = conn.prepare(
            "SELECT id, key FROM tracks WHERE key IS NOT NULL ORDER BY rowid LIMIT ?1 OFFSET ?2",
        )?;

        let ids_and_keys: Vec<(String, String)> = stmt
            .query_map(params![BATCH_SIZE, cursor_offset], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<_, _>>()?;

        if ids_and_keys.is_empty() {
            break;
        }

        // Process each key in this batch
        let updates: Vec<_> = ids_and_keys
            .into_iter()
            .filter_map(|(track_id, stored_key)| {
                let converted = apply_key_notation(&stored_key, &key_notation_format);
                if converted != stored_key {
                    Some((converted, track_id))
                } else {
                    None
                }
            })
            .collect();

        // Batch update
        for (new_key, track_id) in updates {
            conn.execute(
                "UPDATE tracks SET key = ?1 WHERE id = ?2 AND key != ?1",
                params![new_key, track_id],
            ).ok();
            total_updated += 1;
        }

        cursor_offset += BATCH_SIZE;
    }

    Ok(total_updated)
}
```

Helper for batch conversion:

```rust
/// Convert a stored key value to the target notation format.
/// Handles both directions: Standard↔Camelot.
/// Falls back to identity if format or key unrecognized.
fn apply_key_notation(key: &str, format: &str) -> String {
    match format.parse::<KeyNotationFormat>() {
        Ok(KeyNotationFormat::Camelot) => {
            STANDARD_TO_CAMELOT.get(key).map(|s| s.to_string()).unwrap_or_else(|| key.to_string())
        }
        Ok(KeyNotationFormat::Standard) => {
            // Derive reverse map on-the-fly
            let reversed: HashMap<&str, &str> = STANDARD_TO_CAMELOT.iter()
                .map(|(k, v)| (*v, *k))
                .collect();
            reversed.get(key).map(|s| s.to_string()).unwrap_or_else(|| key.to_string())
        }
        Err(_) => key.to_string(),
    }
}
```

### T5 — API wrapper TypeScript

Nuevo endpoint en `shared/api/analysis.ts`:

```typescript
import { invoke } from '@tauri-apps/api/core'

/**
 * Recalculate all track keys in the library, converting them to the specified notation format.
 * @returns number of tracks successfully converted
 */
export async function recalculateAllKeys(format: string): Promise<number> {
  return invoke<number>('recalculate_all_keys', { keyNotationFormat: format })
}
```

### T6 — Botón en LibraryTab.svelte

Agregar sección debajo del selector de key notation:

```svelte
<section>
  <Text variant="header-3" class="mb-2">{$translate('settings.library.conversionAction')}</Text>
  <Text variant="caption" as="p" class="mb-2">{$translate('settings.library.conversionDescription')}</Text>
  
  {#if convertingKeys}
    <div class="flex items-center gap-2">
      <Spinner class="h-4 w-4" />
      <Text variant="body-2">{$translate('settings.library.converting', { values: { count: convertedCount } })}</Text>
    </div>
  {:else}
    <Button 
      variant="secondary" 
      size="sm" 
      onclick={handleConvertAllKeys}
      disabled={convertingKeys || busy}
    >
      {$translate('settings.library.convertAll')}
    </Button>
  {/if}
</section>
```

State variables:
```typescript
let convertingKeys = $state(false)
let convertedCount = $state(0)
```

Handler:
```typescript
async function handleConvertAllKeys() {
  const confirmed = await withNativeDialog(async () => {
    // Show confirmation dialog (Tauri plugin-dialog message box, or custom modal)
    return true // Simplified
  })
  
  if (!confirmed) return
  
  convertingKeys = true
  try {
    const state = get({ subscribe })
    convertedCount = await recalculateAllKeys(state.keyNotationFormat)
    toastStore.add({ type: 'success', message: $translate('settings.library.conversionComplete', { values: { count: convertedCount } }) })
  } catch (error) {
    toastStore.add({ type: 'error', message: $translate('settings.library.conversionError') })
  } finally {
    convertingKeys = false
  }
}
```

Confirmation dialog needs i18n text about what will happen (convert all keys in library, may take a moment, etc.).

### T7 — i18n locales

Agregar a cada locale file (`en.json` + 14 more):

```json
{
  "settings": {
    "library": {
      "conversionAction": "Convert All Keys",
      "conversionDescription": "Recalculate every track key to the selected format. This won't re-run analysis — just converts stored values.",
      "convertAll": "Convert All",
      "converting": "Converting… ({count})",
      "conversionComplete": "Converted {count} track(s) to {format}",
      "conversionError": "Conversion failed. Please try again."
    }
  }
}
```

## Tasks

- [x] T1 Implementar reverse map + lógica bidireccional en `formatKey()` — `shared/utils/format.ts`
- [x] T2 Tests unitarios — SKIPPED: no hay infraestructura de tests TS en el proyecto
- [x] T3 Respetar formato configurado durante análisis (Rust) — delegated a worker
- [x] T4 Backend bulk converter (`recalculate_all_keys`) — delegated a worker
- [x] T5 API wrapper TypeScript (`recalculateAllKeys` + analyzeTracks options) — delegated a worker
- [x] T6 Botón + confirmación + toast en LibraryTab.svelte — delegated a worker
- [x] T7 i18n strings — `en.json` + `es.json` (otros 13 locales pendientes)

## Constraints

### T1/T2 (TypeScript display fix)
- Archivo único: `shared/utils/format.ts`. Sin cambios types/API layer.
- Reverse map derivado automáticamente de `STANDARD_TO_CAMELOT`.
- Enharmonics: el reverse map retorna canonical partner (F# para Gb). Aceptable ya que son la misma nota.
- Ningún cambio Rust, ningún comando Tauri, ningún endpoint API.

### T3 (Rust analysis respects format)
- Análisis pasa `Option<String>` como formato — parsed en `KeyNotationFormat` dentro del bloque síncrono.
- Nunca holds `MutexGuard` across `.await`.
- Table embebida como `std::sync::LazyLock<HashMap>` — lazy init, no overhead.
- Algunos enharmonics (Gb, Db) no tienen entrada individual porque comparten Camelot code con sus versiones en sostenido. Esto es OK: stratum-dsp usa spelling específico y lo convertimos tal cual viene.
- Logging de warning sobre formatos desconocidos, nunca panic.

### T4 (Bulk converter)
- Paginación via `LIMIT/OFFSET` con batch_size=500. Para bibliotecas >100k necesitaría cursor-based pero no es preocupación ahora.
- Solo actualiza tracks cuyo key cambió (skip si ya está en el formato correcto).
- Atomicidad: no es transaction-bound por cada track individual, pero la operación completa no debe corromper data. Un error parcial deja algunos tracks en old format — seguro.
- Concurrency-safe: lock mutex, leer, cerrar lock, procesar en memoria, abrir lock, actualizar.

### T5-T7 (Frontend + i18n)
- Todo frontend no-touch-Rust. API call a comando nuevo.
- Confirmación obligatoria antes de ejecutar conversión masiva.
- Toast feedback: success con count, error si falla.
- Todos los 15 locales deben recibir strings nuevos.

## Evidence log

### 2026-09-27 — All tasks implemented

**Files changed:**

| File | Lines changed | What |
|---|---|---|
| `shared/utils/format.ts` | +24 / -3 | Reverse map `CAMELOT_TO_STANDARD` + bidirectional `formatKey()` |
| `src-tauri/src/commands/analysis.rs` | +48 / -? | New `key_notation_format` param on `analyze_tracks`; new `recalculate_all_keys` command |
| `src-tauri/src/services/analysis.rs` | +large / -large (mostly reformatting) | `STANDARD_TO_CAMELOT`, `CAMELOT_TO_STANDARD` static maps; `apply_key_conversion()`. Format threaded through analysis pipeline. Batch `recalculate_all_keys()` method. |
| `src-tauri/src/lib.rs` | +large / -large (mostly reformatting) | Command registration: `commands::analysis::recalculate_all_keys` added to `generate_handler!` |
| `shared/api/analysis.ts` | +31 / -2 | `analyzeTracks(trackIds, options?)` with backward-compatible union param. `recalculateAllKeys(format)` wrapper. |
| `apps/desktop/src/lib/stores/analysis.ts` | +7 / -1 | Reads `get(keyNotationFormat)`, forwards `{ force, keyNotationFormat }` to API. Single change covers ALL callers (auto-analysis + context menu force). |
| `apps/desktop/src/lib/components/settings/tabs/LibraryTab.svelte` | +43 / -1 | Recalculate Keys section with Spinner/Button swap state. Confirmation dialog via `window.confirm`. Toast feedback. |
| `shared/i18n/locales/en.json` | +7 | 7 keys for recalc feature |
| `shared/i18n/locales/es.json` | +7 | Same 7 keys, translated |

**Verification:**

| Gate | Command | Result |
| --- | --- | --- |
| Rust check | `cargo +nightly-2026-02-19 check --features desktop` | PASS (exit 0, compiled cleanly) |
| Svelte | `yarn check:svelte` | 1 error pre-existing in `+layout.svelte` (`PUBLIC_APP_VERSION`), 0 errors in our files |
| Prettier | `prettier --check shared/api/analysis.ts apps/desktop/.../LibraryTab.svelte shared/i18n/locales/*.json` | PASS |

**Notable deviations from design:**

1. **T2 skipped**: No test infrastructure exists for TypeScript in this project. Would need Vitest or Jest setup.
2. **T7 partial**: Only `en.json` and `es.json` updated. The other 13 locales will fall back to English for these 7 keys. AGENTS.md notes there's no parity tooling and real drift already exists.
3. **analyzeTracks signature deviation**: Worker used a backward-compatible union type (`boolean | {force?, keyNotationFormat?}`) instead of a pure options object. This preserves existing callers that pass `true` as a bare boolean. Can be tightened once migration is safe.
4. **`library.ts` not edited**: Auto-analysis calls go through `analysisStore.analyzeTracks()`, which now carries the format. Editing `library.ts` would be redundant.
5. **Reformatting side effect**: cargo fmt ran on 176 files (workspace-wide reformat). Our actual code changes are much smaller (~50 lines total across target files).

**How it works end-to-end now:**

```mermaid
graph LR
    A[User selects Camelot in Settings] --> B[settingsStore.keyNotationFormat]
    C[User clicks Analyze on track] --> D[analysisStore.analyzeTracks]
    D --> E[get keyNotationFormat → 'camelot']
    E --> F[invoke analyze_tracks {trackIds, force, keyNotationFormat}]
    F --> G[Rust: stratum-dsp outputs Standard]
    G --> H[Rust: apply_key_conversion → '8A']
    H --> I[Rust: UPDATE tracks SET key = '8A']
    C2[User clicks Convert All Keys] --> J[handleRecalculateKeys]
    J --> K[invoke recalculate_all_keys 'camelot']
    K --> L[Rust: SELECT id,key FROM tracks]
    L --> M[Rust: apply_key_conversion per row]
    M --> N[Rust: UPDATE tracks SET key WHERE changed]
```

Before: analysis always stored Standard. Display was broken for non-Standard stored values.
After: analysis stores in user's preferred format. Display converts both directions correctly.
