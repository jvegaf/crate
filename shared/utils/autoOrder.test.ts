import { describe, expect, it } from 'vitest'
import type { Track } from '../types'
import { autoOrderTracks, keySortValue } from './autoOrder'

function makeTrack(id: string, overrides: Partial<Track> = {}): Track {
	return {
		id,
		file_path: `/music/${id}.flac`,
		file_hash: null,
		title: id,
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

describe('keySortValue', () => {
	it('orders Camelot codes as number * 10 + mode, A (minor) before B (major)', () => {
		expect(keySortValue('1A')).toBe(10)
		expect(keySortValue('1B')).toBe(11)
		expect(keySortValue('12A')).toBe(120)
		expect(keySortValue('12B')).toBe(121)
	})

	it('is case-insensitive and tolerant of a lowercase mode letter', () => {
		expect(keySortValue('8a')).toBe(80)
		expect(keySortValue('12b')).toBe(121)
	})

	it('resolves standard notation to the same value as its Camelot code', () => {
		expect(keySortValue('Am')).toBe(keySortValue('8A'))
		expect(keySortValue('C')).toBe(keySortValue('8B'))
		expect(keySortValue('F#m')).toBe(keySortValue('11A'))
		expect(keySortValue('Ebm')).toBe(keySortValue('2A'))
	})

	it('resolves tagger free text to the same value as its Camelot code', () => {
		expect(keySortValue('G Minor')).toBe(keySortValue('6A'))
		expect(keySortValue('G min')).toBe(keySortValue('6A'))
		expect(keySortValue('A minor')).toBe(keySortValue('8A'))
		expect(keySortValue('G major')).toBe(keySortValue('9B'))
	})

	it('sorts keys it cannot resolve to the end instead of guessing a position', () => {
		expect(keySortValue(null)).toBe(999)
		expect(keySortValue('')).toBe(999)
		expect(keySortValue('s-key')).toBe(999)
		expect(keySortValue('H')).toBe(999)
		expect(keySortValue('13A')).toBe(999)
		expect(keySortValue('0A')).toBe(999)
	})
})

describe('autoOrderTracks', () => {
	describe('harmony priority', () => {
		it('orders by Camelot number, with A before B inside the same number', () => {
			const tracks = [
				makeTrack('d', { key: '12B' }),
				makeTrack('a', { key: '1A' }),
				makeTrack('b', { key: '1B' }),
				makeTrack('c', { key: '6A' }),
			]

			expect(ids(autoOrderTracks(tracks, 'harmony'))).toEqual(['a', 'b', 'c', 'd'])
		})

		it('puts unresolvable keys at the end', () => {
			const tracks = [
				makeTrack('d', { key: '8B' }),
				makeTrack('b', { key: null }),
				makeTrack('a', { key: '8A' }),
				makeTrack('c', { key: 'garbage' }),
			]

			expect(ids(autoOrderTracks(tracks, 'harmony'))).toEqual(['a', 'd', 'b', 'c'])
		})

		it('breaks ties on the same key by rating ascending, with unrated last', () => {
			const tracks = [
				makeTrack('a', { key: '8A', rating: 3 }),
				makeTrack('b', { key: '8A', rating: 0 }),
				makeTrack('c', { key: '8A', rating: 5 }),
				makeTrack('d', { key: '8A', rating: 1 }),
			]

			expect(ids(autoOrderTracks(tracks, 'harmony'))).toEqual(['d', 'a', 'c', 'b'])
		})

		it('treats equivalent notations in different formats as the same key', () => {
			const tracks = [
				makeTrack('a', { key: 'Am' }),
				makeTrack('b', { key: '8A' }),
				makeTrack('c', { key: 'A minor' }),
				makeTrack('d', { key: 'G Minor' }),
				makeTrack('e', { key: '6A' }),
			]

			expect(ids(autoOrderTracks(tracks, 'harmony'))).toEqual(['d', 'e', 'a', 'b', 'c'])
		})
	})

	describe('energy priority', () => {
		it('orders by rating ascending, with unrated last, then by key', () => {
			const tracks = [
				makeTrack('a', { key: '8A', rating: 2 }),
				makeTrack('b', { key: '1A', rating: 5 }),
				makeTrack('c', { key: '12B', rating: 0 }),
				makeTrack('d', { key: '1B', rating: 2 }),
			]

			expect(ids(autoOrderTracks(tracks, 'energy'))).toEqual(['d', 'a', 'b', 'c'])
		})

		it('breaks rating ties by key, with unresolvable keys last inside the tie', () => {
			const tracks = [
				makeTrack('a', { key: '8A', rating: 4 }),
				makeTrack('b', { key: null, rating: 4 }),
				makeTrack('c', { key: '3B', rating: 4 }),
			]

			expect(ids(autoOrderTracks(tracks, 'energy'))).toEqual(['c', 'a', 'b'])
		})
	})

	it('is deterministic for fully equal tracks by falling back to id order', () => {
		const tracks = [makeTrack('b', { key: '8A', rating: 2 }), makeTrack('a', { key: '8A', rating: 2 })]

		expect(ids(autoOrderTracks(tracks, 'harmony'))).toEqual(['a', 'b'])
		expect(ids(autoOrderTracks(tracks, 'energy'))).toEqual(['a', 'b'])
	})

	it('does not mutate the input array', () => {
		const tracks = [makeTrack('b', { key: '12B' }), makeTrack('a', { key: '1A' })]

		autoOrderTracks(tracks, 'harmony')

		expect(ids(tracks)).toEqual(['b', 'a'])
	})

	it('returns an empty array for an empty playlist', () => {
		expect(autoOrderTracks([], 'harmony')).toEqual([])
		expect(autoOrderTracks([], 'energy')).toEqual([])
	})
})
