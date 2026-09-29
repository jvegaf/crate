import { describe, expect, it } from 'vitest'
import type { Track, TracklistColumnId, TracklistColumnPref } from '../types'
import { sortTracks } from './sorting'
import {
	TRACKLIST_COLUMN_DEFINITIONS,
	defaultTracklistColumns,
	getTrackOriginFolder,
	moveTracklistColumn,
	normalizeTracklistColumns,
	toggleTracklistColumn,
	tracklistGridTemplate,
	visibleTracklistColumns,
} from './tracklistColumns'

const columnIds: TracklistColumnId[] = [
	'color',
	'artwork',
	'title',
	'artist',
	'album',
	'label',
	'origin',
	'bpm',
	'key',
	'duration_ms',
	'bitrate',
	'year',
	'date_added',
	'tags',
	'rating',
]

function makeTrack(overrides: Partial<Track> = {}): Track {
	return {
		id: 'track',
		file_path: '/music/track.mp3',
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
		format: 'mp3',
		analysis_source: null,
		waveform_data: null,
		rating: 0,
		play_count: 0,
		date_added: '',
		date_modified: '',
		last_played: null,
		rekordbox_id: null,
		artwork_path: null,
		artwork_source: null,
		color: null,
		library_root_id: null,
		relative_path: null,
		tags: [],
		...overrides,
	}
}

describe('tracklist column definitions', () => {
	it('provides the canonical definitions and default preference shape', () => {
		const defaults = defaultTracklistColumns()

		expect(TRACKLIST_COLUMN_DEFINITIONS.map(({ id }) => id)).toEqual(columnIds)
		expect(defaults).toHaveLength(TRACKLIST_COLUMN_DEFINITIONS.length)
		expect(defaults).toEqual(
			TRACKLIST_COLUMN_DEFINITIONS.map(({ id, defaultVisible }) => ({ id, visible: defaultVisible }))
		)
		expect(defaultTracklistColumns()).not.toBe(defaults)
	})
})

describe('normalizeTracklistColumns', () => {
	it('drops unknown ids and keeps only the first occurrence of duplicate ids', () => {
		const normalized = normalizeTracklistColumns([
			{ id: 'color', visible: false },
			{ id: 'unknown', visible: true } as unknown as TracklistColumnPref,
			{ id: 'color', visible: true },
		])

		expect(normalized.filter(({ id }) => id === 'color')).toEqual([{ id: 'color', visible: false }])
		expect(normalized).toHaveLength(TRACKLIST_COLUMN_DEFINITIONS.length)
		expect(normalized.some(({ id }) => id === ('unknown' as TracklistColumnId))).toBe(false)
	})

	it('inserts missing columns at their canonical positions', () => {
		const normalized = normalizeTracklistColumns([
			{ id: 'title', visible: true },
			{ id: 'bpm', visible: true },
		])

		expect(normalized.map(({ id }) => id)).toEqual(columnIds)
	})

	it('preserves the user order of existing entries when inserting missing columns', () => {
		const normalized = normalizeTracklistColumns([
			{ id: 'title', visible: true },
			{ id: 'bpm', visible: true },
			{ id: 'artist', visible: false },
		])
		const existingOrder = normalized
			.filter(({ id }) => id === 'title' || id === 'bpm' || id === 'artist')
			.map(({ id }) => id)

		expect(existingOrder).toEqual(['title', 'bpm', 'artist'])
		expect(normalized.findIndex(({ id }) => id === 'album')).toBeGreaterThan(
			normalized.findIndex(({ id }) => id === 'artist')
		)
	})

	it('anchors inserted columns to the nearest canonical neighbor in a reordered preference list', () => {
		const normalized = normalizeTracklistColumns([
			{ id: 'rating', visible: true },
			{ id: 'color', visible: true },
		])
		const ids = normalized.map(({ id }) => id)

		expect(ids.indexOf('artwork')).toBe(ids.indexOf('color') + 1)
		expect(ids.indexOf('rating')).toBeLessThan(ids.indexOf('color'))
	})

	it('forces title visible and is idempotent', () => {
		const prefs = defaultTracklistColumns().map((pref) => (pref.id === 'title' ? { ...pref, visible: false } : pref))
		const normalized = normalizeTracklistColumns(prefs)

		expect(normalized.find(({ id }) => id === 'title')?.visible).toBe(true)
		expect(normalizeTracklistColumns(normalized)).toEqual(normalized)
	})

	it('uses defaults when preferences are absent', () => {
		expect(normalizeTracklistColumns(undefined)).toEqual(defaultTracklistColumns())
		expect(normalizeTracklistColumns(null)).toEqual(defaultTracklistColumns())
	})
})

