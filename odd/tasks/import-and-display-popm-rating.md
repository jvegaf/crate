# Feature: import-and-display-popm-rating

Goal: leer el rating POPM de etiquetas ID3 al importar archivos de audio y mostrar el rating normalizado de Crate en una nueva columna del tracklist. El modelo y la base ya tienen `Track.rating` como entero entre 0 y 5; no hace falta una migración.

Branch/session: `dev`. El usuario autorizó el commit de implementación `1433315` (`feat(library): import and display POPM ratings`).

## Diagnosis

- `src-tauri/src/services/library/import.rs` extrae metadatos con Lofty, pero no lee POPM. `Track::new` inicializa `rating` en 0.
- `Track.rating` ya se persiste y se devuelve en las consultas; la base limita el valor a 0–5.
- Reimportar un track existente no actualiza el rating, preservando el valor de Crate.
- `TrackRow.svelte` y `TrackListHeader.svelte` tienen una grilla fija sin columna de rating.
- Lofty 0.22.4 expone `Frame::Popularimeter` (`rating: u8`, email del owner); el tag genérico preserva frames específicos por defecto y puede convertirse a `Id3v2Tag`.

## Decisiones

1. Mapear el rating POPM 8-bit a la escala Crate 0–5 por rangos habituales: `0 → 0` (desconocido/sin rating), `1–31 → 1`, `32–95 → 2`, `96–159 → 3`, `160–223 → 4`, `224–255 → 5`.
2. Si hay varias frames POPM, usar la primera con rating distinto de cero en el orden del tag; si todas son cero o no hay frame, dejar 0.
3. Importar el rating solamente al crear el track. En reimportaciones conservar siempre el rating previamente almacenado en Crate.
4. Mostrar cinco estrellas en una columna nueva: las primeras N activas para rating N y las cinco inactivas para 0; localizar el heading y no agregar edición de rating.

Out of scope:
- Cambiar el modelo, la base de datos o la API IPC.
- Sincronizar cambios hechos en Crate de vuelta al archivo de audio.
- Cambiar el rating existente al reimportar tracks.

## Tasks

### T1 — Importar POPM y probar la conversión — Done

En `src-tauri/src/services/library/import.rs`, agregar extracción tipada de `Frame::Popularimeter` desde Lofty y normalización POPM→0–5. Aplicar el valor al track nuevo sin alterar la política existente de conflictos/reimportación. Agregar tests unitarios para cero, límites de cada rango, 255, y la selección de frame según la política anterior.

Evidence: `cargo test --features desktop services::library::import::tests` passed (16 passed, 0 failed); `cargo check --features desktop --tests` passed. Worker observed RED before implementation and GREEN afterward.

### T2 — Mostrar rating en el tracklist — Done

Agregar la celda/columna de rating en `apps/desktop/src/lib/components/library/TrackRow.svelte` y su encabezado correspondiente en `TrackListHeader.svelte`, manteniendo alineada la grilla. Mostrar cinco estrellas: las primeras N activas para un rating N; con 0, las cinco quedan inactivas. Añadir la traducción del heading a los 15 locales de `shared/i18n/locales/`.

Evidence: Prettier check pasó para ambos componentes y los 15 JSON; un JSON parse/key-presence check encontró la clave `library.columns.rating` en todos. Tras la prueba manual del usuario, se sustituyó el texto `N/5` por cinco estrellas visuales, activando las primeras N con `text-warning`; `yarn prettier --check apps/desktop/src/lib/components/library/TrackRow.svelte` y `yarn lint:check` pasaron. `yarn check:svelte` reportó un error preexistente (`PUBLIC_APP_VERSION` en `+layout.svelte`) y dos warnings de accesibilidad en `LibraryTab.svelte`; no reportó problemas en los archivos tocados. `git diff --check` pasó.

### T3 — Verificar y cerrar — Done

Revisar el diff y ejecutar las comprobaciones proporcionales de Rust y frontend. Informar todo gate fallido, omitido o bloqueado. No hacer commit sin pedido explícito.

Evidence:
- Passed: `yarn lint:check`, `yarn check:svelte:mobile`, `cargo test --features desktop` (225 passed, 0 failed; 1 ignored doctest), `git diff --check`, and JSON parse/key-presence checks for all 15 locales. Focused Rust tests passed 16/16; `cargo check --features desktop --tests` passed.
- Passed for touched files: Prettier check on both Svelte files and all locale JSON; no diagnostics on changed Svelte files. The post-feedback star display also passed targeted Prettier and ESLint.
- Failed outside the feature diff: `yarn format:check` flags existing `shared/utils/format.ts`; `yarn check:svelte` reports missing build-time `PUBLIC_APP_VERSION` in unchanged `+layout.svelte` and two a11y warnings in unchanged `LibraryTab.svelte`; `cargo clippy --features desktop -- -D warnings` fails on unchanged `services/file_tags.rs:101` (`too_many_arguments`).
- Environment-sensitive/undetermined: `cargo fmt --check` flags unchanged `build.rs` and `main.rs` under a global 2-space rustfmt config; without that config, the verifier reports 1394 files differ, including `import.rs`. Changed Rust follows the local `dev` branch's prevailing 2-space style.
- `cargo check --release --features desktop`, mobile Rust target check, and JS tests were not run. The verifier reports no JS test script in package manifests; mobile Rust is not affected by this desktop-gated importer change.
- Review size: focused 18-file feature diff (Rust importer/tests, two tracklist components, 15 locale keys); no unrelated source hunks. `git diff --check` passed. Implementation commit: `1433315` (`feat(library): import and display POPM ratings`).

### T4 — Resolver gates preexistentes ajenos al feature — Pending

Los gates de formato global, Svelte desktop y Clippy tienen fallas fuera de los archivos de esta feature; `cargo fmt --check` además depende de configuración global inconsistente con CI. No corregir código ajeno al alcance sin autorización. Queda como follow-up independiente.

La inspección RDD de revisión nativa quedó bloqueada: el paquete local verificado no contiene el binario requerido (`package-local-binary-missing`). No se ejecutó el comando de instalación/reparación porque el usuario autorizó el commit, no mantenimiento del paquete.

## Evidence

- Exploration: `gentle-ai-explore` confirmó que POPM no se lee ni se muestra, y localizó Lofty 0.22.4 `Frame::Popularimeter` con raw `u8`.
- User decisions: rangos habituales POPM; preservar rating de Crate en reimportaciones; mostrar cinco estrellas con las primeras N activas.
- Commit de implementación: `1433315`.
