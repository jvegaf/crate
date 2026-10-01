# Tracklist: add Genre column + rename Origin → Source

- **Status:** VERIFIED, COMMITTED (code `8acb1c3`; odd tooling rides its own commit per repo convention)
- **Base:** `dev` @ `81233cc` (working tree clean before start)
- **Requested by user:** 2026-10-01 — "nos falta una columna en la tabla tracklist... Genre" + "cambia el nombre origin por source de la columna"
- **Route:** delegated direct (one writer). Trigger: 2+ non-trivial files (registry, types, sorting, TrackRow, 15 locales, tests, docs).
- **TDD mode:** OFF (precedent from tracklist-column-config units; ordinary functional checks + existing vitest suites updated alongside).
- **Commit policy:** repo AGENTS.md §1.3 — do NOT commit unless the user explicitly asks. Work stays in the dev worktree until then.

## Scope decisions (evidence-based)

1. **No backend changes.** `genre` already exists: `src-tauri/src/db/schema.rs:16` (`genre TEXT`), Rust `Track.genre: Option<String>` (`models/track.rs:17`, serialized snake_case), writer `services/library/import.rs:80` (`tag.genre()`), TS `Track.genre: string | null` (`shared/types/index.ts:104`). Only the column UI is missing.
2. **New column `genre`**, hidden by default (established convention: new columns hidden; `album` precedent), width `1fr` (fluid text column), inserted after `album` in canonical order. Sortable (text column, matches `artist`/`label` precedent).
3. **Origin → Source is a label rename only.** Internal id stays `origin` (persisted `tracklist_columns` JSON stores ids; renaming would silently reset users' layouts). Change the VALUE of `library.columns.origin` ("Origin" → "Source", translated per locale). `getTrackOriginFolder` and id strings stay untouched.
4. **i18n:** add `library.columns.genre` to all 15 locales; update `library.columns.origin` value in all 15. Keys must land before UI that renders them (`$translate` shows the raw key when missing).
5. **Normalization is migration-free:** `normalizeTracklistColumns` inserts unknown-to-prefs ids at canonical position, so already-saved layouts pick up `genre` automatically (hidden). Verify with a test.

## Tasks

- [x] T1 — Types + registry: add `'genre'` to `TracklistColumnId` and `TrackSortField` (`shared/types/index.ts`); add genre definition after `album` in `TRACKLIST_COLUMN_DEFINITIONS` (`shared/utils/tracklistColumns.ts:12`) with `labelKey: 'library.columns.genre'`, `width: '1fr'`, `sortable: true`, `defaultVisible: false`.
- [x] T2 — Sorting: wire `genre` in `shared/utils/sorting.ts` (getSortValue/comparison path for text fields, `null` handling like other optional strings).
- [x] T3 — Rendering: genre cell in `TrackRow.svelte` (follow label/origin text-cell precedent incl. empty value); confirm `TrackListHeader.svelte` derives label + sort from the definition generically (fix if id→field mapping is hand-written).
- [x] T4 — i18n ×15: add `library.columns.genre`; change `library.columns.origin` value to "Source"/localized equivalent in `shared/i18n/locales/*.json`.
- [x] T5 — Tests: update `tracklistColumns.test.ts` (full default list now includes genre; byte-for-byte visible default template must stay unchanged; add normalize-inserts-genre case); add genre sort coverage in sorting tests if the suite exists.
- [x] T6 — Docs: update `docs/` tracklist-columns page (add Genre; rename Origin label mentions to Source) where the column name is user-facing.
- [x] T7 — Verification: vitest, `check:svelte`, `check:svelte:mobile`, lint (exit code), prettier check. Record outputs.
- [x] T8 — Work-unit commit: user-authorized 2026-10-01 ("si").

## Verification gate list (from crate-verification)

| Command | Proves |
| --- | --- |
| `yarn vitest run` | registry/normalization/sorting unit tests |
| `yarn check:svelte` | desktop type check (only gate that catches TS narrowing in shared code) |
| `yarn check:svelte:mobile` | shared/ stays mobile-safe |
| `yarn lint:check` (judge by EXIT CODE, output has --debug noise) | ESLint CI gate |
| `yarn format:check` (exit code) | Prettier CI gate |

**Known baseline noise (not defects):** `check:svelte` carries 1 pre-existing `PUBLIC_APP_VERSION` error (+2 LibraryTab warnings).

## Evidence log

_(writer fills as tasks complete)_

### 2026-10-01 — writer (delegated direct) implemented T1–T7

**Files changed (21):**

- `shared/types/index.ts` — added `'genre'` to `TrackSortField` (after `'album'`) and `TracklistColumnId` (after `'album'`, before `'label'`). Ids/unions keep registry order; existing member order untouched.
- `shared/utils/tracklistColumns.ts` — genre definition inserted after `album`: `{ id: 'genre', labelKey: 'library.columns.genre', width: '1fr', sortable: true, defaultVisible: false }`.
- `shared/utils/sorting.ts` — `case 'genre': return track.genre?.toLowerCase() ?? null` between the `album` and `label` cases (nulls → last via the existing shared null handling in `sortTracks`).
- `apps/desktop/src/lib/components/library/TrackRow.svelte` — genre cell after the album branch, mirroring the album/label text-cell precedent (`{track.genre ?? ''}`, `truncate text-text-secondary`).
- `shared/i18n/locales/*.json` ×15 — added `library.columns.genre`; changed `library.columns.origin` value where wording actually changed (see table). No reformatting; album→label adjacency used as the unique insertion anchor per file.
- `shared/utils/tracklistColumns.test.ts` — `columnIds` now 16 entries; `toHaveLength(15)` → `16`; new normalize test proves genre inserts at index(album)+1 with `visible: false` into a legacy 8-column saved layout; new sort test proves genres sort case-insensitively with nulls last; byte-for-byte default visible grid-template assertion left identical (genre hidden by default).
- `docs/src/content/docs/user-guide/library-management.md` — columns table: `Origin` row renamed to `Source`, new `Genre | Music genre | No` row after Album; fluid-text-column sentence now lists genre + source; "The Origin column is the name of the folder…" → "The Source column…". No prettier run on docs.

**Per-locale choices** — genre values reuse the app's own established translations (already present as `smartPlaylist.fields.genre` in all 15 locales, so zero new genre translations were invented): de Genre, en Genre, es Género, fr Genre, it Genere, ja ジャンル, ko 장르, nl Genre, pl Gatunek, pt Gênero, ro Gen, sv Genre, tr Tür, uk Жанр, zh 流派. Origin→Source: en Source, de Quelle, es Fuente, fr Source, it Fonte, ja ソース, ko 소스, nl Bron, pt Fonte, ro Sursă, sv Källa. **pl (Źródło), tr (Kaynak), uk (Джерело), zh (来源) already read as "Source" — value left unchanged, only the id-internal wording was already correct.**

**Hand-enumeration audit:** `TrackListHeader.svelte` and `TrackListColumnsMenu.svelte` derive everything from `TRACKLIST_COLUMN_DEFINITIONS` (header casts `column.id as TrackSortField` behind the `sortable` gate — genre qualifies automatically; sortable ids ⊂ `TrackSortField` still holds). `getTrackSortValue`'s switch has a `default:` arm, so no compile break, but genre was wired explicitly. Only `TrackRow.svelte`'s `{#if column.id === …}` chain needed the new branch. `apps/desktop/src/lib/stores/library.ts` around line 390 (`sortedTracks` derived) just forwards `SortConfig` to `sortTracks` — no exhaustive construct.

**Verification runs (all after final edits):**

- `yarn vitest run` → 2 test files passed, 46 tests passed (incl. the 2 new genre cases).
- `yarn check:svelte` → "svelte-check found 1 error and 2 warnings in 2 files" — exactly the documented baseline (1 × `PUBLIC_APP_VERSION` + 2 × LibraryTab a11y warnings). No new findings.
- `yarn check:svelte:mobile` → 0 errors, 0 warnings.
- `yarn lint:check` → exit code 0 (output ignored per policy: `--debug` noise).
- `yarn format:check` → exit code 0.

**Not touched:** `src-tauri/`, `openspec/`, git state (no commit/branch/push — T8 pending explicit user instruction). Locale JSON re-parse validated for all 15 files.

### 2026-10-01 — T8 commit (user-authorized)

- `feat(tracklist): add Genre column and rename Origin header to Source` — the 21 code/docs files only; odd/ tooling kept out of the feature commit per repo convention (AGENTS.md §1.2, tracklist-column-config precedent).