describe('tracklistGridTemplate', () => {
	it('preserves the existing tracklist layout for defaults', () => {
		expect(tracklistGridTemplate(defaultTracklistColumns())).toBe('24px 40px 1fr 1fr 80px 60px 80px 1fr 60px')
	})

	it('always retains a fluid track for representative hide and show combinations', () => {
		const defaults = defaultTracklistColumns()
		const fixedOnlyVisible = defaults.map(({ id }) => ({ id, visible: id === 'title' }))
		const selectedFluidColumns = defaults.map(({ id }) => ({
			id,
			visible: id === 'title' || id === 'album' || id === 'origin',
		}))

		expect(tracklistGridTemplate(fixedOnlyVisible)).toContain('fr')
		expect(tracklistGridTemplate(selectedFluidColumns)).toContain('fr')
		expect(visibleTracklistColumns(fixedOnlyVisible).map(({ id }) => id)).toEqual(['title'])
	})
})

describe('moveTracklistColumn', () => {
	it('reorders the hidden-inclusive list without mutating its input', () => {
		const prefs = defaultTracklistColumns()
		const original = structuredClone(prefs)
		const moved = moveTracklistColumn(prefs, 'rating', 'artist')

		expect(prefs).toEqual(original)
		expect(moved.map(({ id }) => id).slice(0, 5)).toEqual(['color', 'artwork', 'title', 'rating', 'artist'])
		expect(moved).toHaveLength(prefs.length)
	})

	it('moves rightward into the target column’s original slot', () => {
		const prefs: TracklistColumnPref[] = [
			{ id: 'color', visible: true },
			{ id: 'artwork', visible: true },
			{ id: 'title', visible: true },
			{ id: 'artist', visible: true },
		]

		expect(moveTracklistColumn(prefs, 'color', 'artist').map(({ id }) => id)).toEqual([
			'artwork',
			'title',
			'artist',
			'color',
		])
	})

	it('moves leftward into the target column’s original slot', () => {
		const prefs: TracklistColumnPref[] = [
			{ id: 'color', visible: true },
			{ id: 'artwork', visible: true },
			{ id: 'title', visible: true },
			{ id: 'artist', visible: true },
		]

		expect(moveTracklistColumn(prefs, 'artist', 'color').map(({ id }) => id)).toEqual([
			'artist',
			'color',
			'artwork',
			'title',
		])
	})

	it('moves onto an immediate neighbour in either direction', () => {
		const prefs: TracklistColumnPref[] = [
			{ id: 'color', visible: true },
			{ id: 'artwork', visible: true },
			{ id: 'title', visible: true },
			{ id: 'artist', visible: true },
		]

		expect(moveTracklistColumn(prefs, 'artwork', 'title').map(({ id }) => id)).toEqual([
			'color',
			'title',
			'artwork',
			'artist',
		])
		expect(moveTracklistColumn(prefs, 'title', 'artwork').map(({ id }) => id)).toEqual([
			'color',
			'title',
			'artwork',
			'artist',
		])
	})

	it('places a column explicitly before or after the same target', () => {
		const prefs: TracklistColumnPref[] = [
			{ id: 'color', visible: true },
			{ id: 'artwork', visible: true },
			{ id: 'title', visible: true },
			{ id: 'artist', visible: true },
		]

		expect(moveTracklistColumn(prefs, 'color', 'artist', 'before').map(({ id }) => id)).toEqual([
			'artwork',
			'title',
			'color',
			'artist',
		])
		expect(moveTracklistColumn(prefs, 'color', 'artist', 'after').map(({ id }) => id)).toEqual([
			'artwork',
			'title',
			'artist',
			'color',
		])
	})

	it('moves after the last visible target and before the first visible target', () => {
		const prefs: TracklistColumnPref[] = [
			{ id: 'color', visible: true },
			{ id: 'artwork', visible: true },
			{ id: 'title', visible: true },
		]

		expect(moveTracklistColumn(prefs, 'color', 'title', 'after').map(({ id }) => id)).toEqual([
			'artwork',
			'title',
			'color',
		])
		expect(moveTracklistColumn(prefs, 'title', 'color', 'before').map(({ id }) => id)).toEqual([
			'title',
			'color',
			'artwork',
		])
	})

	it('keeps hidden columns in place in the non-dragged order during visible reordering', () => {
		const prefs: TracklistColumnPref[] = [
			{ id: 'title', visible: true },
			{ id: 'album', visible: false },
			{ id: 'artist', visible: true },
			{ id: 'bpm', visible: true },
			...defaultTracklistColumns()
				.filter(({ id }) => id !== 'title' && id !== 'album' && id !== 'artist' && id !== 'bpm')
				.map((pref) => ({ ...pref, visible: false })),
		]

		const moved = moveTracklistColumn(prefs, 'title', 'artist', 'after')

		expect(visibleTracklistColumns(moved).map(({ id }) => id)).toEqual(['artist', 'title', 'bpm'])
		expect(moved.map(({ id }) => id).slice(0, 4)).toEqual(['album', 'artist', 'title', 'bpm'])
		expect(moved.at(-1)?.id).not.toBe('album')
	})

	it('returns the input unchanged when only one column is visible', () => {
		const prefs = defaultTracklistColumns().map((pref) => ({ ...pref, visible: pref.id === 'title' }))

		expect(moveTracklistColumn(prefs, 'artist', 'bpm', 'after')).toBe(prefs)
	})

	it('does not mutate the source array or its entries', () => {
		const prefs = defaultTracklistColumns()
		const originalArray = [...prefs]
		const originalEntries = prefs.map((pref) => ({ ...pref }))

		const moved = moveTracklistColumn(prefs, 'color', 'artist')

		expect(prefs).toEqual(originalEntries)
		expect(prefs).toEqual(originalArray)
		expect(moved).not.toBe(prefs)
		expect(prefs.every((pref, index) => pref === originalArray[index])).toBe(true)
	})

	it('moves a default hidden-inclusive header preference into the target slot', () => {
		const prefs = defaultTracklistColumns()
		const moved = moveTracklistColumn(prefs, 'tags', 'bpm')
		const expectedVisibleIds = prefs.filter(({ visible }) => visible).map(({ id }) => id)
		expectedVisibleIds.splice(expectedVisibleIds.indexOf('tags'), 1)
		expectedVisibleIds.splice(expectedVisibleIds.indexOf('bpm'), 0, 'tags')
		const definitionById = new Map(TRACKLIST_COLUMN_DEFINITIONS.map(({ id, width }) => [id, width]))
		const expectedTemplate = expectedVisibleIds.map((id) => definitionById.get(id)).join(' ')

		expect(prefs).toHaveLength(15)
		expect(tracklistGridTemplate(moved)).toBe(expectedTemplate)
	})

	it('returns the input unchanged for equal or unknown ids', () => {
		const prefs = defaultTracklistColumns()

		expect(moveTracklistColumn(prefs, 'artist', 'artist')).toBe(prefs)
		expect(moveTracklistColumn(prefs, 'missing' as TracklistColumnId, 'artist')).toBe(prefs)
		expect(moveTracklistColumn(prefs, 'artist', 'missing' as TracklistColumnId)).toBe(prefs)
	})
})

