# Feature: openkey-notation-and-key-migration

Goal: agregar **OpenKey** como notación de tonalidad de primera clase (y default de visualización), y
migrar los valores legacy en `tracks.key` que hoy no se resuelven.

Branch: `openkey-notation-and-key-migration` (desde `dev`).

## Origen del pedido

El usuario reportó que "unas keys tienen color y otras no". La causa medida: hay valores en
`tracks.key` con notación `m`/`d` que el normalizador no resuelve, y por eso quedan sin chip de color
**y al final del auto-orden** (ver más abajo, es la consecuencia más grave).

Esos valores son **anteriores** a los colores y vinieron por el tag del archivo: `import.rs:466-471`
copia `ItemKey::InitialKey` (TKEY / INITIALKEY / `com.apple.iTunes:initialkey`) **verbatim**, sin
normalizar. Crate nunca los escribió por su cuenta.

## Decisión de producto (aprobada por el usuario)

1. **Los `m`/`d` legacy son convención harmony** (número Camelot + letra), **no** OpenKey verdadero:
   `11d` = 11B = La mayor, `1m` = 1A = La♭ menor. Sin rotación.
2. **OpenKey entra como notación de primera clase y pasa a ser el default de visualización.**
3. **La DB nunca guarda OpenKey: internamente canonical = Camelot.**
4. Sin comando de preview; el usuario hace backup antes de migrar (acción ya existente en
   `Settings → General`: `createBackup` / `restoreFromBackup`).

## Por qué el canonical es Camelot y no OpenKey (razón dura, no preferencia)

`A`/`B` (Camelot) y los nombres de nota (standard) **se auto-describen**. `m`/`d` **no**: dos
convenciones distintas comparten exactamente el mismo alfabeto.

Si la DB guardara OpenKey, la migración dejaría de ser idempotente y se corrompería sola:

| Paso | Valor almacenado | Significado |
| --- | --- | --- |
| Legacy | `11d` | 11B (convención harmony) |
| 1ª migración | `4d` | 11B ✓ (OpenKey verdadero de 11B) |
| 2ª migración | `9d` | ✗ corrompido: lee `4d` como legacy (4B) → 9d |

Y peor: con el setting en OpenKey, **cada track nuevo analizado reproduce la ambigüedad** que la
migración acaba de resolver.

Con el canonical en Camelot el problema **desaparece por construcción**: como la DB nunca guarda
OpenKey, cualquier `m`/`d` almacenado **solo puede ser legacy harmony**. No hay colisión. La
migración pasa a ser idempotente (después de correrla no queda ningún `m`/`d`) y re-ejecutarla es
seguro.

## Non-goals

- **OpenKey no se almacena nunca.** Si el setting es OpenKey, se guarda el Camelot correspondiente.
- **El export de dispositivos no cambia**: `export/generation.rs:235-236` (`writer.get_or_create_key`)
  alimenta el PDB de Rekordbox con `tracks.key`, o sea la notación almacenada. Hoy ya es Camelot por
  default, así que no hay regresión. Que el export siga la notación de *visualización* es una decisión
  aparte, no de este trabajo.
- **Keys teóricas** (`Cb`, `Fb`, `E#`, `B#`, `Cbm`): quedan sin resolver y por lo tanto sin color. Son
  válidas pero rarísimas en una biblioteca real, y requieren una política de spelling enarmónico.
  Documentado como gap conocido, no se toca acá.
- Sin comando de preview/dry-run (decisión del usuario; el backup es la red).
- No se cambia la notación en que se almacenan los valores que ya son Camelot o standard.

## Diagnóstico medido

Corrí las funciones reales contra una batería de valores plausibles. Resuelven: todos los códigos
Camelot, todos los spellings standard, las variantes enarmónicas de `format.ts`, y las formas de
Beatport (`G Minor`, `G min`, `Gmaj`). **No resuelven**: `m`/`d` (legacy harmony), accidentales en
Unicode (`G♯`, `G♭`), accidentales escritos con palabra (`F sharp minor`), y las keys teóricas.

### El trap del conversor existente

`recalculate_all_keys` (y su botón en `LibraryTab.svelte:108`) **no toca** los valores que no
reconoce: `apply_key_conversion` devuelve la key intacta (`unwrap_or_else(|| key.to_string())`) y la
escritura filtra `WHERE key != ?1` → 0 filas. O sea: hoy la migración **no falla, no hace nada y
reporta éxito**. Este trabajo lo arregla haciendo que el camino de storage reconozca el legacy.

