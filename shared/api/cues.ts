import { invoke } from '@tauri-apps/api/core'
import type { Cue } from '../types'

/**
 * Get all cue points of a track, ordered deterministically
 */
export async function getCuesForTrack(trackId: string): Promise<Cue[]> {
	return invoke<Cue[]>('get_cues_for_track', { trackId })
}

/**
 * Replace the whole cue list of a track. Returns the stored list (identical cues collapsed)
 */
export async function replaceCuesForTrack(trackId: string, cues: Cue[]): Promise<Cue[]> {
	return invoke<Cue[]>('replace_cues_for_track', { trackId, cues })
}

/**
 * Delete every cue point of a track
 */
export async function deleteCuesForTrack(trackId: string): Promise<void> {
	return invoke<void>('delete_cues_for_track', { trackId })
}

/**
 * Delete a single cue point by id
 */
export async function deleteCue(id: string): Promise<void> {
	return invoke<void>('delete_cue', { id })
}
