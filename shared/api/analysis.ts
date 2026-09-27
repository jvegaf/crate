import { invoke } from '@tauri-apps/api/core'
import type { Track } from '../types'

/**
 * Analyze tracks for BPM and key detection
 * Progress events are emitted via 'analysis-track-event' Tauri event for each track.
 *
 * `force` bypasses the skip guard for tracks that already have BPM/key.
 * `keyNotationFormat` converts the detected key to the user's preferred notation before it is
 * persisted; when omitted, the backend keeps the raw standard-notation result.
 *
 * The second argument accepts either the legacy boolean `force` flag or an options object so
 * existing callers keep working while new callers can forward the key notation format.
 */
export async function analyzeTracks(
	trackIds: string[],
	options: boolean | { force?: boolean; keyNotationFormat?: string } = false
): Promise<void> {
	const normalized = typeof options === 'boolean' ? { force: options } : options
	const params = {
		trackIds,
		force: normalized.force ?? false,
		...(normalized.keyNotationFormat && { keyNotationFormat: normalized.keyNotationFormat }),
	}
	return invoke<void>('analyze_tracks', params)
}

/**
 * Recalculate every stored track key in the library, converting it to the given notation format.
 * Returns the number of tracks whose key actually changed. This does not re-run analysis.
 */
export async function recalculateAllKeys(format: string): Promise<number> {
	return invoke<number>('recalculate_all_keys', { keyNotationFormat: format })
}

/**
 * Cancel analysis for a specific track
 * Returns true if the track was found and cancelled, false otherwise
 */
export async function cancelTrackAnalysis(trackId: string): Promise<boolean> {
	return invoke<boolean>('cancel_track_analysis', { trackId })
}

/**
 * Cancel all running analysis operations (legacy)
 */
export async function cancelAnalysis(): Promise<void> {
	return invoke('cancel_analysis')
}

/**
 * Get updated tracks after analysis
 */
export async function getAnalyzedTracks(trackIds: string[]): Promise<Track[]> {
	return invoke<Track[]>('get_analyzed_tracks', { trackIds })
}
