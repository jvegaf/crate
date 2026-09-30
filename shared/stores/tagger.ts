import { writable, get } from 'svelte/store'
import type { ProviderError, ScoredTagCandidate, TagCandidate, Track, TrackMetadataPatch } from '../types'
import { extendTrackTag, searchRankedTrackTags, setTrackArtworkFromUrl } from '../api/tagger'
import { updateTrackMetadata } from '../api/library'
import { translate } from '../i18n'
import { toastStore } from './toast'

// =============================================================================
// State
// =============================================================================

/** One local track and the ranked candidates the batch search returned for it. */
export interface BatchRow {
	track: Track
	candidates: ScoredTagCandidate[]
	errors: ProviderError[]
	/** Per-track failure (the search itself threw), distinct from per-provider errors. */
	error: string | null
}

interface BatchProgress {
	processed: number
	total: number
	currentTitle: string
}

interface TaggerState {
	trackId: string | null
	loading: boolean
	extending: boolean
	candidates: ScoredTagCandidate[]
	errors: ProviderError[]
	error: string | null
	/**
	 * The candidate the user selected, or null. Held by value, not by provider
	 * id: two candidates can share a provider, so the id is not a unique key.
	 */
	selected: ScoredTagCandidate | null
	/** The extended form of the selected candidate once `extendSelected` ran. */
	extended: TagCandidate | null
	/** One row per local track in the batch flow. */
	batchRows: BatchRow[]
	/**
	 * Per-track decision. A missing key means "not decided yet"; an explicit
	 * `null` means the user chose "not available" (skipped).
	 */
	batchSelections: Map<string, ScoredTagCandidate | null>
	batchProgress: BatchProgress
	batchLoading: boolean
	batchApplying: boolean
}

const emptyProgress: BatchProgress = { processed: 0, total: 0, currentTitle: '' }

const initialState: TaggerState = {
	trackId: null,
	loading: false,
	extending: false,
	candidates: [],
	errors: [],
	error: null,
	selected: null,
	extended: null,
	batchRows: [],
	batchSelections: new Map(),
	batchProgress: emptyProgress,
	batchLoading: false,
	batchApplying: false,
}

// =============================================================================
// Helpers
// =============================================================================

/** A string with meaningful content to apply; `null` and `''` are not values. */
function hasValue(value: string | null): value is string {
	return value !== null && value !== ''
}

/**
 * Map a candidate to a metadata patch, omitting every field the candidate does
 * not provide. `TrackMetadataPatch` treats an omitted key as "unchanged" and an
 * explicit `null` as "clear", so omitting keeps an existing tag intact when the
 * provider has no replacement.
 */
function candidateToPatch(candidate: TagCandidate): TrackMetadataPatch {
	const patch: TrackMetadataPatch = {}

	if (hasValue(candidate.title)) {
		// The patch has no version field, so fold a non-empty mix/version into the
		// title to keep the version distinction through the round trip.
		patch.title = hasValue(candidate.version) ? `${candidate.title} (${candidate.version})` : candidate.title
	}

	if (candidate.artists.length > 0) {
		patch.artist = candidate.artists.join(', ')
	}

	const { album, genre, label, catalog_number: catalogNumber } = candidate
	if (hasValue(album)) patch.album = album
	if (hasValue(genre)) patch.genre = genre
	if (hasValue(label)) patch.label = label
	if (hasValue(catalogNumber)) patch.catalog_number = catalogNumber

	const releaseDate = candidate.release_date
	if (releaseDate !== null && /^\d{4}/.test(releaseDate)) {
		const year = Number(releaseDate.slice(0, 4))
		if (Number.isFinite(year)) patch.year = year
	}

	// The text fields above DO overwrite: when a local tag is wrong, the store
	// match is the authority and has to replace it.
	//
	// Deliberately NOT applied:
	// - `bpm` and `key`: Crate analyses these from the audio itself, and a value
	//   measured from the file beats whatever the store lists.
	// - `rating`: the user's own rating, not metadata.
	// - `embedded_artwork`: done by the dedicated artwork download, not here.
	// - comments: not a field of this patch at all, so they cannot be touched.
	return patch
}

