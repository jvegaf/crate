import { writable, get } from 'svelte/store'
import type { ProviderError, ScoredTagCandidate, TagCandidate, Track, TrackMetadataPatch } from '../types'
import { extendTrackTag, searchRankedTrackTags } from '../api/tagger'
import { updateTrackMetadata } from '../api/library'
import { translate } from '../i18n'
import { toastStore } from './toast'

// =============================================================================
// State
// =============================================================================

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
}

const initialState: TaggerState = {
	trackId: null,
	loading: false,
	extending: false,
	candidates: [],
	errors: [],
	error: null,
	selected: null,
	extended: null,
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

	const { album, genre, label, catalog_number: catalogNumber, key } = candidate
	if (hasValue(album)) patch.album = album
	if (hasValue(genre)) patch.genre = genre
	if (hasValue(label)) patch.label = label
	if (hasValue(catalogNumber)) patch.catalog_number = catalogNumber
	if (hasValue(key)) patch.key = key

	const releaseDate = candidate.release_date
	if (releaseDate !== null && /^\d{4}/.test(releaseDate)) {
		const year = Number(releaseDate.slice(0, 4))
		if (Number.isFinite(year)) patch.year = year
	}

	if (typeof candidate.bpm === 'number' && Number.isFinite(candidate.bpm)) {
		patch.bpm = candidate.bpm
	}

	// `rating` and `embedded_artwork` are intentionally not set: the candidate's
	// `artwork_url` is remote and the patch only accepts an embedded file.
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
		 */
		async apply(track: Track): Promise<Track | null> {
			const state = get({ subscribe })
			const candidate = state.extended ?? state.selected
			if (candidate === null) return null

			try {
				const updated = await updateTrackMetadata(track.id, candidateToPatch(candidate))
				update((current) => ({ ...current, error: null }))
				toastStore.success(get(translate)('tagger.toast.applied'))
				return updated
			} catch (error) {
				const errorMessage = error instanceof Error ? error.message : get(translate)('tagger.toast.applyFailed')
				update((current) => ({ ...current, error: errorMessage }))
				toastStore.error(errorMessage)
				return null
			}
		},

		/** Reset the store to its initial state. */
		reset() {
			set(initialState)
		},
	}
}

export const taggerStore = createTaggerStore()
