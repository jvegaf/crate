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

/**
 * Register sample URLs with Crate's localhost stream proxy so the webview plays them from
 * `http://127.0.0.1:{port}/samples/{key}` instead of fetching remote https directly (which
 * hangs in WebKitGTK's libsoup stack on some systems).
 *
 * Resolves with one entry per input URL, in order: the proxy URL when the backend accepted the
 * URL (`https://geo-samples.beatport.com` only), `null` when it was rejected — callers fall
 * back to the direct URL. The command itself reports `null` for every entry if the proxy state
 * is unavailable, so a rejection from this wrapper only means a broken IPC channel.
 */
export async function registerBeatportSampleStreams(urls: string[]): Promise<(string | null)[]> {
	return invoke<(string | null)[]>('register_beatport_sample_streams', { urls })
}