### Consecuencia más grave que el color

`keySortValue` devuelve `999` para un valor irresoluble, así que **todos los tracks con `m`/`d` se van
al final del auto-orden**, después de cualquier key resoluble, y entre ellos ordenan por rating y
después por id. Para una biblioteca mayormente en `m`/`d`, el "auto-orden por armonía" da un orden
casi arbitrario. Se arregla solo cuando los valores se resuelven.

## Design

### T1 — Rust: OpenKey como formato de visualización con storage Camelot

En `src-tauri/src/models/settings.rs` y `src-tauri/src/services/analysis.rs`:

- Variante `KeyNotationFormat::OpenKey` (el `#[serde(rename_all = "lowercase")]` ya serializa
  `"openkey"`), sumada al `Display` existente.
- `#[default]` pasa de `Camelot` a `OpenKey`.
- Una noción explícita de **formato de storage** (`openkey` ⇒ `camelot`), con un comentario que cite
  la razón de auto-descripción de arriba. `apply_key_conversion(key, "openkey")` debe comportarse
  como el target Camelot.
- **El camino de storage reconoce el legacy harmony**: `^([1-9]|1[0-2])[md]$` (case-insensitive) →
  mismo número con `m`→`A` y `d`→`B`. Esto es lo que hace que el botón "Convert All Keys" existente
  migre de verdad, de forma idempotente.
- Tests en `analysis.rs` (usa el harness existente): `11d`→`11B`, `1m`→`1A`, `11D`→`11B`, `12m`→`12A`;
  y que **no** toque `8A`, `Am`, `13d`, `s-key`; y que correrlo dos veces dé el mismo resultado
  (idempotencia).

### T2 — TS: OpenKey de visualización

En `shared/types/index.ts` y `shared/utils/format.ts`:

- `KeyNotationFormat` suma `'openkey'`.
- `formatKey(key, 'openkey')`: resolver el valor almacenado (Camelot o standard, sin ambigüedad) y
  emitir el **OpenKey verdadero**: `openKey = ((camelotNumber + 4) % 12) + 1`, con `A`→`m` y `B`→`d`.
  Fórmula ya verificada contra la tabla publicada de 24 keys (es la misma que usa harmony en su rama
  de Camelot: `((camelotNum + 4) % 12) + 1`).
- Los valores sin resolver siguen pasando intactos (fallback actual).

### T3 — TS: normalizador de storage (legacy harmony + Unicode)

En `shared/utils/format.ts`:

- `keyToCamelot` acepta el legacy `m`/`d` como **convención harmony**. Comentario obligatorio: esto
  solo es válido para valores **almacenados**, nunca como entrada de display, porque la DB nunca
  guarda OpenKey (ver la razón dura de arriba).
- Accidentales en Unicode: `♯` (U+266F) → `#`, `♭` (U+266D) → `b`. Son un caso real: herramientas
  externas escriben esos caracteres en el tag.
- Tests: `11d`→11B, `1m`→1A, `11D`→11B, `G♯ Minor`→1A, `G♭`→2B; y que `4d` **no** se interprete como
  OpenKey verdadero (documenta la decisión).

### T4 — Settings: toggle de 3 opciones + i18n

- `LibraryTab.svelte:236-252`: el toggle de 2 botones pasa a 3 (`OpenKey`, `Camelot`, `Standard`).
- Default de TS en `shared/stores/settings.ts:83` (`'camelot'` → `'openkey'`).
- i18n: `settings.library.keyNotationOpenKey` en **los 15 locales** (la palabra es idéntica en todos;
  no hay oración nueva que traducir).

### T5 — Verificación funcional

- Tests de que un valor legacy (`11d`) resuelve a 11B y por lo tanto **recibe color** (`keyColor`) y
  **ordena** (`keySortValue`) como Camelot 11B. Cierra el pedido original de punta a punta.
- `yarn check:svelte:mobile` debe seguir en 0: todo esto vive en `shared/`.

## Tasks

- [ ] T1 Rust: `KeyNotationFormat::OpenKey` + storage Camelot + legacy harmony en el camino de storage + tests
- [ ] T2 TS: `formatKey` con OpenKey verdadero
- [ ] T3 TS: `keyToCamelot` con legacy harmony + accidentales Unicode
- [ ] T4 Settings: toggle de 3 opciones + default + i18n en 15 locales
- [ ] T5 Tests de color y orden para los valores migrados
- [ ] T6 Gates: `cargo test`, `check:svelte`, `check:svelte:mobile`, `vitest`, `format:check`, `lint:check`

