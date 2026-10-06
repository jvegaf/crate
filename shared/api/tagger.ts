import { invoke } from '@tauri-apps/api/core'
import type { RankedSearchResult, ScoredTagCandidate, TagCandidate, Track } from '../types'

/**
 * Search every metadata provider for candidates matching `artist` + `title` and
 * return the best ones ranked by similarity against the local track, plus any
 * per-provider failures.
 *
 * Command arguments are camelCase in TS; Tauri maps them to the Rust
 * `snake_case` parameters (`duration_ms`, `genre`, `label`, `bpm`, `key`,
 * `max_candidates`, `min_score`).
 */
export async function searchRankedTrackTags(params: {
	artist: string | null
	title: string
	durationMs: number | null
	genre?: string | null
	label?: string | null
	bpm?: number | null
	key?: string | null
	limit?: number
	maxCandidates?: number
	minScore?: number
}): Promise<RankedSearchResult> {
	return invoke<RankedSearchResult>('search_ranked_track_tags', {
		artist: params.artist,
		title: params.title,
		durationMs: params.durationMs,
		genre: params.genre ?? null,
		label: params.label ?? null,
		bpm: params.bpm ?? null,
		key: params.key ?? null,
		limit: params.limit ?? null,
		maxCandidates: params.maxCandidates ?? null,
		minScore: params.minScore ?? null,
	})
}

/**
 * Skip the search and fetch metadata directly from the store URL.
 * Returns null if the URL is not a recognized store URL.
 */
export async function searchTrackByUrl(url: string): Promise<ScoredTagCandidate | null> {
	return invoke<ScoredTagCandidate | null>('search_track_by_url', { url })
}

/**
 * Enrich one provider candidate with its per-ID detail (label, release date,
 * duration, artwork) before the user applies it. The provider is taken from the
 * candidate itself, so the request cannot disagree with it.
 */
export async function extendTrackTag(candidate: TagCandidate): Promise<TagCandidate> {
	return invoke<TagCandidate>('extend_track_tag', { candidate })
}

/**
 * Download a candidate's remote artwork and set it as the track's artwork.
 * The backend owns the download (the app's CSP blocks a frontend fetch) and
 * reuses the library's user-provided artwork path.
 */
export async function setTrackArtworkFromUrl(trackId: string, url: string): Promise<Track> {
	return invoke<Track>('set_track_artwork_from_url', { trackId, url })
}