describe('toggleTracklistColumn', () => {
	it('flips visibility without mutating the input', () => {
		const prefs = defaultTracklistColumns()
		const original = structuredClone(prefs)
		const toggled = toggleTracklistColumn(prefs, 'album')

		expect(prefs).toEqual(original)
		expect(toggled.find(({ id }) => id === 'album')?.visible).toBe(true)
	})

	it('refuses to hide the locked title and leaves the input unchanged', () => {
		const prefs = defaultTracklistColumns()

		expect(toggleTracklistColumn(prefs, 'title')).toBe(prefs)
	})

	it('returns the input unchanged for unknown ids', () => {
		const prefs = defaultTracklistColumns()

		expect(toggleTracklistColumn(prefs, 'missing' as TracklistColumnId)).toBe(prefs)
	})
})

describe('getTrackOriginFolder', () => {
	it('extracts the immediate parent from POSIX and Windows paths', () => {
		expect(getTrackOriginFolder('/music/Artist/Album/track.mp3')).toBe('Album')
		expect(getTrackOriginFolder('C:\\Music\\Artist\\Album\\track.mp3')).toBe('Album')
	})

	it('handles trailing separators and paths without a parent', () => {
		expect(getTrackOriginFolder('/music/Artist/Album/')).toBe('Album')
		expect(getTrackOriginFolder('track.mp3')).toBe('')
	})
})

describe('sortTracks for tracklist columns', () => {
	it('sorts origins case-insensitively with nulls last', () => {
		const tracks = [
			makeTrack({ id: 'null', file_path: 'track.mp3' }),
			makeTrack({ id: 'z', file_path: '/music/Zulu/track.mp3' }),
			makeTrack({ id: 'a', file_path: '/music/alpha/track.mp3' }),
		]

		expect(sortTracks(tracks, { field: 'origin', direction: 'asc' }).map(({ id }) => id)).toEqual(['a', 'z', 'null'])
	})

	it('sorts labels case-insensitively with nulls last', () => {
		const tracks = [
			makeTrack({ id: 'null', label: null }),
			makeTrack({ id: 'z', label: 'Zulu' }),
			makeTrack({ id: 'a', label: 'alpha' }),
		]

		expect(sortTracks(tracks, { field: 'label', direction: 'asc' }).map(({ id }) => id)).toEqual(['a', 'z', 'null'])
	})

	it('sorts bitrates numerically with nulls last', () => {
		const tracks = [
			makeTrack({ id: 'null', bitrate: null }),
			makeTrack({ id: 'high', bitrate: 320_000 }),
			makeTrack({ id: 'low', bitrate: 128_000 }),
		]

		expect(sortTracks(tracks, { field: 'bitrate', direction: 'asc' }).map(({ id }) => id)).toEqual([
			'low',
			'high',
			'null',
		])
	})
})