## Constraints

- Rust pinned a `nightly-2026-02-19`; clippy con `-D warnings`.
- `cargo test --features desktop` es obligatorio (sin `desktop` el crate no compila por el
  `compile_error!`).
- **Nunca** guardar OpenKey en `tracks.key`.
- El legacy `m`/`d` es **convención harmony**, no OpenKey verdadero: `11d` = 11B. No "corregir" esto
  con la rotación.
- Serde: `KeyNotationFormat` es lowercase en el wire.
- Preservar el fallback de valores irresolubles (pasan intactos, nunca se inventa una key).
- Review workload: el diff debe quedar bien por debajo de ~400 líneas.

## Evidence log

### Commits

| Commit | Contenido |
| --- | --- |
| `c58a6fb` | `feat(keys): resolve legacy harmony keys and add OpenKey display` |
| `a7c20cd` | `feat(keys): add OpenKey notation with Camelot storage and migrate legacy keys` |
| `0ccd9e5` | `feat(settings): offer OpenKey in the key notation setting` |

El orden es deliberado para que **cada commit deje el árbol consistente**: el TS entiende `'openkey'`
antes de que Rust lo emita, y la UI lo ofrece después. El doc ODD queda sin trackear (AGENTS.md §1.2).

### Gates (verificados por el orquestador y por un verificador independiente)

| Gate | Resultado |
| --- | --- |
| `cargo test --features desktop` | 529 tests: **519 passed, 0 failed**, 10 ignored |
| `cargo clippy --features desktop -- -D warnings` | limpio |
| `cargo fmt --check` | limpio |
| `yarn vitest run` | 8 archivos, **118 passed** (baseline 109) |
| `yarn check:svelte` | 0 errores, 0 warnings |
| `yarn check:svelte:mobile` | 0 errores, 0 warnings |
| `yarn format:check` | exit 0 |
| `yarn lint:check` | exit 0 |
| JSON parse 15 locales | todos parsean, clave presente con el mismo valor |

Los 10 chequeos adversariales del verificador independiente pasaron. Un hallazgo propio fue
**refutado**: asumí que mobile podía tener un toggle de notación desactualizado que mostrara ninguna
opción seleccionada; grep case-insensitive sobre `apps/mobile/` confirma que **no existe ninguna
superficie de settings ahí**.

### Errores míos que los workers detectaron (registro honesto)

1. **Tabla de canonicalización del target Standard**: mezclé la familia de bemoles del reverse map del
   frontend con la familia de sostenidos del mapa de Rust, y pedí canonicalizar valores que el mapa de
   Rust ni contiene. Ninguna tabla puede dar ambas. Resuelto con la opción mínima: no tocar los mapas,
   aceptar la familia de sostenidos, y dejar la limitación enarmónica como contrato con test propio.
2. **Test de biyección de 24 keys**: era matemáticamente imposible, y por la razón central del diseño —
   `keyToCamelot` lee `m`/`d` como legacy sin rotación, así que releer un valor OpenKey cae en **otra**
   key. Se convirtió en dos tests: la tabla publicada (biyección de la rotación) y el *legacy-read
   shift* como forma ejecutable de por qué el storage no puede guardar OpenKey.

### Hallazgos registrados, fuera de alcance

- **Preexistente, `shared/utils/format.ts`**: el comentario del reverse map dice "sharps, except Ab"
  pero el mapa derivado devuelve bemoles en 9 celdas enarmónicas por last-wins del orden de inserción.
  No lo toqué (es un cambio de display visible ajeno a este pedido) y **no interactúa** con OpenKey:
  el camino de OpenKey nunca consulta `CAMELOT_TO_STANDARD`.
- **Preexistente, pre-migración**: con display `camelot` o `standard`, un valor legacy todavía sin
  migrar se muestra crudo (`11d` en vez de `11B`), porque los brazos `camelot`/`standard` de `formatKey`
  siguen usando lookup exacto y no el resolver tolerante. Con el display default (OpenKey) **no se
  manifiesta**, y desaparece al correr la migración. Es la misma causa raíz que la inconsistencia
  label-vs-color ya reportada (`G Minor` con color pero mostrando `G Minor`).

### Cómo migrar

1. `Settings → General` → backup de la DB.
2. `Settings → Library` → **Convert All Keys** con la notación elegida. El target OpenKey guarda
   Camelot, así que la migración deja `11d` → `11B` y es **idempotente**: correrla de nuevo no cambia
   nada (antes era un no-op silencioso que reportaba éxito sin migrar).
