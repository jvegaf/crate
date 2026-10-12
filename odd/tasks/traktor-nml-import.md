# Feature: traktor-nml-import

Goal: leer la colección de **Traktor** (`collection.nml`) e importarla a Crate — tracks, cues, loops,
beat grid y playlists con su jerarquía de carpetas — sin escribir nunca el archivo de Traktor.

Branch: `traktor-nml-import` (desde `dev`).

## Origen del pedido

El usuario pidió investigar cómo tener sincronización con Traktor, citando dos referencias:

- `https://github.com/digital-dj-tools/dj-data-converter` — conversor CLI NML ↔ `rekordbox.xml`
  (**MIT**), con las reglas de mapeo documentadas y fixtures reales.
- `https://github.com/pestrela/music/tree/master/traktor` — base de conocimiento Traktor; el README
  se mudó a `pestrela/dj_kb`.

Y el antecedente propio: **Harmony** (`jvegaf/harmony`, repo del usuario) ya tiene una integración
NML completa en TypeScript — ~7.439 líneas en `src/main/lib/traktor/**` más 1.015 de IPC y 860 de UI
de settings. Es el diseño de referencia y también la lista de errores a no repetir.

## Decisión de producto (aprobada por el usuario)

1. **Import primero**: Traktor → Crate. El **export queda fuera** de esta feature (slice posterior).
2. **Read-only sobre Traktor**: Crate nunca escribe `collection.nml` acá. Riesgo sobre la biblioteca
   del usuario: nulo.
3. **S1 sin UI**: el primer slice es solo capa de datos + IPC. El panel de cues se decide después.
4. **Fixture**: el usuario exporta a propósito una colección chica desde Traktor (10–20 tracks con
   hot cues, loop, memory cues, beat grid analizado, una playlist anidada y, si se puede, un archivo
   con `&` o apóstrofo en el nombre). **El fixture es la fuente de verdad del formato**: el modelo de
   grid (S2) se diseña a partir de él, no de memoria.

## Formato NML — lo mínimo que hay que respetar

- Raíz: `NML{VERSION, HEAD, MUSICFOLDERS, COLLECTION, SETS, PLAYLISTS, SORTING_ORDER, INDEXING}`.
- `COLLECTION/ENTRY`: `MODIFIED_DATE/TIME`, `AUDIO_ID`, `TITLE`, `ARTIST`,
  `LOCATION{DIR,FILE,VOLUME,VOLUMEID}`, `ALBUM`, `MODIFICATION_INFO`,
  `INFO{RANKING, PLAYTIME(_FLOAT), RELEASE_DATE, COLOR, COVERARTID, KEY}`, `TEMPO{BPM}`,
  `MUSICAL_KEY`, `CUE_V2[]` con `GRID{BPM}` **anidado**.
- **Paths**: `DIR="/:Users/:a/:Music/:"` + `VOLUME="C:"` + `FILE`. Encoding propio con `/:` — **sin
  percent-decoding y sin `file://`**. Es la parte más frágil del formato.
- **Playlists**: `PLAYLISTS → NODE → SUBNODES/PLAYLIST{ENTRIES}`; las entradas referencian tracks por
  `PRIMARYKEY = "C:" + dir + "/:" + file`.
- **Cues**: `CUE_V2` con `START`/`LEN` en **milisegundos float**, `REPEATS=-1`, `DISPL_ORDER`,
  `HOTCUE`. Tipos `0..5` = Cue / Fade-in / Fade-out / Load / **AutoGrid (4)** / Loop.
- **Rating**: `0..255` en pasos de 51 (0–5 estrellas). **Fechas**: `YYYY/M/D` sin padding.
- Trap del formato real: hay **dos cues `AutoGrid` en el mismo `START`** con `TYPE 0` y `TYPE 4`
  (`~/Code/harmony/src/main/lib/traktor/__tests__/fixtures/collection.nml:858-859`). La identidad de
  un cue necesita `position + type + hotCueSlot`, no `position` sola.
- Conversión Rekordbox→Traktor de referencia: `dj-data-converter` documenta un **offset de 26 ms** en
  mp3. Relevante solo si en el futuro se sintetizan grids, no para el import puro.

