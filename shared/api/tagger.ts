import { invoke } from '@tauri-apps/api/core'
import type { RankedSearchResult, TagCandidate } from '../types'

/**
 * Search every metadata provider for candidates matching `artist` + `title` and
 * return the best ones ranked by similarity against the local track, plus any
 * per-provider failures.
 *
 * Command arguments are camelCase in TS; Tauri maps them to the Rust
 * `snake_case` parameters (`duration_ms`, `max_candidates`, `min_score`).
 */
export async function searchRankedTrackTags(params: {
	artist: string | null
	title: string
	durationMs: number | null
	limit?: number
	maxCandidates?: number
	minScore?: number
}): Promise<RankedSearchResult> {
	return invoke<RankedSearchResult>('search_ranked_track_tags', {
		artist: params.artist,
		title: params.title,
		durationMs: params.durationMs,
		limit: params.limit ?? null,
		maxCandidates: params.maxCandidates ?? null,
		minScore: params.minScore ?? null,
	})
}

/**
 * Enrich one provider candidate with its per-ID detail (label, release date,
 * duration, artwork) before the user applies it. The provider is taken from the
 * candidate itself, so the request cannot disagree with it.
 */
export async function extendTrackTag(candidate: TagCandidate): Promise<TagCandidate> {
	return invoke<TagCandidate>('extend_track_tag', { candidate })
}