// =============================================================================
// Store
// =============================================================================

function createTaggerStore() {
	const { subscribe, set, update } = writable<TaggerState>(initialState)

	return {
		subscribe,

		/**
		 * Rank metadata providers against a local track and expose the best
		 * candidates. Clears any previous selection and enrichment.
		 */
		async search(track: Track) {
			update((state) => ({ ...state, trackId: track.id, loading: true, error: null }))

			try {
				const result = await searchRankedTrackTags({
					artist: track.artist ?? null,
					title: track.title ?? '',
					durationMs: track.duration_ms,
				})
				update((state) => ({
					...state,
					candidates: result.candidates,
					errors: result.errors,
					selected: null,
					extended: null,
				}))
			} catch (error) {
				const errorMessage = error instanceof Error ? error.message : get(translate)('tagger.toast.searchFailed')
				update((state) => ({ ...state, error: errorMessage }))
				toastStore.error(errorMessage)
			} finally {
				update((state) => ({ ...state, loading: false }))
			}
		},

		/** Select a candidate; a new selection invalidates any enrichment. */
		select(candidate: ScoredTagCandidate) {
			update((state) => ({ ...state, selected: candidate, extended: null }))
		},

		/**
		 * Enrich the selected candidate with its per-ID detail. On failure the
		 * un-enriched candidate stays selected and usable.
		 */
		async extendSelected() {
			const state = get({ subscribe })
			const selected = state.selected
			if (selected === null) return

			update((current) => ({ ...current, extending: true, error: null }))

			try {
				const extended = await extendTrackTag(selected)
				update((current) => ({ ...current, extended }))
			} catch (error) {
				const errorMessage = error instanceof Error ? error.message : get(translate)('tagger.toast.extendFailed')
				update((current) => ({ ...current, error: errorMessage }))
				toastStore.error(errorMessage)
			} finally {
				update((current) => ({ ...current, extending: false }))
			}
		},

		/**
		 * Apply the extended candidate (or the selected one) to a track. Returns
		 * the updated track, or null when there is nothing to apply or it failed.
		 *
		 * Metadata is written first, then the candidate's remote artwork when it
		 * has one. If the artwork step fails after a successful metadata write,
		 * the metadata is still reflected in the returned track and the partial
		 * failure is surfaced rather than swallowed.
		 */
		async apply(track: Track): Promise<Track | null> {
			const state = get({ subscribe })
			const candidate = state.extended ?? state.selected
			if (candidate === null) return null

			let updated: Track
			try {
				updated = await updateTrackMetadata(track.id, candidateToPatch(candidate))
				update((current) => ({ ...current, error: null }))
				toastStore.success(get(translate)('tagger.toast.applied'))
			} catch (error) {
				const errorMessage = error instanceof Error ? error.message : get(translate)('tagger.toast.applyFailed')
				update((current) => ({ ...current, error: errorMessage }))
				toastStore.error(errorMessage)
				return null
			}

			const artworkUrl = candidate.artwork_url
			if (!hasValue(artworkUrl)) {
				return updated
			}

			try {
				return await setTrackArtworkFromUrl(track.id, artworkUrl)
			} catch (error) {
				// Metadata was applied; make the half-done outcome explicit and
				// still return the metadata-updated track so the UI reflects it.
				const errorMessage = error instanceof Error ? error.message : get(translate)('tagger.toast.artworkFailed')
				update((current) => ({ ...current, error: errorMessage }))
				toastStore.error(errorMessage)
				return updated
			}
		},

		/** Reset the store to its initial state. */
		reset() {
			set(initialState)
		},

		/**
		 * Search every provider for each of `tracks`, sequentially, and expose one
		 * row per track. A track whose search throws becomes a row with `error`
		 * set instead of aborting the whole batch. Once the loop is done, the best
		 * candidate is pre-selected for every row scoring >= 0.85; nothing else is
		 * pre-selected, so "not decided" stays distinct from an explicit skip.
		 */
		async searchBatch(tracks: Track[]) {
			update((state) => ({
				...state,
				batchRows: [],
				batchSelections: new Map(),
				batchProgress: { processed: 0, total: tracks.length, currentTitle: '' },
				batchLoading: true,
				error: null,
			}))

			const rows: BatchRow[] = []
			const selections = new Map<string, ScoredTagCandidate | null>()

			for (const [index, track] of tracks.entries()) {
				update((state) => ({
					...state,
					batchProgress: { processed: index, total: tracks.length, currentTitle: track.title ?? '' },
				}))

				try {
					const result = await searchRankedTrackTags({
						artist: track.artist ?? null,
						title: track.title ?? '',
						durationMs: track.duration_ms,
					})
					rows.push({ track, candidates: result.candidates, errors: result.errors, error: null })

					const best = result.candidates[0]
					if (best !== undefined && best.similarity_score >= 0.85) {
						selections.set(track.id, best)
					}
				} catch (error) {
					// One failed search must not sink the batch: record it on the row
					// and keep going. The per-row status badge surfaces it to the user.
					const errorMessage = error instanceof Error ? error.message : get(translate)('tagger.batch.error')
					rows.push({ track, candidates: [], errors: [], error: errorMessage })
				}

				update((state) => ({
					...state,
					batchProgress: { processed: index + 1, total: tracks.length, currentTitle: track.title ?? '' },
				}))
			}

			update((state) => ({
				...state,
				batchRows: rows,
				batchSelections: selections,
				batchLoading: false,
			}))
		},

		/** Record a decision for one row; `null` is an explicit "not available". */
		selectFor(trackId: string, candidate: ScoredTagCandidate | null) {
			update((state) => {
				const batchSelections = new Map(state.batchSelections)
				batchSelections.set(trackId, candidate)
				return { ...state, batchSelections }
			})
		},

		/**
		 * Apply every non-null selection. Each row runs `extend` then the metadata
		 * patch then, when the candidate has artwork, the artwork download. One
		 * row failing is recorded in `failed` and does not stop the others.
		 */
		async applyBatch(): Promise<{ updated: Track[]; failed: Array<{ trackId: string; error: string }> }> {
			const state = get({ subscribe })
			update((current) => ({ ...current, batchApplying: true }))

			const updated: Track[] = []
			const failed: Array<{ trackId: string; error: string }> = []

			for (const row of state.batchRows) {
				const selected = state.batchSelections.get(row.track.id)
				// `undefined` is "not decided"; `null` is an explicit skip. Neither applies.
				if (selected === undefined || selected === null) continue

				try {
					const extended = await extendTrackTag(selected)
					let track = await updateTrackMetadata(row.track.id, candidateToPatch(extended))
					if (hasValue(extended.artwork_url)) {
						track = await setTrackArtworkFromUrl(row.track.id, extended.artwork_url)
					}
					updated.push(track)
				} catch (error) {
					const errorMessage = error instanceof Error ? error.message : get(translate)('tagger.toast.applyFailed')
					failed.push({ trackId: row.track.id, error: errorMessage })
				}
			}

			update((current) => ({ ...current, batchApplying: false }))

			if (failed.length > 0) {
				toastStore.error(get(translate)('tagger.batch.applyFailed', { values: { count: failed.length } }))
			}

			return { updated, failed }
		},

		/** Clear the batch rows, progress and selections, leaving single-track state alone. */
		resetBatch() {
			update((state) => ({
				...state,
				batchRows: [],
				batchSelections: new Map(),
				batchProgress: emptyProgress,
				batchLoading: false,
				batchApplying: false,
			}))
		},
	}
}

export const taggerStore = createTaggerStore()
