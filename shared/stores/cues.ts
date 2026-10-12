import { writable, derived } from 'svelte/store'
import type { Cue } from '../types'
import * as cuesApi from '../api/cues'
import { toastStore } from './toast'

// =============================================================================
// State
// =============================================================================

interface CuesState {
	cuesByTrack: Record<string, Cue[]>
	loading: boolean
	error: string | null
}

const initialState: CuesState = {
	cuesByTrack: {},
	loading: false,
	error: null,
}

// =============================================================================
// Store
// =============================================================================

function createCuesStore() {
	const { subscribe, set, update } = writable<CuesState>(initialState)

	function fail(error: unknown, fallback: string): string {
		// CrateError serializes to a plain string, so a string rejection is the real message.
		const message = typeof error === 'string' ? error : error instanceof Error ? error.message : fallback
		update((state) => ({ ...state, loading: false, error: message }))
		toastStore.error(message)
		return message
	}

	return {
		subscribe,

		/**
		 * Load the cue list of one track into the store
		 */
		async load(trackId: string): Promise<Cue[]> {
			update((state) => ({ ...state, loading: true, error: null }))
			try {
				const cues = await cuesApi.getCuesForTrack(trackId)
				update((state) => ({
					...state,
					cuesByTrack: { ...state.cuesByTrack, [trackId]: cues },
					loading: false,
				}))
				return cues
			} catch (error) {
				fail(error, 'Failed to load cue points')
				return []
			}
		},

		/**
		 * Replace the whole cue list of a track and cache the stored result
		 */
		async replace(trackId: string, cues: Cue[]): Promise<Cue[]> {
			try {
				const stored = await cuesApi.replaceCuesForTrack(trackId, cues)
				update((state) => ({
					...state,
					cuesByTrack: { ...state.cuesByTrack, [trackId]: stored },
				}))
				return stored
			} catch (error) {
				fail(error, 'Failed to save cue points')
				return []
			}
		},

		/**
		 * Delete a single cue, updating the cached list
		 */
		async deleteOne(trackId: string, id: string): Promise<void> {
			try {
				await cuesApi.deleteCue(id)
				update((state) => {
					const cues = state.cuesByTrack[trackId]
					if (!cues) return state
					return {
						...state,
						cuesByTrack: { ...state.cuesByTrack, [trackId]: cues.filter((cue) => cue.id !== id) },
					}
				})
			} catch (error) {
				fail(error, 'Failed to delete cue point')
			}
		},

		/**
		 * Delete every cue of a track
		 */
		async deleteAllForTrack(trackId: string): Promise<void> {
			try {
				await cuesApi.deleteCuesForTrack(trackId)
				update((state) => ({
					...state,
					cuesByTrack: { ...state.cuesByTrack, [trackId]: [] },
				}))
			} catch (error) {
				fail(error, 'Failed to delete cue points')
			}
		},

		/**
		 * Forget a track's cached cues without touching the backend
		 */
		forget(trackId: string) {
			update((state) => {
				const cuesByTrack = { ...state.cuesByTrack }
				delete cuesByTrack[trackId]
				return { ...state, cuesByTrack }
			})
		},

		/**
		 * Reset the store to its initial state
		 */
		reset() {
			set(initialState)
		},
	}
}

export const cuesStore = createCuesStore()

// =============================================================================
// Derived Stores
// =============================================================================

/**
 * Cue points of one track, `[]` until loaded
 */
export function cuesForTrack(trackId: string) {
	return derived(cuesStore, ($cues) => $cues.cuesByTrack[trackId] ?? [])
}
