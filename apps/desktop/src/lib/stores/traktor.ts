import { writable } from 'svelte/store'
import type { TraktorImportOutcome, TraktorImportPlan } from '$shared/types'
import { importTraktorCollection, previewTraktorImport } from '$shared/api/traktor'

// =============================================================================
// Types
// =============================================================================

interface TraktorState {
	previewing: boolean
	importing: boolean
	plan: TraktorImportPlan | null
	outcome: TraktorImportOutcome | null
	error: string | null
}

// =============================================================================
// State
// =============================================================================

const initialState: TraktorState = {
	previewing: false,
	importing: false,
	plan: null,
	outcome: null,
	error: null,
}

/**
 * Human-readable form of a caught failure. `CrateError` serializes to its
 * `Display` string over IPC, so a Tauri rejection arrives as a plain string,
 * not an `Error` — that string has to survive extraction, not be discarded.
 */
function describeError(error: unknown): string {
	if (error instanceof Error) return error.message
	if (typeof error === 'string' && error !== '') return error
	return String(error)
}

// =============================================================================
// Store
// =============================================================================

function createTraktorStore() {
	const { subscribe, set, update } = writable<TraktorState>(initialState)

	return {
		subscribe,

		/** Parse the collection into a dry-run plan; no database writes happen here. */
		async preview(nmlPath: string) {
			update((state) => ({ ...state, previewing: true, plan: null, outcome: null, error: null }))
			try {
				const plan = await previewTraktorImport(nmlPath)
				update((state) => ({ ...state, plan }))
			} catch (error) {
				update((state) => ({ ...state, error: describeError(error) }))
			} finally {
				update((state) => ({ ...state, previewing: false }))
			}
		},

		/** Commit the plan to the library and keep the outcome report for display. */
		async run(nmlPath: string) {
			update((state) => ({ ...state, importing: true, outcome: null, error: null }))
			try {
				const outcome = await importTraktorCollection(nmlPath)
				update((state) => ({ ...state, outcome }))
			} catch (error) {
				update((state) => ({ ...state, error: describeError(error) }))
			} finally {
				update((state) => ({ ...state, importing: false }))
			}
		},

		reset() {
			set(initialState)
		},
	}
}

export const traktorStore = createTraktorStore()