## Fixture real medido: `data/collection.nml`

Exportado por el usuario desde **Traktor Pro 4**, `NML VERSION="20"` (no es el v3.x que asumían Harmony y
`dj-data-converter`). 264 `ENTRY`, 3.411 líneas, 404 KB.

| Hecho | Medición | Consecuencia de diseño |
| --- | --- | --- |
| `GRID` anidado | 246 `GRID`, 246 cues `TYPE="4"`, **246/246 con `<GRID>` y ninguno self-closing** | El grid **no es un campo del track**: vive dentro del cue `TYPE=4` |
| Grid por `TEMPO` | 261 `TEMPO`, 15 entries con `TEMPO` pero **sin** `GRID` | El grid no se deriva del `TEMPO` |
| Nominación del grid | 245 cues `TYPE=4` se llaman `AutoGrid`, **uno se llama `Beat Marker`** | El grid se detecta por `TYPE="4"`, **jamás** por `NAME` |
| `DISPL_ORDER` | 291/291 en `0` | Inservible como orden de cue |
| Cues | `TYPE="0"`×43, `TYPE="4"`×246, `TYPE="5"`×2, **`TYPE 1/2/3` ausentes** | El mapeo real es `(TYPE, HOTCUE)`, no `TYPE` solo |
| `HOTCUE` | `-1`×246, **`0`×31**, `1`×11, `2`,`3`,`6` ×1 | **Base 0.** `-1` = no es hot cue |
| `INFO KEY` | Heterogéneo: `3d`, `9m`, `8B`, `Ab Major`, `Am`, `Gb` | No hay UNA notación. Ver arbitraje abajo |
| `MUSICAL_KEY VALUE` | 0–11 mayor / 12–23 menor (validado abajo) | **Fuente de verdad de la key**, mejor que `INFO KEY` |
| `MUSICFOLDERS` | Ausente | No hace falta modelarlo |
| `SETS` | `ENTRIES="0"` | Vacío; se preserva como vacío |
| `VOLUME` | `C:` en los 264 | Un solo volumen en este fixture |
| `PRIMARYKEY` | `"C:/:Users/.../:" + FILE` | **Regla confirmada**: `VOLUME + DIR + FILE` |
| Playlists | 15 `NODE`, 12 `PLAYLIST`, 3 `SMARTLIST` | Hay jerarquía real **y** smartlists con lenguaje de query |
| Nombres de playlist | incluye `_LOOPS`, `_RECORDINGS`, `Preparation`, `Set Preparation` | `_LOOPS`/`_RECORDINGS` son internas de Traktor |
| `SORTING_INFO`/`CRITERIA` | 6 / 4, `PATH="$COLLECTION"` o nombre de playlist | Config de orden por playlist; no esencial |
| Escapado XML | 24 atributos `FILE` con entidades o apóstrofos, incluido uno con `’` tipográfica (U+2019) | El escapado es un caso real, no teórico |

### Arbitraje de la notación de key (resuelto con datos)

Se comparó cada `INFO KEY` contra el enum numérico `MUSICAL_KEY VALUE` del **mismo** track, sobre los 167
entries que tienen ambos (comparación por clase de altura + modo, para no pelear por bemoles/sostenidos):

| Población | Hipótesis | Aciertos |
| --- | --- | --- |
| 78 entries con `KEY` = `<n><m\|d>` | **A: Open Key verdadero** (`11d`=B♭, `1d`=C, `3d`=D) | **76/78** |
| 78 entries con `KEY` = `<n><m\|d>` | B: convención legacy harmony (`11d`=11B=A) — la que Crate usa hoy | **0/78** |
| 89 entries con `KEY` en otra notación (`8B`, `Ab Major`, `Am`, `Gb`) | resolución tolerante de Crate | solo 55/89 |

Conclusiones vinculantes:

1. **El `INFO KEY` de Traktor es Open Key verdadero y está rotado respecto de Camelot.** La convención
   "legacy harmony" de Crate (documentada en `odd/tasks/openkey-notation-and-key-migration.md`) **no**
   aplica a valores que vengan de Traktor: esa regla lee `11d` como La mayor cuando Traktor significa
   Si♭ mayor (6B). Guardar el string crudo corrompería ~78 tracks de este fixture.
