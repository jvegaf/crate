import { describe, expect, it } from 'vitest'
import type { Track } from '../types'
import { sortTracks } from './sorting'

function makeTrack(overrides: Partial<Track> = {}): Track {
	return {
		id: 'track',
		file_path: '/music/track.flac',
		file_hash: null,
		title: 'Track',
		artist: null,
		album: null,
		year: null,
		genre: null,
		label: null,
		catalog_number: null,
		duration_ms: 0,
		bpm: null,
		key: null,
		bitrate: null,
		sample_rate: null,
		format: 'flac',
		analysis_source: null,
		waveform_data: null,
		rating: 0,
		play_count: 0,
		date_added: '2026-01-01T00:00:00Z',
		date_modified: '2026-01-01T00:00:00Z',
		last_played: null,
		rekordbox_id: null,
		url: null,
		artwork_path: null,
		artwork_source: null,
		color: null,
		library_root_id: null,
		relative_path: null,
		tags: [],
		...overrides,
	}
}

const ids = (tracks: Track[]): string[] => tracks.map((t) => t.id)

describe('sortTracks', () => {
	describe('playlist_order', () => {
		it('preserves the order it is given instead of re-sorting', () => {
			// The backend already returns playlist members ordered by their persisted
			// junction position, so any client-side sort would mask the stored order.
			const tracks = [
				makeTrack({ id: 'c', title: 'Zeta', key: '12B' }),
				makeTrack({ id: 'a', title: 'Alpha', key: '1A' }),
				makeTrack({ id: 'b', title: 'Mike', key: '6A' }),
			]

			const sorted = sortTracks(tracks, { field: 'playlist_order', direction: 'asc' })

			expect(ids(sorted)).toEqual(['c', 'a', 'b'])
		})

		it('ignores the direction and still preserves the given order', () => {
			const tracks = [makeTrack({ id: 'c' }), makeTrack({ id: 'a' }), makeTrack({ id: 'b' })]

			expect(ids(sortTracks(tracks, { field: 'playlist_order', direction: 'desc' }))).toEqual(['c', 'a', 'b'])
		})

		it('returns a copy rather than mutating the input array', () => {
			const tracks = [makeTrack({ id: 'c' }), makeTrack({ id: 'a' })]

			const sorted = sortTracks(tracks, { field: 'playlist_order', direction: 'asc' })

			expect(sorted).not.toBe(tracks)
			expect(ids(tracks)).toEqual(['c', 'a'])
		})
	})

	describe('regression: real sort fields still sort', () => {
		const tracks = [makeTrack({ id: 'b', title: 'Bravo' }), makeTrack({ id: 'a', title: 'Alpha' })]

		it('sorts ascending', () => {
			expect(ids(sortTracks(tracks, { field: 'title', direction: 'asc' }))).toEqual(['a', 'b'])
		})

		it('sorts descending', () => {
			expect(ids(sortTracks(tracks, { field: 'title', direction: 'desc' }))).toEqual(['b', 'a'])
		})

		it('still sinks empty values to the end when ascending', () => {
			const withEmpty = [makeTrack({ id: 'b', rating: 0 }), makeTrack({ id: 'a', rating: 3 })]

			expect(ids(sortTracks(withEmpty, { field: 'rating', direction: 'asc' }))).toEqual(['a', 'b'])
		})
	})
})
