import { invoke } from '@tauri-apps/api/core'
import type { BeatportRecommendation } from '../types'

/**
 * Fetch Beatport's recommended similar tracks for one Beatport numeric track id.
 *
 * Resolves with an empty array when Beatport simply has no recommendations.
 * Rejects with a plain string (CrateError's Display message, e.g. "Tagger error:
 * … (HTTP 404)") on auth, rate-limit or transport failures — callers surface it.
 */
export async function findBeatportSimilarTracks(trackId: number): Promise<BeatportRecommendation[]> {
	return invoke<BeatportRecommendation[]>('find_beatport_similar_tracks', { trackId })
}