2. **El importador convierte a Camelot canónico y nunca almacena `INFO KEY` crudo.** Es la única forma de no
   reintroducir la ambigüedad de alfabeto que ese documento ya había cerrado.
3. **`MUSICAL_KEY VALUE` es la fuente preferida** (enum, sin ambigüedad); `INFO KEY` es fallback. El 34/89 de
   desacuerdo en el grupo "otra notación" muestra que el texto libre no es confiable.
4. El enum de Harmony (0–11 mayor, 12–23 menor) queda **validado empíricamente**: 76/78 contra Open Key.

## Qué hace Harmony (referencia) y qué NO copiar

Portables: `nml-parser.ts` (357), `nml-writer.ts` (808), `mappers/{track,cue,key,playlist}.ts` (786),
`sync/sync-engine.ts` (307), `sync/conflict-resolver.ts` (218), `sync/auto-sync-service.ts` (285),
`sync/{worker-pool,traktor-worker-manager}.ts` (525).

**Defectos documentados que no se heredan:**

| Defecto | Evidencia | Consecuencia |
| --- | --- | --- |
| El parser **nunca** lee el `<GRID BPM>` anidado | `nml-parser.ts:225-253` | El grid de Traktor se pierde al leer |
| La DB **no tiene** columna `gridBpm` | `schema.ts:96-110` | Cualquier export de cues **destruye** la precisión del `TYPE 4` |
| Export **aplana** las carpetas a `$ROOT` | `nml-writer.ts:495-530` | Se pierde la jerarquía de playlists (Crate ya tiene `parent_id`) |
| Backup + `writeFile` directo | `export-worker.ts:165-173`, `nml-writer.ts:393-396` | Sin temp-file + rename, sin lock, sin detectar Traktor abierto |
| Borrados de tracks no propagan en ninguna dirección | `sync-engine.ts:108-133` | Cues huérfanos |
| Playlists "Harmony-authoritative" one-way | `nml-writer.ts:742-790` | Borrar playlists de Traktor fue un **breaking change** |
| `src/main/lib/traktor/**` **excluido** de vitest | `vitest.config.ts:14` | Esos tests nunca corrieron en CI; uno afirma comportamiento obsoleto |

**Aprendizaje directo para acá:** los tests de esta feature tienen que estar **dentro** del gate.

## Estado de Crate

| Superficie | Hoy |
| --- | --- |
| Track | `src-tauri/src/models/track.rs:7` (snake_case). `file_path` UNIQUE, `file_hash` (blake3) indexado, `rekordbox_id` como precedente de ID externo |
| Cues | Tabla `cues` (`db/schema.rs:140`) + modelo `models/cue.rs:36`. **Cero comandos IPC**: nada la lee ni la escribe |
| Beat grid | **No existe**. `waveform_data` es un BLOB opaco en `tracks` |
| Playlists | `parent_id` (carpetas anidadas), smart lists, `sort_order` + `position` |
| Jobs largos | Patrón `app.emit("<kind>-progress")` + `cancel_flag: Arc<AtomicBool>` + `export_checkpoints` |
| XML | `quick-xml 0.41` ya está en `Cargo.lock` (transitivo) |

### Contrato real de la tabla `cues`

```sql
CREATE TABLE cues (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position_ms INTEGER NOT NULL,
    type TEXT NOT NULL,
    loop_end_ms INTEGER,
    hot_cue_index INTEGER,
    name TEXT,
    color TEXT,
    CONSTRAINT valid_type CHECK (type IN ('memory', 'hot', 'loop')),
    CONSTRAINT loop_has_end CHECK (type != 'loop' OR loop_end_ms IS NOT NULL)
);
CREATE INDEX idx_cues_track ON cues(track_id);
```

Más `_hlc TEXT NOT NULL DEFAULT ''` + `idx_cues_hlc` (`schema.rs:333,348`) de la capa de cloud-sync, y
`cues` ya aparece en backup (`services/backup.rs:111`) y en el pipeline de cloud-sync
(`services/cloud_sync/pipeline/{rows,buckets}.rs`).

## Decisiones abiertas

**Resueltas por el fixture:**

