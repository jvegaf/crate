import type { Track } from '../types'
import { keyToCamelot } from './format'

/**
 * Priority used when auto-ordering a playlist.
 *
 * - `harmony`: key first, rating as the tiebreaker — mix harmonically.
 * - `energy`: rating first, key as the tiebreaker — build the set.
 */
export type AutoOrderPriority = 'harmony' | 'energy'

/** Sort value for a track whose key cannot be resolved: always after known keys. */
const UNKNOWN_KEY_SORT_VALUE = 999

/** Sort value for an unrated track (rating 0): always after rated tracks. */
const UNRATED_SORT_VALUE = Number.MAX_SAFE_INTEGER

const CAMELOT_SORT_PATTERN = /^(\d{1,2})([AB])$/

/**
 * Sortable value for a musical key, grouping by Camelot number and then by mode
 * (A / minor before B / major): `1A` -> 10, `1B` -> 11, `12A` -> 120.
 *
 * Accepts any notation `keyToCamelot` understands, so standard notation
 * ("Am"), Camelot ("8A") and tagger free text ("G Minor") order consistently.
 * Unresolvable keys sort last rather than being assigned a guessed position.
 */
export function keySortValue(key: string | null | undefined): number {
	const camelot = keyToCamelot(key)
	if (!camelot) return UNKNOWN_KEY_SORT_VALUE

	const match = camelot.match(CAMELOT_SORT_PATTERN)
	if (!match) return UNKNOWN_KEY_SORT_VALUE

	const mode = match[2] === 'A' ? 0 : 1
	return parseInt(match[1], 10) * 10 + mode
}

/**
 * Rating 0 means "unrated" in this app, so unrated tracks group after every
 * rated track instead of leading the playlist.
 */
function ratingSortValue(rating: number): number {
	return rating > 0 ? rating : UNRATED_SORT_VALUE
}

/**
 * Return a new array with the playlist tracks ordered for mixing.
 *
 * Deliberately has no direction parameter: an auto-ordered playlist is always
 * ascending (lowest key, then lowest rating) with unresolvable keys and unrated
 * tracks at the end. Ties fall back to `id` so the result is deterministic and
 * repeated runs are idempotent.
 */
export function autoOrderTracks(tracks: Track[], priority: AutoOrderPriority): Track[] {
	const byKey = (a: Track, b: Track): number => keySortValue(a.key) - keySortValue(b.key)
	const byRating = (a: Track, b: Track): number => ratingSortValue(a.rating) - ratingSortValue(b.rating)

	return [...tracks].sort((a, b) => {
		const primary = priority === 'harmony' ? byKey(a, b) : byRating(a, b)
		if (primary !== 0) return primary

		const secondary = priority === 'harmony' ? byRating(a, b) : byKey(a, b)
		if (secondary !== 0) return secondary

		return a.id.localeCompare(b.id)
	})
}
