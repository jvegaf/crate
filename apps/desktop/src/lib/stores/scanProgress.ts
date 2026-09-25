import { writable, get } from 'svelte/store'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { LibraryScanProgress } from '$shared/types'
import { translate } from '$shared/i18n'
import { toastStore } from '$shared/stores/toast'

// =============================================================================
// State
// =============================================================================

/**
 * Live progress of the running music-folder scan, or null when no scan is active.
 *
 * Kept out of `libraryStore` on purpose: `sortedTracks` re-runs `sortTracks` on every
 * library update, so streaming one event per file into it would re-sort the whole library
 * thousands of times and hang the UI.
 */
export const scanProgress = writable<LibraryScanProgress | null>(null)

let unlisten: UnlistenFn | null = null
let toastId: string | null = null

// =============================================================================
// Listener
// =============================================================================

/**
 * Start listening for `library-scan-progress` events. Idempotent: repeated calls are
 * no-ops while a listener is active.
 */
export async function startListening() {
	if (unlisten) return

	unlisten = await listen<LibraryScanProgress>('library-scan-progress', (event) => {
		const payload = event.payload
		scanProgress.set(payload)

		const message = get(translate)('settings.library.musicFolderProgress', {
			values: { current: payload.current, total: payload.total },
		})
		const progress = { current: payload.current, total: payload.total }

		if (toastId) {
			// Refresh the existing toast in place so it does not flicker.
			toastStore.update(toastId, { message, progress })
		} else {
			// duration 0 keeps the toast on screen for the whole scan.
			toastId = toastStore.info(message, 0)
		}
	})
}

/**
 * Stop listening for `library-scan-progress` events.
 */
export function stopListening() {
	if (unlisten) {
		unlisten()
		unlisten = null
	}
}

/**
 * Clear the live progress and dismiss the persistent progress toast.
 */
export function dismiss() {
	scanProgress.set(null)
	if (toastId) {
		toastStore.dismiss(toastId)
		toastId = null
	}
}