1. **`hot_cue_index` es base 1 en Crate (`1..=8`).** Los `HOTCUE` de Traktor son **base 0** (`-1` = no es hot
   cue; `0` aparece 31 veces). El importador mapea `HOTCUE n → n+1`; el futuro exportador, al revés. Razón de
   la base 1: Crate ya exporta a Rekordbox/Pioneer, donde los hot cues son 1-based. **No** se relaja la
   validación de S1 para aceptar 0: el 0 de Traktor es un valor de origen, no un índice de Crate.
2. **Notación de key:** ver el arbitraje de arriba. Traktor = Open Key verdadero; se importa como Camelot.
3. **El grid se lee del cue `TYPE="4"`** (nunca por `NAME`), con su `BPM` en el `<GRID>` anidado.

**Siguen abiertas:**

4. **`position_ms` es `INTEGER` y Traktor usa milisegundos float.** Se acepta el redondeo en S3 y se
   documenta; **no** se migra la columna a REAL sin decisión explícita. Si el grid necesita sub-ms, se decide
   en S2 con el fixture a la vista.
5. **No hay UNIQUE sobre la identidad de un cue.** La unicidad (`track_id, position_ms, type, hot_cue_index`)
   se garantiza en el servicio, no en el schema (implementado en S1).
6. **Política de playlists internas**: `_LOOPS` y `_RECORDINGS` son de Traktor. ¿Se importan, se omiten por
   nombre, o se omiten de otro modo? Se decide antes de S4.
7. **Smartlists de Traktor**: 3 en el fixture con lenguaje propio (`$PLAYED == TRUE`, `$RATING == 5`,
   `$IMPORTDATE > ...`). Traducirlo a `smart_rules` JSON está **fuera de alcance**: se importan como playlist
   normal o se omiten, decisión de S4.
8. **Qué es fuente de verdad por dirección** (para S5, export): se decide antes de escribir el writer.

## Non-goals de esta feature

- **No se escribe `collection.nml`**: eso es un slice posterior con su propia decisión de seguridad de
  escritura (temp-file + rename atómico + backup + lock).
- Sin UI de cues en S1 (decisión del usuario). El editor completo es una feature aparte.
- Sin smartlists ni stems de Traktor (`SETS`/streaming no se modelan).
- Sin `watch`/auto-sync en background: el import es explícito.
- No se toca la notación de keys ni el export a dispositivos.

## Design

### S1 — Capa de datos + IPC de cues (slice actual, sin UI)

Prerequisito de todo: `cues` es hoy una tabla huérfana.

**T1 — Servicio `src-tauri/src/services/cue.rs`.**
- CRUD: listar por track, reemplazar la lista completa de un track, borrar por track.
- Validación: `loop` exige `loop_end_ms > position_ms`; `hot` exige `hot_cue_index`;
  `memory` no lleva ninguno de los dos; rango razonable de `hot_cue_index` (1..=8, el estándar de
  Traktor/Rekordbox).
- **Dedup por identidad** `(position_ms, type, hot_cue_index)` al escribir una lista: es el bug que
  Harmony peleó tres veces. `INSERT OR REPLACE` no alcanza porque el `id` es un UUID por cue.
- Escritura de `_hlc`: seguir el patrón del servicio existente que ya escribe filas con HLC (mirar
  `services/playlist/tracks.rs`), nunca asignarlo a mano con un valor inventado.
- `Result<crate::error::Result<T>>`, `lock().map_err(|_| CrateError::LockPoisoned)?`, y **jamás**
  sostener el guard a través de un `.await`.
- Tests unitarios en el mismo archivo con el harness existente (`src-tauri/src/test_utils.rs`).

**T2 — Comandos `src-tauri/src/commands/cue.rs` + registro.**
- Cada `#[tauri::command]` listado en `generate_handler!` (`lib.rs`). Un comando no registrado no
  existe.
- Los comandos de cues **no** son `desktop`-only: la tabla existe en mobile y el modelo `Cue` no usa
  features. Verificar que `services/mod.rs` y `commands/mod.rs` los expongan sin `#[cfg]`.
- Argumentos: camelCase en TS, snake_case en Rust (los argumentos son el caso especial del wire).

