# Feature: playlist-auto-order-key-colors

Goal: agregar auto-ordenado de tracks dentro de una playlist al estilo de
[harmony `utils-library.ts`](https://github.com/jvegaf/harmony/blob/master/src/renderer/src/lib/utils-library.ts)
y, en el mismo trabajo, colorear las tonalidades en el tracklist.

Branch: `playlist-auto-order-key-colors` (desde `dev`, línea de integración del fork).

## Alcance

1. **Auto-orden persistido** de los tracks de una playlist, con dos modos de prioridad:
   - `harmony` — key primero, rating como desempate
   - `energy` — rating primero, key como desempate
2. **Modo de orden "Playlist Order"** para que la vista respete el orden persistido.
3. **Colores de key** en la columna Key del tracklist.

## Non-goals

- Notación **OpenKey**. Crate solo soporta `standard` y `camelot` (`KeyNotationFormat`).
  El helper normaliza a Camelot internamente; OpenKey no se expone.
- Playlists de contexto discovery (`playlist_discovery_tracks`, Migration 18). Solo playlists de biblioteca.
- Drag & drop de reordenamiento manual. Hoy no existe y no se agrega acá.
- Smart playlists: se auto-computan por fetch y no tienen `position`. La acción se deshabilita.
- Cambios en Rust. `reorder_playlist` ya existe y alcanza.

## Diagnóstico (reconocimiento read-only)

### Lo que ya existe y no hay que escribir

| Pieza | Ubicación | Estado |
| --- | --- | --- |
| `PlaylistService::reorder_tracks(playlist_id, track_ids)` | `src-tauri/src/services/playlist/tracks.rs:236` | Existe. Recibe la lista completa y ordenada de ids, escribe `position = índice`, marca dirty (`PLAYLIST_TRACKS`, `PLAYLISTS`). |
| Comando `reorder_playlist` | `src-tauri/src/commands/playlist.rs:137` | Existe y está registrado. Sin guard de smart playlist. |
| Wrapper TS `reorderPlaylist(playlistId, trackIds)` | `shared/api/playlists.ts:91` | Existe. **Cero callers.** |
| Store `reorderTracks` | `shared/stores/playlists.ts:287` | Existe. **Cero callers.** No recarga tracks. |
| Lectura ordenada | `src-tauri/src/services/playlist/tracks.rs:22` | `ORDER BY pt.position`. El DTO ya trae `key: Option<String>` y `rating: i32`. |
| Mapa Standard ↔ Camelot + `formatKey` | `shared/utils/format.ts:41-105` | Existe, con enharmónicos. `STANDARD_TO_CAMELOT` es privado del módulo. |

### El problema real: el orden persistido está enmascarado

`apps/desktop/src/lib/stores/library.ts:462` re-ordena **siempre** los tracks de la playlist con el
sort global de la biblioteca:

```ts
// displayedTracks
} else if ($library.selectedPlaylistId) {
    tracks = $library.playlistTracks
}
// ...
return sortTracks(tracks, $library.sort)   // ← pisa el orden persistido
```

Consecuencia: persistir `position` no alcanza; sin un modo de orden explícito el usuario **no ve** el
resultado. Desktop no tiene `TrackSortField` de orden de playlist; **mobile sí lo resolvió**
(`apps/mobile/src/lib/components/playlists/PlaylistDetailView.svelte:72`, con
`{ field: 'playlist_order', directionless: true }` y `viewSort = null` cuando el field es
`playlist_order`). Esa es la convención a espejar, no una nueva.

### Las keys son texto libre con notación mixta en la misma DB

| Origen | Valor almacenado |
| --- | --- |
| stratum-dsp / análisis | `"Am"`, `"C"`, `"F#m"` (standard) |
| Default del setting y conversión | `"8A"`, `"8B"` (camelot) |
| Tagger Beatport | `"G Minor"` — **no está en el mapa**, `formatKey` lo devuelve crudo |

`tracks.key` es `TEXT` nullable (`src-tauri/src/db/schema.rs:57`). `formatKey` hace lookup exacto
(`STANDARD_TO_CAMELOT[key] ?? key`), así que un `"G Minor"` no se normaliza hoy. El orden por key
tiene que tolerarlo en vez de inventar una posición.

### Rating y formato

- `tracks.rating INTEGER DEFAULT 0` con `CHECK (rating >= 0 AND rating <= 5)` (`schema.rs:71,83`).
  En `Track` es `i32`, **no** `Option` (`src-tauri/src/models/track.rs:34`).
- La convención de `sortTracks` trata **rating 0 como vacío** y agrupa vacíos de forma
  direction-aware (`shared/utils/sorting.ts:24-38`). El comparador de auto-order debe ser coherente
  con esa convención en vez de inventar otra.

## Decisiones de producto (aprobadas por el usuario)

1. **Rama base**: `dev` (línea de integración del fork). El trabajo usa la columna Key y el
   multi-sort del fork; no es PR-ready a upstream sin cherry-pick posterior.
2. **El auto-orden persiste en DB** vía `reorder_playlist` y además fija la vista en modo
   `playlist_order`, para que el orden sobreviva recargas y se propague por cloud-sync.
3. **Dos modos**: `harmony` y `energy`.
4. **Paleta de colores**: pendiente de aprobación final de hexes (ver T6).

## Design

### T1 — Normalización de key a Camelot (dominio)

En `shared/utils/format.ts`:

- Exportar `keyToCamelot(key: string | null): string | null` — lookup exacto en el mapa existente
  más una normalización tolerante previa para las formas del tagger (`"G Minor"`, `"G min"`,
  `"Gm"` → `"Gm"`), siguiendo el criterio de `formatOpenKey` de harmony
  (quitar espacios, `minor`/`min` → `m`, `major`/`maj` → vacío, capitalizar tónica).
- Sin mapa nuevo: reutiliza `STANDARD_TO_CAMELOT`. El valor debe venir de datos, no inventado.

Criterio: si no se reconoce, devolver `null` (desconocido), **nunca** una posición inventada.

### T2 — Comparador de auto-orden (dominio) + tests

Nuevo `shared/utils/autoOrder.ts`:

- `keySortValue(key): number` — `número * 10 + (A ? 0 : 1)` para Camelot (mismo criterio que
  `getOpenKeySortValue` de harmony). Desconocido ⇒ `999` (al final).
- `autoOrderTracks(tracks: Track[], priority: 'harmony' | 'energy'): Track[]` — copia, sort estable,
  desempate final por `id` para determinismo (convención ya usada en `sorting.ts:112-152`).

Tests (vitest, **test-first**): `shared/utils/autoOrder.test.ts`.

### T3 — TrackSortField `playlist_order`

- `shared/types/index.ts`: agregar `'playlist_order'` a `TrackSortField`.
- `shared/utils/sorting.ts`: caso `playlist_order` que **no re-ordena** (devuelve copia en el orden
  recibido, que ya viene `ORDER BY pt.position` del backend).
- Debe ser directionless, como en mobile.

### T4, T5, T6, T7 — diseño vigente

Las secciones de diseño de T4/T5/T6 escritas antes de explorar quedaron superadas por las versiones
acordadas: ver **T5 — diseño acordado** y **T6 — paleta aprobada**. T4 se disolvió en T5 y T7 (i18n)
quedó cubierto dentro de T5.

## Tasks

- [x] T1 Normalización `keyToCamelot()` en `shared/utils/format.ts` — commit `c762856`
- [x] T2 `shared/utils/autoOrder.ts` + tests vitest (RED → GREEN) — commit `d6b7ee8`
- [x] T3 `TrackSortField 'playlist_order'` + caso no-op en `sortTracks` — commit `64ee513`
- [x] ~~T4 Wiring del modo de orden en la vista de playlist~~ — **absorbida por T5**: el único camino
      para entrar en modo `playlist_order` es la acción de auto-orden, así que una tarea separada no
      tenía contenido propio.
- [x] T5 Acción de auto-orden en el menú del espacio vacío de la playlist (harmony / energy) + i18n — commit `72f932c`
- [x] T6 Helper de color de key + render en `TrackRow` (paleta aprobada: MIK reindexada) — commit `5f72377`
- [x] T7 i18n (`en.json`, `es.json`) — cubierto dentro de T5
- [x] T8 Gates de verificación — los 5 en verde, ver "Gates finales"
- [ ] T9 Acción de auto-orden en el menú de la playlist del sidebar

## T5 — diseño acordado

**Un solo punto de entrada**: el menú contextual del espacio vacío de la playlist
(`ContextMenuOrchestrator.svelte`, rama `visibleMenu?.type === 'playlistView'`, hoy con un solo ítem
"Import"). El menú del sidebar queda como follow-up para no inflar el diff.

Dos ítems, `AutoOrderHarmony` y `AutoOrderEnergy`, **deshabilitados** cuando
`playlist.is_smart || playlist.context === 'discovery'` (las smart se computan por reglas y no tienen
`position`; las de discovery viven en `playlist_discovery_tracks`).

Flujo del handler en `playlistController.ts`:

1. Guard de smart/discovery; si `playlistTracks.length < 2`, salir sin toast.
2. `autoOrderTracks(tracks, priority)` (de `shared/utils/autoOrder.ts`).
3. `playlistsStore.reorderTracks(playlist.id, ordered.map((t) => t.id))` — **la lista completa**, porque
   `reorder_tracks` no valida completitud.
4. `libraryStore.loadPlaylistTracks(playlist.id)` para recargar.
5. `libraryStore.setSort({ field: 'playlist_order', direction: 'asc' })` para que la vista muestre lo
   que se acaba de escribir.
6. Toast con clave nueva del grupo `toast` e interpolación `{ values: { name } }`.

**Coherencia del header**: `displayedTracks` ordena desde `$library.sort`, así que el paso 5 alcanza
para el orden real. Pero `+page.svelte:74` mantiene un `sortConfig` local duplicado que alimenta el
chevron del header: sin sincronizarlo, una columna seguiría mostrando el chevron de un sort que ya no
se aplica. Se reemplaza ese estado local por un `$derived` del store, de modo que `handleSortChange`
solo escriba en el store. Ninguna columna tiene id `playlist_order`, así que el modo correcto es "sin
columna resaltada", que es justo la señal visual buscada.

### Convenciones verificadas para T5

- i18n: interpolación real `$translate('key', { values: { ... } })` (96 usos en el repo). Grupo `toast`
  ya existente (`trackAdded`, `tracksImported`, …). `playlists.playlistOrder` ya existe.
- Controller: `deps` ya incluye `libraryStore`, `playlistsStore` y `toastStore`
  (`playlistController.ts:15-27`).
- `Playlist` es snake_case en el wire (`is_smart`, `context`) — no camel-casear.

## Constraints

- **Sin cambios en Rust.** `reorder_playlist` alcanza; no agregar comando ni migración.
- **Mobile-safe**: todo lo que se toca en `shared/` compila para mobile
  (`yarn check:svelte:mobile` es gate real). Nada de APIs desktop-only en `shared/`.
- **Serde**: `Track` es snake_case en el wire (`key`, `rating`); no camel-casear.
- **No inventar posiciones de key**: key desconocida va al final con `999`, nunca a una posición
  Camelot arbitraria.
- **Reusar convenciones existentes** de `sorting.ts` (vacío/cero, direction-aware, desempate estable)
  en vez de definir otras.
- **Review workload**: el diff debe quedar bien por debajo de ~400 líneas; si el color de key infla el
  cambio, se separa en un work-unit aparte.
- **Commits**: ver la política acordada en la sesión; `AGENTS.md` §1.3 prohíbe commitear sin pedido
  explícito.

## T6 — paleta aprobada (Mixed In Key reindexada por número Camelot)

Fuente: `kMIKKeyColorPalette` de Mixxx `src/util/color/predefinedcolorpalettes.cpp`,
cuyo comentario ancla `C Major is 8B in Camelot Notation`. La paleta original está indexada
cromáticamente; se reindexa por número Camelot para que mismo número (A y B, relativas) comparta
familia de color, que es lo que el círculo comunica. `A` (menor) es el hex base al 82% (18% más oscuro)
que `B` (mayor).

| Camelot | B (mayor) | A (menor) |
| --- | --- | --- |
| 1 | `#FD7EB3` | `#CF6793` |
| 2 | `#20EF7F` | `#1AC468` |
| 3 | `#D18BFD` | `#AB72CF` |
| 4 | `#E0CA6D` | `#B8A659` |
| 5 | `#4DD3F8` | `#3FADCB` |
| 6 | `#FF8693` | `#D16E79` |
| 7 | `#00EECB` | `#00C3A6` |
| 8 | `#F17EDB` | `#C667B4` |
| 9 | `#7FF448` | `#68C83B` |
| 10 | `#9EB4FD` | `#8294CF` |
| 11 | `#FDA078` | `#CF8362` |
| 12 | `#01EAEC` | `#01C0C2` |

Render: chip con fondo tenue siguiendo el patrón de color dinámico de
`apps/desktop/src/lib/components/tags/TagChip.svelte:60-71` (hex + sufijo alfa inline). Key
irresoluble mantiene el texto plano actual y nunca recibe un color inventado.

## T9 — acción de auto-orden en el menú del sidebar

Segundo punto de entrada: `PlaylistContextMenu.svelte`, que se abre desde el árbol de playlists y
**también aparece para carpetas** (`playlist.is_folder`) y para multi-selección (`isBulk`).

### Corrección de un defecto latente que este menú expone

`handlePlaylistAutoOrder` leía `get(libraryStore).playlistTracks` **sin verificar que correspondan a la
playlist objetivo**. Desde el menú del espacio vacío eso era inocuo (siempre es la playlist en
pantalla), pero desde el sidebar se puede clickear una playlist que **no es la seleccionada**: se
habrían computado los tracks de una playlist y se habrían escrito las `position` de **otra**. El
verificador independiente ya había marcado este riesgo como latente.

Corrección: leer los miembros del objetivo desde el backend (`getPlaylistTracks`) en vez de confiar en
el store. Precedente de controllers importando `$shared/api/*`: `deviceController.ts:8`,
`exportController.ts:8`.

Además, la recarga de la vista y el cambio a `playlist_order` ahora son **condicionales**: solo si el
objetivo es la playlist en pantalla. Si no, no corresponde navegar al usuario ni tocar el sort.

La guarda usa `getSelectedPlaylistId()` — el `$state` de `+page.svelte:79`, que es lo que decide qué
vista se renderiza en `+page.svelte:603` — y **no** `libraryStore.selectedPlaylistId`. Los dos valores
divergen cuando hay un filtro de tags activo (`loadTracks({ playlist_id, tag_ids })` no escribe
`libraryStore.selectedPlaylistId`), así que el store puede quedar apuntando a otra playlist y hacer que
la guarda dispare sobre una vista que no es la del objetivo. El verificador independiente detectó esa
divergencia y ésta es la corrección mínima que propuso.

### Dos hallazgos del verificador sobre el caso con filtro de tags

1. **La recarga del handler viejo era invisible con filtro activo.** El worker afirmó que el handler
   viejo "refrescaba la vista completa y limpiaba el filtro"; el verificador lo **refutó**:
   `loadPlaylistTracks` nunca limpia `filter`, y `displayedTracks` renderiza `$library.tracks` (no
   `playlistTracks`) cuando `filter.tag_ids` no está vacío. Por eso el comportamiento nuevo en ese caso
   (persistir y dejar la vista filtrada en paz) **no es una regresión**: el orden queda guardado y se ve
   al limpiar el filtro.
2. **Limitación conocida, no corregible desde el frontend.** La verificación consiste en un re-fetch del
   objetivo; si ese re-fetch falla de forma transitoria *después* de una escritura exitosa, el usuario ve
   el toast de error aunque el orden sí se persistió. Reintentar es inocuo (re-ordenar un orden ya
   correcto es idempotente). La solución real sería que `reorder_tracks` devolviera el error en vez de
   que el store se lo trague, y eso es un cambio en Rust, fuera de alcance.

### Regla de disponibilidad (una sola, compartida con el menú del espacio vacío)

```text
!playlist.is_folder && !playlist.is_smart && playlist.context !== 'discovery'
```

- Sidebar: se **omite** el ítem — es la convención del archivo (no usa `disabled:` en ningún lado y ya
  omite condicionalmente `export` y `move`).
- Menú del espacio vacío: se **deshabilita** (ya implementado en T5). Contextos distintos justifican
  la diferencia: ahí la playlist es siempre la que se está mirando.
- Multi-selección (`isBulk`): sin ítem. Cada playlist requeriría su propio fetch de miembros; la rama
  bulk ya omite rename y export.

### Verificación

La verificación del resultado pasa a apoyarse en un **re-fetch** del objetivo, no en `libraryStore`,
justamente porque el objetivo puede no estar cargado en la vista. Sigue comparando la lista completa
de ids contra el orden pedido, que es lo único que delata que `reorderTracks` se tragó un error.

## Evidence log

### Test-first T1/T2 — RED observado

```text
FAIL shared/utils/format.test.ts > keyToCamelot > ...
TypeError: keyToCamelot is not a function
 Test Files  2 failed (2)
      Tests  4 failed | 19 passed (23)
```

Tras implementar: `Test Files 2 passed (2) / Tests 37 passed (37)`.

### T3 — naturaleza honesta del RED

El RED de T3 es **de tipos**, no de comportamiento: en runtime un field desconocido cae en el
`default` de `getTrackSortValue` → `null` → todos los valores "vacíos" → comparación `0` → el sort
estable ya preservaba el orden. Los tests de `playlist_order` que agregué **habrían pasado sin
implementar nada**; valen como guarda de regresión (si alguien implementa `playlist_order` como un
sort real, el test que exige preservar el orden dado falla), no como driver de comportamiento nuevo.
El error de tipos sí fue real y quedó verde: `svelte-check found 0 errors and 0 warnings`.

### T5 — verificación independiente (`gentle-ai-verify`)

Los 8 chequeos adversariales pasaron: que la acción efectivamente dispare en runtime (`pageActions` se
puebla en `+page.svelte:126-133`, antes de cualquier evento de usuario), que `disabled` se renderice
como no clickeable (`ContextMenu.svelte:106-107,171-177`), que los iconos existan
(`Icon.svelte:71,77`), que `visibleMenu.playlist` sea no-opcional en esa rama, que no quede ninguna
asignación a `sortConfig`, que la detección de fallo sea sólida (`playlist_tracks` tiene PK
`(playlist_id, track_id)` en `schema.rs:131-137`, así que no hay duplicados que rompan la
comparación), que la i18n interpole, y que se pase la lista completa de miembros.

**1 defecto real encontrado y corregido**: yo mismo había envuelto a mano `const applied = …` en
`playlistController.ts:319-320`, y `prettier --check` fallaba — eso habría roto `yarn format:check` en
CI. Corregido con `prettier --write` y re-verificado.

Desvío del worker respecto del brief: `ContextMenuOrchestrator` no importa `playlistController`;
obtiene el controller vía el store `pageActions` (`pageActions.ts:18`, ya usado por `+layout.svelte`).
Legítimo: es el patrón establecido para que componentes profundos alcancen el controller.

### T6 — test-first y bloqueantes stale

RED: `Error: Failed to resolve import "./keyColor"`. GREEN tras implementar: `8 passed (8)`, incluida
una aserción de identidad `expect(utils.keyColor).toBe(keyColor)` sobre el barrel, que cubre el trap
de AGENTS.md §7 y además fue la prueba decisiva de que dos bloqueantes de pi-lens eran cache stale:
el lens reportó "no exported member 'keyColor'" y luego "Cannot find module './keyColor'" para imports
que resolvían en vitest y en `svelte-check`. Tras `touch` de los archivos, el lens reportó clean.

La paleta aprobada está fijada por un test data-driven sobre los 24 hexes exactos, así que un cambio
accidental de color falla fuerte.

### Gates finales (T8)

| Gate | Resultado |
| --- | --- |
| `yarn check:svelte` | 0 errores, 0 warnings |
| `yarn check:svelte:mobile` | 0 errores, 0 warnings |
| `yarn vitest run` | 105 passed, 4 failed (las 4 preexistentes de `tracklistColumns`) |
| `yarn prettier --check` | limpio en los 13 archivos tocados |
| `yarn eslint` | limpio, exit 0 |

### ASSESS y RDD

`gentle-ai review mode status` → receipt-driven development **off** (clone-local), así que el ciclo de
review nativo no corre y la entrega queda bajo política ordinaria del repo. El ASSESS sobre el diff del
writer devolvió `risk: unassessable` porque el doc ODD sin trackear requiere declaración explícita y no
pudo acotar el candidato; por contrato eso se verifica igual que riesgo alto (verificación propia del
writer + verificador independiente siempre), y ambas se ejecutaron.

### Commits

| Commit | Contenido |
| --- | --- |
| `c762856` | `feat(key): normalize mixed-notation keys to Camelot` |
| `d6b7ee8` | `feat(playlist): add key-aware auto ordering comparator` |
| `64ee513` | `feat(playlist): add a playlist_order sort field that preserves stored order` |
| `72f932c` | `feat(playlist): add auto-order action for playlist tracks` |
| `5f72377` | `feat(library): colour the key column by Camelot number` |

El doc ODD queda **sin trackear** por AGENTS.md §1.2 (nada de scaffolding en una rama de feature).

### Fallas preexistentes ajenas a este trabajo

`shared/utils/tracklistColumns.test.ts` tiene 4 fallas (16 vs 17 columnas) que ya existen en `dev`:
el commit `a020ec6` agregó la columna Provider y el test quedó desactualizado. Ambos archivos están
sin modificar en esta rama (`git status` los confirma), así que la falla es previa y queda fuera de
alcance.