**T3 — TS: tipo + API.**
- `shared/types/index.ts`: tipo `Cue` espejo, **snake_case** (el modelo Rust no tiene `rename_all`, y
  `CueType` sí serializa lowercase: `'memory' | 'hot' | 'loop'`).
- `shared/api/cues.ts`: wrappers `invoke`. Ningún componente llama `invoke` directo.
- Barrel `shared/api/index.ts` (es un barrel parcial: hay que agregar la línea).

**T4 — Store.**
- `shared/stores/cues.ts` como **factory clásica de `svelte/store`** (no runes: los stores compartidos
  siguen siendo clásicos).
- Re-export en `shared/stores/index.ts` (si no, la página no lo ve).

**T5 — Gates.** `cargo test --features desktop`, `cargo clippy --features desktop -- -D warnings`,
`cargo fmt --check`, `yarn check:svelte`, `yarn check:svelte:mobile`, `yarn vitest run`,
`yarn format:check`, `yarn lint:check`.

### S2 — Modelo de beat grid (va antes de S4)

**El hecho que define el diseño:** una grilla no es un BPM, es un par **(BPM, ancla)**. Traktor guarda el BPM
en `TEMPO BPM` y el ancla en el `START` del cue `TYPE="4"`; sin el ancla, la grilla no dice dónde cae el
primer pulso.

**Crate hoy no persiste el ancla en ningún lado.** `services/export/anlz/mod.rs:163` construye el
`BeatGridTag` sintetizándolo desde el BPM, y `services/export/anlz/tags/pqtz.rs:55-70` genera las entradas con
`time_ms` empezando en `0.0`: la grilla que se exporta a un pendrive **asume que el primer pulso está en 0 ms**.
`BeatGridEntry` ya tiene los campos necesarios (`beat`, `tempo`, `time_ms`), pero el origen nunca viaja desde
la DB. Es el mismo supuesto que obliga a `dj-data-converter` a aplicar una **corrección de offset de 26 ms** en
mp3: una grilla real casi nunca arranca en 0.

**Modelo decidido: tres columnas nuevas en `tracks`, agregadas al final.**

| Columna | Tipo | Contenido |
| --- | --- | --- |
| `grid_bpm` | `REAL` nullable | BPM de la grilla |
| `grid_anchor_ms` | `INTEGER` nullable | posición del primer pulso |
| `grid_source` | `TEXT` nullable | `'traktor'`, `'crate'`, … |

Razones:

1. **Se mantienen separadas de `tracks.bpm`.** Un BPM y un ancla de orígenes distintos producen una grilla
   incoherente. `tracks.bpm` ya es la medida de Crate (`analysis.rs:733`, con `analysis_source = 'crate'`):
   el trío preserva la medida propia y la grilla importada por separado.
2. **Se agregan al final, no en el medio.** El repo lee `Track` con listas de columnas posicionales
   (`row.get(N)`); insertar una columna en el medio desplaza todos los índices. Agregar al final de la tabla y
   al final de cada `SELECT` deja los índices existentes intactos. Ver `odd/tasks/sql-positional-regression-tests.md`.
3. **Se descartó una tabla aparte `track_beatgrids`.** Aislaría el riesgo posicional, pero obligaría a registrar
   un bucket nuevo en cloud-sync y backup, y `tracks` ya participa de los dos: el total de partes móviles es
   mayor, no menor.
4. **`grid_anchor_ms` es `INTEGER`**, consistente con `cues.position_ms`. El redondeo a milisegundos enteros ya
   quedó medido: no pierde datos.

**Decisión que S2 no toma y S4 sí:** si el import de Traktor además escribe `tracks.bpm`. El schema funciona
igual en los dos casos, así que no bloquea este slice.

**S2 no toca el writer ANLZ.** Alimentar `BeatGridTag` con un ancla real es una mejora aparte, con su propio
riesgo: cambia lo que se exporta a un pendrive. Queda registrado como oportunidad, no se hace acá.

Migración append-only con `effects` exacto (`Effect::Column` por cada columna).

### S3 — Parser NML read-only + dry-run (implementado)

Aterrizó en `services/traktor/` con `quick-xml`: `model.rs`, `parser.rs`, `path.rs`, `key.rs`, `mapper.rs`,
`tests/mod.rs` y `fixtures/collection_small.nml` (anonimizado). Módulo **sin** gating desktop, sin DB, sin
comandos y sin `async`. 36 tests + 1 `#[ignore]`d contra el corpus real.

### S4 — Import real

Match por `file_path` normalizado y `file_hash`; merge (nunca insert ciego: el bug de duplicados de
Harmony); playlists con jerarquía real (`parent_id`), no aplanadas; cues con dedup; reporte de
no-matcheados.

## Tasks

- [x] T1 Servicio `services/cue.rs` (CRUD + validación + dedup + `_hlc` + tests unitarios) — `a2cd6a0`
- [x] T2 Comandos `commands/cue.rs` + registro en `generate_handler!` — `a2cd6a0`
- [x] T3 TS: tipo `Cue` snake_case + `shared/api/cues.ts` + barrel — `1cda13e`
- [x] T4 Store `shared/stores/cues.ts` + barrel — `1cda13e`
- [x] T5 Gates completos (cargo test/clippy/fmt, svelte, mobile, vitest, format, lint)
- [x] S3 Parser NML read-only + dry-run — `df66775`
- [ ] S2 Modelo de beat grid (va **antes** de S4)
- [ ] S4 Import real con merge por path/hash + playlists con jerarquía
- [ ] Follow-up: `<NODE/>` self-closing en el parser
- [ ] Follow-up: variante `TraktorError::Invalid` sin construir

## Constraints

- Rust pinneado a `nightly-2026-02-19`; clippy con `-D warnings`.
- `cargo test --features desktop` es obligatorio (sin `desktop` el crate no compila por el
  `compile_error!`).
- **Nunca** escribir `collection.nml` en esta feature.
- `cues.type` tiene CHECK: solo `'memory'`, `'hot'`, `'loop'`.
- **`hot_cue_index` es base 1 (`1..=8`) en Crate.** Los `HOTCUE` de Traktor son base 0: mapear con `+1` en el
  import. Nunca guardar el 0 crudo de Traktor.
- **Nunca almacenar el `INFO KEY` crudo de Traktor.** Es Open Key verdadero y está rotado; convertir a Camelot
  canónico. `MUSICAL_KEY VALUE` es la fuente preferida sobre `INFO KEY`.
- El grid se detecta por `TYPE="4"`, **nunca** por `NAME` (en el fixture hay un grid llamado `Beat Marker`).
- `DISPL_ORDER` es `0` en el 100% del fixture: no usarlo para ordenar cues.
- El fixture `data/collection.nml` **no está en `.gitignore`** y contiene rutas absolutas del usuario: no debe
  commitearse tal cual.
- La capa de cloud-sync ya conoce `cues` (`_hlc`): no romperla ni escribir `_hlc` a mano.
- **Las columnas nuevas de `tracks` van al final de la tabla y al final de cada `SELECT`.** El repo lee `Track`
  con índices posicionales (`row.get(N)`): agregar en el medio desplaza todo y es un bug silencioso.
- **La grilla es el trío `(grid_bpm, grid_anchor_ms, grid_source)`.** No pisar `tracks.bpm` sin decidirlo
  explícitamente en S4.
- `odd/` no entra en una rama de feature pura para upstream (AGENTS.md §1.2).
- Review workload: el diff debe quedar bien por debajo de ~400 líneas por PR.

## Evidence log

### Commits

| Commit | Contenido |
| --- | --- |
| `a2cd6a0` | `feat(cues): add the cue points service and IPC commands` |
| `1cda13e` | `feat(cues): expose cue points through the api and store layer` |
| `df66775` | `feat(traktor): parse Traktor NML collections read-only` |

El orden es deliberado: Rust primero, así cada commit deja el árbol consistente (el servicio y los comandos
compilan y pasan sus tests sin depender de la capa TS).

Rama `traktor-nml-import` (solo la feature). El doc ODD y el `.gitignore` van a `dev`, según AGENTS.md §1.2.

### Gates

| Gate | Resultado |
| --- | --- |
| `cargo test --features desktop` | 540 passed, 0 failed, 10 ignored |
| `cargo clippy --features desktop -- -D warnings` | limpio |
| `cargo fmt --check` | exit 0 |
| `yarn lint:check` | exit 0 |
| `yarn format:check` | exit 0 |
| `yarn vitest run` | 124 passed (9 archivos) |
| `yarn check:svelte` | 0 errores, 0 warnings |
| `yarn check:svelte:mobile` | 0 errores, 0 warnings |

Verificado por un verificador independiente, que confirmó: servicio `app.manage`ado (`lib.rs:598,656`), 4
comandos registrados una sola vez, sin gating desktop, HLC desde `dirty::next_hlc` dentro de la transacción,
tombstones solo para ids descartados, y tipos TS espejo campo por campo. Los tests de dedup y de tombstones
**fallan** si se quita la lógica: no son tautologías.

Desvío detectado y corregido por el orquestador: `shared/stores/cues.ts` manejaba el error con
`error instanceof Error`, pero `CrateError` serializa a **string plano**; se alineó al patrón del repo
(`typeof error === 'string' ? ...`). Gates de TS re-corridos después del arreglo: lint y format exit 0.

No verificado: compile Rust de mobile (no hay toolchain iOS en este host), el round-trip de cloud-sync de un
tombstone de cue, y los bordes de `hot_cue_index` contra Traktor (queda cubierto por el mapeo `+1` de S3).

### Gates de S3 (verificados por un verificador independiente)

| Gate | Resultado |
| --- | --- |
| `cargo test --features desktop` | 575 passed, 0 failed, 11 ignored (+35 sobre S1) |
| `cargo clippy --features desktop -- -D warnings` | limpio |
| `cargo fmt --check` | exit 0 |
| `cargo test --features desktop -- --ignored real_collection` | pasa: 264 tracks, 246 grids, 3 smartlists contra el archivo real |

Confirmado adversarialmente: la aritmética de Open Key es correcta en las 12 rotaciones y la tabla
`MUSICAL_KEY` en sus 24 entradas; el camino legacy `m`/`d` de `services/analysis.rs` quedó **intacto**
(`git diff dev` vacío); el grid se lee por `TYPE="4"` y nunca entra a la lista de cues; `HOTCUE n → n+1`;
cero fugas de la ruta real del usuario (`grep -rn josev` fuera de `data/` sin coincidencias); `Cargo.lock`
solo suma una línea en la lista de deps; `quick-xml` no arrastra nada desktop-only.

Dos hipótesis propias resultaron **refutadas** por la verificación, y se registran:

1. El orquestador propuso `9m → 11A` como caso de prueba: **estaba mal**. `9m` (F menor) es `4A`. La
   implementación y su test ya daban `4A`.
2. El orquestador sospechó que redondear a milisegundos enteros podía **colapsar cues distintos** vía el dedup
   de `(position_ms, type, hot_cue_index)` de S1. Medido sobre los 264 tracks por dos métodos independientes:
   **0 colisiones con valor exacto, 0 con valor redondeado, 0 tracks que ganen una colisión por redondeo**. El
   `INTEGER` no pierde datos en esta colección.

### Hallazgos abiertos (no bloqueantes)

| Hallazgo | Evidencia | Impacto |
| --- | --- | --- |
| Un `<NODE .../>` self-closing corrompería la pila del parser | `parser.rs:132-140,168-173` | 0 casos en el corpus real, pero otro exportador podría emitirlo |
| `TraktorError::Invalid` nunca se construye | `mod.rs:32` | Lo enmascara el `#![allow(dead_code)]` del módulo; el `allow` es deliberado porque S4 consume la superficie |
| El fixture tiene 7 tracks y no 10–20 | `fixtures/collection_small.nml` | Cubre los 13 casos pedidos; solo es menos volumen del sugerido |

### Hallazgos registrados

- `upstream/blackboxaudio/crate` tiene **cero issues** que mencionen Traktor: si esto se ofrece
  upstream, hace falta crear el issue primero (el repo pide issue-first en su plantilla de PR).
- La línea de integración real del fork es `dev` (149 commits adelante de `origin/develop`, que
  quedó obsoleto). `AGENTS.md` habla de `develop`; la rama viva es `dev`.
